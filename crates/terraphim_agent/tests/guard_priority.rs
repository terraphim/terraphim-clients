//! Regression tests for #129: `terraphim-agent guard --explain` reveals the
//! priority order (allowlist > destructive > suspicious > default) and the
//! docs match the runtime behaviour.
//!
//! Spawns the compiled binary directly via `CARGO_BIN_EXE_terraphim-agent`.

use std::process::{Command, Stdio};

fn agent_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-agent")
}

fn run_guard(args: &[&str], stdin_payload: Option<&str>) -> (i32, String, String) {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mut cmd = Command::new(agent_binary());
    cmd.arg("guard").args(args).current_dir(tmp.path());
    if stdin_payload.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn terraphim-agent guard");
    if let (Some(payload), Some(mut stdin)) = (stdin_payload, child.stdin.take()) {
        use std::io::Write;
        stdin
            .write_all(payload.as_bytes())
            .expect("failed to write to stdin");
    }

    let output = child.wait_with_output().expect("failed to read output");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.code().unwrap_or(-1), stdout, stderr)
}

#[test]
fn allowlist_short_circuits_before_destructive() {
    // `rm -rf /tmp/foo` matches both the allowlist (`rm -rf /tmp/`) and the
    // destructive pattern (`rm -rf`). The allowlist must win so the trace
    // shows exactly one stage with outcome=allow.
    let (code, stdout, _stderr) = run_guard(&["--explain", "--json"], Some("rm -rf /tmp/foo"));
    assert_eq!(code, 0);
    let trace: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("expected JSON trace");
    assert_eq!(trace["decision"], "allow");
    let stages = trace["stages"].as_array().expect("stages must be array");
    assert_eq!(stages.len(), 1, "allowlist must short-circuit");
    assert_eq!(stages[0]["stage"], "allowlist");
    assert_eq!(stages[0]["matched"], true);
    assert_eq!(stages[0]["outcome"], "allow");
}

#[test]
fn destructive_short_circuits_before_suspicious() {
    // `rm -rf /` is not in the allowlist; destructive must block before
    // suspicious ever runs.
    let (code, stdout, _stderr) =
        run_guard(&["--explain", "--json", "--fail-open"], Some("rm -rf /"));
    assert_eq!(code, 0);
    let trace: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("expected JSON trace");
    assert_eq!(trace["decision"], "block");
    let stages = trace["stages"].as_array().expect("stages must be array");
    // Two stages run: allowlist (no_match) + destructive (block).
    assert_eq!(stages.len(), 2, "destructive must short-circuit");
    assert_eq!(stages[0]["stage"], "allowlist");
    assert_eq!(stages[0]["matched"], false);
    assert_eq!(stages[1]["stage"], "destructive");
    assert_eq!(stages[1]["matched"], true);
    assert_eq!(stages[1]["outcome"], "block");
    assert_eq!(stages[1]["matched_term"], "rm -rf");
}

#[test]
fn default_allow_path_emits_default_stage() {
    // `echo hello` matches nothing -- the trace must include the
    // `default` stage with outcome=allow.
    let (code, stdout, _stderr) = run_guard(&["--explain", "--json"], Some("echo hello"));
    assert_eq!(code, 0);
    let trace: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("expected JSON trace");
    assert_eq!(trace["decision"], "allow");
    let stages = trace["stages"].as_array().expect("stages must be array");
    assert_eq!(stages.len(), 4, "all four stages must be reported");
    assert_eq!(stages[3]["stage"], "default");
    assert_eq!(stages[3]["outcome"], "allow");
}

#[test]
fn explain_exits_one_on_block() {
    // Without --fail-open, a blocked command must exit 1 even with --explain
    // so the trace can be used as a gate in shell pipelines.
    let (code, _stdout, stderr) = run_guard(&["--explain"], Some("rm -rf /"));
    assert_eq!(code, 1, "blocked command must exit 1");
    assert!(
        stderr.contains("stage=destructive"),
        "trace must be on stderr; got: {}",
        stderr
    );
}

#[test]
fn explain_text_output_is_human_readable() {
    let (code, _stdout, stderr) = run_guard(&["--explain"], Some("echo hello"));
    assert_eq!(code, 0);
    assert!(stderr.contains("# guard evaluation trace"));
    assert!(stderr.contains("# stage=allowlist"));
    assert!(stderr.contains("# stage=destructive"));
    assert!(stderr.contains("# stage=suspicious"));
    assert!(stderr.contains("# stage=default"));
    assert!(stderr.contains("# decision=Allow"));
}

/// The temporary root is the temporary root, with or without a trailing
/// slash (terraphim-clients#510).
///
/// The shipped allowlist carries the literal entry `rm -rf /tmp/`, so the
/// whole-command comparison used to exempt it before the safety evaluation
/// ran: `rm -rf /tmp/` was allowed while `rm -rf /tmp` was blocked, although
/// the two delete the same thing.
#[test]
fn the_temporary_root_is_blocked_with_or_without_a_trailing_slash() {
    for command in [
        "rm -rf /tmp/",
        "rm -fr /tmp/",
        "rm -rf /var/tmp/",
        "rm -fr /var/tmp/",
        "rm -rf /tmp",
    ] {
        let (code, stdout, stderr) = run_guard(&["--json"], Some(command));
        assert_eq!(code, 0, "the JSON output mode always exits 0");
        let result: serde_json::Value =
            serde_json::from_str(stdout.trim()).expect("expected JSON decision");
        assert_eq!(
            result["decision"], "block",
            "{command:?} deletes the temporary root itself and must be blocked: {stdout} {stderr}"
        );
    }
}

/// The trace shows that the allowlist no longer short-circuits a destructive
/// command (terraphim-clients#510).
#[test]
fn the_allowlist_does_not_short_circuit_the_temporary_root() {
    let (code, stdout, _stderr) = run_guard(&["--explain", "--json"], Some("rm -rf /tmp/"));
    assert_eq!(
        code, 1,
        "a blocked command exits 1 even with --explain, so the trace can gate a pipeline"
    );
    let trace: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("expected JSON trace");
    assert_eq!(trace["decision"], "block");

    let stages = trace["stages"].as_array().expect("stages must be array");
    let allowlist = stages
        .iter()
        .find(|stage| stage["stage"] == "allowlist")
        .expect("the allowlist stage must be reported");
    assert_eq!(
        allowlist["outcome"], "continue",
        "the allowlist must not short-circuit a destructive command: {stages:?}"
    );

    let destructive = stages
        .iter()
        .find(|stage| stage["stage"] == "destructive")
        .expect("the destructive stage must be reported");
    assert_eq!(destructive["outcome"], "block");
}

/// A cleanup inside the temporary root stays allowed (terraphim-clients#510).
#[test]
fn a_cleanup_inside_the_temporary_root_stays_allowed() {
    let (code, stdout, _stderr) = run_guard(&["--json"], Some("rm -rf /tmp/build-cache"));
    assert_eq!(code, 0);
    let result: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("expected JSON decision");
    assert_eq!(
        result["decision"], "allow",
        "a cleanup inside the temporary root must stay allowed: {stdout}"
    );
}
