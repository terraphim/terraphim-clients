//! Judge-free retrieval quality benchmark over a committed memory fixture.
//!
//! This module measures what [`crate::memory_retrieve::retrieve`] actually
//! returns, with no LLM anywhere on the path: a fixture of memory items and
//! query-to-expected-id pairs goes in, recall@1, recall@5 and MRR come out.
//! The report names the corpus and thesaurus it ran on by SHA-256 so a number
//! can never be quoted without the inputs that produced it.
//!
//! Ranking is not touched here. Every query goes through the unchanged
//! `retrieve` with `limit = 5`; this module only counts.
//!
//! Metric definitions, per query, with `top_k` the first `k` hit ids:
//!
//! * `recall@k = |expected_ids ∩ top_k| / |expected_ids|`
//! * `reciprocal rank = 1 / (1-based rank of the first hit in expected_ids)`,
//!   or `0` when none of the top five hits is expected.
//!
//! The report carries the arithmetic mean of each over all queries.

use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use terraphim_agent_evolution::MemoryItem;
use terraphim_config::Role;
use terraphim_types::Thesaurus;

use crate::memory_retrieve::retrieve;

/// Number of hits requested per query. recall@5 and MRR are computed over
/// exactly this many hits.
pub const RETRIEVAL_LIMIT: usize = 5;

/// File name of the corpus inside a fixture directory.
pub const CORPUS_FILE: &str = "corpus.jsonl";
/// File name of the query set inside a fixture directory.
pub const QUERIES_FILE: &str = "queries.jsonl";

/// One benchmark query with the item ids a correct retrieval must include.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub query: String,
    pub expected_ids: Vec<String>,
}

/// Fixture loaded from `corpus.jsonl` and `queries.jsonl`.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub items: Vec<MemoryItem>,
    pub queries: Vec<Query>,
    /// SHA-256 of the raw bytes of `corpus.jsonl`, so reports name the corpus
    /// they ran on.
    pub corpus_sha256: String,
    /// SHA-256 of the raw bytes of `queries.jsonl`, so the relevance labels
    /// are part of the provenance as well as the corpus.
    pub queries_sha256: String,
}

/// Judge-free retrieval quality over a fixture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalQualityReport {
    pub corpus_size: usize,
    pub query_count: usize,
    pub recall_at_1: f64,
    pub recall_at_5: f64,
    pub mrr: f64,
    pub corpus_sha256: String,
    pub queries_sha256: String,
    /// SHA-256 of the thesaurus file, taken from `Thesaurus::source_hash`.
    /// Empty when the thesaurus was not loaded through [`load_thesaurus`].
    pub thesaurus_sha256: String,
    pub terraphim_agent_version: String,
}

