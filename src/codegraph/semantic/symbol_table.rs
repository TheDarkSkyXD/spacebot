//! Global symbol lookup index.
//!
//! Replaces the ad-hoc `HashMap<String, Vec<SymbolEntry>>` that
//! `pipeline/calls.rs` currently builds inline on every run. Exposing
//! it as a named module lets Phase 4's resolver pick up all three
//! lookup flavors (exact qname, simple name within a file, snake ↔
//! camel variants) without touching the resolver's tier logic.

use std::collections::HashMap;

use anyhow::Result;

use super::SymbolKind;
use crate::codegraph::db::CodeGraphDb;

/// One entry in the symbol table. Ties a symbol's qualified name to
/// its declaring file and label — the three pieces every tier of the
/// call resolver needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    pub qualified_name: String,
    pub name: String,
    pub source_file: String,
    pub kind: SymbolKind,
}

/// Queryable symbol index. All lookups are O(1) hash probes; loading
/// cost is one Cypher query per label. Built once per pipeline run
/// and read-only thereafter.
#[derive(Debug, Default, Clone)]
pub struct SymbolTable {
    /// Exact qualified-name → entry. Primary lookup path.
    by_qname: HashMap<String, SymbolEntry>,
    /// Simple name → every matching entry. Used for cross-file
    /// resolution when the qname isn't known.
    by_simple_name: HashMap<String, Vec<SymbolEntry>>,
    /// `(source_file, simple_name)` → entry. Used for tier-1
    /// same-file resolution. An entry also lives in `by_qname` and
    /// `by_simple_name` — this is a denormalized index, not a
    /// separate data set.
    by_file_name: HashMap<(String, String), SymbolEntry>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct symbols (by qname). Diagnostic.
    pub fn len(&self) -> usize {
        self.by_qname.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_qname.is_empty()
    }

    /// Insert a symbol into all three indexes. Duplicate qnames are
    /// dropped — whichever registration arrives first wins so the
    /// resulting index is deterministic across load orders.
    pub fn insert(&mut self, entry: SymbolEntry) {
        if self.by_qname.contains_key(&entry.qualified_name) {
            return;
        }
        let name = entry.name.clone();
        let file = entry.source_file.clone();
        let qname = entry.qualified_name.clone();
        self.by_qname.insert(qname.clone(), entry.clone());
        self.by_simple_name
            .entry(name.clone())
            .or_default()
            .push(entry.clone());
        self.by_file_name.insert((file, name), entry);
        // Register snake ↔ camel variants under `by_simple_name` only
        // — they never match exact qnames. Keeps the variant noise
        // out of `by_qname` while still allowing fuzzy lookup.
        for variant in name_variants(&self.by_qname[&qname].name) {
            if variant == self.by_qname[&qname].name {
                continue;
            }
            self.by_simple_name
                .entry(variant)
                .or_default()
                .push(self.by_qname[&qname].clone());
        }
    }

    /// Exact-qname lookup. Tier 1 / tier 2 resolvers use this once
    /// they've narrowed down to a specific target.
    pub fn find_exact(&self, qname: &str) -> Option<&SymbolEntry> {
        self.by_qname.get(qname)
    }

    /// Return all symbols sharing the simple name. `Vec` length ≥ 2
    /// means the resolver must fall back to a lower-confidence tier.
    pub fn find_by_simple_name(&self, name: &str) -> &[SymbolEntry] {
        static EMPTY: Vec<SymbolEntry> = Vec::new();
        self.by_simple_name
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or(EMPTY.as_slice())
    }

    /// Same-file lookup — the tier-1 path in the call resolver.
    pub fn find_in_file(&self, source_file: &str, name: &str) -> Option<&SymbolEntry> {
        self.by_file_name
            .get(&(source_file.to_string(), name.to_string()))
    }

