//! CLI tests for `terraphim-agent memory rubric` (#262).
//!
//! Two things are pinned here:
//!
//! 1. The rubric names its scorer as `heuristic-v1` in JSON output, so a
//!    reader can tell it apart from the judge-driven scorer specified in the
//!    memory lifecycle feature request.
//! 2. Items stored in the long-term bucket (High and Critical importance) are
//!    visible to `rubric`, `validate --all`, `export`, `list` and `show`.
//!    Before #262 every one of those read only `short_term` (#207).
//!
//! The tests run the real binary against a hermetic `HOME`, so the evolution
//! store lives in a temp directory and nothing touches the developer's own
//! store. No mocks: the store file is the real one the binary writes.
//!
//! `memory capture` hard-codes `ImportanceLevel::Medium` and has no flag to
//! raise it, so a Critical item cannot be produced through the CLI. The test
//! therefore captures a Medium item through the CLI (which proves the store
//! location and exercises the real save path), then loads the store file,
//! routes a Critical item through the real `MemoryState::add_memory` (which
//! places it in `long_term`) and writes the file back in the same envelope.

use std::path::{Path, PathBuf};
use std::process::Command;

use terraphim_agent_evolution::{
    ImportanceLevel, LessonsState, MemoryItem, MemoryItemType, MemoryState,
};

fn agent_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-agent")
}

/// Run the binary with a hermetic HOME so `dirs::config_dir()` resolves under
/// the temp directory on both macOS (`Library/Application Support`) and Linux
/// (`$XDG_CONFIG_HOME`, pinned to `$HOME/.config`).
fn run(home: &Path, args: &[&str]) -> (String, String, bool) {
    let output = Command::new(agent_binary())
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local").join("share"))
        .current_dir(home)
        .output()
        .expect("failed to run terraphim-agent");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

fn capture_medium_item(home: &Path) -> String {
    let (stdout, stderr, ok) = run(
        home,
        &[
            "--format",
            "json",
            "memory",
            "capture",
            "--provenance-tag",
            "rubric-cli-test",
        ],
    );
    assert!(
        ok,
        "memory capture failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("capture JSON");
    assert_eq!(value["status"], "ok", "capture status: {value}");
    value["memory_id"]
        .as_str()
        .expect("capture emits memory_id")
        .to_string()
}

/// Locate the evolution store the binary wrote under the hermetic HOME.
fn find_store(dir: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_store(&path) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("cli-agent.json") {
            return Some(path);
        }
    }
    None
}

/// Add a Critical item to the persisted store through the real
/// `MemoryState::add_memory` routing, preserving the file envelope that
/// `load_evolution` expects (a malformed envelope is silently treated as an
/// empty store, which would hide the item for the wrong reason).
fn add_critical_item(store: &Path, id: &str) {
    let raw = std::fs::read_to_string(store).expect("read store");
    let mut envelope: serde_json::Value = serde_json::from_str(&raw).expect("store JSON");
    let mut memory: MemoryState =
        serde_json::from_value(envelope["memory"].clone()).expect("deserialise MemoryState");
    let _lessons: LessonsState =
        serde_json::from_value(envelope["lessons"].clone()).expect("deserialise LessonsState");

    memory.add_memory(MemoryItem {
        id: id.to_string(),
        item_type: MemoryItemType::LessonLearned,
        content: "Critical: never run the release script without the tag check".to_string(),
        created_at: chrono::Utc::now(),
        last_accessed: None,
        access_count: 0,
        importance: ImportanceLevel::Critical,
        tags: vec!["release".to_string(), "critical".to_string()],
        associations: std::collections::HashMap::new(),
    });
    assert!(
        memory.long_term.contains_key(id),
        "add_memory must route a Critical item into long_term"
    );
    assert!(
        !memory.short_term.iter().any(|m| m.id == id),
        "a Critical item must not also sit in short_term"
    );

    envelope["memory"] = serde_json::to_value(&memory).expect("serialise MemoryState");
    std::fs::write(
        store,
        serde_json::to_string_pretty(&envelope).expect("serialise envelope"),
    )
    .expect("write store");

    // Self-check: re-read and confirm the item is in the long-term bucket on disk.
    let reread: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(store).expect("re-read store"))
            .expect("re-read JSON");
    assert!(
        reread["memory"]["long_term"].get(id).is_some(),
        "store on disk must hold the Critical item under long_term"
    );
}

