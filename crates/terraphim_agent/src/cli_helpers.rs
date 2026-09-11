//! Small CLI/output formatting and UI-building helpers extracted from `main.rs`.
//!
//! Originally part of the monolithic `main.rs`; moved here as step 1 of the
//! de-monolithization tracked in terraphim/terraphim-clients#211.
//!
//! These helpers have no shared mutable state with the dispatch logic in
//! `main.rs` and are reusable by `repl/handler.rs` and `service.rs`.

use ratatui::{
    style::{Color, Style},
    widgets::{Block, Borders},
};

/// Truncate a snippet at a UTF-8 char boundary, appending "..." when truncated.
///
/// Naive `&s[..max]` panics when `max` lands inside a multi-byte char (e.g. typographic
/// quotes from email subjects). This walks char boundaries and stops at the last one
/// whose byte index is ≤ max.
pub(crate) fn truncate_snippet(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let cutoff = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_bytes)
        .last()
        .unwrap_or(0);
    format!("{}...", &s[..cutoff])
}

#[cfg(test)]
mod truncate_snippet_tests {
    use super::truncate_snippet;

    #[test]
    fn short_string_unchanged() {
        assert_eq!(truncate_snippet("hello", 120), "hello");
    }

    #[test]
    fn ascii_truncated() {
        let s = "a".repeat(200);
        let out = truncate_snippet(&s, 120);
        assert!(out.ends_with("..."));
        assert_eq!(out.len(), 123);
    }

    #[test]
    fn multibyte_does_not_panic() {
        // Reproduces crates/terraphim_agent/src/main.rs:1414 panic where
        // `&s[..120]` landed inside a typographic quote (3 bytes: e2 80 9c).
        let s = "Includes dependencies for llama.cpp, integration with retreival, and CLI/GUI flows; the project positions itself as \u{201C}ultimate open-source RAG app\u{201D} with curated features.";
        let out = truncate_snippet(s, 120);
        // Must not panic and must be a valid UTF-8 string ending in "..."
        assert!(out.ends_with("..."));
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn cyrillic_safe() {
        let s = "консенсус ".repeat(20);
        let out = truncate_snippet(&s, 120);
        assert!(out.ends_with("..."));
    }
}

/// Format the one-line stderr explainability message emitted when the search
/// command auto-routes (i.e. the user did not pass `--role`).
///
/// Exact format pinned by the design (section 5):
///   `[auto-route] picked role "<name>" (score=<n>, candidates=<m>); to override, pass --role`
pub(crate) fn format_auto_route_line(
    result: &terraphim_service::auto_route::AutoRouteResult,
) -> String {
    format!(
        "[auto-route] picked role \"{}\" (score={}, candidates={}); to override, pass --role",
        result.role.as_str(),
        result.score,
        result.candidates.len(),
    )
}

#[cfg(test)]
mod format_auto_route_line_tests {
    use super::format_auto_route_line;
    use terraphim_service::auto_route::{AutoRouteReason, AutoRouteResult};
    use terraphim_types::RoleName;

