//! Full-text search index using LadybugDB's native FTS extension.
//!
//! Creates per-table FTS indexes on node properties (name, qualified_name,
//! source_file) using the porter stemmer. Matches GitNexus's approach of
//! indexing directly in the graph DB rather than a separate SQLite sidecar.

use anyhow::Result;

use super::PhaseResult;
use crate::codegraph::db::SharedCodeGraphDb;

const SEARCHABLE_LABELS: &[&str] = &[
    "Function", "Method", "Class", "Interface", "Struct", "Trait",
    "Enum", "TypeAlias", "Const", "Module", "Route", "Variable",
    "Import", "Decorator", "File", "Constructor", "Property",
    "UnionType", "Typedef", "Static", "Delegate",
];

const FTS_PROPERTIES: &[&str] = &["name", "qualified_name", "source_file"];

fn index_name(label: &str) -> String {
    format!("{}_fts", label.to_lowercase())
}

pub async fn build_fts_index(
    project_id: &str,
    db: &SharedCodeGraphDb,
) -> Result<PhaseResult> {
    let mut result = PhaseResult::default();

    let fts_ready = db.load_fts_extension().await?;
    if !fts_ready {
        tracing::warn!(
            project_id = %project_id,
            "FTS extension not available — skipping index build"
        );
        return Ok(result);
    }

    let prop_list = FTS_PROPERTIES
        .iter()
        .map(|p| format!("'{p}'"))
        .collect::<Vec<_>>()
        .join(", ");

    for label in SEARCHABLE_LABELS {
        let idx = index_name(label);

        // Drop existing index (ignore errors — index may not exist).
        let _ = db
            .execute(&format!("CALL DROP_FTS_INDEX('{label}', '{idx}')"))
            .await;

        if let Err(e) = db
            .execute(&format!(
                "CALL CREATE_FTS_INDEX('{label}', '{idx}', [{prop_list}], stemmer := 'porter')"
            ))
            .await
        {
            tracing::debug!(label, err = %e, "FTS index creation failed (table may be empty)");
            continue;
        }

        result.nodes_created += 1;
    }

    tracing::info!(
        project_id = %project_id,
        indexes = result.nodes_created,
        "LadybugDB FTS indexes built"
    );

    Ok(result)
}