#[test]
fn rubric_json_names_heuristic_scorer() {
    let home = tempfile::tempdir().expect("temp home");
    capture_medium_item(home.path());

    let (stdout, stderr, ok) = run(
        home.path(),
        &[
            "--format",
            "json",
            "memory",
            "rubric",
            "--project",
            "rubric-cli-test",
        ],
    );
    assert!(
        ok,
        "memory rubric failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("\"scorer\":\"heuristic-v1\""),
        "rubric JSON must name the scorer.\nstdout: {stdout}"
    );

    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("rubric JSON");
    assert_eq!(value["scorer"], "heuristic-v1");
    let note = value["scorer_note"]
        .as_str()
        .expect("scorer_note is a string");
    assert!(
        note.contains("not the judge-driven scorer"),
        "scorer_note must disclose that this is not the judge-driven scorer: {note}"
    );
    assert_eq!(value["items_analysed"], 1);
}

#[test]
fn rubric_json_names_scorer_even_when_store_is_empty() {
    let home = tempfile::tempdir().expect("temp home");

    let (stdout, stderr, ok) = run(
        home.path(),
        &[
            "--format",
            "json",
            "memory",
            "rubric",
            "--project",
            "rubric-cli-test",
        ],
    );
    assert!(
        ok,
        "memory rubric failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("\"scorer\":\"heuristic-v1\""),
        "empty-store rubric JSON must still name the scorer.\nstdout: {stdout}"
    );
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("rubric JSON");
    assert_eq!(value["items_analysed"], 0);
}

#[test]
fn rubric_text_report_names_heuristic_scorer() {
    let home = tempfile::tempdir().expect("temp home");
    capture_medium_item(home.path());

    let (stdout, stderr, ok) = run(
        home.path(),
        &["memory", "rubric", "--project", "rubric-cli-test"],
    );
    assert!(
        ok,
        "memory rubric failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("**Scorer:** heuristic-v1"),
        "text report must name the scorer.\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("not the judge-driven scorer"),
        "text report must carry the disclosure note.\nstdout: {stdout}"
    );
}

