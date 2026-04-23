//! `Cargo.toml` workspace parser.
//!
//! We only care about the `[workspace] members = [...]` section — it
//! tells us which subdirectories are crates so monorepo-aware walking
//! can attribute imports to the correct crate. Per-crate package
//! metadata (name/edition/deps) is out of scope for Phase 1 because the
//! Rust provider handles that entirely via the crate graph.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{workspace_mut, ConfigContext};

#[derive(Debug, Deserialize, Default)]
struct RawCargo {
    #[serde(default)]
    workspace: Option<RawWorkspace>,
}

#[derive(Debug, Deserialize, Default)]
struct RawWorkspace {
    #[serde(default)]
    members: Vec<String>,
}

/// Load a `Cargo.toml` and record workspace member paths, if any.
pub async fn load(
    path: &Path,
    workspace_root: &Path,
    ctx: &mut ConfigContext,
) -> Result<()> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    // Cargo.toml sometimes contains TOML-table-array trailing-comma
    // patterns that older `toml` crates reject. The maintained `toml`
    // 0.8 crate handles them; failure here just means we skip.
    let parsed: RawCargo = toml::from_str(&raw).context("parsing Cargo.toml")?;

    let members: Vec<PathBuf> = parsed
        .workspace
        .map(|w| w.members.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default();
    if !members.is_empty() {
        workspace_mut(ctx, workspace_root).cargo_workspace_members = members;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn parses_workspace_members() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("Cargo.toml");
        tokio::fs::write(
            &p,
            r#"[workspace]
members = ["crates/core", "crates/api", "tools/cli"]
"#,
        )
        .await
        .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        let members = &ctx.workspaces[0].cargo_workspace_members;
        assert_eq!(members.len(), 3);
        assert!(members.contains(&PathBuf::from("crates/core")));
    }

    #[tokio::test]
    async fn handles_cargo_without_workspace() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("Cargo.toml");
        tokio::fs::write(&p, "[package]\nname = \"x\"\nversion = \"0.1.0\"\n")
            .await
            .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        // No workspace = no members recorded, no workspace created.
        assert!(ctx
            .workspaces
            .iter()
            .all(|w| w.cargo_workspace_members.is_empty()));
    }
}
