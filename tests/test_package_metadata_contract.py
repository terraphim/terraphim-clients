"""Package-metadata / license contract for Omarchy-packaged crates.

Gitea #246 (see docs/plans/design-omarchy-metadata-licenses-2026-09-11.md):
fix stale `repository` metadata and dangling deb `license-file` references
for the two workspace crates that actually ship packaged (Omarchy/deb)
binaries, and enforce the contract with tests so it cannot silently regress.

Scope note: `discover_deb_packaged_crates()` scans `crates/*/Cargo.toml` for
a `[package.metadata.deb]` section rather than hard-coding the crate list;
`test_discovered_deb_packaged_crates_matches_intended_set` asserts that scan
finds exactly `terraphim_agent` and `terraphim_grep` today. The
repository-consistency check is intentionally scoped to deb-packaged crates,
not all 11 workspace members -- see the design doc's Scope / Non-goals
section for why the other 9 crates' stale GitHub URLs are an out-of-scope
follow-up.

Helper functions here (`assert_repository_matches_workspace`,
`assert_license_file_resolves`, `assert_tag_matches_workspace_version`) are
intended for reuse by O3's release-workflow contract tests rather than
re-implementing Cargo.toml/tag parsing -- see the design doc's Handoff to
O3 section.
"""

import hashlib
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKSPACE_CARGO_TOML = ROOT / "Cargo.toml"
CRATES_DIR = ROOT / "crates"

# The set of workspace crates this contract is *intended* to cover today
# (see the design doc's Scope / Non-goals). Discovery below must find
# exactly this set -- a mismatch means a crate gained or lost
# [package.metadata.deb] without this contract being updated.
INTENDED_DEB_PACKAGED_CRATES = ("terraphim_agent", "terraphim_grep")


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def crate_cargo_toml(crate_name: str) -> Path:
    return ROOT / "crates" / crate_name / "Cargo.toml"


def discover_deb_packaged_crates() -> tuple:
    """Deterministically scan crates/*/Cargo.toml for a
    [package.metadata.deb] section, returning crate directory names in
    sorted order. Replaces a one-time hard-coded crate list so a crate
    that later gains (or loses) deb packaging metadata is caught by
    test_discovered_deb_packaged_crates_matches_intended_set instead of
    silently falling outside this contract."""
    discovered = []
    for cargo_toml in sorted(CRATES_DIR.glob("*/Cargo.toml")):
        package = load_toml(cargo_toml).get("package", {})
        if "deb" in package.get("metadata", {}):
            discovered.append(cargo_toml.parent.name)
    return tuple(discovered)


DEB_PACKAGED_CRATES = discover_deb_packaged_crates()


def workspace_repository() -> str:
    return load_toml(WORKSPACE_CARGO_TOML)["workspace"]["package"]["repository"]


def workspace_version() -> str:
    return load_toml(WORKSPACE_CARGO_TOML)["workspace"]["package"]["version"]


def assert_tag_matches_workspace_version(test_case: unittest.TestCase, tag: str) -> None:
    """Reusable helper, no release-YAML edits required: fail if a release
    tag (e.g. 'v9.9.9') does not match the checked-in workspace version.
    O3's workflow-contract tests can import and call this directly instead
    of re-implementing Cargo.toml + tag parsing."""
    version = workspace_version()
    tag_version = tag[1:] if tag.startswith("v") else tag
    test_case.assertEqual(
        tag_version,
        version,
        f"release tag {tag!r} does not match workspace version {version!r}",
    )


def crate_deb_metadata(crate_name: str) -> dict:
    return load_toml(crate_cargo_toml(crate_name))["package"]["metadata"]["deb"]


def crate_package_table(crate_name: str) -> dict:
    return load_toml(crate_cargo_toml(crate_name))["package"]


EXPECTED_PACKAGE_LICENSE_FILE = {
    "terraphim_agent": "../../LICENSE-Apache-2.0",
    "terraphim_grep": "../../LICENSE-MIT",
}


