//! Hybrid BM25 + semantic search for the code graph.
//!
//! Combines LadybugDB's native FTS extension (per-table keyword indexes)
//! with vector similarity search (CodeEmbedding HNSW index). Results are
//! merged via Reciprocal Rank Fusion (k=60), matching GitNexus's approach.

use std::collections::HashMap;

use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use super::db::SharedCodeGraphDb;
use super::types::GraphSearchResult;
use crate::codegraph::NodeLabel;

const SEARCHABLE_LABELS: &[(&str, &str)] = &[
    ("Function", "function_fts"),
    ("Method", "method_fts"),
    ("Class", "class_fts"),
    ("Interface", "interface_fts"),
    ("Struct", "struct_fts"),
    ("Trait", "trait_fts"),
    ("Enum", "enum_fts"),
    ("TypeAlias", "typealias_fts"),
    ("Const", "const_fts"),
    ("Module", "module_fts"),
    ("Route", "route_fts"),
    ("Variable", "variable_fts"),
    ("Import", "import_fts"),
    ("Decorator", "decorator_fts"),
    ("File", "file_fts"),
    ("Constructor", "constructor_fts"),
    ("Property", "property_fts"),
    ("UnionType", "uniontype_fts"),
    ("Typedef", "typedef_fts"),
    ("Static", "static_fts"),
    ("Delegate", "delegate_fts"),
];

pub async fn hybrid_search(
    project_id: &str,
    query: &str,
    limit: usize,
    db: &SharedCodeGraphDb,
) -> Result<Vec<GraphSearchResult>> {
    let sanitized = sanitize_fts_query(query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }

    // Run BM25 and semantic search in parallel.
    let bm25_future = bm25_search(db, &sanitized, limit * 3);
    let semantic_future = semantic_search(project_id, db, query, limit * 3);

    let (bm25_results, semantic_results) = tokio::join!(bm25_future, semantic_future);
    let bm25_results = bm25_results.unwrap_or_default();
    let semantic_results = semantic_results.unwrap_or_default();

    // Reciprocal Rank Fusion (k=60) — merge both sources by qualified_name.
    let k = 60.0;
    let mut scores: HashMap<String, (f64, GraphSearchResult)> = HashMap::new();

    for (rank, result) in bm25_results.into_iter().enumerate() {
        let rrf = 1.0 / (k + rank as f64 + 1.0);
        scores
            .entry(result.qualified_name.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, result));
    }

    for (rank, result) in semantic_results.into_iter().enumerate() {
        let rrf = 1.0 / (k + rank as f64 + 1.0);
        scores
            .entry(result.qualified_name.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, result));
    }

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
        "hybrid search complete"
    );

    Ok(results)
}

/// BM25 keyword search via LadybugDB FTS indexes.
async fn bm25_search(
    db: &SharedCodeGraphDb,
    sanitized_query: &str,
    limit: usize,
) -> Result<Vec<GraphSearchResult>> {
    let fts_ready = db.load_fts_extension().await?;
    if !fts_ready {
        return Ok(Vec::new());
    }

    let mut results: Vec<GraphSearchResult> = Vec::new();

    for &(label, index_name) in SEARCHABLE_LABELS {
        let escaped = sanitized_query.replace('\\', "\\\\").replace('\'', "\\'");
        let cypher = format!(
            "CALL QUERY_FTS_INDEX('{label}', '{index_name}', '{escaped}', conjunctive := false) \
             RETURN node.qualified_name, node.name, node.source_file, score \
             ORDER BY score DESC \
             LIMIT {limit}"
        );

        let rows = match db.query(&cypher).await {
            Ok(r) => r,
            Err(_) => continue,
        };

        for row in &rows {
            if let (
                Some(lbug::Value::String(q)),
                Some(lbug::Value::String(n)),
                sf,
                Some(score_val),
            ) = (row.first(), row.get(1), row.get(2), row.get(3))
            {
                let sf_str = match sf {
                    Some(lbug::Value::String(s)) if !s.is_empty() => Some(s.clone()),
                    _ => None,
                };
                let score_f64 = match score_val {
                    lbug::Value::Double(d) => *d,
                    lbug::Value::Float(f) => *f as f64,
                    lbug::Value::Int64(i) => *i as f64,
                    lbug::Value::Int32(i) => *i as f64,
                    _ => 0.0,
                };

                let parsed_label = serde_json::from_value::<NodeLabel>(
                    serde_json::Value::String(label.to_lowercase()),
                )
                .unwrap_or(NodeLabel::Function);

                results.push(GraphSearchResult {
                    node_id: 0,
                    qualified_name: q.clone(),
                    name: n.clone(),
                    label: parsed_label,
                    source_file: sf_str,
                    line_start: None,
                    score: score_f64.abs(),
                    community: None,
                    snippet: None,
                });
            }
        }
    }

    Ok(results)
}

