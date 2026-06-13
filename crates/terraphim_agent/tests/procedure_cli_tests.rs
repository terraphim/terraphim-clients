//! Integration tests for the procedural memory CLI (`learn procedure` subcommands).
//!
//! These tests exercise the full binary to verify that procedures can be created,
//! steps added, confidence updated, listed, and shown via the CLI.

use std::process::Command;

fn agent_binary() -> Option<String> {
    // Cargo builds the binary before running integration tests and hands over
    // its path; a nested `cargo build` here would deadlock on the outer build
    // lock. Refs #113.
    let path = std::path::PathBuf::from(env!("CARGO_BIN_EXE_terraphim-agent"));
    if path.exists() {
        Some(path.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Run a procedure subcommand, returning (stdout, stderr, success).
fn run_procedure_cmd(binary: &str, args: &[&str], env_home: &str) -> (String, String, bool) {
    let mut full_args = vec!["learn", "procedure"];
    full_args.extend_from_slice(args);

    let output = match Command::new(binary)
        .args(&full_args)
        .env("HOME", env_home)
        .env("XDG_DATA_HOME", format!("{}/data", env_home))
        .output()
    {
        Ok(o) => o,
        Err(e) => return (String::new(), e.to_string(), false),
    };

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

macro_rules! require_binary {
    () => {
        match agent_binary() {
            Some(b) => b,
            None => return,
        }
    };
}

#[test]
fn procedure_list_empty() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    let (stdout, _stderr, success) = run_procedure_cmd(&binary, &["list"], &home);
    assert!(success, "list on empty store should succeed");
    assert!(
        stdout.contains("No procedures found"),
        "expected empty message, got: {}",
        stdout
    );
}

#[test]
fn procedure_record_and_show() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record a new procedure
    let (stdout, _stderr, success) = run_procedure_cmd(
        &binary,
        &[
            "record",
            "Build Rust project",
            "--description",
            "Steps to build a Rust project from scratch",
        ],
        &home,
    );
    assert!(success, "record should succeed");
    assert!(
        stdout.contains("Created procedure:"),
        "expected creation message, got: {}",
        stdout
    );

    // Extract the procedure ID from output
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .expect("should have procedure ID")
        .to_string();

    // Show it
    let (stdout, _stderr, success) = run_procedure_cmd(&binary, &["show", &id], &home);
    assert!(success, "show should succeed");
    assert!(stdout.contains("Build Rust project"), "title in output");
    assert!(
        stdout.contains("Steps to build a Rust project from scratch"),
        "description in output"
    );
    assert!(stdout.contains("Steps (0):"), "zero steps initially");
}

#[test]
fn procedure_add_step_and_list() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record
    let (stdout, _, _) = run_procedure_cmd(&binary, &["record", "Deploy app"], &home);
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .unwrap()
        .to_string();

    // Add steps
    let (stdout, _, success) = run_procedure_cmd(
        &binary,
        &[
            "add-step",
            &id,
            "cargo build --release",
            "--precondition",
            "Rust toolchain installed",
            "--postcondition",
            "Binary exists in target/release",
        ],
        &home,
    );
    assert!(success, "add-step should succeed");
    assert!(stdout.contains("Added step 1"), "first step added");

    let (stdout, _, success) = run_procedure_cmd(
        &binary,
        &["add-step", &id, "scp target/release/app server:/opt/"],
        &home,
    );
    assert!(success, "second add-step should succeed");
    assert!(stdout.contains("Added step 2"), "second step added");

    // Show with steps
    let (stdout, _, success) = run_procedure_cmd(&binary, &["show", &id], &home);
    assert!(success);
    assert!(stdout.contains("Steps (2):"), "two steps");
    assert!(stdout.contains("cargo build --release"));
    assert!(stdout.contains("pre: Rust toolchain installed"));
    assert!(stdout.contains("post: Binary exists in target/release"));
    assert!(stdout.contains("scp target/release/app server:/opt/"));

    // List
    let (stdout, _, success) = run_procedure_cmd(&binary, &["list"], &home);
    assert!(success);
    assert!(stdout.contains("Deploy app"));
    assert!(stdout.contains("2 steps"));
}

#[test]
fn procedure_success_and_failure_update_confidence() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record
    let (stdout, _, _) = run_procedure_cmd(&binary, &["record", "Test procedure"], &home);
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .unwrap()
        .to_string();

    // Record successes
    let (_, _, success) = run_procedure_cmd(&binary, &["success", &id], &home);
    assert!(success);
    let (_, _, success) = run_procedure_cmd(&binary, &["success", &id], &home);
    assert!(success);

    // Record a failure
    let (_, _, success) = run_procedure_cmd(&binary, &["failure", &id], &home);
    assert!(success);

    // Show to verify confidence: 2 successes, 1 failure = 67%
    let (stdout, _, success) = run_procedure_cmd(&binary, &["show", &id], &home);
    assert!(success);
    assert!(
        stdout.contains("67%"),
        "expected 67% confidence, got: {}",
        stdout
    );
    assert!(stdout.contains("2 successes"));
    assert!(stdout.contains("1 failures"));
}