def assert_package_license_file_resolves(test_case: unittest.TestCase, crate_name: str) -> Path:
    """Reusable helper: the crate's *package-level* `[package].license-file`
    (distinct from `[package.metadata.deb].license-file`) must be present
    and resolve to the exact expected root license file. `cargo package`
    only proves license inclusion (Issue #246's explicit acceptance
    criterion) via this top-level field -- Cargo >= 1.43 copies an
    out-of-package-root `license-file` into the package output under its
    basename; the deb-metadata field alone is invisible to `cargo package
    --list`."""
    pkg = crate_package_table(crate_name)
    entry = pkg.get("license-file")
    test_case.assertIsNotNone(
        entry,
        f"crates/{crate_name} [package].license-file is not set",
    )
    test_case.assertEqual(
        entry,
        EXPECTED_PACKAGE_LICENSE_FILE[crate_name],
        f"crates/{crate_name} [package].license-file {entry!r} != expected "
        f"{EXPECTED_PACKAGE_LICENSE_FILE[crate_name]!r}",
    )
    crate_dir = crate_cargo_toml(crate_name).parent
    resolved = (crate_dir / entry).resolve()
    test_case.assertTrue(
        resolved.exists(),
        f"crates/{crate_name} [package].license-file {entry!r} does not resolve to an existing file",
    )
    return resolved


def assert_repository_matches_workspace(test_case: unittest.TestCase, crate_name: str) -> None:
    """Reusable helper: a deb-packaged crate's explicit `repository` field
    (if set -- crates that inherit `.workspace = true` are trivially in
    sync) must equal the canonical workspace repository. Scoped to
    deb-packaged crates per the design doc's Scope / Non-goals -- intended
    for reuse by O3's release-workflow contract tests."""
    repo = crate_package_table(crate_name).get("repository")
    if repo is None:
        return
    test_case.assertEqual(
        repo,
        workspace_repository(),
        f"crates/{crate_name} repository {repo!r} != workspace repository "
        f"{workspace_repository()!r}",
    )


EXPECTED_LICENSE_FOR_CRATE = {
    "terraphim_agent": "Apache-2.0",
    "terraphim_grep": "MIT",
}

# Cheap, distinctive marker phrases -- not full-text diffing -- used to
# sanity-check that a license file's content actually matches its SPDX
# identifier (e.g. LICENSE-Apache-2.0 contains Apache-2.0 grant text, not
# MIT text).
LICENSE_MARKER_FOR_SPDX = {
    "Apache-2.0": "Apache License",
    "MIT": 'Permission is hereby granted, free of charge, to any person obtaining a copy',
}

# Authoritative upstream bytes, per the implementation brief: exact SHA-256
# of the two license files as published at
# https://raw.githubusercontent.com/terraphim/terraphim-ai/main/LICENSE-Apache-2.0
# and .../LICENSE-MIT. This is a stronger, exact-byte-provenance invariant
# than the marker-phrase heuristic above -- it fails if anyone edits the
# root license files (e.g. changes the MIT copyright line) even if the
# marker phrase still happens to match.
EXPECTED_LICENSE_SHA256 = {
    "LICENSE-Apache-2.0": "47528e762efc05e17ae569ffeacf044b65cbe2c94bc9c58c576f267a5cd7d039",
    "LICENSE-MIT": "3ec3e4145b74567ba29578785140bc15af1309576cceb9c921c1a23d43060eea",
}


def assert_license_file_resolves(test_case: unittest.TestCase, crate_name: str) -> Path:
    """Reusable helper: the crate's deb license-file path must resolve to an
    existing file on disk. Returns the resolved path. Intended for reuse by
    O3's release-workflow contract tests (see design doc Handoff to O3)."""
    deb = crate_deb_metadata(crate_name)
    entry = deb["license-file"]
    rel_path = entry[0] if isinstance(entry, list) else entry
    crate_dir = crate_cargo_toml(crate_name).parent
    resolved = (crate_dir / rel_path).resolve()
    test_case.assertTrue(
        resolved.exists(),
        f"crates/{crate_name} license-file {rel_path!r} does not resolve to an existing file",
    )
    return resolved