    /// Populate a SymbolTable from every resolvable label in the
    /// project's graph. Runs one query per label — the labels are
    /// small enough that a UNION would not meaningfully speed this up.
    pub async fn load(db: &CodeGraphDb, project_id: &str) -> Result<Self> {
        let mut table = Self::new();
        let pid = cypher_escape(project_id);

        for kind in [
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Class,
            SymbolKind::Interface,
            SymbolKind::Struct,
            SymbolKind::Trait,
            SymbolKind::Enum,
            SymbolKind::TypeAlias,
            SymbolKind::Const,
        ] {
            let label = kind.label();
            let rows = db
                .query(&format!(
                    "MATCH (n:{label}) WHERE n.project_id = '{pid}' \
                     RETURN n.qualified_name, n.name, n.source_file"
                ))
                .await?;
            for row in &rows {
                if let (
                    Some(lbug::Value::String(qname)),
                    Some(lbug::Value::String(name)),
                    Some(lbug::Value::String(file)),
                ) = (row.first(), row.get(1), row.get(2))
                {
                    table.insert(SymbolEntry {
                        qualified_name: qname.clone(),
                        name: name.clone(),
                        source_file: file.clone(),
                        kind,
                    });
                }
            }
        }

        tracing::debug!(
            project_id = %project_id,
            entries = table.len(),
            "loaded symbol table"
        );
        Ok(table)
    }
}

/// Produce snake ↔ camel variants of a simple name so call resolution
/// can match `get_user` against `getUser`. Returns an empty `Vec`
/// when `name` has no separator to flip.
fn name_variants(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if name.contains('_') {
        out.push(snake_to_camel(name));
    }
    if has_upper_after_lower(name) {
        out.push(camel_to_snake(name));
    }
    out
}

fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    for c in s.chars() {
        if c == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

fn has_upper_after_lower(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    chars
        .windows(2)
        .any(|w| w[0].is_ascii_lowercase() && w[1].is_ascii_uppercase())
}

fn cypher_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(qname: &str, name: &str, file: &str, kind: SymbolKind) -> SymbolEntry {
        SymbolEntry {
            qualified_name: qname.to_string(),
            name: name.to_string(),
            source_file: file.to_string(),
            kind,
        }
    }

    #[test]
    fn insert_and_find_exact() {
        let mut t = SymbolTable::new();
        t.insert(entry("crate::foo", "foo", "lib.rs", SymbolKind::Function));
        let got = t.find_exact("crate::foo").unwrap();
        assert_eq!(got.name, "foo");
        assert_eq!(got.source_file, "lib.rs");
    }

    #[test]
    fn duplicate_qnames_ignored() {
        let mut t = SymbolTable::new();
        t.insert(entry("crate::foo", "foo", "a.rs", SymbolKind::Function));
        t.insert(entry("crate::foo", "foo", "b.rs", SymbolKind::Function));
        assert_eq!(t.len(), 1);
        assert_eq!(t.find_exact("crate::foo").unwrap().source_file, "a.rs");
    }

    #[test]
    fn find_by_simple_name_returns_all_matches() {
        let mut t = SymbolTable::new();
        t.insert(entry("a::foo", "foo", "a.rs", SymbolKind::Function));
        t.insert(entry("b::foo", "foo", "b.rs", SymbolKind::Function));
        let hits = t.find_by_simple_name("foo");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn find_in_file_scopes_to_file() {
        let mut t = SymbolTable::new();
        t.insert(entry("a::foo", "foo", "a.rs", SymbolKind::Function));
        t.insert(entry("b::foo", "foo", "b.rs", SymbolKind::Function));
        assert_eq!(t.find_in_file("a.rs", "foo").unwrap().qualified_name, "a::foo");
        assert_eq!(t.find_in_file("b.rs", "foo").unwrap().qualified_name, "b::foo");
        assert!(t.find_in_file("c.rs", "foo").is_none());
    }

    #[test]
    fn snake_name_also_registered_under_camel_variant() {
        let mut t = SymbolTable::new();
        t.insert(entry(
            "crate::get_user",
            "get_user",
            "lib.rs",
            SymbolKind::Function,
        ));
        let hits = t.find_by_simple_name("getUser");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].qualified_name, "crate::get_user");
    }

    #[test]
    fn camel_name_also_registered_under_snake_variant() {
        let mut t = SymbolTable::new();
        t.insert(entry("App::getUser", "getUser", "app.ts", SymbolKind::Function));
        let hits = t.find_by_simple_name("get_user");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].qualified_name, "App::getUser");
    }

    #[test]
    fn snake_to_camel_basic() {
        assert_eq!(snake_to_camel("get_user_by_id"), "getUserById");
        assert_eq!(snake_to_camel("foo"), "foo");
    }

    #[test]
    fn camel_to_snake_basic() {
        assert_eq!(camel_to_snake("getUserById"), "get_user_by_id");
        assert_eq!(camel_to_snake("foo"), "foo");
    }
}
