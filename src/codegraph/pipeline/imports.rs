//! Resolve import/require/use statements.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;

use super::phase::{Phase, PhaseCtx};
use super::PhaseResult;
use crate::codegraph::config::{ConfigContext, TsPathAlias};
use crate::codegraph::db::SharedCodeGraphDb;
use crate::codegraph::types::PipelinePhase;

/// Escape a string for use in a Cypher string literal.
fn cypher_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Result from import resolution including the import map for downstream phases.
pub struct ImportPhaseResult {
    pub phase: PhaseResult,
    /// Map of source_file → set of imported file paths (for call resolution).
    pub import_map: HashMap<String, HashSet<String>>,
    /// Number of files that participate in at least one import cycle.
    /// Computed via Kahn's topological sort over `import_map`.
    pub cycle_count: u32,
}

/// Count files that sit in at least one import cycle. Runs Kahn's
/// topological sort: files with 0 in-edges are peeled off repeatedly;
/// anything left is part of a strongly-connected component. Cheap O(V+E).
fn count_import_cycle_files(import_map: &HashMap<String, HashSet<String>>) -> u32 {
    if import_map.is_empty() {
        return 0;
    }

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for (src, targets) in import_map {
        in_degree.entry(src.as_str()).or_insert(0);
        for tgt in targets {
            *in_degree.entry(tgt.as_str()).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter_map(|(k, v)| (*v == 0).then_some(*k))
        .collect();

    while let Some(node) = queue.pop() {
        if let Some(targets) = import_map.get(node) {
            for tgt in targets {
                if let Some(d) = in_degree.get_mut(tgt.as_str()) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push(tgt.as_str());
                    }
                }
            }
        }
        in_degree.remove(node);
    }

    in_degree.len() as u32
}

/// Resolve import statements and create IMPORTS edges between files.
///
/// Queries all Import nodes, resolves their `import_source` to target File
/// nodes, and creates CodeRelation edges of type IMPORTS.
pub async fn resolve_imports(
    project_id: &str,
    db: &SharedCodeGraphDb,
    config: &ConfigContext,
) -> Result<ImportPhaseResult> {
    resolve_imports_scoped(project_id, db, config, None).await
}

