//! Project-configuration parsing.
//!
//! Each supported build system / package manager has a parser in its
//! own submodule. Parsers are **best-effort**: they should never fail
//! the pipeline on a malformed config — they return `Result` so callers
//! can log and continue. The unified [`ConfigContext`] produced by
//! [`load_config_context`] is threaded through the pipeline via
//! [`super::pipeline::phase::PhaseCtx`] so the imports phase can resolve
//! path aliases, module prefixes, and PSR-4 namespaces into concrete
//! file paths.
//!
//! ## Why a per-workspace model
//!
//! Real projects are monorepos. A pnpm workspace might have a
//! `tsconfig.json` at the root **and** a different one in each package
//! under `packages/*`. The correct tsconfig for resolving an import in
//! `packages/core/src/foo.ts` is the closest-ancestor tsconfig.
//!
//! We model this with one [`WorkspaceConfig`] per manifest-containing
//! directory. At resolution time [`ConfigContext::workspace_for`]
//! returns the nearest-ancestor config for a given source file. That
//! way every import resolution is scoped to its local build system.

use std::path::{Path, PathBuf};

pub mod cargo_workspace;
pub mod composer;
pub mod csproj;
pub mod go_mod;
pub mod maven_gradle;
pub mod package_swift;
pub mod pubspec;
pub mod tsconfig;

/// All build-system configuration discovered under a project root,
/// grouped by workspace directory. See the module docs for the
/// workspace model.
#[derive(Debug, Clone, Default)]
pub struct ConfigContext {
    /// Workspaces in no particular order. [`Self::workspace_for`]
    /// picks the nearest-ancestor at lookup time, so storage order
    /// doesn't matter.
    pub workspaces: Vec<WorkspaceConfig>,
}

impl ConfigContext {
    /// Find the workspace whose root is the nearest ancestor of
    /// `source_file_rel` (a project-root-relative path). Returns
    /// `None` when no workspace contains the file — typically meaning
    /// the project has no manifests at all.
    pub fn workspace_for(&self, source_file_rel: &Path) -> Option<&WorkspaceConfig> {
        self.workspaces
            .iter()
            .filter(|ws| source_file_rel.starts_with(&ws.root))
            .max_by_key(|ws| ws.root.components().count())
    }

    /// Append a workspace entry. Duplicates (same root) are ignored —
    /// the first registration wins because structure.rs walks
    /// manifests in an order we don't want to perturb.
    pub fn add_workspace(&mut self, workspace: WorkspaceConfig) {
        if self.workspaces.iter().any(|w| w.root == workspace.root) {
            return;
        }
        self.workspaces.push(workspace);
    }
}

/// Build-system configuration rooted at a single manifest-containing
/// directory. Every field is optional so a workspace with (say) only a
/// `go.mod` populates just `go_module` and leaves the rest empty.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceConfig {
    /// Project-root-relative path to this workspace's manifest
    /// directory. Empty `PathBuf` means "project root".
    pub root: PathBuf,

    /// TypeScript/JavaScript path aliases from `tsconfig.json`.
    pub ts_paths: Vec<TsPathAlias>,
    /// `tsconfig.json` `compilerOptions.baseUrl`, relative to `root`.
    /// When unset, relative imports resolve from the source file's
    /// directory.
    pub ts_base_url: Option<PathBuf>,

    /// Go module path prefix from `go.mod`. E.g. `github.com/foo/bar`.
    pub go_module: Option<String>,

    /// PHP PSR-4 autoload mappings from `composer.json`.
    pub php_psr4: Vec<Psr4Mapping>,
    /// PHP PSR-0 autoload mappings from `composer.json` (legacy).
    pub php_psr0: Vec<Psr4Mapping>,

    /// C# `<RootNamespace>` from `.csproj`.
    pub csharp_root_namespace: Option<String>,
    /// C# `<AssemblyName>`. Often equals `RootNamespace`.
    pub csharp_assembly_name: Option<String>,

    /// Swift `Package.swift` targets.
    pub swift_targets: Vec<SwiftTarget>,

    /// Dart package name from `pubspec.yaml`.
    pub dart_package: Option<String>,

    /// JVM coordinates from `pom.xml` / `build.gradle(.kts)`.
    pub jvm_coords: Option<JvmCoords>,

    /// Cargo workspace member paths (relative to `root`) from a
    /// `Cargo.toml` `[workspace] members` entry.
    pub cargo_workspace_members: Vec<PathBuf>,
}

/// One row of a `tsconfig.json` `compilerOptions.paths` map.
///
/// Example: `"@/*": ["src/*"]` → `pattern: "@/*"`, `targets: ["src/*"]`.
/// Globs use `*` as the single wildcard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsPathAlias {
    pub pattern: String,
    pub targets: Vec<String>,
}

/// One PSR-4 or PSR-0 autoload row.
///
/// Example: `"App\\": "src/"` → `namespace: "App\\"`, `directory: "src/"`.
/// The trailing backslash on `namespace` is significant — PSR-4 requires
/// it — so parsers preserve it verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Psr4Mapping {
    pub namespace: String,
    pub directory: String,
}

/// One `Package.swift` target declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwiftTarget {
    pub name: String,
    /// Target source directory, usually `Sources/<name>` unless overridden.
    pub path: Option<String>,
}

/// Maven / Gradle coordinates. One of `pom.xml` or a `build.gradle*`
/// file produces this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JvmCoords {
    pub group_id: Option<String>,
    pub artifact_id: Option<String>,
}

