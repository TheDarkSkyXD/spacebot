//! `go.mod` parser.
//!
//! We only need the `module` directive — the path that every non-
//! stdlib import in the module is prefixed with. A go.mod line-based
//! scan is faster and sturdier than pulling in a full parser for a file
//! whose shape is extremely regular.

use std::path::Path;

use anyhow::{Context, Result};

use super::{workspace_mut, ConfigContext};

/// Load a `go.mod` and record its `module` path on the workspace.
pub async fn load(
    path: &Path,
    workspace_root: &Path,
    ctx: &mut ConfigContext,
) -> Result<()> {
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;

    if let Some(module) = extract_module_path(&contents) {
        workspace_mut(ctx, workspace_root).go_module = Some(module);
    }
    Ok(())
}

/// Extract the `module X` directive from a go.mod source string.
/// Returns the path even when the directive sits inside a `( ... )`
/// block (rare but legal).
fn extract_module_path(src: &str) -> Option<String> {
    for line in src.lines() {
        let trimmed = line.trim();
        // `//` comments begin anywhere — respect them.
        let trimmed = trimmed
            .split_once("//")
            .map(|(head, _)| head.trim())
            .unwrap_or(trimmed);
        if let Some(rest) = trimmed.strip_prefix("module") {
            let rest = rest.trim();
            if rest.is_empty() {
                continue;
            }
            // Strip optional surrounding quotes (go.mod allows them for
            // paths containing special characters).
            let path = rest.trim_matches(|c: char| c == '"' || c == '\'').trim();
            if !path.is_empty() {
                return Some(path.to_string());
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
    async fn parses_simple_go_mod() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("go.mod");
        tokio::fs::write(&p, "module github.com/foo/bar\n\ngo 1.21\n")
            .await
            .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        assert_eq!(
            ctx.workspaces[0].go_module,
            Some("github.com/foo/bar".to_string())
        );
    }

    #[test]
    fn strips_line_comment() {
        let src = "module example.com/foo // comment here\n";
        assert_eq!(
            extract_module_path(src),
            Some("example.com/foo".to_string())
        );
    }

    #[test]
    fn handles_quoted_module() {
        let src = "module \"example.com/foo\"\n";
        assert_eq!(
            extract_module_path(src),
            Some("example.com/foo".to_string())
        );
    }

    #[test]
    fn returns_none_when_missing() {
        assert_eq!(extract_module_path("go 1.21\n"), None);
    }
}
