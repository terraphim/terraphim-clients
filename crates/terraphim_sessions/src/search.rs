//! BM25-ranked session search adapter with KG hybrid boost
//!
//! Converts sessions into `terraphim_types::Document` instances and uses
//! the existing BM25 scoring infrastructure for ranked full-text search.
//!
//! When the `enrichment` feature is also enabled and a `Thesaurus` is provided,
//! sessions matching knowledge graph concepts for the current role are boosted
//! above pure BM25 results (hybrid search).

use crate::model::{MessageRole, Session};
use terraphim_types::score::{OkapiBM25Scorer, Query, QueryScorer, Scored, Scorer, SearchResults};
use terraphim_types::{Document, DocumentType};

const MAX_BODY_LENGTH: usize = 50_000;
const MAX_SEARCH_RESULTS: usize = 50;
const MIN_SCORE_FRACTION: f64 = 0.1;

/// Score multiplier applied to sessions with KG concept matches.
#[cfg(feature = "enrichment")]
const KG_BOOST_MULTIPLIER: f64 = 10_000.0;

/// Adapter that converts a `Session` into a searchable `Document`.
pub fn session_to_document(session: &Session) -> Document {
    let title = session
        .title
        .clone()
        .unwrap_or_else(|| session.source.clone());

    let body = build_body(session);

    Document {
        id: session.id.clone(),
        url: session.source_path.to_string_lossy().to_string(),
        title,
        body,
        description: session.summary(),
        summarization: None,
        stub: None,
        tags: if session.metadata.tags.is_empty() {
            None
        } else {
            Some(session.metadata.tags.clone())
        },
        rank: None,
        source_haystack: Some(session.source.clone()),
        doc_type: DocumentType::default(),
        synonyms: None,
        route: None,
        priority: None,
        quality_score: None,
    }
}

fn build_body(session: &Session) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(path) = &session.metadata.project_path {
        parts.push(path.clone());
    }

    if let Some(model) = &session.metadata.model {
        parts.push(model.clone());
    }

    for msg in &session.messages {
        if msg.content.is_empty() {
            continue;
        }
        let prefix = match msg.role {
            MessageRole::User => "User: ",
            MessageRole::Assistant => "Assistant: ",
            MessageRole::System => "System: ",
            MessageRole::Tool => "Tool: ",
            MessageRole::Other => "",
        };
        parts.push(format!("{}{}", prefix, msg.content));
    }

    let body = parts.join("\n");
    if body.len() > MAX_BODY_LENGTH {
        let mut end = MAX_BODY_LENGTH;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body[..end].to_string()
    } else {
        body
    }
}