/// Scoped variant of `resolve_imports`: if `scope_files` is `Some`, only
/// Import nodes whose `source_file` is in the set are processed. Used by
/// the incremental pipeline to avoid duplicating IMPORTS edges for
/// unchanged files.
pub async fn resolve_imports_scoped(
    project_id: &str,
    db: &SharedCodeGraphDb,
    config: &ConfigContext,
    scope_files: Option<&HashSet<String>>,
) -> Result<ImportPhaseResult> {
    let mut result = PhaseResult::default();
    let mut import_map: HashMap<String, HashSet<String>> = HashMap::new();
    let pid = cypher_escape(project_id);

    tracing::debug!(
        project_id = %project_id,
        scoped = scope_files.is_some(),
        "resolving imports"
    );

    // 1. Query all Import nodes with source file, import_source, name,
    //    and extends_type (which carries the original name for aliased
    //    imports like `import { Foo as Bar }`).
    let imports = db.query(&format!(
        "MATCH (i:Import) WHERE i.project_id = '{pid}' \
         RETURN i.source_file, i.import_source, i.name, i.extends_type"
    )).await?;

    // 2. Query all File nodes to build a lookup map.
    let files = db.query(&format!(
        "MATCH (f:File) WHERE f.project_id = '{pid}' \
         RETURN f.source_file, f.qualified_name"
    )).await?;

    let mut file_by_path: HashMap<String, String> = HashMap::new();
    for row in &files {
        if let (Some(lbug::Value::String(path)), Some(lbug::Value::String(qname))) =
            (row.first(), row.get(1))
        {
            file_by_path.insert(path.clone(), qname.clone());
            // Also index without extension for fuzzy matching
            if let Some(stem) = path.strip_suffix(".ts")
                .or_else(|| path.strip_suffix(".tsx"))
                .or_else(|| path.strip_suffix(".js"))
                .or_else(|| path.strip_suffix(".jsx"))
                .or_else(|| path.strip_suffix(".rs"))
                .or_else(|| path.strip_suffix(".py"))
            {
                file_by_path.entry(stem.to_string()).or_insert_with(|| qname.clone());
                // index.ts convention
                let index_path = format!("{stem}/index.ts");
                file_by_path.entry(index_path).or_insert_with(|| qname.clone());
            }
        }
    }

    // 2b. Build a (source_file, symbol_name) → (symbol_qname, symbol_label)
    //     lookup across every symbol kind that an import can target.
    //     Keyed by source_file because the resolver first narrows to the
    //     target file via import_source, then looks up the name inside.
    let mut symbols_by_file_name: HashMap<(String, String), (String, String)> = HashMap::new();
    for label in &["Function", "Method", "Class", "Interface", "Struct", "Trait", "Variable", "Enum", "TypeAlias", "Const"] {
        let rows = db.query(&format!(
            "MATCH (n:{label}) WHERE n.project_id = '{pid}' \
             RETURN n.source_file, n.name, n.qualified_name"
        )).await?;
        for row in &rows {
            if let (
                Some(lbug::Value::String(sf)),
                Some(lbug::Value::String(name)),
                Some(lbug::Value::String(qname)),
            ) = (row.first(), row.get(1), row.get(2))
            {
                symbols_by_file_name
                    .entry((sf.clone(), name.clone()))
                    .or_insert_with(|| (qname.clone(), label.to_string()));
            }
        }
    }

    // 3. For each import, resolve to a file and create an IMPORTS edge.
    let mut edge_stmts: Vec<String> = Vec::new();

    for row in &imports {
        let (source_file, import_source, name, original_name) = match (row.first(), row.get(1), row.get(2), row.get(3)) {
            (
                Some(lbug::Value::String(sf)),
                Some(lbug::Value::String(is)),
                Some(lbug::Value::String(n)),
                orig,
            ) => {
                let orig_str = match orig {
                    Some(lbug::Value::String(o)) if !o.is_empty() => o.as_str(),
                    _ => "",
                };
                (sf, is, n.as_str(), orig_str)
            }
            _ => continue,
        };

        if import_source.is_empty() {
            continue;
        }

        // Scoped runs only process imports originating in the scope set.
        if let Some(scope) = scope_files
            && !scope.contains(source_file)
        {
            continue;
        }

        // Clean up import source: strip quotes, normalize
        let cleaned = import_source
            .trim_matches(|c| c == '\'' || c == '"')
            .replace("use ", "")
            .trim()
            .to_string();

        // Try to resolve relative to source file's directory
        let source_dir = source_file
            .rfind(['/', '\\'])
            .map(|i| &source_file[..i])
            .unwrap_or("");

        // Alias-based candidates (tsconfig paths, go.mod module prefix,
        // PSR-4 namespaces, etc.) go first — they're the right answer
        // when they match, and the tail heuristics below are the
        // fallback for imports with no config-driven resolution.
        let mut candidates: Vec<String> =
            apply_aliases(&cleaned, source_file, config);
        candidates.extend([
            // Direct path
            cleaned.clone(),
            // Relative to source dir
            format!("{source_dir}/{cleaned}"),
            // With common extensions
            format!("{source_dir}/{cleaned}.ts"),
            format!("{source_dir}/{cleaned}.tsx"),
            format!("{source_dir}/{cleaned}.rs"),
            format!("{source_dir}/{cleaned}.py"),
            format!("{cleaned}.ts"),
            format!("{cleaned}.tsx"),
            format!("{cleaned}.rs"),
            format!("{cleaned}.py"),
            // Rust-style mod resolution
            format!("{cleaned}/mod.rs"),
        ]);

        // Normalize path separators and strip leading ./
        for candidate in &candidates {
            let normalized = candidate
                .replace('\\', "/")
                .trim_start_matches("./")
                .to_string();

            if let Some(target_qname) = file_by_path.get(&normalized) {
                let src_qname = match file_by_path.get(source_file.as_str()) {
                    Some(q) => q,
                    None => continue,
                };

                let src_escaped = cypher_escape(src_qname);
                let tgt_escaped = cypher_escape(target_qname);

                edge_stmts.push(format!(
                    "MATCH (s:File), (t:File) WHERE s.qualified_name = '{src_escaped}' \
                     AND t.qualified_name = '{tgt_escaped}' \
                     CREATE (s)-[:CodeRelation {{type: 'IMPORTS', confidence: 1.0, reason: 'import statement', step: 0}}]->(t)",
                ));

                if !name.is_empty() && name != "*" {
                    // Named import — try the local name first, then the
                    // original name for aliased imports. `Bar` from
                    // `import { Foo as Bar }` won't match any symbol
                    // in the target file, but `Foo` will.
                    let lookup_name = if let Some((sq, sl)) =
                        symbols_by_file_name.get(&(normalized.clone(), name.to_string()))
                    {
                        Some((sq.clone(), sl.clone()))
                    } else if !original_name.is_empty() {
                        symbols_by_file_name
                            .get(&(normalized.clone(), original_name.to_string()))
                            .map(|(sq, sl)| (sq.clone(), sl.clone()))
                    } else {
                        None
                    };
                    if let Some((sym_qname, sym_label)) = lookup_name.as_ref()
                    {
                        let sym_escaped = cypher_escape(sym_qname);
                        edge_stmts.push(format!(
                            "MATCH (s:File), (t:{sym_label}) WHERE s.qualified_name = '{src_escaped}' \
                             AND t.qualified_name = '{sym_escaped}' \
                             CREATE (s)-[:CodeRelation {{type: 'IMPORTS', confidence: 0.95, reason: 'symbol import', step: 0}}]->(t)",
                        ));
                    }
                } else if name == "*" {
                    // Wildcard import — synthesize per-symbol edges for
                    // every exported symbol in the target file. This
                    // expands Go whole-module imports, Python `from x
                    // import *`, and C++ `using namespace` into the same
                    // File→Symbol edges that named imports produce.
                    for ((sf, sym_name), (sym_qname, sym_label)) in &symbols_by_file_name {
                        if sf == &normalized {
                            let sym_escaped = cypher_escape(sym_qname);
                            edge_stmts.push(format!(
                                "MATCH (s:File), (t:{sym_label}) WHERE s.qualified_name = '{src_escaped}' \
                                 AND t.qualified_name = '{sym_escaped}' \
                                 CREATE (s)-[:CodeRelation {{type: 'IMPORTS', confidence: 0.80, reason: 'wildcard import', step: 0}}]->(t)",
                            ));
                            let _ = sym_name; // suppress unused warning
                        }
                    }
                }

                // Record in import map for call resolution
                import_map
                    .entry(source_file.clone())
                    .or_default()
                    .insert(normalized);

                break;
            }
        }
    }

    // 4. Execute edge batch.
    if !edge_stmts.is_empty() {
        let batch = db.execute_batch(edge_stmts).await?;
        result.edges_created += batch.success;
        result.errors += batch.errors;
    }

    let cycle_count = count_import_cycle_files(&import_map);

    tracing::info!(
        project_id = %project_id,
        edges = result.edges_created,
        import_entries = import_map.len(),
        cycle_files = cycle_count,
        "import resolution complete"
    );

    Ok(ImportPhaseResult {
        phase: result,
        import_map,
        cycle_count,
    })
}

