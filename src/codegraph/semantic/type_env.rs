//! Declared type environment.
//!
//! Captures the subset of type information we can recover by reading
//! declared types directly from the graph: class fields, module-level
//! variables, parameters (while they exist in the pipeline). Inferred
//! types — expression results, generic parameter narrowing, control-
//! flow refinement — are out of scope here and belong to the Phase 4
//! upgrade.
//!
//! Today's consumer is the call resolver in `pipeline/calls.rs`, which
//! currently rebuilds an equivalent `HashMap<(class, field), type>`
//! inline on every run. This module is the replacement; wiring flips
//! in Phase 4 so the refactor cost doesn't spread across two phases.

use std::collections::HashMap;

use anyhow::Result;

use crate::codegraph::db::CodeGraphDb;

/// A queryable view of declared types in a project's graph.
///
/// Keyed primarily by `(owner_qname, member_name)`. The owner is the
/// class/struct/module whose member we're looking up; `None` owner
/// represents a top-level binding (module-scope variable, free
/// function). Call-site resolution uses this to answer questions like
/// "what type is `user.profile`?" by walking `user`'s declared type
/// and looking up `profile` as a member.
#[derive(Debug, Default, Clone)]
pub struct TypeEnv {
    /// `(Some(owner_qname), member_name) → declared_type_string`.
    /// Owner-less entries use `(None, qname)` for module-scope
    /// variables — still keyed for fast lookup without walking every
    /// pair.
    entries: HashMap<(Option<String>, String), String>,
}

impl TypeEnv {
    /// Create an empty environment. Test fixtures and the incremental
    /// path build small envs directly; production code uses
    /// [`Self::load`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a declared type for a member of a named container.
    /// `declared_type` is stored as-is — callers normalize it (strip
    /// generics, unwrap `Optional<T>`, etc.) before lookup if they
    /// need to.
    pub fn insert_member(&mut self, owner_qname: &str, member_name: &str, declared_type: &str) {
        if declared_type.is_empty() {
            return;
        }
        self.entries.insert(
            (Some(owner_qname.to_string()), member_name.to_string()),
            declared_type.to_string(),
        );
    }

    /// Record a declared type for a module-level (owner-less) binding.
    pub fn insert_module(&mut self, qname: &str, declared_type: &str) {
        if declared_type.is_empty() {
            return;
        }
        self.entries.insert(
            (None, qname.to_string()),
            declared_type.to_string(),
        );
    }

    /// Look up `owner.member`. Returns the recorded declared type
    /// string, if any.
    pub fn member_type(&self, owner_qname: &str, member_name: &str) -> Option<&str> {
        self.entries
            .get(&(Some(owner_qname.to_string()), member_name.to_string()))
            .map(String::as_str)
    }

    /// Look up a module-scope binding.
    pub fn module_type(&self, qname: &str) -> Option<&str> {
        self.entries
            .get(&(None, qname.to_string()))
            .map(String::as_str)
    }

    /// Total recorded entries — diagnostic / metrics.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Build a TypeEnv by scanning every Property and Variable node in
    /// the graph for its `declared_type`. Property owners come from
    /// the `HAS_PROPERTY` edge's source; module-scope Variables are
    /// keyed by their own qname. Unparseable rows are logged and
    /// skipped — a bad row is always preferable to an empty env on a
    /// production re-index.
    pub async fn load(db: &CodeGraphDb, project_id: &str) -> Result<Self> {
        let mut env = Self::new();
        let pid = cypher_escape(project_id);

        // Properties: `(owner)-[:HAS_PROPERTY]->(prop)`. We want
        // owner.qname, prop.name, prop.declared_type. Any label can own
        // a property, so we project `labels(owner)[0]` isn't available
        // in all Cypher dialects — we match on any source and rely on
        // the edge to narrow.
        let rows = db
            .query(&format!(
                "MATCH (owner)-[r:CodeRelation]->(prop:Property) \
                 WHERE r.type = 'HAS_PROPERTY' \
                   AND owner.project_id = '{pid}' \
                   AND prop.project_id = '{pid}' \
                 RETURN owner.qualified_name, prop.name, prop.declared_type"
            ))
            .await?;
        for row in &rows {
            if let (
                Some(lbug::Value::String(owner)),
                Some(lbug::Value::String(name)),
                Some(lbug::Value::String(declared)),
            ) = (row.first(), row.get(1), row.get(2))
            {
                env.insert_member(owner, name, declared);
            }
        }

        // Module-scope variables. Property owner inference would
        // misattribute these — a top-level `const FOO: Bar = ...` has
        // no enclosing class. We key by the variable's own qname so
        // `module_type("crate::FOO")` returns `"Bar"`.
        let rows = db
            .query(&format!(
                "MATCH (v:Variable) WHERE v.project_id = '{pid}' \
                 RETURN v.qualified_name, v.declared_type"
            ))
            .await?;
        for row in &rows {
            if let (
                Some(lbug::Value::String(qname)),
                Some(lbug::Value::String(declared)),
            ) = (row.first(), row.get(1))
            {
                env.insert_module(qname, declared);
            }
        }

        tracing::debug!(
            project_id = %project_id,
            entries = env.len(),
            "loaded type env"
        );
        Ok(env)
    }
}

/// Escape a string for use in a Cypher string literal. Kept private to
/// the semantic layer because every module rolls its own identical
/// helper and pulling in a shared one would require reorganizing
/// `db.rs` — out of scope for Phase 1.
fn cypher_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_lookup_member() {
        let mut env = TypeEnv::new();
        env.insert_member("app::User", "email", "String");
        assert_eq!(env.member_type("app::User", "email"), Some("String"));
        assert_eq!(env.member_type("app::User", "name"), None);
        assert_eq!(env.member_type("other::User", "email"), None);
    }

    #[test]
    fn insert_and_lookup_module() {
        let mut env = TypeEnv::new();
        env.insert_module("app::DEFAULT_LIMIT", "u32");
        assert_eq!(env.module_type("app::DEFAULT_LIMIT"), Some("u32"));
    }

    #[test]
    fn module_and_member_live_in_different_namespaces() {
        let mut env = TypeEnv::new();
        env.insert_module("app::foo", "T1");
        env.insert_member("app", "foo", "T2");
        assert_eq!(env.module_type("app::foo"), Some("T1"));
        assert_eq!(env.member_type("app", "foo"), Some("T2"));
    }

    #[test]
    fn empty_declared_type_is_ignored() {
        let mut env = TypeEnv::new();
        env.insert_member("app::User", "email", "");
        assert!(env.is_empty());
    }

    #[test]
    fn len_tracks_insertions() {
        let mut env = TypeEnv::new();
        assert_eq!(env.len(), 0);
        env.insert_member("A", "x", "i32");
        env.insert_module("B", "String");
        assert_eq!(env.len(), 2);
    }
}
