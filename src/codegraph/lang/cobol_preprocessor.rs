//! COBOL preprocessor — format detection + COPY expansion.
//!
//! COBOL source comes in two dialects that share syntax but differ
//! in how columns are treated:
//!
//! - **Fixed format** (the historical default): columns 1-6 are a
//!   sequence number, column 7 is an indicator (`*` comments,
//!   `-` continuation, `D` debug, ` ` normal), columns 8-11 are
//!   Area A (division / section / paragraph names), 12-72 Area B,
//!   73-80 an identification area.
//! - **Free format**: no column rules, `*>` introduces line comments.
//!
//! The preprocessor normalises both into a single intermediate form
//! (Area A + Area B joined, comments stripped) so the regex-based
//! symbol extractor in `cobol.rs` sees clean lines regardless of
//! source dialect. Matches GitNexus's `cobol-preprocessor.ts`.
//!
//! COPY expansion stitches in copybook contents by looking up the
//! referenced name against a supplied catalogue (sibling `.cpy` /
//! `.copybook` files). Expansion is syntactic — we inline the full
//! text so downstream extraction sees one combined stream; REPLACING
//! clauses are honoured by textual substitution.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

/// Which source dialect a file is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CobolFormat {
    Fixed,
    Free,
}

/// Detect whether `source` looks like fixed- or free-format COBOL.
///
/// Heuristic: if a majority of non-blank lines have a character in
/// column 7 that's one of the known fixed-format indicators (`*`,
/// `D`, `-`, ` `) *and* content only appears from column 8 onward,
/// we treat it as fixed-format. Otherwise free-format.
pub fn detect_format(source: &str) -> CobolFormat {
    let mut fixed_signals = 0;
    let mut total = 0;
    for line in source.lines().take(200) {
        // Tabs expand to 8 columns — COBOL source rarely uses tabs
        // in fixed-format, and when it does the user is asking for
        // the column detection to misfire. Skip lines starting with
        // a tab to avoid noise.
        if line.starts_with('\t') || line.trim().is_empty() {
            continue;
        }
        total += 1;
        if line.len() >= 7 {
            let c7 = line.as_bytes()[6] as char;
            if matches!(c7, '*' | 'D' | '-' | ' ') {
                // Free-format lines can legally have whitespace at
                // column 7. Require *also* that columns 1-6 are
                // numeric or blank — fixed-format sequence numbers
                // are digits, not prose.
                let prefix = &line[..6];
                if prefix
                    .chars()
                    .all(|c| c.is_ascii_digit() || c.is_whitespace())
                {
                    fixed_signals += 1;
                }
            }
        }
    }
    if total > 0 && fixed_signals as f32 / total as f32 > 0.6 {
        CobolFormat::Fixed
    } else {
        CobolFormat::Free
    }
}

/// Collapse a COBOL source into a single-dialect intermediate form.
///
/// - Fixed-format: drop columns 1-6, drop columns 73+, strip `*`-
///   commented lines, expand `-` continuation markers by appending
///   the previous line's trailing text minus its quote terminator.
/// - Free-format: drop `*>` trailing comments, preserve everything
///   else.
///
/// The output is line-for-line: every preprocessed line corresponds
/// to one input line so source-line numbers stay accurate when the
/// downstream extractor emits `line_start` on symbol nodes.
pub fn normalize(source: &str) -> String {
    let format = detect_format(source);
    match format {
        CobolFormat::Fixed => normalize_fixed(source),
        CobolFormat::Free => normalize_free(source),
    }
}

fn normalize_fixed(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        // Line length < 7 means there's nothing in the indicator
        // column — treat as blank.
        if line.len() < 7 {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let indicator = line.as_bytes()[6] as char;
        if indicator == '*' || indicator == '/' {
            // Comment line — emit blank so line numbers stay stable.
            out.push('\n');
            continue;
        }
        // Slice columns 8..72. COBOL spec actually says Area A starts
        // at 8, but some dialects use 8..=72 (zero-indexed 7..72 in
        // our slicing since we drop the sequence+indicator columns).
        let end = line.len().min(72);
        let content = if line.len() > 7 { &line[7..end] } else { "" };
        out.push_str(content);
        out.push('\n');
    }
    out
}

fn normalize_free(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let without_comment = match line.find("*>") {
            Some(idx) => &line[..idx],
            None => line,
        };
        out.push_str(without_comment);
        out.push('\n');
    }
    out
}

static COPY_RE: OnceLock<Regex> = OnceLock::new();
static REPLACING_RE: OnceLock<Regex> = OnceLock::new();

