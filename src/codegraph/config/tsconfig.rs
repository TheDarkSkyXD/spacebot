//! `tsconfig.json` parser.
//!
//! Extracts `compilerOptions.paths` (path aliases) and
//! `compilerOptions.baseUrl`. These two together control how a TS/JS
//! import string like `"@/utils/foo"` resolves to a file on disk —
//! without them, modern TS monorepos are effectively unindexable.
//!
//! ## Extends chain
//!
//! tsconfigs can extend a parent via `"extends": "..."`. We follow the
//! chain up to [`MAX_EXTENDS_DEPTH`] so a package inheriting from a
//! root tsconfig picks up the root's `paths`. The child's `paths`
//! override the parent's on a per-pattern basis (standard tsc semantics
//! — last writer wins).
//!
//! ## Comments / trailing commas
//!
//! Real-world tsconfigs frequently contain JSONC comments and trailing
//! commas. We strip them before handing off to `serde_json` so parsing
//! succeeds on `tsconfig.json` files the TypeScript compiler itself
//! would accept.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{workspace_mut, ConfigContext, TsPathAlias};

/// Hard cap on `extends` chain length. Real tsconfigs use at most a
/// handful; higher numbers usually mean a cycle or broken symlink.
const MAX_EXTENDS_DEPTH: usize = 8;

#[derive(Debug, Deserialize, Default)]
struct RawTsconfig {
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    #[serde(rename = "compilerOptions")]
    compiler_options: Option<RawCompilerOptions>,
}

#[derive(Debug, Deserialize, Default)]
struct RawCompilerOptions {
    #[serde(default)]
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(default)]
    paths: Option<std::collections::BTreeMap<String, Vec<String>>>,
}

/// Load a single `tsconfig.json` (following its `extends` chain) into
/// the given context's workspace entry.
pub async fn load(
    path: &Path,
    workspace_root: &Path,
    ctx: &mut ConfigContext,
) -> Result<()> {
    let (paths, base_url) = load_resolved(path).await?;

    let workspace = workspace_mut(ctx, workspace_root);
    if !paths.is_empty() {
        workspace.ts_paths = paths;
    }
    if let Some(b) = base_url {
        workspace.ts_base_url = Some(b);
    }
    Ok(())
}

/// Fully resolve a tsconfig into its effective `paths` and `baseUrl`,
/// following the `extends` chain. Returns `(paths, base_url)` where
/// `base_url` is relative to the *leaf* tsconfig's directory (that's
/// how tsc interprets it).
async fn load_resolved(leaf: &Path) -> Result<(Vec<TsPathAlias>, Option<PathBuf>)> {
    let mut chain: Vec<RawTsconfig> = Vec::new();
    let mut visited: Vec<PathBuf> = Vec::new();

    let mut current: Option<PathBuf> = Some(leaf.to_path_buf());
    while let Some(p) = current.take() {
        if chain.len() >= MAX_EXTENDS_DEPTH {
            tracing::debug!(
                "tsconfig extends chain exceeded {MAX_EXTENDS_DEPTH} — stopping"
            );
            break;
        }
        if visited.iter().any(|v| v == &p) {
            tracing::debug!("tsconfig extends cycle at {} — stopping", p.display());
            break;
        }
        let raw = read_tsconfig(&p)
            .await
            .with_context(|| format!("reading tsconfig at {}", p.display()))?;
        let parent_dir = p
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let next = raw.extends.as_deref().and_then(|rel| {
            resolve_extends_target(&parent_dir, rel)
        });
        visited.push(p);
        chain.push(raw);
        current = next;
    }

    // Merge from root (oldest ancestor) outward so the leaf overrides.
    let mut merged_paths: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut merged_base_url: Option<String> = None;
    for raw in chain.iter().rev() {
        if let Some(co) = &raw.compiler_options {
            if let Some(b) = &co.base_url {
                merged_base_url = Some(b.clone());
            }
            if let Some(paths) = &co.paths {
                for (pattern, targets) in paths {
                    merged_paths.insert(pattern.clone(), targets.clone());
                }
            }
        }
    }

    let paths: Vec<TsPathAlias> = merged_paths
        .into_iter()
        .map(|(pattern, targets)| TsPathAlias { pattern, targets })
        .collect();
    let base_url = merged_base_url.map(PathBuf::from);
    Ok((paths, base_url))
}

/// Read and parse a tsconfig.json, stripping JSONC artefacts first.
async fn read_tsconfig(path: &Path) -> Result<RawTsconfig> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .context("reading file")?;
    let cleaned = strip_jsonc(&raw);
    let parsed: RawTsconfig = serde_json::from_str(&cleaned).context("parsing JSON")?;
    Ok(parsed)
}

