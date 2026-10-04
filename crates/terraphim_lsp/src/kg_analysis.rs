//! KG analysis engine: Aho-Corasick term matching for knowledge-graph markdown documents.
//!
//! Provides `analyse_kg_document()` which identifies known KG terms in a text
//! with their byte positions and hover information, enabling LSP diagnostics and
//! hover support for terraphim knowledge-graph authoring.

use terraphim_automata::find_matches;
use terraphim_types::Thesaurus;

/// A single KG term matched within a document.
#[derive(Debug, Clone, PartialEq)]
pub struct TermMatch {
    /// The matched term string as it appears in the text.
    pub term: String,
    /// Start byte offset in the source text.
    pub start: usize,
    /// End byte offset in the source text (exclusive).
    pub end: usize,
    /// Hover documentation shown in the editor for this term.
    pub hover_info: String,
}

/// Result of analysing a document against the knowledge-graph thesaurus.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KgAnalysis {
    /// Terms found in the document that are present in the thesaurus.
    pub matched_terms: Vec<TermMatch>,
    /// Words longer than `MIN_WORD_LEN` not matched by any thesaurus entry.
    /// Useful for surfacing potential new KG terms to document authors.
    pub unknown_terms: Vec<String>,
}

/// Minimum word length to include in `unknown_terms`.
const MIN_WORD_LEN: usize = 3;

/// Analyse `text` against the knowledge-graph `thesaurus`.
///
/// Returns matched KG terms with byte positions and hover info, plus a list
/// of words not found in the thesaurus (candidates for future KG additions).
///
/// # Example
///
/// ```
/// use terraphim_types::{Thesaurus, NormalizedTermValue, NormalizedTerm};
/// use terraphim_lsp::kg_analysis::analyse_kg_document;
///
/// let mut thesaurus = Thesaurus::new("test".to_string());
/// let key = NormalizedTermValue::new("rust".to_string());
/// let term = NormalizedTerm::new(1, key.clone());
/// thesaurus.insert(key, term);
///
/// let analysis = analyse_kg_document("I love Rust programming.", &thesaurus);
/// assert!(!analysis.matched_terms.is_empty());
/// ```
pub fn analyse_kg_document(text: &str, thesaurus: &Thesaurus) -> KgAnalysis {
    // find_matches takes ownership; clone the reference input.
    let raw_matches = match find_matches(text, thesaurus, true) {
        Ok(m) => m,
        Err(e) => {
            log::warn!("kg_analysis: find_matches failed: {e}");
            return KgAnalysis::default();
        }
    };

    let matched_terms: Vec<TermMatch> = raw_matches
        .iter()
        .filter_map(|m| {
            let (start, end) = m.pos?;
            // Use the actual text slice for display; m.term is the normalised pattern.
            let display_text = text.get(start..end).unwrap_or(&m.term);
            Some(TermMatch {
                term: display_text.to_string(),
                start,
                end,
                hover_info: build_hover_info(display_text, m),
            })
        })
        .collect();

    // Collect the byte-ranges of all matched terms so they can be excluded
    // from the unknown-terms list.
    let matched_ranges: std::collections::HashSet<(usize, usize)> =
        matched_terms.iter().map(|t| (t.start, t.end)).collect();

    // Build a set of matched term strings (lowercase) for fast lookup.
    let matched_lower: std::collections::HashSet<String> = matched_terms
        .iter()
        .map(|t| t.term.to_lowercase())
        .collect();

    // Unknown terms: words not covered by any matched term.
    let mut unknown_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut byte_offset = 0usize;
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        let word_len = word.len();
        let word_start = byte_offset;
        let word_end = byte_offset + word_len;

        if word_len >= MIN_WORD_LEN {
            let lower = word.to_lowercase();
            let overlaps = matched_ranges
                .iter()
                .any(|&(s, e)| !(word_end <= s || word_start >= e));
            if !overlaps && !matched_lower.contains(&lower) {
                unknown_set.insert(word.to_string());
            }
        }

        // Advance past the word and the following delimiter (if any).
        byte_offset += word_len;
        if byte_offset < text.len() {
            byte_offset += text[byte_offset..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
        }
    }
    let mut unknown_terms: Vec<String> = unknown_set.into_iter().collect();
    unknown_terms.sort();

    KgAnalysis {
        matched_terms,
        unknown_terms,
    }
}

