//! Integration test: terraphim-grep works without a knowledge-graph thesaurus.
//!
//! Verifies that the CLI falls back to `fff-search` enhanced grep mode when no
//! thesaurus is available, returning valid JSON results with empty concepts.

use std::process::Command;

#[test]
fn cli_runs_without_thesaurus() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("sample.rs");
    std::fs::write(&file_path, "fn search_target() { /* found */ }\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_terraphim-grep");

    let output = Command::new(bin)
        .args([
            "search_target",
            "--json",
            "--haystack",
            "code",
            "--paths",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to run terraphim-grep");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "terraphim-grep should succeed without a thesaurus\nstdout: {stdout}\nstderr: {stderr}"
    );

    let result: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert!(
        result.get("chunks").is_some(),
        "JSON result should contain chunks"
    );
    let chunks = result["chunks"].as_array().expect("chunks is an array");
    assert!(
        !chunks.is_empty(),
        "expected at least one fff-search chunk without thesaurus"
    );

    let concepts = result["concepts"].as_array().expect("concepts is an array");
    assert!(
        concepts.is_empty(),
        "expected empty KG concepts without thesaurus"
    );

    assert_eq!(
        result["stats"]["kg_hits"].as_u64(),
        Some(0),
        "kg_hits should be zero"
    );

    // Truthful-stats invariant (blocks release wrapper #3208): the reported
    // counter must always match the number of chunks actually returned, even
    // when the sufficiency heuristic classifies the result as RlmInsufficient
    // (fewer than min_results matches, as in this single-file corpus).
    let chunks_returned = result["stats"]["chunks_returned"]
        .as_u64()
        .expect("chunks_returned is a number") as usize;
    assert_eq!(
        chunks_returned,
        chunks.len(),
        "stats.chunks_returned must equal chunks.len() (got {chunks_returned}, chunks = {})",
        chunks.len()
    );
    assert!(
        chunks_returned >= 1,
        "known-match corpus must report at least one returned chunk"
    );
}
