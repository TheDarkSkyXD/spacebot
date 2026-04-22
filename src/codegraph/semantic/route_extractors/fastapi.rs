//! FastAPI route extractor.
//!
//! FastAPI declares routes via decorators on the router / app
//! instance: `@app.get("/path")`, `@router.post("/users")`, etc.
//! We match the decorator line and then take the immediately-
//! following `def` / `async def` line for the handler name.
//!
//! ```python
//! @app.get("/users/{user_id}")
//! async def read_user(user_id: int):
//!     ...
//! ```

use std::sync::OnceLock;

use regex::Regex;

use super::DetectedRoute;

static DECORATOR_RE: OnceLock<Regex> = OnceLock::new();
static HANDLER_RE: OnceLock<Regex> = OnceLock::new();

fn decorator_re() -> &'static Regex {
    DECORATOR_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*@\s*(?:\w+\.)+(get|post|put|delete|patch|options|head)\s*\(\s*["']([^"']+)["']"#,
        )
        .expect("static regex")
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
        if let Some(cap) = decorator_re().captures(line) {
            let method = cap
                .get(1)
                .map(|m| m.as_str().to_ascii_uppercase())
                .unwrap_or_default();
            let path = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();

            // Walk forward looking for the first `def` — skip over
            // blank lines and additional decorator lines stacked on
            // the same handler.
            let mut handler_name = None;
            for next_line in lines.iter().skip(i + 1).take(4) {
                if let Some(h) = handler_re().captures(next_line) {
                    handler_name = Some(h.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                    break;
                }
            }
            if let Some(name) = handler_name
                && !name.is_empty()
            {
                out.push(DetectedRoute {
                    method,
                    path,
                    handler_name: name,
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
    fn parses_single_route() {
        let src = r#"
from fastapi import FastAPI
app = FastAPI()

@app.get("/users/{user_id}")
async def read_user(user_id: int):
    pass
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/users/{user_id}");
        assert_eq!(routes[0].handler_name, "read_user");
    }

    #[test]
    fn parses_router_decorator() {
        let src = r#"
@router.post("/items")
def create_item(item: Item):
    return item
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "POST");
        assert_eq!(routes[0].handler_name, "create_item");
    }

    #[test]
    fn skips_decorator_without_method() {
        let src = r#"
@app.middleware("http")
async def add_header(req, call_next):
    pass
"#;
        assert!(extract(src).is_empty());
    }
}
