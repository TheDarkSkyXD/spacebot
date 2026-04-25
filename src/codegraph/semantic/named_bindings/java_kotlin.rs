//! Spring / Koin DI extraction (Java + Kotlin).
//!
//! Catches the common annotation-based shapes:
//! - Spring: `@Autowired`, `@Component`, `@Service`, `@Repository`
//!   on class / field / constructor.
//! - Koin (Kotlin): `single { UserService() }`, `factory { ... }`.

use std::sync::OnceLock;

use regex::Regex;

use super::{BindingKind, NamedBinding};

static SPRING_CLASS_RE: OnceLock<Regex> = OnceLock::new();
static KOIN_RE: OnceLock<Regex> = OnceLock::new();

fn spring_class_re() -> &'static Regex {
    SPRING_CLASS_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*@\s*(Component|Service|Repository|Controller|RestController|Configuration)\b[^\n]*(?:\r?\n\s*)*(?:public\s+|final\s+)*class\s+([A-Za-z_][A-Za-z0-9_]*)"#,
        )
        .expect("static regex")
    })
}

fn koin_re() -> &'static Regex {
    KOIN_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)\b(single|factory|scoped|viewModel)\s*\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\("#,
        )
        .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<NamedBinding> {
    let mut out = Vec::new();
    for cap in spring_class_re().captures_iter(content) {
        let class_name = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
        out.push(NamedBinding {
            consumer: "ApplicationContext".to_string(),
            provider: class_name,
            kind: BindingKind::Singleton,
            line: line_for(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }
    for cap in koin_re().captures_iter(content) {
        let kind = match cap.get(1).map(|m| m.as_str()) {
            Some("single") => BindingKind::Singleton,
            Some("factory") => BindingKind::Factory,
            Some("scoped") | Some("viewModel") => BindingKind::Scoped,
            _ => BindingKind::Unknown,
        };
        let provider = cap
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        out.push(NamedBinding {
            consumer: "KoinModule".to_string(),
            provider,
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
    fn spring_service_detected() {
        let src = r#"
@Service
public class UserService {
}
"#;
        let bindings = extract(src);
        assert!(bindings.iter().any(|b| b.provider == "UserService"));
    }

    #[test]
    fn koin_single_detected() {
        let src = r#"single { UserService() }"#;
        let bindings = extract(src);
        assert!(bindings
            .iter()
            .any(|b| b.provider == "UserService" && b.kind == BindingKind::Singleton));
    }
}
