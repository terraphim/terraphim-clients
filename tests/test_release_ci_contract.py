import hashlib
import importlib.util
import json
import re
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
        # On Gitea the strict-v2 manifest / R2-update contract suite runs in
        # .gitea/workflows/native-ci.yml, which is intentionally not part of
        # the GitHub port. On GitHub the same updater integration contract
        # runs as an explicit lane in the main ci.yml job, so the assertion
        # is checked against that home instead.
        self.assertIn(
            "cargo test -p terraphim_update --test manifest --test r2_update",
            text,
        )

    def test_github_ci_runs_manifest_builder_compatibility(self) -> None:
        """The dual-mode `scripts/build-manifest.sh` is the only seam
        shared by the v1.21.14 finalizer (3-arg legacy stdout) and the
        v1.21.16 producer (4-arg strict v2 candidate). Any future edit to
        either call site must be paired with a run of the compatibility
        contract; without it the cross-contract path-key reconciliation
        can drift silently. Fail here when the run is omitted so the
        omission cannot pass CI.
        """
        text = (ROOT / ".github" / "workflows" / "ci.yml").read_text()
        self.assertIn(
            "python3 -m unittest tests.test_manifest_builder_compatibility",
            text,
            "ci.yml must run tests.test_manifest_builder_compatibility; "
            "the dual-mode build-manifest.sh is the only seam shared by "
            "the v1.21.14 finalizer (3-arg legacy stdout) and the "
            "v1.21.16 producer (4-arg strict v2 candidate) and the "
            "compat test guards the cross-contract path-key reconciliation.",
        )
        # The compatibility module must exist as a runnable unittest
        # module: a deletion would also break CI indirectly, but the
        # contract fails first to keep the failure surface tight.
        compat_path = ROOT / "tests" / "test_manifest_builder_compatibility.py"
        self.assertTrue(compat_path.is_file())
        spec = importlib.util.spec_from_file_location(
            "compat_under_test", compat_path
        )
        self.assertIsNotNone(spec, "compat module must be importable")
        self.assertIsNotNone(spec.loader)

    # --- actionlint provisioning contract -------------------------------
    #
    # Hosted PR run 35433041935 failed both ci.yml jobs with
    # FileNotFoundError: actionlint -- tests/test_release_binaries_
    # workflow_contract.py shells out to actionlint but the workflow never
    # installed it. The contract below makes both jobs provision the
    # checksum-pinned tool *before* any actionlint-using suite and keeps
    # the pin from drifting.

    ACTIONLINT_VERSION = "1.7.12"
    ACTIONLINT_SHA256 = (
        "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"
    )
    INSTALLER = ROOT / ".github" / "scripts" / "install-actionlint.sh"
    # The suites that invoke actionlint (as a subprocess) and therefore
    # require the binary on PATH before they run.
    ACTIONLINT_USING_SUITES = (
        "python3 -m unittest discover -s tests -p 'test_*release*contract.py' -v",
        "python3 -m unittest -v tests.test_release_binaries_workflow_contract",
    )

    def installer_text(self) -> str:
        return self.INSTALLER.read_text()

    def assert_actionlint_provisioning_contract(self, ci_text: str) -> None:
        installer = self.installer_text()
        # The installer itself must stay pinned to the official release
        # artefact, its official checksum, and verify the exact version.
        # The archive/URL lines are the script's own parameterised forms:
        # single-sourcing the version string means a version bump cannot
        # leave a stale literal behind.
        for pinned in (
            'ACTIONLINT_VERSION="1.7.12"',
            'ACTIONLINT_ARCHIVE="actionlint_${ACTIONLINT_VERSION}_linux_amd64.tar.gz"',
            'ACTIONLINT_URL="https://github.com/rhysd/actionlint/releases/'
            'download/v${ACTIONLINT_VERSION}/${ACTIONLINT_ARCHIVE}"',
            'ACTIONLINT_SHA256="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"',
            "set -euo pipefail",
            "sha256sum -c -",
            ': "${RUNNER_TEMP:?RUNNER_TEMP must point at the job-scoped temporary directory}"',
            ': "${GITHUB_PATH:?GITHUB_PATH must point at the job PATH mutation file}"',
            'bin_dir="${RUNNER_TEMP}/actionlint-${ACTIONLINT_VERSION}-bin"',
            'grep -qx "${ACTIONLINT_VERSION}"',
            'printf \'%s\\n\' "$bin_dir" >> "$GITHUB_PATH"',
        ):
            self.assertIn(pinned, installer, f"installer lost {pinned!r}")
        # The CI-only installer must not regain a success-with-guidance
        # fallback or an off-RUNNER_TEMP install root: both would let the
        # provisioning step "succeed" without the job ever seeing the tool.
        self.assertNotIn("add it to PATH", installer)
        self.assertNotIn(":-$(mktemp -d)", installer)
        self.assertNotIn("${RUNNER_TEMP:-", installer)
        # Both jobs must provision the tool before any actionlint-using
        # suite can run.
        for job in ("build:", "client-packaging-contracts:"):
            job_start = ci_text.index(f"  {job}")
            boundary = re.search(
                r"\n  [A-Za-z0-9_-]+:", ci_text[job_start + 1 :]
            )
            job_end = (
                len(ci_text)
                if boundary is None
                else job_start + 1 + boundary.start()
            )
            block = ci_text[job_start:job_end]
            self.assertIn(
                ".github/scripts/install-actionlint.sh",
                block,
                f"ci.yml job {job!r} must run the pinned actionlint "
                "installer; the workflow-contract suites shell out to "
                "actionlint and fail with FileNotFoundError otherwise "
                "(hosted run 35433041935)",
            )
            installer_at = block.index(".github/scripts/install-actionlint.sh")
            for suite in self.ACTIONLINT_USING_SUITES:
                if suite in block:
                    self.assertLess(
                        installer_at,
                        block.index(suite),
                        f"ci.yml job {job!r} must install actionlint "
                        f"before {suite!r}",
                    )

    def test_ci_provisions_checksum_pinned_actionlint_before_contracts(self) -> None:
        ci_text = (ROOT / ".github" / "workflows" / "ci.yml").read_text()
        installer_text = self.installer_text()
        self.assert_actionlint_provisioning_contract(ci_text)

        # ci.yml mutations are compared against the ci.yml baseline and
        # checked by pointing the contract at the mutated workflow; installer
        # mutations are compared against the installer baseline and checked
        # by swapping the installer on disk (restored in a finally block).
        ci_mutations = {
            "drop-build-installer": ci_text.replace(
                "      - name: Install pinned actionlint\n"
                "        run: .github/scripts/install-actionlint.sh\n"
                "      - name: Release workflow and sealing contracts\n",
                "      - name: Release workflow and sealing contracts\n",
                1,
            ),
            "drop-packaging-installer": ci_text.replace(
                "      - name: Install pinned actionlint\n"
                "        run: .github/scripts/install-actionlint.sh\n"
                "      - name: Managed package producer contracts (workflow, shell)\n",
                "      - name: Managed package producer contracts (workflow, shell)\n",
                1,
            ),
        }
        for name, mutant in ci_mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, ci_text)
                with self.assertRaises(AssertionError):
                    self.assert_actionlint_provisioning_contract(mutant)

        installer_mutations = {
            "drift-version": installer_text.replace(
                f'ACTIONLINT_VERSION="{self.ACTIONLINT_VERSION}"',
                'ACTIONLINT_VERSION="1.7.11"',
                1,
            ),
            "drift-checksum": installer_text.replace(
                self.ACTIONLINT_SHA256,
                "0" * 64,
                1,
            ),
            "drift-archive": installer_text.replace(
                "_linux_amd64.tar.gz",
                "_linux_386.tar.gz",
                1,
            ),
            "drop-version-proof": installer_text.replace(
                'grep -qx "${ACTIONLINT_VERSION}"', "true", 1
            ),
            "drop-runner-temp-guard": installer_text.replace(
                ': "${RUNNER_TEMP:?RUNNER_TEMP must point at the job-scoped '
                'temporary directory}"\n',
                "",
                1,
            ),
            "drop-github-path-guard": installer_text.replace(
                ': "${GITHUB_PATH:?GITHUB_PATH must point at the job PATH '
                'mutation file}"\n',
                "",
                1,
            ),
            "restore-guidance-fallback": installer_text.replace(
                "printf '%s\\n' \"$bin_dir\" >> \"$GITHUB_PATH\"",
                'if [ -n "${GITHUB_PATH:-}" ]; then\n'
                "    printf '%s\\n' \"$bin_dir\" >> \"$GITHUB_PATH\"\n"
                "else\n"
                "    echo \"actionlint installed at ${bin_dir}; add it to PATH\" >&2\n"
                "fi",
                1,
            ),
        }
        for name, mutant in installer_mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, installer_text)
                original = self.INSTALLER.read_text()
                try:
                    self.INSTALLER.write_text(mutant)
                    with self.assertRaises(AssertionError):
                        self.assert_actionlint_provisioning_contract(ci_text)
                finally:
                    self.INSTALLER.write_text(original)

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
