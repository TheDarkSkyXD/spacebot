//! Named-binding / dependency-injection extraction.
//!
//! In large web codebases, a lot of wiring happens through DI
//! containers and decorator registration rather than direct
//! constructor calls. `@Service` / `@Autowired` (Spring),
//! `@Injectable` (Angular / NestJS), `Route::get('Cls@action')`
//! (Laravel), `services.AddSingleton<IFoo, Foo>()` (ASP.NET), etc.
//! Without picking those patterns up, the call graph misses a
//! significant fraction of the real cross-file dependencies.
//!
//! Each submodule here recognises one language's DI dialects and
//! emits [`NamedBinding`] records that describe a logical
//! `consumer → provider` tie. The extractors are deliberately
//! regex-based: DI declarations are short, lexically distinctive,
//! and rarely span multiple lines; a full AST walk would be
//! overkill. When a language has multiple DI ecosystems (Python's
//! `Depends(...)` vs `dependency_injector`, Java's Spring vs
//! Guice), each is covered by one pattern inside the same
//! submodule.

pub mod csharp;
pub mod java_kotlin;
pub mod php;
pub mod python;
pub mod rust;
pub mod typescript;

/// Lifecycle kind attached to a binding.
///
/// Ordered alphabetically so downstream dedup + set operations are
/// stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// Built once per request / operation.
    Scoped,
    /// Built once and shared across the container's lifetime.
    Singleton,
    /// Built anew for every injection site.
    Transient,
    /// Resolved through a factory function.
    Factory,
    /// Unclassified — we know it's a DI binding but not the lifetime.
    Unknown,
}

impl BindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scoped => "scoped",
            Self::Singleton => "singleton",
            Self::Transient => "transient",
            Self::Factory => "factory",
            Self::Unknown => "unknown",
        }
    }
}

/// One detected `consumer → provider` binding.
///
/// Emitted by per-language extractors and consumed by the pipeline
/// to create USES edges with the lifecycle stored on `binding_kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedBinding {
    /// The identifier / namespace receiving the injection (the
    /// consumer). Usually a class or function name.
    pub consumer: String,
    /// The identifier of the provider being injected (the dependency).
    /// For interface-based DI this is the interface name; for
    /// concrete registrations it's the implementation class.
    pub provider: String,
    /// Lifecycle hint (singleton, transient, etc.).
    pub kind: BindingKind,
    /// 1-based source line number. Diagnostics only.
    pub line: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_kind_round_trip() {
        for (k, s) in [
            (BindingKind::Singleton, "singleton"),
            (BindingKind::Transient, "transient"),
            (BindingKind::Scoped, "scoped"),
            (BindingKind::Factory, "factory"),
            (BindingKind::Unknown, "unknown"),
        ] {
            assert_eq!(k.as_str(), s);
        }
    }
}
