import os
import re
import subprocess
import textwrap
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "release-binaries.yml"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
SIGN_MACOS_BINARY = ROOT / "scripts" / "sign-macos-binary.sh"
ASSEMBLE_CLIENT_INVENTORY = (
    ROOT / ".github" / "scripts" / "release" / "assemble-client-release-inventory.sh"
)
NATIVE_ACTUAL_LIFECYCLE = (
    ROOT / ".github" / "scripts" / "nfpm" / "tests" / "test_client_nfpm_native_actual.sh"
)

CHECKOUT_SHA = "11bd71901bbe5b1630ceea73d27597364c9af683"
SETUP_PYTHON_SHA = "a26af69be951a213d495a4c3e4e4022e16d87065"
NFPM_VERSION = "2.47.0"
NFPM_X86_ARCHIVE_SHA256 = "0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783"
NFPM_X86_BINARY_SHA256 = "17133a2467ffb7cec851c2d7bae0c6098d09d7ed7d3d101a9605f6a473323936"
NFPM_ARM_ARCHIVE_SHA256 = "1c0f5f2999b9a974bfb04fdb0cc3306096de530ac5dbb25d739cc5f5219c919c"
NFPM_ARM_BINARY_SHA256 = "4d7ddf169945f7f557ac5373035d373429050d00476925953560e1ac65e16c74"


def workflow_text() -> str:
    return WORKFLOW.read_text()


def ci_workflow_text() -> str:
    return CI_WORKFLOW.read_text()


def preflight_python_validator() -> str:
    text = workflow_text()
    start = text.index("          python3 - <<'PY'\n") + len("          python3 - <<'PY'\n")
    end = text.index("          PY\n", start)
    lines = text[start:end].splitlines()
    return textwrap.dedent("\n".join(line[10:] for line in lines) + "\n")


def job_block_from(text: str, job_name: str) -> str:
    start = text.index(f"  {job_name}:")
    match = re.search(r"\n  [a-zA-Z0-9_-]+:\n", text[start + 1 :])
    if match is None:
        return text[start:]
    return text[start : start + 1 + match.start()]


def job_block(job_name: str) -> str:
    return job_block_from(workflow_text(), job_name)


def checkout_contract(text: str) -> list[tuple[str, str, str]]:
    """Return every checkout as (job, immutable action, explicit ref)."""
    lines = text.splitlines()
    current_job = ""
    checkouts: list[tuple[str, str, str]] = []
    for index, line in enumerate(lines):
        job_match = re.fullmatch(r"  ([A-Za-z0-9_-]+):", line)
        if job_match:
            current_job = job_match.group(1)
            continue
        action_match = re.fullmatch(
            r"(\s*)- uses: (actions/checkout@[^\s#]+)(?:\s+#.*)?", line
        )
        if not action_match:
            continue
        indent = len(action_match.group(1))
        body: list[str] = []
        for following in lines[index + 1 :]:
            if re.match(rf"^\s{{{indent}}}- ", following):
                break
            body.append(following)
        refs = [
            match.group(1).strip()
            for body_line in body
            if (match := re.fullmatch(r"\s+ref:\s*(.+)", body_line))
        ]
        if len(refs) != 1:
            refs = []
        checkouts.append((current_job, action_match.group(2), refs[0] if refs else ""))
    return checkouts


def remove_nth_matching_line(text: str, pattern: str, occurrence: int) -> str:
    matches = list(re.finditer(pattern, text, re.MULTILINE))
    if occurrence >= len(matches):
        raise AssertionError(f"missing mutation occurrence {occurrence} for {pattern!r}")
    match = matches[occurrence]
    return text[: match.start()] + text[match.end() :]


