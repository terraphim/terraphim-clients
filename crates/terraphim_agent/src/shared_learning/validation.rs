//! Input validation for the shared learning system.
//!
//! Lexical validators for the three fields that cross trust boundaries:
//!
//! - `source_agent` and `learning.id` are interpolated into filesystem paths
//!   by `MarkdownLearningStore` (P1-3, lexical half; race-resistant path
//!   containment is deferred to ADR-011).
//! - `wiki_page_name` is passed as a `gitea-robot` subprocess argument (P1-2).
//! - Wiki markdown content is stripped of XSS-capable HTML tags before the
//!   subprocess call (P1-1).
//!
//! All validators return `Result<(), &'static str>` — a minimal API with no
//! per-call allocation; callers map the message into their own error enums.

use crate::shared_learning::redact_secrets;

/// Validate `source_agent` against the safe alphabet.
///
/// **Alphabet**: `^[a-zA-Z0-9_][a-zA-Z0-9_-]{0,99}$`
/// - First character: alphanumeric or underscore (NO `-`, NO `.`)
/// - Subsequent characters: alphanumeric, hyphen, or underscore
/// - Length: 1-100 chars inclusive
/// - Rejected: empty, `> 100` chars, `.`, `..`, leading `-`, `/`, non-ASCII
pub fn validate_source_agent(s: &str) -> Result<(), &'static str> {
    if s.is_empty() {
        return Err("source_agent is empty");
    }
    if s.len() > 100 {
        return Err("source_agent exceeds 100 characters");
    }
    if !s.is_ascii() {
        return Err("source_agent contains non-ASCII characters");
    }
    let mut chars = s.chars();
    let first = chars.next().expect("non-empty checked above");
    if !(first.is_ascii_alphanumeric() || first == '_') {
        return Err("source_agent must start with an ASCII alphanumeric or underscore");
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("source_agent contains characters outside [a-zA-Z0-9_-]");
    }
    Ok(())
}

/// Validate `learning.id` against the safe alphabet.
///
/// **Alphabet**: `^[a-zA-Z0-9_][a-zA-Z0-9_.-]{0,199}$`
/// - First character: alphanumeric or underscore (NO `-`, NO `.`)
/// - Subsequent characters: alphanumeric, hyphen, underscore, or period
/// - Length: 1-200 chars inclusive
/// - Rejected: empty, `> 200` chars, `.` alone, `..`, leading `-`, leading
///   `.`, `/`, non-ASCII
///
/// Note: `.` is permitted in non-leading positions to support versioned
/// learning IDs (`draft.v2`). Bare `.` and `..` are rejected by an explicit
/// post-character-class check (defence in depth; the first-character rule
/// already excludes them).
pub fn validate_learning_id(s: &str) -> Result<(), &'static str> {
    if s.is_empty() {
        return Err("learning id is empty");
    }
    if s.len() > 200 {
        return Err("learning id exceeds 200 characters");
    }
    if !s.is_ascii() {
        return Err("learning id contains non-ASCII characters");
    }
    if s == "." || s == ".." {
        return Err("learning id must not be '.' or '..'");
    }
    let mut chars = s.chars();
    let first = chars.next().expect("non-empty checked above");
    if !(first.is_ascii_alphanumeric() || first == '_') {
        return Err("learning id must start with an ASCII alphanumeric or underscore");
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err("learning id contains characters outside [a-zA-Z0-9_.-]");
    }
    Ok(())
}

/// Validate `wiki_page_name` against the safe alphabet.
///
/// **Alphabet**: `^[a-zA-Z0-9_][a-zA-Z0-9_-]{0,199}$`
/// - First character: alphanumeric or underscore (NO leading `-` to avoid
///   option-identifier injection into gitea-robot subprocess argv)
/// - Subsequent characters: alphanumeric, hyphen, or underscore
/// - Length: 1-200 chars inclusive
/// - Rejected: empty, `> 200` chars, `.`, `..`, leading `-`, `/`, non-ASCII
pub fn validate_wiki_page_name(s: &str) -> Result<(), &'static str> {
    if s.is_empty() {
        return Err("wiki page name is empty");
    }
    if s.len() > 200 {
        return Err("wiki page name exceeds 200 characters");
    }
    if !s.is_ascii() {
        return Err("wiki page name contains non-ASCII characters");
    }
    let mut chars = s.chars();
    let first = chars.next().expect("non-empty checked above");
    if !(first.is_ascii_alphanumeric() || first == '_') {
        return Err("wiki page name must start with an ASCII alphanumeric or underscore");
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("wiki page name contains characters outside [a-zA-Z0-9_-]");
    }
    Ok(())
}

