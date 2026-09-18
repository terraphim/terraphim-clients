import hashlib
import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HEALTH = ROOT / "scripts" / "validate-r2-manifests.py"
COMMON = {
    "aarch64-apple-darwin", "aarch64-unknown-linux-musl", "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl",
}


def prepare_tree(
    root: Path, version: str, strict: bool = True, legacy_live_shape: bool = False
) -> None:
    targets_by_binary = {
        "terraphim-agent": COMMON | {"universal-apple-darwin"},
        "terraphim-grep": COMMON | {"universal-apple-darwin"},
        "terraphim-cli": COMMON,
    }
    for binary, targets in targets_by_binary.items():
        directory = root / binary
        directory.mkdir()
        legacy_assets = {}
        strict_assets = {}
        for target in targets:
            extension = ".zip" if target == "x86_64-pc-windows-msvc" else ".tar.gz"
            path = f"{binary}/{binary}-{version}-{target}{extension}"
            payload = f"{binary}-{target}".encode()
            (root / path).write_bytes(payload)
            legacy_assets[target] = path
            strict_assets[target] = {
                "path": path, "sha256": hashlib.sha256(payload).hexdigest(), "size": len(payload)
            }
        if legacy_live_shape:
            legacy_assets.pop("x86_64-pc-windows-msvc")
            if binary == "terraphim-cli":
                target = "universal-apple-darwin"
                path = f"{binary}/{binary}-{version}-{target}.tar.gz"
                (root / path).write_bytes(b"legacy-cli-universal")
                legacy_assets[target] = path
        metadata = {
            "version": version, "released_at": "2026-09-18T00:00:00Z",
            "notes_url": "https://example.invalid/release",
        }
        (directory / "stable.json").write_text(json.dumps(metadata | {"assets": legacy_assets}))
        if strict:
            (directory / "stable-v2.json").write_text(json.dumps(metadata | {"assets": strict_assets}))


class ReleaseCiContract(unittest.TestCase):
    def test_github_ci_executes_release_contract_suite(self) -> None:
        text = (ROOT / ".github" / "workflows" / "ci.yml").read_text()
        self.assertIn("python3 -m unittest discover -s tests -p 'test_*release*contract.py' -v", text)
        self.assertIn("python3 -m unittest tests.test_build_manifest_contract", text)
        self.assertIn("python3 -m unittest tests.test_promotion_contract", text)
        native = (ROOT / ".gitea" / "workflows" / "native-ci.yml").read_text()
        self.assertIn(
            "cargo test --locked -p terraphim_update --test manifest --test r2_update",
            native,
        )

    def test_health_validator_is_migration_aware_and_adversarial(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepare_tree(root, "1.21.14", strict=False, legacy_live_shape=True)
            legacy = subprocess.run(
                [str(HEALTH), "--base-url", root.as_uri()], text=True, capture_output=True
            )
            self.assertEqual(legacy.returncode, 0, legacy.stderr)

        for strict in (False, True):
            with self.subTest(activation_legacy_skew=strict), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                prepare_tree(root, "1.21.15", strict=strict, legacy_live_shape=True)
                result = subprocess.run(
                    [str(HEALTH), "--base-url", root.as_uri()], text=True, capture_output=True
                )
                self.assertNotEqual(result.returncode, 0)

        mutations = (
            "missing-v2",
            "duplicate",
            "wrong-size",
            "wrong-target",
            "tampered",
            "legacy-skew",
        )
        for mutation in mutations:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                prepare_tree(root, "1.21.15", strict=mutation != "missing-v2")
                pointer = root / "terraphim-agent" / "stable-v2.json"
                if mutation == "duplicate":
                    text = pointer.read_text()
                    pointer.write_text(text[:-1] + ',"version":"1.21.15"}')
                elif mutation in {"wrong-size", "wrong-target", "tampered"}:
                    manifest = json.loads(pointer.read_text())
                    target = sorted(manifest["assets"])[0]
                    if mutation == "wrong-size":
                        manifest["assets"][target]["size"] += 1
                    elif mutation == "wrong-target":
                        manifest["assets"]["not-a-target"] = manifest["assets"].pop(target)
                    else:
                        (root / manifest["assets"][target]["path"]).write_bytes(b"tampered")
                    pointer.write_text(json.dumps(manifest))
                elif mutation == "legacy-skew":
                    legacy_pointer = root / "terraphim-agent" / "stable.json"
                    manifest = json.loads(legacy_pointer.read_text())
                    manifest["released_at"] = "2026-09-17T00:00:00Z"
                    legacy_pointer.write_text(json.dumps(manifest))
                result = subprocess.run(
                    [str(HEALTH), "--base-url", root.as_uri()], text=True, capture_output=True
                )
                self.assertNotEqual(result.returncode, 0)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepare_tree(root, "1.21.15", strict=True)
            valid = subprocess.run(
                [str(HEALTH), "--base-url", root.as_uri()], text=True, capture_output=True
            )
            self.assertEqual(valid.returncode, 0, valid.stderr)

    def test_health_asset_verification_streams_and_enforces_declared_size(self) -> None:
        spec = importlib.util.spec_from_file_location("validate_r2_manifests", HEALTH)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

        class Response:
            def __init__(self, payload: bytes) -> None:
                self.payload = payload
                self.offset = 0
                self.calls: list[int | None] = []

            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return False

            def read(self, amount=None):
                self.calls.append(amount)
                if amount is None:
                    raise AssertionError("unbounded read is forbidden")
                chunk = self.payload[self.offset : self.offset + amount]
                self.offset += len(chunk)
                return chunk

        payload = b"streamed-payload"
        response = Response(payload)
        module.verify_asset(
            "https://downloads.invalid",
            "terraphim-agent/asset.tar.gz",
            len(payload),
            hashlib.sha256(payload).hexdigest(),
            chunk_size=4,
            opener=lambda *_args, **_kwargs: response,
        )
        self.assertGreater(len(response.calls), 2)
        self.assertNotIn(None, response.calls)

        for name, body, declared in (
            ("oversized", b"123456", 5),
            ("short", b"1234", 5),
        ):
            with self.subTest(name=name):
                response = Response(body)
                with self.assertRaises(ValueError):
                    module.verify_asset(
                        "https://downloads.invalid",
                        "asset",
                        declared,
                        hashlib.sha256(body).hexdigest(),
                        chunk_size=2,
                        opener=lambda *_args, **_kwargs: response,
                    )
                self.assertNotIn(None, response.calls)
                if name == "oversized":
                    self.assertLessEqual(response.offset, declared + 1)

    def test_operator_docs_match_separate_authorized_promotion(self) -> None:
        text = (ROOT / "docs" / "release-operator-checklist.md").read_text()
        for token in (
            "client-release-stage-<version>-<source-sha>",
            "SHA256SUMS",
            "candidate.json",
            "draft",
            "prerelease",
            "scripts/promote-release.sh",
            "Do not use `--clobber`",
            "DEB/RPM",
        ):
            self.assertIn(token, text)


if __name__ == "__main__":
    unittest.main()
