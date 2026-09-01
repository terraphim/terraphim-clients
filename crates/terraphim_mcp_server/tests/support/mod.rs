//! Test support for `terraphim_mcp_server` integration tests.
//!
//! Provides a hermetic test root + `apply_hermetic_env` so stdio-driven tests
//! can spawn the real `terraphim_mcp_server` binary without depending on a
//! sibling `terraphim_settings/` repository or the host's `.terraphim/`
//! config. Refs #143.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

// Each integration-test binary compiles its own copy of this module, so the
// helpers below can appear "unused" when only some of them are referenced by a
// particular test target. Suppress the noise rather than gating on a feature
// flag we do not need.
#[allow(dead_code)]
static COUNTER: AtomicU64 = AtomicU64::new(0);

#[allow(dead_code)]
fn create_unique_test_root() -> Result<PathBuf> {
    let nonce = COUNTER.fetch_add(1, Ordering::SeqCst);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before unix epoch")?
        .as_nanos();

    let root = std::env::temp_dir().join(format!(
        "terraphim-mcp-server-hermetic-tests-{}-{}-{}",
        std::process::id(),
        ts,
        nonce
    ));

    fs::create_dir_all(&root)?;
    Ok(root)
}

/// Resolve the path to the terraphim_mcp_server binary.
///
/// Priority:
/// 1. `TERRAPHIM_MCP_SERVER_BIN` environment variable (set by CI/build-runner)
/// 2. `../../target/debug/terraphim_mcp_server` relative to current dir
/// 3. `../../target/release/terraphim_mcp_server` relative to current dir
#[allow(dead_code)]
pub fn mcp_server_binary() -> anyhow::Result<std::path::PathBuf> {
    if let Ok(bin) = std::env::var("TERRAPHIM_MCP_SERVER_BIN") {
        let path = std::path::PathBuf::from(bin);
        if path.exists() {
            return Ok(path);
        }
    }

    let crate_dir = std::env::current_dir()?;
    let candidates = [
        crate_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|w| w.join("target").join("debug").join("terraphim_mcp_server")),
        crate_dir.parent().and_then(|p| p.parent()).map(|w| {
            w.join("target")
                .join("release")
                .join("terraphim_mcp_server")
        }),
    ];

    for path in candidates.into_iter().flatten() {
        if path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "terraphim_mcp_server binary not found. Set TERRAPHIM_MCP_SERVER_BIN or run: cargo build -p terraphim_mcp_server"
    )
}

/// Create a fresh, unique hermetic test root under `std::env::temp_dir()`.
/// Tests should `cmd.current_dir(&root)` so `terraphim_config::project::discover()`
/// does not walk up to a host `.terraphim/` directory. Refs #143.
#[allow(dead_code)]
pub fn create_hermetic_root() -> Result<PathBuf> {
    create_unique_test_root()
}