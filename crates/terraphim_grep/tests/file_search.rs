//! `terraphim-grep --files` searches file *names* (#472).
//!
//! The mode must return ranked absolute paths one per line (or JSON), honour
//! `--paths` scoping, and never require a thesaurus or a content query.

use std::process::Command;

fn grep_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-grep")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(grep_binary())
        .env("RUST_LOG", "error")
        .env_remove("OPENROUTER_API_KEY")
        .args(args)
        .output()
        .expect("failed to run terraphim-grep")
}

struct Fixture {
    _root: tempfile::TempDir,
    plugins: std::path::PathBuf,
    docs: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::TempDir::new().expect("tempdir");
        let plugins = root.path().join("plugins");
        let docs = root.path().join("docs");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(plugins.join("terraphim-grep.js"), "// plugin\n").unwrap();
        std::fs::write(plugins.join("other.js"), "// other\n").unwrap();
        std::fs::write(docs.join("readme.md"), "# docs\n").unwrap();
        Self {
            _root: root,
            plugins,
            docs,
        }
    }
}

#[test]
fn files_mode_ranks_fuzzy_name_matches() {
    let fixture = Fixture::new();
    let output = run(&[
        "tgp",
        "--files",
        "--paths",
        fixture.plugins.to_str().unwrap(),
    ]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "filename search should succeed\nstdout: {stdout}\nstderr: {stderr}"
    );
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert!(
        !lines.is_empty(),
        "tgp should match a file name\nstderr: {stderr}"
    );
    assert!(
        lines[0].ends_with("terraphim-grep.js"),
        "ranked output should put the plugin first, got {lines:?}"
    );
    assert!(
        lines.iter().all(|path| path.starts_with('/')),
        "paths must be absolute, got {lines:?}"
    );
    assert!(
        lines.iter().all(|path| !path.ends_with(".md")),
        "--paths docs must not leak into a plugins search, got {lines:?}"
    );
}

#[test]
fn files_mode_json_reports_path_and_score() {
    let fixture = Fixture::new();
    let output = run(&[
        "terraphim",
        "--files",
        "--json",
        "--paths",
        fixture.plugins.to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON array");
    let entries = parsed.as_array().expect("array");
    assert!(!entries.is_empty(), "expected a match, got {parsed}");
    let first = &entries[0];
    assert!(
        first["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("terraphim-grep.js")),
        "expected the plugin path, got {first}"
    );
    assert!(first["score"].is_i64(), "score must be an integer");
}

#[test]
fn files_mode_scopes_to_the_given_paths() {
    let fixture = Fixture::new();
    let output = run(&[
        "readme",
        "--files",
        "--paths",
        fixture.docs.to_str().unwrap(),
    ]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert!(
        lines
            .iter()
            .all(|path| path.starts_with(fixture.docs.to_str().unwrap())),
        "results must stay inside --paths, got {lines:?}"
    );
}

#[test]
fn files_mode_exits_one_when_nothing_matches() {
    let fixture = Fixture::new();
    let output = run(&[
        "zzzz-no-such-file-name",
        "--files",
        "--paths",
        fixture._root.path().to_str().unwrap(),
    ]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "a no-match filename search must exit 1\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "no-match output must be empty"
    );
}

/// Overlapping `--paths` roots must not produce the same file twice.
///
/// The contract is one path per line, and the limit is a limit on *distinct*
/// files: with overlapping roots the same file is found under each of them,
/// and the duplicates used to eat the limit slots
/// (terraphim-clients#515).
#[test]
fn files_mode_deduplicates_overlapping_roots() {
    let fixture = Fixture::new();
    let output = run(&[
        "js",
        "--files",
        "--paths",
        fixture.plugins.to_str().unwrap(),
        "--paths",
        fixture.plugins.to_str().unwrap(),
    ]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();

    let mut unique = lines.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        lines.len(),
        unique.len(),
        "a path must be listed once, got {lines:?}"
    );
    assert!(
        !unique.is_empty(),
        "the overlapping roots should still find the files\nstdout: {stdout}"
    );
}

/// The limit is applied once, over the pooled results of all roots.
///
/// With overlapping roots the same file is found under each of them; before
/// the fix the duplicates filled the limit, so `-n 2` returned one distinct
/// file instead of two (terraphim-clients#515).
#[test]
fn files_mode_limit_counts_distinct_files_across_roots() {
    let fixture = Fixture::new();
    let output = run(&[
        "js",
        "--files",
        "--paths",
        fixture.plugins.to_str().unwrap(),
        "--paths",
        fixture.plugins.to_str().unwrap(),
        "-n",
        "2",
    ]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();

    let mut unique = lines.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        2,
        "the limit must count distinct files, got {lines:?}"
    );
}

/// A root that sorts lower must still appear when the limit allows it.
///
/// The limit is the global top-N over the pooled results, not the top-N of the
/// first root (terraphim-clients#515).
#[test]
fn files_mode_limit_is_global_across_roots() {
    let fixture = Fixture::new();
    // A file in the second root, so both roots have a match for the pattern.
    std::fs::write(fixture.docs.join("helper.js"), "// helper\n").unwrap();
    let output = run(&[
        "js",
        "--files",
        "--paths",
        fixture.plugins.to_str().unwrap(),
        "--paths",
        fixture.docs.to_str().unwrap(),
        "-n",
        "50",
    ]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();

    assert!(
        lines
            .iter()
            .any(|path| path.starts_with(fixture.plugins.to_str().unwrap())),
        "the plugins root must be represented, got {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|path| path.starts_with(fixture.docs.to_str().unwrap())),
        "the docs root must be represented too, got {lines:?}"
    );
}