fn build_hover_info(display_text: &str, m: &terraphim_automata::Matched) -> String {
    let url_part = m
        .normalized_term
        .url
        .as_deref()
        .map(|url| format!("\n\nSee: {url}"))
        .unwrap_or_default();
    format!("**{display_text}**: KG term{url_part}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use terraphim_types::{NormalizedTerm, NormalizedTermValue};

    fn make_thesaurus(terms: &[&str]) -> Thesaurus {
        let mut t = Thesaurus::new("test".to_string());
        for (i, term) in terms.iter().enumerate() {
            let key = NormalizedTermValue::new(term.to_string());
            let nterm = NormalizedTerm::new(i as u64 + 1, key.clone());
            t.insert(key, nterm);
        }
        t
    }

    #[test]
    fn empty_text_returns_empty_analysis() {
        let thesaurus = make_thesaurus(&["rust"]);
        let analysis = analyse_kg_document("", &thesaurus);
        assert!(analysis.matched_terms.is_empty());
        assert!(analysis.unknown_terms.is_empty());
    }

    #[test]
    fn matched_term_has_position() {
        let thesaurus = make_thesaurus(&["rust"]);
        let text = "I love Rust programming.";
        let analysis = analyse_kg_document(text, &thesaurus);
        assert!(!analysis.matched_terms.is_empty(), "should match 'rust'");
        let m = &analysis.matched_terms[0];
        // The matched substring should be the correct slice of the text.
        assert_eq!(m.term.to_lowercase(), "rust");
        assert_eq!(&text[m.start..m.end], &text[m.start..m.end]);
        assert!(m.start < m.end);
    }

    #[test]
    fn hover_info_contains_term_name() {
        let thesaurus = make_thesaurus(&["rust"]);
        let analysis = analyse_kg_document("Rust is great.", &thesaurus);
        assert!(!analysis.matched_terms.is_empty());
        assert!(analysis.matched_terms[0].hover_info.contains("Rust"));
    }

    #[test]
    fn hover_info_includes_url_when_present() {
        let mut thesaurus = Thesaurus::new("test".to_string());
        let key = NormalizedTermValue::new("cargo".to_string());
        let nterm = NormalizedTerm::new(1, key.clone())
            .with_url("https://doc.rust-lang.org/cargo/".to_string());
        thesaurus.insert(key, nterm);

        let analysis = analyse_kg_document("Use cargo to build.", &thesaurus);
        assert!(!analysis.matched_terms.is_empty());
        let hover = &analysis.matched_terms[0].hover_info;
        assert!(
            hover.contains("https://doc.rust-lang.org/cargo/"),
            "hover should have url"
        );
    }

    #[test]
    fn unknown_terms_excludes_matched_terms() {
        let thesaurus = make_thesaurus(&["rust"]);
        let analysis = analyse_kg_document("Rust programming language", &thesaurus);
        let unknown_lower: Vec<String> = analysis
            .unknown_terms
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        assert!(
            !unknown_lower.contains(&"rust".to_string()),
            "matched term should not appear in unknown_terms"
        );
    }

    #[test]
    fn short_words_not_in_unknown_terms() {
        let thesaurus = make_thesaurus(&["rust"]);
        // "is" and "a" are too short to be unknown terms
        let analysis = analyse_kg_document("Rust is a language", &thesaurus);
        for w in &analysis.unknown_terms {
            assert!(w.len() >= 3, "word '{w}' is shorter than MIN_WORD_LEN");
        }
    }

    #[test]
    fn multiple_matches_in_same_text() {
        let thesaurus = make_thesaurus(&["rust", "cargo"]);
        let analysis = analyse_kg_document("Rust uses cargo for builds.", &thesaurus);
        assert!(analysis.matched_terms.len() >= 2, "both terms should match");
    }

    #[test]
    fn empty_thesaurus_gives_no_matches() {
        let thesaurus = Thesaurus::new("empty".to_string());
        let analysis = analyse_kg_document("Rust is great.", &thesaurus);
        assert!(analysis.matched_terms.is_empty());
        // unknown_terms may be non-empty with an empty thesaurus
    }

    #[test]
    fn analyse_never_panics_on_unicode() {
        let thesaurus = make_thesaurus(&["rust"]);
        // Multi-byte unicode: should not panic
        let result = std::panic::catch_unwind(|| {
            analyse_kg_document("Rust merhaba مرحبا 你好 Rust", &thesaurus)
        });
        assert!(
            result.is_ok(),
            "analyse_kg_document panicked on unicode input"
        );
    }
}
