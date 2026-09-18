import os
import re
import subprocess
import textwrap
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "release-binaries.yml"
SIGN_MACOS_BINARY = ROOT / "scripts" / "sign-macos-binary.sh"


def workflow_text() -> str:
    return WORKFLOW.read_text()


def preflight_python_validator() -> str:
    text = workflow_text()
    start = text.index("          python3 - <<'PY'\n") + len("          python3 - <<'PY'\n")
    end = text.index("          PY\n", start)
    lines = text[start:end].splitlines()
    return textwrap.dedent("\n".join(line[10:] for line in lines) + "\n")


def job_block(job_name: str) -> str:
    text = workflow_text()
    start = text.index(f"  {job_name}:")
    match = re.search(r"\n  [a-zA-Z0-9_-]+:\n", text[start + 1 :])
    if match is None:
        return text[start:]
    return text[start : start + 1 + match.start()]


def stage_env(**updates: str) -> dict[str, str]:
    env = os.environ.copy()
    env.update(
        {
            "VERSION": "1.21.15",
            "RELEASE_TAG": "v1.21.15",
            "SOURCE_REF": "v1.21.15",
            "EXPECTED_SOURCE_SHA": "b" * 40,
            "TARGET_REPO": "terraphim-ai",
            "CORRELATION_ID": "terraphim-ai/release-1.21.15:248",
            "PUBLISH_TO_TARGET_RELEASE": "false",
        }
    )
    env.update(updates)
    return env


