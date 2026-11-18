//! `terraphim-agent cache list` / `cache export` (#406).
//!
//! The export must produce a file that `terraphim-grep --thesaurus` can load,
//! and the listing must describe what is actually persisted.

use std::path::PathBuf;

use terraphim_agent::cache_export::{list_cached_thesauri, thesaurus_json};
use terraphim_automata::AutomataPath;
use terraphim_persistence::{DeviceStorage, Persistable};
use terraphim_types::Thesaurus;

mod support;

/// A thesaurus with a couple of terms and a source hash.
fn sample_thesaurus(name: &str) -> Thesaurus {
    let mut thesaurus = Thesaurus::new(name.to_string());
    thesaurus.insert(
        terraphim_types::NormalizedTermValue::from("npm install"),
        terraphim_types::NormalizedTerm::new(
            1,
            terraphim_types::NormalizedTermValue::from("bun install"),
        ),
    );
    thesaurus.insert(
        terraphim_types::NormalizedTermValue::from("pnpm add"),
        terraphim_types::NormalizedTerm::new(
            2,
            terraphim_types::NormalizedTermValue::from("bun add"),
        ),
    );
    thesaurus.with_source_hash("abc123".to_string())
}

#[tokio::test]
async fn exported_thesaurus_loads_with_load_thesaurus() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path: PathBuf = tmp.path().join("role.json");

    let json = thesaurus_json(&sample_thesaurus("code reviewer")).expect("serialize");
    std::fs::write(&path, &json).expect("write");

    let loaded = terraphim_automata::load_thesaurus(&AutomataPath::from_local(path))
        .await
        .expect("the exported file must load with load_thesaurus");

    assert_eq!(loaded.name(), "code reviewer");
    assert_eq!(loaded.len(), 2, "both terms must survive the round trip");
}

#[tokio::test]
async fn listing_describes_what_is_persisted() {
    let storage = DeviceStorage::init_memory_only()
        .await
        .expect("memory storage");
    let thesaurus = sample_thesaurus("listing probe");
    thesaurus
        .save_to_one("memory")
        .await
        .expect("save thesaurus");

    let cached = list_cached_thesauri(storage).await.expect("list cache");

    let entry = cached
        .iter()
        .find(|entry| entry.name == "listing probe")
        .expect("the saved thesaurus must be listed");
    assert!(entry.key.starts_with("thesaurus_"), "key was {}", entry.key);
    assert!(entry.key.ends_with(".json"), "key was {}", entry.key);
    assert_eq!(entry.entries, 2, "the entry count must match the thesaurus");
    assert_eq!(
        entry.source_hash.as_deref(),
        Some("abc123"),
        "the source hash must be reported"
    );
}

#[test]
fn cli_export_writes_a_loadable_thesaurus() {
    use std::process::Command;

    use support::cli_test_env::set_hermetic_env;

    let root = support::cli_test_env::create_hermetic_root().expect("hermetic root");
    let out = root.join("exported.json");

    let mut command = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    set_hermetic_env(&mut command, &root).expect("hermetic env");
    let output = command
        .args([
            "cache",
            "export",
            "--role",
            "Terraphim Engineer",
            "--out",
            out.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run terraphim-agent cache export");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "export must succeed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        out.is_file(),
        "the export must write the file\nstdout: {stdout}\nstderr: {stderr}"
    );

    let contents = std::fs::read_to_string(&out).expect("read export");
    let value: serde_json::Value = serde_json::from_str(&contents).expect("valid JSON");
    assert!(
        value.get("name").and_then(|name| name.as_str()).is_some(),
        "the export must carry the thesaurus name: {contents}"
    );
    assert!(
        value
            .get("data")
            .and_then(|data| data.as_object())
            .is_some(),
        "the export must carry the term map: {contents}"
    );
}

/// Write a role configuration whose knowledge graph is `kg_dir`.
///
/// The fixture configuration is reused so the test exercises the same shape
/// the agent loads in production, with only the knowledge-graph path changed.
fn write_config_with_kg(kg_dir: &std::path::Path, target: &std::path::Path) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/terraphim_engineer_config.json");
    let raw = std::fs::read_to_string(&fixture).expect("read role config fixture");
    let mut config: serde_json::Value = serde_json::from_str(&raw).expect("parse role config");

    config["roles"]["Terraphim Engineer"]["kg"]["knowledge_graph_local"]["path"] =
        serde_json::Value::String(kg_dir.to_string_lossy().to_string());

    std::fs::write(
        target,
        serde_json::to_string_pretty(&config).expect("serialize config"),
    )
    .expect("write role config");
}