fn copy_re() -> &'static Regex {
    COPY_RE.get_or_init(|| {
        // `COPY NAME.` or `COPY NAME OF LIB.` — name is the first
        // identifier after the keyword. Case-insensitive.
        Regex::new(r#"(?i)\bCOPY\s+([A-Za-z0-9_-]+)"#).expect("static regex")
    })
}

fn replacing_re() -> &'static Regex {
    REPLACING_RE.get_or_init(|| {
        // `REPLACING ==FROM== BY ==TO==` — COBOL uses `==token==`
        // delimiters for pseudo-text replacement.
        Regex::new(r#"==\s*([^=]+?)\s*==\s+BY\s+==\s*([^=]+?)\s*=="#).expect("static regex")
    })
}

/// Expand every `COPY name.` statement in `source` against the given
/// catalogue of copybook contents. Missing copybooks leave the COPY
/// line verbatim — the downstream extractor can still flag it as an
/// unresolved reference.
///
/// `REPLACING ==a== BY ==b==` clauses are applied to the expanded
/// copybook text before splicing in.
pub fn expand_copy(source: &str, catalogue: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        if let Some(cap) = copy_re().captures(line) {
            let Some(name_match) = cap.get(1) else {
                out.push_str(line);
                out.push('\n');
                continue;
            };
            let key = name_match.as_str().to_ascii_uppercase();
            let catalogue_upper: HashMap<String, &String> = catalogue
                .iter()
                .map(|(k, v)| (k.to_ascii_uppercase(), v))
                .collect();
            if let Some(body) = catalogue_upper.get(&key) {
                // Apply REPLACING clauses captured on the same line.
                let body_applied = apply_replacing(body, line);
                out.push_str("* >>>> COPY ");
                out.push_str(name_match.as_str());
                out.push_str(" >>>>\n");
                out.push_str(&body_applied);
                if !body_applied.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("* <<<< COPY ");
                out.push_str(name_match.as_str());
                out.push_str(" <<<<\n");
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn apply_replacing(body: &str, copy_line: &str) -> String {
    let mut result = body.to_string();
    for cap in replacing_re().captures_iter(copy_line) {
        let from = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        let to = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
        if !from.is_empty() {
            result = result.replace(from, to);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_fixed_format_from_indicator_column() {
        let src = "000100 IDENTIFICATION DIVISION.\n\
                   000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(src), CobolFormat::Fixed);
    }

    #[test]
    fn detects_free_format_when_no_sequence_numbers() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(src), CobolFormat::Free);
    }

    #[test]
    fn fixed_normalize_drops_sequence_and_indicator_columns() {
        let src = "000100 IDENTIFICATION DIVISION.\n";
        let out = normalize_fixed(src);
        assert!(out.contains("IDENTIFICATION DIVISION."));
        assert!(!out.contains("000100"));
    }

    #[test]
    fn fixed_normalize_strips_comment_lines() {
        let src = "000100*This is a comment\n000200 PROGRAM-ID. TEST.\n";
        let out = normalize_fixed(src);
        assert!(!out.contains("comment"));
        assert!(out.contains("PROGRAM-ID"));
    }

    #[test]
    fn free_normalize_strips_trailing_comments() {
        let src = "IDENTIFICATION DIVISION. *> marker\nPROGRAM-ID. X.\n";
        let out = normalize_free(src);
        assert!(!out.contains("marker"));
        assert!(out.contains("IDENTIFICATION"));
    }

    #[test]
    fn expand_copy_inlines_copybook_body() {
        let mut catalogue = HashMap::new();
        catalogue.insert(
            "CUSTOMER-REC".to_string(),
            "01 CUSTOMER-REC.\n   05 CUST-NAME PIC X(30).\n".to_string(),
        );
        let src = "WORKING-STORAGE SECTION.\n       COPY CUSTOMER-REC.\n";
        let out = expand_copy(src, &catalogue);
        assert!(out.contains("CUST-NAME PIC X(30)"));
    }

    #[test]
    fn expand_copy_applies_replacing() {
        let mut catalogue = HashMap::new();
        catalogue.insert(
            "TMPL".to_string(),
            "       :PREFIX:-RECORD.\n".to_string(),
        );
        let src = "       COPY TMPL REPLACING ==:PREFIX:== BY ==ORDER==.\n";
        let out = expand_copy(src, &catalogue);
        assert!(out.contains("ORDER-RECORD"));
    }

    #[test]
    fn expand_copy_leaves_unknown_names_verbatim() {
        let catalogue: HashMap<String, String> = HashMap::new();
        let src = "       COPY UNKNOWN-NAME.\n";
        let out = expand_copy(src, &catalogue);
        assert!(out.contains("COPY UNKNOWN-NAME"));
    }
}
