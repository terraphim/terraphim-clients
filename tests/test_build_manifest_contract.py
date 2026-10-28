import hashlib
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "build-manifest.sh"
LEGACY_SCRIPT = ROOT / "scripts" / "build-legacy-manifest.py"

COMMON_TARGETS = (
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
)


def extension(target: str) -> str:
    return ".zip" if target == "x86_64-pc-windows-msvc" else ".tar.gz"


class BuildManifestContract(unittest.TestCase):
    def run_builder(
        self, binary: str, artifacts: Path, output: Path
    ) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["SOURCE_DATE_EPOCH"] = "1789689600"
        return subprocess.run(
            [str(SCRIPT), "1.21.15", binary, str(artifacts), str(output)],
            env=env,
            text=True,
            capture_output=True,
        )

    def populate(self, artifacts: Path, binary: str, universal: bool) -> list[Path]:
        targets = list(COMMON_TARGETS)
        if universal:
            targets.append("universal-apple-darwin")
        paths = []
        for index, target in enumerate(targets, start=1):
            path = artifacts / f"{binary}-1.21.15-{target}{extension(target)}"
            path.write_bytes(f"sealed-{index}".encode())
            paths.append(path)
        return paths

    def test_builds_deterministic_strict_manifest_with_integrity_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            artifacts = Path(directory) / "artifacts"
            artifacts.mkdir()
            targets = (*COMMON_TARGETS, "universal-apple-darwin")
            expected_assets = {}
            for index, target in enumerate(targets, start=1):
                filename = f"terraphim-agent-1.21.15-{target}{extension(target)}"
                payload = f"sealed-{index}".encode()
                (artifacts / filename).write_bytes(payload)
                expected_assets[target] = {
                    "path": f"terraphim-agent/{filename}",
                    "sha256": hashlib.sha256(payload).hexdigest(),
                    "size": len(payload),
                }

            output = Path(directory) / "terraphim-agent.candidate.json"
            first = self.run_builder("terraphim-agent", artifacts, output)
            self.assertEqual(first.returncode, 0, first.stderr)
            first_bytes = output.read_bytes()
            second = self.run_builder("terraphim-agent", artifacts, output)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual(output.read_bytes(), first_bytes)

            manifest = json.loads(first_bytes)
            self.assertEqual(
                tuple(manifest), ("assets", "notes_url", "released_at", "version")
            )
            self.assertEqual(manifest["version"], "1.21.15")
            self.assertEqual(manifest["released_at"], "2026-09-18T00:00:00Z")
            self.assertEqual(manifest["assets"], expected_assets)

    def test_cli_exact_set_excludes_universal_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            artifacts = Path(directory) / "artifacts"
            artifacts.mkdir()
            self.populate(artifacts, "terraphim-cli", universal=False)
            output = Path(directory) / "terraphim-cli.candidate.json"
            result = self.run_builder("terraphim-cli", artifacts, output)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(set(json.loads(output.read_text())["assets"]), set(COMMON_TARGETS))

    def test_rejects_missing_empty_extra_wrong_version_and_stable_output(self) -> None:
        cases = ("missing", "empty", "extra", "wrong-version", "stable-output", "stable-v2-output")
        for case in cases:
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                artifacts = Path(directory) / "artifacts"
                artifacts.mkdir()
                paths = self.populate(artifacts, "terraphim-agent", universal=True)
                output = Path(directory) / "terraphim-agent.candidate.json"
                if case == "missing":
                    paths[0].unlink()
                elif case == "empty":
                    paths[0].write_bytes(b"")
                elif case == "extra":
                    (artifacts / "terraphim-agent-1.21.15-riscv64gc-unknown-linux-gnu.tar.gz").write_bytes(b"extra")
                elif case == "wrong-version":
                    (artifacts / "terraphim-agent-1.21.14-x86_64-unknown-linux-gnu.tar.gz").write_bytes(b"old")
                elif case == "stable-output":
                    output = Path(directory) / "stable.json"
                elif case == "stable-v2-output":
                    output = Path(directory) / "stable-v2.json"
                result = self.run_builder("terraphim-agent", artifacts, output)
                self.assertNotEqual(result.returncode, 0, result.stdout)

    def test_failure_preserves_previous_candidate_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            artifacts = Path(directory) / "artifacts"
            artifacts.mkdir()
            paths = self.populate(artifacts, "terraphim-grep", universal=True)
            output = Path(directory) / "terraphim-grep.candidate.json"
            output.write_bytes(b"previous-candidate\n")
            paths[0].unlink()
            result = self.run_builder("terraphim-grep", artifacts, output)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(output.read_bytes(), b"previous-candidate\n")

    def test_legacy_candidate_derives_string_asset_map_from_strict_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            self.populate(artifacts, "terraphim-agent", universal=True)
            strict = root / "terraphim-agent.v2.candidate.json"
            legacy = root / "terraphim-agent.v1.candidate.json"
            built = self.run_builder("terraphim-agent", artifacts, strict)
            self.assertEqual(built.returncode, 0, built.stderr)

            derived = subprocess.run(
                ["python3", str(LEGACY_SCRIPT), str(strict), str(legacy)],
                text=True,
                capture_output=True,
            )
            self.assertEqual(derived.returncode, 0, derived.stderr)
            data = json.loads(legacy.read_text())
            self.assertEqual(data["version"], "1.21.15")
            self.assertTrue(data["assets"])
            self.assertTrue(all(isinstance(path, str) for path in data["assets"].values()))

            target = "x86_64-unknown-linux-gnu"
            advertised = data["assets"][target]
            self.assertEqual(
                advertised,
                "terraphim-agent/terraphim-agent-1.21.15-"
                "x86_64-unknown-linux-gnu.tar.gz",
            )


if __name__ == "__main__":
    unittest.main()
