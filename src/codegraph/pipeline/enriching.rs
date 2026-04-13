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

use anyhow::Result;

use crate::codegraph::db::SharedCodeGraphDb;

/// Run the deterministic enrichment pass for a project.
///
/// No-op today — see module docs.
pub async fn enrich(_project_id: &str, _db: &SharedCodeGraphDb) -> Result<()> {
    Ok(())
}