/// Point the hermetic settings at a specific role configuration.
fn patch_role_config(root: &std::path::Path, config_path: &std::path::Path) {
    let settings = root.join("home/.config/terraphim/settings.toml");
    let text = std::fs::read_to_string(&settings).expect("read hermetic settings");
    let patched = text
        .lines()
        .map(|line| {
            if line.starts_with("role_config = ") {
                format!("role_config = \"{}\"", config_path.display())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&settings, patched).expect("write hermetic settings");
}

/// Whether the exported thesaurus holds a term containing `needle`.
///
/// The comparison is case-insensitive because the builder normalises terms.
fn export_has_term(export: &serde_json::Value, needle: &str) -> bool {
    export
        .get("data")
        .and_then(|data| data.as_object())
        .map(|data| {
            data.keys()
                .any(|term| term.to_lowercase().contains(&needle.to_lowercase()))
        })
        .unwrap_or(false)
}

/// The acceptance criterion of #406: an export reflects a knowledge-graph edit
/// without a prior search.
///
/// The role's vocabulary is compiled from its Markdown knowledge graph when the
/// export runs, so the second export sees the edited file even though no
/// search happened in between.
#[test]
fn export_reflects_a_knowledge_graph_edit_without_a_prior_search() {
    use std::process::Command;

    use support::cli_test_env::set_hermetic_env;

    let root = support::cli_test_env::create_hermetic_root().expect("hermetic root");

    let kg_dir = root.join("kg");
    std::fs::create_dir_all(&kg_dir).expect("create kg dir");
    // The knowledge-graph format is one Markdown file per concept, with the
    // synonyms declared as `synonyms:: a, b`.
    std::fs::write(kg_dir.join("alpha.md"), "synonyms:: alpha synonym\n").expect("write kg");

    let config_path = root.join("role_config.json");
    write_config_with_kg(&kg_dir, &config_path);

    let run_export = |out: &std::path::Path| -> serde_json::Value {
        let mut command = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
        set_hermetic_env(&mut command, &root).expect("hermetic env");
        patch_role_config(&root, &config_path);
        let output = command
            .args([
                "cache",
                "export",
                "--role",
                "Terraphim Engineer",
                "--out",
                out.to_str().expect("utf8 path"),
            ])
            .output()
            .expect("run terraphim-agent cache export");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "export must succeed\nstdout: {stdout}\nstderr: {stderr}"
        );
        let contents = std::fs::read_to_string(out).expect("read export");
        serde_json::from_str(&contents).expect("valid JSON export")
    };

    let first = run_export(&root.join("first.json"));
    assert!(
        export_has_term(&first, "alpha"),
        "the first export must hold the knowledge-graph term: {first}"
    );
    let first_hash = first
        .get("source_hash")
        .and_then(|hash| hash.as_str())
        .map(str::to_string);

    // Edit the knowledge graph. No search runs between the two exports.
    std::fs::write(kg_dir.join("beta.md"), "synonyms:: beta synonym\n").expect("edit kg");

    let second = run_export(&root.join("second.json"));
    assert!(
        export_has_term(&second, "beta"),
        "the export must reflect the knowledge-graph edit without a prior search: {second}"
    );
    assert!(
        export_has_term(&second, "alpha"),
        "the export must keep the terms that were already there: {second}"
    );

    let second_hash = second
        .get("source_hash")
        .and_then(|hash| hash.as_str())
        .map(str::to_string);
    assert_ne!(
        first_hash, second_hash,
        "the source hash must change with the knowledge graph"
    );
}
