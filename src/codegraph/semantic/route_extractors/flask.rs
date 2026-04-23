//! Flask route extractor.
//!
//! Flask routes use `@app.route("/path")` with optional
//! `methods=["GET", "POST"]`. When `methods` is omitted Flask
//! defaults to `GET`, which we mirror by emitting `GET` as the
//! method.
//!
//! ```python
//! @app.route("/users", methods=["GET", "POST"])
//! def users():
//!     ...
//! ```

use std::sync::OnceLock;

use regex::Regex;

use super::DetectedRoute;

static ROUTE_RE: OnceLock<Regex> = OnceLock::new();
static METHOD_LIST_RE: OnceLock<Regex> = OnceLock::new();
static HANDLER_RE: OnceLock<Regex> = OnceLock::new();

fn route_re() -> &'static Regex {
    ROUTE_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*@\s*(?:\w+\.)*route\s*\(\s*["']([^"']+)["']([^)]*)\)"#,
        )
        .expect("static regex")
    })
}

fn method_list_re() -> &'static Regex {
    METHOD_LIST_RE.get_or_init(|| {
        Regex::new(r#"methods\s*=\s*\[([^\]]+)\]"#).expect("static regex")
    })
}

fn handler_re() -> &'static Regex {
    HANDLER_RE.get_or_init(|| {
        Regex::new(r#"^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)"#)
            .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<DetectedRoute> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(cap) = route_re().captures(line) {
            let path = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            let tail = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let methods: Vec<String> = match method_list_re().captures(tail) {
                Some(m) => m[1]
                    .split(',')
                    .map(|s| {
                        s.trim()
                            .trim_matches(|c| c == '"' || c == '\'')
                            .to_ascii_uppercase()
                    })
                    .filter(|s| !s.is_empty())
                    .collect(),
                None => vec!["GET".to_string()],
            };

            // Find the handler `def` on a subsequent line.
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
    fn default_method_is_get() {
        let src = r#"
@app.route("/users")
def users():
    pass
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/users");
    }

    #[test]
    fn explicit_methods_list_expanded() {
        let src = r#"
@app.route("/users", methods=["GET", "POST"])
def users():
    pass
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 2);
        let methods: Vec<_> = routes.iter().map(|r| r.method.as_str()).collect();
        assert!(methods.contains(&"GET"));
        assert!(methods.contains(&"POST"));
    }
}
