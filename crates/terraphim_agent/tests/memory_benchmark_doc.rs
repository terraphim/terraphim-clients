//! Single source of truth for the memory benchmark floor (#263, epic #255).
//!
//! `docs/memory-benchmark.md` quotes the recall@5 floor that
//! `tests/memory_retrieval_quality.rs` asserts from
//! `tests/fixtures/memory_bench/floor.json`. If the two ever disagree the
//! document is lying about what the test enforces, so this test parses the
//! document's marker line `<!-- floor:recall_at_5=<value> -->` and its results
//! table row and asserts both equal `floor.json` exactly.
//!
//! It also checks that the corpus and thesaurus SHA-256 values quoted in the
//! document are the hashes of the committed files, so the document can never
//! describe inputs other than the ones in the tree. No mocks: the real files
//! are read.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use terraphim_agent::memory_bench::sha256_hex;

const FLOOR_MARKER: &str = "<!-- floor:recall_at_5=";
const RESULTS_ROW: &str = "| recall@5 |";

#[derive(Debug, Deserialize)]
struct Floor {
    recall_at_5: f64,
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_dir() -> PathBuf {
    crate_dir()
        .join("tests")
        .join("fixtures")
        .join("memory_bench")
}

fn doc_path() -> PathBuf {
    crate_dir()
        .join("..")
        .join("..")
        .join("docs")
        .join("memory-benchmark.md")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The single line in `doc` that starts with `prefix`; panics when the line
/// is missing or appears more than once, so the marker cannot silently drift.
fn single_line<'a>(doc: &'a str, prefix: &str) -> &'a str {
    let lines: Vec<&str> = doc
        .lines()
        .filter(|line| line.trim_start().starts_with(prefix))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "docs/memory-benchmark.md must contain exactly one line starting with {prefix:?}, found {}",
        lines.len()
    );
    lines[0].trim()
}

fn floor_from_json() -> Floor {
    let path = fixture_dir().join("floor.json");
    serde_json::from_str(&read(&path)).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn parse_value(raw: &str, what: &str) -> f64 {
    raw.trim()
        .parse::<f64>()
        .unwrap_or_else(|e| panic!("{what} value {raw:?} is not a number: {e}"))
}

#[test]
fn doc_floor_marker_equals_floor_json() {
    let doc = read(&doc_path());
    let line = single_line(&doc, FLOOR_MARKER);
    let raw = line
        .strip_prefix(FLOOR_MARKER)
        .and_then(|rest| rest.strip_suffix("-->"))
        .unwrap_or_else(|| panic!("malformed floor marker line: {line:?}"));
    let quoted = parse_value(raw, "floor marker");

    let floor = floor_from_json();
    assert_eq!(
        quoted, floor.recall_at_5,
        "docs/memory-benchmark.md quotes recall@5 floor {quoted} but floor.json holds {}",
        floor.recall_at_5
    );
}

#[test]
fn doc_results_table_recall_at_5_equals_floor_json() {
    let doc = read(&doc_path());
    let line = single_line(&doc, RESULTS_ROW);
    let cells: Vec<&str> = line.split('|').map(str::trim).collect();
    // A row `| recall@5 | 0.04 |` splits into ["", "recall@5", "0.04", ""].
    let value = cells
        .get(2)
        .unwrap_or_else(|| panic!("results row has no value cell: {line:?}"));
    let quoted = parse_value(value, "results table recall@5");

    let floor = floor_from_json();
    assert_eq!(
        quoted, floor.recall_at_5,
        "results table quotes recall@5 {quoted} but floor.json holds {}",
        floor.recall_at_5
    );
}

#[test]
fn doc_quotes_the_committed_corpus_and_thesaurus_hashes() {
    let doc = read(&doc_path());
    for file in ["corpus.jsonl", "thesaurus.json", "queries.jsonl"] {
        let path = fixture_dir().join(file);
        let hash = sha256_hex(&fs::read(&path).unwrap_or_else(|e| panic!("read {file}: {e}")));
        assert!(
            doc.contains(&hash),
            "docs/memory-benchmark.md does not quote the SHA-256 of the committed {file} ({hash})"
        );
    }
}
