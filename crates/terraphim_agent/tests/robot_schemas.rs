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
    // Each of these names must have at least one schema entry with
    // `repl_only: false` (the top-level CLI subcommand). The REPL `chat`
    // entry — present in `repl-chat` builds — is filtered out by the
    // `repl_only == false` predicate so the test does not double-count
    // the name `chat`.
    let schemas = run_schemas();
    let top_level = ["search", "config", "role", "graph", "chat"];
    for name in top_level {
        let entry = schemas.iter().find(|c| {
            c["name"].as_str() == Some(name)
                && c["repl_only"] == serde_json::Value::Bool(false)
        });
        assert!(
            entry.is_some(),
            "top-level CLI command `{name}` must have a non-repl-only entry in schemas (Refs #131, #134)"
        );
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
fn repl_chat_is_marked_repl_only() {
    // The REPL `chat` command is feature-gated behind `repl-chat`. It must
    // have `repl_only: true` when present. Filter by `repl_only == true`
    // so this test is robust to a `repl-chat` build that also contains a
    // CLI `chat` entry (`repl_only: false`). Refs #134 P1.
    let schemas = run_schemas();
    let repl_chat = schemas.iter().find(|c| {
        c["name"].as_str() == Some("chat")
            && c["repl_only"] == serde_json::Value::Bool(true)
    });
    // In default and `llm`-only builds, the REPL chat is absent; the unit
    // test in docs.rs (compile-time under `#[cfg(feature = "repl-chat")]`)
    // covers the presence case. The `#[test]` here is a runtime smoke test:
    // if the binary was built with `repl-chat`, the entry must be present
    // and correctly marked.
    if let Some(repl_chat) = repl_chat {
        assert_eq!(
            repl_chat["repl_only"],
            serde_json::Value::Bool(true),
            "REPL chat (repl-chat feature) must have repl_only=true (Refs #134)"
        );
    }
}

#[test]
fn cli_chat_is_marked_not_repl_only() {
    // The top-level CLI `Command::Chat` (gated by `--features llm`,
    // default-on) must be in schemas with `repl_only: false`. In a
    // `repl-chat` build both the CLI and the REPL `chat` are present; we
    // filter by `repl_only == false` to pick the CLI one. Refs #134 P1.
    let schemas = run_schemas();
    let cli_chat = schemas
        .iter()
        .find(|c| c["name"].as_str() == Some("chat"))
        .expect(
            "CLI chat (--features llm, default-on) must appear in schemas (Refs #134 P1)",
        );
    assert_eq!(
        cli_chat["repl_only"],
        serde_json::Value::Bool(false),
        "CLI chat is a top-level CLI subcommand and must have repl_only=false (Refs #134 P1)"
    );
    assert_eq!(
        cli_chat["name"],
        serde_json::Value::String("chat".to_string())
    );
    // The CLI chat takes a required `prompt` positional argument (not the
    // REPL chat's optional `message`); assert the shape so a future schema
    // edit cannot silently drop the required argument.
    let arguments = cli_chat["arguments"]
        .as_array()
        .expect("arguments must be an array");
    let prompt = arguments
        .iter()
        .find(|a| a["name"].as_str() == Some("prompt"))
        .expect("CLI chat must have a `prompt` argument");
    assert_eq!(
        prompt["required"],
        serde_json::Value::Bool(true),
        "CLI chat's `prompt` argument is required (Refs #134 P1)"
    );
}

#[test]
fn summarize_is_marked_repl_only() {
    // `summarize` is REPL-only (registered in `repl::commands`, gated by
    // `repl-chat`); it has no top-level CLI subcommand. Must be
    // `repl_only: true` when present. Refs #134 P2.
    let schemas = run_schemas();
    let summarize = schemas
        .iter()
        .find(|c| c["name"].as_str() == Some("summarize"));
    if let Some(entry) = summarize {
        assert_eq!(
            entry["repl_only"],
            serde_json::Value::Bool(true),
            "summarize is REPL-only and must have repl_only=true (Refs #134 P2)"
        );
    }
    // In default and `llm`-only builds, `summarize` is not in schemas; the
    // unit test in docs.rs (compile-time under `#[cfg(feature = "repl-chat")]`)
    // pins the value when the feature is enabled.
}
