//! Repository guards that must run in CI.
//!
//! These are Rust tests rather than shell steps because the Gitea runner
//! enforces a program allowlist on workflow steps:
//!
//! ```text
//! runner error: policy rejected command:
//!   program `scripts/tests/publish-gate-test.sh` is not on the allowlist
//! ```
//!
//! Every other terraphim repo's `native-ci` runs `cargo` and nothing else. A
//! test binary invoked by `cargo test` is allowlisted, and spawning tools from
//! inside it is fine -- `packaged_install_graph_regression` already runs
//! `cargo package` this way. Refs #118.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/terraphim_agent
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// No `terraphim_*` crate may appear at more than one version or source.
///
/// Two copies of a crate mean two copies of its types, which the compiler
/// reports as `expected terraphim_config::ConfigState, found ConfigState` --
/// indistinguishable from a bug in the calling code, and the reason #112 and
/// #118 each cost hours. Fail here, with the crate named.
///
/// Third-party duplicates are ignored: they are normal in a graph this size and
/// nothing in this repo can resolve them.
#[test]
fn no_duplicate_terraphim_crates() {
    let root = workspace_root();
    let out = Command::new(env!("CARGO"))
        .args(["tree", "--workspace", "--all-features", "--duplicates"])
        .current_dir(&root)
        .output()
        .expect("run cargo tree");

    assert!(
        out.status.success(),
        "`cargo tree --duplicates` failed ({}); this is an environment problem, \
         not a duplicate, and is not being treated as a pass:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut dupes: Vec<&str> = stdout
        .lines()
        .map(str::trim_end)
        .filter(|l| l.starts_with("terraphim") && l.contains(" v"))
        .collect();
    dupes.sort_unstable();
    dupes.dedup();

    assert!(
        dupes.is_empty(),
        "terraphim crates resolved at more than one version:\n  {}\n\n\
         Every terraphim_* dependency must resolve to a single version from the \
         Gitea registry. A crates.io copy creeps in when a dependency names a \
         version the [patch.crates-io] entry does not satisfy (an exact `=x.y.z` \
         pin does not satisfy a `^x.y.w` requirement, and cargo falls back to \
         crates.io silently), or when a manifest omits `registry = \"terraphim\"`. \
         Run `cargo tree -i <crate>@<version>` to find the offender.",
        dupes.join("\n  "),
    );
}

/// The publish provenance gate must keep working.
///
/// It is what stops another unreproducible release: four of the last four
/// artefacts before #112 were published from dirty trees or commits unreachable
/// from `main`. Its own tests build throwaway repos per failure mode.
#[test]
fn publish_gate_tests_pass() {
    let root = workspace_root();
    let script = root.join("scripts/tests/publish-gate-test.sh");
    assert!(script.is_file(), "missing {}", script.display());

    let out = Command::new("bash")
        .arg(&script)
        .current_dir(&root)
        .output()
        .expect("run publish-gate tests");

    assert!(
        out.status.success(),
        "publish-gate tests failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
