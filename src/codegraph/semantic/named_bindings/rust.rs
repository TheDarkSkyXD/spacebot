//! Rust DI extraction — shaku modules + dependency-injection-style
//! trait-object factories.
//!
//! Rust's idiomatic DI story is sparser than other languages: most
//! code uses plain constructors, and when a framework is involved
//! it's usually `shaku` (`module! { components = [UserServiceImpl] }`)
//! or hand-rolled factory traits. We start with `shaku` and leave
//! room for more patterns as they show up.

use std::sync::OnceLock;

use regex::Regex;

use super::{BindingKind, NamedBinding};

static SHAKU_COMPONENTS_RE: OnceLock<Regex> = OnceLock::new();

fn shaku_components_re() -> &'static Regex {
    SHAKU_COMPONENTS_RE.get_or_init(|| {
        Regex::new(r#"components\s*=\s*\[([^\]]+)\]"#).expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<NamedBinding> {
    let mut out = Vec::new();
    for cap in shaku_components_re().captures_iter(content) {
        let list = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let start = cap.get(0).map(|m| m.start()).unwrap_or(0);
        for item in list.split(',') {
            let name = item
                .trim()
                .trim_end_matches(',')
                .split_whitespace()
                .next()
                .unwrap_or("");
            if name.is_empty() {
                continue;
            }
            out.push(NamedBinding {
                consumer: "Module".to_string(),
                provider: name.to_string(),
                kind: BindingKind::Singleton,
                line: line_for(content, start),
            });
        }
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
    fn shaku_module_detected() {
        let src = r#"
module! {
    MyModule {
        components = [UserServiceImpl, EmailService],
        providers = []
    }
}
"#;
        let bindings = extract(src);
        let providers: Vec<&str> = bindings.iter().map(|b| b.provider.as_str()).collect();
        assert!(providers.contains(&"UserServiceImpl"));
        assert!(providers.contains(&"EmailService"));
    }
}