/// Errors from loading a fixture or running the benchmark.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    #[error("fixture io: {0}")]
    Io(#[from] std::io::Error),
    #[error("fixture parse error at {file}:{line}: {source}")]
    Parse {
        file: String,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("fixture has no {0}")]
    Empty(&'static str),
    #[error(transparent)]
    Retrieve(#[from] anyhow::Error),
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Parse one JSON-lines file into `T`s, naming the file and 1-based line on
/// failure. Blank lines are skipped.
fn read_jsonl<T: serde::de::DeserializeOwned>(
    dir: &Path,
    file: &str,
) -> Result<(Vec<T>, Vec<u8>), BenchError> {
    let bytes = fs::read(dir.join(file))?;
    let mut records = Vec::new();
    for (index, line) in BufReader::new(bytes.as_slice()).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str(&line).map_err(|source| BenchError::Parse {
            file: file.to_string(),
            line: index + 1,
            source,
        })?;
        records.push(record);
    }
    Ok((records, bytes))
}

/// Load a fixture directory containing `corpus.jsonl` and `queries.jsonl`.
///
/// # Errors
/// `BenchError::Io` on unreadable files; `BenchError::Parse` on a malformed
/// line (the file name and line number are included); `BenchError::Empty` if
/// either file has no records or a query lists no expected ids (a query with
/// no expected ids has an undefined recall and would poison the mean).
pub fn load_fixture(dir: &Path) -> Result<Fixture, BenchError> {
    let (items, corpus_bytes): (Vec<MemoryItem>, _) = read_jsonl(dir, CORPUS_FILE)?;
    if items.is_empty() {
        return Err(BenchError::Empty("items"));
    }
    let (queries, queries_bytes): (Vec<Query>, _) = read_jsonl(dir, QUERIES_FILE)?;
    if queries.is_empty() {
        return Err(BenchError::Empty("queries"));
    }
    if queries.iter().any(|q| q.expected_ids.is_empty()) {
        return Err(BenchError::Empty("expected_ids"));
    }
    Ok(Fixture {
        items,
        queries,
        corpus_sha256: sha256_hex(&corpus_bytes),
        queries_sha256: sha256_hex(&queries_bytes),
    })
}

/// Load a thesaurus JSON file and stamp the file's SHA-256 into its
/// `source_hash`, so [`evaluate`] can report which thesaurus it ran on.
///
/// # Errors
/// `BenchError::Io` on an unreadable file; `BenchError::Parse` (line 0) when
/// the JSON is not a thesaurus.
pub fn load_thesaurus(path: &Path) -> Result<Thesaurus, BenchError> {
    let bytes = fs::read(path)?;
    let json = String::from_utf8_lossy(&bytes);
    let thesaurus: Thesaurus = serde_json::from_str(&json).map_err(|source| BenchError::Parse {
        file: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        line: 0,
        source,
    })?;
    Ok(thesaurus.with_source_hash(sha256_hex(&bytes)))
}

/// Per-query scores over one ranked list of hit ids.
fn score_query(hit_ids: &[String], expected_ids: &[String]) -> (f64, f64, f64) {
    let expected: HashSet<&str> = expected_ids.iter().map(String::as_str).collect();
    let recall_at = |k: usize| {
        let found = hit_ids
            .iter()
            .take(k)
            .filter(|id| expected.contains(id.as_str()))
            .count();
        found as f64 / expected.len() as f64
    };
    let reciprocal_rank = hit_ids
        .iter()
        .take(RETRIEVAL_LIMIT)
        .position(|id| expected.contains(id.as_str()))
        .map_or(0.0, |pos| 1.0 / (pos + 1) as f64);
    (recall_at(1), recall_at(RETRIEVAL_LIMIT), reciprocal_rank)
}

/// Run every query through `memory_retrieve::retrieve` with `limit = 5` and
/// aggregate recall@1, recall@5 and MRR. Deterministic for a given fixture and
/// thesaurus: `retrieve` sorts by rank then id before paging.
///
/// # Errors
/// Propagates retrieval errors; returns `BenchError::Empty` for a fixture with
/// no queries or a query with no expected ids.
pub fn evaluate(
    fixture: &Fixture,
    role: &Role,
    thesaurus: Thesaurus,
) -> Result<RetrievalQualityReport, BenchError> {
    if fixture.queries.is_empty() {
        return Err(BenchError::Empty("queries"));
    }
    if fixture.queries.iter().any(|q| q.expected_ids.is_empty()) {
        return Err(BenchError::Empty("expected_ids"));
    }

    let thesaurus_sha256 = thesaurus.source_hash.clone().unwrap_or_default();
    let (mut r1, mut r5, mut rr) = (0.0, 0.0, 0.0);
    for query in &fixture.queries {
        let outcome = retrieve(
            &role.name,
            thesaurus.clone(),
            &fixture.items,
            &query.query,
            None,
            Some(RETRIEVAL_LIMIT),
        )?;
        let hit_ids: Vec<String> = outcome.hits.into_iter().map(|h| h.item.id).collect();
        let (q1, q5, qrr) = score_query(&hit_ids, &query.expected_ids);
        r1 += q1;
        r5 += q5;
        rr += qrr;
    }
    let n = fixture.queries.len() as f64;

    Ok(RetrievalQualityReport {
        corpus_size: fixture.items.len(),
        query_count: fixture.queries.len(),
        recall_at_1: r1 / n,
        recall_at_5: r5 / n,
        mrr: rr / n,
        corpus_sha256: fixture.corpus_sha256.clone(),
        queries_sha256: fixture.queries_sha256.clone(),
        thesaurus_sha256,
        terraphim_agent_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::io::Write;
    use terraphim_agent_evolution::{ImportanceLevel, MemoryItemType};
    use terraphim_types::{NormalizedTerm, NormalizedTermValue};

    fn thesaurus(entries: &[(&str, &str, u64)]) -> Thesaurus {
        let mut t = Thesaurus::new("bench-test".to_string());
        for (synonym, concept, id) in entries {
            t.insert(
                NormalizedTermValue::from(*synonym),
                NormalizedTerm::new(*id, NormalizedTermValue::from(*concept)),
            );
        }
        t
    }

    /// Four concepts, enough to give every item a distinct concept pair.
    fn four_concepts() -> Thesaurus {
        thesaurus(&[
            ("bun", "bun", 1),
            ("install", "install", 2),
            ("cargo", "cargo", 3),
            ("clippy", "clippy", 4),
        ])
    }

    fn memory(id: &str, content: &str) -> MemoryItem {
        MemoryItem {
            id: id.to_string(),
            item_type: MemoryItemType::LessonLearned,
            content: content.to_string(),
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch"),
            last_accessed: None,
            access_count: 0,
            importance: ImportanceLevel::Medium,
            tags: Vec::new(),
            associations: std::collections::HashMap::new(),
        }
    }

    fn role() -> Role {
        Role::new("bench-role")
    }

    /// Write a fixture directory from real serialised items (never hand-written
    /// JSON, so the `MemoryItem` wire shape is whatever serde says it is).
    fn write_fixture(dir: &Path, items: &[MemoryItem], query_lines: &[String]) {
        let mut corpus = fs::File::create(dir.join(CORPUS_FILE)).expect("create corpus");
        for item in items {
            writeln!(
                corpus,
                "{}",
                serde_json::to_string(item).expect("serialise item")
            )
            .expect("write corpus line");
        }
        let mut queries = fs::File::create(dir.join(QUERIES_FILE)).expect("create queries");
        for line in query_lines {
            writeln!(queries, "{line}").expect("write query line");
        }
    }

    #[test]
    fn load_fixture_rejects_empty_queries() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_fixture(dir.path(), &[memory("m1", "bun install")], &[]);

        let err = load_fixture(dir.path()).expect_err("empty queries must be rejected");
        assert!(
            matches!(err, BenchError::Empty("queries")),
            "expected Empty(\"queries\"), got {err:?}"
        );
    }

    #[test]
    fn load_fixture_rejects_query_without_expected_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_fixture(
            dir.path(),
            &[memory("m1", "bun install")],
            &[r#"{"query":"bun install","expected_ids":[]}"#.to_string()],
        );

        let err = load_fixture(dir.path()).expect_err("query with no expected ids");
        assert!(matches!(err, BenchError::Empty("expected_ids")), "{err:?}");
    }

    #[test]
    fn load_fixture_reports_line_on_bad_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_fixture(
            dir.path(),
            &[memory("m1", "bun install")],
            &[
                r#"{"query":"bun install","expected_ids":["m1"]}"#.to_string(),
                "{not json".to_string(),
            ],
        );

        let err = load_fixture(dir.path()).expect_err("malformed line must be rejected");
        match &err {
            BenchError::Parse { file, line, .. } => {
                assert_eq!(file, QUERIES_FILE);
                assert_eq!(*line, 2, "line numbers are 1-based");
            }
            other => panic!("expected Parse, got {other:?}"),
        }
        let message = err.to_string();
        assert!(
            message.contains("queries.jsonl:2"),
            "error message must name file and line: {message}"
        );
    }

    #[test]
    fn load_fixture_hashes_corpus_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_fixture(
            dir.path(),
            &[memory("m1", "bun install")],
            &[r#"{"query":"bun install","expected_ids":["m1"]}"#.to_string()],
        );

        let fixture = load_fixture(dir.path()).expect("valid fixture");
        let bytes = fs::read(dir.path().join(CORPUS_FILE)).expect("read corpus");
        assert_eq!(fixture.corpus_sha256, sha256_hex(&bytes));
        assert_eq!(fixture.corpus_sha256.len(), 64);
        let query_bytes = fs::read(dir.path().join(QUERIES_FILE)).expect("read queries");
        assert_eq!(fixture.queries_sha256, sha256_hex(&query_bytes));
        assert_ne!(fixture.queries_sha256, fixture.corpus_sha256);
        assert_eq!(fixture.items.len(), 1);
        assert_eq!(fixture.queries.len(), 1);
    }

    /// Each item carries a distinct concept pair and each query is the item's
    /// own content, so the expected item is the only one sharing both
    /// concepts with the query and must rank first.
    #[test]
    fn evaluate_perfect_fixture_scores_one() {
        let items = vec![
            memory("a", "bun install"),
            memory("b", "cargo clippy"),
            memory("c", "bun clippy"),
        ];
        let queries = items
            .iter()
            .map(|item| Query {
                query: item.content.clone(),
                expected_ids: vec![item.id.clone()],
            })
            .collect();
        let fixture = Fixture {
            items,
            queries,
            corpus_sha256: "deadbeef".to_string(),
            queries_sha256: "cafebabe".to_string(),
        };

        let report = evaluate(&fixture, &role(), four_concepts()).expect("evaluate");

        assert_eq!(report.recall_at_1, 1.0, "{report:?}");
        assert_eq!(report.recall_at_5, 1.0, "{report:?}");
        assert_eq!(report.mrr, 1.0, "{report:?}");
        assert_eq!(report.corpus_size, 3);
        assert_eq!(report.query_count, 3);
        assert_eq!(report.corpus_sha256, "deadbeef");
        assert_eq!(report.queries_sha256, "cafebabe");
        assert_eq!(report.terraphim_agent_version, env!("CARGO_PKG_VERSION"));
    }

    /// A query whose expected item is not retrievable scores zero, and the
    /// means are taken over every query, not only the ones with hits.
    #[test]
    fn evaluate_partial_fixture_averages_over_all_queries() {
        let items = vec![memory("a", "bun install"), memory("solo", "cargo")];
        let queries = vec![
            Query {
                query: "bun install".to_string(),
                expected_ids: vec!["a".to_string()],
            },
            Query {
                query: "cargo".to_string(),
                expected_ids: vec!["solo".to_string()],
            },
        ];
        let fixture = Fixture {
            items,
            queries,
            corpus_sha256: String::new(),
            queries_sha256: String::new(),
        };

        let report = evaluate(&fixture, &role(), four_concepts()).expect("evaluate");

        assert_eq!(report.recall_at_1, 0.5, "{report:?}");
        assert_eq!(report.recall_at_5, 0.5, "{report:?}");
        assert_eq!(report.mrr, 0.5, "{report:?}");
    }

    #[test]
    fn evaluate_is_deterministic() {
        let items = vec![
            memory("a", "bun install and bun clippy"),
            memory("b", "bun install"),
            memory("c", "cargo clippy then bun install"),
            memory("d", "cargo install"),
        ];
        let queries = vec![
            Query {
                query: "bun install clippy".to_string(),
                expected_ids: vec!["a".to_string(), "c".to_string()],
            },
            Query {
                query: "cargo install".to_string(),
                expected_ids: vec!["d".to_string()],
            },
        ];
        let fixture = Fixture {
            items,
            queries,
            corpus_sha256: String::new(),
            queries_sha256: String::new(),
        };

        let first = evaluate(&fixture, &role(), four_concepts()).expect("evaluate");
        for _ in 0..8 {
            let again = evaluate(&fixture, &role(), four_concepts()).expect("evaluate");
            assert_eq!(again, first, "two runs must produce identical reports");
        }
    }

    #[test]
    fn evaluate_rejects_fixture_without_queries() {
        let fixture = Fixture {
            items: vec![memory("a", "bun install")],
            queries: Vec::new(),
            corpus_sha256: String::new(),
            queries_sha256: String::new(),
        };
        let err = evaluate(&fixture, &role(), four_concepts()).expect_err("no queries");
        assert!(matches!(err, BenchError::Empty("queries")), "{err:?}");
    }

    #[test]
    fn load_thesaurus_stamps_file_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("thesaurus.json");
        let json = serde_json::to_vec(&four_concepts()).expect("serialise thesaurus");
        fs::write(&path, &json).expect("write thesaurus");

        let loaded = load_thesaurus(&path).expect("load thesaurus");
        assert_eq!(
            loaded.source_hash.as_deref(),
            Some(sha256_hex(&json).as_str())
        );
        assert_eq!(loaded.len(), 4);

        let report = evaluate(
            &Fixture {
                items: vec![memory("a", "bun install")],
                queries: vec![Query {
                    query: "bun install".to_string(),
                    expected_ids: vec!["a".to_string()],
                }],
                corpus_sha256: String::new(),
                queries_sha256: String::new(),
            },
            &role(),
            loaded,
        )
        .expect("evaluate");
        assert_eq!(report.thesaurus_sha256, sha256_hex(&json));
    }

    #[test]
    fn load_thesaurus_rejects_non_thesaurus_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("thesaurus.json");
        fs::write(&path, b"[1,2,3]").expect("write");
        let err = load_thesaurus(&path).expect_err("array is not a thesaurus");
        assert!(matches!(err, BenchError::Parse { .. }), "{err:?}");
    }

    fn content_strategy() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                Just("bun"),
                Just("install"),
                Just("cargo"),
                Just("clippy"),
                Just("noise")
            ],
            0..6,
        )
        .prop_map(|words| words.join(" "))
    }

    fn fixture_items(max: usize) -> impl Strategy<Value = Vec<MemoryItem>> {
        proptest::collection::vec(content_strategy(), 0..max).prop_map(|contents| {
            contents
                .into_iter()
                .enumerate()
                .map(|(i, content)| memory(&format!("item-{i}"), &content))
                .collect()
        })
    }

    fn fixture_queries(max: usize) -> impl Strategy<Value = Vec<Query>> {
        proptest::collection::vec(
            (
                content_strategy(),
                proptest::collection::vec(0usize..12, 1..4),
            ),
            0..max,
        )
        .prop_map(|pairs| {
            pairs
                .into_iter()
                .map(|(query, ids)| Query {
                    query,
                    expected_ids: ids.into_iter().map(|i| format!("item-{i}")).collect(),
                })
                .collect()
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn evaluate_never_panics_and_bounds_scores(
            items in fixture_items(12),
            queries in fixture_queries(6),
        ) {
            let fixture = Fixture { items, queries, corpus_sha256: String::new(), queries_sha256: String::new() };
            if let Ok(r) = evaluate(&fixture, &role(), four_concepts()) {
                prop_assert!((0.0..=1.0).contains(&r.recall_at_1), "{r:?}");
                prop_assert!((0.0..=1.0).contains(&r.recall_at_5), "{r:?}");
                prop_assert!((0.0..=1.0).contains(&r.mrr), "{r:?}");
                prop_assert!(r.recall_at_1 <= r.recall_at_5, "{r:?}");
                prop_assert_eq!(r.corpus_size, fixture.items.len());
                prop_assert_eq!(r.query_count, fixture.queries.len());
            }
        }
    }
}
