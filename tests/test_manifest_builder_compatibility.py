"""Compatibility bridge between the two manifest-builder contracts.

The release producer workflow (`.github/workflows/release-binaries.yml`,
`seal-release-stage`) calls `scripts/build-manifest.sh` with four arguments
to emit a strict v2 candidate manifest (object-valued assets carrying
sha256 + size, Windows .zip mandatory for every shipped binary, with
deterministic `released_at` from SOURCE_DATE_EPOCH).

The release finalizer (`.github/workflows/finalize-prebuilt-release.yml`)
calls the same script with three arguments and writes the legacy v1 stdout
manifest (path-only assets, seven unix targets, no Windows, dated at the
finalizer's wall-clock invocation).

The two contracts do **not** consume the same artifact directory and do
**not** emit the same JSON shape. What they share is the per-target
advertised R2 object key for the targets both contracts agree on, and the
strict candidate builder's refusal to overwrite a stable pointer (which
would let a build race a promotion). This module asserts both invariants
explicitly, using a separate inventory per contract so the strict builder's
"unexpected or duplicate artifacts" rejection is never weakened.
"""

import hashlib
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "build-manifest.sh"
VERSION = "1.21.16"

# Every target the producer must build for the agent and grep packages
# (universal Apple included). cli does not ship a universal Apple archive
# in the v1.21.16 producer; see #246 / Gitea producer.
UNIX_TARGETS = (
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "universal-apple-darwin",
)

# Every target the v1.21.14 release finalizer advertises in its v1 stdout
# manifest, including universal-apple-darwin for every binary. Windows is
# intentionally absent because the tagged v1.21.14 updater cannot verify
# ZIP signatures. The list is identical to UNIX_TARGETS in this release
# family; aliased here so the legacy fixtures read semantically (the
# legacy builder is universal-cli inclusive, even though the strict
# producer is not) rather than appearing to duplicate the same tuple.
LEGACY_UNIX_TARGETS = UNIX_TARGETS

# Targets the strict v2 candidate builder requires. cli gets the common set
# (no universal); agent and grep add universal-apple-darwin. Every binary
# (including cli) carries the Windows zip per the producer's seal step.
def strict_targets_for(binary: str) -> tuple:
    base = {
        "aarch64-apple-darwin": ".tar.gz",
        "aarch64-unknown-linux-musl": ".tar.gz",
        "x86_64-apple-darwin": ".tar.gz",
        "x86_64-pc-windows-msvc": ".zip",
        "x86_64-unknown-linux-gnu": ".tar.gz",
        "x86_64-unknown-linux-musl": ".tar.gz",
    }
    if binary in ("terraphim-agent", "terraphim-grep"):
        base["universal-apple-darwin"] = ".tar.gz"
    return tuple(sorted(base))


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _write_payload(directory: Path, filename: str) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / filename).write_bytes(
        f"sealed-payload-for-{filename}".encode()
    )


class LegacyInventory:
    """Inventory the v1.21.14 finalizer creates in `release-assets/`.

    Seven unix .tar.gz archives for every binary (universal included) plus
    the Windows .exe that ships alongside but is not advertised in the v1
    manifest. Matches what `.github/workflows/finalize-prebuilt-release.yml`
    builds before calling the legacy 3-arg form of `build-manifest.sh`.
    """

    def __init__(self, binary: str, version: str) -> None:
        self.binary = binary
        self.version = version
        self.directory: Path | None = None
        self.expected_files: list[Path] = []

    def __enter__(self) -> "LegacyInventory":
        self.directory = Path(tempfile.mkdtemp())
        for target in LEGACY_UNIX_TARGETS:
            filename = f"{self.binary}-{self.version}-{target}.tar.gz"
            _write_payload(self.directory, filename)
            self.expected_files.append(self.directory / filename)
        return self

    def __exit__(self, *_exc) -> None:
        return None


class StrictInventory:
    """Inventory the v1.21.16 producer creates before the seal step.

    Strictly the union the strict candidate builder will accept: exactly
    one archive per `target_sets` entry, with the matching filename and
    extension, and nothing else bearing the binary's prefix. Adding an
    extra archive (intentional or accidental) makes the strict builder
    fail closed, which is the point of this test module: that behaviour
    must not be weakened by the compat bridge.
    """

    def __init__(self, binary: str, version: str) -> None:
        self.binary = binary
        self.version = version
        self.directory: Path | None = None
        self.expected_files: list[Path] = []

    def __enter__(self) -> "StrictInventory":
        self.directory = Path(tempfile.mkdtemp())
        for target in strict_targets_for(self.binary):
            extension = ".zip" if target == "x86_64-pc-windows-msvc" else ".tar.gz"
            filename = f"{self.binary}-{self.version}-{target}{extension}"
            _write_payload(self.directory, filename)
            self.expected_files.append(self.directory / filename)
        return self

    def __exit__(self, *_exc) -> None:
        return None


