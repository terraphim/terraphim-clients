import hashlib
import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "release-mcp.yml"
RELEASE_BINARIES = ROOT / ".github" / "workflows" / "release-binaries.yml"
CHECK_TAG = ROOT / ".github" / "scripts" / "release-mcp" / "check-tag.py"
ASSEMBLE = ROOT / ".github" / "scripts" / "release-mcp" / "assemble-assets.sh"

ASSETS = [
    "terraphim_mcp_server-aarch64-apple-darwin",
    "terraphim_mcp_server-aarch64-unknown-linux-musl",
    "terraphim_mcp_server-universal-apple-darwin",
    "terraphim_mcp_server-x86_64-apple-darwin",
    "terraphim_mcp_server-x86_64-unknown-linux-gnu",
    "terraphim_mcp_server-x86_64-unknown-linux-musl",
]
LINUX = "[self-hosted, Linux, X64, release-linux]"
MACOS = "[self-hosted, macOS, ARM64, release-macos]"


def text() -> str:
    return WORKFLOW.read_text()


def job_block(name: str) -> str:
    body = text()
    start = body.index(f"\n  {name}:\n") + 1
    match = re.search(r"\n  [a-zA-Z0-9_-]+:\n", body[start + 1 :])
    return body[start:] if match is None else body[start : start + 1 + match.start()]


def run_assemble(input_dir: Path, output_dir: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["bash", str(ASSEMBLE), str(input_dir), str(output_dir)],
        text=True,
        capture_output=True,
    )


def stage_assets(root: Path, names: list[str]) -> Path:
    for index, name in enumerate(names):
        directory = root / f"mcp-binary-{name}"
        directory.mkdir(parents=True)
        (directory / name).write_bytes(f"binary-{index}-{name}".encode())
    return root