    #[test]
    fn pinned_exact_format() {
        let r = AutoRouteResult {
            role: RoleName::new("Personal Assistant"),
            score: 42,
            candidates: vec![
                (RoleName::new("Personal Assistant"), 42),
                (RoleName::new("Default"), 0),
            ],
            reason: AutoRouteReason::ScoredWinner,
        };
        assert_eq!(
            format_auto_route_line(&r),
            "[auto-route] picked role \"Personal Assistant\" (score=42, candidates=2); to override, pass --role"
        );
    }
}

/// Check if a character is a word boundary character (not alphanumeric).
pub(crate) fn is_word_boundary_char(c: char) -> bool {
    !c.is_alphanumeric() && c != '_'
}

/// Check if a match position is at word boundaries in the text.
/// Returns true if the character before start (or start of string) and
/// the character after end (or end of string) are word boundary characters.
pub(crate) fn is_at_word_boundary(text: &str, start: usize, end: usize) -> bool {
    // Check character before start
    let before_ok = if start == 0 {
        true
    } else {
        text[..start]
            .chars()
            .last()
            .map(is_word_boundary_char)
            .unwrap_or(true)
    };

    // Check character after end
    let after_ok = if end >= text.len() {
        true
    } else {
        text[end..]
            .chars()
            .next()
            .map(is_word_boundary_char)
            .unwrap_or(true)
    };

    before_ok && after_ok
}

/// Format a replacement link from a NormalizedTerm and LinkType.
pub(crate) fn format_replacement_link(
    term: &terraphim_types::NormalizedTerm,
    link_type: terraphim_hooks::LinkType,
) -> String {
    let display_text = term.display();
    match link_type {
        terraphim_hooks::LinkType::WikiLinks => format!("[[{}]]", display_text),
        terraphim_hooks::LinkType::HTMLLinks => format!(
            "<a href=\"{}\">{}</a>",
            term.url.as_deref().unwrap_or_default(),
            display_text
        ),
        terraphim_hooks::LinkType::MarkdownLinks => format!(
            "[{}]({})",
            display_text,
            term.url.as_deref().unwrap_or_default()
        ),
        terraphim_hooks::LinkType::PlainText => display_text.to_string(),
    }
}

/// Create a transparent style for UI elements
pub(crate) fn transparent_style() -> Style {
    Style::default().bg(Color::Reset)
}

/// Create a block with optional transparent background
pub(crate) fn create_block(title: &str, transparent: bool) -> Block<'_> {
    let block = Block::default().title(title).borders(Borders::ALL);

    if transparent {
        block.style(transparent_style())
    } else {
        block
    }
}

#[cfg(test)]
mod word_boundary_tests {
    use super::{is_at_word_boundary, is_word_boundary_char};

    #[test]
    fn test_is_word_boundary_char() {
        // Non-alphanumeric chars are boundaries
        assert!(is_word_boundary_char(' '));
        assert!(is_word_boundary_char('\t'));
        assert!(is_word_boundary_char('\n'));
        assert!(is_word_boundary_char('.'));
        assert!(is_word_boundary_char(','));
        assert!(is_word_boundary_char('('));
        assert!(is_word_boundary_char(')'));
        assert!(is_word_boundary_char('"'));

        // Alphanumeric chars are NOT boundaries
        assert!(!is_word_boundary_char('a'));
        assert!(!is_word_boundary_char('Z'));
        assert!(!is_word_boundary_char('0'));
        assert!(!is_word_boundary_char('9'));

        // Underscore is NOT a boundary (word char in most regex)
        assert!(!is_word_boundary_char('_'));
    }

    #[test]
    fn test_is_at_word_boundary_start_of_string() {
        // At start of string, "npm" should be at boundary
        let text = "npm install";
        assert!(is_at_word_boundary(text, 0, 3)); // "npm" at start
    }

    #[test]
    fn test_is_at_word_boundary_end_of_string() {
        // At end of string, "npm" should be at boundary
        let text = "install npm";
        assert!(is_at_word_boundary(text, 8, 11)); // "npm" at end
    }

    #[test]
    fn test_is_at_word_boundary_middle_with_spaces() {
        // In middle with spaces, "npm" should be at boundary
        let text = "run npm install";
        assert!(is_at_word_boundary(text, 4, 7)); // "npm" surrounded by spaces
    }

    #[test]
    fn test_is_at_word_boundary_not_at_boundary() {
        // "npm" embedded in "anpmb" should NOT be at boundary
        let text = "anpmb";
        assert!(!is_at_word_boundary(text, 1, 4)); // "npm" embedded
    }

    #[test]
    fn test_is_at_word_boundary_partial_boundary() {
        // "npm" at start but not end: "npma"
        let text = "npma";
        assert!(!is_at_word_boundary(text, 0, 3)); // "npm" no boundary after

        // "npm" at end but not start: "anpm"
        let text2 = "anpm";
        assert!(!is_at_word_boundary(text2, 1, 4)); // "npm" no boundary before
    }

    #[test]
    fn test_is_at_word_boundary_with_punctuation() {
        // Punctuation counts as boundary
        let text = "(npm)";
        assert!(is_at_word_boundary(text, 1, 4)); // "npm" between parens

        let text2 = "use npm, please";
        assert!(is_at_word_boundary(text2, 4, 7)); // "npm" followed by comma
    }
}
