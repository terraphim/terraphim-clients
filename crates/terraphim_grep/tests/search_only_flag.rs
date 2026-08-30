//! Regression test for #128: `terraphim-grep --search-only` must skip the
//! LLM client build entirely, so a stray `OPENROUTER_API_KEY` cannot cost
//! a single network call. Also guards the freshly-installed binary against
//! silently dropping the flag when the build is out of sync with the
//! source (Refs #128 -- installed binary was 1.21.11, source is 1.21.13).
//!
//! Spawns the compiled binary via `CARGO_BIN_EXE_terraphim-grep`.

use std::process::Command;

fn grep_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-grep")
}

#[test]
fn search_only_flag_is_accepted() {
    // Run from a tempdir so we know what file content is being searched.
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("sample.rs");
    std::fs::write(&target, "fn search_target() { /* found */ }\n").unwrap();

    let output = Command::new(grep_binary())
        .args([
            "--search-only",
            "search_target",
            "--json",
            "--haystack",
            "code",
            "--paths",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to run terraphim-grep --search-only");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "--search-only must be a recognised flag (Refs #128).\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Stderr must include the explicit log line proving the LLM client
    // setup was skipped (Refs terraphim-clients#81).
    assert!(
        stderr.contains("--search-only") || stderr.contains("search-only mode"),
        "expected an info log confirming search-only mode; got: {stderr}"
    );
}

#[test]
fn search_only_skips_llm_client_with_openrouter_key_present() {
    // If OPENROUTER_API_KEY is set in the test environment, the binary
    // would normally try to build an LLM client. With --search-only, it
    // must skip that step entirely. We assert by inspecting stderr for
    // the "skipping LLM client setup" debug log.
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("hello.rs"),
        "fn hello_target() {}\n",
    )
    .unwrap();

    let output = Command::new(grep_binary())
        .args([
            "--search-only",
            "hello_target",
            "--haystack",
            "code",
            "--paths",
            tmp.path().to_str().unwrap(),
        ])
        .env("OPENROUTER_API_KEY", "sk-test-placeholder")
        .output()
        .expect("failed to run terraphim-grep --search-only");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "--search-only must succeed even when OPENROUTER_API_KEY is set"
    );
    assert!(
        stderr.contains("skipping LLM client setup"),
        "--search-only must skip LLM client setup; got stderr: {stderr}"
    );
}

#[test]
fn help_documents_search_only_flag() {
    // Defensive: ensures `--help` mentions --search-only so users
    // can discover it. If a release removes the flag without
    // updating the help text, this test will fail.
    let output = Command::new(grep_binary())
        .arg("--help")
        .output()
        .expect("failed to run terraphim-grep --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--search-only"),
        "--help must document --search-only (Refs #128)"
    );
}
