//! Laravel route extractor.
//!
//! Handles the classic static-helper form — `Route::get('/path',
//! 'Controller@action')` and `Route::post('/path', [Controller::class,
//! 'action'])`. Attribute-based routing (PHP 8 `#[Route]`) is more
//! common in Symfony and is covered there; Laravel projects almost
//! always use the helper API.

use std::sync::OnceLock;

use regex::Regex;

use super::DetectedRoute;

static ROUTE_HELPER_RE: OnceLock<Regex> = OnceLock::new();

fn route_helper_re() -> &'static Regex {
    ROUTE_HELPER_RE.get_or_init(|| {
        Regex::new(
            r#"Route::(get|post|put|delete|patch|options|any)\s*\(\s*['"]([^'"]+)['"]\s*,\s*(?:\[[^,\]]+,\s*['"]([^'"]+)['"]|['"][^@'"]*@([A-Za-z_][A-Za-z0-9_]*)['"]|([A-Za-z_][A-Za-z0-9_\\]*))"#,
        )
        .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<DetectedRoute> {
    let mut out = Vec::new();
    for cap in route_helper_re().captures_iter(content) {
        let method = cap
            .get(1)
            .map(|m| m.as_str().to_ascii_uppercase())
            .unwrap_or_default();
        let path = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
        // handler_name picks whichever of the three alternatives
        // matched — `[Class::class, 'action']`, `Class@action`, or
        // a bare closure-like identifier.
        let handler = cap
            .get(3)
            .or_else(|| cap.get(4))
            .or_else(|| cap.get(5))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        let handler_name = handler
            .rsplit(['\\', ':'])
            .next()
            .unwrap_or(handler.as_str())
            .to_string();
        if method.is_empty() || path.is_empty() || handler_name.is_empty() {
            continue;
        }
        let line = content[..cap.get(0).map(|m| m.start()).unwrap_or(0)]
            .bytes()
            .filter(|&b| b == b'\n')
            .count() as u32
            + 1;
        out.push(DetectedRoute {
            method,
            path,
            handler_name,
            line,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_at_action_form() {
        let src = r#"<?php
Route::get('/users', 'UserController@index');
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/users");
        assert_eq!(routes[0].handler_name, "index");
    }

    #[test]
    fn parses_array_action_form() {
        let src = r#"<?php
Route::post('/users', [UserController::class, 'store']);
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "POST");
        assert_eq!(routes[0].handler_name, "store");
    }
}
