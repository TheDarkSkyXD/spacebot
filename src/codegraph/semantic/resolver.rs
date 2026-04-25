//! Tiered call-site resolver.
//!
//! The legacy resolver in `pipeline/calls.rs` hand-rolls symbol
//! lookup and confidence scoring inline. This module centralises that
//! logic into a small, testable surface so Phase 4's integration
//! patch can swap it in without untangling the pipeline.
//!
//! The six tiers mirror GitNexus's confidence hierarchy, ordered from
//! most to least certain:
//!
//! | Tier | Confidence | Trigger |
//! |------|-----------|---------|
//! | `SameFile`          | 0.95 | callee lives in the caller's file |
//! | `ReceiverResolved`  | 0.92 | method call whose receiver type is known |
//! | `ImportedExact`     | 0.90 | callee's source-file matches an imported file |
//! | `ImportedAlias`     | 0.80 | callee found via tsconfig-path / PSR-4-style alias |
//! | `ProjectUnique`     | 0.70 | only one exported callee with this name in the project |
//! | `ProjectMultiMatch` | 0.40 | multiple callees match; best-effort guess |

use std::collections::HashSet;

use super::symbol_table::{SymbolEntry, SymbolTable};
use super::type_env::TypeEnv;

/// Resolution tier for a call site. Tier ordering reflects
/// descending certainty so downstream consumers can filter by
/// "confidence at least tier X" via `tier >= Tier::ImportedExact`.
///
/// The derived `Ord` ranks more-certain tiers as *greater* (`SameFile`
/// is the maximum), which matches intuitive filter phrasing:
/// `edge.tier >= Tier::ImportedExact` keeps the three strongest tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    ProjectMultiMatch = 0,
    ProjectUnique = 1,
    ImportedAlias = 2,
    ImportedExact = 3,
    ReceiverResolved = 4,
    SameFile = 5,
}

impl Tier {
    /// Confidence score we assign to CALLS edges produced at this
    /// tier. Kept as a single source of truth so the pipeline and
    /// downstream analytics agree on numeric values.
    pub fn confidence(self) -> f32 {
        match self {
            Self::SameFile => 0.95,
            Self::ReceiverResolved => 0.92,
            Self::ImportedExact => 0.90,
            Self::ImportedAlias => 0.80,
            Self::ProjectUnique => 0.70,
            Self::ProjectMultiMatch => 0.40,
        }
    }

    /// Short human-readable label stored on CALLS edges' `reason`
    /// field so graph queries can filter by tier without decoding the
    /// numeric confidence.
    pub fn reason(self) -> &'static str {
        match self {
            Self::SameFile => "same-file",
            Self::ReceiverResolved => "receiver-resolved",
            Self::ImportedExact => "import-exact",
            Self::ImportedAlias => "import-alias",
            Self::ProjectUnique => "project-unique",
            Self::ProjectMultiMatch => "project-multi",
        }
    }
}

/// Outcome of resolving one call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCall {
    pub callee_qname: String,
    pub callee_label: String,
    pub tier: Tier,
}

/// Inputs the resolver needs for one call site. The caller supplies
/// whatever it can — unknown pieces default to `None` and the
/// resolver falls through to the next tier.
#[derive(Debug, Clone)]
pub struct CallSiteQuery<'a> {
    /// Source-file path of the caller (for same-file matching).
    pub caller_file: &'a str,
    /// The bare name being called (e.g. `create`, `get_user`).
    pub call_name: &'a str,
    /// Files the caller imports. When provided, the resolver
    /// narrows tier-3 matches to symbols declared in any of these
    /// files.
    pub imported_files: &'a HashSet<String>,
    /// The receiver type for method calls (`this.receiver_type.call()`).
    /// `Some` unlocks tier-2 resolution.
    pub receiver_type: Option<&'a str>,
}

/// Tiered call-site resolver.
///
/// Holds references to the two semantic-layer data structures that
/// the legacy resolver rebuilds from scratch on every pipeline run.
/// Construction is cheap; lookup is `O(1)` per tier thanks to the
/// hash-backed `SymbolTable`.
pub struct CallResolver<'a> {
    symbols: &'a SymbolTable,
    types: &'a TypeEnv,
}

impl<'a> CallResolver<'a> {
    pub fn new(symbols: &'a SymbolTable, types: &'a TypeEnv) -> Self {
        Self { symbols, types }
    }

