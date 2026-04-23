//! HTML / template language provider.
//!
//! HTML files participate in the graph for two reasons:
//!
//! 1. **Form-action URLs** — `<form action="/login">` references a
//!    backend route; picking it up lets downstream analysis link
//!    template files to the handlers they submit to.
//! 2. **Script references** — `<script src="./foo.js">` tells us
//!    which JS file this template runs.
//!
//! Tree-sitter has HTML grammars, but for the patterns we care about
//! a pair of regexes is smaller and faster. No structural symbols
//! are extracted — HTML files only contribute Section-shaped
//! `form` / `a` / `script` references through the pipeline's
//! FETCHES / IMPORTS edges. Those edges are emitted by the routes
//! phase (form action → Route) and parsing phase (script src →
//! IMPORTS).
//!
//! The provider itself returns no symbols today; the extractor
//! below is exposed as a standalone helper so the routes phase can
//! consume it without going through the trait.

use std::sync::OnceLock;

use regex::Regex;

use super::languages::SupportedLanguage;
use super::provider::{ExtractedSymbol, LanguageProvider};
use crate::codegraph::types::NodeLabel;

pub struct HtmlProvider;

impl LanguageProvider for HtmlProvider {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Html
    }

    fn file_extensions(&self) -> &[&str] {
        &["html", "htm"]
    }

    fn supported_labels(&self) -> &[NodeLabel] {
        &[]
    }

    /// HTML has no code-symbol declarations worth emitting as graph
    /// nodes. Files show up as `File` nodes via the structure phase;
    /// their in-template references are picked up by the routes /
    /// parsing phases through [`extract_form_actions`] and
    /// [`extract_script_srcs`].
    fn extract_symbols(&self, _file_path: &str, _content: &str) -> Vec<ExtractedSymbol> {
        Vec::new()
    }
}

static FORM_ACTION_RE: OnceLock<Regex> = OnceLock::new();
static SCRIPT_SRC_RE: OnceLock<Regex> = OnceLock::new();

fn form_action_re() -> &'static Regex {
    FORM_ACTION_RE.get_or_init(|| {
        Regex::new(r#"(?i)<\s*form\b[^>]*\baction\s*=\s*['"]([^'"]+)['"][^>]*(?:\bmethod\s*=\s*['"]([^'"]+)['"])?"#)
            .expect("static regex")
    })
}

fn script_src_re() -> &'static Regex {
    SCRIPT_SRC_RE.get_or_init(|| {
        Regex::new(r#"(?i)<\s*script\b[^>]*\bsrc\s*=\s*['"]([^'"]+)['"]"#).expect("static regex")
    })
}

/// One `<form action="...">` reference discovered in HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlFormAction {
    pub url: String,
    /// HTTP method in uppercase. Defaults to `GET` when the form
    /// omits the `method` attribute (HTML default).
    pub method: String,
    pub line: u32,
}

/// One `<script src="...">` reference. Drives IMPORTS edges from
/// the template file to the referenced script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlScriptSrc {
    pub src: String,
    pub line: u32,
}

/// Extract every `<form action="url" method="X">` pair from HTML.
/// Method defaults to `GET` per the HTML spec.
pub fn extract_form_actions(content: &str) -> Vec<HtmlFormAction> {
    let mut out = Vec::new();
    for cap in form_action_re().captures_iter(content) {
        let url = cap
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        let method = cap
            .get(2)
            .map(|m| m.as_str().to_ascii_uppercase())
            .unwrap_or_else(|| "GET".to_string());
        if url.is_empty() {
            continue;
        }
        out.push(HtmlFormAction {
            url,
            method,
            line: line_for_offset(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }
    out
}

/// Extract `<script src="...">` references. Relative targets become
/// candidate File→File IMPORTS edges; absolute URLs are left for
/// external-reference tracking.
pub fn extract_script_srcs(content: &str) -> Vec<HtmlScriptSrc> {
    let mut out = Vec::new();
    for cap in script_src_re().captures_iter(content) {
        let src = cap
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if src.is_empty() {
            continue;
        }
        out.push(HtmlScriptSrc {
            src,
            line: line_for_offset(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }
    out
}

fn line_for_offset(content: &str, offset: usize) -> u32 {
    content[..offset.min(content.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count() as u32
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_form_action_default_get() {
        let src = r#"<form action="/login"><input name="user"></form>"#;
        let out = extract_form_actions(src);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url, "/login");
        assert_eq!(out[0].method, "GET");
    }

    #[test]
    fn extracts_form_action_with_method() {
        let src = r#"<form action="/users" method="post"><input /></form>"#;
        let out = extract_form_actions(src);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].method, "POST");
    }

    #[test]
    fn extracts_script_src() {
        let src = r#"<script src="./bundle.js"></script>"#;
        let out = extract_script_srcs(src);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].src, "./bundle.js");
    }

    #[test]
    fn ignores_inline_script_blocks() {
        let src = r#"<script>console.log("hi")</script>"#;
        let out = extract_script_srcs(src);
        assert!(out.is_empty());
    }
}
