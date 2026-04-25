//! PHP DI extraction — Laravel service container + Symfony
//! autowiring bindings.

use std::sync::OnceLock;

use regex::Regex;

use super::{BindingKind, NamedBinding};

static LARAVEL_BIND_RE: OnceLock<Regex> = OnceLock::new();
static LARAVEL_SINGLETON_RE: OnceLock<Regex> = OnceLock::new();

fn laravel_bind_re() -> &'static Regex {
    LARAVEL_BIND_RE.get_or_init(|| {
        Regex::new(
            r#"(?:\$this->app|app\(\))\s*->\s*bind\s*\(\s*([A-Za-z_\\][A-Za-z0-9_\\]*(?:::class)?)\s*,\s*([A-Za-z_\\][A-Za-z0-9_\\]*(?:::class)?)"#,
        )
        .expect("static regex")
    })
}

fn laravel_singleton_re() -> &'static Regex {
    LARAVEL_SINGLETON_RE.get_or_init(|| {
        Regex::new(
            r#"(?:\$this->app|app\(\))\s*->\s*singleton\s*\(\s*([A-Za-z_\\][A-Za-z0-9_\\]*(?:::class)?)\s*,\s*([A-Za-z_\\][A-Za-z0-9_\\]*(?:::class)?)"#,
        )
        .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<NamedBinding> {
    let mut out = Vec::new();
    for cap in laravel_bind_re().captures_iter(content) {
        out.push(NamedBinding {
            consumer: strip_class_suffix(cap.get(1).map(|m| m.as_str()).unwrap_or("")),
            provider: strip_class_suffix(cap.get(2).map(|m| m.as_str()).unwrap_or("")),
            kind: BindingKind::Transient,
            line: line_for(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }
    for cap in laravel_singleton_re().captures_iter(content) {
        out.push(NamedBinding {
            consumer: strip_class_suffix(cap.get(1).map(|m| m.as_str()).unwrap_or("")),
            provider: strip_class_suffix(cap.get(2).map(|m| m.as_str()).unwrap_or("")),
            kind: BindingKind::Singleton,
            line: line_for(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }
    out
}

fn strip_class_suffix(s: &str) -> String {
    s.trim_end_matches("::class")
        .rsplit('\\')
        .next()
        .unwrap_or(s)
        .to_string()
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
    fn laravel_bind_detected() {
        let src = r#"$this->app->bind(UserRepository::class, EloquentUserRepository::class);"#;
        let bindings = extract(src);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].consumer, "UserRepository");
        assert_eq!(bindings[0].provider, "EloquentUserRepository");
        assert_eq!(bindings[0].kind, BindingKind::Transient);
    }

    #[test]
    fn laravel_singleton_detected() {
        let src =
            r#"$this->app->singleton(ILogger::class, FileLogger::class);"#;
        let bindings = extract(src);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].kind, BindingKind::Singleton);
    }
}
