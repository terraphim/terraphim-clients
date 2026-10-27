// crates/terraphim_agent/tests/support/kg_paths.rs
//
// KG path resolution helper for terraphim_agent tests.
//
// Refs: terraphim-agents#86 (post-polyrepo extraction path drift)
//       terraphim-agents commit 3cd4c00 (Refs #45)
//       terraphim-agents commit 26bd716 (Fixes #94) -- prior fix for cross_mode_consistency
//
// Background: Pre-2026-06-13, `terraphim_agent` was a single-crate workspace rooted
// at `/home/alex/projects/terraphim/terraphim-agent/`. The KG fixtures lived at
// `<workspace>/docs/src/kg/`. After the polyrepo extraction (3cd4c00), the crate
// moved to `terraphim/terraphim-agents/crates/terraphim_agent/` and the KG fixtures
// moved with it to `crates/terraphim_agent/docs/src/kg/`.
//
// Tests that hardcode `<workspace_root>/docs/src/kg` will fail because that path
// no longer exists. This helper resolves the crate's actual KG dir via
// `CARGO_MANIFEST_DIR` at compile time, so it works regardless of cwd.

use std::path::PathBuf;

/// Returns the absolute path to the vendored `terraphim_agent`'s `docs/src/kg/`
/// directory. Uses `CARGO_MANIFEST_DIR` at compile time, so the result is
/// deterministic and independent of cwd.
///
/// # Panics
///
/// Panics at runtime if the resolved path does not exist or is not a directory.
/// This is intentional: a missing KG dir means the test fixtures were not
/// vendored, which is a build-system bug, not a runtime condition to paper over.
pub fn crate_kg_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("src")
        .join("kg");

    assert!(
        dir.is_dir(),
        "terraphim_agent KG dir missing or not a directory: {}. \
         This usually means the fixtures weren't vendored with the crate. \
         See terraphim-agents#86.",
        dir.display(),
    );

    dir
}

/// Returns a writable scratch directory for KG files a test generates at
/// runtime, creating it if necessary.
///
/// Use this — never [`crate_kg_dir`] — as the destination for `fs::write`.
/// `crate_kg_dir()` resolves inside the crate source tree and every file under
/// it is tracked by git, so writing (or deleting) there mutates the repository:
/// the same test run stops reproducing the same tree, and any agent or CI job
/// that baselines a diff inherits changes it did not make.
///
/// The path is `<temp_dir>/terraphim_test_kg`, which the existing
/// `cleanup_test_resources()` helpers already remove.
///
/// # Panics
///
/// Panics if the directory cannot be created, which means the temp dir is not
/// writable — a broken environment, not a condition to paper over.
///
/// Refs: terraphim-agents#124.
pub fn scratch_kg_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("terraphim_test_kg");

    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("failed to create scratch KG dir {}: {e}", dir.display()));

    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_kg_dir_points_at_existing_directory() {
        let dir = crate_kg_dir();
        assert!(
            dir.is_dir(),
            "kg dir must be a directory: {}",
            dir.display()
        );
    }

    /// Refs: terraphim-agents#124 -- the scratch dir must never resolve inside
    /// the tracked source tree, or generated KG files dirty the repository.
    #[test]
    fn scratch_kg_dir_is_outside_the_tracked_kg_dir() {
        let scratch = scratch_kg_dir();
        let tracked = crate_kg_dir();

        assert!(
            !scratch.starts_with(&tracked),
            "scratch KG dir {} must not live under the tracked fixture dir {}",
            scratch.display(),
            tracked.display()
        );
        assert!(
            !scratch.starts_with(env!("CARGO_MANIFEST_DIR")),
            "scratch KG dir {} must not live under the crate source tree {}",
            scratch.display(),
            env!("CARGO_MANIFEST_DIR")
        );
    }

    /// Refs: terraphim-agents#124 -- callers `fs::write` into this directory, so
    /// it has to exist by the time they do.
    #[test]
    fn scratch_kg_dir_is_writable() {
        let dir = scratch_kg_dir();
        assert!(dir.is_dir(), "scratch dir must exist: {}", dir.display());

        let probe = dir.join("kg_paths_write_probe.md");
        std::fs::write(&probe, "probe")
            .unwrap_or_else(|e| panic!("write {} failed: {e}", probe.display()));
        std::fs::remove_file(&probe).ok();
    }

    #[test]
    fn crate_kg_dir_contains_at_least_one_fixture() {
        let dir = crate_kg_dir();
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read_dir({}) failed: {}", dir.display(), e))
            .filter_map(Result::ok)
            .collect();
        assert!(
            !entries.is_empty(),
            "kg dir must contain at least one fixture: {}",
            dir.display()
        );
    }
}
