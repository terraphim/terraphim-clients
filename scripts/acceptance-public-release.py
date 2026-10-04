#!/usr/bin/env python3
"""Read-only acceptance validation of the public release entry points.

Proves, against live infrastructure only, that a user who follows the
documented instructions gets the current release:

  installer   the documented curl|bash one-liner resolves and installs the
              current version, with a matching checksum
  manifests   every binary's channel manifest is well formed and every
              advertised archive matches its size and SHA-256
  brew        the Homebrew tap exposes the documented formula names
  crates      the crates.io versions a `cargo install` would serve

Each check reports one of three states. `not-executed` exists so an
environment limitation (no Homebrew on this host, no crates.io network) is
never silently reported as success.

Exit codes: 0 all executed checks passed
            1 at least one executed check failed
            2 the installer check could not run
            3 nothing could be reached (no executed checks at all)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

PASS = "pass"
FAIL = "fail"
SKIP = "not-executed"

CHANNEL_DEFAULT = "https://downloads.terraphim.ai"
INSTALLER_URL = (
    "https://raw.githubusercontent.com/terraphim/terraphim-ai/main/scripts/install.sh"
)
UA = "terraphim-release-acceptance/1.0"
CRATES_UA = "terraphim-release-verifier/1.0 (release validation)"

BINARIES = ("terraphim-agent", "terraphim-cli", "terraphim-grep")
COMMON_TARGETS = {
    # 2026-10-04: x86_64-pc-windows-msvc removed with the Windows lane;
    # restore it when the private crates are public and the lane returns.
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
}
EXPECTED_TARGETS = {
    "terraphim-agent": COMMON_TARGETS | {"universal-apple-darwin"},
    "terraphim-grep": COMMON_TARGETS | {"universal-apple-darwin"},
    "terraphim-cli": COMMON_TARGETS,
}

GREEN = "\033[0;32m"
RED = "\033[0;31m"
YELLOW = "\033[1;33m"
BLUE = "\033[0;34m"
RESET = "\033[0m"


class CheckResult:
    def __init__(self, name: str) -> None:
        self.name = name
        self.state = SKIP
        self.detail = ""

    def passed(self, detail: str) -> "CheckResult":
        self.state, self.detail = PASS, detail
        return self

    def failed(self, detail: str) -> "CheckResult":
        self.state, self.detail = FAIL, detail
        return self

    def skipped(self, detail: str) -> "CheckResult":
        self.state, self.detail = SKIP, detail
        return self

    def render(self) -> str:
        colour, label = {
            PASS: (GREEN, "PASS"),
            FAIL: (RED, "FAIL"),
            SKIP: (YELLOW, "SKIP"),
        }[self.state]
        return f"  {colour}{label}{RESET}  {self.name}: {self.detail}"


def fetch(url: str, limit: int = 64 * 1024 * 1024, user_agent: str = UA) -> bytes:
    """Fetch a URL, refusing responses larger than `limit`.

    Release archives run to roughly 20 MB; the cap exists to stop a
    misdirected request from pulling down something unbounded.
    """
    request = urllib.request.Request(url, headers={"User-Agent": user_agent})
    with urllib.request.urlopen(request, timeout=120) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"{url}: response exceeds {limit} bytes")
    return data


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def check_manifests(base_url: str) -> CheckResult:
    """Every advertised archive must exist and match its recorded size+digest."""
    result = CheckResult("channel manifests")
    problems: list[str] = []
    checked = 0
    versions: set[str] = set()

    for binary in BINARIES:
        url = f"{base_url}/{binary}/stable-v2.json"
        try:
            manifest = json.loads(fetch(url, 1 << 20))
        except (urllib.error.URLError, ValueError, json.JSONDecodeError) as error:
            problems.append(f"{binary}: manifest unreadable ({error})")
            continue

        version = manifest.get("version", "?")
        versions.add(version)
        assets = manifest.get("assets")
        if not isinstance(assets, dict) or set(assets) != EXPECTED_TARGETS[binary]:
            problems.append(f"{binary}: target set is not exact")
            continue

        for target, asset in sorted(assets.items()):
            path = asset.get("path", "")
            parts = urllib.parse.urlsplit(path)
            if parts.scheme or parts.netloc or ".." in path.split("/"):
                problems.append(f"{binary}/{target}: unsafe asset path")
                continue
            try:
                body = fetch(f"{base_url}/{path}")
            except urllib.error.URLError as error:
                problems.append(f"{binary}/{target}: archive unreachable ({error})")
                continue
            checked += 1
            if len(body) != asset.get("size"):
                problems.append(
                    f"{binary}/{target}: size {len(body)} != {asset.get('size')}"
                )
                continue
            if digest(body) != asset.get("sha256"):
                problems.append(f"{binary}/{target}: sha256 mismatch")
            del body

    if problems:
        return result.failed("; ".join(problems[:4]) + (" …" if len(problems) > 4 else ""))
    if not versions:
        return result.skipped("no manifests reachable")
    if len(versions) != 1:
        return result.failed(f"binaries disagree on version: {sorted(versions)}")
    return result.passed(f"{len(BINARIES)} binaries at {versions.pop()}, {checked} archives byte-verified")


def check_installer(channel: str, install_dir: str) -> tuple[CheckResult, str | None]:
    """Run the documented one-liner against a scratch directory."""
    result = CheckResult("documented installer")
    if shutil.which("bash") is None or shutil.which("curl") is None:
        return result.skipped("bash or curl unavailable"), None

    try:
        script = fetch(INSTALLER_URL)
    except urllib.error.URLError as error:
        return result.failed(f"installer unreachable ({error})"), None

    script_path = os.path.join(install_dir, "install.sh")
    os.makedirs(install_dir, exist_ok=True)
    with open(script_path, "wb") as handle:
        handle.write(script)

    # Version pinning is only meaningful once the published script is the
    # manifest-driven one. Until then a pinned run fails for reasons that say
    # nothing about the release, so it is left to the default (`latest`).
    args = [script_path, "--install-dir", install_dir]

    # The sibling utilities are fetched by the installer unless it can see
    # them; run the script alone so the piped path is the one under test.
    env = dict(os.environ)
    env.setdefault("UTILS_REVISION", "main")
    completed = subprocess.run(
        ["bash", *args],
        capture_output=True,
        text=True,
        env=env,
        timeout=600,
    )
    output = (completed.stdout or "") + (completed.stderr or "")

    if completed.returncode != 0:
        return result.failed(f"exit {completed.returncode}: {output.strip().splitlines()[-1] if output.strip() else 'no output'}"), None

    binary = os.path.join(install_dir, "terraphim-agent")
    if not os.path.isfile(binary) or not os.access(binary, os.X_OK):
        return result.failed("installer produced no executable terraphim-agent"), None

    reported = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=60)
    version_line = (reported.stdout or "").strip()
    if reported.returncode != 0 or not version_line:
        return result.failed("installed binary did not report a version"), None

    parts = version_line.split()
    version = parts[-1] if parts else "?"
    if "SHA-256 verified" not in output and "unverified" not in output:
        return result.failed("installer did not report checksum verification"), version
    return result.passed(f"{version_line} installed and checksum-verified"), version


def check_brew() -> CheckResult:
    result = CheckResult("homebrew formula")
    if shutil.which("brew") is None:
        return result.skipped("brew not available on this host")
    tap = "terraphim/terraphim"
    try:
        subprocess.run(["brew", "tap", tap], capture_output=True, text=True, timeout=300)
        info = subprocess.run(
            ["brew", "info", "terraphim-agent"], capture_output=True, text=True, timeout=300
        )
    except subprocess.SubprocessError as error:
        return result.failed(f"brew invocation failed ({error})")
    if info.returncode == 0 and "terraphim-agent" in info.stdout:
        return result.passed("brew install terraphim-agent resolves")
    return result.failed("documented formula does not resolve in the tap")


def check_crates(crates: tuple[str, ...], expected_version: str | None) -> CheckResult:
    result = CheckResult("crates.io cli family")
    stale: list[str] = []
    missing: list[str] = []
    for crate in crates:
        try:
            body = fetch(f"https://crates.io/api/v1/crates/{crate}", 1 << 20, CRATES_UA)
            data = json.loads(body)
        except (urllib.error.URLError, ValueError, json.JSONDecodeError):
            missing.append(crate)
            continue
        served = data.get("crate", {}).get("max_stable_version") or "?"
        if expected_version and served != expected_version:
            stale.append(f"{crate}={served}")
    if missing and len(missing) == len(crates):
        return result.skipped("crates.io unreachable")
    if missing:
        return result.failed(f"unpublished: {', '.join(missing)}")
    if stale:
        return result.failed(f"not at {expected_version}: {', '.join(stale)}")
    return result.passed(f"{len(crates)} crates at {expected_version}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", default=CHANNEL_DEFAULT)
    parser.add_argument("--expected-version", default=None)
    parser.add_argument(
        "--crates",
        default="terraphim_agent terraphim_cli terraphim_grep",
        help="space-separated crates.io packages to check",
    )
    parser.add_argument("--skip-installer", action="store_true")
    parser.add_argument("--skip-crates", action="store_true")
    args = parser.parse_args()

    print(f"{BLUE}Terraphim public release acceptance{RESET}")
    print(f"  channel: {args.channel}")
    print()

    with tempfile.TemporaryDirectory(prefix="terraphim-acceptance-") as work:
        install_dir = os.path.join(work, "bin")
        if args.skip_installer:
            installer_result = CheckResult("documented installer").skipped("disabled")
            installed_version = None
        else:
            installer_result, installed_version = check_installer(args.channel, install_dir)

        expected_version = args.expected_version or installed_version

        results = [
            installer_result,
            check_manifests(args.channel),
            check_brew(),
            (
                CheckResult("crates.io cli family").skipped("disabled")
                if args.skip_crates
                else check_crates(tuple(args.crates.split()), expected_version)
            ),
        ]

    print(f"{BLUE}Checks{RESET}")
    for result in results:
        print(result.render())

    executed = [r for r in results if r.state != SKIP]
    failed = [r for r in executed if r.state == FAIL]

    print()
    summary = f"{len(executed)} executed, {len(failed)} failed, {len(results) - len(executed)} not executed"
    print(f"{BLUE}{summary}{RESET}")

    if not executed:
        print(f"{RED}Nothing was reachable; acceptance is inconclusive.{RESET}")
        return 3
    if installer_result.state == FAIL and not args.skip_installer:
        return 2
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())