/// Imports phase: resolves `import_source` paths on Import nodes into
/// IMPORTS edges between files and stashes the resulting `import_map` on
/// the context so the Calls phase can use it for tier-2 resolution.
pub struct ImportsPhase;

#[async_trait::async_trait]
impl Phase for ImportsPhase {
    fn label(&self) -> &'static str {
        "imports"
    }

    fn phase(&self) -> Option<PipelinePhase> {
        Some(PipelinePhase::Imports)
    }

    async fn run(&self, ctx: &mut PhaseCtx) -> Result<()> {
        ctx.emit_progress(PipelinePhase::Imports, 0.0, "Resolving imports");
        let result = resolve_imports(&ctx.project_id, &ctx.db, &ctx.config_context).await?;
        ctx.stats.nodes_created += result.phase.nodes_created;
        ctx.stats.edges_created += result.phase.edges_created;
        ctx.import_map = result.import_map;
        ctx.emit_progress(PipelinePhase::Imports, 1.0, "Imports resolved");
        Ok(())
    }
}

/// Translate an `import_source` through the workspace's configured
/// build-system aliases into one or more project-root-relative
/// candidate paths. The calling resolver then tries each candidate
/// against the file index.
///
/// The helper is a *producer* of candidates — it never decides which
/// wins. When no alias matches, it returns an empty `Vec` and the
/// fallback heuristics below take over.
fn apply_aliases(
    import_source: &str,
    source_file: &str,
    config: &ConfigContext,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let source_path = Path::new(source_file);
    let ws = match config.workspace_for(source_path) {
        Some(w) => w,
        None => return out,
    };

    // TypeScript / JavaScript path aliases.
    for alias in &ws.ts_paths {
        for candidate in expand_ts_alias(import_source, alias, &ws.root) {
            out.push(candidate);
        }
    }

    // TypeScript `baseUrl`: non-relative, non-aliased imports resolve
    // from `baseUrl`. We add it as a lower-priority candidate so an
    // explicit path alias still wins when both match.
    if let Some(base) = &ws.ts_base_url
        && !import_source.starts_with('.')
        && !import_source.starts_with('/')
    {
        out.push(join_rel(&ws.root, &base.join(import_source)));
    }

    // Go: strip the module prefix so `github.com/foo/bar/pkg/x` maps
    // to `pkg/x` under the workspace root.
    if let Some(module) = &ws.go_module
        && let Some(stripped) = import_source
            .strip_prefix(module)
            .and_then(|rest| rest.strip_prefix('/').or(Some(rest)))
        && !stripped.is_empty()
    {
        out.push(join_rel(&ws.root, Path::new(stripped)));
    }

    // PHP PSR-4: `App\Models\User` under `App\ → src/` maps to
    // `src/Models/User`. The candidate list below will try `.php`.
    for mapping in ws.php_psr4.iter().chain(ws.php_psr0.iter()) {
        if let Some(stripped) = strip_php_namespace(import_source, &mapping.namespace) {
            let rel = Path::new(&mapping.directory).join(stripped.replace('\\', "/"));
            out.push(join_rel(&ws.root, &rel));
        }
    }

    // C# root namespace: `MyApp.Services.Foo` under RootNamespace
    // `MyApp` maps to `Services/Foo`.
    if let Some(ns) = &ws.csharp_root_namespace
        && let Some(stripped) = import_source
            .strip_prefix(ns)
            .and_then(|rest| rest.strip_prefix('.').or(Some(rest)))
        && !stripped.is_empty()
    {
        let rel = stripped.replace('.', "/");
        out.push(join_rel(&ws.root, Path::new(&rel)));
    }

    // Dart: `package:my_app/utils/foo.dart` under package `my_app`
    // maps to `lib/utils/foo.dart`.
    if let Some(pkg) = &ws.dart_package {
        let prefix = format!("package:{pkg}/");
        if let Some(rest) = import_source.strip_prefix(&prefix) {
            let rel = Path::new("lib").join(rest);
            out.push(join_rel(&ws.root, &rel));
        }
    }

    // Swift: `import Foo` resolves to `Sources/Foo/` (or the target's
    // `path:` override). Swift imports are module-granularity, so the
    // candidate is the target root directory — the resolver's file
    // index still has to find a matching file inside.
    for target in &ws.swift_targets {
        if target.name == import_source {
            let dir = target
                .path
                .as_deref()
                .map(Path::new)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| Path::new("Sources").join(&target.name));
            out.push(join_rel(&ws.root, &dir));
        }
    }

    out
}

