//! Framework detection.
//!
//! Scans a project's manifests, file layout, and imports to identify
//! which web / application frameworks are in use. The detected set
//! drives two downstream consumers:
//!
//! 1. **Entry-point scoring** in `pipeline/processes.rs` applies a
//!    framework-specific multiplier to functions that look like HTTP
//!    handlers. A Next.js page is a stronger entry-point signal than
//!    a bare `main()`.
//! 2. **Cluster enrichment** in `pipeline/enriching.rs` uses the
//!    dominant framework per community as the Community's label
//!    (e.g. "Django views", "Next.js API routes").
//!
//! Detection is **best-effort** and deliberately loose. False
//! positives produce a slightly wrong multiplier; false negatives
//! reduce entry-point scores but don't break resolution. We lean
//! toward detection by manifest signals + characteristic import
//! patterns because file-layout conventions vary too widely.

use std::collections::HashSet;

use crate::codegraph::config::ConfigContext;

/// One of the frameworks we know how to score.
///
/// Ordered alphabetically so snapshot diffs stay deterministic when
/// new frameworks are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Framework {
    AspNet,
    Django,
    Expo,
    Express,
    FastApi,
    Flask,
    Koa,
    Laravel,
    NextJs,
    Rails,
    Spring,
    Symfony,
}

impl Framework {
    /// Entry-point scoring multiplier. Matches the weights in the
    /// master plan's Phase 5 section — higher = stronger signal that
    /// a matching function is a real entry point.
    pub fn entry_point_multiplier(self) -> f64 {
        match self {
            Self::NextJs => 3.0,
            Self::Django => 3.0,
            Self::Spring => 3.0,
            Self::AspNet => 3.0,
            Self::FastApi => 2.8,
            Self::Rails => 2.8,
            Self::Laravel => 2.8,
            Self::Symfony => 2.8,
            Self::Expo => 2.5,
            Self::Express => 2.5,
            Self::Koa => 2.5,
            Self::Flask => 2.5,
        }
    }

    /// Human-readable community-label fragment used by Phase 6
    /// cluster enrichment.
    pub fn label(self) -> &'static str {
        match self {
            Self::NextJs => "Next.js",
            Self::Django => "Django",
            Self::Spring => "Spring",
            Self::AspNet => "ASP.NET",
            Self::FastApi => "FastAPI",
            Self::Rails => "Rails",
            Self::Laravel => "Laravel",
            Self::Symfony => "Symfony",
            Self::Expo => "Expo",
            Self::Express => "Express",
            Self::Koa => "Koa",
            Self::Flask => "Flask",
        }
    }
}

/// Aggregate of every framework detected in a project.
///
/// Built by [`detect`] and consumed during entry-point scoring +
/// cluster enrichment. `Default` is an empty set — a project that
/// uses none of our recognized frameworks (an internal library, a
/// CLI) gets no multipliers.
#[derive(Debug, Clone, Default)]
pub struct FrameworkContext {
    pub detected: HashSet<Framework>,
}

impl FrameworkContext {
    pub fn has(&self, fw: Framework) -> bool {
        self.detected.contains(&fw)
    }

    pub fn is_empty(&self) -> bool {
        self.detected.is_empty()
    }

    pub fn len(&self) -> usize {
        self.detected.len()
    }

    /// Best multiplier applicable to a function. Picks the highest
    /// recorded multiplier when multiple frameworks match — e.g. a
    /// monorepo containing both Next.js and Express gets 3.0 for
    /// Next.js pages, not 2.5 for the Express fallback.
    pub fn best_multiplier(&self) -> f64 {
        self.detected
            .iter()
            .map(|fw| fw.entry_point_multiplier())
            .fold(1.0, f64::max)
    }
}

