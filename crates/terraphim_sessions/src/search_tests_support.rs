//! Shared test-support builders for the cass-parity session-search suite.
//!
//! Deterministic, tempdir-based corpus builders so tests never read real user
//! session stores (`~/.claude`, `~/.codex`, `~/.cursor`, ...). Refs the
//! session-test-suite design: docs/plans/design-session-test-suite-2026-09.md
//! (issue #150) and the parity research artefact
//! docs/plans/research-session-test-parity-2026-09.md.
//!
//! Gate: `#[cfg(test)]` only — never compiled into release builds.

#![cfg(test)]

use crate::model::{Message, MessageRole, Session, SessionMetadata};
use std::path::{Path, PathBuf};

#[cfg(feature = "enrichment")]
use crate::enrichment::{ConceptMatch, ConceptOccurrence, SessionConcepts};
#[cfg(feature = "enrichment")]
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

/// Build a minimal session with `count` alternating user/assistant messages.
pub fn make_session(id: &str, title: &str, messages: Vec<(&str, MessageRole, &str)>) -> Session {
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

/// Build a session with KG enrichment concepts attached (enrichment feature).
///
/// `concepts` are `(normalized_term, occurrence_count)` pairs; each concept
/// gets `count` synthetic occurrences spread over the first messages.
#[cfg(feature = "enrichment")]
pub fn make_enriched_session(
    id: &str,
    title: &str,
    messages: Vec<(&str, MessageRole, &str)>,
    concepts: &[(&str, u64)],
) -> Session {
    let mut session = make_session(id, title, messages);
    let mut sc = SessionConcepts::default();
    for (term, count) in concepts {
        let mut cm = ConceptMatch::new(
            term.to_string(),
            term.to_string(),
            0,
            None,
        );
        for i in 0..*count {
            cm.add_occurrence(ConceptOccurrence {
                message_idx: 0,
                start_pos: 0,
                end_pos: 0,
                context: None,
            });
            let _ = i; // occurrence index unused; count drives the boost
        }
        cm.count = *count as usize;
        sc.insert_or_update(cm);
    }
    session.metadata.enrichment = Some(sc);
    session
}

/// Build a `Thesaurus` whose terms the automata matcher can find.
///
/// `terms` are `(normalized_term, term_id)` pairs. IDs must be unique and
/// non-zero. Term values are lowercased via `NormalizedTermValue::new`.
#[cfg(feature = "enrichment")]
pub fn make_thesaurus(terms: &[(&str, u64)]) -> Thesaurus {
    let mut thesaurus = Thesaurus::new("parity-fixture".to_string());
    for (term, id) in terms {
        thesaurus.insert(
            NormalizedTermValue::new(term.to_string()),
            NormalizedTerm::new(*id, NormalizedTermValue::new(term.to_string())),
        );
    }
    thesaurus
}

/// Write a Claude Code–style JSONL transcript into `dir` (creating parents).
///
/// `entries` are raw JSON objects; the native connector skips malformed lines,
/// so callers can deliberately include garbage entries for tolerance tests.
pub fn write_claude_jsonl(dir: &Path, name: &str, entries: &[serde_json::Value]) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create fixture dir");
    let path = dir.join(name);
    let body = entries
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, body).expect("write jsonl fixture");
    path
}

/// A well-formed Claude Code user entry (session_meta + user message).
pub fn claude_user_entry(session_id: &str, cwd: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "user",
        "sessionId": session_id,
        "cwd": cwd,
        "message": { "role": "user", "content": text }
    })
}

/// A well-formed Claude Code assistant entry.
pub fn claude_assistant_entry(session_id: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "assistant",
        "sessionId": session_id,
        "message": { "role": "assistant", "content": text }
    })
}

/// Write an Aider-style chat history markdown file into `dir`.
///
/// `turns` are `(prompt, response)` pairs rendered as `#### prompt` /
/// `> response` blocks, the shape `AiderConnector::parse` expects.
pub fn write_aider_history(dir: &Path, turns: &[(&str, &str)]) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create aider fixture dir");
    let path = dir.join(".aider.chat.history.md");
    let mut body = String::new();
    for (prompt, response) in turns {
        body.push_str(&format!("#### {prompt}\n\n> {response}\n\n"));
    }
    std::fs::write(&path, body).expect("write aider fixture");
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_session_defaults() {
        let s = make_session(
            "s1",
            "Rust async",
            vec![
                ("user", MessageRole::User, "hello rust"),
                ("assistant", MessageRole::Assistant, "hi there"),
            ],
        );
        assert_eq!(s.id, "s1");
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.title.as_deref(), Some("Rust async"));
    }

    #[test]
    fn make_session_empty_title_falls_back() {
        let s = make_session("s2", "", vec![]);
        assert!(s.title.is_none());
    }

    #[cfg(feature = "enrichment")]
    #[test]
    fn enriched_session_carries_concepts() {
        let s = make_enriched_session(
            "e1",
            "tokio deep dive",
            vec![("user", MessageRole::User, "explain tokio")],
            &[("tokio", 3)],
        );
        let enr = s.metadata.enrichment.expect("enrichment attached");
        assert_eq!(enr.concepts.len(), 1);
        let c = enr.concepts.values().next().expect("one concept");
        assert_eq!(c.count, 3);
    }

    #[cfg(feature = "enrichment")]
    #[test]
    fn thesaurus_terms_are_findable_by_automata() {
        let thesaurus = make_thesaurus(&[("tokio", 1), ("rust", 2)]);
        let matches =
            terraphim_automata::matcher::find_matches("I love Tokio and Rust", &thesaurus, false)
                .expect("matcher works");
        let terms: Vec<String> = matches
            .iter()
            .map(|m| m.normalized_term.value.as_str().to_string())
            .collect();
        assert!(terms.contains(&"tokio".to_string()));
        assert!(terms.contains(&"rust".to_string()));
    }

    #[test]
    fn claude_jsonl_fixture_is_parseable_json_per_line() {
        let tmp = std::env::temp_dir().join(format!("parity-harness-{}", std::process::id()));
        let path = write_claude_jsonl(
            &tmp,
            "s1.jsonl",
            &[
                claude_user_entry("abc", "/proj", "hello"),
                claude_assistant_entry("abc", "hi"),
            ],
        );
        let content = std::fs::read_to_string(&path).expect("read fixture");
        for line in content.lines() {
            let v: serde_json::Value = serde_json::from_str(line).expect("valid JSONL line");
            assert!(v.is_object());
        }
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn aider_fixture_shape() {
        let tmp = std::env::temp_dir().join(format!("parity-aider-{}", std::process::id()));
        let path = write_aider_history(&tmp, &[("how to bun", "use bun install")]);
        let content = std::fs::read_to_string(&path).expect("read aider fixture");
        assert!(content.contains("#### how to bun"));
        assert!(content.contains("> use bun install"));
        std::fs::remove_dir_all(&tmp).ok();
    }
}
