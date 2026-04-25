//! Django `urls.py` route extractor.
//!
//! Handles both the classic `url()` form and the modern `path()` /
//! `re_path()` helpers. Examples covered:
//!
//! ```python
//! path("users/", views.UserListView.as_view(), name="user-list"),
//! path("users/<int:pk>/", views.UserDetailView.as_view()),
//! re_path(r"^admin/.*", admin_view),
//! url(r"^healthz/$", healthz_handler),
//! ```
//!
//! The regex is line-anchored and tolerant of whitespace + leading
//! `django.urls.path(...)` / `urls.path(...)` attribute access.

use std::sync::OnceLock;

use regex::Regex;

use super::DetectedRoute;

static PATH_CALL_RE: OnceLock<Regex> = OnceLock::new();

fn path_call_re() -> &'static Regex {
    PATH_CALL_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*(?:\w+\.)*(?:path|re_path|url)\s*\(\s*(?:r?["']([^"']+)["'])\s*,\s*([A-Za-z_][A-Za-z0-9_.]*)"#,
        )
        .expect("static regex")
    })
}

/// Extract Django routes from a `urls.py` source. Returns one entry
/// per recognised route call. Method is always `*` because Django
/// routes accept every HTTP verb — the view itself decides which
/// verbs to handle.
pub fn extract(content: &str) -> Vec<DetectedRoute> {
    let mut out = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        if let Some(cap) = path_call_re().captures(line) {
            let path = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            let handler_full = cap
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            // `views.UserListView.as_view()` → take the last segment
            // for handler_name. The caller resolves against the
            // symbol table, which keys by simple name.
            let handler_name = handler_full
                .rsplit('.')
                .next()
                .unwrap_or(handler_full.as_str())
                .to_string();
            if handler_name.is_empty() || path.is_empty() {
                continue;
            }
            out.push(DetectedRoute {
                method: "*".to_string(),
                path,
                handler_name,
                line: (line_idx + 1) as u32,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_path_helper() {
        let src = r#"
from django.urls import path
from . import views

urlpatterns = [
    path("users/", views.list_users, name="user-list"),
    path("users/<int:pk>/", views.user_detail),
]
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "*");
        assert_eq!(routes[0].path, "users/");
        assert_eq!(routes[0].handler_name, "list_users");
        assert_eq!(routes[1].path, "users/<int:pk>/");
        assert_eq!(routes[1].handler_name, "user_detail");
    }

    #[test]
    fn parses_re_path_helper() {
        let src = r#"re_path(r"^admin/.*", admin_view)"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].path, "^admin/.*");
        assert_eq!(routes[0].handler_name, "admin_view");
    }

    #[test]
    fn ignores_non_route_calls() {
        let src = r#"print("just a line"); other_func()"#;
        assert!(extract(src).is_empty());
    }
}
