//! Regression tests for #131: `terraphim-agent robot schemas` JSON output
//! must include a `repl_only` boolean field per command. `vm` (always
//! available, firecracker-gated) and `chat` (feature-gated behind
//! `repl-chat`) must be `repl_only: true`; the rest must be `false`.
//!
//! Spawns the compiled binary directly via `CARGO_BIN_EXE_terraphim-agent`.

use std::process::Command;

fn agent_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-agent")
}

fn run_schemas() -> Vec<serde_json::Value> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let output = Command::new(agent_binary())
        .args(["--robot", "--format", "json", "robot", "schemas"])
        .current_dir(tmp.path())
        .output()
        .expect("failed to run terraphim-agent robot schemas");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "robot schemas must succeed.\nstdout: {stdout}"
    );
    serde_json::from_str(stdout.trim()).expect("robot schemas must be valid JSON")
}

#[test]
fn every_command_has_repl_only_field() {
    let schemas = run_schemas();
    assert!(!schemas.is_empty(), "expected at least one schema");
    for cmd in &schemas {
        let name = cmd.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        assert!(
            cmd.get("repl_only").is_some(),
            "command `{name}` is missing the `repl_only` field (Refs #131)"
        );
        assert!(
            cmd["repl_only"].is_boolean(),
            "command `{name}` has non-boolean `repl_only`"
        );
    }
}

#[test]
fn top_level_cli_commands_are_not_repl_only() {
    let schemas = run_schemas();
    let top_level = ["search", "config", "role", "graph"];
    for cmd in &schemas {
        let name = cmd["name"].as_str().unwrap_or("");
        if top_level.contains(&name) {
            assert_eq!(
                cmd["repl_only"],
                serde_json::Value::Bool(false),
                "top-level command `{name}` must have repl_only=false (Refs #131)"
            );
        }
    }
}

#[test]
fn vm_is_marked_repl_only() {
    let schemas = run_schemas();
    let vm = schemas
        .iter()
        .find(|c| c["name"].as_str() == Some("vm"))
        .expect("vm command must appear in schemas");
    assert_eq!(
        vm["repl_only"],
        serde_json::Value::Bool(true),
        "vm is REPL-only (firecracker-gated) and must have repl_only=true (Refs #131)"
    );
}

#[test]
fn chat_is_marked_repl_only() {
    // chat is feature-gated behind repl-chat; if the test binary was built
    // without that feature, chat will not appear. We must tolerate that.
    let schemas = run_schemas();
    if let Some(chat) = schemas.iter().find(|c| c["name"].as_str() == Some("chat")) {
        assert_eq!(
            chat["repl_only"],
            serde_json::Value::Bool(true),
            "chat is REPL-only and must have repl_only=true (Refs #131)"
        );
    }
    // If chat is absent (default build), the field still has the right value
    // when the feature is enabled. The unit test in docs.rs (compile-time)
    // covers that path.
}