/// Scan `root` for recognized manifests and parse each one. Manifest
/// parsers are best-effort: failures are logged and the workspace is
/// still registered so downstream phases see it as a structural marker.
///
/// `files` is the walker's output — we only inspect files already
/// selected for indexing, so the same ignore rules apply.
pub async fn load_config_context(root: &Path, files: &[PathBuf]) -> ConfigContext {
    let mut ctx = ConfigContext::default();

    for file in files {
        let name = match file.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let manifest_dir = match file.parent() {
            Some(p) => p,
            None => continue,
        };
        let workspace_root = manifest_dir
            .strip_prefix(root)
            .map(|p| p.to_path_buf())
            .unwrap_or_default();

        match name {
            "tsconfig.json" => {
                if let Err(err) = tsconfig::load(file, &workspace_root, &mut ctx).await {
                    tracing::debug!(%err, path = %file.display(), "tsconfig parse failed");
                }
            }
            "go.mod" => {
                if let Err(err) = go_mod::load(file, &workspace_root, &mut ctx).await {
                    tracing::debug!(%err, path = %file.display(), "go.mod parse failed");
                }
            }
            "composer.json" => {
                if let Err(err) = composer::load(file, &workspace_root, &mut ctx).await {
                    tracing::debug!(%err, path = %file.display(), "composer.json parse failed");
                }
            }
            "Package.swift" => {
                if let Err(err) = package_swift::load(file, &workspace_root, &mut ctx).await {
                    tracing::debug!(%err, path = %file.display(), "Package.swift parse failed");
                }
            }
            "pubspec.yaml" => {
                if let Err(err) = pubspec::load(file, &workspace_root, &mut ctx).await {
                    tracing::debug!(%err, path = %file.display(), "pubspec.yaml parse failed");
                }
            }
            "pom.xml" | "build.gradle" | "build.gradle.kts" => {
                if let Err(err) =
                    maven_gradle::load(file, &workspace_root, &mut ctx).await
                {
                    tracing::debug!(%err, path = %file.display(), "JVM manifest parse failed");
                }
            }
            "Cargo.toml" => {
                if let Err(err) =
                    cargo_workspace::load(file, &workspace_root, &mut ctx).await
                {
                    tracing::debug!(%err, path = %file.display(), "Cargo.toml parse failed");
                }
            }
            n if n.ends_with(".csproj") => {
                if let Err(err) = csproj::load(file, &workspace_root, &mut ctx).await {
                    tracing::debug!(%err, path = %file.display(), ".csproj parse failed");
                }
            }
            _ => {}
        }
    }

    ctx
}

/// Return a mutable handle to the workspace rooted at `root`, inserting
/// a fresh one if none exists. Parsers use this so several manifests in
/// the same directory (e.g. `Cargo.toml` + `tsconfig.json`) all
/// contribute to a single `WorkspaceConfig`.
fn workspace_mut<'a>(ctx: &'a mut ConfigContext, root: &Path) -> &'a mut WorkspaceConfig {
    let pos = ctx.workspaces.iter().position(|w| w.root == root);
    match pos {
        Some(i) => &mut ctx.workspaces[i],
        None => {
            ctx.workspaces.push(WorkspaceConfig {
                root: root.to_path_buf(),
                ..Default::default()
            });
            ctx.workspaces.last_mut().expect("just pushed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_for_picks_nearest_ancestor() {
        let mut ctx = ConfigContext::default();
        ctx.add_workspace(WorkspaceConfig {
            root: PathBuf::from(""),
            ..Default::default()
        });
        ctx.add_workspace(WorkspaceConfig {
            root: PathBuf::from("packages/core"),
            ..Default::default()
        });
        ctx.add_workspace(WorkspaceConfig {
            root: PathBuf::from("packages/core/nested"),
            ..Default::default()
        });

        let ws = ctx
            .workspace_for(Path::new("packages/core/nested/src/foo.ts"))
            .expect("must resolve");
        assert_eq!(ws.root, PathBuf::from("packages/core/nested"));
    }

    #[test]
    fn workspace_for_falls_back_to_root() {
        let mut ctx = ConfigContext::default();
        ctx.add_workspace(WorkspaceConfig::default()); // empty root
        let ws = ctx
            .workspace_for(Path::new("some/file.rs"))
            .expect("root workspace always matches");
        assert_eq!(ws.root, PathBuf::from(""));
    }

    #[test]
    fn workspace_for_returns_none_when_no_ancestors() {
        let mut ctx = ConfigContext::default();
        ctx.add_workspace(WorkspaceConfig {
            root: PathBuf::from("packages/core"),
            ..Default::default()
        });
        assert!(ctx.workspace_for(Path::new("other/file.rs")).is_none());
    }

    #[test]
    fn add_workspace_is_idempotent() {
        let mut ctx = ConfigContext::default();
        ctx.add_workspace(WorkspaceConfig {
            root: PathBuf::from("a"),
            go_module: Some("first".to_string()),
            ..Default::default()
        });
        ctx.add_workspace(WorkspaceConfig {
            root: PathBuf::from("a"),
            go_module: Some("second".to_string()),
            ..Default::default()
        });
        assert_eq!(ctx.workspaces.len(), 1);
        assert_eq!(
            ctx.workspaces[0].go_module,
            Some("first".to_string()),
            "first registration wins"
        );
    }
}
