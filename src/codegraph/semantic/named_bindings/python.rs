//! Python DI extraction (FastAPI `Depends`, `dependency_injector`,
//! Django class-based views).

use std::sync::OnceLock;

use regex::Regex;

use super::{BindingKind, NamedBinding};

static DEPENDS_RE: OnceLock<Regex> = OnceLock::new();
static PROVIDER_ASSIGN_RE: OnceLock<Regex> = OnceLock::new();

fn depends_re() -> &'static Regex {
    DEPENDS_RE.get_or_init(|| {
        // `user: User = Depends(get_current_user)` — the consumer is
        // the parameter name, the provider is the callable.
        Regex::new(
            r#"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*[^=]*=\s*Depends\s*\(\s*([A-Za-z_][A-Za-z0-9_.]*)\s*\)"#,
        )
        .expect("static regex")
    })
}

fn provider_assign_re() -> &'static Regex {
    PROVIDER_ASSIGN_RE.get_or_init(|| {
        // `service = providers.Singleton(UserService)` from the
        // dependency-injector library — the attribute is the
        // lifecycle kind.
        Regex::new(
            r#"([A-Za-z_][A-Za-z0-9_]*)\s*=\s*providers\.(Singleton|Factory|Resource|Callable|ThreadLocalSingleton)\s*\(\s*([A-Za-z_][A-Za-z0-9_.]*)"#,
        )
        .expect("static regex")
    })
}

pub fn extract(content: &str) -> Vec<NamedBinding> {
    let mut out = Vec::new();

    for cap in depends_re().captures_iter(content) {
        out.push(NamedBinding {
            consumer: cap
                .get(1)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            provider: cap
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            kind: BindingKind::Scoped,
            line: line_for(content, cap.get(0).map(|m| m.start()).unwrap_or(0)),
        });
    }

    for cap in provider_assign_re().captures_iter(content) {
        let kind = match cap.get(2).map(|m| m.as_str()) {
            Some("Singleton") | Some("ThreadLocalSingleton") => BindingKind::Singleton,
            Some("Factory") | Some("Callable") => BindingKind::Factory,
            Some("Resource") => BindingKind::Scoped,
            _ => BindingKind::Unknown,
        };
        out.push(NamedBinding {
            consumer: cap
                .get(1)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            provider: cap
                .get(3)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
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
    fn fastapi_depends_detected() {
        let src = r#"
def read_users(user: User = Depends(get_current_user)):
    return user
"#;
        let bindings = extract(src);
        assert!(bindings
            .iter()
            .any(|b| b.consumer == "user" && b.provider == "get_current_user"));
    }

    #[test]
    fn dependency_injector_singleton_detected() {
        let src = r#"
user_service = providers.Singleton(UserService)
"#;
        let bindings = extract(src);
        assert!(bindings.iter().any(
            |b| b.consumer == "user_service" && b.provider == "UserService" && b.kind == BindingKind::Singleton
        ));
    }
}
