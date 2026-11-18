//! State persistence for update history
//!
//! This module handles saving and loading update history to disk,
//! providing persistent state management for the auto-update system.

use anyhow::{Context, Result};
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};

/// Get the default path for update history storage
///
/// Returns `~/.config/terraphim/update_history.json` on Unix systems
/// and appropriate location on other platforms.
pub fn get_history_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("Could not determine config directory")?;
    get_history_path_in(&config_dir)
}

/// The update-history path under an explicit configuration root.
///
/// The root is a parameter so a caller (a test, or a tool that wants the
/// history somewhere else) does not have to redirect the process environment:
/// `std::env::set_var` is `unsafe` and not thread safe, and this crate's tests
/// ran in the same binary as every other test in the crate
/// (terraphim-clients#438).
pub fn get_history_path_in(config_root: &Path) -> Result<PathBuf> {
    let mut config_dir = config_root.to_path_buf();
    config_dir.push("terraphim");
    fs::create_dir_all(&config_dir).context("Could not create terraphim config directory")?;

    Ok(config_dir.join("update_history.json"))
}

/// Save update history to disk
///
/// # Arguments
/// * `history` - The UpdateHistory to save
///
/// # Returns
/// * `Ok(())` - Successfully saved
/// * `Err(anyhow::Error)` - Error during save
///
/// # Example
/// ```no_run
/// use terraphim_update::config::UpdateHistory;
/// use terraphim_update::state::save_update_history;
///
/// let history = UpdateHistory::default();
/// save_update_history(&history)?;
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn save_update_history(history: &crate::config::UpdateHistory) -> Result<()> {
    let path = get_history_path()?;
    save_update_history_in(history, &path)
}

/// Save the update history to an explicit path.
pub fn save_update_history_in(history: &crate::config::UpdateHistory, path: &Path) -> Result<()> {
    let serialized =
        serde_json::to_string_pretty(history).context("Failed to serialize update history")?;

    fs::write(path, serialized).context(format!("Failed to write update history to {:?}", path))?;

    tracing::debug!("Update history saved to {:?}", path);
    Ok(())
}

/// Load update history from disk
///
/// Returns a default UpdateHistory if the file doesn't exist.
///
/// # Returns
/// * `Ok(UpdateHistory)` - Loaded or default history
/// * `Err(anyhow::Error)` - Error during load
///
/// # Example
/// ```no_run
/// use terraphim_update::state::load_update_history;
///
/// let history = load_update_history()?;
/// println!("Current version: {}", history.current_version);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn load_update_history() -> Result<crate::config::UpdateHistory> {
    let path = get_history_path()?;
    load_update_history_in(&path)
}

/// Load the update history from an explicit path.
pub fn load_update_history_in(path: &Path) -> Result<crate::config::UpdateHistory> {
    if !path.exists() {
        tracing::debug!("Update history file not found, using defaults");
        return Ok(crate::config::UpdateHistory::default());
    }

    let content = fs::read_to_string(path)
        .context(format!("Failed to read update history from {:?}", path))?;

    let history: crate::config::UpdateHistory =
        serde_json::from_str(&content).context("Failed to deserialize update history")?;

    tracing::debug!("Update history loaded from {:?}", path);
    Ok(history)
}

/// Delete update history file
///
/// # Returns
/// * `Ok(())` - Successfully deleted or didn't exist
/// * `Err(anyhow::Error)` - Error during deletion
pub fn delete_update_history() -> Result<()> {
    let path = get_history_path()?;
    delete_update_history_in(&path)
}

/// Delete the update history at an explicit path.
pub fn delete_update_history_in(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).context(format!("Failed to delete update history at {:?}", path))?;
        tracing::debug!("Update history deleted from {:?}", path);
    } else {
        tracing::debug!("Update history file not found, nothing to delete");
    }

    Ok(())
}

/// Check if update history file exists
///
/// # Returns
/// * `true` - File exists
/// * `false` - File doesn't exist
pub fn history_exists() -> bool {
    match get_history_path() {
        Ok(path) => path.exists(),
        Err(_) => false,
    }
}

