//! Semantic-layer infrastructure: global symbol lookup and declared
//! type environment.
//!
//! The modules here (`symbol_table` and `type_env`) are the foundation
//! the Phase 4 call-resolution upgrade will consume. They live in the
//! same tree as the pipeline phases but are **query-only** — they read
//! from a post-parsing graph and never mutate it.
//!
//! - [`symbol_table::SymbolTable`] builds a lookup index over every
//!   Function / Method / Class / Interface / Struct / Trait / Enum /
//!   Const declared in the graph, with exact, case-insensitive, and
//!   snake ↔ camel variant matching.
//! - [`type_env::TypeEnv`] captures declared types on Variable /
//!   Property nodes so receiver-type inference can walk
//!   `this.foo.bar` through the chain.
//!
//! Both are deliberately narrow today: each replaces one inline data
//! structure the calls phase currently rebuilds at every resolution
//! pass. Broader scope-stack locals, control-flow inference, and
//! cross-language plumbing are Phase 4 concerns.

pub mod frameworks;
pub mod markdown_links;
pub mod member_rules;
pub mod named_bindings;
pub mod resolver;
pub mod response_shapes;
pub mod route_extractors;
pub mod symbol_table;
pub mod type_env;

/// Node label categories the semantic layer cares about. Keeps lookup
/// functions polymorphic without leaking Cypher strings to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Struct,
    Trait,
    Enum,
    TypeAlias,
    Const,
    Variable,
    Property,
}

impl SymbolKind {
    /// The Cypher label string used for this kind in the graph.
    pub fn label(self) -> &'static str {
        match self {
            Self::Function => "Function",
            Self::Method => "Method",
            Self::Class => "Class",
            Self::Interface => "Interface",
            Self::Struct => "Struct",
            Self::Trait => "Trait",
            Self::Enum => "Enum",
            Self::TypeAlias => "TypeAlias",
            Self::Const => "Const",
            Self::Variable => "Variable",
            Self::Property => "Property",
        }
    }
}