/// Perform BM25-ranked search over sessions.
///
/// Returns sessions ranked by relevance to the query, with BM25 scoring
/// applied to the combined title + message body text.
pub fn search_sessions(sessions: &[Session], query: &str) -> Vec<Scored<Session>> {
    if query.trim().is_empty() || sessions.is_empty() {
        return Vec::new();
    }

    let documents: Vec<Document> = sessions.iter().map(session_to_document).collect();

    let mut bm25 = OkapiBM25Scorer::new();
    bm25.initialize(&documents);

    let mut q = Query::new(query);
    q.name_scorer = QueryScorer::BM25;
    q.size = MAX_SEARCH_RESULTS;

    let mut scorer = Scorer::new()
        .with_scorer(Box::new(bm25))
        .with_similarity(terraphim_types::score::Similarity::None);

    let results: SearchResults<Document> = match scorer.score(&q, documents) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("BM25 scoring failed: {}", e);
            return Vec::new();
        }
    };

    let session_map: std::collections::HashMap<&str, &Session> =
        sessions.iter().map(|s| (s.id.as_str(), s)).collect();

    let mut scored: Vec<Scored<Session>> = results
        .as_slice()
        .iter()
        .filter_map(|scored_doc| {
            let score = scored_doc.score();
            session_map
                .get(scored_doc.value().id.as_str())
                .map(|session| Scored::new((*session).clone()).with_score(score))
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score()
            .partial_cmp(&a.score())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if let Some(top_score) = scored.first().map(|s| s.score())
        && top_score > 0.0
    {
        let threshold = top_score * MIN_SCORE_FRACTION;
        scored.retain(|s| s.score() >= threshold);
    }

    scored
}

/// Hybrid search: KG concept matches for the current role come first, BM25 fallback.
///
/// When a `Thesaurus` is provided, the query is matched against KG terms via
/// Aho-Corasick. Sessions whose enrichment data contains matching concepts
/// receive a large score boost so they always rank above pure BM25 results.
///
/// Without a thesaurus (or when no sessions are enriched), falls back to pure BM25.
#[cfg(feature = "enrichment")]
pub fn search_sessions_hybrid(
    sessions: &[Session],
    query: &str,
    thesaurus: Option<terraphim_types::Thesaurus>,
) -> Vec<Scored<Session>> {
    if query.trim().is_empty() || sessions.is_empty() {
        return Vec::new();
    }

    let mut scored = search_sessions(sessions, query);

    let Some(thesaurus) = thesaurus else {
        return scored;
    };

    let kg_terms = match extract_kg_terms(query, &thesaurus) {
        Ok(terms) if !terms.is_empty() => terms,
        _ => return scored,
    };

    let kg_term_set: std::collections::HashSet<String> = kg_terms
        .iter()
        .map(|t| t.normalized_term.value.as_str().to_lowercase())
        .collect();

    if kg_term_set.is_empty() {
        return scored;
    }

    for scored_session in &mut scored {
        let session = scored_session.value();
        let boost = compute_kg_boost(session, &kg_term_set);
        if boost > 0 {
            let new_score = scored_session.score() + (boost as f64 * KG_BOOST_MULTIPLIER);
            scored_session.set_score(new_score);
        }
    }

    scored.sort_by(|a, b| {
        b.score()
            .partial_cmp(&a.score())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    scored
}

#[cfg(feature = "enrichment")]
fn extract_kg_terms(
    query: &str,
    thesaurus: &terraphim_types::Thesaurus,
) -> Result<Vec<terraphim_automata::matcher::Matched>, terraphim_automata::TerraphimAutomataError> {
    terraphim_automata::matcher::find_matches(query, thesaurus, false)
}

#[cfg(feature = "enrichment")]
fn compute_kg_boost(session: &Session, kg_term_set: &std::collections::HashSet<String>) -> usize {
    let Some(enrichment) = &session.metadata.enrichment else {
        return 0;
    };

    let mut match_count = 0usize;
    for (normalized, concept) in &enrichment.concepts {
        if kg_term_set.contains(&normalized.to_lowercase()) {
            match_count += concept.count;
        }
    }

    match_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Message, SessionMetadata};
    use std::path::PathBuf;

    fn make_session(id: &str, title: &str, messages: Vec<(&str, MessageRole, &str)>) -> Session {
        Session {
            id: id.to_string(),
            source: "test".to_string(),
            external_id: id.to_string(),
            title: if title.is_empty() {
                None
            } else {
                Some(title.to_string())
            },
            source_path: PathBuf::from(format!("/sessions/{}.jsonl", id)),
            started_at: None,
            ended_at: None,
            messages: messages
                .into_iter()
                .enumerate()
                .map(|(i, (role, role_type, content))| {
                    let mut msg = Message::text(i, role_type, content);
                    msg.author = Some(role.to_string());
                    msg
                })
                .collect(),
            metadata: SessionMetadata::default(),
        }
    }

    #[test]
    fn test_session_to_document_basic() {
        let session = make_session(
            "s1",
            "Rust async help",
            vec![("user", MessageRole::User, "How do I use tokio?")],
        );
        let doc = session_to_document(&session);

        assert_eq!(doc.id, "s1");
        assert_eq!(doc.title, "Rust async help");
        assert!(doc.body.contains("How do I use tokio?"));
        assert!(doc.body.contains("User: "));
        assert_eq!(doc.source_haystack, Some("test".to_string()));
    }

    #[test]
    fn test_session_to_document_no_title() {
        let session = make_session("s2", "", vec![]);
        let doc = session_to_document(&session);

        assert_eq!(doc.title, "test");
    }

    #[test]
    fn test_session_to_document_tags() {
        let mut session = make_session("s3", "test", vec![]);
        session.metadata.tags = vec!["rust".to_string(), "async".to_string()];
        let doc = session_to_document(&session);

        assert_eq!(
            doc.tags,
            Some(vec!["rust".to_string(), "async".to_string()])
        );
    }

    #[test]
    fn test_search_sessions_basic() {
        let sessions = vec![
            make_session(
                "s1",
                "Rust async programming",
                vec![("user", MessageRole::User, "How to use async await in Rust?")],
            ),
            make_session(
                "s2",
                "Python web scraping",
                vec![("user", MessageRole::User, "Best library for web scraping?")],
            ),
            make_session(
                "s3",
                "Rust error handling",
                vec![("user", MessageRole::User, "How to handle errors in Rust?")],
            ),
        ];

        let results = search_sessions(&sessions, "Rust async");

        assert!(!results.is_empty());
        assert_eq!(results[0].value().id, "s1");
    }

    #[test]
    fn test_search_sessions_empty_query() {
        let sessions = vec![make_session("s1", "test", vec![])];
        let results = search_sessions(&sessions, "");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_sessions_empty_sessions() {
        let results = search_sessions(&[], "test query");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_sessions_ranking_order() {
        let sessions = vec![
            make_session(
                "s1",
                "Rust async",
                vec![
                    ("user", MessageRole::User, "Rust async Rust async Rust"),
                    ("assistant", MessageRole::Assistant, "async rust patterns"),
                ],
            ),
            make_session(
                "s2",
                "General programming",
                vec![("user", MessageRole::User, "What is async?")],
            ),
            make_session(
                "s3",
                "Unrelated",
                vec![("user", MessageRole::User, "How to bake bread?")],
            ),
        ];

        let results = search_sessions(&sessions, "Rust async");

        assert!(!results.is_empty());
        assert!(results.len() <= 3);

        for window in results.windows(2) {
            assert!(window[0].score() >= window[1].score());
        }
    }

    #[test]
    fn test_build_body_truncation() {
        let long_content = "x".repeat(60_000);
        let session = make_session(
            "s1",
            "test",
            vec![("user", MessageRole::User, long_content.as_str())],
        );

        let body = build_body(&session);
        assert_eq!(body.len(), MAX_BODY_LENGTH);
    }

    #[test]
    fn test_build_body_includes_metadata() {
        let mut session = make_session("s1", "test", vec![]);
        session.metadata.project_path = Some("/my/project".to_string());
        session.metadata.model = Some("claude-3".to_string());

        let body = build_body(&session);
        assert!(body.contains("/my/project"));
        assert!(body.contains("claude-3"));
    }

    #[test]
    fn test_build_body_truncation_multibyte_utf8() {
        let emoji = "🎉";
        let emoji_bytes = emoji.len();
        let count = (MAX_BODY_LENGTH / emoji_bytes) + 10;
        let long_content: String = emoji.repeat(count);
        let session = make_session(
            "s1",
            "test",
            vec![("user", MessageRole::User, long_content.as_str())],
        );

        let body = build_body(&session);
        assert!(body.len() <= MAX_BODY_LENGTH + emoji_bytes);
        assert!(body.is_char_boundary(body.len()));
        assert!(!body.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Cass-parity hybrid KG-boost suite (issue #151).
//
// Covers parity rows C07(p)/C62/T15/T18/E17a from
// docs/plans/research-session-test-parity-2026-09.md — `search_with_thesaurus`
// previously had zero tests. Per the design's assert rule: ORDERING and boost
// direction only, never score equality (fusion math is implementation detail).
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "enrichment"))]
mod hybrid_tests {
    use super::*;
    use crate::model::{MessageRole, Session};
    use crate::search_tests_support::{
        make_enriched_session, make_session as mk_session, make_thesaurus,
    };

    fn make_session(id: &str, title: &str, messages: Vec<(&str, MessageRole, &str)>) -> Session {
        mk_session(id, title, messages)
    }

    /// Two sessions; s2 wins on raw BM25 for the query. s1 carries the
    /// thesaurus concept. The hybrid boost must promote s1 above s2.
    fn boost_fixture() -> (Vec<Session>, terraphim_types::Thesaurus) {
        let sessions = vec![
            make_enriched_session(
                "s1",
                "tokio runtime notes",
                vec![("user", MessageRole::User, "runtime setup walkthrough")],
                &[("tokio", 3)],
            ),
            make_session(
                "s2",
                "tokio runtime configuration deep dive",
                vec![(
                    "user",
                    MessageRole::User,
                    "tokio runtime configuration for workers",
                )],
            ),
        ];
        let thesaurus = make_thesaurus(&[("tokio", 1)]);
        (sessions, thesaurus)
    }

    /// TC-SEARCH-01 (P0): thesaurus-matching session ranks above the
    /// higher-raw-BM25 session.
    #[test]
    fn hybrid_boost_promotes_kg_session_over_pure_bm25() {
        let (sessions, thesaurus) = boost_fixture();

        // Sanity: without the thesaurus the raw-BM25 leader is s2.
        let plain = search_sessions(&sessions, "tokio runtime");
        assert!(!plain.is_empty(), "plain search must hit the corpus");
        assert_eq!(
            plain[0].value().id,
            "s2",
            "fixture precondition: s2 leads raw BM25"
        );

        let hybrid = search_sessions_hybrid(&sessions, "tokio runtime", Some(thesaurus.clone()));
        assert!(!hybrid.is_empty());
        assert_eq!(
            hybrid[0].value().id,
            "s1",
            "KG concept boost must promote the enriched session above raw BM25 leader"
        );
    }

    /// TC-SEARCH-02: boost is monotone in thesaurus match count.
    #[test]
    fn hybrid_boost_monotone_in_match_count() {
        let sessions = vec![
            make_enriched_session(
                "a1",
                "one match",
                vec![("user", MessageRole::User, "rust ecosystem")],
                &[("rust", 1)],
            ),
            make_enriched_session(
                "a2",
                "two matches",
                vec![("user", MessageRole::User, "rust async ecosystem")],
                &[("rust", 2), ("async", 2)],
            ),
        ];
        let thesaurus = make_thesaurus(&[("rust", 1), ("async", 2)]);
        let results = search_sessions_hybrid(&sessions, "rust async", Some(thesaurus.clone()));
        assert!(!results.is_empty());
        assert_eq!(
            results[0].value().id,
            "a2",
            "session matching more thesaurus terms outranks the single-term session"
        );
    }

    /// TC-SEARCH-03: `Some(&empty thesaurus)` degrades to plain BM25
    /// (no KG terms found in query => identical ordering).
    #[test]
    fn hybrid_with_no_matching_terms_equals_plain_bm25() {
        let (sessions, _) = boost_fixture();
        let empty_thesaurus = make_thesaurus(&[("unrelated-term", 9)]);
        let plain = search_sessions(&sessions, "tokio runtime");
        let hybrid =
            search_sessions_hybrid(&sessions, "tokio runtime", Some(empty_thesaurus.clone()));
        let plain_ids: Vec<&str> = plain.iter().map(|s| s.value().id.as_str()).collect();
        let hybrid_ids: Vec<&str> = hybrid.iter().map(|s| s.value().id.as_str()).collect();
        assert_eq!(plain_ids, hybrid_ids);
    }

    /// TC-SEARCH-03b: `None` thesaurus degrades to plain BM25, no panic.
    #[test]
    fn hybrid_none_thesaurus_is_plain_bm25() {
        let (sessions, _) = boost_fixture();
        let plain = search_sessions(&sessions, "tokio runtime");
        let hybrid = search_sessions_hybrid(&sessions, "tokio runtime", None);
        let plain_ids: Vec<&str> = plain.iter().map(|s| s.value().id.as_str()).collect();
        let hybrid_ids: Vec<&str> = hybrid.iter().map(|s| s.value().id.as_str()).collect();
        assert_eq!(plain_ids, hybrid_ids);
    }

    /// TC-SEARCH-05/07: empty query and empty corpus return empty, no panic.
    #[test]
    fn hybrid_empty_query_and_empty_corpus() {
        let thesaurus = make_thesaurus(&[("tokio", 1)]);
        assert!(search_sessions_hybrid(&[], "tokio", Some(thesaurus.clone())).is_empty());
        let sessions = vec![make_enriched_session(
            "s1",
            "t",
            vec![("user", MessageRole::User, "tokio")],
            &[("tokio", 1)],
        )];
        assert!(search_sessions_hybrid(&sessions, "   ", Some(thesaurus.clone())).is_empty());
    }

    /// TC-SEARCH-08: deterministic ordering across repeated calls.
    #[test]
    fn hybrid_ordering_is_deterministic() {
        let (sessions, thesaurus) = boost_fixture();
        let first: Vec<String> =
            search_sessions_hybrid(&sessions, "tokio runtime", Some(thesaurus.clone()))
                .iter()
                .map(|s| s.value().id.clone())
                .collect();
        for _ in 0..5 {
            let again: Vec<String> =
                search_sessions_hybrid(&sessions, "tokio runtime", Some(thesaurus.clone()))
                    .iter()
                    .map(|s| s.value().id.clone())
                    .collect();
            assert_eq!(first, again);
        }
    }

    /// Unenriched sessions are untouched by the boost path: their relative
    /// order among themselves is preserved.
    #[test]
    fn hybrid_leaves_unenriched_sessions_in_bm25_order() {
        let (sessions, thesaurus) = boost_fixture();
        let hybrid = search_sessions_hybrid(&sessions, "tokio runtime", Some(thesaurus.clone()));
        let s2_pos = hybrid
            .iter()
            .position(|s| s.value().id == "s2")
            .expect("unenriched session still present");
        assert_eq!(
            s2_pos,
            hybrid.len() - 1,
            "sole unenriched session stays after the boosted one"
        );
    }

    /// A query whose KG terms match nothing the session carries must not
    /// zero-out the corpus: results still come back (from BM25).
    #[test]
    fn hybrid_no_boost_still_returns_bm25_results() {
        let (sessions, thesaurus) = boost_fixture();
        let hybrid = search_sessions_hybrid(&sessions, "runtime", Some(thesaurus.clone()));
        // "runtime" is not a thesaurus term; results are pure BM25 but present.
        assert!(!hybrid.is_empty());
    }
}