/// Detect frameworks from the project's manifest dependencies and
/// workspace-level signals.
///
/// Today this is a manifest-only scan — we look at the declared
/// dependency lists surfaced by the Phase 1 config parsers
/// ([`crate::codegraph::config`]). The richer "also scan imports"
/// pass happens in `pipeline/enriching.rs`, which has access to the
/// full import graph.
pub fn detect(
    config: &ConfigContext,
    package_json_deps: &[&str],
    composer_deps: &[&str],
    python_deps: &[&str],
    gemfile_deps: &[&str],
    maven_coords: &[&str],
) -> FrameworkContext {
    let mut out = FrameworkContext::default();

    for dep in package_json_deps {
        match *dep {
            "next" | "next.js" => {
                out.detected.insert(Framework::NextJs);
            }
            "expo" | "expo-router" => {
                out.detected.insert(Framework::Expo);
            }
            "express" => {
                out.detected.insert(Framework::Express);
            }
            "koa" => {
                out.detected.insert(Framework::Koa);
            }
            _ => {}
        }
    }

    for dep in python_deps {
        match *dep {
            "django" | "Django" => {
                out.detected.insert(Framework::Django);
            }
            "fastapi" | "FastAPI" => {
                out.detected.insert(Framework::FastApi);
            }
            "flask" | "Flask" => {
                out.detected.insert(Framework::Flask);
            }
            _ => {}
        }
    }

    for dep in composer_deps {
        if dep.starts_with("laravel/") {
            out.detected.insert(Framework::Laravel);
        }
        if dep.starts_with("symfony/") {
            out.detected.insert(Framework::Symfony);
        }
    }

    for dep in gemfile_deps {
        if *dep == "rails" {
            out.detected.insert(Framework::Rails);
        }
    }

    for coord in maven_coords {
        // group:artifact form — match common Spring coordinates.
        if coord.contains("org.springframework") {
            out.detected.insert(Framework::Spring);
        }
    }

    // File-layout heuristics driven by per-workspace configs.
    for ws in &config.workspaces {
        // Next.js: presence of an `app/` or `pages/` dir is implied
        // by the TypeScript paths config referencing them. When the
        // JSON-based manifest scan didn't hit, this is the fallback.
        if ws
            .ts_paths
            .iter()
            .any(|p| p.targets.iter().any(|t| t.contains("app/") || t.contains("pages/")))
        {
            out.detected.insert(Framework::NextJs);
        }
        // Dart + Flutter apps register a `lib/main.dart` — we don't
        // label that explicitly yet, but the hook exists for when we
        // add Framework::Flutter.
        let _ = ws.dart_package.as_ref();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::config::{TsPathAlias, WorkspaceConfig};

    #[test]
    fn multiplier_ranges_match_plan() {
        assert_eq!(Framework::NextJs.entry_point_multiplier(), 3.0);
        assert_eq!(Framework::Django.entry_point_multiplier(), 3.0);
        assert_eq!(Framework::FastApi.entry_point_multiplier(), 2.8);
        assert_eq!(Framework::Express.entry_point_multiplier(), 2.5);
    }

    #[test]
    fn detect_nextjs_from_package_deps() {
        let ctx = detect(
            &ConfigContext::default(),
            &["next", "react"],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(ctx.has(Framework::NextJs));
    }

    #[test]
    fn detect_fastapi_from_python_deps() {
        let ctx = detect(
            &ConfigContext::default(),
            &[],
            &[],
            &["fastapi", "uvicorn"],
            &[],
            &[],
        );
        assert!(ctx.has(Framework::FastApi));
    }

    #[test]
    fn detect_laravel_from_composer() {
        let ctx = detect(
            &ConfigContext::default(),
            &[],
            &["laravel/framework", "guzzlehttp/guzzle"],
            &[],
            &[],
            &[],
        );
        assert!(ctx.has(Framework::Laravel));
    }

    #[test]
    fn detect_spring_from_maven_coords() {
        let ctx = detect(
            &ConfigContext::default(),
            &[],
            &[],
            &[],
            &[],
            &["org.springframework:spring-core"],
        );
        assert!(ctx.has(Framework::Spring));
    }

    #[test]
    fn detect_nextjs_from_tsconfig_paths() {
        let mut config = ConfigContext::default();
        config.add_workspace(WorkspaceConfig {
            ts_paths: vec![TsPathAlias {
                pattern: "@/*".to_string(),
                targets: vec!["app/*".to_string()],
            }],
            ..Default::default()
        });
        let ctx = detect(&config, &[], &[], &[], &[], &[]);
        assert!(ctx.has(Framework::NextJs));
    }

    #[test]
    fn best_multiplier_picks_highest() {
        let mut ctx = FrameworkContext::default();
        ctx.detected.insert(Framework::Express);
        ctx.detected.insert(Framework::NextJs);
        assert_eq!(ctx.best_multiplier(), 3.0);
    }

    #[test]
    fn empty_context_has_default_multiplier() {
        let ctx = FrameworkContext::default();
        assert!(ctx.is_empty());
        assert_eq!(ctx.best_multiplier(), 1.0);
    }
}
