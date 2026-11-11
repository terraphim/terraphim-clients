//! Exit-code contract for `terraphim-grep` (#450).
//!
//! Callers must be able to tell "no match" from "match" without parsing stdout:
//! ripgrep's convention is 0 = match, 1 = no match, 2 = usage/IO error. Before
//! the fix every successful search exited 0, so `grep ... || fallback` could
//! never fire.

use std::process::Command;

fn grep_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-grep")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(grep_binary())
        .args(args)
        .env("RUST_LOG", "error")
        // Keep the run hermetic: a stray key must not change exit semantics.
        .env_remove("OPENROUTER_API_KEY")
        .output()
        .expect("failed to run terraphim-grep")
}

#[test]
fn exits_zero_when_a_query_matches() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(tmp.path().join("sample.rs"), "fn exact_match_marker() {}\n").unwrap();

    let output = run(&[
        "exact_match_marker",
        "--search-only",
        "--haystack",
        "code",
        "--json",
        "--paths",
        tmp.path().to_str().unwrap(),
    ]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "a matching query must exit 0\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn exits_one_when_a_query_matches_nothing() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(tmp.path().join("sample.rs"), "fn unrelated() {}\n").unwrap();

    let output = run(&[
        "aQueryThatMatchesNothingXYZ123",
        "--search-only",
        "--haystack",
        "code",
        "--json",
        "--paths",
        tmp.path().to_str().unwrap(),
    ]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a no-match query must exit 1\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    // The JSON is still printed: the exit code is the cheap signal, not the
    // only one.
    let result: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should stay valid JSON on no match");
    assert_eq!(
        result["chunks"].as_array().map(Vec::len),
        Some(0),
        "no-match run must report zero chunks"
    );
}

#[test]
fn exits_one_when_the_haystack_excludes_every_match() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(
        tmp.path().join("sample.rs"),
        "fn haystack_excluded_marker() {}\n",
    )
    .unwrap();

    // The term exists, but only in source; `docs` must exclude it and the exit
    // code must say so (#404 + #450 together).
    let output = run(&[
        "haystack_excluded_marker",
        "--search-only",
        "--haystack",
        "docs",
        "--json",
        "--paths",
        tmp.path().to_str().unwrap(),
    ]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "a haystack that excludes every match must exit 1\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
