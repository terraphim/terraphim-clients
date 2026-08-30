//! Regression tests for #126: PreToolUse hook must not silently rewrite
//! destructive commands. Substitution is opt-in via `--rewrite`. The guard
//! check defaults ON for pre-tool-use and can be disabled via `--no-with-guard`.
//!
//! These tests spawn the compiled `terraphim-agent` binary directly using
//! `CARGO_BIN_EXE_terraphim-agent` (set by Cargo for integration tests) so
//! they run in seconds without nesting `cargo build`.
//!
//! The hook service takes ~7s to build the thesaurus for the default role, so
//! each test only runs one invocation.

use std::process::{Command, Stdio};

fn agent_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-agent")
}

/// Spawn the binary, pipe JSON to stdin, return parsed stdout.
fn run_hook(extra_args: &[&str], payload: &str) -> (i32, String, String) {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mut child = Command::new(agent_binary())
        .arg("hook")
        .arg("--hook-type")
        .arg("pre-tool-use")
        .args(extra_args)
        .current_dir(tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn terraphim-agent hook");

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(payload.as_bytes())
            .expect("failed to write payload to stdin");
    }

    let output = child.wait_with_output().expect("failed to read output");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.code().unwrap_or(-1), stdout, stderr)
}

fn parse(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim()).expect("hook output must be valid JSON")
}

fn make_payload(command: &str) -> String {
    format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":"{}"}}}}"#,
        command
    )
}

#[test]
fn rm_rf_tmp_foo_passes_through_with_warning() {
    // Default: substitution is suppressed, command passes through unchanged
    // and a `warnings` field documents the suppressed replacement.
    let (code, stdout, _stderr) =
        run_hook(&[], &make_payload("rm -rf /tmp/foo"));
    assert_eq!(code, 0, "hook should exit 0");
    let output = parse(&stdout);
    let command = output["tool_input"]["command"]
        .as_str()
        .expect("tool_input.command must be a string");
    assert_eq!(
        command, "rm -rf /tmp/foo",
        "command must pass through unchanged (Refs #126)"
    );
    let warnings = output["warnings"]
        .as_array()
        .expect("warnings must be an array");
    assert!(
        warnings.iter().any(|w| w.as_str().unwrap_or("").contains("KG-replaceable")),
        "expected a warning explaining the suppressed substitution; got {:?}",
        warnings
    );
}

#[test]
fn rm_rf_root_denied_by_default_guard() {
    // `rm -rf /` is not in the allowlist (`/tmp/`, `/var/folders/`, etc.) so the
    // destructive-pattern guard must deny it. The output uses Claude Code's
    // PreToolUse envelope.
    let (code, stdout, _stderr) = run_hook(&[], &make_payload("rm -rf /"));
    assert_eq!(code, 0, "hook exits 0 even when denying");
    let output = parse(&stdout);
    let decision = output["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .expect("permissionDecision must be a string");
    assert_eq!(
        decision, "deny",
        "rm -rf / must be denied by default guard (Refs #126)"
    );
}

#[test]
fn rewrite_flag_substitutes_when_set() {
    // With `--rewrite`, the thesaurus substitution is applied as before.
    let (code, stdout, _stderr) =
        run_hook(&["--rewrite"], &make_payload("rm -rf /tmp/foo"));
    assert_eq!(code, 0);
    let output = parse(&stdout);
    let command = output["tool_input"]["command"]
        .as_str()
        .expect("tool_input.command must be a string");
    assert_ne!(
        command, "rm -rf /tmp/foo",
        "with --rewrite the command must be substituted (Refs #126)"
    );
}

#[test]
fn no_with_guard_overrides_default_guard() {
    // `--no-with-guard` is the explicit escape hatch; even `rm -rf /` passes
    // through because the user accepted the risk.
    let (code, stdout, _stderr) = run_hook(
        &["--no-with-guard"],
        &make_payload("rm -rf /"),
    );
    assert_eq!(code, 0);
    let output = parse(&stdout);
    // No permissionDecision means no deny — the original payload survives.
    assert!(
        output.get("hookSpecificOutput").is_none(),
        "--no-with-guard must suppress the guard envelope (Refs #126)"
    );
    let command = output["tool_input"]["command"]
        .as_str()
        .expect("tool_input.command must be a string");
    assert_eq!(command, "rm -rf /", "command must pass through verbatim");
}

#[test]
fn allowlisted_rm_rf_path_passes_default_guard() {
    // `/tmp/` is in the allowlist, so `rm -rf /tmp/something` should NOT be
    // denied by the guard. This guards against accidental regressions in the
    // priority order documented in ADR-002 (allowlist > destructive).
    let (code, stdout, _stderr) =
        run_hook(&[], &make_payload("rm -rf /tmp/foo"));
    assert_eq!(code, 0);
    let output = parse(&stdout);
    assert!(
        output.get("hookSpecificOutput").is_none(),
        "/tmp/ must remain allowlisted; got decision envelope: {:?}",
        output
    );
}