#[test]
fn procedure_success_nonexistent_fails() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    let (_, _stderr, success) = run_procedure_cmd(&binary, &["success", "nonexistent-id"], &home);
    assert!(!success, "success on nonexistent procedure should fail");
}

#[test]
fn procedure_replay_dry_run() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record a procedure
    let (stdout, _, _) = run_procedure_cmd(&binary, &["record", "Echo things"], &home);
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .unwrap()
        .to_string();

    // Add two echo steps
    run_procedure_cmd(&binary, &["add-step", &id, "echo hello"], &home);
    run_procedure_cmd(&binary, &["add-step", &id, "echo world"], &home);

    // Replay with --dry-run
    let (stdout, _stderr, success) =
        run_procedure_cmd(&binary, &["replay", &id, "--dry-run"], &home);
    assert!(success, "dry-run replay should succeed");
    assert!(
        stdout.contains("[DRY RUN]"),
        "should indicate dry run, got: {}",
        stdout
    );
    assert!(
        stdout.contains("step 1: OK"),
        "step 1 should report OK, got: {}",
        stdout
    );
    assert!(
        stdout.contains("step 2: OK"),
        "step 2 should report OK, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Dry run completed"),
        "should report dry run completed, got: {}",
        stdout
    );
}

#[test]
fn procedure_replay_real_execution() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record
    let (stdout, _, _) = run_procedure_cmd(&binary, &["record", "Echo commands"], &home);
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .unwrap()
        .to_string();

    // Add echo steps
    run_procedure_cmd(&binary, &["add-step", &id, "echo hello"], &home);
    run_procedure_cmd(&binary, &["add-step", &id, "echo world"], &home);

    // Replay for real
    let (stdout, _stderr, success) = run_procedure_cmd(&binary, &["replay", &id], &home);
    assert!(success, "replay should succeed, stderr: {}", _stderr);
    assert!(
        stdout.contains("Replay completed successfully"),
        "should report success, got: {}",
        stdout
    );

    // Verify confidence was updated (1 success recorded)
    let (stdout, _, _) = run_procedure_cmd(&binary, &["show", &id], &home);
    assert!(
        stdout.contains("1 successes"),
        "should show 1 success after replay, got: {}",
        stdout
    );
}

#[test]
fn procedure_replay_failure_stops_early() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record
    let (stdout, _, _) = run_procedure_cmd(&binary, &["record", "Failing procedure"], &home);
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .unwrap()
        .to_string();

    // Add a failing step followed by an echo step
    run_procedure_cmd(&binary, &["add-step", &id, "false"], &home);
    run_procedure_cmd(&binary, &["add-step", &id, "echo should-not-run"], &home);

    // Replay -- should fail
    let (stdout, _stderr, success) = run_procedure_cmd(&binary, &["replay", &id], &home);
    assert!(!success, "replay with failure should exit non-zero");
    assert!(
        stdout.contains("FAILED"),
        "should report failure, got: {}",
        stdout
    );
    // The second step should not appear as OK
    assert!(
        !stdout.contains("step 2: OK"),
        "step 2 should not have run, got: {}",
        stdout
    );
}

#[test]
fn procedure_replay_nonexistent_fails() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    let (_, _stderr, success) = run_procedure_cmd(&binary, &["replay", "nonexistent-id"], &home);
    assert!(!success, "replay of nonexistent procedure should fail");
}