class ReleaseBinariesWorkflowContract(unittest.TestCase):
    def test_run_name_preserves_exact_correlation_identity(self) -> None:
        self.assertIn(
            "run-name: Release ${{ inputs.release_tag }} from "
            "${{ inputs.expected_source_sha }} "
            "(correlation ${{ inputs.correlation_id }})",
            workflow_text(),
        )

    def test_dispatch_is_stage_only_by_default(self) -> None:
        text = workflow_text()
        self.assertRegex(
            text,
            r"publish_to_target_release:\n"
            r"\s+description:.*\n\s+required: false\n\s+default: false\n\s+type: boolean",
        )
        self.assertRegex(
            text,
            r"expected_source_sha:\n\s+description:.*\n\s+required: true\n\s+type: string",
        )
        self.assertRegex(
            text,
            r"correlation_id:\n\s+description:.*\n\s+required: true\n\s+type: string",
        )

    def test_preflight_validator_accepts_only_stage_identity(self) -> None:
        accepted = subprocess.run(
            ["python3", "-c", preflight_python_validator()],
            env=stage_env(),
            text=True,
            capture_output=True,
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr)

        cases = (
            ({"VERSION": "v1.21.15"}, "stable semantic version"),
            ({"RELEASE_TAG": "v1.21.14"}, "must equal 'v' plus version"),
            ({"SOURCE_REF": "main"}, "must equal source_ref"),
            ({"EXPECTED_SOURCE_SHA": "B" * 40}, "lowercase hex SHA"),
            ({"TARGET_REPO": "terraphim-clients"}, "stage-only mode"),
            ({"PUBLISH_TO_TARGET_RELEASE": "true"}, "stage-only producer"),
            ({"CORRELATION_ID": "unsafe value"}, "unsafe characters"),
        )
        for updates, message in cases:
            with self.subTest(updates=updates):
                result = subprocess.run(
                    ["python3", "-c", preflight_python_validator()],
                    env=stage_env(**updates),
                    text=True,
                    capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)

    def test_preflight_recursively_peels_to_exact_expected_sha(self) -> None:
        block = job_block("preflight")
        self.assertIn("while [ \"$object_type\" != \"commit\" ]", block)
        self.assertIn("/git/ref/tags/${ref_name}", block)
        self.assertIn("/git/tags/${object_sha}", block)
        self.assertIn('[ "$source_sha" = "$EXPECTED_SOURCE_SHA" ]', block)

    def test_release_uses_checked_in_version_and_never_mutates_source(self) -> None:
        text = workflow_text()
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
        self.assertEqual(workspace["workspace"]["package"]["version"], "1.21.15")
        for forbidden in (
            "Set release version",
            "set_section_version",
            'p.write_text(',
            "cargo update",
        ):
            self.assertNotIn(forbidden, text)
        self.assertIn("cargo metadata --locked --no-deps --format-version 1", text)
        self.assertGreaterEqual(text.count("git diff --exit-code -- Cargo.toml Cargo.lock"), 4)
        self.assertGreaterEqual(text.count("git status --porcelain"), 4)
        self.assertIn('release_tag != f"v{workspace_version}"', text)
        self.assertIn('version != workspace_version', text)

    def test_every_source_checkout_consumes_the_peeled_sha(self) -> None:
        text = workflow_text()
        checkout_refs = re.findall(r"uses: actions/checkout@[0-9a-f]{40}[^\n]*\n\s+with:\n\s+ref: ([^\n]+)", text)
        self.assertGreaterEqual(len(checkout_refs), 4)
        for ref in checkout_refs:
            self.assertIn("source_sha", ref)
        self.assertNotIn("recovery-tooling", text)
        self.assertNotIn("workflow_sha", text)

    def test_matrix_is_the_exact_six_lane_contract(self) -> None:
        text = workflow_text()
        expected = {
            ("ubuntu-22.04", "x86_64-unknown-linux-gnu", "false"),
            ("ubuntu-22.04", "x86_64-unknown-linux-musl", "true"),
            ("ubuntu-22.04", "aarch64-unknown-linux-musl", "true"),
            ("macos-15-intel", "x86_64-apple-darwin", "false"),
            ("macos-15", "aarch64-apple-darwin", "false"),
            ("windows-latest", "x86_64-pc-windows-msvc", "false"),
        }
        actual = set(
            re.findall(
                r"- os: ([^\n]+)\n\s+target: ([^\n]+)\n\s+use_cross: (true|false)",
                text,
            )
        )
        self.assertEqual(actual, expected)
        self.assertIn("fail-fast: false", text)

    def test_builds_are_locked_and_grep_features_are_preserved(self) -> None:
        block = job_block("build-binaries")
        for package, binary in (
            ("terraphim_agent", "terraphim-agent"),
            ("terraphim-cli", "terraphim-cli"),
        ):
            self.assertIn(
                f'build --locked --release --target "${{{{ matrix.target }}}}" -p {package} --bin {binary}',
                block,
            )
        self.assertIn(
            '-p terraphim_grep --bin terraphim-grep --features "code-search openrouter"',
            block,
        )

    def test_all_binaries_get_exact_version_and_architecture_checks(self) -> None:
        block = job_block("build-binaries")
        self.assertIn("for binary in terraphim-agent terraphim-cli terraphim-grep", block)
        self.assertIn("qemu-aarch64-static", block)
        self.assertIn('scripts/validate_release_binary.py "$TARGET" "$path"', block)
        self.assertIn("--version", block)
        self.assertIn('[ "$reported" = "$VERSION" ]', block)

    def test_omarchy_targets_are_required_for_agent_and_grep(self) -> None:
        stage = job_block("seal-release-stage")
        self.assertIn("x86_64-unknown-linux-musl", stage)
        self.assertIn("aarch64-unknown-linux-musl", stage)
        self.assertIn("for binary in terraphim-agent terraphim-cli terraphim-grep", stage)
        self.assertIn('if [ "$binary" != "terraphim-cli" ]; then targets+=(universal-apple-darwin); fi', stage)
        self.assertIn('test "$(wc -l < expected-assets.txt | tr -d \' \')" = 20', stage)

    def test_macos_is_signed_before_deterministic_packaging(self) -> None:
        text = workflow_text()
        signing = job_block("sign-and-notarize-macos")
        stage = job_block("seal-release-stage")
        self.assertIn("scripts/sign-macos-binary.sh", signing)
        self.assertIn("codesign --verify --strict", signing)
        self.assertIn("signed-client-binaries-apple-darwin", signing)
        self.assertIn("name: signed-client-binaries-apple-darwin", stage)
        self.assertLess(text.index("  sign-and-notarize-macos:"), text.index("  seal-release-stage:"))

    def test_archives_are_deterministic_and_have_exact_layout(self) -> None:
        stage = job_block("seal-release-stage")
        for token in (
            "SOURCE_DATE_EPOCH",
            "tar --sort=name",
            '--owner=0 --group=0 --numeric-owner',
            "gzip -n -9",
            "scripts/create-deterministic-zip.py",
            "LICENSE-Apache-2.0",
            "LICENSE-MIT",
            "expected-assets.txt",
            "diff -u expected-assets.txt actual-assets.txt",
            "scripts/validate-release-archive.py",
        ):
            self.assertIn(token, stage)

    def test_final_bytes_are_signed_before_checksums_and_manifests(self) -> None:
        stage = job_block("seal-release-stage")
        sign = stage.index("scripts/sign-release-archives.sh release-assets")
        verify = stage.index("--verify-only release-assets", sign)
        validate = stage.index("scripts/validate-release-archive.py", verify)
        sums = stage.index("../SHA256SUMS", validate)
        manifests = stage.index("scripts/build-manifest.sh", sums)
        self.assertLess(sign, verify)
        self.assertLess(verify, validate)
        self.assertLess(validate, sums)
        self.assertLess(sums, manifests)
        self.assertNotIn("../SHA256SUMS", stage[:sign])

    def test_producer_is_stage_only_and_has_no_public_writer(self) -> None:
        text = workflow_text()
        for forbidden in (
            "upload-to-target-release:",
            "gh release upload",
            "wrangler r2 object put",
            "contents: write",
            "--clobber",
            "CLOUDFLARE_API_TOKEN",
            "TERRAPHIM_AI_RELEASE_TOKEN",
        ):
            self.assertNotIn(forbidden, text)
        self.assertIn("  seal-release-stage:", text)
        self.assertIn("overwrite: false", text)

    def test_every_job_has_read_only_contents_permission(self) -> None:
        text = workflow_text()
        self.assertIn("permissions:\n  contents: read", text.split("jobs:", 1)[0])
        for job in (
            "preflight",
            "build-binaries",
            "create-universal-macos",
            "sign-and-notarize-macos",
            "seal-release-stage",
        ):
            self.assertIn("permissions:\n      contents: read", job_block(job))

    def test_final_stage_artifact_is_immutable_and_complete(self) -> None:
        stage = job_block("seal-release-stage")
        self.assertIn(
            "name: client-release-stage-${{ needs.preflight.outputs.version }}-${{ needs.preflight.outputs.source_sha }}",
            stage,
        )
        for path in (
            "release-assets/*",
            "canonical-binaries/*",
            "manifests/*.candidate.json",
            "SHA256SUMS",
            "BINARY_SHA256SUMS",
            "expected-assets.txt",
            "provenance.json",
        ):
            self.assertIn(path, stage)
        self.assertIn('"stage_identity": f"client-release-stage-', stage)
        self.assertIn("if-no-files-found: error", stage)
        self.assertIn("overwrite: false", stage)

    def test_macos_notarization_binds_exact_submission_and_fails_closed(self) -> None:
        text = SIGN_MACOS_BINARY.read_text()
        self.assertIn("--output-format json", text)
        self.assertIn('data["id"], data["status"]', text)
        self.assertIn('if [ "$SUBMISSION_STATUS" != "Accepted" ]; then', text)
        self.assertIn('notarytool log "$SUBMISSION_ID"', text)
        self.assertNotIn("notarytool history", text)

    def test_toolchains_actions_and_secret_scopes_are_pinned(self) -> None:
        text = workflow_text()
        for mutable in (
            "actions/checkout@v4", "actions/upload-artifact@v4",
            "actions/download-artifact@v4", "Swatinem/rust-cache@v2",
            "dtolnay/rust-toolchain@stable", "cargo install zipsign --locked",
        ):
            self.assertNotIn(mutable, text)
        self.assertIn("rustup toolchain install 1.96.0", text)
        self.assertIn("cargo install zipsign --version 0.2.1 --locked", text)
        self.assertIn("--rev 88f49ff79e777bef6d3564531636ee4d3cc2f8d2", text)
        self.assertIn(
            "1password/install-cli-action@9a0c9dd934086b7ab1d90115d455bda1c53c2bdb",
            text,
        )
        for job in ("preflight", "build-binaries", "sign-and-notarize-macos", "seal-release-stage"):
            uses = re.findall(r"^\s*- uses:\s+([^\s#]+)", job_block(job), re.MULTILINE)
            for action in uses:
                self.assertRegex(
                    action,
                    r"^[^@]+@[0-9a-f]{40}$",
                    f"{job} contains a mutable action reference: {action}",
                )
        build = job_block("build-binaries")
        prefix = build[: build.index("steps:")]
        self.assertNotIn("CARGO_REGISTRIES_TERRAPHIM_TOKEN", prefix)
        install_cross = build[build.index("Install cross") : build.index("Install QEMU")]
        self.assertNotIn("secrets.", install_cross)
        signer = job_block("seal-release-stage")
        install_signer = signer[signer.index("Install archive signer") : signer.index("Sign every")]
        self.assertNotIn("secrets.", install_signer)

    def test_linux_canonical_bytes_are_stripped_before_all_qualification_and_hashing(self) -> None:
        build = job_block("build-binaries")
        built = build.index("Build all shipped binaries")
        strip = build.index("Reject unstripped final Linux package bytes", built)
        qualify = build.index("Verify exact binary versions and architectures", strip)
        collect = build.index("Collect canonical binaries without byte mutation", qualify)
        upload = build.index("upload-artifact@", collect)
        self.assertLess(built, strip)
        self.assertLess(strip, qualify)
        self.assertLess(qualify, collect)
        self.assertLess(collect, upload)
        self.assertIn("CARGO_PROFILE_RELEASE_STRIP: symbols", build)

        stage = job_block("seal-release-stage")
        canonical = stage.index("Stage and hash canonical Linux package bytes")
        binary_sums = stage.index("BINARY_SHA256SUMS", canonical)
        archive = stage.index("Create deterministic archives", binary_sums)
        self.assertLess(canonical, binary_sums)
        self.assertLess(binary_sums, archive)
        self.assertIn("scripts/stage-canonical-linux.py raw canonical-binaries BINARY_SHA256SUMS", stage)
        self.assertIn('source="canonical-binaries/$binary-$target"', stage)

    def test_macos_thin_execution_has_deterministic_runner_semantics(self) -> None:
        text = workflow_text()
        self.assertIn("os: macos-15-intel\n            target: x86_64-apple-darwin", text)
        self.assertIn("os: macos-15\n            target: aarch64-apple-darwin", text)
        signing = job_block("sign-and-notarize-macos")
        self.assertIn("runs-on: macos-15", signing)
        provision = signing.index("softwareupdate --install-rosetta --agree-to-license")
        execute = signing.index('arch -x86_64 "$path" --version')
        self.assertLess(provision, execute)
        self.assertNotIn("skip", signing.lower())

    def test_workflow_is_parsed_by_actionlint(self) -> None:
        result = subprocess.run(
            ["actionlint", str(WORKFLOW)], text=True, capture_output=True
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
