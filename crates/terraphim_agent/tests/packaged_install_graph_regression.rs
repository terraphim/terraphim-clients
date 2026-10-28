//! Registry-free regression checks for terraphim-clients#95 and #431.
//!
//! These checks deliberately inspect the source manifest and workspace metadata
//! only. The registry-dependent package and install verification lives in the
//! `agent_package_release_gate` target and is run by the release script.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::{Version, VersionReq};

const CANONICAL_SPARSE_INDEX: &str =
    "sparse+https://git.terraphim.cloud/api/packages/terraphim/cargo/";
const KNOWN_BAD: [(&str, &str); 2] = [
    ("1.21.0", "stale lock pin"),
    ("1.21.1", "missing cursor-connector"),
];
const REQUIRED_FEATURES: [&str; 4] = [
    "tsa-full",
    "aider-connector",
    "cursor-connector",
    "search-index",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn verify_terraphim_registry_alias(config_path: &Path) -> Result<(), String> {
    let cargo_config = fs::read_to_string(config_path)
        .map_err(|error| format!("read {}: {error}", config_path.display()))?;
    let config: toml::Table = toml::from_str(&cargo_config)
        .map_err(|error| format!("parse {}: {error}", config_path.display()))?;
    let index = config
        .get("registries")
        .and_then(toml::Value::as_table)
        .and_then(|registries| registries.get("terraphim"))
        .and_then(toml::Value::as_table)
        .and_then(|terraphim| terraphim.get("index"))
        .and_then(toml::Value::as_str);

    if index == Some(CANONICAL_SPARSE_INDEX) {
        Ok(())
    } else {
        Err(format!(
            "the `terraphim` registry alias must map to {CANONICAL_SPARSE_INDEX}, found {index:?}"
        ))
    }
}

/// Obtain the floor from Cargo's view of the local workspace rather than
/// copying the sessions version into this regression test.
fn workspace_sessions_version(root: &Path) -> Version {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .expect("spawn cargo metadata --format-version 1");
    assert!(
        output.status.success(),
        "cargo metadata --format-version 1 failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse cargo metadata JSON");
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages array");
    let mut local_sessions = packages.iter().filter(|package| {
        package["name"].as_str() == Some("terraphim_sessions") && package["source"].is_null()
    });
    let package = local_sessions
        .next()
        .expect("local workspace terraphim_sessions package in cargo metadata");
    assert!(
        local_sessions.next().is_none(),
        "cargo metadata contains more than one source-less terraphim_sessions package"
    );
    Version::parse(
        package["version"]
            .as_str()
            .expect("workspace terraphim_sessions version"),
    )
    .expect("valid workspace terraphim_sessions version")
}

#[test]
fn agent_manifest_tracks_workspace_sessions_release() {
    let root = workspace_root();
    let workspace_version = workspace_sessions_version(&root);
    let manifest = fs::read_to_string(root.join("crates/terraphim_agent/Cargo.toml"))
        .expect("read terraphim_agent Cargo.toml");
    let document: toml::Table = toml::from_str(&manifest).expect("parse agent manifest");
    let dependency = document["dependencies"]["terraphim_sessions"]
        .as_table()
        .expect("terraphim_sessions must use an explicit dependency table");

    let requirement_text = dependency
        .get("version")
        .and_then(toml::Value::as_str)
        .expect("terraphim_sessions must have an explicit version requirement");
    let requirement = VersionReq::parse(requirement_text)
        .expect("terraphim_sessions must have a valid version requirement");
    assert!(
        requirement.matches(&workspace_version),
        "terraphim_sessions requirement {requirement} must match workspace version {workspace_version}"
    );

    for (version, reason) in KNOWN_BAD {
        let version = Version::parse(version).expect("known-bad version must be valid semver");
        assert!(
            !requirement.matches(&version),
            "terraphim_sessions requirement {requirement} must exclude known-bad {version}: {reason}"
        );
    }

    assert_eq!(
        dependency.get("registry").and_then(toml::Value::as_str),
        Some("terraphim"),
        "terraphim_sessions must use the canonical `registry = \"terraphim\"` attribute"
    );
    verify_terraphim_registry_alias(&root.join(".cargo/config.toml"))
        .expect("the `terraphim` registry alias must map to the canonical sparse index");

    let features = dependency
        .get("features")
        .and_then(toml::Value::as_array)
        .expect("terraphim_sessions must declare its required features");
    for required in REQUIRED_FEATURES {
        assert!(
            features
                .iter()
                .any(|feature| feature.as_str() == Some(required)),
            "terraphim_sessions must enable required feature {required}"
        );
    }

    assert_eq!(
        dependency.get("optional").and_then(toml::Value::as_bool),
        Some(true),
        "terraphim_sessions must remain optional behind repl-sessions"
    );
    assert_eq!(
        dependency.get("path").and_then(toml::Value::as_str),
        Some("../terraphim_sessions"),
        "workspace builds must continue to use the sibling sessions crate"
    );
}

#[test]
fn rejects_noncanonical_terraphim_registry_alias() {
    let source = workspace_root().join(".cargo/config.toml");
    let config = fs::read_to_string(&source).expect("read Cargo config fixture");
    let modified = config.replace(
        CANONICAL_SPARSE_INDEX,
        "sparse+https://example.invalid/cargo/",
    );
    assert_ne!(modified, config, "fixture must contain the canonical index");

    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, modified).expect("write modified Cargo config fixture");

    let error = verify_terraphim_registry_alias(&config_path)
        .expect_err("a noncanonical registry index must fail verification");
    assert!(
        error.contains("must map to") && error.contains("example.invalid"),
        "failure must identify the invalid registry mapping: {error}"
    );
}
