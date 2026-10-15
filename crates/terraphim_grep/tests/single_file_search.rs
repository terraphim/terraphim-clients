//! Real CLI regressions for explicitly selected files, using an authored synonym graph.
#![cfg(feature = "code-search")]

use std::path::Path;
use std::process::{Command, Output};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("brief.md"),
        "Judgment: review Café.\r\nRisk: lost context.\r\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("neighbour.md"),
        "Risk: do not retrieve a sibling.\n",
    )
    .unwrap();
    std::fs::write(root.path().join("graph.json"), r#"{"name":"file-search","data":{"risk":{"id":1,"nterm":"risk"},"concern":{"id":1,"nterm":"risk"}}}"#).unwrap();
    root
}

fn search(root: &Path, query: &str, paths: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_terraphim-grep"))
        .current_dir(root)
        .args([
            query,
            "--search-only",
            "--json",
            "--thesaurus",
            "graph.json",
            "--paths",
        ])
        .args(paths)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("OLLAMA_BASE_URL")
        .output()
        .unwrap()
}

fn chunks(output: Output) -> Vec<serde_json::Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["answer"].is_null());
    value["chunks"].as_array().unwrap().clone()
}

#[test]
fn absolute_and_relative_file_search_use_synonyms_without_scanning_neighbours() {
    let root = fixture();
    let absolute = root.path().join("brief.md");
    for path in [
        absolute.as_path(),
        Path::new("brief.md"),
        Path::new("./brief.md"),
    ] {
        let found = chunks(search(root.path(), "Concern", &[path]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0]["source"], "brief.md");
        assert_eq!(found[0]["line_start"], 2);
        assert_eq!(found[0]["content"], "Risk: lost context.");
    }
    assert_eq!(
        std::fs::read(&absolute).unwrap(),
        b"Judgment: review Caf\xc3\xa9.\r\nRisk: lost context.\r\n"
    );
}

#[test]
fn explicit_file_preserves_plain_queries_and_file_filters() {
    let root = fixture();
    let selected = [Path::new("brief.md")];
    let found = chunks(search(root.path(), "Café *.md", &selected));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["line_start"], 1);
    assert!(chunks(search(root.path(), "Café *.rs", &selected)).is_empty());
}

#[test]
fn file_filters_match_the_existing_directory_query_semantics() {
    let root = fixture();
    for query in [
        "Café",
        "Café *.md",
        "Café *.rs",
        "Café !*.rs",
        "Café !*.md",
        "Café b*.md",
        "Café n*.md",
        "Café !b*.md",
        "Café brief.md",
        "Café missing.md",
    ] {
        let explicit = chunks(search(root.path(), query, &[Path::new("brief.md")]));
        let directory = chunks(search(root.path(), query, &[Path::new(".")]));
        assert_eq!(explicit, directory, "query {query:?}");
    }
}

#[test]
fn unicode_filename_and_nested_relative_input_retain_actual_source() {
    let root = fixture();
    std::fs::create_dir(root.path().join("notes")).unwrap();
    std::fs::write(
        root.path().join("notes/résumé.md"),
        "Risk: review Café.\r\n",
    )
    .unwrap();
    let found = chunks(search(
        root.path(),
        "Concern",
        &[Path::new("notes/résumé.md")],
    ));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["source"], "résumé.md");
    assert_eq!(found[0]["content"], "Risk: review Café.");
    let found = chunks(search(
        root.path(),
        "Concern",
        &[Path::new("notes/../brief.md")],
    ));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["source"], "brief.md");
}

#[test]
fn multiple_explicit_files_do_not_add_unselected_siblings() {
    let root = fixture();
    std::fs::write(
        root.path().join("second.md"),
        "Risk: explicit second file.\n",
    )
    .unwrap();
    let found = chunks(search(
        root.path(),
        "Concern",
        &[Path::new("brief.md"), Path::new("second.md")],
    ));
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found
            .iter()
            .all(|c| c["source"] == "brief.md" || c["source"] == "second.md")
    );
}

#[test]
fn missing_file_is_an_error_instead_of_an_empty_success() {
    let root = fixture();
    assert!(
        !search(root.path(), "Concern", &[Path::new("missing.md")])
            .status
            .success()
    );
}

#[cfg(unix)]
#[test]
fn explicit_symlink_and_socket_are_refused() {
    use std::os::unix::{fs::symlink, net::UnixListener};
    let root = fixture();
    symlink(root.path().join("brief.md"), root.path().join("linked.md")).unwrap();
    let _socket = UnixListener::bind(root.path().join("socket")).unwrap();
    for path in [Path::new("linked.md"), Path::new("socket")] {
        let output = search(root.path(), "Concern", &[path]);
        assert!(!output.status.success(), "must refuse {path:?}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn invalid_utf8_single_file_is_refused_without_inspecting_neighbours() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let root = fixture();
    let invalid = root
        .path()
        .join(OsString::from_vec(b"bad-\xff.md".to_vec()));
    std::fs::write(&invalid, "Risk: invalid path.\n").unwrap();
    assert!(!search(root.path(), "Concern", &[&invalid]).status.success());
    let found = chunks(search(root.path(), "Concern", &[Path::new("brief.md")]));
    assert_eq!(
        found.len(),
        1,
        "the explicit file must not scan hostile siblings"
    );
}