/// Expand a tsconfig `paths` entry against an import source. Returns
/// zero-or-more candidates (tsconfig allows an alias to resolve to
/// multiple target directories).
fn expand_ts_alias(
    import_source: &str,
    alias: &TsPathAlias,
    ws_root: &Path,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(star) = alias.pattern.find('*') {
        let prefix = &alias.pattern[..star];
        let suffix = &alias.pattern[star + 1..];
        if import_source.starts_with(prefix) && import_source.ends_with(suffix) {
            let captured = &import_source[prefix.len()..import_source.len() - suffix.len()];
            for target in &alias.targets {
                let resolved = target.replace('*', captured);
                out.push(join_rel(ws_root, Path::new(&resolved)));
            }
        }
    } else if import_source == alias.pattern {
        for target in &alias.targets {
            out.push(join_rel(ws_root, Path::new(target)));
        }
    }
    out
}

/// Strip a PSR-4 / PSR-0 namespace prefix from a class-style import.
/// PSR-4 namespaces use `\` as the separator and always end with `\`.
/// We normalize by checking both with and without the trailing
/// separator.
fn strip_php_namespace(import_source: &str, namespace: &str) -> Option<String> {
    // `App\\` canonical form.
    if let Some(rest) = import_source.strip_prefix(namespace) {
        return Some(rest.to_string());
    }
    // Tolerate a missing trailing backslash in the namespace config.
    let without_trailing = namespace.trim_end_matches('\\');
    if !without_trailing.is_empty()
        && let Some(rest) = import_source.strip_prefix(without_trailing)
    {
        return Some(rest.trim_start_matches('\\').to_string());
    }
    None
}

