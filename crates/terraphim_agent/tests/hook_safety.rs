//! Regression tests for #126: PreToolUse hook must not silently rewrite
//! destructive commands. Substitution is opt-in via `--rewrite`. The guard
//! check defaults ON for pre-tool-use and can be disabled via `--no-with-guard`.
//!
//! These tests spawn the compiled `terraphim-agent` binary directly using
//! `CARGO_BIN_EXE_terraphim-agent` (set by Cargo for integration tests) so
//! they run in seconds without nesting `cargo build`.
//!
//! Each invocation runs under the hermetic environment from
//! `support::cli_test_env`, so the thesaurus comes from the fixture role
//! config (`tests/fixtures/terraphim_engineer_config.json`, KG at
//! `tests/test_kg/`) rather than whatever `~/.config/terraphim` holds on the
//! host. The substitution tests rely on `tests/test_kg/trash.md`, which maps
//! `rm -rf` to `trash`; without a fixture term the "KG-replaceable" branch is
//! never reached and the tests only pass on machines whose personal KG
//! happens to contain a matching synonym (which is how they passed locally
//! and failed on the Gitea runner).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

mod support;
use support::cli_test_env::apply_hermetic_env;

fn agent_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-agent")
}

/// Spawn the binary, pipe JSON to stdin, return parsed stdout.
fn run_hook(extra_args: &[&str], payload: &str) -> (i32, String, String) {
    let mut cmd = Command::new(agent_binary());
    cmd.arg("hook")
        .arg("--hook-type")
        .arg("pre-tool-use")
        .args(extra_args);
    apply_hermetic_env(&mut cmd).expect("apply hermetic env");
    let mut child = cmd
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

fn isolated_role_config(dir: &Path) -> PathBuf {
    let kg_dir = dir.join("role-kg");
    std::fs::create_dir(&kg_dir).expect("create isolated role KG");
    std::fs::write(
        kg_dir.join("bun-install.md"),
        "# bun install\n\nsynonyms:: npm install\n",
    )
    .expect("write isolated role KG");
    std::fs::write(kg_dir.join("bun.md"), "# bun\n\nsynonyms:: npm\n")
        .expect("write isolated role KG term");

    let config_path = dir.join("config.json");
    let config = serde_json::json!({
        "id": "Server",
        "global_shortcut": null,
        "roles": {
            "Isolated": {
                "shortname": "isolated",
                "name": "Isolated",
                "relevance_function": "terraphim-graph",
                "terraphim_it": true,
                "theme": "lumen",
                "kg": {
                    "automata_path": null,
                    "knowledge_graph_local": {
                        "input_type": "markdown",
                        "path": kg_dir
                    },
                    "public": false,
                    "publish": false
                },
                "haystacks": [],
                "llm_provider": null,
                "llm_auto_summarize": false,
                "llm_system_prompt": null,
                "extra": {}
            }
        },
        "default_role": "Isolated",
        "selected_role": "Isolated"
    });
    std::fs::write(
        &config_path,
        serde_json::to_vec(&config).expect("serialize isolated config"),
    )
    .expect("write isolated config");
    config_path
}

/// Run from a directory with no `docs/src/kg`, while selecting a role whose
/// KG is explicitly configured. This prevents the legacy global-KG lookup
/// from making the test pass accidentally.
fn run_hook_with_isolated_role(
    dir: &Path,
    config_path: &Path,
    extra_args: &[&str],
    payload: &str,
) -> (i32, String, String) {
    let mut cmd = Command::new(agent_binary());
    cmd.arg("--config")
        .arg(config_path)
        .arg("hook")
        .arg("--hook-type")
        .arg("pre-tool-use")
        .args(extra_args);
    apply_hermetic_env(&mut cmd).expect("apply hermetic env");
    cmd.current_dir(dir);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn isolated terraphim-agent hook");

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(payload.as_bytes())
            .expect("failed to write payload to stdin");
    }

    let output = child.wait_with_output().expect("failed to read output");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
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
    let (code, stdout, _stderr) = run_hook(&[], &make_payload("rm -rf /tmp/foo"));
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
        warnings
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("KG-replaceable")),
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
    let (code, stdout, _stderr) = run_hook(&["--rewrite"], &make_payload("rm -rf /tmp/foo"));
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
    let (code, stdout, _stderr) = run_hook(&["--no-with-guard"], &make_payload("rm -rf /"));
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
    let (code, stdout, _stderr) = run_hook(&[], &make_payload("rm -rf /tmp/foo"));
    assert_eq!(code, 0);
    let output = parse(&stdout);
    assert!(
        output.get("hookSpecificOutput").is_none(),
        "/tmp/ must remain allowlisted; got decision envelope: {:?}",
        output
    );
}