/// Whether the update history exists at an explicit path.
pub fn history_exists_in(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{UpdateCheckEntry, UpdateCheckResult, UpdateHistory};
    use serial_test::serial;
    use tempfile::TempDir;

    /// Helper to create a temporary config directory for testing.
    ///
    /// The tests point the history functions at this directory with the
    /// `_in` variants instead of redirecting `HOME` and friends: mutating the
    /// process environment is `unsafe` and not thread safe, and this crate's
    /// tests share a binary with every other test in the crate
    /// (terraphim-clients#438).
    fn setup_temp_config_dir() -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join("terraphim");
        fs::create_dir_all(&config_path).expect("Failed to create config dir");
        temp_dir
    }

    /// The update-history path inside a test's temporary config directory.
    fn history_path_in(temp_dir: &TempDir) -> PathBuf {
        get_history_path_in(temp_dir.path()).expect("Failed to resolve history path")
    }

    #[test]
    #[serial]
    fn test_save_and_load_history() {
        let _temp_dir = setup_temp_config_dir();
        // Clean up first
        let _ = delete_update_history_in(&history_path_in(&_temp_dir));

        let original = UpdateHistory {
            current_version: "1.0.0".to_string(),
            ..Default::default()
        };

        save_update_history_in(&original, &history_path_in(&_temp_dir))
            .expect("Failed to save history");
        assert!(
            history_exists_in(&history_path_in(&_temp_dir)),
            "History file should exist after save"
        );

        let loaded =
            load_update_history_in(&history_path_in(&_temp_dir)).expect("Failed to load history");
        assert_eq!(loaded.current_version, "1.0.0");
    }

    #[test]
    #[serial]
    fn test_load_default_when_missing() {
        let _temp_dir = setup_temp_config_dir();
        // Clean up any existing history file first
        let _ = delete_update_history_in(&history_path_in(&_temp_dir));

        // Now test loading when file doesn't exist
        let history =
            load_update_history_in(&history_path_in(&_temp_dir)).expect("Should load default");
        assert_eq!(history.current_version, String::new());
        assert!(history.backup_versions.is_empty());
    }

    #[test]
    #[serial]
    fn test_history_with_check_entries() {
        let _temp_dir = setup_temp_config_dir();
        let _ = delete_update_history_in(&history_path_in(&_temp_dir));

        let mut history = UpdateHistory {
            current_version: "1.0.0".to_string(),
            ..Default::default()
        };

        let entry = UpdateCheckEntry {
            timestamp: jiff::Timestamp::now(),
            result: UpdateCheckResult::UpToDate,
        };
        history.add_check_entry(entry);

        save_update_history_in(&history, &history_path_in(&_temp_dir))
            .expect("Failed to save history");

        let loaded =
            load_update_history_in(&history_path_in(&_temp_dir)).expect("Failed to load history");
        assert_eq!(loaded.check_history.len(), 1);
    }

    #[test]
    #[serial]
    fn test_history_with_pending_update() {
        let _temp_dir = setup_temp_config_dir();
        let _ = delete_update_history_in(&history_path_in(&_temp_dir));

        let info = crate::config::UpdateInfo {
            version: "1.1.0".to_string(),
            release_date: jiff::Timestamp::now(),
            notes: "Test release".to_string(),
            download_url: "https://example.com/binary".to_string(),
            signature_url: "https://example.com/binary.sig".to_string(),
            arch: "x86_64".to_string(),
        };

        let history = UpdateHistory {
            current_version: "1.0.0".to_string(),
            pending_update: Some(info),
            ..Default::default()
        };

        save_update_history_in(&history, &history_path_in(&_temp_dir))
            .expect("Failed to save history");

        let loaded =
            load_update_history_in(&history_path_in(&_temp_dir)).expect("Failed to load history");
        assert!(loaded.pending_update.is_some());
        assert_eq!(loaded.pending_update.unwrap().version, "1.1.0");
    }

    #[test]
    #[serial]
    fn test_history_with_backups() {
        let _temp_dir = setup_temp_config_dir();
        let _ = delete_update_history_in(&history_path_in(&_temp_dir));

        let mut history = UpdateHistory {
            current_version: "1.0.0".to_string(),
            ..Default::default()
        };
        history.add_backup_version("0.9.0".to_string(), 3);
        history.add_backup_version("0.8.0".to_string(), 3);

        save_update_history_in(&history, &history_path_in(&_temp_dir))
            .expect("Failed to save history");

        let loaded =
            load_update_history_in(&history_path_in(&_temp_dir)).expect("Failed to load history");
        assert_eq!(loaded.backup_versions.len(), 2);
        assert_eq!(loaded.backup_versions[0], "0.9.0");
        assert_eq!(loaded.backup_versions[1], "0.8.0");
    }

    #[test]
    #[serial]
    fn test_delete_history() {
        let _temp_dir = setup_temp_config_dir();
        let _ = delete_update_history_in(&history_path_in(&_temp_dir));

        let history = UpdateHistory {
            current_version: "1.0.0".to_string(),
            ..Default::default()
        };

        save_update_history_in(&history, &history_path_in(&_temp_dir))
            .expect("Failed to save history");
        assert!(
            history_exists_in(&history_path_in(&_temp_dir)),
            "History should exist"
        );

        delete_update_history_in(&history_path_in(&_temp_dir)).expect("Failed to delete history");
        assert!(
            !history_exists_in(&history_path_in(&_temp_dir)),
            "History should not exist after delete"
        );
    }

    #[test]
    #[serial]
    fn test_delete_nonexistent_history() {
        let _temp_dir = setup_temp_config_dir();
        let result = delete_update_history();
        assert!(
            result.is_ok(),
            "Deleting non-existent history should succeed"
        );
    }

    #[test]
    #[serial]
    fn test_invalid_json_fails_gracefully() {
        let _temp_dir = setup_temp_config_dir();

        let path = get_history_path().expect("Failed to get history path");
        fs::write(&path, "invalid json").expect("Failed to write invalid json");

        let result = load_update_history();
        assert!(result.is_err(), "Should fail to load invalid JSON");
    }

    #[test]
    #[serial]
    fn test_schema_migration_compatibility() {
        let _temp_dir = setup_temp_config_dir();

        // Create a history file with only current_version (simulating old schema)
        let old_schema = r#"{
            "last_check": "2024-01-01T00:00:00Z",
            "current_version": "1.0.0",
            "pending_update": null,
            "backup_versions": [],
            "check_history": []
        }"#;

        let path = get_history_path().expect("Failed to get history path");
        fs::write(&path, old_schema).expect("Failed to write old schema");

        let history = load_update_history().expect("Should load old schema");
        assert_eq!(history.current_version, "1.0.0");
        assert!(history.pending_update.is_none());
    }

    #[test]
    #[serial]
    fn test_get_history_path() {
        let _temp_dir = setup_temp_config_dir();
        let path = get_history_path();
        assert!(path.is_ok(), "Should get history path");

        let path = path.unwrap();
        assert!(
            path.ends_with("update_history.json"),
            "Should have correct filename"
        );
        assert!(
            path.to_string_lossy().contains("terraphim"),
            "Should contain terraphim directory"
        );
    }
}
