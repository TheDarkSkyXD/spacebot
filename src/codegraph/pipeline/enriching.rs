//! Deterministic community enrichment pass.
//!
//! Reserved as the wiring point for any future deterministic enrichment
//! work that runs against the existing LadybugDB graph (e.g. framework
//! detection from member imports, language-mix per cluster). Currently a
//! no-op because `communities.rs` already produces human-readable labels
//! at insertion time using folder-prefix + dominant-symbol-kind heuristics.
//!
//! No model is invoked here. No network call is made. The function exists
//! so the pipeline orchestrator has a stable hook to extend later without
//! reshuffling phase order.

use std::time::Instant;

use anyhow::Result;

use super::phase::{Phase, PhaseCtx};
use crate::codegraph::db::SharedCodeGraphDb;
use crate::codegraph::schema;
use crate::codegraph::types::PipelinePhase;

/// Run the deterministic enrichment pass for a project.
///
/// Walks the graph's Community nodes and annotates each one with a
/// dominant-language tag derived from its member files'  extensions.
/// The tag goes into the `description` column alongside whatever
/// `communities.rs` already wrote there — we append so the clustering
/// phase's density / modularity annotation isn't clobbered.
pub async fn enrich(project_id: &str, db: &SharedCodeGraphDb) -> Result<()> {
    let pid = project_id.replace('\\', "\\\\").replace('\'', "\\'");

    // Pull Community nodes with their current description. Small
    // projects can have zero communities — skip gracefully.
    let rows = db
        .query(&format!(
            "MATCH (c:Community) WHERE c.project_id = '{pid}' \
             RETURN c.qualified_name, c.description"
        ))
        .await?;
    if rows.is_empty() {
        return Ok(());
    }

    for row in &rows {
        let Some(lbug::Value::String(qname)) = row.first() else {
            continue;
        };
        let existing_desc = match row.get(1) {
            Some(lbug::Value::String(s)) => s.clone(),
            _ => String::new(),
        };

        // Count files per extension inside this community. Memberships
        // are File → Community MEMBER_OF edges (symbols inherit their
        // File's language, so file-level counting is sufficient).
        let qname_escaped = qname.replace('\\', "\\\\").replace('\'', "\\'");
        let member_rows = db
            .query(&format!(
                "MATCH (f:File)-[r:CodeRelation]->(c:Community) \
                 WHERE r.type = 'MEMBER_OF' \
                   AND c.qualified_name = '{qname_escaped}' \
                   AND c.project_id = '{pid}' \
                 RETURN f.source_file"
            ))
            .await
            .unwrap_or_default();

        let mut ext_counts: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        for mrow in &member_rows {
            if let Some(lbug::Value::String(path)) = mrow.first() {
                let ext = path
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !ext.is_empty() {
                    *ext_counts.entry(ext).or_default() += 1;
                }
            }
        }

        if ext_counts.is_empty() {
            continue;
        }

        // Pick the extension with the highest count; ties broken
        // alphabetically by the BTreeMap iteration order for
        // determinism.
        let (dominant, count) = ext_counts
            .iter()
            .max_by_key(|(_, c)| **c)
            .map(|(e, c)| (e.clone(), *c))
            .unwrap();

        let annotation = format!("lang={dominant};files={count}");
        let merged = if existing_desc.is_empty() {
            annotation
        } else if existing_desc.contains("lang=") {
            // Already enriched — leave alone so re-runs are idempotent.
            continue;
        } else {
            format!("{existing_desc}; {annotation}")
        };
        let merged_escaped = merged.replace('\\', "\\\\").replace('\'', "\\'");

        let _ = db
            .execute(&format!(
                "MATCH (c:Community) \
                 WHERE c.qualified_name = '{qname_escaped}' \
                   AND c.project_id = '{pid}' \
                 SET c.description = '{merged_escaped}'"
            ))
            .await;
    }

    tracing::debug!(
        project_id = %project_id,
        communities = rows.len(),
        "enriched communities with dominant-language tags"
    );
    Ok(())
}

/// Enriching phase: bundles deterministic enrichment, embedding
/// generation, pipeline-only-node cleanup (Parameter nodes), and FTS
/// index construction. Reports a single Enriching progress segment to
/// the UI (0.0 → 1.0) but records an additional `fts` phase_timings
/// entry for the FTS sub-step so perf dashboards can see it separately.
pub struct EnrichingPhase;

