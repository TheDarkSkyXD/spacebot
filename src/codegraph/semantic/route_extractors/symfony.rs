//! Symfony attribute-routing extractor.
//!
//! Symfony PHP 8+ routes use attribute syntax on controller methods:
//!
//! ```php
//! #[Route('/users', name: 'user_list', methods: ['GET'])]
//! public function index(): Response { ... }
//! ```
//!
//! We match the attribute line and pull the handler from the
//! subsequent `function` declaration (public/private doesn't matter
//! for routing — Symfony invokes the attribute's bound method).

use std::sync::OnceLock;

use regex::Regex;

use super::DetectedRoute;

static ATTR_RE: OnceLock<Regex> = OnceLock::new();
static METHODS_RE: OnceLock<Regex> = OnceLock::new();
static HANDLER_RE: OnceLock<Regex> = OnceLock::new();

fn attr_re() -> &'static Regex {
    ATTR_RE.get_or_init(|| {
        Regex::new(r#"(?m)^\s*#\[\s*Route\s*\(\s*['"]([^'"]+)['"]([^)]*)\)\s*\]"#)
            .expect("static regex")
    })
}

fn methods_re() -> &'static Regex {
    METHODS_RE.get_or_init(|| {
        Regex::new(r#"methods\s*:\s*\[([^\]]+)\]"#).expect("static regex")
    })
}

fn handler_re() -> &'static Regex {
    HANDLER_RE.get_or_init(|| {
        Regex::new(
            r#"^\s*(?:public|private|protected)?\s*function\s+([A-Za-z_][A-Za-z0-9_]*)"#,
        )
        .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<DetectedRoute> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(cap) = attr_re().captures(line) {
            let path = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            let tail = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let methods: Vec<String> = match methods_re().captures(tail) {
                Some(m) => m[1]
                    .split(',')
                    .map(|s| {
                        s.trim()
                            .trim_matches(|c| c == '"' || c == '\'')
                            .to_ascii_uppercase()
                    })
                    .filter(|s| !s.is_empty())
                    .collect(),
                None => vec!["*".to_string()],
            };

            let mut handler_name = None;
            for next_line in lines.iter().skip(i + 1).take(4) {
                if let Some(h) = handler_re().captures(next_line) {
                    handler_name = Some(h.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                    break;
                }
            }
            let Some(name) = handler_name else { continue };
            for method in &methods {
                out.push(DetectedRoute {
                    method: method.clone(),
                    path: path.clone(),
                    handler_name: name.clone(),
                    line: (i + 1) as u32,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_symfony_attribute_route() {
        let src = r#"<?php
#[Route('/users', name: 'user_list', methods: ['GET'])]
public function index(): Response {
    return new Response();
}
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/users");
        assert_eq!(routes[0].handler_name, "index");
    }

    #[test]
    fn parses_multiple_methods() {
        let src = r#"<?php
#[Route('/items', methods: ['GET', 'POST'])]
public function items(): Response { }
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 2);
    }
}
