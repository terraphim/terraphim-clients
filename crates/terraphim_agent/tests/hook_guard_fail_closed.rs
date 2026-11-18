//! The PreToolUse hook fails closed when the guard produces no decision (#465).
//!
//! The hook is the last line of defence. If the guard crashes, is killed or
//! prints something that is not a decision, the hook must **block** the call -
//! reading that as an allow would turn every internal failure of the guard
//! into a bypass of the guard itself.
//!
//! The tests drive the real hook binary with a stub guard, so the assertions
//! are on the envelope the agent's caller actually receives.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// Write a stub that stands in for the guard.
///
/// The stub receives the `guard --json` arguments and the command on stdin;
/// what it does with them is what the test is about.
fn write_guard_stub(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, script).expect("write guard stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make the stub executable");
    }
    path
}

/// Run the hook with the given guard program and return (exit code, stdout).
fn run_hook(guard_program: &Path, command: &str) -> (i32, String) {
    let payload = format!(
        r#"{{"tool_name": "Bash", "tool_input": {{"command": "{}"}}}}"#,
        command
    );

    let output = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"))
        .args([
            "hook",
            "--hook-type",
            "pre-tool-use",
            "--guard-program",
            guard_program.to_str().expect("utf8 path"),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(payload.as_bytes())
                .expect("write payload");
            child.wait_with_output()
        })
        .expect("run the hook");

    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    )
}

/// The permission decision the hook emitted, if any.
fn decision(stdout: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    value["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .map(str::to_string)
}

/// The reason the hook emitted, if any.
fn reason(stdout: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    value["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .map(str::to_string)
}

/// A guard that dies without producing any output must not become an allow.
#[test]
fn a_guard_that_produces_no_output_blocks_the_call() {
    let dir = TempDir::new().expect("tempdir");
    let stub = write_guard_stub(
        dir.path(),
        "guard_dies.sh",
        "#!/bin/sh\ncat >/dev/null\nexit 101\n",
    );

    let (code, stdout) = run_hook(&stub, "rm -rf /");
    assert_eq!(code, 0, "the hook exits 0 even when denying");

    assert_eq!(
        decision(&stdout).as_deref(),
        Some("deny"),
        "a guard that produced no decision must block the call: {stdout}"
    );
    let reason = reason(&stdout).expect("a reason must be given");
    assert!(
        reason.contains("guard failed to produce a decision"),
        "the reason must name the failure: {reason}"
    );
    assert!(
        reason.contains("101"),
        "the reason must name the exit status: {reason}"
    );
}

/// A guard that is killed (no exit status) must not become an allow either.
#[test]
fn a_guard_that_is_killed_blocks_the_call() {
    let dir = TempDir::new().expect("tempdir");
    let stub = write_guard_stub(
        dir.path(),
        "guard_killed.sh",
        "#!/bin/sh\ncat >/dev/null\nkill -9 $$\n",
    );

    let (_code, stdout) = run_hook(&stub, "rm -rf /");
    assert_eq!(
        decision(&stdout).as_deref(),
        Some("deny"),
        "a killed guard must block the call: {stdout}"
    );
    let reason = reason(&stdout).expect("a reason must be given");
    assert!(
        reason.contains("guard failed to produce a decision"),
        "the reason must name the failure: {reason}"
    );
}

/// A guard whose output is not a decision must not become an allow.
#[test]
fn a_guard_that_prints_rubbish_blocks_the_call() {
    let dir = TempDir::new().expect("tempdir");
    let stub = write_guard_stub(
        dir.path(),
        "guard_rubbish.sh",
        "#!/bin/sh\ncat >/dev/null\necho 'Segmentation fault'\n",
    );

    let (_code, stdout) = run_hook(&stub, "rm -rf /");
    assert_eq!(
        decision(&stdout).as_deref(),
        Some("deny"),
        "a guard that printed no decision must block the call: {stdout}"
    );
}

/// A guard that cannot even be started must not become an allow.
#[test]
fn a_guard_that_cannot_be_started_blocks_the_call() {
    let dir = TempDir::new().expect("tempdir");
    let missing = dir.path().join("does-not-exist");

    let (_code, stdout) = run_hook(&missing, "rm -rf /");
    assert_eq!(
        decision(&stdout).as_deref(),
        Some("deny"),
        "a guard that cannot be started must block the call: {stdout}"
    );
}

/// A guard that blocks keeps blocking, now through the out-of-process path.
#[test]
fn a_block_decision_still_blocks_the_call() {
    let dir = TempDir::new().expect("tempdir");
    let stub = write_guard_stub(
        dir.path(),
        "guard_blocks.sh",
        "#!/bin/sh\ncat >/dev/null\necho '{\"decision\":\"block\",\"reason\":\"rm -rf is destructive\"}'\n",
    );

    let (_code, stdout) = run_hook(&stub, "rm -rf /");
    assert_eq!(
        decision(&stdout).as_deref(),
        Some("deny"),
        "a block decision must block the call: {stdout}"
    );
    let reason = reason(&stdout).expect("a reason must be given");
    assert!(
        reason.contains("rm -rf is destructive"),
        "the guard's reason must be reported: {reason}"
    );
}

/// A guard that allows lets the call through.
#[test]
fn an_allow_decision_lets_the_call_through() {
    let dir = TempDir::new().expect("tempdir");
    let stub = write_guard_stub(
        dir.path(),
        "guard_allows.sh",
        "#!/bin/sh\ncat >/dev/null\necho '{\"decision\":\"allow\"}'\n",
    );

    let (_code, stdout) = run_hook(&stub, "ls -la");
    assert_ne!(
        decision(&stdout).as_deref(),
        Some("deny"),
        "an allow decision must not block the call: {stdout}"
    );
}
