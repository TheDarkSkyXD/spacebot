//! `pubspec.yaml` parser (Dart / Flutter).
//!
//! We only need the top-level `name:` field — the package name that
//! Dart uses as the import prefix for everything under `lib/`. The file
//! is YAML, but a line-based scan for the `name:` key is sufficient
//! because the shape is extremely regular and pulling in a YAML crate
//! for one field would be wasteful.

use std::path::Path;

use anyhow::{Context, Result};

use super::{workspace_mut, ConfigContext};

/// Load a `pubspec.yaml` and record the package name.
pub async fn load(
    path: &Path,
    workspace_root: &Path,
    ctx: &mut ConfigContext,
) -> Result<()> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    if let Some(name) = extract_package_name(&raw) {
        workspace_mut(ctx, workspace_root).dart_package = Some(name);
    }
    Ok(())
}

/// Pull the top-level `name:` value out of a pubspec.yaml. Returns
/// `None` when the field is missing or when it sits under a nested
/// mapping (we detect nesting by leading whitespace).
fn extract_package_name(src: &str) -> Option<String> {
    for line in src.lines() {
        // Top-level means zero leading whitespace. Nested `name:`
        // fields live under dependencies/dev_dependencies blocks which
        // are indented; those are package references, not our package's
        // name.
        if line.starts_with(|c: char| c.is_whitespace()) {
            continue;
        }
        let line = line.split_once('#').map(|(h, _)| h).unwrap_or(line);
        if let Some(rest) = line.strip_prefix("name:") {
            let value = rest.trim().trim_matches(|c| c == '"' || c == '\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn parses_package_name() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("pubspec.yaml");
        tokio::fs::write(
            &p,
            "name: my_app\nversion: 1.0.0\n\ndependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .await
        .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        assert_eq!(
            ctx.workspaces[0].dart_package,
            Some("my_app".to_string())
        );
    }

    #[test]
    fn ignores_nested_name_fields() {
        let src = "name: outer\ndependencies:\n  inner_pkg:\n    name: inner\n";
        assert_eq!(extract_package_name(src), Some("outer".to_string()));
    }

    #[test]
    fn returns_none_when_missing() {
        assert_eq!(extract_package_name("version: 1.0.0\n"), None);
    }

    #[test]
    fn strips_trailing_comment() {
        assert_eq!(
            extract_package_name("name: foo # the package\n"),
            Some("foo".to_string())
        );
    }
}
