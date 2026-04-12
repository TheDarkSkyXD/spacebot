//! Deterministic code-symbol embeddings pass.
//!
//! Reserved as the wiring point for any future opt-in vector embeddings
//! work. Currently a no-op because spacebot's codegraph indexing is
//! fully deterministic — no model is invoked at index time, and
//! codegraph persistence is LadybugDB-only.
//!
//! The function exists so the pipeline orchestrator has a stable hook
//! to extend later without reshuffling phase order.

use std::path::Path;

use anyhow::Result;

use crate::codegraph::db::SharedCodeGraphDb;

/// Result summary returned to the caller.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmbeddingStats {
    pub embedded: u64,
    pub skipped: u64,
}

/// Run the deterministic embeddings pass for a project.
///
/// No-op today — see module docs.
pub async fn generate_embeddings(
    _project_id: &str,
    _root_path: &Path,
    _db: &SharedCodeGraphDb,
) -> Result<EmbeddingStats> {
    Ok(EmbeddingStats::default())
}