class ReleaseMcpWorkflow(unittest.TestCase):
    def test_triggers_are_the_narrow_tag_pattern_and_guarded_dispatch(self) -> None:
        body = text()
        self.assertRegex(body, r'tags:\n\s+- "terraphim_mcp_server-v\*"')
        self.assertRegex(
            body,
            r"workflow_dispatch:\n\s+inputs:\n\s+tag:\n(?:.*\n)*?\s+required: true"
            r"(?:.*\n)*?\s+dry_run:\n(?:.*\n)*?\s+default: true\n\s+type: boolean",
        )
        self.assertNotRegex(body, r"branches:")
        self.assertNotIn("pull_request", body)

    def test_triggers_do_not_collide_with_other_release_workflows(self) -> None:
        self.assertNotIn("terraphim_mcp_server-v", RELEASE_BINARIES.read_text())
        for other in (ROOT / ".github" / "workflows").glob("*.yml"):
            if other.name == "release-mcp.yml":
                continue
            for pattern in re.findall(r'tags:\s*\[([^\]]*)\]', other.read_text()):
                for tag in re.findall(r'"([^"]+)"', pattern):
                    self.assertFalse(
                        re.fullmatch(tag.replace("*", ".*"), "terraphim_mcp_server-v1.0.0"),
                        f"{other.name} tag {tag} would also fire for MCP tags",
                    )

    def test_cargo_jobs_run_only_on_self_hosted_runners(self) -> None:
        build = job_block("build")
        for lane in (LINUX, MACOS):
            self.assertIn(f"- os: {lane}", build)
        self.assertEqual(len(re.findall(r"- os: \[", build)), 5)
        self.assertIn("runs-on: ${{ matrix.os }}", build)
        self.assertNotIn("ubuntu", build)
        self.assertNotIn("macos-15", build)
        self.assertNotIn("pc-windows", text())
        self.assertNotIn("windows-", text())
        for job in ("preflight", "build", "universal-macos", "assemble"):
            block = job_block(job)
            if "cargo " in block and job != "preflight":
                self.assertTrue(
                    "self-hosted" in block or "matrix.os" in block, f"{job} runs cargo off-host"
                )
        self.assertIn(f"runs-on: {MACOS}", job_block("universal-macos"))

    def test_matrix_targets_and_toolchain_follow_release_binaries(self) -> None:
        build = job_block("build")
        expected = {
            (LINUX, "x86_64-unknown-linux-gnu", "false"),
            (LINUX, "x86_64-unknown-linux-musl", "true"),
            (LINUX, "aarch64-unknown-linux-musl", "true"),
            (MACOS, "x86_64-apple-darwin", "false"),
            (MACOS, "aarch64-apple-darwin", "false"),
        }
        actual = set(
            re.findall(
                r"- os: (\[[^\n]+\])\n\s+target: ([^\n]+)\n\s+use_cross: (true|false)", build
            )
        )
        self.assertEqual(actual, expected)
        reference = RELEASE_BINARIES.read_text()
        for pinned in (
            "rustup toolchain install 1.96.0",
            "--rev 88f49ff79e777bef6d3564531636ee4d3cc2f8d2",
        ):
            self.assertIn(pinned, build)
            self.assertIn(pinned, reference)
        self.assertIn('"${build[@]}" build --locked --release --target', build)
        self.assertIn("-p terraphim_mcp_server --bin terraphim_mcp_server", build)

    def test_assets_are_raw_binaries_with_exact_names(self) -> None:
        self.assertIn(
            'terraphim_mcp_server-${{ matrix.target }}"', job_block("build")
        )
        self.assertIn(
            "terraphim_mcp_server-universal-apple-darwin", job_block("universal-macos")
        )
        self.assertIn("lipo -create", job_block("universal-macos"))
        script = ASSEMBLE.read_text()
        for name in ASSETS:
            self.assertIn(name, script)

    def test_publish_is_an_isolated_draft_then_prerelease_and_dry_run_skips_it(self) -> None:
        publish = job_block("publish")
        self.assertIn("needs.preflight.outputs.dry_run != 'true'", publish)
        self.assertIn("gh release create", publish)
        self.assertIn("--draft", publish)
        self.assertIn("--prerelease", publish)
        self.assertIn('--title "$TAG"', publish)
        self.assertIn("--verify-tag", publish)
        self.assertIn("release/checksums.txt", publish)
        self.assertIn('--draft=false --prerelease', publish)
        self.assertLess(publish.index("--json assets"), publish.index("--draft=false"))
        # The write-scoped job checks out nothing and runs no repository script.
        self.assertNotIn("actions/checkout", publish)
        self.assertNotIn(".github/scripts", publish)
        self.assertEqual(
            len(re.findall(r"contents: write", text())), 1, "only publish may write"
        )
        self.assertIn("contents: write", publish)
        self.assertIn("contents: read", job_block("assemble"))
        self.assertNotIn("gh release", job_block("assemble"))

    def test_assembled_artefact_preserves_the_assets_directory(self) -> None:
        assemble = job_block("assemble")
        self.assertIn("path: release/\n", assemble)
        self.assertNotIn("release/assets/*", assemble)
        publish = job_block("publish")
        self.assertIn("test -d release/assets", publish)
        self.assertIn("test -s release/checksums.txt", publish)

    @staticmethod
    def assert_lipo_verification(block: str) -> None:
        # Newer lipo rejects several architectures in one -verify_arch and
        # wants the file first: verify each separately, then require exactly
        # the two slices.
        assert "for arch in x86_64 arm64; do" in block
        assert 'lipo "$out" -verify_arch "$arch"' in block
        assert 'lipo -archs "$out"' in block
        assert '= "arm64 x86_64 "' in block
        assert "-verify_arch x86_64 arm64" not in block
        assert "lipo -verify_arch" not in block

    def test_universal_binary_verifies_each_architecture_separately(self) -> None:
        block = job_block("universal-macos")
        self.assert_lipo_verification(block)
        safe = 'lipo "$out" -verify_arch "$arch"'
        mutations = {
            "flags-first": block.replace(safe, 'lipo -verify_arch "$arch" "$out"', 1),
            "multi-arch": block.replace(safe, 'lipo "$out" -verify_arch x86_64 arm64', 1),
            "verify-dropped": block.replace(safe, "true", 1),
            "archs-check-dropped": block.replace('= "arm64 x86_64 "', '= ""', 1),
        }
        for name, mutant in mutations.items():
            with self.subTest(mutation=name):
                self.assertNotEqual(mutant, block)
                with self.assertRaises(AssertionError):
                    self.assert_lipo_verification(mutant)

    def test_source_is_trusted_before_any_self_hosted_job(self) -> None:
        preflight = job_block("preflight")
        self.assertIn('git merge-base --is-ancestor "$SOURCE_SHA" origin/main', preflight)
        self.assertIn("gh release view", preflight)
        for job in ("build", "universal-macos"):
            self.assertIn("preflight", job_block(job).split("steps:")[0])

    def test_checkouts_do_not_persist_credentials(self) -> None:
        body = text()
        checkouts = body.count("actions/checkout@")
        self.assertEqual(body.count("persist-credentials: false"), checkouts)

    def test_untrusted_dispatch_inputs_never_reach_shell_inline(self) -> None:
        for match in re.finditer(r"run: \|\n((?:\s{10,}.*\n)+)", text()):
            self.assertNotIn("${{ inputs.", match.group(1))
            self.assertNotIn("${{ github.ref_name", match.group(1))

    def test_third_party_actions_are_pinned_by_sha(self) -> None:
        for use in re.findall(r"uses:\s+([^\s#]+)", text()):
            self.assertRegex(use, r"@[0-9a-f]{40}$", use)