/// Remove all occurrences of `<TAG ...>...</TAG>` (case-insensitive) from
/// `content`. A self-closing `<TAG/>` is treated as an opening tag. An
/// unclosed opening tag strips everything to end-of-string. A closing tag
/// without a matching opener is left unchanged.
///
/// Matching is ASCII-case-insensitive and byte-exact:
/// `to_ascii_lowercase` only rewrites ASCII bytes, so byte offsets in the
/// lowered copy align 1:1 with the original even for multibyte content.
fn strip_tag_block(content: &str, tag: &str) -> String {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let lower = content.to_ascii_lowercase();

    let mut out = String::with_capacity(content.len());
    let mut pos = 0usize;

    while pos < content.len() {
        match lower[pos..].find(open_pat.as_str()) {
            None => {
                out.push_str(&content[pos..]);
                break;
            }
            Some(rel_start) => {
                let abs_start = pos + rel_start;
                out.push_str(&content[pos..abs_start]);
                let search_from = abs_start + open_pat.len();
                match lower[search_from..].find(close_pat.as_str()) {
                    Some(rel_close) => {
                        pos = search_from + rel_close + close_pat.len();
                    }
                    None => {
                        // Unclosed opening tag: strip to end-of-string.
                        pos = content.len();
                    }
                }
            }
        }
    }
    out
}

/// Strip `<script>`, `<iframe>`, `<object>`, and `<embed>` blocks from
/// `content`. Case-insensitive, multi-occurrence. Self-closing tags are
/// treated as opening tags; unclosed tags strip to end-of-string.
/// Ordinary markdown (headings, code fences, tables, benign angle brackets
/// like `Result<T>`) is preserved unchanged.
pub fn strip_dangerous_tags(content: &str) -> String {
    static DANGEROUS_TAGS: &[&str] = &["script", "iframe", "object", "embed"];
    let mut result = content.to_string();
    for tag in DANGEROUS_TAGS {
        result = strip_tag_block(&result, tag);
    }
    result
}

