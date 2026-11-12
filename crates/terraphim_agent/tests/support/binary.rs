//! Shared helpers for locating the binaries the test suites drive.
//!
//! `terraphim_server` is not a member of this workspace (it lives in
//! terraphim-ai), so CI installs it and points `TERRAPHIM_SERVER_BIN` at it
//! (Refs #113). A local checkout usually has neither, and the server-dependent
//! suites used to *fail* in that case, burying real regressions under 21
//! environment failures.
//!
//! The convention in this repository is therefore: a suite that needs a server
//! asks the shared helper first, and skips with a notice when no binary is
//! available. The skip is decided on **absence** only -- a server that exists
//! but misbehaves must still fail the test (Refs #466).

// The support module is compiled into every test binary that declares
// `mod support;`, and each binary uses a different subset of these helpers.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Path to the `terraphim-agent` binary under test.
///
/// `TERRAPHIM_AGENT_BIN` overrides; otherwise `CARGO_BIN_EXE_terraphim-agent`,
/// which Cargo sets for this package's integration tests and builds first, so
/// the path is correct under any `CARGO_TARGET_DIR`.
pub fn agent_binary() -> String {
    if let Ok(bin) = std::env::var("TERRAPHIM_AGENT_BIN") {
        return bin;
    }
    env!("CARGO_BIN_EXE_terraphim-agent").to_string()
}

/// Path to a prebuilt `terraphim_server` binary, whether or not it exists.
///
/// The path is `TERRAPHIM_SERVER_BIN` when that variable is set, otherwise
/// `<workspace>/target/debug/terraphim_server`. Callers that need to know
/// whether the binary is *usable* should use [`server_binary_path`] or
/// [`skip_without_server`] instead of checking the path themselves.
pub fn server_binary() -> String {
    if let Ok(bin) = std::env::var("TERRAPHIM_SERVER_BIN") {
        return bin;
    }
    workspace_root()
        .join("target/debug/terraphim_server")
        .to_string_lossy()
        .to_string()
}

/// Resolve a `terraphim_server` binary from an explicit environment value and
/// a fallback path.
///
/// Pure: the inputs are arguments, so the resolution rules are testable
/// without touching the process environment.
///
/// * An environment value that points at a real file wins.
/// * An environment value that points at a missing file is treated as
///   *absent* rather than silently falling back, so a misconfigured run is
///   visible instead of quietly using a different binary.
/// * Without an environment value, the fallback path is used when it is a file.
pub fn resolve_server_binary(env_value: Option<&str>, fallback: &Path) -> Option<PathBuf> {
    match env_value {
        Some(value) => {
            let path = PathBuf::from(value);
            path.is_file().then_some(path)
        }
        None => fallback.is_file().then(|| fallback.to_path_buf()),
    }
}

/// The `terraphim_server` binary this machine can use, if any.
///
/// Resolution order: `TERRAPHIM_SERVER_BIN`, then
/// `<workspace>/target/debug/terraphim_server`.
pub fn server_binary_path() -> Option<PathBuf> {
    let env_value = std::env::var("TERRAPHIM_SERVER_BIN").ok();
    let fallback = workspace_root().join("target/debug/terraphim_server");
    resolve_server_binary(env_value.as_deref(), &fallback)
}

/// Announce that a test is skipped because no server binary is available.
///
/// Prints the repository's standard notice and returns `true`. Returns `false`
/// when a binary exists, in which case the caller must run the test.
pub fn skip_without_server(test: &str) -> bool {
    if server_binary_path().is_some() {
        return false;
    }
    println!(
        "skipping: {test}: no terraphim_server binary \
         (set TERRAPHIM_SERVER_BIN, or install one into target/debug); refs #113, #466"
    );
    true
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .expect("CARGO_MANIFEST_DIR should be crates/terraphim_agent")
}