#[test]
fn critical_item_is_visible_to_rubric_validate_export_list_and_show() {
    let home = tempfile::tempdir().expect("temp home");
    let medium_id = capture_medium_item(home.path());

    let store = find_store(home.path()).expect("capture must create cli-agent.json under HOME");
    let critical_id = "critical-rubric-cli-test";
    add_critical_item(&store, critical_id);

    // rubric
    let (stdout, stderr, ok) = run(
        home.path(),
        &[
            "--format",
            "json",
            "memory",
            "rubric",
            "--project",
            "rubric-cli-test",
        ],
    );
    assert!(
        ok,
        "memory rubric failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let rubric: serde_json::Value = serde_json::from_str(stdout.trim()).expect("rubric JSON");
    assert_eq!(
        rubric["items_analysed"], 2,
        "rubric must score both buckets: {rubric}"
    );
    let rubric_ids: Vec<&str> = rubric["items"]
        .as_array()
        .expect("items array")
        .iter()
        .filter_map(|i| i["memory_id"].as_str())
        .collect();
    assert!(
        rubric_ids.contains(&critical_id),
        "rubric ids: {rubric_ids:?}"
    );
    assert!(
        rubric_ids.contains(&medium_id.as_str()),
        "rubric ids: {rubric_ids:?}"
    );

    // validate --all
    let (stdout, stderr, ok) = run(
        home.path(),
        &["--format", "json", "memory", "validate", "--all"],
    );
    assert!(
        ok,
        "memory validate failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let validate: serde_json::Value = serde_json::from_str(stdout.trim()).expect("validate JSON");
    let validate_ids: Vec<&str> = validate["scores"]
        .as_array()
        .expect("scores array")
        .iter()
        .filter_map(|s| s["memory_id"].as_str())
        .collect();
    assert!(
        validate_ids.contains(&critical_id),
        "validate ids: {validate_ids:?}"
    );
    assert!(
        validate_ids.contains(&medium_id.as_str()),
        "validate ids: {validate_ids:?}"
    );
    assert_eq!(validate["scorer"], "heuristic-v1");

    // validate --lesson-id (single-item lookup shares the same bucket union)
    let (stdout, stderr, ok) = run(
        home.path(),
        &[
            "--format",
            "json",
            "memory",
            "validate",
            "--lesson-id",
            critical_id,
        ],
    );
    assert!(
        ok,
        "memory validate --lesson-id failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let single: serde_json::Value = serde_json::from_str(stdout.trim()).expect("validate JSON");
    assert_eq!(single["scores"][0]["memory_id"], critical_id, "{single}");

    // export --format json (the `--format` after `export` is the export format;
    // the global one selects machine-readable output)
    let (stdout, stderr, ok) = run(home.path(), &["memory", "export", "--format", "json"]);
    assert!(
        ok,
        "memory export failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let export: serde_json::Value = serde_json::from_str(stdout.trim()).expect("export JSON");
    let exported: Vec<&serde_json::Value> = export["memory_items"]
        .as_array()
        .expect("memory_items array")
        .iter()
        .collect();
    assert_eq!(export["summary"]["memory_count"], 2, "{export}");
    let critical = exported
        .iter()
        .find(|m| m["id"] == critical_id)
        .unwrap_or_else(|| panic!("export must include the Critical item: {export}"));
    assert_eq!(critical["importance"], "Critical");
    assert!(exported.iter().any(|m| m["id"] == medium_id.as_str()));

    // list
    let (stdout, stderr, ok) = run(home.path(), &["--format", "json", "memory", "list"]);
    assert!(
        ok,
        "memory list failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(critical_id),
        "list must include the Critical item.\nstdout: {stdout}"
    );

    // show
    let (stdout, stderr, ok) = run(home.path(), &["memory", "show", critical_id]);
    assert!(
        ok,
        "memory show failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(critical_id),
        "show must find the Critical item.\nstdout: {stdout}"
    );
    assert!(
        !stdout.to_lowercase().contains("not found"),
        "show must not report the Critical item as missing.\nstdout: {stdout}"
    );
}

#[test]
fn list_default_limit_still_shows_critical_item_behind_many_short_term_items() {
    let home = tempfile::tempdir().expect("temp home");
    // Default `--limit` is 20; fill short_term past it so a short-term-first
    // order would push every long-term item off the end.
    for _ in 0..25 {
        capture_medium_item(home.path());
    }
    let store = find_store(home.path()).expect("capture must create cli-agent.json under HOME");
    let critical_id = "critical-behind-the-limit";
    add_critical_item(&store, critical_id);

    let (stdout, stderr, ok) = run(home.path(), &["--format", "json", "memory", "list"]);
    assert!(
        ok,
        "memory list failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let list: serde_json::Value = serde_json::from_str(stdout.trim()).expect("list JSON");
    assert_eq!(list["count"], 20, "default limit is 20: {list}");
    let ids: Vec<&str> = list["items"]
        .as_array()
        .expect("items array")
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert_eq!(
        ids.first().copied(),
        Some(critical_id),
        "Critical item must be listed first (importance descending): {ids:?}"
    );
}

#[test]
fn validate_json_is_machine_readable_when_nothing_matches() {
    let home = tempfile::tempdir().expect("temp home");

    // Empty store.
    let (stdout, stderr, ok) = run(
        home.path(),
        &["--format", "json", "memory", "validate", "--all"],
    );
    assert!(
        ok,
        "memory validate failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("validate must emit JSON when empty: {e}\nstdout: {stdout}"));
    assert_eq!(value["status"], "ok");
    assert_eq!(value["action"], "validate");
    assert_eq!(value["scorer"], "heuristic-v1");
    assert_eq!(value["scores"], serde_json::json!([]));

    // Missing --lesson-id on a non-empty store.
    capture_medium_item(home.path());
    let (stdout, stderr, ok) = run(
        home.path(),
        &[
            "--format",
            "json",
            "memory",
            "validate",
            "--lesson-id",
            "does-not-exist",
        ],
    );
    assert!(
        ok,
        "memory validate failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("validate must emit JSON on no match: {e}\nstdout: {stdout}"));
    assert_eq!(value["scorer"], "heuristic-v1");
    assert_eq!(value["scores"], serde_json::json!([]));
}
