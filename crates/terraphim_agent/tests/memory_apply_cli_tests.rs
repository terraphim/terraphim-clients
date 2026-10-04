//! CLI tests for the injected size reported by `terraphim-agent memory apply`
//! (#261, epic #255).
//!
//! `memory apply --format json` must report `injected_bytes` and
//! `estimated_tokens`, computed by `memory_bench::injected_size` from the
//! exact text the memory hook would inject for the prompt: the items the
//! unchanged `memory_retrieve::retrieve` returns for it from the real store.
//!
//! The tests run the real binary under the hermetic environment from
//! `support::cli_test_env`, so the thesaurus comes from the fixture role
//! config (`tests/fixtures/terraphim_engineer_config.json`, knowledge graph
//! at `tests/test_kg/`) and the evolution store lives under a temp `HOME`.
//! No mocks: the store file is the one the binary writes.
//!
//! `memory capture` writes fixed content with no knowledge-graph term in it,
//! so a captured item alone is never retrieved and the injected size is zero.
//! The non-zero case adds an item whose content names the `trash` concept
//! twice (`rm -rf` and `rm -r`), through the real `MemoryState::add_memory`,
//! into the store the CLI created.

use std::path::{Path, PathBuf};
use std::process::Command;

use terraphim_agent::memory_bench::{estimate_tokens, injected_size};
use terraphim_agent_evolution::{
    ImportanceLevel, LessonsState, MemoryItem, MemoryItemType, MemoryState,
};

mod support;
use support::cli_test_env::{create_hermetic_root, set_hermetic_env};

fn agent_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-agent")
}

fn run(root: &Path, args: &[&str]) -> (String, String, bool) {
    let mut cmd = Command::new(agent_binary());
    cmd.args(args);
    set_hermetic_env(&mut cmd, root).expect("hermetic env");
    let output = cmd.output().expect("failed to run terraphim-agent");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

fn capture_item(root: &Path, tag: &str) -> String {
    let (stdout, stderr, ok) = run(
        root,
        &[
            "--format",
            "json",
            "memory",
            "capture",
            "--provenance-tag",
            tag,
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

fn apply_json(root: &Path, prompt: &str) -> serde_json::Value {
    let (stdout, stderr, ok) = run(
        root,
        &["--format", "json", "memory", "apply", "--prompt", prompt],
    );
    assert!(
        ok,
        "memory apply failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("memory apply must print JSON: {e}\nstdout: {stdout}\nstderr: {stderr}")
    })
}

/// Locate the evolution store the binary wrote under the hermetic root.
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

fn trash_item(id: &str) -> MemoryItem {
    MemoryItem {
        id: id.to_string(),
        item_type: MemoryItemType::LessonLearned,
        content: "Never rm -rf the build directory; use rm -r on the cache only after a backup"
            .to_string(),
        created_at: chrono::Utc::now(),
        last_accessed: None,
        access_count: 0,
        importance: ImportanceLevel::Medium,
        tags: Vec::new(),
        associations: std::collections::HashMap::new(),
    }
}

/// Add `item` to the persisted store through the real `MemoryState::add_memory`,
/// preserving the file envelope `load_evolution` expects.
fn add_item_to_store(store: &Path, item: MemoryItem) {
    let raw = std::fs::read_to_string(store).expect("read store");
    let mut envelope: serde_json::Value = serde_json::from_str(&raw).expect("store JSON");
    let mut memory: MemoryState =
        serde_json::from_value(envelope["memory"].clone()).expect("deserialise MemoryState");
    let _lessons: LessonsState =
        serde_json::from_value(envelope["lessons"].clone()).expect("deserialise LessonsState");
    memory.add_memory(item);
    envelope["memory"] = serde_json::to_value(&memory).expect("serialise MemoryState");
    std::fs::write(
        store,
        serde_json::to_string_pretty(&envelope).expect("serialise envelope"),
    )
    .expect("write store");
}

fn assert_fields_consistent(value: &serde_json::Value) -> (u64, u64) {
    assert_eq!(value["status"], "ok", "{value}");
    assert_eq!(value["action"], "apply", "{value}");
    // Existing fields are kept.
    assert!(value["role"].is_string(), "role kept: {value}");
    assert!(value["count"].is_u64(), "count kept: {value}");
    assert!(value["matches"].is_array(), "matches kept: {value}");
    let bytes = value["injected_bytes"]
        .as_u64()
        .unwrap_or_else(|| panic!("injected_bytes must be an unsigned integer: {value}"));
    let tokens = value["estimated_tokens"]
        .as_u64()
        .unwrap_or_else(|| panic!("estimated_tokens must be an unsigned integer: {value}"));
    assert_eq!(
        tokens,
        bytes.div_ceil(4),
        "estimated_tokens must be ceil(injected_bytes / 4): {value}"
    );
    assert_eq!(tokens, estimate_tokens(bytes));
    (bytes, tokens)
}

#[test]
fn apply_json_reports_injected_bytes_and_estimated_tokens() {
    let root = create_hermetic_root().expect("hermetic root");
    capture_item(&root, "apply-cli-test");

    let value = apply_json(&root, "why did bun install fail");
    let (bytes, tokens) = assert_fields_consistent(&value);
    // The captured item names no knowledge-graph term, so nothing is
    // retrieved and nothing would be injected.
    assert_eq!(value["retrieved_items"], 0, "{value}");
    assert_eq!(bytes, 0, "{value}");
    assert_eq!(tokens, 0, "{value}");
}

#[test]
fn apply_json_injected_size_matches_retrieved_items() {
    let root = create_hermetic_root().expect("hermetic root");
    capture_item(&root, "apply-cli-test-trash");
    let store = find_store(&root).expect("capture must have written cli-agent.json");
    let item = trash_item("trash-lesson-1");
    add_item_to_store(&store, item.clone());

    let prompt = "rm -rf target";
    let value = apply_json(&root, prompt);
    let (bytes, tokens) = assert_fields_consistent(&value);

    // The prompt names the `trash` concept, the item carries it twice, so the
    // real store retrieval returns the item and the injected size is the size
    // of that item rendered by `injected_size`.
    assert_eq!(value["retrieved_items"], 1, "{value}");
    let expected = injected_size(prompt, std::slice::from_ref(&item));
    assert!(bytes > 0, "{value}");
    assert_eq!(bytes, expected.bytes, "{value}");
    assert_eq!(tokens, expected.estimated_tokens, "{value}");
    // The existing replacement preview still reports the prompt match.
    assert!(
        value["count"].as_u64().unwrap_or(0) >= 1,
        "the prompt term must still be listed as a replacement: {value}"
    );
}