#[test]
fn isolated_role_kg_emits_findings_without_rewriting_or_leaking_warning_input() {
    let dir = TempDir::new().expect("tempdir");
    let config = isolated_role_config(dir.path());
    let secret = "secret-token-7d938f";
    let command = format!("npm install react --token={secret}");
    let payload = make_payload(&command);

    let (code, stdout, stderr) = run_hook_with_isolated_role(dir.path(), &config, &[], &payload);
    assert_eq!(code, 0, "stderr: {stderr}");
    let output = parse(&stdout);
    assert_eq!(output["tool_input"]["command"], command);
    assert_eq!(output["validations"]["has_findings"], true);
    assert!(
        output["validations"]["findings"]
            .as_array()
            .is_some_and(|findings| findings.iter().any(|finding| {
                finding["matched_term"] == "npm install"
                    && finding["suggested_replacement"] == "bun install"
            })),
        "selected role KG finding missing: {output}"
    );
    let warnings = output["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .all(|warning| !warning.as_str().unwrap_or_default().contains(secret)),
        "warnings must not disclose command arguments: {warnings:?}"
    );
}

#[test]
fn isolated_role_kg_rewrite_and_findings_use_the_same_snapshot() {
    let dir = TempDir::new().expect("tempdir");
    let config = isolated_role_config(dir.path());
    let payload = make_payload("npm install react");

    let (code, stdout, stderr) =
        run_hook_with_isolated_role(dir.path(), &config, &["--rewrite"], &payload);
    assert_eq!(code, 0, "stderr: {stderr}");
    let output = parse(&stdout);
    assert_eq!(output["tool_input"]["command"], "bun install react");
    assert_eq!(output["validations"]["has_findings"], true);
}

#[test]
fn isolated_role_kg_no_match_passes_through_without_findings_or_warnings() {
    let dir = TempDir::new().expect("tempdir");
    let config = isolated_role_config(dir.path());
    let payload = make_payload("printf hello");

    let (code, stdout, stderr) = run_hook_with_isolated_role(dir.path(), &config, &[], &payload);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(parse(&stdout), parse(&payload));
}

#[test]
fn guard_denial_precedes_role_kg_findings_and_rewrite() {
    let dir = TempDir::new().expect("tempdir");
    let config = isolated_role_config(dir.path());
    let payload = make_payload("rm -rf / && npm install react");

    let (code, stdout, stderr) =
        run_hook_with_isolated_role(dir.path(), &config, &["--rewrite"], &payload);
    assert_eq!(code, 0, "stderr: {stderr}");
    let output = parse(&stdout);
    assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(output.get("validations").is_none(), "guard must run first");
    assert!(output.get("warnings").is_none(), "guard must run first");
    assert!(output.get("tool_input").is_none(), "guard must run first");
}

#[test]
fn isolated_role_kg_ignores_url_only_matches() {
    let dir = TempDir::new().expect("tempdir");
    let config = isolated_role_config(dir.path());
    let payload = make_payload("curl https://registry.npmjs.org/npm");

    let (code, stdout, stderr) = run_hook_with_isolated_role(dir.path(), &config, &[], &payload);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(parse(&stdout), parse(&payload));
}

#[test]
fn isolated_role_kg_counts_and_reports_only_actual_rewrites() {
    let dir = TempDir::new().expect("tempdir");
    let config = isolated_role_config(dir.path());
    let command = "npm install && curl https://registry.npmjs.org/npm";
    let payload = make_payload(command);

    let (code, stdout, stderr) = run_hook_with_isolated_role(dir.path(), &config, &[], &payload);
    assert_eq!(code, 0, "stderr: {stderr}");
    let output = parse(&stdout);
    assert_eq!(output["tool_input"]["command"], command);
    assert_eq!(
        output["warnings"],
        serde_json::json!([
            "command contained 1 KG-replaceable substring(s); pass --rewrite to enable substitution"
        ])
    );
    let findings = output["validations"]["findings"]
        .as_array()
        .expect("findings array");
    assert_eq!(findings.len(), 1, "findings must equal actual rewrites");
    assert_eq!(findings[0]["matched_term"], "npm install");

    let (code, stdout, stderr) =
        run_hook_with_isolated_role(dir.path(), &config, &["--rewrite"], &payload);
    assert_eq!(code, 0, "stderr: {stderr}");
    let output = parse(&stdout);
    assert_eq!(
        output["tool_input"]["command"],
        "bun install && curl https://registry.npmjs.org/npm"
    );
    assert_eq!(
        output["validations"]["findings"].as_array().map(Vec::len),
        Some(1)
    );
}
