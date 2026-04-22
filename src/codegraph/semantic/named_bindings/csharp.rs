//! C# DI extraction — `services.AddSingleton<IFoo, Foo>()` and
//! friends against IServiceCollection.

use std::sync::OnceLock;

use regex::Regex;

use super::{BindingKind, NamedBinding};

static ADD_SERVICE_RE: OnceLock<Regex> = OnceLock::new();

fn add_service_re() -> &'static Regex {
    ADD_SERVICE_RE.get_or_init(|| {
        Regex::new(
            r#"Add(Singleton|Scoped|Transient)\s*<\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?:,\s*([A-Za-z_][A-Za-z0-9_]*))?\s*>"#,
        )
        .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<NamedBinding> {
    let mut out = Vec::new();
    for cap in add_service_re().captures_iter(content) {
        let kind = match cap.get(1).map(|m| m.as_str()) {
            Some("Singleton") => BindingKind::Singleton,
            Some("Scoped") => BindingKind::Scoped,
            Some("Transient") => BindingKind::Transient,
            _ => BindingKind::Unknown,
        };
        let first = cap
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        // Two-generic form: consumer = interface, provider = impl.
        // Single-generic form: consumer and provider are the same.
        let second = cap
            .get(3)
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| first.clone());
        out.push(NamedBinding {
            consumer: first,
            provider: second,
            kind,
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
    fn two_generic_form_detected() {
        let src = r#"services.AddSingleton<IUserService, UserService>();"#;
        let bindings = extract(src);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].consumer, "IUserService");
        assert_eq!(bindings[0].provider, "UserService");
        assert_eq!(bindings[0].kind, BindingKind::Singleton);
    }

    #[test]
    fn single_generic_form_detected() {
        let src = r#"services.AddScoped<DataService>();"#;
        let bindings = extract(src);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].consumer, "DataService");
        assert_eq!(bindings[0].provider, "DataService");
        assert_eq!(bindings[0].kind, BindingKind::Scoped);
    }
}
