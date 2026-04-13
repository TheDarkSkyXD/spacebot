//! Code-symbol embeddings table — LadybugDB-only stub.
//!
//! Spacebot's codegraph storage lives entirely in LadybugDB. The struct
//! here survives as an empty stub so the module path stays stable for
//! any future opt-in vector work that would target LadybugDB's VECTOR
//! extension directly via Cypher.
//!
//! All methods are no-ops. Nothing is read, nothing is written, nothing
//! is opened on disk.

use std::path::Path;

use anyhow::Result;

/// Stub wrapper for the per-project code-symbol embeddings table.
///
/// No internal state — kept as a unit-like type so callers can hold a
/// handle without allocating any backing storage.
pub struct CodeEmbeddingTable;

impl CodeEmbeddingTable {
    /// Open or create the embeddings handle for a project.
    ///
    /// No I/O is performed. The `_project_dir` argument is preserved so
    /// the signature stays stable for any future LadybugDB-backed
    /// implementation.
    pub async fn open(_project_dir: &Path) -> Result<Self> {
        Ok(Self)
    }

    /// Insert a batch of symbol embeddings.
    ///
    /// No-op stub.
    pub async fn store_batch(&self, _rows: &[CodeEmbeddingRow]) -> Result<()> {
        Ok(())
    }

    /// Delete all rows whose `source_file` matches one of the given paths.
    ///
    /// No-op stub.
    pub async fn delete_by_source_files(&self, _files: &[String]) -> Result<()> {
        Ok(())
    }

    /// Nearest-neighbour vector search.
    ///
    /// Returns an empty result set — codegraph no longer maintains an
    /// embeddings table, so vector search has nothing to match against.
    pub async fn vector_search(
        &self,
        _query: &[f32],
        _limit: usize,
    ) -> Result<Vec<(String, String, String, f32)>> {
        Ok(Vec::new())
    }
}

/// A single row that would be inserted into the embeddings table.
///
/// Kept so any caller building rows for a future LadybugDB vector index
/// has a stable shape to populate.
#[derive(Debug, Clone)]
pub struct CodeEmbeddingRow {
    pub qualified_name: String,
    pub source_file: String,
    pub snippet: String,
    pub embedding: Vec<f32>,
}
