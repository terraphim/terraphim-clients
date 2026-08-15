//! Regression test for terraphim-clients#95: the published `terraphim_agent`
//! package must carry an install graph that actually resolves.
//!
//! Root cause of #95: `terraphim_agent` 1.21.1 declared
//! `terraphim_sessions = "1.6.0"`, so the Cargo.lock packaged into the .crate
//! pinned the stale/broken `terraphim_sessions` 1.21.0, and
//! `cargo install --locked --registry terraphim terraphim_agent --version 1.21.1`
//! failed with an unresolved `terraphim_markdown_parser` dependency.
//!
//! This test exercises the *packaged* artifact (`cargo package`), not the
//! workspace path build, and asserts:
//!   1. packaging succeeds (the publish-time dependency graph resolves),
//!   2. the packaged manifest requires a `terraphim_sessions` floor that
//!      excludes the broken 1.21.0/1.21.1 releases,
//!   3. the packaged dep points at the canonical terraphim sparse index
//!      (the `registry` attribute is preserved through `cargo package`'s
//!      normalization), so the manifest knows the source of truth,
//!   4. the packaged Cargo.lock pins `terraphim_sessions` >= 1.21.2 with the
//!      `source` field set to the canonical sparse index, and the resolved
//!      graph contains `terraphim-markdown-parser`,
//!   5. `cargo install --path <unpacked crate> --locked --root <fresh>` from
//!      the workspace root succeeds, produces `bin/terraphim-agent` (with the
//!      platform `EXE_SUFFIX`), and the installed binary reports
//!      `terraphim-agent 1.21.2` from `--version`. This is the end-to-end
//!      proof that the published dependency graph is installable as shipped.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::{Version, VersionReq};
use tempfile::TempDir;

/// Minimum terraphim_sessions version that carries the cursor-connector
/// feature and the terraphim-markdown-parser dependency (issue #95).
const FIXED_SESSIONS_FLOOR: Version = Version::new(1, 21, 2);

/// Canonical sparse index for the terraphim registry. The packaged manifest
/// must reference this URL (via the `registry-index` attribute that cargo
/// produces from `registry = "terraphim"`) and the packaged Cargo.lock must
/// pin `terraphim_sessions` against this source.
const CANONICAL_SPARSE_INDEX: &str =
    "sparse+https://git.terraphim.cloud/api/packages/terraphim/cargo/";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Resolve the terraphim_agent package version, expanding `version.workspace`.
fn agent_version(root: &Path) -> Version {
    let manifest = fs::read_to_string(root.join("crates/terraphim_agent/Cargo.toml"))
        .expect("read agent manifest");
    let doc: toml::Table = toml::from_str(&manifest).expect("parse agent manifest");
    let package = doc.get("package").expect("[package]");
    match package.get("version") {
        Some(toml::Value::String(v)) => Version::parse(v).expect("agent version"),
        Some(toml::Value::Table(t)) if t.get("workspace").is_some() => {
            let root_manifest =
                fs::read_to_string(root.join("Cargo.toml")).expect("read root manifest");
            let root_doc: toml::Table = toml::from_str(&root_manifest).expect("parse root");
            let v = root_doc["workspace"]["package"]["version"]
                .as_str()
                .expect("workspace version");
            Version::parse(v).expect("workspace version")
        }
        other => panic!("unexpected version field: {other:?}"),
    }
}

/// Parse `name`/`version` pairs out of a Cargo.lock without a TOML dep on the
/// lock format details: every `[[package]]` block starts with name+version.
fn lock_version_of(lock: &str, name: &str) -> Option<Version> {
    let doc: toml::Table = toml::from_str(lock).expect("parse packaged Cargo.lock");
    doc.get("package")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
        .and_then(|p| p.get("version").and_then(|v| v.as_str()))
        .map(|v| Version::parse(v).expect("lock version"))
}

/// Return the `source` field of the first `[[package]]` block whose
/// `name` and `version` both match. This is the registry URL the packaged
/// Cargo.lock pins the dep against and must be the canonical sparse index.
fn lock_source_of(lock: &str, name: &str, version: &Version) -> Option<String> {
    let doc: toml::Table = toml::from_str(lock).expect("parse packaged Cargo.lock");
    doc.get("package")?
        .as_array()?
        .iter()
        .find(|p| {
            p.get("name").and_then(|n| n.as_str()) == Some(name)
                && p.get("version").and_then(|v| v.as_str()) == Some(version.to_string().as_str())
        })
        .and_then(|p| p.get("source").and_then(|s| s.as_str()))
        .map(|s| s.to_string())
}