/// Semantic vector search via LadybugDB HNSW index on CodeEmbedding.
async fn semantic_search(
    project_id: &str,
    db: &SharedCodeGraphDb,
    query: &str,
    limit: usize,
) -> Result<Vec<GraphSearchResult>> {
    let vector_ready = db.load_vector_extension().await?;
    if !vector_ready {
        return Ok(Vec::new());
    }

    let pid = project_id.replace('\\', "\\\\").replace('\'', "\\'");

    // Embed the query.
    let query_text = query.to_string();
    let query_vec = tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
        let model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
            .context("fastembed model init")?;
        let embeddings = model.embed(vec![query_text], None)
            .context("fastembed embed")?;
        Ok(embeddings.into_iter().next().unwrap_or_default())
    })
    .await
    .context("semantic embed task panicked")??;

    if query_vec.is_empty() {
        return Ok(Vec::new());
    }

    let vec_str = query_vec
        .iter()
        .map(|v| format!("{v:.6}"))
        .collect::<Vec<_>>()
        .join(",");

    let cypher = format!(
        "CALL QUERY_VECTOR_INDEX('CodeEmbedding', 'code_embedding_idx', \
         CAST([{vec_str}] AS FLOAT[{dim}]), {limit}) \
         YIELD node AS emb, distance \
         WHERE distance < 0.5 \
         RETURN emb.nodeId AS nodeId, distance \
         ORDER BY distance \
         LIMIT {limit}",
        dim = query_vec.len(),
    );

    let rows = match db.query(&cypher).await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(err = %e, "vector search query failed — embeddings may not exist");
            return Ok(Vec::new());
        }
    };

    // Resolve node metadata from the graph by nodeId (qualified_name).
    let mut results: Vec<GraphSearchResult> = Vec::new();
    for row in &rows {
        let (node_id, distance) = match (row.first(), row.get(1)) {
            (Some(lbug::Value::String(id)), Some(dist_val)) => {
                let d = match dist_val {
                    lbug::Value::Double(d) => *d,
                    lbug::Value::Float(f) => *f as f64,
                    _ => 0.5,
                };
                (id.clone(), d)
            }
            _ => continue,
        };

        // Look up the node across symbol tables.
        let escaped_id = node_id.replace('\\', "\\\\").replace('\'', "\\'");
        for &label in &["Function", "Method", "Class", "Interface", "Struct", "Trait", "File", "Enum", "Module", "Route"] {
            let meta = db.query(&format!(
                "MATCH (n:{label}) WHERE n.qualified_name = '{escaped_id}' \
                 AND n.project_id = '{pid}' \
                 RETURN n.name, n.source_file"
            )).await;

            if let Ok(meta_rows) = meta
                && let Some(meta_row) = meta_rows.first()
            {
                let name = match meta_row.first() {
                    Some(lbug::Value::String(s)) => s.clone(),
                    _ => continue,
                };
                let sf = match meta_row.get(1) {
                    Some(lbug::Value::String(s)) if !s.is_empty() => Some(s.clone()),
                    _ => None,
                };
                let parsed_label = serde_json::from_value::<NodeLabel>(
                    serde_json::Value::String(label.to_lowercase()),
                )
                .unwrap_or(NodeLabel::Function);

                results.push(GraphSearchResult {
                    node_id: 0,
                    qualified_name: node_id.clone(),
                    name,
                    label: parsed_label,
                    source_file: sf,
                    line_start: None,
                    score: 1.0 - distance,
                    community: None,
                    snippet: None,
                });
                break;
            }
        }
    }

    Ok(results)
}

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
            clean
        })
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        return String::new();
    }

    tokens.join(" ")
}
