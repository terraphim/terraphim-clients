use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .expect("CARGO_MANIFEST_DIR should be crates/terraphim_agent")
}

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

/// Path to a prebuilt `terraphim_server` binary.
///
/// `terraphim_server` is not a workspace member (it lives in terraphim-ai), so
/// CI installs it and points `TERRAPHIM_SERVER_BIN` at it (Refs #113). The
/// `target/debug` fallback only serves developers who copied a build there.
pub fn server_binary() -> String {
    if let Ok(bin) = std::env::var("TERRAPHIM_SERVER_BIN") {
        return bin;
    }
    workspace_root()
        .join("target/debug/terraphim_server")
        .to_string_lossy()
        .to_string()
}