#[test]
fn procedure_health_shows_critical_after_failures() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record a procedure
    let (stdout, _, _) = run_procedure_cmd(&binary, &["record", "Fragile procedure"], &home);
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .unwrap()
        .to_string();

    // Record 5 failures (enough for auto-disable)
    for _ in 0..5 {
        let (_, _, success) = run_procedure_cmd(&binary, &["failure", &id], &home);
        assert!(success);
    }

    // Run health check
    let (stdout, _stderr, success) = run_procedure_cmd(&binary, &["health"], &home);
    assert!(
        success,
        "health command should succeed, stderr: {}",
        _stderr
    );
    assert!(
        stdout.contains("Critical"),
        "expected Critical status, got: {}",
        stdout
    );
    assert!(
        stdout.contains("auto-disabled"),
        "expected auto-disabled message, got: {}",
        stdout
    );
}

#[test]
fn procedure_disable_prevents_replay() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record a procedure with a step
    let (stdout, _, _) = run_procedure_cmd(&binary, &["record", "Disable test"], &home);
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .unwrap()
        .to_string();

    run_procedure_cmd(&binary, &["add-step", &id, "echo hello"], &home);

    // Disable it
    let (stdout, _, success) = run_procedure_cmd(&binary, &["disable", &id], &home);
    assert!(success, "disable should succeed");
    assert!(
        stdout.contains("disabled"),
        "expected disabled message, got: {}",
        stdout
    );

    // Attempt replay -- should be refused
    let (_, stderr, success) = run_procedure_cmd(&binary, &["replay", &id], &home);
    assert!(!success, "replay of disabled procedure should fail");
    assert!(
        stderr.contains("disabled"),
        "expected disabled error, got stderr: {}",
        stderr
    );
}

