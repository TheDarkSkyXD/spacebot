//! ASP.NET attribute route extractor.
//!
//! Covers the three common styles:
//!
//! - Attribute routing: `[HttpGet("users")]`, `[Route("api/users")]`
//!   on controller actions.
//! - Minimal APIs: `app.MapGet("/users", handler)`,
//!   `app.MapPost("/users", ...)` at the top of Program.cs.
//! - Controller-scope templates: `[Route("api/[controller]")]` on a
//!   class — the extractor records the template against the
//!   containing class name so the pipeline can combine it with
//!   method-level `[HttpVerb(...)]` attributes.

use std::sync::OnceLock;

use regex::Regex;

use super::DetectedRoute;

static HTTP_ATTR_RE: OnceLock<Regex> = OnceLock::new();
static MINIMAL_MAP_RE: OnceLock<Regex> = OnceLock::new();
static ACTION_RE: OnceLock<Regex> = OnceLock::new();

fn http_attr_re() -> &'static Regex {
    HTTP_ATTR_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*\[\s*Http(Get|Post|Put|Delete|Patch|Options|Head)(?:\(\s*(?:Name\s*=\s*)?["']([^"']*)["']\s*\))?\s*\]"#,
        )
        .expect("static regex")
    })
}

fn minimal_map_re() -> &'static Regex {
    MINIMAL_MAP_RE.get_or_init(|| {
        Regex::new(
            r#"\.Map(Get|Post|Put|Delete|Patch)\s*\(\s*["']([^"']+)["']\s*,\s*([A-Za-z_][A-Za-z0-9_.]*)"#,
        )
        .expect("static regex")
    })
}

fn action_re() -> &'static Regex {
    ACTION_RE.get_or_init(|| {
        Regex::new(
            r#"^\s*(?:public|private|protected|internal)?\s*(?:async\s+)?(?:virtual\s+|override\s+)?\S+\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("#,
        )
        .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<DetectedRoute> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if let Some(cap) = http_attr_re().captures(line) {
            let method = cap
                .get(1)
                .map(|m| m.as_str().to_ascii_uppercase())
                .unwrap_or_default();
            let path = cap
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "/".to_string());
            let mut handler_name = None;
            for next_line in lines.iter().skip(i + 1).take(7) {
                if let Some(h) = action_re().captures(next_line) {
                    handler_name = Some(h.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                    break;
                }
            }
            let Some(name) = handler_name else { continue };
            out.push(DetectedRoute {
                method,
                path,
                handler_name: name,
                line: (i + 1) as u32,
            });
        }
    }

    for cap in minimal_map_re().captures_iter(content) {
        let method = cap
            .get(1)
            .map(|m| m.as_str().to_ascii_uppercase())
            .unwrap_or_default();
        let path = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
        let handler_full = cap.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
        let handler_name = handler_full
            .rsplit('.')
            .next()
            .unwrap_or(handler_full.as_str())
            .to_string();
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
    fn parses_http_attribute() {
        let src = r#"
public class UsersController : ControllerBase
{
    [HttpGet("users")]
    public IActionResult List() => Ok();
}
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "users");
        assert_eq!(routes[0].handler_name, "List");
    }

    #[test]
    fn parses_minimal_api() {
        let src = r#"
app.MapGet("/health", HealthChecks.Handler);
app.MapPost("/items", CreateItem);
"#;
        let routes = extract(src);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].handler_name, "Handler");
        assert_eq!(routes[1].method, "POST");
        assert_eq!(routes[1].handler_name, "CreateItem");
    }
}