def stage_env(**updates: str) -> dict[str, str]:
    env = os.environ.copy()
    env.update(
        {
            "VERSION": "1.21.16",
            "RELEASE_TAG": "v1.21.16",
            "SOURCE_REF": "v1.21.16",
            "EXPECTED_SOURCE_SHA": "b" * 40,
            "TARGET_REPO": "terraphim-ai",
            "CORRELATION_ID": "terraphim-ai/release-1.21.16:248",
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
            ({"VERSION": "v1.21.16"}, "stable semantic version"),
            ({"RELEASE_TAG": "v1.21.15"}, "must equal 'v' plus version"),
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
        self.assertEqual(workspace["workspace"]["package"]["version"], "1.21.16")
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

    def assert_release_checkout_contract(self, text: str) -> None:
        expected = [
            ("preflight", f"actions/checkout@{CHECKOUT_SHA}", "${{ steps.contract.outputs.source_sha }}"),
            ("build-binaries", f"actions/checkout@{CHECKOUT_SHA}", "${{ needs.preflight.outputs.source_sha }}"),
            ("stage-canonical-linux", f"actions/checkout@{CHECKOUT_SHA}", "${{ needs.preflight.outputs.source_sha }}"),
            ("build-client-packages", f"actions/checkout@{CHECKOUT_SHA}", "${{ needs.preflight.outputs.source_sha }}"),
            ("sign-and-notarize-macos", f"actions/checkout@{CHECKOUT_SHA}", "${{ needs.preflight.outputs.source_sha }}"),
            ("seal-release-stage", f"actions/checkout@{CHECKOUT_SHA}", "${{ needs.preflight.outputs.source_sha }}"),
        ]
        self.assertEqual(checkout_contract(text), expected)
        self.assertNotIn("recovery-tooling", text)
        self.assertNotIn("workflow_sha", text)

    def test_every_checkout_is_counted_pinned_and_bound_to_the_peeled_sha(self) -> None:
        text = workflow_text()
        self.assert_release_checkout_contract(text)

        ref_pattern = r"^\s+ref: \$\{\{ (?:steps\.contract|needs\.preflight)\.outputs\.source_sha \}\}\n"
        for checkout_index in range(6):
            with self.subTest(mutation="delete-checkout-ref", checkout=checkout_index):
                mutant = remove_nth_matching_line(text, ref_pattern, checkout_index)
                with self.assertRaises(AssertionError):
                    self.assert_release_checkout_contract(mutant)

        mutable = text.replace(
            f"actions/checkout@{CHECKOUT_SHA}", "actions/checkout@v4", 1
        )
        with self.assertRaises(AssertionError):
            self.assert_release_checkout_contract(mutable)

        extra_mutable = text.replace(
            "    steps:\n",
            "    steps:\n      - uses: actions/checkout@v4\n",
            1,
        )
        with self.assertRaises(AssertionError):
            self.assert_release_checkout_contract(extra_mutable)

    def assert_client_packaging_ci_contract(self, text: str) -> None:
        self.assertIn("  client-packaging-contracts:\n", text)
        block = job_block_from(text, "client-packaging-contracts")
        self.assertIn('REQUIRE_TOOLS: "1"', block)
        self.assertNotIn('REQUIRE_TOOLS: "0"', block)

        required_suites = (
            "python3 -m unittest -v tests.test_release_binaries_workflow_contract",
            ".github/scripts/nfpm/tests/test_client_nfpm.sh",
            ".github/scripts/nfpm/tests/test_client_nfpm_arch.sh",
            ".github/scripts/nfpm/tests/test_client_nfpm_policy.sh",
            ".github/scripts/nfpm/tests/test_client_nfpm_static_lint.sh",
            ".github/scripts/nfpm/tests/test_client_nfpm_strip.sh",
            ".github/scripts/nfpm/tests/test_verify_nfpm.sh",
            ".github/scripts/release/tests/test_assemble_client_release_inventory.sh",
        )
        for suite in required_suites:
            self.assertEqual(block.count(suite), 1, suite)

        self.assertIn(f"NFPM_VERSION: {NFPM_VERSION}", block)
        self.assertIn(f"NFPM_ARCHIVE_SHA256: {NFPM_X86_ARCHIVE_SHA256}", block)
        self.assertIn(f"NFPM_BINARY_SHA256: {NFPM_X86_BINARY_SHA256}", block)
        for wiring in (
            'nfpm_${NFPM_VERSION}_Linux_x86_64.tar.gz',
            'v${NFPM_VERSION}/nfpm_${NFPM_VERSION}_Linux_x86_64.tar.gz',
            '"$NFPM_ARCHIVE_SHA256" "$archive" | sha256sum -c -',
            '"$NFPM_BINARY_SHA256" "$install_dir/nfpm" | sha256sum -c -',
            '--version "$NFPM_VERSION"',
            '--sha256 "$NFPM_BINARY_SHA256"',
        ):
            self.assertIn(wiring, block)

        uses = re.findall(r"^\s*- uses:\s+([^\s#]+)", block, re.MULTILINE)
        self.assertEqual(
            uses,
            [
                f"actions/checkout@{CHECKOUT_SHA}",
                f"actions/setup-python@{SETUP_PYTHON_SHA}",
            ],
        )
        self.assertNotIn("secrets.", block)

    def test_client_packaging_ci_job_is_non_vacuous_and_mutation_sensitive(self) -> None:
        text = ci_workflow_text()
        self.assert_client_packaging_ci_contract(text)
        block = job_block_from(text, "client-packaging-contracts")

        mutations = {
            "delete-job": text.replace(block, ""),
            "disable-required-tools": text.replace('REQUIRE_TOOLS: "1"', 'REQUIRE_TOOLS: "0"', 1),
            "change-nfpm-version": text.replace(f"NFPM_VERSION: {NFPM_VERSION}", "NFPM_VERSION: 2.46.0", 1),
            "change-nfpm-archive-sha": text.replace(NFPM_X86_ARCHIVE_SHA256, "0" * 64, 1),
            "change-nfpm-binary-sha": text.replace(NFPM_X86_BINARY_SHA256, "0" * 64, 1),
            "unpin-checkout": text.replace(f"actions/checkout@{CHECKOUT_SHA}", "actions/checkout@v4", 1),
            "unpin-setup-python": text.replace(f"actions/setup-python@{SETUP_PYTHON_SHA}", "actions/setup-python@v5", 1),
            "delete-archive-verification": text.replace(
                '          printf \'%s  %s\\n\' "$NFPM_ARCHIVE_SHA256" "$archive" | sha256sum -c -\n',
                "",
                1,
            ),
            "delete-binary-verification": text.replace(
                '          printf \'%s  %s\\n\' "$NFPM_BINARY_SHA256" "$install_dir/nfpm" | sha256sum -c -\n',
                "",
                1,
            ),
        }
        suites = (
            "python3 -m unittest -v tests.test_release_binaries_workflow_contract",
            ".github/scripts/nfpm/tests/test_client_nfpm.sh",
            ".github/scripts/nfpm/tests/test_client_nfpm_arch.sh",
            ".github/scripts/nfpm/tests/test_client_nfpm_policy.sh",
            ".github/scripts/nfpm/tests/test_client_nfpm_static_lint.sh",
            ".github/scripts/nfpm/tests/test_client_nfpm_strip.sh",
            ".github/scripts/nfpm/tests/test_verify_nfpm.sh",
            ".github/scripts/release/tests/test_assemble_client_release_inventory.sh",
        )
        mutations.update(
            {
                f"delete-suite-{index}": text.replace(suite, f"missing-suite-{index}", 1)
                for index, suite in enumerate(suites)
            }
        )
        for name, mutant in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, text)
                with self.assertRaises((AssertionError, ValueError)):
                    self.assert_client_packaging_ci_contract(mutant)

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

    def assert_seal_package_result_contract(self, text: str) -> None:
        seal = job_block_from(text, "seal-release-stage")
        condition = seal.split("    if:", 1)[1].split("    runs-on:", 1)[0]
        package_results = set(
            re.findall(
                r"needs\.build-client-packages\.result\s*==\s*'([^']+)'",
                condition,
            )
        )
        self.assertEqual(package_results, {"success", "skipped"})
        self.assertNotIn("failure", condition)
        self.assertNotRegex(
            condition,
            r"needs\.build-client-packages\.result\s*!=",
        )

    def test_managed_package_job_and_seal_gates_fail_closed(self) -> None:
        text = workflow_text()
        canonical = job_block_from(text, "stage-canonical-linux")
        create_universal = job_block_from(text, "create-universal-macos")
        sign_and_notarize = job_block_from(text, "sign-and-notarize-macos")
        packages = job_block_from(text, "build-client-packages")
        seal = job_block_from(text, "seal-release-stage")

        self.assertIn("needs: [preflight, build-binaries]", canonical)
        self.assertIn("needs.build-binaries.result == 'success'", canonical)
        self.assertIn("needs: [preflight, build-binaries]", create_universal)
        self.assertIn("needs.build-binaries.result == 'success'", create_universal)
        self.assertIn(
            "needs: [preflight, build-binaries, create-universal-macos]",
            sign_and_notarize,
        )
        self.assertIn("needs.create-universal-macos.result == 'success'", sign_and_notarize)

        self.assertIn("needs: [preflight, stage-canonical-linux]", packages)
        self.assertIn("needs.stage-canonical-linux.result == 'success'", packages)
        self.assertIn("needs.preflight.outputs.stable_version == 'true'", packages)

        self.assertIn("build-client-packages", seal.split("    if:", 1)[0])
        self.assertIn("needs.stage-canonical-linux.result == 'success'", seal)
        self.assertIn("needs.build-client-packages.result == 'success'", seal)
        self.assertIn("needs.build-client-packages.result == 'skipped'", seal)
        self.assertIn("needs.sign-and-notarize-macos.result == 'success'", seal)
        self.assert_seal_package_result_contract(text)

        mutations = {
            "allow-failure": text.replace(
                "needs.build-client-packages.result == 'skipped')",
                "needs.build-client-packages.result == 'skipped' || "
                "needs.build-client-packages.result == 'failure')",
                1,
            ),
            "invert-success": text.replace(
                "needs.build-client-packages.result == 'success'",
                "needs.build-client-packages.result != 'success'",
                1,
            ),
            "delete-skipped": text.replace(
                " || needs.build-client-packages.result == 'skipped'", "", 1
            ),
        }
        for name, mutant in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, text)
                with self.assertRaises(AssertionError):
                    self.assert_seal_package_result_contract(mutant)

    def assert_lipo_verify_arch_order(self, block: str) -> None:
        # Xcode 16.4 lipo requires the input file before the -verify_arch
        # command and its architecture flags: the legacy order
        # `lipo -verify_arch x86_64 arm64 FILE` parses FILE as an
        # architecture and fails the universal macOS step. The only safe
        # order is `lipo FILE -verify_arch x86_64 arm64`.
        self.assertIn(
            'lipo "universal/${binary}-universal-apple-darwin" -verify_arch x86_64 arm64',
            block,
        )
        self.assertNotIn("lipo -verify_arch x86_64 arm64", block)

    def test_universal_lipo_verify_arch_uses_safe_argument_order(self) -> None:
        block = job_block("create-universal-macos")
        self.assert_lipo_verify_arch_order(block)

        mutations = {
            "flags-first": block.replace(
                'lipo "universal/${binary}-universal-apple-darwin" -verify_arch x86_64 arm64',
                'lipo -verify_arch x86_64 arm64 "universal/${binary}-universal-apple-darwin"',
                1,
            ),
            "verify-dropped": block.replace(" -verify_arch x86_64 arm64", "", 1),
        }
        for name, mutant in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, block)
                with self.assertRaises(AssertionError):
                    self.assert_lipo_verify_arch_order(mutant)

    def test_build_client_packages_matrix_is_two_musl_targets_only(self) -> None:
        block = job_block("build-client-packages")

        actual_targets = set(re.findall(r"- target: ([^\n]+)\n", block))
        self.assertEqual(
            actual_targets,
            {"x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"},
        )
        self.assertIn("fail-fast: false", block)
        self.assertNotIn("x86_64-unknown-linux-gnu", block)
        self.assertNotIn("apple-darwin", block)
        self.assertNotIn("windows", block)

    def assert_native_package_runner_contract(self, text: str) -> None:
        block = job_block_from(text, "build-client-packages")
        expected_entries = (
            "- target: x86_64-unknown-linux-musl\n"
            "            runner: ubuntu-22.04\n"
            "            runner_arch: X64\n"
            "            nfpm_arch: x86_64\n"
            f"            nfpm_archive_sha256: {NFPM_X86_ARCHIVE_SHA256}\n"
            f"            nfpm_binary_sha256: {NFPM_X86_BINARY_SHA256}",
            "- target: aarch64-unknown-linux-musl\n"
            "            runner: ubuntu-22.04-arm\n"
            "            runner_arch: ARM64\n"
            "            nfpm_arch: arm64\n"
            f"            nfpm_archive_sha256: {NFPM_ARM_ARCHIVE_SHA256}\n"
            f"            nfpm_binary_sha256: {NFPM_ARM_BINARY_SHA256}",
        )
        for entry in expected_entries:
            self.assertIn(entry, block)
        self.assertIn("runs-on: ${{ matrix.runner }}", block)
        self.assertEqual(block.count('REQUIRE_INSTALL: "1"'), 2)
        self.assertEqual(
            block.count("EXPECTED_RUNNER_ARCH: ${{ matrix.runner_arch }}"), 2
        )
        self.assertEqual(
            block.count('test "${{ runner.arch }}" = "$EXPECTED_RUNNER_ARCH"'), 2
        )
        self.assertNotIn('REQUIRE_INSTALL: "0"', block)
        self.assertNotIn("&& '1' || '0'", block)

    def test_each_musl_package_target_runs_a_real_native_lifecycle(self) -> None:
        text = workflow_text()
        self.assert_native_package_runner_contract(text)
        mutations = {
            "arm-on-x86-runner": text.replace("runner: ubuntu-22.04-arm", "runner: ubuntu-22.04", 1),
            "arm-require-install-zero": text.replace('REQUIRE_INSTALL: "1"', 'REQUIRE_INSTALL: "0"', 2),
            "remove-runner-arch-proof": text.replace(
                '          test "${{ runner.arch }}" = "$EXPECTED_RUNNER_ARCH"\n',
                "",
                1,
            ),
            "remove-arm-target": text.replace(
                "          - target: aarch64-unknown-linux-musl\n",
                "          - target: removed-aarch64-target\n",
                1,
            ),
        }
        for name, mutant in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, text)
                with self.assertRaises(AssertionError):
                    self.assert_native_package_runner_contract(mutant)

    def test_build_client_packages_pins_and_verifies_nfpm_before_use(self) -> None:
        block = job_block("build-client-packages")

        self.assertIn("NFPM_VERSION: 2.47.0", block)
        self.assertIn(
            "NFPM_ARCHIVE_SHA256: ${{ matrix.nfpm_archive_sha256 }}",
            block,
        )
        self.assertIn(
            "NFPM_BINARY_SHA256: ${{ matrix.nfpm_binary_sha256 }}",
            block,
        )
        self.assertIn("NFPM_ARCH: ${{ matrix.nfpm_arch }}", block)
        self.assertIn("sha256sum -c -", block)
        self.assertIn(".github/scripts/nfpm/verify-nfpm.sh", block)
        install_index = block.index("Install pinned nFPM")
        native_index = block.index("Run native managed-package lifecycle gate")
        build_index = block.index("Build managed DEB/RPM packages")
        self.assertLess(install_index, native_index)
        self.assertLess(native_index, build_index)

    def test_build_client_packages_runs_native_lifecycle_gate_with_arch_aware_require_install(
        self,
    ) -> None:
        block = job_block("build-client-packages")

        self.assertIn(".github/scripts/nfpm/tests/test_client_nfpm_native.sh", block)
        self.assertIn('REQUIRE_INSTALL: "1"', block)
        self.assertIn("EXPECTED_RUNNER_ARCH: ${{ matrix.runner_arch }}", block)
        self.assertIn('test "${{ runner.arch }}" = "$EXPECTED_RUNNER_ARCH"', block)

    def test_packages_consume_hashed_canonical_bytes_without_rebuild_or_strip(self) -> None:
        canonical = job_block("stage-canonical-linux")
        block = job_block("build-client-packages")

        self.assertIn(
            "scripts/stage-canonical-linux.py raw canonical-binaries BINARY_SHA256SUMS",
            canonical,
        )
        self.assertIn("name: canonical-linux-binaries-", canonical)
        self.assertIn("name: canonical-linux-binaries-", block)
        self.assertIn("sha256sum -c ../BINARY_SHA256SUMS", block)
        self.assertIn(
            'AGENT_BIN="canonical-stage/canonical-binaries/terraphim-agent-${TARGET}"',
            block,
        )
        self.assertIn(
            'GREP_BIN="canonical-stage/canonical-binaries/terraphim-grep-${TARGET}"',
            block,
        )
        self.assertIn(".github/scripts/nfpm/build-client-packages.sh", block)
        self.assertIn("--agent-binary \"$AGENT_BIN\"", block)
        self.assertIn("--grep-binary \"$GREP_BIN\"", block)
        self.assertIn('--out-dir "client-managed-packages/${TARGET}"', block)
        self.assertNotIn("cargo build", block)
        self.assertNotIn("cargo deb", block)
        self.assertNotRegex(block, r"(?m)^\s+strip(?:\s|$)")

    def assert_package_epoch_contract(self, text: str) -> None:
        block = job_block_from(text, "build-client-packages")
        header = block[: block.index("    strategy:")]
        self.assertIn(
            "    env:\n"
            "      SOURCE_DATE_EPOCH: ${{ needs.preflight.outputs.source_date_epoch }}\n",
            header,
        )
        self.assertNotIn("SOURCE_DATE_EPOCH: 1700000000", block)
        self.assertNotIn("SOURCE_DATE_EPOCH: ${{ needs.preflight.outputs.source_sha }}", block)

    def test_package_builds_inherit_the_exact_preflight_source_date_epoch(self) -> None:
        text = workflow_text()
        self.assert_package_epoch_contract(text)
        mutations = {
            "delete-epoch": text.replace(
                "    env:\n"
                "      SOURCE_DATE_EPOCH: ${{ needs.preflight.outputs.source_date_epoch }}\n",
                "",
                1,
            ),
            "bind-epoch-to-source-sha": text.replace(
                "SOURCE_DATE_EPOCH: ${{ needs.preflight.outputs.source_date_epoch }}",
                "SOURCE_DATE_EPOCH: ${{ needs.preflight.outputs.source_sha }}",
                1,
            ),
            "hardcode-epoch": text.replace(
                "SOURCE_DATE_EPOCH: ${{ needs.preflight.outputs.source_date_epoch }}",
                "SOURCE_DATE_EPOCH: 1700000000",
                1,
            ),
        }
        for name, mutant in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, text)
                with self.assertRaises(AssertionError):
                    self.assert_package_epoch_contract(mutant)

    def test_build_client_packages_runs_actual_package_lifecycle_gate_after_build(self) -> None:
        # The synthetic-fixture native gate proves the packaging mechanism;
        # this gate additionally installs the REAL DEB/RPM produced from the
        # real qualified binaries (for both terraphim-agent and
        # terraphim-grep) and must run only after those real packages exist.
        block = job_block("build-client-packages")

        self.assertIn(".github/scripts/nfpm/tests/test_client_nfpm_native_actual.sh", block)
        self.assertIn("PACKAGE_DIR: client-managed-packages/${{ matrix.target }}", block)
        self.assertIn(
            "AGENT_BINARY: canonical-stage/canonical-binaries/terraphim-agent-${{ matrix.target }}",
            block,
        )
        self.assertIn(
            "GREP_BINARY: canonical-stage/canonical-binaries/terraphim-grep-${{ matrix.target }}",
            block,
        )
        self.assertIn('REQUIRE_INSTALL: "1"', block)
        self.assertIn("EXPECTED_RUNNER_ARCH: ${{ matrix.runner_arch }}", block)
        self.assertIn('test "${{ runner.arch }}" = "$EXPECTED_RUNNER_ARCH"', block)

        build_index = block.index("Build managed DEB/RPM packages")
        actual_gate_index = block.index("test_client_nfpm_native_actual.sh")
        upload_index = block.index("actions/upload-artifact@")
        self.assertLess(build_index, actual_gate_index)
        self.assertLess(actual_gate_index, upload_index)

        lifecycle_text = NATIVE_ACTUAL_LIFECYCLE.read_text()
        self.assertIn("both native hosted runner legs", lifecycle_text)
        self.assertIn("x86_64 and", lifecycle_text)
        self.assertIn("aarch64 MUSL", lifecycle_text)
        self.assertNotIn("only for the x86_64 MUSL leg", lifecycle_text)

    def test_managed_inventory_is_version_bound_and_separate_from_20_archives(self) -> None:
        stage = job_block("seal-release-stage")
        assemble = stage.index("assemble-client-release-inventory.sh")
        checksum = stage.index("../SHA256SUMS", assemble)

        self.assertIn("--output managed-release-assets", stage)
        self.assertIn('--expected-version "$VERSION"', stage)
        self.assertIn("--managed-staging client-managed-staging", stage)
        self.assertIn("--managed-target x86_64-unknown-linux-musl", stage)
        self.assertIn("--managed-target aarch64-unknown-linux-musl", stage)
        self.assertIn("managed-release-assets/*", stage)
        self.assertLess(assemble, checksum)
        self.assertIn('test "$(wc -l < SHA256SUMS | tr -d \' \')" = 20', stage)

    def test_checksum_sealing_and_credential_export_are_word_split_safe(self) -> None:
        """ShellCheck SC2163/SC2046 hardening is contract, not decoration.

        GitHub-hosted runners ship shellcheck, so actionlint's embedded-script
        pass fails the workflow contract on the unsafe forms. These textual
        assertions keep the safe forms required even where a local actionlint
        runs without shellcheck (the unsafe forms then pass lint vacuously).

        - SC2163: `export "${name?}"` is the ShellCheck-approved dynamic
          export -- it exports the variable *named by* `name` and fails
          closed if the credential name is unset or null. The masked 1Password
          loading (`::add-mask::` + `printf -v`) is unchanged.
        - SC2046: SHA256SUMS is sealed from a NUL-delimited `find -printf
          '%f\\0' | sort -z` pipeline into an array, checksummed by a single
          `sha256sum "${sealed_assets[@]}"` invocation (exact bare-filename
          output format preserved for `sha256sum -c` and promote-release.sh),
          with an explicit emptiness guard so a zero-asset stage fails closed
          instead of reading stdin.
        """
        text = workflow_text()
        signing = job_block("sign-and-notarize-macos")
        self.assertIn('export "${name?}"', signing)
        self.assertNotIn('export "$name"', signing)
        stage = job_block("seal-release-stage")
        self.assertIn("mapfile -d '' sealed_assets", stage)
        self.assertIn("-printf '%f\\0'", stage)
        self.assertIn("LC_ALL=C sort -z", stage)
        self.assertIn('LC_ALL=C sha256sum "${sealed_assets[@]}"', stage)
        self.assertIn('[ "${#sealed_assets[@]}" -gt 0 ]', stage)
        self.assertNotIn("sha256sum $(LC_ALL=C find", stage)

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
            "stage-canonical-linux",
            "build-client-packages",
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
            "managed-release-assets/*",
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
        for job in (
            "preflight",
            "build-binaries",
            "stage-canonical-linux",
            "build-client-packages",
            "create-universal-macos",
            "sign-and-notarize-macos",
            "seal-release-stage",
        ):
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

    def assert_package_and_staging_jobs_are_secret_free(self, text: str) -> None:
        for job in ("stage-canonical-linux", "build-client-packages"):
            block = job_block_from(text, job)
            self.assertNotIn("secrets.", block, job)
            for secret_name in (
                "ZIPSIGN_PRIVATE_KEY",
                "OP_SERVICE_ACCOUNT_TOKEN",
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
            ):
                self.assertNotIn(secret_name, block, job)

    def test_package_and_canonical_staging_jobs_reject_secret_injection(self) -> None:
        text = workflow_text()
        self.assert_package_and_staging_jobs_are_secret_free(text)
        secret_names = (
            "ZIPSIGN_PRIVATE_KEY",
            "OP_SERVICE_ACCOUNT_TOKEN",
            "AWS_ACCESS_KEY_ID",
        )
        for job in ("stage-canonical-linux", "build-client-packages"):
            marker = f"  {job}:\n"
            for secret_name in secret_names:
                with self.subTest(job=job, secret=secret_name):
                    mutant = text.replace(
                        marker,
                        marker
                        + "    env:\n"
                        + f"      {secret_name}: ${{{{ secrets.{secret_name} }}}}\n",
                        1,
                    )
                    self.assertNotEqual(mutant, text)
                    with self.assertRaises(AssertionError):
                        self.assert_package_and_staging_jobs_are_secret_free(mutant)

    def assert_assembler_output_documentation(self, text: str) -> None:
        normalized = re.sub(r"\s+", " ", text)
        self.assertIn("dedicated, initially-empty managed-package destination", normalized)
        self.assertIn("pre-existing destination entries", normalized)
        self.assertIn("none in the release workflow", normalized)
        self.assertIn("entries across both managed targets", normalized)
        self.assertIn("generated stage-only marker", normalized)
        self.assertNotIn("hold the merged binary artifacts", normalized)
        self.assertNotIn("merged binary inventory", normalized)

    def test_assembler_documents_the_separate_initially_empty_destination(self) -> None:
        text = ASSEMBLE_CLIENT_INVENTORY.read_text()
        self.assert_assembler_output_documentation(text)
        mutations = {
            "restore-false-binary-inventory-description": text.replace(
                "dedicated, initially-empty managed-package destination",
                "directory that must hold the merged binary artifacts",
                1,
            ),
            "drop-duplicate-scope": text.replace(
                "entries across both managed targets", "managed entries", 1
            ),
        }
        for name, mutant in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, text)
                with self.assertRaises(AssertionError):
                    self.assert_assembler_output_documentation(mutant)

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

        canonical_stage = job_block("stage-canonical-linux")
        canonical = canonical_stage.index("Stage and hash canonical Linux package bytes")
        binary_sums = canonical_stage.index("BINARY_SHA256SUMS", canonical)
        upload = canonical_stage.index("actions/upload-artifact@", binary_sums)
        self.assertLess(canonical, binary_sums)
        self.assertLess(binary_sums, upload)
        self.assertIn(
            "scripts/stage-canonical-linux.py raw canonical-binaries BINARY_SHA256SUMS",
            canonical_stage,
        )

        stage = job_block("seal-release-stage")
        receipt = stage.index("Verify canonical Linux binary receipt")
        archive = stage.index("Create deterministic archives", receipt)
        self.assertLess(receipt, archive)
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

    def test_managed_packages_are_unsigned_stage_only_pending_3336(self) -> None:
        # The DEB/RPM managed packages carry no package-manager signature and
        # no centralized provenance attestation (#3336). sign-release-archives
        # (an unmodified, previously-reviewed script) signs *.tar.gz only, so
        # this asserts the boundary stays machine-enforced at the only two
        # places that could silently widen it: no signing tool is ever invoked
        # against a .deb/.rpm and no public package repository writer exists.
        text = workflow_text()

        forbidden_signing_tools = (
            "debsigs",
            "dpkg-sig",
            "rpmsign",
            "rpm --addsign",
            "rpm --resign",
        )
        for tool in forbidden_signing_tools:
            self.assertNotIn(
                tool,
                text,
                f"DEB/RPM packages must remain unsigned pending #3336: found {tool!r}",
            )

        forbidden_repo_publishers = ("reprepro", "createrepo", "aptly", "apt-ftparchive")
        for tool in forbidden_repo_publishers:
            self.assertNotIn(
                tool,
                text,
                f"no public apt/dnf repository publication is implemented (#3336): found {tool!r}",
            )

        self.assertNotIn("contents: write", text)
        self.assertNotIn("upload-to-target-release:", text)
        self.assertIn("--output managed-release-assets", job_block("seal-release-stage"))

        assemble_script = (
            ROOT / ".github" / "scripts" / "release" / "assemble-client-release-inventory.sh"
        ).read_text()
        self.assertIn("UNSIGNED-STAGE-ONLY", assemble_script)
        self.assertIn("https://github.com/terraphim/terraphim-ai/issues/3336", assemble_script)


if __name__ == "__main__":
    unittest.main()