#[async_trait::async_trait]
impl Phase for EnrichingPhase {
    fn label(&self) -> &'static str {
        "enriching"
    }

    fn phase(&self) -> Option<PipelinePhase> {
        Some(PipelinePhase::Enriching)
    }

    async fn run(&self, ctx: &mut PhaseCtx) -> Result<()> {
        ctx.emit_progress(PipelinePhase::Enriching, 0.0, "Enriching graph");
        if let Err(err) = enrich(&ctx.project_id, &ctx.db).await {
            tracing::warn!(%err, "enrichment pass failed, continuing");
        }

        ctx.emit_progress(PipelinePhase::Enriching, 0.2, "Generating embeddings");
        if ctx.config.skip_embeddings {
            tracing::debug!(
                project_id = %ctx.project_id,
                "skip_embeddings set — bypassing fastembed init"
            );
        } else if let Err(err) =
            super::embeddings::generate_embeddings(&ctx.project_id, &ctx.root_path, &ctx.db).await
        {
            tracing::warn!(%err, "embeddings pass failed, continuing");
        }

        ctx.emit_progress(
            PipelinePhase::Enriching,
            0.4,
            "Cleaning up temporary nodes",
        );
        cleanup_pipeline_only_nodes(ctx).await;

        ctx.emit_progress(PipelinePhase::Enriching, 0.7, "Building search index");

        // FTS timing recorded separately from overall enriching so perf
        // dashboards can surface it; the outer loop still records the
        // umbrella "enriching" timing via `Phase::label`.
        let fts_start = Instant::now();
        match super::fts::build_fts_index(&ctx.project_id, &ctx.db).await {
            Ok(fts_result) => {
                tracing::info!(
                    project_id = %ctx.project_id,
                    indexed = fts_result.nodes_created,
                    "FTS index ready"
                );
            }
            Err(err) => {
                tracing::warn!(%err, "FTS indexing failed, continuing without search index");
            }
        }
        ctx.phase_timings
            .insert("fts".to_string(), fts_start.elapsed().as_secs_f64());

        ctx.emit_progress(PipelinePhase::Enriching, 1.0, "Enrichment complete");
        Ok(())
    }
}

/// Delete pipeline-only nodes (Parameter) and their connected edges, then
/// re-count the remaining edges so `ctx.stats.edges_created` reflects the
/// post-cleanup total. Parameter nodes are needed to resolve argument
/// types against callee signatures during the Calls phase but would
/// flood the graph with per-call noise, so they're purged here.
async fn cleanup_pipeline_only_nodes(ctx: &mut PhaseCtx) {
    let pid = ctx
        .project_id
        .replace('\\', "\\\\")
        .replace('\'', "\\'");
    let mut nodes_removed: u64 = 0;
    for &label in schema::PIPELINE_ONLY_LABELS {
        let count = ctx
            .db
            .query_scalar_i64(&format!(
                "MATCH (n:{label}) WHERE n.project_id = '{pid}' RETURN count(n)"
            ))
            .await
            .unwrap_or(Some(0))
            .unwrap_or(0);
        if count > 0 {
            ctx.db
                .execute(&format!(
                    "MATCH (n:{label}) WHERE n.project_id = '{pid}' DETACH DELETE n"
                ))
                .await
                .ok();
            nodes_removed += count as u64;
            tracing::debug!(label, count, "deleted pipeline-only nodes");
        }
    }
    if nodes_removed > 0 {
        ctx.stats.nodes_created = ctx.stats.nodes_created.saturating_sub(nodes_removed);
        // Recount edges since DETACH DELETE removed connected edges too.
        let mut total_edges: u64 = 0;
        for &from_label in schema::DISPLAY_NODE_LABELS {
            let edge_count = ctx
                .db
                .query_scalar_i64(&format!(
                    "MATCH (a:{from_label})-[r:CodeRelation]->() \
                     WHERE a.project_id = '{pid}' RETURN count(r)"
                ))
                .await
                .unwrap_or(Some(0))
                .unwrap_or(0);
            total_edges += edge_count as u64;
        }
        ctx.stats.edges_created = total_edges;
        tracing::info!(
            nodes_removed,
            final_nodes = ctx.stats.nodes_created,
            final_edges = ctx.stats.edges_created,
            "pipeline-only nodes cleaned up"
        );
    }
}
