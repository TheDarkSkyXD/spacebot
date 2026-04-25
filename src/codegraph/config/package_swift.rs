//! `Package.swift` parser.
//!
//! Swift Package Manager's manifest is itself a Swift file — parsing it
//! fully would require a Swift interpreter. In practice only a handful
//! of call-site patterns matter for import resolution:
//!
//! - `.target(name: "Foo", ...)` declares target directory
//!   `Sources/Foo/` by default.
//! - `.target(name: "Foo", path: "Custom")` overrides the directory.
//!
//! We extract target names and optional `path:` overrides with a
//! tolerant regex. Failure modes (unclosed strings, trailing commas) all
//! simply yield fewer matches, which is the right failure semantic for
//! a best-effort config parser.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;

use super::{workspace_mut, ConfigContext, SwiftTarget};

static TARGET_BLOCK_RE: OnceLock<Regex> = OnceLock::new();
static NAME_FIELD_RE: OnceLock<Regex> = OnceLock::new();
static PATH_FIELD_RE: OnceLock<Regex> = OnceLock::new();

fn target_block_re() -> &'static Regex {
    // Match a `.target(...)` or `.testTarget(...)` call and capture the
    // argument list greedily up to the closing paren on the same line.
    // Multi-line targets are handled by scanning across newlines via
    // `(?s)`.
    TARGET_BLOCK_RE.get_or_init(|| {
        Regex::new(r"(?s)\.(?:test)?[Tt]arget\s*\(([^)]*)\)")
            .expect("static regex")
    })
}
fn name_field_re() -> &'static Regex {
    NAME_FIELD_RE
        .get_or_init(|| Regex::new(r#"name\s*:\s*"([^"]+)""#).expect("static regex"))
}
fn path_field_re() -> &'static Regex {
    PATH_FIELD_RE
        .get_or_init(|| Regex::new(r#"path\s*:\s*"([^"]+)""#).expect("static regex"))
}

/// Load a `Package.swift` and extract declared targets.
pub async fn load(
    path: &Path,
    workspace_root: &Path,
    ctx: &mut ConfigContext,
) -> Result<()> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let targets = extract_targets(&raw);
    if !targets.is_empty() {
        workspace_mut(ctx, workspace_root).swift_targets = targets;
    }
    Ok(())
}

fn extract_targets(src: &str) -> Vec<SwiftTarget> {
    let mut out: Vec<SwiftTarget> = Vec::new();
    for cap in target_block_re().captures_iter(src) {
        let args = match cap.get(1) {
            Some(m) => m.as_str(),
            None => continue,
        };
        let name = name_field_re()
            .captures(args)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());
        let Some(name) = name else { continue };
        let path = path_field_re()
            .captures(args)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());
        out.push(SwiftTarget { name, path });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn parses_simple_package_swift() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("Package.swift");
        tokio::fs::write(
            &p,
            r#"// swift-tools-version:5.7
import PackageDescription

let package = Package(
    name: "MyLib",
    targets: [
        .target(name: "MyLib", dependencies: []),
        .target(name: "MyCLI", dependencies: ["MyLib"], path: "Sources/cli"),
        .testTarget(name: "MyLibTests", dependencies: ["MyLib"])
    ]
)"#,
        )
        .await
        .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        let targets = &ctx.workspaces[0].swift_targets;
        assert_eq!(targets.len(), 3);
        let names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"MyLib"));
        assert!(names.contains(&"MyCLI"));
        assert!(names.contains(&"MyLibTests"));
        let my_cli = targets.iter().find(|t| t.name == "MyCLI").unwrap();
        assert_eq!(my_cli.path, Some("Sources/cli".to_string()));
    }

    #[test]
    fn extract_ignores_targets_without_name() {
        let src = r#".target(dependencies: [])"#;
        assert!(extract_targets(src).is_empty());
    }
}