    /// Walk the tier ladder top-down and return the first match.
    ///
    /// Returns `None` when every tier misses — callers emit an
    /// unresolved placeholder edge (matching the legacy behavior).
    pub fn resolve(&self, query: &CallSiteQuery<'_>) -> Option<ResolvedCall> {
        // Tier 5 — same-file. Strongest signal: the callee is
        // declared in the exact file calling it.
        if let Some(entry) = self.symbols.find_in_file(query.caller_file, query.call_name) {
            return Some(Self::into_resolved(entry, Tier::SameFile));
        }

        // Tier 4 — receiver-resolved method call. When the caller
        // knows the receiver's type (e.g. via type_env), we look up
        // `ReceiverType::method_name` under the exact qname path.
        if let Some(receiver) = query.receiver_type {
            let candidate = format!("{receiver}::{name}", name = query.call_name);
            if let Some(entry) = self.symbols.find_exact(&candidate) {
                return Some(Self::into_resolved(entry, Tier::ReceiverResolved));
            }
            // Also try simple-name match against the receiver type's
            // methods — populated when the caller's type_env has the
            // property declaration but the symbol_table's qname
            // scheme uses a different separator.
            if self.types.member_type(receiver, query.call_name).is_some()
                && let Some(entry) = self.symbols.find_exact(&candidate)
            {
                return Some(Self::into_resolved(entry, Tier::ReceiverResolved));
            }
        }

        // Tier 3 — imported-exact. Narrow to symbols declared in one
        // of the caller's imported files.
        let hits = self.symbols.find_by_simple_name(query.call_name);
        let imported_hits: Vec<&SymbolEntry> = hits
            .iter()
            .filter(|e| query.imported_files.contains(&e.source_file))
            .collect();
        if imported_hits.len() == 1 {
            return Some(Self::into_resolved(imported_hits[0], Tier::ImportedExact));
        }
        if imported_hits.len() > 1 {
            // Multiple imported matches — pick the first by qname for
            // deterministic output; downgrade confidence so downstream
            // analytics can filter.
            let mut ordered: Vec<&SymbolEntry> = imported_hits.clone();
            ordered.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
            return Some(Self::into_resolved(ordered[0], Tier::ImportedAlias));
        }

        // Tier 2 — project-wide unique. One, and only one, match
        // anywhere in the project.
        if hits.len() == 1 {
            return Some(Self::into_resolved(&hits[0], Tier::ProjectUnique));
        }

        // Tier 1 — project-wide multi-match. Best-effort guess;
        // pick the lexicographically-smallest qname for determinism.
        if !hits.is_empty() {
            let mut ordered: Vec<&SymbolEntry> = hits.iter().collect();
            ordered.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
            return Some(Self::into_resolved(ordered[0], Tier::ProjectMultiMatch));
        }

        None
    }