/// Test that `learn procedure from-session <id>` extracts non-trivial successful Bash
/// commands from a session JSON cache and creates a procedure.
///
/// This test satisfies AC from terraphim-ai#2350:
/// - from-session creates a procedure from session history
/// - trivial commands (cd) are filtered out
/// - title is auto-generated from the first non-trivial command
/// - save_with_dedup() is called (one procedure created, not two on repeat)
#[cfg(feature = "repl-sessions")]
#[test]
fn procedure_from_session_extracts_non_trivial_commands() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Write a session JSON to the cache path that get_session_cache_path() resolves to.
    // On Linux, dirs::cache_dir() = $XDG_CACHE_HOME (if set), so we control the path.
    let cache_dir = tmp.path().join("xdg-cache").join("terraphim-agent");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");

    // Session with 4 Bash blocks:
    //   tu1: cargo build --release  (exit 0, keep)
    //   tu2: cd /tmp                (exit 0, trivial → filter)
    //   tu3: cargo test --lib       (exit 1, failed → filter)
    //   tu4: cargo clippy           (exit 0, keep)
    let session_json = r#"[
      {
        "id": "test-session-2350",
        "source": "test",
        "external_id": "test-session-2350",
        "title": "Test session for #2350",
        "source_path": "/dev/null",
        "started_at": null,
        "ended_at": null,
        "messages": [
          {"idx": 0, "role": "assistant", "content": "cmd",
           "blocks": [{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"cargo build --release"}}]},
          {"idx": 1, "role": "tool", "content": "ok",
           "blocks": [{"type":"tool_result","tool_use_id":"tu1","content":"Compiled","exit_code":0}]},
          {"idx": 2, "role": "assistant", "content": "cmd",
           "blocks": [{"type":"tool_use","id":"tu2","name":"Bash","input":{"command":"cd /tmp"}}]},
          {"idx": 3, "role": "tool", "content": "ok",
           "blocks": [{"type":"tool_result","tool_use_id":"tu2","content":"","exit_code":0}]},
          {"idx": 4, "role": "assistant", "content": "cmd",
           "blocks": [{"type":"tool_use","id":"tu3","name":"Bash","input":{"command":"cargo test --lib"}}]},
          {"idx": 5, "role": "tool", "content": "fail",
           "blocks": [{"type":"tool_result","tool_use_id":"tu3","content":"FAILED","exit_code":1}]},
          {"idx": 6, "role": "assistant", "content": "cmd",
           "blocks": [{"type":"tool_use","id":"tu4","name":"Bash","input":{"command":"cargo clippy"}}]},
          {"idx": 7, "role": "tool", "content": "ok",
           "blocks": [{"type":"tool_result","tool_use_id":"tu4","content":"ok","exit_code":0}]}
        ],
        "metadata": {}
      }
    ]"#;

    let session_file = cache_dir.join("sessions.json");
    std::fs::write(&session_file, session_json).expect("write session file");

    let output = Command::new(&binary)
        .args(["learn", "procedure", "from-session", "test-session-2350"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", format!("{}/xdg-data", home))
        .env("XDG_CACHE_HOME", format!("{}/xdg-cache", home))
        .output()
        .expect("run binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "from-session should succeed; stderr: {}",
        stderr
    );

    // Should report 2 steps (cargo build + cargo clippy; cd and failed test filtered)
    assert!(
        stdout.contains("2 steps"),
        "expected 2 steps in output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("4 commands"),
        "expected 4 total commands counted, got: {}",
        stdout
    );
}

/// Running from-session twice with the same session deduplicates via save_with_dedup.
#[cfg(feature = "repl-sessions")]
#[test]
fn procedure_from_session_deduplicates_on_repeat() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    let cache_dir = tmp.path().join("xdg-cache").join("terraphim-agent");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");

    let session_json = r#"[
      {
        "id": "dedup-session-2350",
        "source": "test",
        "external_id": "dedup-session-2350",
        "title": null,
        "source_path": "/dev/null",
        "started_at": null,
        "ended_at": null,
        "messages": [
          {"idx": 0, "role": "assistant", "content": "cmd",
           "blocks": [{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"cargo build"}}]},
          {"idx": 1, "role": "tool", "content": "ok",
           "blocks": [{"type":"tool_result","tool_use_id":"tu1","content":"ok","exit_code":0}]}
        ],
        "metadata": {}
      }
    ]"#;

    std::fs::write(cache_dir.join("sessions.json"), session_json).expect("write session file");

    let run = |extra_args: &[&str]| {
        Command::new(&binary)
            .args(["learn", "procedure", "from-session", "dedup-session-2350"])
            .args(extra_args)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", format!("{}/xdg-data", home))
            .env("XDG_CACHE_HOME", format!("{}/xdg-cache", home))
            .output()
            .expect("run binary")
    };

    // First run: creates a procedure
    let first = run(&[]);
    assert!(
        first.status.success(),
        "first run should succeed, stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    // Second run: same session, should still succeed (dedup merges or reuses)
    let second = run(&[]);
    assert!(
        second.status.success(),
        "second run should succeed, stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    // Verify only 1 procedure in the store after both runs
    let list_output = Command::new(&binary)
        .args(["learn", "procedure", "list"])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", format!("{}/xdg-data", home))
        .env("XDG_CACHE_HOME", format!("{}/xdg-cache", home))
        .output()
        .expect("list procedures");

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    // The list output shows "Procedures (N of N)" — should be 1
    assert!(
        list_stdout.contains("(1 of 1)"),
        "expected exactly 1 procedure after two identical runs, got: {}",
        list_stdout
    );
}

/// Verify that from-session with a missing session ID exits non-zero.
#[cfg(feature = "repl-sessions")]
#[test]
fn procedure_from_session_missing_id_fails() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Cache dir exists but contains no matching session
    let cache_dir = tmp.path().join("xdg-cache").join("terraphim-agent");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");
    std::fs::write(cache_dir.join("sessions.json"), "[]").expect("write empty sessions");

    let output = Command::new(&binary)
        .args([
            "learn",
            "procedure",
            "from-session",
            "nonexistent-session-id",
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", format!("{}/xdg-data", home))
        .env("XDG_CACHE_HOME", format!("{}/xdg-cache", home))
        .output()
        .expect("run binary");

    assert!(
        !output.status.success(),
        "from-session with missing session ID should exit non-zero"
    );
}

#[test]
fn procedure_enable_allows_replay() {
    let binary = require_binary!();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let home = tmp.path().to_string_lossy().to_string();

    // Record a procedure with a step
    let (stdout, _, _) = run_procedure_cmd(&binary, &["record", "Enable test"], &home);
    let id = stdout
        .trim()
        .strip_prefix("Created procedure: ")
        .unwrap()
        .to_string();

    run_procedure_cmd(&binary, &["add-step", &id, "echo hello"], &home);

    // Disable then re-enable
    run_procedure_cmd(&binary, &["disable", &id], &home);
    let (stdout, _, success) = run_procedure_cmd(&binary, &["enable", &id], &home);
    assert!(success, "enable should succeed");
    assert!(
        stdout.contains("enabled"),
        "expected enabled message, got: {}",
        stdout
    );

    // Replay should work now
    let (stdout, _stderr, success) = run_procedure_cmd(&binary, &["replay", &id], &home);
    assert!(
        success,
        "replay of re-enabled procedure should succeed, stderr: {}",
        _stderr
    );
    assert!(
        stdout.contains("Replay completed successfully"),
        "expected success message, got: {}",
        stdout
    );
}
