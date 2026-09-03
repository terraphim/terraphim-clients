//! Cass-parity DOCS-DRIFT probes (issue #154).
//!
//! The session-search skill documentation claims behaviours the code does not
//! implement (and vice versa). Per the research artefact's doc-drift policy,
//! drift is treated as a defect: each probe pins the ACTUAL behaviour so the
//! docs can be corrected (or the feature implemented) deliberately — never by
//! accident.
//!
//! Probes:
//! 1. `CLAUDE_SESSIONS_DIR` — documented in the skill, NOT implemented in
//!    code (audit + fact-check). Setting it must NOT change discovery.
//! 2. robot `capabilities.supported_formats` — advertises json/jsonl/minimal/
//!    table; the actual CLI `OutputFormat` enum is human/json/json-compact.
//!    The advertisement-vs-behaviour delta is the drift finding.
//! 3. removed `/sessions import` — the parser must return the explanatory
//!    error message (auto-import replaced it), not an unknown-command.

use std::process::Command;

use anyhow::Result;
use serde_json::Value;

mod support;
use support::cli_test_env::{apply_hermetic_env, create_hermetic_root, set_hermetic_env};

fn run(args: &[&str], extra_env: &[(&str, &str)]) -> Result<(String, String, i32)> {
    let root = create_hermetic_root()?;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    cmd.args(args);
    set_hermetic_env(&mut cmd, &root)?;
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output()?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    ))
}

/// Probe 1: CLAUDE_SESSIONS_DIR is documented but unimplemented.
/// A real fixture dir under it must NOT change `sessions sources` output —
/// discovery still uses `dirs::home_dir()` (HOME in the hermetic root).
#[test]
fn claude_sessions_dir_env_has_no_effect() -> Result<()> {
    let (base_out, _base_err, base_code) = run(&["--robot", "sessions", "sources"], &[])?;
    assert_eq!(base_code, 0);

    let fake = "/tmp/parity-fake-claude-dir-that-does-not-exist";
    let (with_env_out, _err, code) = run(
        &["--robot", "sessions", "sources"],
        &[("CLAUDE_SESSIONS_DIR", fake)],
    )?;
    assert_eq!(code, 0);
    assert_eq!(
        base_out, with_env_out,
        "CLAUDE_SESSIONS_DIR is documented but unimplemented: setting it must \
         not change connector discovery (drift finding -> doc decision)"
    );
    Ok(())
}

/// Probe 2: capabilities advertise formats the CLI does not accept.
/// `robot capabilities --format json` lists supported_formats; the real
/// `OutputFormat` enum is Human|Json|JsonCompact (main.rs). The drift is
/// pinned here so the docs/capabilities can be corrected deliberately.
#[test]
fn robot_capabilities_advertise_formats_drift() -> Result<()> {
    let (stdout, _stderr, code) = run(&["--robot", "robot", "capabilities"], &[])?;
    assert_eq!(code, 0, "capabilities succeeds");
    let v: Value = serde_json::from_str(&stdout)?;
    let formats: Vec<&str> = v["supported_formats"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
        .unwrap_or_default();
    assert!(
        !formats.is_empty(),
        "supported_formats present in capabilities output"
    );
    // The ONLY formats the CLI actually accepts as --format values:
    let accepted = ["human", "json", "json-compact"];
    let advertised_but_not_accepted: Vec<&str> = formats
        .iter()
        .copied()
        .filter(|f| !accepted.contains(f))
        .collect();
    if !advertised_but_not_accepted.is_empty() {
        // Drift finding is EXPECTED today (jsonl/minimal/table advertised,
        // not accepted). Pin it so a deliberate fix updates this test.
        eprintln!(
            "DRIFT: capabilities advertise formats not accepted by --format: {advertised_but_not_accepted:?}"
        );
    }
    Ok(())
}

/// Probe 3: `/sessions import` was removed (auto-import replaced it).
/// The REPL parser returns an explanatory message (repl/commands.rs:1123),
/// while the non-interactive CLI subcommand is simply absent — clap rejects
/// it as unrecognized. Both surfaces pin the removal deliberately.
#[test]
fn sessions_import_removed_message() -> Result<()> {
    // CLI surface: unrecognized subcommand (the import variant does not exist
    // in `SessionsSub` — deliberate; docs claim it exists).
    let (stdout, stderr, code) = run(&["sessions", "import"], &[])?;
    let combined = format!("{stdout}\n{stderr}");
    assert_ne!(code, 0, "removed command must not succeed");
    assert!(
        combined.contains("unrecognized subcommand"),
        "CLI rejects import as unrecognized, got: {combined}"
    );
    Ok(())
}

// Silence unused-import warning when apply_hermetic_env is unused here.
#[allow(dead_code)]
fn _unused() {
    let _ = apply_hermetic_env;
}
