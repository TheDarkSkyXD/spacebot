//! BM25 + RRF search for the code graph.
//!
//! Codegraph indexing is fully deterministic and LadybugDB-only — no
//! vector embeddings are produced at index time, so search runs purely
//! over the FTS5 sqlite index built by the `fts` phase. Reciprocal rank
//! fusion is preserved as a single-source pass so the result shape and
//! scoring stay stable for any future hybrid extension.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use super::db::SharedCodeGraphDb;
use super::types::GraphSearchResult;
use crate::codegraph::NodeLabel;

/// Execute a search across the code graph.
///
/// Runs BM25 via FTS5 and re-scores results with RRF (k=60) so the
/// returned ordering matches the format any future hybrid extension
/// would produce. Returns an empty list if no FTS index exists yet.
pub async fn hybrid_search(
    project_id: &str,
    query: &str,
    limit: usize,
    db: &SharedCodeGraphDb,
) -> Result<Vec<GraphSearchResult>> {
    let project_dir = db.db_path.parent().unwrap_or(Path::new("."));

    // 1. BM25 search via FTS5 sqlite.
    let fts_results = fts_search(project_dir, query, limit * 3).await;
    let fts_results = fts_results.unwrap_or_default();

    // 2. Reciprocal Rank Fusion with k=60. Single-source today; the
    //    fusion math is preserved so adding a second source later is a
    //    drop-in change.
    let k = 60.0;
    let mut scores: HashMap<String, (f64, GraphSearchResult)> = HashMap::new();

    for (rank, result) in fts_results.into_iter().enumerate() {
        let rrf_score = 1.0 / (k + rank as f64 + 1.0);
        scores
            .entry(result.qualified_name.clone())
            .and_modify(|(s, _)| *s += rrf_score)
            .or_insert((rrf_score, result));
    }

    // 3. Sort by fused score, take top-k.
    let mut results: Vec<GraphSearchResult> = scores
        .into_values()
        .map(|(score, mut r)| {
            r.score = score;
            r
        })
        .collect();
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);

    tracing::debug!(
        project_id = %project_id,
        query = %query,
        results = results.len(),
        "code graph search complete"
    );

    Ok(results)
}

/// BM25 search via the FTS5 sqlite database.
async fn fts_search(
    project_dir: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<GraphSearchResult>> {
    let fts_path = project_dir.join("fts.sqlite");
    if !fts_path.exists() {
        return Ok(Vec::new());
    }

    let fts_url = format!(
        "sqlite:{}?mode=ro",
        fts_path.to_string_lossy().replace('\\', "/")
    );
    let pool = sqlx::sqlite::SqlitePool::connect(&fts_url)
        .await
        .with_context(|| format!("opening FTS database at {}", fts_path.display()))?;

    let fts_query = sanitize_fts_query(query);

    let rows: Vec<(String, String, String, String, f64)> = sqlx::query_as(
        "SELECT c.qualified_name, c.name, c.label, c.source_file, \
         bm25(symbols_fts) as score \
         FROM symbols_fts f \
         JOIN symbols_content c ON f.rowid = c.id \
         WHERE symbols_fts MATCH ?1 \
         ORDER BY score \
         LIMIT ?2",
    )
    .bind(&fts_query)
    .bind(limit as i64)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    pool.close().await;

    let results: Vec<GraphSearchResult> = rows
        .into_iter()
        .map(|(qname, name, label, source_file, score)| {
            let parsed_label = serde_json::from_value::<NodeLabel>(
                serde_json::Value::String(label.to_lowercase()),
            )
            .unwrap_or(NodeLabel::Function);

            GraphSearchResult {
                node_id: 0,
                qualified_name: qname,
                name,
                label: parsed_label,
                source_file: if source_file.is_empty() {
                    None
                } else {
                    Some(source_file)
                },
                line_start: None,
                score: score.abs(),
                community: None,
                snippet: None,
            }
        })
        .collect();

    Ok(results)
}

/// Sanitize a user query for FTS5 MATCH syntax.
fn sanitize_fts_query(query: &str) -> String {
    let tokens: Vec<String> = query
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|token| {
            let clean: String = token
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if clean.is_empty() {
                return String::new();
            }
            format!("{clean}*")
        })
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        return query.to_string();
    }

    tokens.join(" ")
}