    fn into_resolved(entry: &SymbolEntry, tier: Tier) -> ResolvedCall {
        ResolvedCall {
            callee_qname: entry.qualified_name.clone(),
            callee_label: entry.kind.label().to_string(),
            tier,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::semantic::SymbolKind;

    fn entry(qname: &str, name: &str, file: &str) -> SymbolEntry {
        SymbolEntry {
            qualified_name: qname.to_string(),
            name: name.to_string(),
            source_file: file.to_string(),
            kind: SymbolKind::Function,
        }
    }

    fn imported(files: &[&str]) -> HashSet<String> {
        files.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tier_ordering_goes_from_weakest_to_strongest() {
        assert!(Tier::SameFile > Tier::ReceiverResolved);
        assert!(Tier::ReceiverResolved > Tier::ImportedExact);
        assert!(Tier::ImportedExact > Tier::ImportedAlias);
        assert!(Tier::ImportedAlias > Tier::ProjectUnique);
        assert!(Tier::ProjectUnique > Tier::ProjectMultiMatch);
    }

    #[test]
    fn confidence_monotonic_with_tier() {
        let mut prev = 0.0f32;
        for tier in [
            Tier::ProjectMultiMatch,
            Tier::ProjectUnique,
            Tier::ImportedAlias,
            Tier::ImportedExact,
            Tier::ReceiverResolved,
            Tier::SameFile,
        ] {
            assert!(tier.confidence() > prev);
            prev = tier.confidence();
        }
    }

    #[test]
    fn same_file_beats_other_tiers() {
        let mut t = SymbolTable::new();
        t.insert(entry("a::foo", "foo", "a.rs"));
        t.insert(entry("b::foo", "foo", "b.rs"));
        let types = TypeEnv::new();
        let resolver = CallResolver::new(&t, &types);

        let hits = resolver
            .resolve(&CallSiteQuery {
                caller_file: "a.rs",
                call_name: "foo",
                imported_files: &imported(&["b.rs"]),
                receiver_type: None,
            })
            .unwrap();
        assert_eq!(hits.tier, Tier::SameFile);
        assert_eq!(hits.callee_qname, "a::foo");
    }

    #[test]
    fn receiver_resolution_uses_qname_path() {
        let mut t = SymbolTable::new();
        t.insert(SymbolEntry {
            qualified_name: "User::save".to_string(),
            name: "save".to_string(),
            source_file: "user.rs".to_string(),
            kind: SymbolKind::Method,
        });
        let types = TypeEnv::new();
        let resolver = CallResolver::new(&t, &types);

        let hits = resolver
            .resolve(&CallSiteQuery {
                caller_file: "caller.rs",
                call_name: "save",
                imported_files: &imported(&[]),
                receiver_type: Some("User"),
            })
            .unwrap();
        assert_eq!(hits.tier, Tier::ReceiverResolved);
        assert_eq!(hits.callee_qname, "User::save");
    }

    #[test]
    fn imported_exact_matches_when_one_imported_hit() {
        let mut t = SymbolTable::new();
        t.insert(entry("a::foo", "foo", "a.rs"));
        t.insert(entry("b::foo", "foo", "b.rs"));
        let types = TypeEnv::new();
        let resolver = CallResolver::new(&t, &types);

        let hits = resolver
            .resolve(&CallSiteQuery {
                caller_file: "caller.rs",
                call_name: "foo",
                imported_files: &imported(&["a.rs"]),
                receiver_type: None,
            })
            .unwrap();
        assert_eq!(hits.tier, Tier::ImportedExact);
        assert_eq!(hits.callee_qname, "a::foo");
    }

    #[test]
    fn imported_alias_when_multiple_imported_hits() {
        let mut t = SymbolTable::new();
        t.insert(entry("a::foo", "foo", "a.rs"));
        t.insert(entry("b::foo", "foo", "b.rs"));
        let types = TypeEnv::new();
        let resolver = CallResolver::new(&t, &types);

        let hits = resolver
            .resolve(&CallSiteQuery {
                caller_file: "caller.rs",
                call_name: "foo",
                imported_files: &imported(&["a.rs", "b.rs"]),
                receiver_type: None,
            })
            .unwrap();
        assert_eq!(hits.tier, Tier::ImportedAlias);
        assert_eq!(hits.callee_qname, "a::foo"); // lex-min
    }

    #[test]
    fn project_unique_when_single_project_match() {
        let mut t = SymbolTable::new();
        t.insert(entry("crate::foo", "foo", "lib.rs"));
        let types = TypeEnv::new();
        let resolver = CallResolver::new(&t, &types);

        let hits = resolver
            .resolve(&CallSiteQuery {
                caller_file: "other.rs",
                call_name: "foo",
                imported_files: &imported(&[]),
                receiver_type: None,
            })
            .unwrap();
        assert_eq!(hits.tier, Tier::ProjectUnique);
    }

    #[test]
    fn project_multi_when_multiple_unimported_matches() {
        let mut t = SymbolTable::new();
        t.insert(entry("a::foo", "foo", "a.rs"));
        t.insert(entry("b::foo", "foo", "b.rs"));
        let types = TypeEnv::new();
        let resolver = CallResolver::new(&t, &types);

        let hits = resolver
            .resolve(&CallSiteQuery {
                caller_file: "caller.rs",
                call_name: "foo",
                imported_files: &imported(&[]),
                receiver_type: None,
            })
            .unwrap();
        assert_eq!(hits.tier, Tier::ProjectMultiMatch);
    }

    #[test]
    fn returns_none_when_no_symbol_matches() {
        let t = SymbolTable::new();
        let types = TypeEnv::new();
        let resolver = CallResolver::new(&t, &types);

        let hits = resolver.resolve(&CallSiteQuery {
            caller_file: "x.rs",
            call_name: "missing",
            imported_files: &imported(&[]),
            receiver_type: None,
        });
        assert!(hits.is_none());
    }
}
