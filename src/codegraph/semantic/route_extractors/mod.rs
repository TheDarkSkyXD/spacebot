//! Framework-specific HTTP route extractors.
//!
//! Each submodule recognises one framework's route-declaration
//! dialect and emits [`DetectedRoute`] records. The extractors work
//! on raw file content (plus an optional file-path hint) rather than
//! parsed ASTs because:
//!
//! - The decorator / attribute / helper-call patterns we scan for
//!   are unambiguous at the text level and cheap to regex.
//! - Tree-sitter queries for the same patterns would need per-
//!   language grammar-specific forms, duplicating work better done
//!   once here.
//! - Framework detection runs during the Phase 5 routes phase, after
//!   parsing has already produced handler function nodes — the
//!   extractors here only need to report the `(method, path,
//!   handler_name)` triple; the pipeline looks up the matching
//!   Function/Method node by name.
//!
//! Extractors are **best-effort**. Missed routes just don't get
//! HANDLES_ROUTE edges; false positives create dangling Route nodes
//! that the enrichment phase later prunes. Neither failure mode is
//! load-bearing.

pub mod aspnet;
pub mod django;
pub mod fastapi;
pub mod flask;
pub mod laravel;
pub mod symfony;

use std::path::Path;

/// A single detected HTTP route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRoute {
    /// HTTP method (`GET`, `POST`, `*` for catch-all) in uppercase.
    pub method: String,
    /// URL path pattern as written in the source. May contain
    /// framework-specific placeholders like `:id` or `<int:id>`.
    pub path: String,
    /// Bare name of the handler function / method. The caller
    /// resolves this to a Function/Method qualified-name via the
    /// existing symbol index.
    pub handler_name: String,
    /// 1-based source line where the route was declared.
    pub line: u32,
}

/// Dispatch to the right extractor based on file extension + content
/// heuristics. Returns an empty vec when no pattern matches — the
/// caller shouldn't panic on files that happen to look like routes
/// but belong to unsupported frameworks.
pub fn extract_all(path: &Path, content: &str) -> Vec<DetectedRoute> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let mut out: Vec<DetectedRoute> = Vec::new();
    match ext.as_str() {
        "py" => {
            if file_name == "urls.py" {
                out.extend(django::extract(content));
            }
            out.extend(fastapi::extract(content));
            out.extend(flask::extract(content));
        }
        "php" => {
            out.extend(laravel::extract(content));
            out.extend(symfony::extract(content));
        }
        "cs" => {
            out.extend(aspnet::extract(content));
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unknown_extension_returns_empty() {
        assert!(extract_all(&PathBuf::from("foo.txt"), "anything").is_empty());
    }
}
