//! `composer.json` parser (PHP).
//!
//! We only care about the autoload maps — they tell us how namespaces
//! translate to directories, which is how `use Foo\Bar\Baz;` resolves
//! to a file on disk.
//!
//! PSR-4 (modern): `"App\\": "src/"` means class `App\Models\User`
//! lives at `src/Models/User.php`.
//!
//! PSR-0 (legacy): similar but with underscore-to-slash and
//! namespace-in-path semantics. We still extract it because a surprising
//! number of enterprise PHP codebases rely on it.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{workspace_mut, ConfigContext, Psr4Mapping};

#[derive(Debug, Deserialize, Default)]
struct RawComposer {
    #[serde(default)]
    autoload: Option<RawAutoload>,
    #[serde(default, rename = "autoload-dev")]
    autoload_dev: Option<RawAutoload>,
}

#[derive(Debug, Deserialize, Default)]
struct RawAutoload {
    #[serde(default, rename = "psr-4")]
    psr4: Option<std::collections::BTreeMap<String, AutoloadTarget>>,
    #[serde(default, rename = "psr-0")]
    psr0: Option<std::collections::BTreeMap<String, AutoloadTarget>>,
}

/// PSR-4 / PSR-0 targets can be either a single directory or an array
/// of directories. We flatten to a list of mappings.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AutoloadTarget {
    One(String),
    Many(Vec<String>),
}

/// Parse a `composer.json` and store PSR-4/PSR-0 mappings on the
/// workspace.
pub async fn load(
    path: &Path,
    workspace_root: &Path,
    ctx: &mut ConfigContext,
) -> Result<()> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let parsed: RawComposer = serde_json::from_str(&raw).context("parsing composer.json")?;

    let mut psr4: Vec<Psr4Mapping> = Vec::new();
    let mut psr0: Vec<Psr4Mapping> = Vec::new();
    for auto in [parsed.autoload.as_ref(), parsed.autoload_dev.as_ref()]
        .into_iter()
        .flatten()
    {
        if let Some(map) = &auto.psr4 {
            for (ns, target) in map {
                for dir in targets_to_vec(target) {
                    psr4.push(Psr4Mapping {
                        namespace: ns.clone(),
                        directory: dir,
                    });
                }
            }
        }
        if let Some(map) = &auto.psr0 {
            for (ns, target) in map {
                for dir in targets_to_vec(target) {
                    psr0.push(Psr4Mapping {
                        namespace: ns.clone(),
                        directory: dir,
                    });
                }
            }
        }
    }

    let ws = workspace_mut(ctx, workspace_root);
    if !psr4.is_empty() {
        ws.php_psr4 = psr4;
    }
    if !psr0.is_empty() {
        ws.php_psr0 = psr0;
    }
    Ok(())
}

fn targets_to_vec(t: &AutoloadTarget) -> Vec<String> {
    match t {
        AutoloadTarget::One(s) => vec![s.clone()],
        AutoloadTarget::Many(v) => v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn parses_psr4_and_psr0() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("composer.json");
        tokio::fs::write(
            &p,
            r#"{
              "autoload": {
                "psr-4": {
                  "App\\": "src/",
                  "Tests\\": ["tests/unit/", "tests/integration/"]
                },
                "psr-0": {
                  "Legacy_": "legacy/"
                }
              }
            }"#,
        )
        .await
        .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        let ws = &ctx.workspaces[0];
        assert_eq!(ws.php_psr4.len(), 3);
        assert_eq!(ws.php_psr0.len(), 1);
        assert!(ws
            .php_psr4
            .iter()
            .any(|m| m.namespace == "App\\" && m.directory == "src/"));
    }

    #[tokio::test]
    async fn handles_empty_composer() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("composer.json");
        tokio::fs::write(&p, r#"{"name":"x/y"}"#).await.unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        assert_eq!(ctx.workspaces.len(), 1);
        assert!(ctx.workspaces[0].php_psr4.is_empty());
    }
}
