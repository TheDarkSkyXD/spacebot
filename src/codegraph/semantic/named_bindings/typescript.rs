//! TypeScript / JavaScript DI extraction.
//!
//! Covers:
//! - Angular / NestJS class decorators: `@Injectable()` marks a
//!   class as a provider; constructor parameter types imply the
//!   bindings.
//! - NestJS `providers: [Foo]` module arrays.
//! - InversifyJS `container.bind<IFoo>(TYPES.IFoo).to(Foo)`.

use std::sync::OnceLock;

use regex::Regex;

use super::{BindingKind, NamedBinding};

static INJECTABLE_CLASS_RE: OnceLock<Regex> = OnceLock::new();
static NEST_PROVIDERS_RE: OnceLock<Regex> = OnceLock::new();
static INVERSIFY_BIND_RE: OnceLock<Regex> = OnceLock::new();

fn injectable_class_re() -> &'static Regex {
    INJECTABLE_CLASS_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*@\s*Injectable\s*\([^)]*\)\s*(?:\r?\n\s*)+export\s+class\s+([A-Za-z_][A-Za-z0-9_]*)"#,
        )
        .expect("static regex")
    })
}

fn nest_providers_re() -> &'static Regex {
    NEST_PROVIDERS_RE.get_or_init(|| {
        Regex::new(r#"providers\s*:\s*\[([^\]]+)\]"#).expect("static regex")
    })
}

fn inversify_bind_re() -> &'static Regex {
    INVERSIFY_BIND_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)bind\s*<\s*([A-Za-z_][A-Za-z0-9_]*)\s*>\s*\([^)]*\)\s*\.\s*to\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)"#,
        )
        .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<NamedBinding> {
    let mut out = Vec::new();

    for cap in injectable_class_re().captures_iter(content) {
        if let Some(name) = cap.get(1) {
            out.push(NamedBinding {
                consumer: "container".to_string(),
                provider: name.as_str().to_string(),
                kind: BindingKind::Singleton,
                line: line_for(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
            });
        }
    }

    for cap in nest_providers_re().captures_iter(content) {
        let list = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let start = cap.get(0).map(|m| m.start()).unwrap_or(0);
        for item in list.split(',') {
            let trimmed = item.trim();
            // Strip curly-brace object-literal forms — those have
            // explicit `provide:` / `useClass:` keys we don't parse
            // here.
            let class_name = trimmed
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(',');
            if !class_name.is_empty()
                && class_name.chars().next().is_some_and(|c| c.is_alphabetic())
            {
                out.push(NamedBinding {
                    consumer: "module".to_string(),
                    provider: class_name.to_string(),
                    kind: BindingKind::Singleton,
                    line: line_for(content, start),
                });
            }
        }
    }

    for cap in inversify_bind_re().captures_iter(content) {
        let interface = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        let impl_class = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
        out.push(NamedBinding {
            consumer: interface,
            provider: impl_class,
            kind: BindingKind::Unknown,
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
    fn injectable_class_detected() {
        let src = r#"
@Injectable()
export class UserService {
  constructor(private db: Database) {}
}
"#;
        let bindings = extract(src);
        assert!(bindings.iter().any(|b| b.provider == "UserService"));
    }

    #[test]
    fn inversify_bind_detected() {
        let src = r#"
container.bind<IUserService>(TYPES.IUserService).to(UserService);
"#;
        let bindings = extract(src);
        assert!(bindings
            .iter()
            .any(|b| b.consumer == "IUserService" && b.provider == "UserService"));
    }
}