def _tarname(binary: str, version: str, target: str, extension: str) -> str:
    return f"{binary}-{version}-{target}{extension}"


def _run_legacy(binary: str, artifacts: Path) -> dict:
    result = subprocess.run(
        [str(SCRIPT), VERSION, binary, str(artifacts)],
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(result.stdout)


def _run_v2(binary: str, artifacts: Path, output: Path) -> dict:
    env = os.environ.copy()
    env["SOURCE_DATE_EPOCH"] = "1789689600"
    result = subprocess.run(
        [str(SCRIPT), VERSION, binary, str(artifacts), str(output)],
        env=env,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, (
        f"strict builder failed: rc={result.returncode} stderr={result.stderr}"
    )
    return json.loads(output.read_text())


class ManifestBuilderCompatibilityTests(unittest.TestCase):
    """Cross-contract reconciliation between the legacy and strict builders.

    Each test sets up an inventory that exactly matches the contract it
    exercises, never one shared inventory, so the strict builder's
    "unexpected or duplicate artifacts" rejection cannot be silently
    neutralised by an over-broad test fixture.
    """

    def test_legacy_emits_seven_unix_targets_with_path_only_assets(self) -> None:
        with LegacyInventory("terraphim-agent", VERSION) as inventory, \
                tempfile.NamedTemporaryFile("w+") as sink:
            manifest = _run_legacy("terraphim-agent", inventory.directory)
            sink.write(json.dumps(manifest))
            sink.flush()
            self.assertEqual(manifest["version"], VERSION)
            self.assertEqual(set(manifest["assets"]), set(LEGACY_UNIX_TARGETS))
            for target in LEGACY_UNIX_TARGETS:
                self.assertIsInstance(manifest["assets"][target], str)
                self.assertEqual(
                    manifest["assets"][target],
                    f"terraphim-agent/{_tarname('terraphim-agent', VERSION, target, '.tar.gz')}",
                )

    def test_strict_v2_emits_full_target_set_with_integrity_metadata(self) -> None:
        with StrictInventory("terraphim-agent", VERSION) as inventory:
            output = inventory.directory / "terraphim-agent.v2.candidate.json"
            manifest = _run_v2("terraphim-agent", inventory.directory, output)
            self.assertEqual(manifest["version"], VERSION)
            # The strict agent target set is the producer's UNIX_TARGETS
            # (universal-apple-darwin included) plus the Windows zip. The
            # cross-contract reconciliation pins that composition: if the
            # producer drops a target from UNIX_TARGETS for agent/grep, or
            # universal Apple is removed entirely, the strict and legacy
            # builders' path keys will drift apart and the contract test
            # below catches the regression.
            self.assertEqual(
                set(manifest["assets"]),
                set(UNIX_TARGETS) | {"x86_64-pc-windows-msvc"},
            )
            self.assertEqual(
                set(strict_targets_for("terraphim-agent")),
                set(UNIX_TARGETS) | {"x86_64-pc-windows-msvc"},
            )
            for target, asset in manifest["assets"].items():
                self.assertIsInstance(asset, dict)
                expected_extension = (
                    ".zip" if target == "x86_64-pc-windows-msvc" else ".tar.gz"
                )
                expected_filename = _tarname(
                    "terraphim-agent", VERSION, target, expected_extension
                )
                self.assertEqual(
                    asset["path"],
                    f"terraphim-agent/{expected_filename}",
                )
                on_disk = inventory.directory / expected_filename
                self.assertEqual(asset["size"], on_disk.stat().st_size)
                self.assertEqual(asset["sha256"], _sha256(on_disk))
                self.assertEqual(len(asset["sha256"]), 64)
                int(asset["sha256"], 16)

    def test_legacy_does_not_advertise_windows_zip(self) -> None:
        """The v1.21.14 finalizer omits Windows by design (the tagged
        updater cannot verify ZIP signatures). The legacy builder must
        keep that property even when Windows zips happen to be present
        in the artifacts directory.
        """
        with LegacyInventory("terraphim-agent", VERSION) as inventory:
            # Add the Windows zip the producer would have shipped, even
            # though the finalizer must not advertise it.
            _write_payload(
                inventory.directory,
                f"terraphim-agent-{VERSION}-x86_64-pc-windows-msvc.zip",
            )
            manifest = _run_legacy("terraphim-agent", inventory.directory)
            self.assertNotIn("x86_64-pc-windows-msvc", manifest["assets"])

    def test_strict_v2_keeps_legacy_per_target_path_semantics(self) -> None:
        """For every target both contracts advertise, the public R2 object
        key MUST agree. The strict candidate builder is the canonical
        source of truth for v1.21.16; the legacy builder is canonical for
        the v1.21.14 finalizer. If they diverge on any common target the
        published manifest cannot be consumed by every terraphim_update
        reader, breaking the producer <-> updater contract.
        """
        for binary in ("terraphim-agent", "terraphim-cli", "terraphim-grep"):
            for target in LEGACY_UNIX_TARGETS:
                # cli does not ship a universal Apple archive in the v2
                # producer (target_sets excludes universal for cli), so
                # only the legacy builder advertises that path for cli.
                if binary == "terraphim-cli" and target == "universal-apple-darwin":
                    continue
                with self.subTest(binary=binary, target=target):
                    with LegacyInventory(binary, VERSION) as legacy_inv:
                        legacy = _run_legacy(binary, legacy_inv.directory)
                    with StrictInventory(binary, VERSION) as strict_inv:
                        output = (
                            strict_inv.directory
                            / f"{binary}.v2.candidate.json"
                        )
                        strict = _run_v2(binary, strict_inv.directory, output)
                    expected_filename = _tarname(
                        binary, VERSION, target, ".tar.gz"
                    )
                    self.assertEqual(
                        legacy["assets"][target],
                        f"{binary}/{expected_filename}",
                    )
                    self.assertEqual(
                        strict["assets"][target]["path"],
                        f"{binary}/{expected_filename}",
                    )
                    self.assertEqual(
                        legacy["assets"][target],
                        strict["assets"][target]["path"],
                    )

    def test_strict_v2_distinguishes_cli_universal_from_agent_grep(self) -> None:
        """The strict candidate builder's `target_sets` omits universal for
        cli but keeps it for agent and grep. This is the producer-side
        equivalent of `if [ "$binary" != "terraphim-cli" ]; then targets+=(universal)`.
        The compat bridge must not silently widen cli to advertise a path
        the producer never sealed.
        """
        with StrictInventory("terraphim-cli", VERSION) as inventory:
            output = inventory.directory / "terraphim-cli.v2.candidate.json"
            manifest = _run_v2("terraphim-cli", inventory.directory, output)
            self.assertNotIn("universal-apple-darwin", manifest["assets"])
        with StrictInventory("terraphim-agent", VERSION) as inventory:
            output = inventory.directory / "terraphim-agent.v2.candidate.json"
            manifest = _run_v2("terraphim-agent", inventory.directory, output)
            self.assertIn("universal-apple-darwin", manifest["assets"])

    def test_strict_v2_rejects_stable_pointer_output_name(self) -> None:
        """The candidate builder must never overwrite a stable pointer:
        stable promotion is a separately authorized promote-release.sh
        operation. This guard binds the same invariant the Gitea workflow
        contract asserts in `test_build_manifest_contract`.
        """
        with StrictInventory("terraphim-agent", VERSION) as inventory:
            for forbidden in ("stable.json", "stable-v2.json"):
                with self.subTest(output=forbidden):
                    output = inventory.directory / forbidden
                    env = os.environ.copy()
                    env["SOURCE_DATE_EPOCH"] = "1789689600"
                    result = subprocess.run(
                        [
                            str(SCRIPT),
                            VERSION,
                            "terraphim-agent",
                            str(inventory.directory),
                            str(output),
                        ],
                        env=env,
                        capture_output=True,
                        text=True,
                    )
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(
                        "candidate builder refuses to replace a stable pointer",
                        result.stderr,
                    )
                    self.assertFalse(output.exists())

    def test_strict_v2_rejects_extra_archive_in_inventory(self) -> None:
        """The strict candidate builder's "unexpected or duplicate
        artifacts" rejection is a key invariant the compat bridge must
        not silently weaken: a build that drops an extra archive into
        the same directory (intentional or accidental) cannot silently
        widen the published target set.
        """
        with StrictInventory("terraphim-agent", VERSION) as inventory:
            # Drop an extra archive that bears the binary's prefix but is
            # not in target_sets for agent (riscv64gc-linux-gnu is not a
            # shipped client target).
            _write_payload(
                inventory.directory,
                f"terraphim-agent-{VERSION}-riscv64gc-unknown-linux-gnu.tar.gz",
            )
            output = inventory.directory / "terraphim-agent.v2.candidate.json"
            env = os.environ.copy()
            env["SOURCE_DATE_EPOCH"] = "1789689600"
            result = subprocess.run(
                [
                    str(SCRIPT),
                    VERSION,
                    "terraphim-agent",
                    str(inventory.directory),
                    str(output),
                ],
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "unexpected or duplicate artifacts",
                result.stderr,
            )
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
