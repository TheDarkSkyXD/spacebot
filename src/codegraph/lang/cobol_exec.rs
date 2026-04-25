//! COBOL `EXEC SQL` / `EXEC CICS` / `EXEC DLI` block extraction.
//!
//! Mainframe COBOL programs embed non-COBOL languages through
//! `EXEC <dialect> ... END-EXEC.` blocks. These blocks are ignored
//! by the COBOL parser but carry the real business semantics:
//!
//! - `EXEC SQL SELECT ... FROM T END-EXEC.` — SQL queries hitting DB2.
//! - `EXEC CICS LINK PROGRAM('XYZ') END-EXEC.` — CICS transaction calls.
//! - `EXEC DLI GU SEGMENT END-EXEC.` — IMS DL/I database operations.
//!
//! We extract each block as a structured record so the pipeline can
//! emit SqlQuery / CicsCall / DliCall nodes with QUERIES edges from
//! the containing program / paragraph.

use std::sync::OnceLock;

use regex::Regex;

/// Which dialect an `EXEC` block belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecDialect {
    Sql,
    Cics,
    Dli,
}

impl ExecDialect {
    /// Node-table label used for this dialect's `EXEC` blocks.
    pub fn node_label(self) -> &'static str {
        match self {
            Self::Sql => "SqlQuery",
            Self::Cics => "CicsCall",
            Self::Dli => "DliCall",
        }
    }
}

/// One `EXEC dialect ... END-EXEC` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecBlock {
    pub dialect: ExecDialect,
    /// Raw body between the opening `EXEC X` and `END-EXEC` markers.
    /// Trimmed and single-line-normalised.
    pub body: String,
    /// 1-based source line where the `EXEC` keyword starts.
    pub line: u32,
}

static EXEC_RE: OnceLock<Regex> = OnceLock::new();

fn exec_re() -> &'static Regex {
    // Multi-line, case-insensitive match. `[\s\S]` rather than `.`
    // so newlines are consumed inside the block — COBOL EXEC blocks
    // routinely span dozens of lines.
    EXEC_RE.get_or_init(|| {
        Regex::new(
            r#"(?i)\bEXEC\s+(SQL|CICS|DLI)\b([\s\S]*?)END-EXEC"#,
        )
        .expect("static regex")
    })
}

/// Extract every EXEC block from a (preprocessed) COBOL source.
pub fn extract(content: &str) -> Vec<ExecBlock> {
    let mut out = Vec::new();
    for cap in exec_re().captures_iter(content) {
        let dialect_str = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let dialect = match dialect_str.to_ascii_uppercase().as_str() {
            "SQL" => ExecDialect::Sql,
            "CICS" => ExecDialect::Cics,
            "DLI" => ExecDialect::Dli,
            _ => continue,
        };
        let body_raw = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let body = body_raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = content[..cap.get(0).map(|m| m.start()).unwrap_or(0)]
            .bytes()
            .filter(|&b| b == b'\n')
            .count() as u32
            + 1;
        out.push(ExecBlock {
            dialect,
            body,
            line,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_exec_sql_block() {
        let src = r#"
      EXEC SQL
         SELECT NAME FROM CUSTOMER
         WHERE ID = :CUST-ID
      END-EXEC.
"#;
        let blocks = extract(src);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dialect, ExecDialect::Sql);
        assert!(blocks[0].body.contains("SELECT NAME FROM CUSTOMER"));
    }

    #[test]
    fn extracts_exec_cics_block() {
        let src = r#"      EXEC CICS LINK PROGRAM('XYZ01') END-EXEC."#;
        let blocks = extract(src);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dialect, ExecDialect::Cics);
    }

    #[test]
    fn extracts_exec_dli_block() {
        let src = r#"      EXEC DLI GU SEGMENT(CUSTOMER) END-EXEC."#;
        let blocks = extract(src);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dialect, ExecDialect::Dli);
    }

    #[test]
    fn multiple_blocks_per_source() {
        let src = r#"
      EXEC SQL SELECT 1 FROM DUAL END-EXEC.
      EXEC CICS RETURN END-EXEC.
      EXEC SQL SELECT 2 FROM DUAL END-EXEC.
"#;
        let blocks = extract(src);
        assert_eq!(blocks.len(), 3);
    }

    #[test]
    fn dialect_node_labels_match_schema() {
        assert_eq!(ExecDialect::Sql.node_label(), "SqlQuery");
        assert_eq!(ExecDialect::Cics.node_label(), "CicsCall");
        assert_eq!(ExecDialect::Dli.node_label(), "DliCall");
    }
}
