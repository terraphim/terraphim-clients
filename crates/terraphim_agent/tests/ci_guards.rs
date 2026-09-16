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

/// The GitHub Actions `taiki-e/install-action` tool versions must match the
/// locally-installed `cargo-llvm-cov` and `cargo-nextest` so a `cargo install`
/// on the native lane (which is `--locked` to the workspace) and the GH lane
/// (which is hand-pinned in `.github/workflows/ci.yml:30`) stay in lockstep.
///
/// If you upgrade the local toolchain and forget to bump the GH `with: tool:`
/// block, the two runners will produce coverage reports from different
/// rustc-instrumentation ABIs. The drift shows up as identical test sets but
/// divergent SF: counts in the lcov artefacts. Refs #313.
#[test]
fn coverage_tool_pinning_matches_local_toolchain() {
    let root = workspace_root();

    // The GH ci.yml `with: tool:` line we want to keep in sync with.
    let ci_yml = root.join(".github/workflows/ci.yml");
    assert!(ci_yml.is_file(), "missing {}", ci_yml.display());
    let ci_text = std::fs::read_to_string(&ci_yml).expect("read ci.yml");

    // Extract the `tool: cargo-llvm-cov@vX.Y.Z,nextest@vX.Y.Z` value.
    let pinned_block = ci_text
        .lines()
        .find(|l| l.trim_start().starts_with("tool:"))
        .expect("ci.yml has no `tool:` line; the GH coverage toolchain is unpinned");
    let pinned_block = pinned_block.trim_start();
    let pinned_block = pinned_block
        .strip_prefix("tool:")
        .expect("expected `tool:` prefix")
        .trim();

    let mut pinned = std::collections::HashMap::<&str, &str>::new();
    for entry in pinned_block.split(',') {
        let entry = entry.trim();
        let (name, version) = entry
            .split_once('@')
            .unwrap_or_else(|| panic!("expected `name@version` in `tool:` block, got `{}`", entry));
        // Strip the leading `v` so `cargo-llvm-cov@v0.8.5` matches the
        // local `cargo llvm-cov --version` output of `cargo-llvm-cov 0.8.5`.
        let version = version.strip_prefix('v').unwrap_or(version);
        pinned.insert(name, version);
    }
    let gh_cov = pinned.get("cargo-llvm-cov").copied().unwrap_or_else(|| {
        panic!(
            "ci.yml `tool:` block does not pin cargo-llvm-cov; got `{}`",
            pinned_block
        )
    });
    let gh_nextest = pinned.get("nextest").copied().unwrap_or_else(|| {
        panic!(
            "ci.yml `tool:` block does not pin nextest; got `{}`",
            pinned_block
        )
    });

    // Resolve the locally-installed versions.
    let cov_out = Command::new(env!("CARGO"))
        .args(["llvm-cov", "--version"])
        .output()
        .expect("run cargo llvm-cov --version");
    assert!(
        cov_out.status.success(),
        "cargo llvm-cov --version failed ({}):\n{}",
        cov_out.status,
        String::from_utf8_lossy(&cov_out.stderr),
    );
    let local_cov = String::from_utf8_lossy(&cov_out.stdout)
        .trim()
        .trim_start_matches("cargo-llvm-cov ")
        .trim()
        .to_string();

    let nextest_out = Command::new("cargo-nextest")
        .args(["--version"])
        .output()
        .expect("run cargo-nextest --version");
    let local_nextest = if nextest_out.status.success() {
        String::from_utf8_lossy(&nextest_out.stdout)
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string()
    } else {
        // cargo nextest --version is also valid.
        let nextest_alt = Command::new(env!("CARGO"))
            .args(["nextest", "--version"])
            .output()
            .expect("run cargo nextest --version");
        assert!(
            nextest_alt.status.success(),
            "cargo nextest --version failed ({}):\n{}",
            nextest_alt.status,
            String::from_utf8_lossy(&nextest_alt.stderr),
        );
        String::from_utf8_lossy(&nextest_alt.stdout)
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string()
    };

    assert_eq!(
        local_cov, gh_cov,
        "cargo-llvm-cov version drift: local toolchain has {local_cov}, but \
         .github/workflows/ci.yml pins {gh_cov}. Bump the GH `with: tool:` block \
         (or downgrade the local toolchain) so the two runners use the same \
         rustc-instrumentation ABI. Refs #313."
    );
    assert_eq!(
        local_nextest, gh_nextest,
        "cargo-nextest version drift: local toolchain has {local_nextest}, but \
         .github/workflows/ci.yml pins {gh_nextest}. Bump the GH `with: tool:` \
         block (or downgrade the local toolchain) so the two runners agree. \
         Refs #313."
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