class PackageMetadataContract(unittest.TestCase):
    def test_discovered_deb_packaged_crates_matches_intended_set(self) -> None:
        self.assertEqual(
            DEB_PACKAGED_CRATES,
            INTENDED_DEB_PACKAGED_CRATES,
            f"crates/*/Cargo.toml scan found deb-packaged crates "
            f"{DEB_PACKAGED_CRATES} but this contract intends "
            f"{INTENDED_DEB_PACKAGED_CRATES} -- a crate gained or lost "
            f"[package.metadata.deb]; update INTENDED_DEB_PACKAGED_CRATES "
            f"(and the design doc's scope) deliberately",
        )

    def test_license_files_exist_at_root(self) -> None:
        for filename in ("LICENSE-Apache-2.0", "LICENSE-MIT"):
            with self.subTest(filename=filename):
                path = ROOT / filename
                self.assertTrue(path.exists(), f"{filename} not found at repo root")
                self.assertGreater(path.stat().st_size, 0, f"{filename} is empty")

    def test_license_file_paths_resolve(self) -> None:
        for crate in DEB_PACKAGED_CRATES:
            with self.subTest(crate=crate):
                assert_license_file_resolves(self, crate)

    def test_package_level_license_file_resolves(self) -> None:
        # Distinct from the deb-metadata `license-file` check above: this
        # is the manifest field `cargo package`/`cargo publish` actually
        # reads to copy a license file into the package output (Issue
        # #246's explicit "cargo package --list must show the license"
        # acceptance criterion -- the deb-metadata field alone never shows
        # up there).
        for crate in DEB_PACKAGED_CRATES:
            with self.subTest(crate=crate):
                assert_package_license_file_resolves(self, crate)

    def test_crate_repository_matches_workspace(self) -> None:
        for crate in DEB_PACKAGED_CRATES:
            with self.subTest(crate=crate):
                assert_repository_matches_workspace(self, crate)

    def test_license_file_content_matches_identifier(self) -> None:
        for crate in DEB_PACKAGED_CRATES:
            with self.subTest(crate=crate):
                resolved = assert_license_file_resolves(self, crate)
                spdx = EXPECTED_LICENSE_FOR_CRATE[crate]
                marker = LICENSE_MARKER_FOR_SPDX[spdx]
                text = resolved.read_text()
                self.assertIn(
                    marker,
                    text,
                    f"{resolved.name} content does not contain expected {spdx} marker text",
                )

    def test_license_file_bytes_match_authoritative_sha256(self) -> None:
        # Exact-byte-provenance regression invariant (implementation
        # brief): the root license files must match the verified upstream
        # hashes exactly, not just contain a marker phrase.
        for filename, expected_hash in EXPECTED_LICENSE_SHA256.items():
            with self.subTest(filename=filename):
                path = ROOT / filename
                actual_hash = hashlib.sha256(path.read_bytes()).hexdigest()
                self.assertEqual(
                    actual_hash,
                    expected_hash,
                    f"{filename} sha256 {actual_hash} != expected authoritative hash {expected_hash}",
                )

    def test_license_identifiers_unchanged(self) -> None:
        # Regression guard: this issue fixes metadata, not licensing policy
        # -- terraphim_agent stays Apache-2.0, terraphim_grep stays MIT.
        for crate, expected in EXPECTED_LICENSE_FOR_CRATE.items():
            with self.subTest(crate=crate):
                self.assertEqual(crate_package_table(crate).get("license"), expected)

    def test_crate_version_inherits_workspace(self) -> None:
        # Both packaged crates must inherit (not pin) the workspace
        # version, so a version bump can't silently desync from a stale
        # per-crate pin.
        for crate in DEB_PACKAGED_CRATES:
            with self.subTest(crate=crate):
                self.assertEqual(
                    crate_package_table(crate).get("version"),
                    {"workspace": True},
                )

    def test_tag_workspace_mismatch_detected(self) -> None:
        # Reusable assertion (no release-YAML edits needed) that a release
        # tag must match the checked-in workspace version -- must fail
        # before build for e.g. v9.9.9 vs the actual checked-in version.
        with self.assertRaises(AssertionError):
            assert_tag_matches_workspace_version(self, "v9.9.9")
        # Sanity: a tag that does match must not raise.
        assert_tag_matches_workspace_version(self, f"v{workspace_version()}")


if __name__ == "__main__":
    unittest.main()