/// Join a workspace-root-relative base with a sub-path and normalize
/// to forward-slash form. The resolver downstream expects forward
/// slashes because the file index keys are stored that way.
fn join_rel(ws_root: &Path, rel: &Path) -> String {
    let joined = ws_root.join(rel);
    joined.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::config::{
        ConfigContext, Psr4Mapping, SwiftTarget, TsPathAlias, WorkspaceConfig,
    };
    use std::path::PathBuf;

    fn ctx_with(mut ws: WorkspaceConfig) -> ConfigContext {
        ws.root = PathBuf::new();
        let mut c = ConfigContext::default();
        c.add_workspace(ws);
        c
    }

    #[test]
    fn ts_path_alias_expands_wildcard() {
        let ctx = ctx_with(WorkspaceConfig {
            ts_paths: vec![TsPathAlias {
                pattern: "@/*".to_string(),
                targets: vec!["src/*".to_string()],
            }],
            ..Default::default()
        });
        let got = apply_aliases("@/utils/foo", "src/app.ts", &ctx);
        assert_eq!(got, vec!["src/utils/foo".to_string()]);
    }

    #[test]
    fn ts_alias_with_multiple_targets_produces_multiple_candidates() {
        let ctx = ctx_with(WorkspaceConfig {
            ts_paths: vec![TsPathAlias {
                pattern: "@shared/*".to_string(),
                targets: vec!["shared/*".to_string(), "vendor/shared/*".to_string()],
            }],
            ..Default::default()
        });
        let got = apply_aliases("@shared/logger", "src/app.ts", &ctx);
        assert_eq!(
            got,
            vec![
                "shared/logger".to_string(),
                "vendor/shared/logger".to_string()
            ]
        );
    }

    #[test]
    fn go_module_prefix_stripped() {
        let ctx = ctx_with(WorkspaceConfig {
            go_module: Some("github.com/acme/svc".to_string()),
            ..Default::default()
        });
        let got = apply_aliases("github.com/acme/svc/pkg/foo", "cmd/main.go", &ctx);
        assert_eq!(got, vec!["pkg/foo".to_string()]);
    }

    #[test]
    fn psr4_namespace_mapped_to_directory() {
        let ctx = ctx_with(WorkspaceConfig {
            php_psr4: vec![Psr4Mapping {
                namespace: "App\\".to_string(),
                directory: "src/".to_string(),
            }],
            ..Default::default()
        });
        let got = apply_aliases("App\\Models\\User", "src/app.php", &ctx);
        assert_eq!(got, vec!["src/Models/User".to_string()]);
    }

    #[test]
    fn csharp_root_namespace_mapped_to_path() {
        let ctx = ctx_with(WorkspaceConfig {
            csharp_root_namespace: Some("MyApp".to_string()),
            ..Default::default()
        });
        let got = apply_aliases("MyApp.Services.UserService", "Program.cs", &ctx);
        assert_eq!(got, vec!["Services/UserService".to_string()]);
    }

    #[test]
    fn dart_package_prefix_mapped_to_lib() {
        let ctx = ctx_with(WorkspaceConfig {
            dart_package: Some("my_app".to_string()),
            ..Default::default()
        });
        let got = apply_aliases("package:my_app/utils/foo.dart", "lib/app.dart", &ctx);
        assert_eq!(got, vec!["lib/utils/foo.dart".to_string()]);
    }

    #[test]
    fn swift_target_name_mapped_to_sources() {
        let ctx = ctx_with(WorkspaceConfig {
            swift_targets: vec![SwiftTarget {
                name: "MyLib".to_string(),
                path: None,
            }],
            ..Default::default()
        });
        let got = apply_aliases("MyLib", "Sources/App/Main.swift", &ctx);
        assert_eq!(got, vec!["Sources/MyLib".to_string()]);
    }

    #[test]
    fn swift_target_with_custom_path_used() {
        let ctx = ctx_with(WorkspaceConfig {
            swift_targets: vec![SwiftTarget {
                name: "MyLib".to_string(),
                path: Some("Custom/Dir".to_string()),
            }],
            ..Default::default()
        });
        let got = apply_aliases("MyLib", "Sources/App/Main.swift", &ctx);
        assert_eq!(got, vec!["Custom/Dir".to_string()]);
    }

    #[test]
    fn no_matches_returns_empty() {
        let ctx = ctx_with(WorkspaceConfig::default());
        let got = apply_aliases("./utils/foo", "src/app.ts", &ctx);
        assert!(got.is_empty());
    }
}