class CheckTag(unittest.TestCase):
    def run_check(self, tag: str, crate_version: str = "1.21.18"):
        with tempfile.TemporaryDirectory() as directory:
            metadata = Path(directory) / "metadata.json"
            metadata.write_text(
                json.dumps({"packages": [{"name": "terraphim_mcp_server", "version": crate_version}]})
            )
            return subprocess.run(
                ["python3", str(CHECK_TAG), tag, str(metadata)], text=True, capture_output=True
            )

    def test_accepts_exact_and_prerelease_versions(self) -> None:
        exact = self.run_check("terraphim_mcp_server-v1.21.18")
        self.assertEqual(exact.returncode, 0, exact.stderr)
        self.assertIn("crate_version=1.21.18", exact.stdout)
        self.assertEqual(
            self.run_check("terraphim_mcp_server-v1.21.18-rc.0-x.1a").returncode, 0
        )
        rc = self.run_check("terraphim_mcp_server-v1.21.18-rc.1")
        self.assertEqual(rc.returncode, 0, rc.stderr)
        self.assertIn("version=1.21.18-rc.1", rc.stdout)

    def test_rejects_mismatched_or_malformed_tags(self) -> None:
        for tag in (
            "terraphim_mcp_server-v1.21.19",
            "terraphim_mcp_server-v1.21.1-rc.1",
            "terraphim_mcp_server-1.21.18",
            "v1.21.18",
            "terraphim_mcp_server-v1.21",
            "terraphim_mcp_server-v1.21.18-",
            "terraphim_mcp_server-v01.21.18",
            "terraphim_mcp_server-v1.21.18-alpha..1",
            "terraphim_mcp_server-v1.21.18-01",
            "terraphim_mcp_server-v1.21.18+build.1",
        ):
            with self.subTest(tag=tag):
                self.assertNotEqual(self.run_check(tag).returncode, 0)

    def test_rejects_missing_package(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            metadata = Path(directory) / "metadata.json"
            metadata.write_text(json.dumps({"packages": []}))
            result = subprocess.run(
                ["python3", str(CHECK_TAG), "terraphim_mcp_server-v1.0.0", str(metadata)],
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)


class AssembleAssets(unittest.TestCase):
    def test_checksums_cover_exactly_the_six_assets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stage_assets(root / "in", ASSETS)
            result = run_assemble(root / "in", root / "out")
            self.assertEqual(result.returncode, 0, result.stderr)
            lines = (root / "out" / "checksums.txt").read_text().splitlines()
            self.assertEqual(len(lines), 6)
            listed = {}
            for line in lines:
                digest, name = line.split("  ", 1)
                listed[name] = digest
            self.assertEqual(sorted(listed), sorted(ASSETS))
            self.assertNotIn("checksums.txt", listed)
            for name in ASSETS:
                data = (root / "out" / "assets" / name).read_bytes()
                self.assertEqual(listed[name], hashlib.sha256(data).hexdigest())
                self.assertEqual((root / "out" / "assets" / name).stat().st_mode & 0o777, 0o755)

    def test_fails_closed_on_missing_extra_or_empty_asset(self) -> None:
        cases = {
            "missing": ASSETS[:-1],
            "extra": ASSETS + ["terraphim_mcp_server-x86_64-pc-windows-msvc"],
        }
        for label, names in cases.items():
            with self.subTest(case=label), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                stage_assets(root / "in", names)
                self.assertNotEqual(run_assemble(root / "in", root / "out").returncode, 0)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stage_assets(root / "in", ASSETS)
            (root / "in" / f"mcp-binary-{ASSETS[0]}" / ASSETS[0]).write_bytes(b"")
            self.assertNotEqual(run_assemble(root / "in", root / "out").returncode, 0)


class ActionlintParses(unittest.TestCase):
    def test_workflow_is_parsed_by_actionlint(self) -> None:
        result = subprocess.run(
            ["actionlint", str(WORKFLOW)], cwd=ROOT, text=True, capture_output=True
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
