//! Markdown link extraction.
//!
//! Parses `[text](./target.md)` / `[text](../dir/target.md#heading)`
//! style links out of markdown source so the parsing phase can emit
//! IMPORTS edges between Section / File nodes. This is the piece
//! GitNexus covers in `markdown-processor.ts` that our current
//! extractor skips.
//!
//! We only care about **relative** links. External `https://` URLs,
//! mailto, and anchor-only `#heading` references never become
//! cross-file edges in the graph — they're documentation, not
//! structure.

use std::sync::OnceLock;

use regex::Regex;

/// One extracted link to another document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLink {
    /// The link's display text (between the square brackets).
    pub text: String,
    /// The relative file path portion of the target — everything
    /// before the `#fragment`. For `./guide.md#setup` this is
    /// `./guide.md`.
    pub target: String,
    /// Optional heading fragment after `#`, for Section-level
    /// resolution.
    pub fragment: Option<String>,
    /// 1-based source line of the link.
    pub line: u32,
}

static LINK_RE: OnceLock<Regex> = OnceLock::new();

fn link_re() -> &'static Regex {
    // `[text](target)` — text can contain anything except `]` on the
    // same line, target is parenthesis-scoped. We pick up escaped
    // `\]` by matching non-greedy inside the brackets; bracketed
    // targets like `![alt](image.png)` are filtered out explicitly
    // below since `!` precedes the `[`.
    LINK_RE.get_or_init(|| {
        Regex::new(r#"(^|[^!])\[([^\]]+)\]\(([^)]+)\)"#).expect("static regex")
    })
}

/// Extract every relative markdown link from `content`. Absolute URLs
/// (any target starting with a scheme like `http://`) and bare
/// fragment references (`#heading`) are filtered out because they
/// don't carry cross-file structure.
pub fn extract(content: &str) -> Vec<MarkdownLink> {
    let mut out = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        for cap in link_re().captures_iter(line) {
            let text = cap
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let target_raw = cap
                .get(3)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            if target_raw.is_empty() {
                continue;
            }
            if is_external(&target_raw) || target_raw.starts_with('#') {
                continue;
            }
            let (target, fragment) = split_fragment(&target_raw);
            out.push(MarkdownLink {
                text,
                target,
                fragment,
                line: (line_idx + 1) as u32,
            });
        }
    }
    out
}

fn is_external(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with("ftp://")
        || target.starts_with("file://")
}

fn split_fragment(target: &str) -> (String, Option<String>) {
    match target.split_once('#') {
        Some((path, frag)) => (path.to_string(), Some(frag.to_string())),
        None => (target.to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_relative_link() {
        let links = extract("See [the guide](./guide.md) for details.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "the guide");
        assert_eq!(links[0].target, "./guide.md");
        assert!(links[0].fragment.is_none());
    }

    #[test]
    fn captures_fragment() {
        let links = extract("Read [setup](./guide.md#installation) first.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "./guide.md");
        assert_eq!(links[0].fragment, Some("installation".to_string()));
    }

    #[test]
    fn filters_external_links() {
        let links = extract("Visit [our site](https://example.com) or [email](mailto:x@y.com).");
        assert!(links.is_empty());
    }

    #[test]
    fn filters_image_links() {
        let links = extract("![alt text](./image.png)");
        assert!(links.is_empty());
    }

    #[test]
    fn filters_bare_anchor() {
        let links = extract("Jump to [top](#top).");
        assert!(links.is_empty());
    }

    #[test]
    fn line_numbers_are_one_based() {
        let src = "\n\n[x](./x.md)\n";
        let links = extract(src);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].line, 3);
    }

    #[test]
    fn multiple_links_per_line() {
        let links = extract("[a](./a.md) and [b](./b.md)");
        assert_eq!(links.len(), 2);
    }
}
