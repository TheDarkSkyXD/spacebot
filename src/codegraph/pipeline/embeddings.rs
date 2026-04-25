//! Code-symbol embeddings via fastembed + LadybugDB vector storage.
//!
//! Generates 384-dim vectors (all-MiniLM-L6-v2) for code symbols and
//! stores them in a LadybugDB `CodeEmbedding` node table with an HNSW
//! index for cosine similarity search. Matches GitNexus's architecture.

use std::sync::Arc;

use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use crate::codegraph::db::SharedCodeGraphDb;

const EMBEDDING_NODE_LIMIT: usize = 50_000;

const EMBEDDABLE_LABELS: &[&str] = &[
    "Function", "Method", "Class", "Interface", "Struct", "Trait", "File",
    "Enum", "TypeAlias", "Module", "Route", "Constructor", "UnionType",
];

#[derive(Debug, Default, Clone, Copy)]
pub struct EmbeddingStats {
    pub embedded: u64,
    pub skipped: u64,
}

pub async fn generate_embeddings(
    project_id: &str,
    _root_path: &std::path::Path,
    db: &SharedCodeGraphDb,
) -> Result<EmbeddingStats> {
    let mut stats = EmbeddingStats::default();
    let pid = project_id.replace('\\', "\\\\").replace('\'', "\\'");

    let vector_ready = db.load_vector_extension().await?;
    if !vector_ready {
        tracing::warn!(project_id = %project_id, "vector extension not available — skipping embeddings");
        return Ok(stats);
    }

    // Clear existing embeddings for this project.
    let _ = db.execute(&format!(
        "MATCH (e:CodeEmbedding) WHERE e.nodeId STARTS WITH '{pid}' DELETE e"
    )).await;

    // Collect nodes to embed.
    let mut texts: Vec<(String, String)> = Vec::new();
    for &label in EMBEDDABLE_LABELS {
        let rows = db.query(&format!(
            "MATCH (n:{label}) WHERE n.project_id = '{pid}' \
             RETURN n.qualified_name, n.name, n.source_file"
        )).await?;

        for row in &rows {
            if let (
                Some(lbug::Value::String(qname)),
                Some(lbug::Value::String(name)),
                sf,
            ) = (row.first(), row.get(1), row.get(2))
            {
                let source_file = match sf {
                    Some(lbug::Value::String(s)) => s.as_str(),
                    _ => "",
                };
                let text = format!("{label} {name} in {source_file}");
                texts.push((qname.clone(), text));
            }
        }
    }

    if texts.is_empty() {
        tracing::info!(project_id = %project_id, "no embeddable nodes found");
        return Ok(stats);
    }

    if texts.len() > EMBEDDING_NODE_LIMIT {
        tracing::warn!(
            project_id = %project_id,
            nodes = texts.len(),
            limit = EMBEDDING_NODE_LIMIT,
            "too many nodes for embedding — skipping"
        );
        stats.skipped = texts.len() as u64;
        return Ok(stats);
    }

    tracing::info!(
        project_id = %project_id,
        nodes = texts.len(),
        "generating embeddings"
    );

    // Initialize fastembed model (all-MiniLM-L6-v2, 384 dims).
    let model = Arc::new(
        tokio::task::spawn_blocking(|| {
            TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
        })
        .await
        .context("fastembed init task panicked")?
        .context("fastembed model initialization failed")?,
    );

    // Batch embed in chunks of 256.
    const BATCH_SIZE: usize = 256;
    let mut insert_stmts: Vec<String> = Vec::new();

    for chunk in texts.chunks(BATCH_SIZE) {
        let batch_texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
        let batch_qnames: Vec<String> = chunk.iter().map(|(q, _)| q.clone()).collect();

        let embeddings = {
            let m = Arc::clone(&model);
            tokio::task::spawn_blocking(move || m.embed(batch_texts, None))
                .await
                .context("embedding task panicked")?
                .context("embedding batch failed")?
        };

        for (qname, embedding) in batch_qnames.iter().zip(embeddings.iter()) {
            let vec_str = embedding
                .iter()
                .map(|v| format!("{v:.6}"))
                .collect::<Vec<_>>()
                .join(",");
            let node_id = qname.replace('\\', "\\\\").replace('\'', "\\'");
            insert_stmts.push(format!(
                "CREATE (:CodeEmbedding {{nodeId: '{node_id}', \
                 embedding: CAST([{vec_str}] AS FLOAT[384])}})"
            ));
            stats.embedded += 1;
        }
    }

    // Batch insert embeddings.
    if !insert_stmts.is_empty() {
        for chunk in insert_stmts.chunks(50) {
            let batch = db.execute_batch(chunk.to_vec()).await?;
            if batch.errors > 0 {
                tracing::debug!(errors = batch.errors, "some embedding inserts failed");
            }
        }
    }

    // Create HNSW vector index for cosine similarity search.
    let _ = db.execute(
        "CALL CREATE_VECTOR_INDEX('CodeEmbedding', 'code_embedding_idx', 'embedding', metric := 'cosine')"
    ).await;

    tracing::info!(
        project_id = %project_id,
        embedded = stats.embedded,
        "embeddings generated and indexed"
    );

    Ok(stats)
}
