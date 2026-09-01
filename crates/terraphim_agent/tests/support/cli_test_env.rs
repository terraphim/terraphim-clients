use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn workspace_root() -> Result<PathBuf> {
    let mut current = std::env::current_dir()?;

    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = fs::read_to_string(&cargo_toml)?;
            if content.contains("[workspace]") {
                return Ok(current);
            }
        }

        if !current.pop() {
            break;
        }
    }

    Err(anyhow::anyhow!("could not locate workspace root"))
}

fn create_unique_test_root() -> Result<PathBuf> {
    let nonce = COUNTER.fetch_add(1, Ordering::SeqCst);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before unix epoch")?
        .as_nanos();

    let root = std::env::temp_dir().join(format!(
        "terraphim-agent-hermetic-tests-{}-{}-{}",
        std::process::id(),
        ts,
        nonce
    ));

    fs::create_dir_all(&root)?;
    Ok(root)
}

/// Create a fresh, unique hermetic test root under `std::env::temp_dir()`.
/// Returns the root path so callers that need to read files written by the
/// spawned subprocess (e.g. `user_prompt_submit_tests` reading correction
/// files at `<root>/data/terraphim/learnings/`) can locate them. Refs #144.
pub fn create_hermetic_root() -> Result<PathBuf> {
    let root = create_unique_test_root()?;
    let data_dir = root.join("data");
    fs::create_dir_all(&data_dir)?;
    Ok(root)
}

/// Configure `cmd` with a fresh hermetic environment rooted at a unique temp
/// dir. Used by `integration_tests` and `offline_mode_tests`.
///
/// Each integration-test binary compiles its own copy of this support module,
/// so `apply_hermetic_env` appears unused in `user_prompt_submit_tests` (which
/// needs the root path and so calls `set_hermetic_env` directly). The function
/// itself is not dead — only the per-binary view is. Removing this annotation
/// would require splitting the support helpers into a separate crate so that
/// each test binary only sees the items it imports; tracked for follow-up.
#[allow(dead_code)]
pub fn apply_hermetic_env(cmd: &mut Command) -> Result<()> {
    let root = create_hermetic_root()?;
    set_hermetic_env(cmd, &root)
}

/// Apply the hermetic test environment rooted at `root` to `cmd`. Use this
/// when the caller needs to know the root path (e.g. to read files written
/// by the spawned subprocess). Refs #144.
///
/// Only referenced by `user_prompt_submit_tests`, which needs to read the
/// correction files the hook writes; the other agent integration tests use
/// the simpler `apply_hermetic_env` wrapper. Hence the cross-binary allow.
#[allow(dead_code)]
pub fn set_hermetic_env(cmd: &mut Command, root: &PathBuf) -> Result<()> {
    let home_dir = root.join("home");
    let xdg_config_home = home_dir.join(".config");
    let terraphim_config_dir = xdg_config_home.join("terraphim");
    let data_dir = root.join("data");
    let dashmap_dir = root.join("dashmap");
    let sqlite_dir = root.join("sqlite");

    fs::create_dir_all(&home_dir)?;
    fs::create_dir_all(&terraphim_config_dir)?;
    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(&dashmap_dir)?;
    fs::create_dir_all(&sqlite_dir)?;

    // Write a per-test settings.toml that scopes ALL persistence profiles to
    // this hermetic root. Otherwise terraphim_settings ships defaults that
    // hardcode /tmp/terraphim_sqlite + /tmp/terraphim_dashmap; multiple
    // processes (other tests, ADF agents, dev REPL) would contend on the
    // same SQLite WAL and update_selected_role's save() would block for
    // minutes. See settings_local_dev.toml for the upstream defaults.
    //
    // terraphim_settings uses `directories::ProjectDirs` to locate the
    // config dir, which differs by platform:
    //   - Linux:   $XDG_CONFIG_HOME/terraphim or $HOME/.config/terraphim
    //   - macOS:   $HOME/Library/Application Support/com.aks.terraphim
    //   - Windows: %APPDATA%/aks/terraphim/config
    // Write to all three under the hermetic HOME so whichever runs first
    // finds our settings.
    let sqlite_db = sqlite_dir.join("terraphim.db");
    let role_config = workspace_root()?
        .join("crates/terraphim_agent/tests/fixtures/terraphim_engineer_config.json");

    let settings_toml = format!(
        r#"
server_hostname = "127.0.0.1:8000"
api_endpoint = "http://localhost:8000/api"
initialized = "false"
default_data_path = "{data}"
role_config = "{role_config}"

[profiles.dashmap]
type = "dashmap"
root = "{dashmap}"

[profiles.sqlite]
type = "sqlite"
datadir = "{sqlite}"
connection_string = "{db}"
table = "terraphim_kv"
"#,
        data = data_dir.display(),
        dashmap = dashmap_dir.display(),
        sqlite = sqlite_dir.display(),
        db = sqlite_db.display(),
        role_config = role_config.display(),
    );
    let settings_dirs = [
        terraphim_config_dir.clone(),
        home_dir
            .join("Library")
            .join("Application Support")
            .join("com.aks.terraphim"),
    ];
    for dir in &settings_dirs {
        fs::create_dir_all(dir)?;
        fs::write(dir.join("settings.toml"), &settings_toml)?;
    }

    let workspace = workspace_root()?;

    cmd.current_dir(&workspace)
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("TERRAPHIM_SETTINGS_PATH", &terraphim_config_dir)
        .env("TERRAPHIM_DEFAULT_DATA_PATH", &data_dir);

    Ok(())
}