fn lock_has_package(lock: &str, name: &str) -> bool {
    let doc: toml::Table = toml::from_str(lock).expect("parse packaged Cargo.lock");
    doc.get("package")
        .and_then(|p| p.as_array())
        .map(|pkgs| {
            pkgs.iter()
                .any(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
        })
        .unwrap_or(false)
}

/// Extract the `version = "..."` requirement of a dependency from the
/// packaged (normalized) Cargo.toml, e.g. `[dependencies.terraphim_sessions]`.
fn packaged_dep_req(manifest: &str, dep: &str) -> VersionReq {
    let doc: toml::Table = toml::from_str(manifest).expect("parse packaged manifest");
    for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
        if let Some(deps) = doc.get(section).and_then(|d| d.as_table())
            && let Some(entry) = deps.get(dep)
        {
            let req = entry
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("packaged dep {dep} has no version req"));
            return VersionReq::parse(req).expect("valid version req");
        }
    }
    panic!("dependency {dep} not found in packaged manifest");
}

/// Read the registry URL the packaged manifest pins `dep` against. cargo
/// normalizes `registry = "terraphim"` into the concrete `registry-index`
/// URL, so we look for either key to keep the assertion robust against
/// future cargo formatting changes.
fn packaged_dep_registry(manifest: &str, dep: &str) -> String {
    let doc: toml::Table = toml::from_str(manifest).expect("parse packaged manifest");
    for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
        let Some(entry) = doc
            .get(section)
            .and_then(|d| d.as_table())
            .and_then(|t| t.get(dep))
        else {
            continue;
        };
        let entry = entry
            .as_table()
            .expect("dep entry must be a table in packaged manifest");
        if let Some(idx) = entry.get("registry-index").and_then(|v| v.as_str()) {
            return idx.to_string();
        }
        if let Some(reg) = entry.get("registry").and_then(|v| v.as_str()) {
            return reg.to_string();
        }
    }
    panic!("dependency {dep} has no registry attribute in packaged manifest");
}