/// Resolve a parent tsconfig's `extends` value into an on-disk path.
///
/// tsc supports three extension shapes:
/// 1. Node-style relative paths (`./base` / `../shared/tsconfig.json`).
/// 2. Bare specifiers (`@tsconfig/node18/tsconfig.json`), resolved from
///    `node_modules`. Rare in monorepos we'd index without their
///    dependencies; we skip these quietly.
/// 3. Absolute paths (unusual; we honour them if they exist).
///
/// We default-append `.json` / `tsconfig.json` when the extension is
/// missing — mirroring tsc.
fn resolve_extends_target(parent_dir: &Path, rel: &str) -> Option<PathBuf> {
    // Bare specifier → skip (see docstring).
    if !rel.starts_with('.') && !Path::new(rel).is_absolute() {
        return None;
    }
    let mut candidate = parent_dir.join(rel);
    if candidate.is_file() {
        return Some(candidate);
    }
    // Try `.json` suffix.
    let with_json = candidate.with_extension("json");
    if with_json.is_file() {
        return Some(with_json);
    }
    // Try appending `/tsconfig.json` for directory-style extends.
    candidate.push("tsconfig.json");
    candidate.is_file().then_some(candidate)
}

/// Strip `// line`, `/* block */` comments and trailing commas from a
/// JSONC string. Simple state machine — not a full JSONC parser, but
/// handles everything tsc allows.
fn strip_jsonc(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    let mut escape = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if matches!(chars.peek(), Some('/')) => {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if matches!(chars.peek(), Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for c2 in chars.by_ref() {
                    if prev == '*' && c2 == '/' {
                        break;
                    }
                    prev = c2;
                }
            }
            _ => out.push(c),
        }
    }

    strip_trailing_commas(&out)
}

/// Remove trailing commas before `]` and `}` so serde_json accepts
/// JSONC. Preserves string contents.
fn strip_trailing_commas(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes: Vec<char> = src.chars().collect();
    let mut in_string = false;
    let mut escape = false;

    for (i, &c) in bytes.iter().enumerate() {
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }
        if c == ',' {
            // Look ahead past whitespace for the next structural char.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == ']' || bytes[j] == '}') {
                // Skip this comma.
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&p, contents).await.unwrap();
        p
    }

    #[tokio::test]
    async fn parses_plain_paths() {
        let tmp = TempDir::new().unwrap();
        let p = write_file(
            tmp.path(),
            "tsconfig.json",
            r#"{
              "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                  "@/*": ["src/*"],
                  "@utils/*": ["src/utils/*"]
                }
              }
            }"#,
        )
        .await;
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        let ws = &ctx.workspaces[0];
        assert_eq!(ws.ts_base_url, Some(PathBuf::from(".")));
        assert_eq!(ws.ts_paths.len(), 2);
        let patterns: Vec<_> = ws.ts_paths.iter().map(|p| p.pattern.as_str()).collect();
        assert!(patterns.contains(&"@/*"));
        assert!(patterns.contains(&"@utils/*"));
    }

    #[tokio::test]
    async fn tolerates_jsonc_comments() {
        let tmp = TempDir::new().unwrap();
        let p = write_file(
            tmp.path(),
            "tsconfig.json",
            r#"{
              // inherit nothing
              "compilerOptions": {
                /* path aliases for the app */
                "paths": {
                  "@/*": ["src/*"],
                }
              },
            }"#,
        )
        .await;
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        let ws = &ctx.workspaces[0];
        assert_eq!(ws.ts_paths.len(), 1);
        assert_eq!(ws.ts_paths[0].pattern, "@/*");
    }

    #[tokio::test]
    async fn follows_extends_chain() {
        let tmp = TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "tsconfig.base.json",
            r#"{
              "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                  "@shared/*": ["shared/*"]
                }
              }
            }"#,
        )
        .await;
        let leaf = write_file(
            tmp.path(),
            "tsconfig.json",
            r#"{
              "extends": "./tsconfig.base.json",
              "compilerOptions": {
                "paths": {
                  "@/*": ["src/*"]
                }
              }
            }"#,
        )
        .await;
        let mut ctx = ConfigContext::default();
        load(&leaf, Path::new(""), &mut ctx).await.unwrap();
        let ws = &ctx.workspaces[0];
        // Leaf's paths + parent's paths merged (leaf wins on overlap).
        assert_eq!(ws.ts_paths.len(), 2);
        let patterns: Vec<_> = ws.ts_paths.iter().map(|p| p.pattern.as_str()).collect();
        assert!(patterns.contains(&"@/*"));
        assert!(patterns.contains(&"@shared/*"));
        assert_eq!(ws.ts_base_url, Some(PathBuf::from(".")));
    }

    #[test]
    fn strip_jsonc_preserves_string_contents() {
        let src = r#"{"msg": "// not a comment", "p": "/*also not*/"}"#;
        let cleaned = strip_jsonc(src);
        assert!(cleaned.contains("// not a comment"));
        assert!(cleaned.contains("/*also not*/"));
    }

    #[test]
    fn strip_trailing_commas_handles_nested_structures() {
        let src = r#"{"a": [1, 2, 3,], "b": {"c": 1,},}"#;
        let cleaned = strip_trailing_commas(src);
        assert_eq!(cleaned, r#"{"a": [1, 2, 3], "b": {"c": 1}}"#);
    }
}
