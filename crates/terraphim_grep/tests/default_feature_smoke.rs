//! Regression guard: a default-feature build of terraphim-grep must return
//! non-zero chunks for a query that matches a file.
//!
//! This is the explicit CI guard for the silent zero-chunk regression
//! documented in terraphim/terraphim-ai#3025 / #4325: if the `code-search`
//! feature is ever removed from the `default` set, `search_code()` compiles
//! to a no-op stub (`Ok(vec![])`) and the CLI silently returns
//! `{chunks:[], latency:0, exit:0}` -- success-with-zero-items. This test
//! fails loudly in that case.
//!
//! Distinct from `no_thesaurus_cli.rs`, which guards KG-absent fallback
//! behaviour. This test's single purpose is the default-feature contract.

use std::process::Command;

#[test]
fn default_feature_build_returns_nonzero_chunks() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("smoke_target.rs");
    std::fs::write(&file_path, "fn smoke_target_match() { /* hit */ }\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_terraphim-grep");

    let output = Command::new(bin)
        .args([
            "smoke_target_match",
            "--json",
            "--haystack",
            "code",
            "--paths",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to run terraphim-grep");

    assert!(
        output.status.success(),
        "terraphim-grep should exit 0 on a default-feature build\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    let chunks = result["chunks"]
        .as_array()
        .expect("JSON result should contain a chunks array");

    assert!(
        !chunks.is_empty(),
        "DEFAULT-FEATURE REGRESSION (terraphim/terraphim-ai#3025): \
         terraphim-grep returned 0 chunks for a query that matches a file. \
         Is `code-search` still in the `default` feature set?"
    );
}