#[test]
fn packaged_agent_install_graph_uses_canonical_sparse_index() {
    let root = workspace_root();
    let version = agent_version(&root);
    let package_target = TempDir::new().expect("create isolated cargo package target");

    // 1. Build the actual publish artifact. This regenerates the packaged
    //    Cargo.lock against the registries exactly like `cargo publish` does.
    //    An isolated target also avoids Cargo 1.93 leaving trailing bytes when
    //    overwriting a previously larger .crate artifact.
    let status = Command::new(env!("CARGO"))
        .args([
            "package",
            "-p",
            "terraphim_agent",
            "--allow-dirty",
            "--no-verify",
        ])
        .env("CARGO_TARGET_DIR", package_target.path())
        .current_dir(&root)
        .status()
        .expect("spawn cargo package");
    assert!(
        status.success(),
        "cargo package must succeed: the published install graph has to resolve (#95)"
    );

    let crate_file = package_target
        .path()
        .join(format!("package/terraphim_agent-{version}.crate"));
    assert!(crate_file.exists(), "packaged .crate must exist");

    // 2. Unpack the artifact into a TempDir that cleans itself up on drop,
    //    so the test is hermetic and doesn't leave predictable temp dirs
    //    behind on success or failure.
    let extract_dir = TempDir::new().expect("create tempdir for packaged crate");
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&crate_file)
        .arg("-C")
        .arg(extract_dir.path())
        .status()
        .expect("spawn tar");
    assert!(status.success(), "extract packaged crate");
    let pkg_dir = extract_dir
        .path()
        .join(format!("terraphim_agent-{version}"));

    let packaged_manifest =
        fs::read_to_string(pkg_dir.join("Cargo.toml")).expect("packaged Cargo.toml");
    let packaged_lock = fs::read_to_string(pkg_dir.join("Cargo.lock"))
        .expect("packaged Cargo.lock must ship with the binary crate");

    // 3. The packaged manifest must not resolve terraphim_sessions via a
    //    workspace path, and its version floor must exclude the broken
    //    1.21.0 (stale lock pin) and 1.21.1 (missing cursor-connector).
    let req = packaged_dep_req(&packaged_manifest, "terraphim_sessions");
    assert!(
        !req.matches(&Version::new(1, 21, 0)),
        "packaged terraphim_sessions req {req} must exclude broken 1.21.0 (#95)"
    );
    assert!(
        !req.matches(&Version::new(1, 21, 1)),
        "packaged terraphim_sessions req {req} must exclude 1.21.1 without cursor-connector (#95)"
    );
    assert!(
        req.matches(&FIXED_SESSIONS_FLOOR),
        "packaged terraphim_sessions req {req} must accept {FIXED_SESSIONS_FLOOR} (#95)"
    );

    // 4. The packaged manifest must pin terraphim_sessions to the canonical
    //    terraphim sparse index. cargo normalizes `registry = "terraphim"`
    //    into the concrete `registry-index` URL, so we compare the resolved
    //    URL against the canonical sparse index.
    let registry = packaged_dep_registry(&packaged_manifest, "terraphim_sessions");
    assert_eq!(
        registry, CANONICAL_SPARSE_INDEX,
        "packaged terraphim_sessions must be sourced from the canonical terraphim sparse index (#95)"
    );

    // 5. The packaged lock (used by `cargo install --locked`) must pin the
    //    fixed sessions crate against the canonical sparse index, and the
    //    resolved graph must contain the markdown parser dependency that
    //    was unresolved in the broken 1.21.1 release.
    let locked_version = lock_version_of(&packaged_lock, "terraphim_sessions")
        .expect("terraphim_sessions must be in the packaged lock");
    assert!(
        locked_version >= FIXED_SESSIONS_FLOOR,
        "packaged lock pins terraphim_sessions {locked_version}, need >= {FIXED_SESSIONS_FLOOR} (#95)"
    );
    let locked_source = lock_source_of(&packaged_lock, "terraphim_sessions", &locked_version)
        .expect("packaged lock must record a source for terraphim_sessions");
    assert_eq!(
        locked_source, CANONICAL_SPARSE_INDEX,
        "packaged lock must pin terraphim_sessions against the canonical terraphim sparse index (#95)"
    );
    assert!(
        lock_has_package(&packaged_lock, "terraphim-markdown-parser"),
        "packaged lock must resolve terraphim-markdown-parser (was unresolved in #95)"
    );
    assert!(
        lock_has_package(&packaged_lock, "terraphim-session-analyzer"),
        "packaged lock must resolve terraphim-session-analyzer"
    );

    // 6. End-to-end proof: a real `cargo install --path --debug` against the
    //    unpacked packaged crate, using a fresh CARGO_TARGET_DIR for compile
    //    isolation and a fresh --root TempDir so the test does not mutate the
    //    user's cargo home. Debug mode compiles the same locked dependency
    //    graph without making this release-safety regression pay for a full
    //    optimized build; the post-publication Guardian runs the exact default
    //    release-profile registry install. Run cargo from the workspace root
    //    so .cargo/config.toml supplies the named `terraphim` registry.
    let install_target = TempDir::new().expect("create isolated cargo install target");
    let install_root = TempDir::new().expect("create fresh install --root");
    let install_output = Command::new(env!("CARGO"))
        .args(["install", "--path"])
        .arg(&pkg_dir)
        .args(["--locked", "--debug", "--root"])
        .arg(install_root.path())
        .env("CARGO_TARGET_DIR", install_target.path())
        .current_dir(&root)
        .output()
        .expect("spawn cargo install --path");
    assert!(
        install_output.status.success(),
        "cargo install --path <unpacked {}-{version}> --locked --root <fresh> must succeed (#95);\nstdout:\n{}\nstderr:\n{}",
        "terraphim_agent",
        String::from_utf8_lossy(&install_output.stdout),
        String::from_utf8_lossy(&install_output.stderr),
    );

    // The installed binary must be present at `<root>/bin/terraphim-agent`
    // (plus the platform `EXE_SUFFIX`, e.g. ".exe" on Windows) and it must
    // report the exact version we just packaged.
    let bin_name = format!("terraphim-agent{}", std::env::consts::EXE_SUFFIX);
    let installed_bin = install_root.path().join("bin").join(&bin_name);
    assert!(
        installed_bin.exists(),
        "install --root must produce bin/{bin_name} (looked at {})",
        installed_bin.display(),
    );

    let version_output = Command::new(&installed_bin)
        .arg("--version")
        .output()
        .expect("spawn installed terraphim-agent --version");
    assert!(
        version_output.status.success(),
        "installed {bin_name} --version must succeed (exit {:?})",
        version_output.status.code(),
    );
    let stdout = String::from_utf8_lossy(&version_output.stdout);
    let reported_version = stdout
        .split_whitespace()
        .last()
        .expect("installed binary --version must report a version token");
    let expected_version = version.to_string();
    assert_eq!(
        reported_version, expected_version,
        "installed {bin_name} must report exact version {expected_version} (#95); got: {stdout}",
    );
}
