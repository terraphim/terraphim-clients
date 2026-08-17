import re
import os
import subprocess
import textwrap
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "release-binaries.yml"


def workflow_text() -> str:
    return WORKFLOW.read_text()


def preflight_python_validator() -> str:
    text = workflow_text()
    start = text.index("          python3 - <<'PY'\n") + len("          python3 - <<'PY'\n")
    end = text.index("          PY\n", start)
    lines = text[start:end].splitlines()
    return textwrap.dedent("\n".join(line[10:] for line in lines) + "\n")


class ReleaseBinariesWorkflowContract(unittest.TestCase):
    def test_dispatch_requires_immutable_source_inputs(self) -> None:
        text = workflow_text()

        self.assertRegex(text, r"source_ref:\n\s+description:")
        self.assertRegex(text, r"expected_source_sha:\n\s+description:")
        self.assertIn("required: true", text)

    def test_preflight_validates_hostile_inputs_before_checkout(self) -> None:
        text = workflow_text()
        preflight_index = text.index("  preflight:")
        first_checkout_index = text.index("actions/checkout@v4")

        self.assertLess(preflight_index, first_checkout_index)
        self.assertIn("input version", text)
        self.assertIn("release_tag", text)
        self.assertIn("source_ref", text)
        self.assertIn("target_repo", text)
        self.assertIn("expected_source_sha", text)
        self.assertIn("is not valid semver", text)
        self.assertIn("must equal source_ref", text)
        self.assertIn("is not allowed", text)
        self.assertIn("is not a 40-character lowercase hex SHA", text)
        self.assertIn("does not match expected_source_sha", text)

    def test_preflight_python_validator_accepts_recovery_contract(self) -> None:
        for target_repo in ("terraphim-clients", "terraphim-ai"):
            with self.subTest(target_repo=target_repo):
                env = os.environ.copy()
                env.update(
                    {
                        "VERSION": "1.21.12",
                        "RELEASE_TAG": "v1.21.12",
                        "SOURCE_REF": "v1.21.12",
                        "EXPECTED_SOURCE_SHA": "e080475ac26f44ad4674a438d753f6ab185fb787",
                        "TARGET_REPO": target_repo,
                    }
                )

                result = subprocess.run(
                    ["python3", "-c", preflight_python_validator()],
                    env=env,
                    text=True,
                    capture_output=True,
                )

                self.assertEqual(result.returncode, 0, result.stderr)

    def test_preflight_python_validator_rejects_hostile_inputs(self) -> None:
        base_env = os.environ.copy()
        base_env.update(
            {
                "VERSION": "1.21.12",
                "RELEASE_TAG": "v1.21.12",
                "SOURCE_REF": "v1.21.12",
                "EXPECTED_SOURCE_SHA": "e080475ac26f44ad4674a438d753f6ab185fb787",
                "TARGET_REPO": "terraphim-clients",
            }
        )
        cases = (
            ("VERSION", "v1.21.12", "is not valid semver"),
            ("RELEASE_TAG", "v1.21.13", "must equal 'v' plus version"),
            ("SOURCE_REF", "main", "must equal source_ref"),
            ("EXPECTED_SOURCE_SHA", "E080475AC26F44AD4674A438D753F6AB185FB787", "40-character lowercase hex SHA"),
            ("TARGET_REPO", "terraphim", "is not allowed"),
        )

        for key, value, error in cases:
            with self.subTest(key=key):
                env = base_env.copy()
                env[key] = value
                result = subprocess.run(
                    ["python3", "-c", preflight_python_validator()],
                    env=env,
                    text=True,
                    capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(error, result.stderr)

    def test_preflight_recursively_peels_tag_to_commit(self) -> None:
        text = workflow_text()

        self.assertIn("gh api", text)
        self.assertIn("repos/${{ github.repository }}/git/ref/tags/", text)
        self.assertIn("repos/${{ github.repository }}/git/tags/", text)
        self.assertIn('ref_json="$(gh api "repos/${{ github.repository }}/git/ref/tags/${ref_name}")"', text)
        self.assertIn('tag_json="$(gh api "repos/${{ github.repository }}/git/tags/${object_sha}")"', text)
        self.assertIn("while object_type != \"commit\"", text)
        self.assertIn("source_sha=", text)
        self.assertNotIn("release_sha=", text)

    def test_build_mutation_trusts_preflight_and_only_rewrites_versions(self) -> None:
        text = workflow_text()
        start = text.index("      - name: Set release version")
        end = text.index("      - name: Assert host binary reports", start)
        block = text[start:end]

        self.assertIn('VERSION = os.environ["VERSION"]', block)
        self.assertIn('set_section_version("Cargo.toml", "workspace.package")', block)
        self.assertIn(
            'set_section_version("crates/terraphim_agent/Cargo.toml", "package")',
            block,
        )
        self.assertIn("cargo metadata --no-deps --format-version 1", block)
        self.assertNotIn("SEMVER", block)
        self.assertNotIn("RELEASE_TAG", block)
        self.assertNotIn("SOURCE_REF", block)
        self.assertNotIn("TARGET_REPO", block)

    def test_all_source_checkouts_use_preflight_sha_and_assert_head(self) -> None:
        text = workflow_text()

        checkout_blocks = re.findall(
            r"- uses: actions/checkout@v4\n(?:\s+with:\n(?:\s{10,}.+\n)+)?",
            text,
        )
        self.assertGreaterEqual(len(checkout_blocks), 3)
        for block in checkout_blocks:
            self.assertIn("ref: ${{ needs.preflight.outputs.source_sha }}", block)

        self.assertGreaterEqual(
            text.count('git rev-parse HEAD)" != "${{ needs.preflight.outputs.source_sha }}"'),
            3,
        )

    def test_matrix_preserves_six_mandatory_lanes(self) -> None:
        text = workflow_text()

        expected_lanes = {
            ("ubuntu-22.04", "x86_64-unknown-linux-gnu", "false"),
            ("ubuntu-22.04", "x86_64-unknown-linux-musl", "true"),
            ("ubuntu-22.04", "aarch64-unknown-linux-musl", "true"),
            ("macos-latest", "x86_64-apple-darwin", "false"),
            ("macos-latest", "aarch64-apple-darwin", "false"),
            ("windows-latest", "x86_64-pc-windows-msvc", "false"),
        }
        actual_lanes = set(
            re.findall(
                r"- os: ([^\n]+)\n\s+target: ([^\n]+)\n\s+use_cross: (true|false)",
                text,
            )
        )

        self.assertEqual(expected_lanes, actual_lanes)
        self.assertIn("fail-fast: false", text)

    def test_windows_builds_release_binary_with_msvc_stack_and_asserts_version(self) -> None:
        text = workflow_text()

        self.assertIn("Diagnostic Windows build/run without /STACK:8388608", text)
        self.assertIn("set +e", text)
        self.assertIn("windows-no-stack-diagnostic.log", text)
        self.assertIn("Assert Windows mitigated release binary reports the release version", text)
        self.assertIn("/STACK:8388608", text)
        self.assertIn("cargo build --release --target ${{ matrix.target }} -p terraphim_agent --bin terraphim-agent", text)
        self.assertIn("target/${{ matrix.target }}/release/terraphim-agent.exe", text)
        self.assertIn("--version", text)
        self.assertIn("awk '{print $NF}'", text)
        self.assertIn('if [ "$reported" != "$VERSION" ]; then', text)
        self.assertIn("Build client binaries (Windows stack reserve)", text)

    def test_non_windows_builds_do_not_set_empty_rustflags(self) -> None:
        text = workflow_text()

        self.assertNotIn("|| ''", text)
        self.assertNotIn("RUSTFLAGS: ${{ matrix.os == 'windows-latest'", text)
        self.assertRegex(
            text,
            r"- name: Build client binaries\n\s+if: matrix\.os != 'windows-latest'\n\s+shell: bash\n\s+run:",
        )
        self.assertRegex(
            text,
            r"- name: Build client binaries \(Windows stack reserve\)\n\s+if: matrix\.os == 'windows-latest'\n\s+shell: bash\n\s+env:\n\s+RUSTFLAGS: -C link-arg=/STACK:8388608",
        )

    def test_upload_and_r2_are_fail_closed_on_preflight_and_builds(self) -> None:
        text = workflow_text()

        self.assertIn("needs: [preflight, build-binaries]", text)
        self.assertIn("needs.preflight.result == 'success'", text)
        self.assertIn("needs.build-binaries.result == 'success'", text)
        self.assertNotIn("needs.build-binaries.result != 'cancelled'", text)
        self.assertIn("needs: [preflight, build-binaries, sign-and-notarize-macos]", text)
        self.assertIn("needs.preflight.result == 'success'", text)
        self.assertIn("needs.build-binaries.result == 'success'", text)
        self.assertIn("needs.sign-and-notarize-macos.result == 'success'", text)
        self.assertIn("needs: [preflight, create-universal-macos]", text)
        self.assertIn("needs.preflight.result == 'success'", text)
        self.assertIn("needs.create-universal-macos.result == 'success'", text)
        self.assertIn("RELEASE_TAG: ${{ needs.preflight.outputs.release_tag }}", text)
        self.assertIn("TARGET_REPO: ${{ needs.preflight.outputs.target_repo }}", text)
        self.assertIn("VERSION: ${{ needs.preflight.outputs.version }}", text)

    def test_upload_downloads_platform_artifacts_and_only_signed_universal(self) -> None:
        text = workflow_text()
        start = text.index("  upload-to-target-release:")
        end = text.index("      - name: Install zipsign", start)
        block = text[start:end]

        expected_artifacts = (
            "client-binaries-x86_64-unknown-linux-gnu",
            "client-binaries-x86_64-unknown-linux-musl",
            "client-binaries-aarch64-unknown-linux-musl",
            "client-binaries-x86_64-apple-darwin",
            "client-binaries-aarch64-apple-darwin",
            "client-binaries-x86_64-pc-windows-msvc",
            "client-binaries-signed-universal-apple-darwin",
        )
        for artifact in expected_artifacts:
            self.assertIn(f"name: {artifact}", block)

        self.assertNotIn("pattern: client-binaries", block)
        self.assertNotIn("merge-multiple", block)
        self.assertNotIn("name: client-binaries-universal-apple-darwin", block)


if __name__ == "__main__":
    unittest.main()
