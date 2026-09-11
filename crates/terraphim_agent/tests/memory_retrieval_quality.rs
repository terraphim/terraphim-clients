//! Judge-free retrieval quality regression test (#260, epic #255).
//!
//! Loads the committed fixture (`tests/fixtures/memory_bench/`, from #259) and
//! the committed Terraphim Engineer thesaurus, runs every query through the
//! unchanged `memory_retrieve::retrieve` via `memory_bench::evaluate`, asserts
//! recall@5 is at or above the recorded floor in `floor.json`, and writes the
//! full report to `target/memory-benchmark/report.json`.
//!
//! The floor is committed by hand from a real run, never written by this test:
//! a write-if-missing floor would hide a regression on a fresh checkout.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use terraphim_agent::memory_bench::{evaluate, load_fixture, load_thesaurus, sha256_hex};
use terraphim_config::Role;

const ROLE_NAME: &str = "Terraphim Engineer";
const THESAURUS_FILE: &str = "thesaurus.json";
const FLOOR_FILE: &str = "floor.json";

/// Recorded floor for recall@5, committed next to the fixture.
#[derive(Debug, Deserialize)]
struct Floor {
    recall_at_5: f64,
    recorded_at: String,
    terraphim_agent_version: String,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("memory_bench")
}

/// `target/memory-benchmark/` under the workspace target directory.
fn report_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("target")
        });
    target.join("memory-benchmark")
}

#[test]
fn retrieval_quality_meets_floor() {
    let dir = fixture_dir();
    let fixture = load_fixture(&dir).expect("committed fixture must load");
    let thesaurus_path = dir.join(THESAURUS_FILE);
    let thesaurus = load_thesaurus(&thesaurus_path).expect("committed thesaurus must load");
    let thesaurus_sha256 = sha256_hex(&fs::read(&thesaurus_path).expect("read thesaurus"));

    let report = evaluate(&fixture, &Role::new(ROLE_NAME), thesaurus).expect("evaluate");

    // Write the report first so a failing floor still leaves the numbers on disk.
    let out_dir = report_dir();
    fs::create_dir_all(&out_dir).expect("create report dir");
    let report_path = out_dir.join("report.json");
    fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).expect("serialise report"),
    )
    .expect("write report");
    eprintln!(
        "memory benchmark report written to {}",
        report_path.display()
    );
    eprintln!("{report:#?}");

    assert_eq!(report.corpus_size, fixture.items.len());
    assert_eq!(report.query_count, fixture.queries.len());
    assert_eq!(report.corpus_sha256, fixture.corpus_sha256);
    assert_eq!(
        report.thesaurus_sha256, thesaurus_sha256,
        "report must name the committed thesaurus by hash"
    );
    assert_eq!(report.terraphim_agent_version, env!("CARGO_PKG_VERSION"));

    let floor_json = fs::read_to_string(dir.join(FLOOR_FILE))
        .unwrap_or_else(|e| panic!("{FLOOR_FILE} must be committed next to the fixture: {e}"));
    let floor: Floor = serde_json::from_str(&floor_json).expect("floor.json must parse");
    assert!(
        (0.0..=1.0).contains(&floor.recall_at_5),
        "floor out of range: {floor:?}"
    );
    assert!(
        !floor.recorded_at.is_empty() && !floor.terraphim_agent_version.is_empty(),
        "floor must record when and on which version it was taken: {floor:?}"
    );

    assert!(
        report.recall_at_5 >= floor.recall_at_5,
        "recall@5 regressed below the recorded floor: got {} < floor {} (recorded {} on {})",
        report.recall_at_5,
        floor.recall_at_5,
        floor.recorded_at,
        floor.terraphim_agent_version
    );
}

/// The benchmark is deterministic: two evaluations of the committed fixture
/// with the committed thesaurus produce byte-identical reports.
#[test]
fn retrieval_quality_is_deterministic_on_committed_fixture() {
    let dir = fixture_dir();
    let fixture = load_fixture(&dir).expect("committed fixture must load");
    let load = || load_thesaurus(&dir.join(THESAURUS_FILE)).expect("thesaurus");
    let role = Role::new(ROLE_NAME);

    let first = evaluate(&fixture, &role, load()).expect("evaluate");
    let second = evaluate(&fixture, &role, load()).expect("evaluate");
    assert_eq!(first, second);
}
