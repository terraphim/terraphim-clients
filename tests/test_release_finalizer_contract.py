import base64
import hashlib
import importlib.util
import json
import pathlib
import re
import subprocess
import tarfile
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/finalize-prebuilt-release.yml"
CONTRACT = ROOT / ".github/release-inputs/v1.21.14.json"
PINNED_KEY = ROOT / ".github/release-signing/zipsign-primary-public-key.base64"
UPDATER_SIGNATURES = ROOT / "crates/terraphim_update/src/signature.rs"
VALIDATOR_PATH = ROOT / "scripts/validate-release-inputs.py"
VALIDATOR_SOURCE = VALIDATOR_PATH.read_text()

SPEC = importlib.util.spec_from_file_location("release_input_validator", VALIDATOR_PATH)
assert SPEC is not None and SPEC.loader is not None
VALIDATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VALIDATOR)


class ReleaseFinalizerContractTests(unittest.TestCase):
    def test_contract_has_exact_binary_matrix_and_unique_hashes(self):
        contract = json.loads(CONTRACT.read_text())
        binaries = contract["binaries"]
        names = {item["name"] for item in binaries}
        expected = set()
        for binary in ("terraphim-agent", "terraphim-cli", "terraphim-grep"):
            for target in (
                "aarch64-apple-darwin",
                "x86_64-apple-darwin",
                "x86_64-unknown-linux-gnu",
                "x86_64-unknown-linux-musl",
                "aarch64-unknown-linux-musl",
            ):
                expected.add(f"{binary}-{target}")
            expected.add(f"{binary}-x86_64-pc-windows-msvc.exe")

        self.assertEqual(names, expected)
        self.assertEqual(len(binaries), 18)
        self.assertTrue(all(re.fullmatch(r"[0-9a-f]{64}", item["sha256"]) for item in binaries))
        self.assertTrue(re.fullmatch(r"[0-9a-f]{40}", contract["source_sha"]))
        self.assertTrue(re.fullmatch(r"[0-9a-f]{64}", contract["staging_sha256"]))

    def test_workflow_is_main_only_review_bound_and_fail_closed(self):
        workflow = WORKFLOW.read_text()
        required_fragments = (
            "github.ref == 'refs/heads/main'",
            "environment: tsm-production-release",
            "Load review-bound release contract",
            "Validate immutable source and draft release",
            "scripts/validate-release-inputs.py",
            "Apple-sign and notarize every shipped macOS binary",
            "Upload and byte-verify draft assets",
            "unexpected draft release inventory before publication",
            "Publish atomically and verify final inventory",
        )
        for fragment in required_fragments:
            self.assertIn(fragment, workflow)
        self.assertIn("non-regular staging member rejected", VALIDATOR_SOURCE)
        self.assertIn("digest mismatch", VALIDATOR_SOURCE)

        action_refs = re.findall(r"^\s*- uses: [^@\s]+@([^\s]+)", workflow, re.MULTILINE)
        self.assertGreaterEqual(len(action_refs), 2)
        self.assertTrue(all(re.fullmatch(r"[0-9a-f]{40}", ref) for ref in action_refs))
        self.assertNotIn("OP_SERVICE_ACCOUNT_TOKEN", workflow)
        for secret in (
            "APPLE_ID",
            "APPLE_TEAM_ID",
            "APPLE_APP_PASSWORD",
            "CERT_BASE64",
            "CERT_PASSWORD",
            "ZIPSIGN_PRIVATE_KEY",
        ):
            self.assertIn(f"secrets.{secret}", workflow)
        self.assertLess(
            workflow.index("unexpected draft release inventory before publication"),
            workflow.index('gh release edit "$RELEASE_TAG" --draft=false'),
        )
        failed_publish = workflow.index('if ! gh release edit "$RELEASE_TAG" --draft=false')
        state_query = workflow.index('if ! publication_state="$(gh api', failed_publish)
        restore_staging = workflow.index(
            'gh release upload "$RELEASE_TAG" "staging/$STAGING_ASSET" --clobber',
            failed_publish,
        )
        self.assertLess(state_query, restore_staging)
        self.assertIn("publication result is ambiguous; no recovery mutation attempted", workflow)
        self.assertIn("publication state is unknown; no recovery mutation attempted", workflow)
        self.assertIn("publish command failed after GitHub committed publication", workflow)

    def test_archive_signer_uses_the_client_trusted_primary_key(self):
        pinned = PINNED_KEY.read_text().strip()
        self.assertEqual(len(base64.b64decode(pinned, validate=True)), 32)
        updater = UPDATER_SIGNATURES.read_text()
        embedded = re.search(
            r"EMBEDDED_PUBLIC_KEYS.*?=\s*&\[(.*?)\];", updater, re.DOTALL
        )
        self.assertIsNotNone(embedded)
        keys = re.findall(r'"([A-Za-z0-9+/]{43}=)"', embedded.group(1))
        self.assertGreaterEqual(len(keys), 1)
        self.assertEqual(pinned, keys[0])

    def test_real_staging_bundle_matches_reviewed_contract(self):
        archive = pathlib.Path(
            "/private/tmp/terraphim-clients-1.21.14-release-inputs.tar.gz"
        )
        if not archive.exists():
            self.skipTest("local release staging bundle is unavailable")
        with tempfile.TemporaryDirectory() as temporary:
            destination = pathlib.Path(temporary) / "inputs"
            VALIDATOR.validate_and_extract(CONTRACT, archive, destination)
            self.assertEqual(
                {path.name for path in destination.iterdir()},
                VALIDATOR.expected_binary_names(),
            )

    def test_validator_rejects_a_symlink_even_with_an_expected_name(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            source = root / "source"
            source.mkdir()
            payload = b"binary"
            digest = hashlib.sha256(payload).hexdigest()
            names = sorted(VALIDATOR.expected_binary_names())
            for name in names:
                (source / name).write_bytes(payload)
            symlink_name = names[0]
            (source / symlink_name).unlink()
            (source / symlink_name).symlink_to(source / names[1])
            contract = root / "contract.json"
            contract.write_text(json.dumps({
                "binaries": [{"name": name, "sha256": digest} for name in names]
            }))
            archive = root / "malicious.tar.gz"
            with tarfile.open(archive, "w:gz") as bundle:
                for name in names:
                    bundle.add(source / name, arcname=name, recursive=False)
            destination = root / "output"
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "non-regular"):
                VALIDATOR.validate_and_extract(contract, archive, destination)

    def test_manifest_builder_emits_valid_complete_json(self):
        with tempfile.TemporaryDirectory() as temporary:
            artifacts = pathlib.Path(temporary)
            manifest_targets = VALIDATOR.UNIX_TARGETS + ("universal-apple-darwin",)
            for target in manifest_targets:
                (artifacts / f"terraphim-agent-1.21.14-{target}.tar.gz").write_bytes(b"x")
            result = subprocess.run(
                [
                    str(ROOT / "scripts/build-manifest.sh"),
                    "1.21.14",
                    "terraphim-agent",
                    str(artifacts),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            manifest = json.loads(result.stdout)
            self.assertEqual(manifest["version"], "1.21.14")
            self.assertEqual(set(manifest["assets"]), set(manifest_targets))

    def test_manifest_builder_fails_when_a_target_is_missing(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = subprocess.run(
                [
                    str(ROOT / "scripts/build-manifest.sh"),
                    "1.21.14",
                    "terraphim-agent",
                    temporary,
                ],
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing manifest asset", result.stderr)


if __name__ == "__main__":
    unittest.main()
