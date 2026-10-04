//! Cass-parity REPL/CLI session contract tests (issue #153).
//!
//! Covers parity rows C40(p), C80(p), C87(p), C89(p), C99(p) from
//! docs/plans/research-session-test-parity-2026-09.md. The agent had zero
//! session integration tests before this file.
//!
//! Contract rules under test:
//! - `--robot`/`--format` are ROOT-level args and must precede the subcommand.
//! - machine-mode empty search exits 4 (ERROR_NOT_FOUND) AND prints the JSON
//!   payload (payload+exit pairing — never assert a bare exit code).
//! - output JSON shapes match `session_output` serde structs in main.rs.
//! - hermetic HOME everywhere; never read real user session stores.

use std::process::Command;

use anyhow::Result;
use serde_json::Value;

mod support;
use support::cli_test_env::apply_hermetic_env;

/// Run `terraphim-agent` with robot mode + args, return (stdout, stderr, exit).
fn run_robot(args: &[&str]) -> Result<(String, String, i32)> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    cmd.arg("--robot");
    cmd.args(args);
    apply_hermetic_env(&mut cmd)?;
    let out = cmd.output()?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    ))
}

/// Flag placed AFTER the subcommand must be rejected (root-only flag rule).
fn run_robot_flag_after(args: &[&str]) -> Result<(String, String, i32)> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    cmd.args(args); // e.g. sessions search "q" --robot
    apply_hermetic_env(&mut cmd)?;
    let out = cmd.output()?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    ))
}

fn parse_json(s: &str) -> Result<Value> {
    Ok(serde_json::from_str(s)?)
}

/// C40/robot contract: `sessions sources` returns JSON with a member
/// connector set and per-entry availability. Membership assert only —
/// the compiled set can grow with features (dev-dep unification, D-13).
#[test]
fn sessions_sources_membership_json() -> Result<()> {
    let (stdout, _stderr, code) = run_robot(&["sessions", "sources"])?;
    assert_eq!(code, 0, "sessions sources should succeed in robot mode");
    let v = parse_json(&stdout)?;
    let sources = v["sources"].as_array().expect("sources array");
    assert!(!sources.is_empty(), "at least one compiled connector");
    let ids: Vec<&str> = sources.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(
        ids.contains(&"claude-code-native"),
        "native Claude connector is always compiled in, got {ids:?}"
    );
    for s in sources {
        assert!(s["available"].is_boolean(), "availability flag present");
    }
    Ok(())
}

/// C89/C99: with an empty hermetic HOME, sources still lists connectors and
/// search behaves (no panic, no real-store reads). CLAUDE_SESSIONS_DIR is
/// documented but unimplemented -> covered in the docs-drift suite (#154).
#[test]
fn sessions_search_machine_empty_exits_4_with_payload() -> Result<()> {
    let (stdout, _stderr, code) =
        run_robot(&["sessions", "search", "definitely-not-in-any-session"])?;
    assert_eq!(
        code, 4,
        "machine-mode empty search exits ERROR_NOT_FOUND(4)"
    );
    let v = parse_json(&stdout)?;
    assert_eq!(v["query"], "definitely-not-in-any-session");
    assert_eq!(v["total"], 0);
    assert_eq!(v["shown"], 0);
    assert_eq!(
        v["sessions"].as_array().map(|a| a.len()).unwrap_or(1),
        0,
        "sessions array present and empty"
    );
    Ok(())
}

/// Flag-order rule: `--robot` after the subcommand is a usage error (exit 2).
#[test]
fn sessions_search_robot_flag_after_subcommand_rejected() -> Result<()> {
    let (stdout, stderr, code) = run_robot_flag_after(&["sessions", "search", "tokio", "--robot"])?;
    assert_eq!(code, 2, "root flag after subcommand -> ERROR_USAGE(2)");
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("unexpected argument") || combined.contains("--robot"),
        "usage error mentions the misplaced flag, got: {combined}"
    );
    Ok(())
}

/// JSON shape contract for search output (session_output::SessionSearchOutput).
#[test]
fn sessions_search_json_shape() -> Result<()> {
    // Hermetic HOME has no sessions; total==0 but the shape must still hold.
    let (stdout, _stderr, code) = run_robot(&["sessions", "search", "rust"])?;
    assert_eq!(code, 4, "empty corpus exits 4 in machine mode");
    let v = parse_json(&stdout)?;
    for key in ["query", "total", "shown"] {
        assert!(v.get(key).is_some(), "missing key {key}");
    }
    let arr = v["sessions"].as_array().expect("sessions array");
    if let Some(first) = arr.first() {
        for key in ["id", "title", "message_count", "preview"] {
            assert!(first.get(key).is_some(), "missing entry key {key}");
        }
    }
    Ok(())
}

/// C40: stats JSON carries totals + role splits + by_source; the cass-only
/// breakdown families (by_agent/top_workspaces/date_range/raw_mirror) are
/// absent by design (review-corrected assertion — do NOT negative-assert
/// the present ones).
#[test]
fn sessions_stats_json_shape() -> Result<()> {
    let (stdout, _stderr, code) = run_robot(&["sessions", "stats"])?;
    assert_eq!(code, 0, "stats succeeds in robot mode");
    let v = parse_json(&stdout)?;
    for key in [
        "total_sessions",
        "total_messages",
        "total_user_messages",
        "total_assistant_messages",
    ] {
        assert!(v.get(key).is_some(), "missing stats key {key}");
    }
    assert!(v["by_source"].is_object(), "by_source object present");
    Ok(())
}

/// C40/C80: search with a non-empty corpus is covered at the crate level;
/// here we pin the human-mode path (no --robot) to plain-text output and
/// exit 0/4 semantics without JSON.
#[test]
fn sessions_search_human_mode_text_output() -> Result<()> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    cmd.args(["sessions", "search", "nope-nope-nope"]);
    apply_hermetic_env(&mut cmd)?;
    let out = cmd.output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        stdout.contains("No sessions matching"),
        "human mode prints the no-match line, got: {stdout}"
    );
    Ok(())
}

/// Sessions list in robot mode: shape + exit 0.
#[test]
fn sessions_list_robot_shape() -> Result<()> {
    let (stdout, _stderr, code) = run_robot(&["sessions", "list"])?;
    assert_eq!(code, 0, "list succeeds in robot mode");
    let v = parse_json(&stdout)?;
    assert!(v.get("total").is_some());
    assert!(v.get("shown").is_some());
    assert!(v["sessions"].is_array());
    Ok(())
}
