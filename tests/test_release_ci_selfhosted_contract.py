import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CI = ROOT / ".github" / "workflows" / "ci.yml"
RELEASE_BINARIES = ROOT / ".github" / "workflows" / "release-binaries.yml"


def text() -> str:
    return CI.read_text()


def job_block(name: str) -> str:
    body = text()
    start = body.index(f"\n  {name}:\n") + 1
    match = re.search(r"\n  [a-zA-Z0-9_-]+:\n", body[start + 1 :])
    return body[start:] if match is None else body[start : start + 1 + match.start()]


def condition(block: str) -> str:
    match = re.search(r"    if: >-\n((?:      .*\n)+)", block)
    assert match, "job has no multi-line if"
    return " ".join(line.strip() for line in match.group(1).splitlines())


class SelfHostedCi(unittest.TestCase):
    def test_build_runs_on_the_self_hosted_runner_and_packaging_stays_hosted(self) -> None:
        self.assertIn("runs-on: [self-hosted, Linux, X64, release-linux]", job_block("build"))
        self.assertIn("runs-on: ubuntu-latest", job_block("client-packaging-contracts"))

    def test_fork_guard_and_rejection_are_complementary(self) -> None:
        run = condition(job_block("build"))
        reject = condition(job_block("reject-fork-pr"))
        self.assertEqual(
            run,
            "github.event_name != 'pull_request' || "
            "github.event.pull_request.head.repo.full_name == github.repository",
        )
        self.assertEqual(
            reject,
            "github.event_name == 'pull_request' && "
            "github.event.pull_request.head.repo.full_name != github.repository",
        )

    def test_fork_rejection_fails_closed_without_code_or_privilege(self) -> None:
        block = job_block("reject-fork-pr")
        self.assertIn("runs-on: ubuntu-latest", block)
        self.assertIn("contents: read", block)
        self.assertIn("exit 1", block)
        self.assertNotIn("actions/checkout", block)
        self.assertNotIn("runs-on: [self-hosted", block)

    def test_cargo_state_is_per_run_and_cleaned_up(self) -> None:
        block = job_block("build")
        self.assertIn('CARGO_HOME=$cargo_home', block)
        self.assertIn('cargo_home="$RUNNER_TEMP/cargo-home"', block)
        self.assertIn("CARGO_TARGET_DIR=$RUNNER_TEMP/target", block)
        self.assertIn("sed '/rustc-wrapper/d'", block)
        self.assertIn('--root "$RUNNER_TEMP/tools"', block)
        self.assertIn('echo "$RUNNER_TEMP/tools/bin" >> "$GITHUB_PATH"', block)
        cleanup = block[block.index("Remove per-run state") :]
        self.assertIn("if: always()", cleanup)
        for path in ("cargo-home", "target", "tools"):
            self.assertIn(f'$RUNNER_TEMP/{path}', cleanup)
        # Isolation must precede anything that runs cargo.
        self.assertLess(block.index("- name: Isolate cargo state"), block.index("cargo install"))
        self.assertLess(block.index("- name: Isolate cargo state"), block.index("cargo clippy"))

    def test_toolchain_is_pinned_like_release_binaries(self) -> None:
        block = job_block("build")
        self.assertIn("RUSTUP_TOOLCHAIN: 1.96.0", block)
        self.assertIn("rustup toolchain install 1.96.0", block)
        self.assertIn("rustup toolchain install 1.96.0", RELEASE_BINARIES.read_text())

    def test_checkout_is_asserted_clean_before_and_after(self) -> None:
        block = job_block("build")
        self.assertEqual(block.count('test -z "$(git status --porcelain)"'), 2)
        self.assertIn("git diff --exit-code -- Cargo.toml Cargo.lock", block)
        self.assertLess(
            block.index("- name: Assert clean checkout"), block.index("- name: Isolate cargo state")
        )

    def test_no_cross_workflow_concurrency_group_on_matrix_jobs(self) -> None:
        # GitHub keeps one running and one pending job per group and cancels
        # other pending ones, so a shared group would cancel queued release
        # matrix lanes. Serialisation is replaced by per-run isolation.
        self.assertNotIn("release-linux-runner", RELEASE_BINARIES.read_text())
        self.assertNotIn("release-linux-runner", text())

    def test_registry_token_is_confined_to_the_fetch_step_and_the_rest_is_offline(self) -> None:
        block = job_block("build")
        # Exactly one use of the secret, inside the fetch step.
        self.assertEqual(block.count("secrets.CARGO_REGISTRIES_TERRAPHIM_TOKEN"), 1)
        fetch = block[block.index("- name: Fetch dependencies") :]
        fetch = fetch[: fetch.index("- name: Install pinned actionlint")]
        self.assertIn("secrets.CARGO_REGISTRIES_TERRAPHIM_TOKEN", fetch)
        self.assertIn("cargo fetch --locked", fetch)
        self.assertIn('echo "CARGO_NET_OFFLINE=true" >> "$GITHUB_ENV"', fetch)
        # The only steps before it that run cargo must not need the registry
        # (zipsign comes from crates.io), and nothing repository-controlled
        # runs before the fetch.
        for later in ("cargo clippy", "cargo build", "cargo test"):
            self.assertGreater(block.index(later), block.index("cargo fetch --locked"))


if __name__ == "__main__":
    unittest.main()
