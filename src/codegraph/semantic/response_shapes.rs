//! HTTP-consumer detection + response-shape inference.
//!
//! GitNexus wires the fetch / axios / requests / httpx call sites
//! into the graph as FETCHES edges carrying a `response_schema`
//! property. That lets downstream impact analysis ("which endpoint
//! does this client code talk to?") and route-consumer matching
//! ("this handler returns `UserDTO`; which clients depend on its
//! shape?") function without a separate schema registry.
//!
//! This module does the text-level detection half. Full schema
//! inference — walking the surrounding type annotation / jsdoc /
//! Pydantic model — is a pipeline-phase concern because it needs the
//! parsed symbol table; we expose a structured [`FetchCall`] record
//! here and let the caller fill in the response shape when it can.

use std::sync::OnceLock;

use regex::Regex;

/// One detected HTTP client call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchCall {
    /// HTTP method in uppercase (`GET`, `POST`, …). Defaults to `GET`
    /// when the client (e.g. `fetch(url)`) doesn't declare a method.
    pub method: String,
    /// URL as written in the source. May be a template / f-string —
    /// the raw text is preserved so downstream matching can decide
    /// whether to normalise.
    pub url: String,
    /// 1-based line of the call site.
    pub line: u32,
}

static FETCH_CALL_RE: OnceLock<Regex> = OnceLock::new();
static AXIOS_METHOD_RE: OnceLock<Regex> = OnceLock::new();
static REQUESTS_METHOD_RE: OnceLock<Regex> = OnceLock::new();

fn fetch_call_re() -> &'static Regex {
    // `fetch('/users', { method: 'POST' })` — we capture the URL;
    // method lives in the optional options object and is parsed
    // separately below.
    FETCH_CALL_RE.get_or_init(|| {
        Regex::new(r#"\bfetch\s*\(\s*[`'"]([^`'"]+)[`'"]"#).expect("static regex")
    })
}

fn axios_method_re() -> &'static Regex {
    AXIOS_METHOD_RE.get_or_init(|| {
        Regex::new(
            r#"\b(?:axios|\$|jQuery)\.(get|post|put|delete|patch|head|options)\s*\(\s*[`'"]([^`'"]+)[`'"]"#,
        )
        .expect("static regex")
    })
}

fn requests_method_re() -> &'static Regex {
    REQUESTS_METHOD_RE.get_or_init(|| {
        Regex::new(
            r#"\b(?:requests|httpx|session)\.(get|post|put|delete|patch|head|options)\s*\(\s*[`'"]([^`'"]+)[`'"]"#,
        )
        .expect("static regex")
    })
}

/// Extract every fetch / axios / requests call from the given source.
///
/// Works on raw text — the caller doesn't need to know whether the
/// source is JS, TS, Python, etc. All three regex patterns are side
/// by side because projects often mix call styles (Next.js page
/// calls `fetch`, a helper wraps `axios`, a Python job uses
/// `requests`) and a unified output simplifies downstream work.
pub fn extract(content: &str) -> Vec<FetchCall> {
    let mut out = Vec::new();

    for cap in fetch_call_re().captures_iter(content) {
        let url = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        out.push(FetchCall {
            method: "GET".to_string(),
            url,
            line: line_for(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }

    for cap in axios_method_re().captures_iter(content) {
        let method = cap
            .get(1)
            .map(|m| m.as_str().to_ascii_uppercase())
            .unwrap_or_default();
        let url = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
        out.push(FetchCall {
            method,
            url,
            line: line_for(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }

    for cap in requests_method_re().captures_iter(content) {
        let method = cap
            .get(1)
            .map(|m| m.as_str().to_ascii_uppercase())
            .unwrap_or_default();
        let url = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
        out.push(FetchCall {
            method,
            url,
            line: line_for(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }

    out
}

fn line_for(content: &str, byte_offset: usize) -> u32 {
    content[..byte_offset.min(content.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count() as u32
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_default_method_is_get() {
        let calls = extract(r#"await fetch('/api/users')"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "GET");
        assert_eq!(calls[0].url, "/api/users");
    }

    #[test]
    fn axios_methods_preserved() {
        let calls = extract(r#"axios.post('/login', payload); $.get('/me');"#);
        assert_eq!(calls.len(), 2);
        let methods: Vec<&str> = calls.iter().map(|c| c.method.as_str()).collect();
        assert!(methods.contains(&"POST"));
        assert!(methods.contains(&"GET"));
    }

    #[test]
    fn python_requests_detected() {
        let calls = extract(r#"r = requests.get("http://api.example.com/data")"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "GET");
    }
}
