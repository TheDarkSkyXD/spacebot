//! Prisma schema language provider.
//!
//! Prisma schemas (`.prisma`) declare data models that application
//! code queries through the Prisma client (`prisma.user.findUnique`,
//! `prisma.post.create`). Indexing the schema lets downstream ORM
//! dataflow extraction link those query calls back to the declared
//! model — a Class node with the model's name, properties for each
//! field, and enum nodes for each `enum` block.
//!
//! Prisma schemas are simple enough that a regex-driven scan is
//! sufficient. The grammar has very regular block shapes:
//!
//! ```prisma
//! model User {
//!   id    Int    @id @default(autoincrement())
//!   email String @unique
//!   posts Post[]
//! }
//!
//! enum Role {
//!   USER
//!   ADMIN
//! }
//! ```
//!
//! We extract `model <Name>` as a Class node and `enum <Name>` as an
//! Enum node, plus every field inside a `model` as a Property node
//! with the field name as its identifier.

use std::sync::OnceLock;

use regex::Regex;

use super::languages::SupportedLanguage;
use super::provider::{ExtractedSymbol, LanguageProvider};
use crate::codegraph::types::NodeLabel;

pub struct PrismaProvider;

static MODEL_RE: OnceLock<Regex> = OnceLock::new();
static ENUM_RE: OnceLock<Regex> = OnceLock::new();
static FIELD_RE: OnceLock<Regex> = OnceLock::new();

fn model_re() -> &'static Regex {
    MODEL_RE.get_or_init(|| {
        Regex::new(r"(?m)^\s*model\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{").expect("static regex")
    })
}

fn enum_re() -> &'static Regex {
    ENUM_RE.get_or_init(|| {
        Regex::new(r"(?m)^\s*enum\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{").expect("static regex")
    })
}

fn field_re() -> &'static Regex {
    // Simple field line: `name Type ...`. Attributes (`@id`, `@default(...)`)
    // are ignored — we only need the field name for Property extraction.
    FIELD_RE.get_or_init(|| {
        Regex::new(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s+[A-Za-z_][A-Za-z0-9_\[\]?]*")
            .expect("static regex")
    })
}

impl LanguageProvider for PrismaProvider {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Prisma
    }

    fn file_extensions(&self) -> &[&str] {
        &["prisma"]
    }

    fn supported_labels(&self) -> &[NodeLabel] {
        &[NodeLabel::Class, NodeLabel::Enum, NodeLabel::Property]
    }

    fn extract_symbols(&self, file_path: &str, content: &str) -> Vec<ExtractedSymbol> {
        let mut symbols = Vec::new();

        // Models → Class + their fields → Property.
        for cap in model_re().captures_iter(content) {
            let Some(name_m) = cap.get(1) else { continue };
            let name = name_m.as_str();
            let line_start = line_for_offset(content, cap.get(0).unwrap().start());
            let (body, body_start_line) = extract_block(content, cap.get(0).unwrap().end());
            let line_end = body_start_line + body.lines().count() as u32;

            symbols.push(ExtractedSymbol {
                name: name.to_string(),
                qualified_name: format!("{file_path}::model::{name}"),
                label: NodeLabel::Class,
                line_start,
                line_end,
                is_exported: true,
                visibility: Some("public".to_string()),
                ..Default::default()
            });

            let parent_qname = format!("{file_path}::model::{name}");
            for (offset, field_line) in body.lines().enumerate() {
                // Skip attribute-only lines (`@@index([...])`) and
                // separators.
                let trimmed = field_line.trim();
                if trimmed.is_empty()
                    || trimmed.starts_with("//")
                    || trimmed.starts_with('@')
                    || trimmed.starts_with('}')
                {
                    continue;
                }
                let Some(field_cap) = field_re().captures(field_line) else {
                    continue;
                };
                let field_name = field_cap.get(1).unwrap().as_str();
                let field_line_num = body_start_line + offset as u32;
                symbols.push(ExtractedSymbol {
                    name: field_name.to_string(),
                    qualified_name: format!("{parent_qname}::{field_name}"),
                    label: NodeLabel::Property,
                    line_start: field_line_num,
                    line_end: field_line_num,
                    parent: Some(parent_qname.clone()),
                    is_exported: true,
                    ..Default::default()
                });
            }
        }

        // Enums → Enum node. We don't extract variants individually;
        // that would flood the graph for little downstream benefit.
        for cap in enum_re().captures_iter(content) {
            let Some(name_m) = cap.get(1) else { continue };
            let name = name_m.as_str();
            let line_start = line_for_offset(content, cap.get(0).unwrap().start());
            let (body, body_start) = extract_block(content, cap.get(0).unwrap().end());
            let line_end = body_start + body.lines().count() as u32;
            symbols.push(ExtractedSymbol {
                name: name.to_string(),
                qualified_name: format!("{file_path}::enum::{name}"),
                label: NodeLabel::Enum,
                line_start,
                line_end,
                is_exported: true,
                visibility: Some("public".to_string()),
                ..Default::default()
            });
        }

        symbols
    }
}

/// Extract the body text between a `{` at `start_offset-1` and its
/// matching `}`. Returns the body plus its starting line number.
/// Nesting-aware: counts `{` / `}` so a model containing a nested
/// block (e.g. `@@index`) doesn't terminate early.
fn extract_block(content: &str, start_offset: usize) -> (&str, u32) {
    let bytes = content.as_bytes();
    let mut depth: i32 = 1;
    let mut i = start_offset;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    let body_end = i.saturating_sub(1);
    let body = &content[start_offset..body_end];
    let body_start_line = line_for_offset(content, start_offset);
    (body, body_start_line)
}

fn line_for_offset(content: &str, offset: usize) -> u32 {
    content[..offset.min(content.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count() as u32
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_models_and_fields() {
        let src = r#"
generator client {
  provider = "prisma-client-js"
}

model User {
  id    Int    @id @default(autoincrement())
  email String @unique
  posts Post[]
}

model Post {
  id       Int    @id
  title    String
  authorId Int
}
"#;
        let provider = PrismaProvider;
        let symbols = provider.extract_symbols("schema.prisma", src);
        let models: Vec<&str> = symbols
            .iter()
            .filter(|s| matches!(s.label, NodeLabel::Class))
            .map(|s| s.name.as_str())
            .collect();
        assert!(models.contains(&"User"));
        assert!(models.contains(&"Post"));

        let user_fields: Vec<&str> = symbols
            .iter()
            .filter(|s| {
                matches!(s.label, NodeLabel::Property)
                    && s.parent.as_deref() == Some("schema.prisma::model::User")
            })
            .map(|s| s.name.as_str())
            .collect();
        assert!(user_fields.contains(&"id"));
        assert!(user_fields.contains(&"email"));
        assert!(user_fields.contains(&"posts"));
    }

    #[test]
    fn extracts_enums() {
        let src = r#"
enum Role {
  USER
  ADMIN
}
"#;
        let provider = PrismaProvider;
        let symbols = provider.extract_symbols("schema.prisma", src);
        let enums: Vec<&str> = symbols
            .iter()
            .filter(|s| matches!(s.label, NodeLabel::Enum))
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(enums, vec!["Role"]);
    }
}