/// Pipeline used by `sync_learning`: applies `redact_secrets` (Refs #178)
/// then `strip_dangerous_tags` (P1-1). Tests exercise this function directly
/// to verify the same byte sequence the production code passes to
/// `gitea-robot`.
pub fn preprocess_wiki_content(content: &str) -> String {
    strip_dangerous_tags(&redact_secrets(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- validate_source_agent ----

    #[test]
    fn validate_source_agent_accepts_valid() {
        assert!(validate_source_agent("agent-123").is_ok());
        assert!(validate_source_agent("agent").is_ok());
        assert!(validate_source_agent("_agent").is_ok());
    }

    #[test]
    fn validate_source_agent_rejects_empty() {
        assert!(validate_source_agent("").is_err());
    }

    #[test]
    fn validate_source_agent_rejects_slash() {
        assert!(validate_source_agent("a/b").is_err());
        assert!(validate_source_agent("../etc").is_err());
    }

    #[test]
    fn validate_source_agent_rejects_dotdot() {
        assert!(validate_source_agent("..").is_err());
        assert!(validate_source_agent("agent.id").is_err());
        assert!(validate_source_agent("-agent").is_err());
    }

    #[test]
    fn validate_source_agent_rejects_too_long() {
        assert!(validate_source_agent(&"a".repeat(101)).is_err());
        assert!(validate_source_agent(&"a".repeat(100)).is_ok());
    }

    // ---- validate_learning_id ----

    #[test]
    fn validate_learning_id_accepts_uuid() {
        assert!(validate_learning_id("550e8400-e29b-41d4-a716-446655440000").is_ok());
        assert!(validate_learning_id("learning-uuid-1234").is_ok());
    }

    #[test]
    fn validate_learning_id_accepts_dot_in_middle() {
        assert!(validate_learning_id("learning.draft").is_ok());
        assert!(validate_learning_id("draft.v2").is_ok());
        assert!(validate_learning_id("_id").is_ok());
    }

    #[test]
    fn validate_learning_id_rejects_dotdot() {
        assert!(validate_learning_id("..").is_err());
        assert!(validate_learning_id(".").is_err());
        assert!(validate_learning_id("-id").is_err());
        assert!(validate_learning_id(".id").is_err());
        assert!(validate_learning_id("a/b").is_err());
    }

    #[test]
    fn validate_learning_id_rejects_too_long() {
        assert!(validate_learning_id(&"a".repeat(201)).is_err());
        assert!(validate_learning_id(&"a".repeat(200)).is_ok());
    }

    #[test]
    fn validate_learning_id_rejects_non_ascii() {
        assert!(validate_learning_id("learning✨").is_err());
        assert!(validate_learning_id("").is_err());
    }

    // ---- validate_wiki_page_name ----

    #[test]
    fn validate_wiki_page_name_accepts_valid() {
        assert!(validate_wiki_page_name("page_name-123").is_ok());
        assert!(validate_wiki_page_name("_page").is_ok());
    }

    #[test]
    fn validate_wiki_page_name_rejects_leading_dash() {
        // Option-identifier injection into gitea-robot argv.
        assert!(validate_wiki_page_name("-page").is_err());
        assert!(validate_wiki_page_name("-flag").is_err());
    }

    #[test]
    fn validate_wiki_page_name_rejects_dotdot() {
        assert!(validate_wiki_page_name("../etc").is_err());
        assert!(validate_wiki_page_name("..").is_err());
        assert!(validate_wiki_page_name("page.with.dot").is_err());
        assert!(validate_wiki_page_name("page/id").is_err());
    }

    #[test]
    fn validate_wiki_page_name_rejects_empty() {
        assert!(validate_wiki_page_name("").is_err());
    }

    #[test]
    fn validate_wiki_page_name_rejects_too_long_and_non_ascii() {
        assert!(validate_wiki_page_name(&"a".repeat(201)).is_err());
        assert!(validate_wiki_page_name(&"a".repeat(200)).is_ok());
        assert!(validate_wiki_page_name("page✨").is_err());
    }

    // ---- strip_dangerous_tags ----

    #[test]
    fn strip_dangerous_tags_strips_script() {
        assert_eq!(strip_dangerous_tags("<script>alert(1)</script>"), "");
    }

    #[test]
    fn strip_dangerous_tags_strips_iframe() {
        assert_eq!(
            strip_dangerous_tags(r#"<iframe src="evil"></iframe>"#),
            ""
        );
    }

    #[test]
    fn strip_dangerous_tags_strips_object() {
        assert_eq!(strip_dangerous_tags("<object data=x></object>"), "");
    }

    #[test]
    fn strip_dangerous_tags_strips_embed() {
        assert_eq!(strip_dangerous_tags("<embed src=x></embed>"), "");
    }

    #[test]
    fn strip_dangerous_tags_handles_attributes() {
        assert_eq!(
            strip_dangerous_tags(r#"<script src="evil.js">alert(1)</script>"#),
            ""
        );
    }

    #[test]
    fn strip_dangerous_tags_handles_self_closing() {
        // Self-closing treated as opening: strips through the close tag.
        assert_eq!(strip_dangerous_tags("<script/>alert(1)</script>"), "");
    }

    #[test]
    fn strip_dangerous_tags_handles_unclosed_to_eos() {
        assert_eq!(strip_dangerous_tags("<script>alert(1)"), "");
        assert_eq!(
            strip_dangerous_tags("safe text <script>alert(1)"),
            "safe text "
        );
    }

    #[test]
    fn strip_dangerous_tags_handles_mixed_case() {
        assert_eq!(strip_dangerous_tags("<Script>alert(1)</SCRIPT>"), "");
        assert_eq!(strip_dangerous_tags("<IFRAME>x</iFrAmE>"), "");
    }

    #[test]
    fn strip_dangerous_tags_handles_nested() {
        assert_eq!(
            strip_dangerous_tags("<iframe><script>alert(1)</script></iframe>"),
            ""
        );
    }

    #[test]
    fn strip_dangerous_tags_ignores_malformed_close() {
        // Closing tag with no opener is left unchanged.
        assert_eq!(
            strip_dangerous_tags("</script>alert(1)"),
            "</script>alert(1)"
        );
    }

    #[test]
    fn strip_dangerous_tags_handles_multiple() {
        assert_eq!(
            strip_dangerous_tags("<script>a</script>keep<iframe>b</iframe>tail"),
            "keeptail"
        );
    }

    #[test]
    fn strip_dangerous_tags_preserves_benign() {
        let benign = "We use Result<T> not unwrap(), and status <200> is fine.";
        assert_eq!(strip_dangerous_tags(benign), benign);
    }

    #[test]
    fn strip_dangerous_tags_handles_multibyte_safely() {
        let input = "emoji ✨🚀 before <script>alert(1)</script> and after ✨";
        assert_eq!(
            strip_dangerous_tags(input),
            "emoji ✨🚀 before  and after ✨"
        );
    }

    // ---- preprocess_wiki_content ----

    #[test]
    fn preprocess_wiki_content_redacts_then_strips() {
        let input = "AWS_KEY=AKIAIOSFODNN7EXAMPLE <script>alert(1)</script>";
        let out = preprocess_wiki_content(input);
        assert!(
            !out.contains("AKIAIOSFODNN7EXAMPLE"),
            "secret not redacted: {out}"
        );
        assert!(
            !out.to_ascii_lowercase().contains("<script"),
            "script tag not stripped: {out}"
        );
    }
}
