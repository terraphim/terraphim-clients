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

/// Whether a path is a file this process can run.
///
/// A stale artifact left behind by an interrupted build is a regular file, so
/// `is_file` alone would accept a truncated server and turn a broken local
/// artifact into an opaque test failure (terraphim-clients#513). The
/// executable bit is required on Unix.
pub fn is_usable_binary(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolve a `terraphim_server` binary from an explicit environment value and
/// a fallback path.
///
/// Pure: the inputs are arguments, so the resolution rules are testable
/// without touching the process environment.
///
/// * An environment value that points at a runnable file wins.
/// * An environment value that is empty or only whitespace counts as *unset*:
///   setting a variable to the empty string is how a CI matrix usually spells
///   "not configured", and it must not silently skip the suite
///   (terraphim-clients#513).
/// * An environment value that points at a missing or non-executable file is
///   treated as *absent* rather than silently falling back, so a misconfigured
///   run is visible instead of quietly using a different binary.
/// * Without an environment value, the fallback path is used when it is a
///   runnable file.
pub fn resolve_server_binary(env_value: Option<&str>, fallback: &Path) -> Option<PathBuf> {
    match env_value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value);
            is_usable_binary(&path).then_some(path)
        }
        None => is_usable_binary(fallback).then(|| fallback.to_path_buf()),
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
    // A file that exists but cannot be run is a different failure from an
    // absent binary, and the notice must say so: the developer sees the real
    // cause instead of "no binary" (terraphim-clients#513).
    let configured = std::env::var("TERRAPHIM_SERVER_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(configured) = configured {
        let path = PathBuf::from(&configured);
        if path.is_file() {
            println!(
                "skipping: {test}: the terraphim_server binary at '{configured}' is not \
                 executable; refs #113, #466, #513"
            );
            return true;
        }
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
