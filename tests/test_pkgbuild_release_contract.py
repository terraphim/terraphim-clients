"""Contract tests for the canonical Arch/AUR split PKGBUILD.

Covers Gitea terraphim-clients#316: one 'pkgbase=terraphim-clients-bin'
producing the 'terraphim-agent-bin' and 'terraphim-grep-bin' split outputs
from immutable public GitHub release assets, with per-output ownership of
exactly one binary, one license text and one pacman receipt.

The real makepkg validation ('.SRCINFO' drift, 'namcap', install,
'pacman -Qkk', smoke, removal) lives in
'pkgbuilds/terraphim-clients-bin/verify.sh' and runs in an Arch container;
the '.SRCINFO' drift assertion here is skipped when 'makepkg' is absent.
"""

import os
import re
import shutil
import subprocess
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PKGDIR = ROOT / "pkgbuilds" / "terraphim-clients-bin"
PKGBUILD = PKGDIR / "PKGBUILD"
SRCINFO = PKGDIR / ".SRCINFO"
CARGO = ROOT / "Cargo.toml"

D = chr(36)  # dollar sign, kept out of template literals in the test source

PKGVER = "1.21.16"
REPO = "terraphim/terraphim-clients"
RELEASE_BASE = f"https://github.com/{REPO}/releases/download/v{PKGVER}"

# Authoritative SHA-256 values from the release's SHA256SUMS asset.
ASSETS = {
    "x86_64": {
        "terraphim-agent": (
            f"terraphim-agent-{PKGVER}-x86_64-unknown-linux-musl.tar.gz",
            "71cb87455959f2eaa04fab1bd2655564635df58988dfae306c4aeb5583938151",
        ),
        "terraphim-grep": (
            f"terraphim-grep-{PKGVER}-x86_64-unknown-linux-musl.tar.gz",
            "872a7d7fe1167c75340807be0e3a0bb079389e28d813eb4e198b3d1757e7a583",
        ),
    },
    "aarch64": {
        "terraphim-agent": (
            f"terraphim-agent-{PKGVER}-aarch64-unknown-linux-musl.tar.gz",
            "f8530d963171e3f72b52b8b729653cafa5ae5e105e219f7ecec7b170037acaa7",
        ),
        "terraphim-grep": (
            f"terraphim-grep-{PKGVER}-aarch64-unknown-linux-musl.tar.gz",
            "194d4cac04eccbfb998c4f060e44b286873510d354fd3a3a160bf63b2c5e025c",
        ),
    },
}

# Split-output contract: package -> (binary, license basename, SPDX)
OUTPUTS = {
    "terraphim-agent-bin": ("terraphim-agent", "LICENSE-Apache-2.0", "Apache-2.0"),
    "terraphim-grep-bin": ("terraphim-grep", "LICENSE-MIT", "MIT"),
}

STRIP_COMMENT = re.compile(r"(?m)^\s*#.*$")


def pkgbuild_text() -> str:
    return PKGBUILD.read_text()


def code_only() -> str:
    """PKGBUILD text with comments removed (comments describe, not define)."""
    return STRIP_COMMENT.sub("", pkgbuild_text())


def scalar(name: str, text: str | None = None) -> str | None:
    body = text if text is not None else code_only()
    match = re.search(rf"(?m)^{re.escape(name)}=([^\s(][^\n]*)$", body)
    if not match:
        return None
    return match.group(1).strip().strip("'\"")


def array(name: str, text: str | None = None) -> list[str]:
    body = text if text is not None else code_only()
    match = re.search(rf"(?m)^{re.escape(name)}=\(", body)
    if not match:
        return []
    index = match.end()
    depth = 1
    while index < len(body) and depth:
        if body[index] == "(":
            depth += 1
        elif body[index] == ")":
            depth -= 1
        index += 1
    inner = body[match.end() : index - 1]
    raw_items = re.findall(r"'([^']*)'|\"([^\"]*)\"|([^\s()]+)", inner)
    return [a or b or c for a, b, c in raw_items]


def expand(value: str) -> str:
    return (
        value.replace(D + "{_repo}", REPO)
        .replace(D + "{pkgver}", PKGVER)
        .replace(D + "{_release}", RELEASE_BASE)
    )


def package_function(name: str) -> str:
    pattern = "(?ms)^package_" + re.escape(name) + r"\(\) \{(.*?)^\}"
    match = re.search(pattern, code_only())
    return match.group(1) if match else ""


def parse_srcinfo() -> tuple[dict[str, list[str]], dict[str, dict[str, list[str]]]]:
    base: dict[str, list[str]] = {}
    packages: dict[str, dict[str, list[str]]] = {}
    current = base
    for raw in SRCINFO.read_text().splitlines():
        if not raw.strip():
            continue
        if raw.startswith("pkgbase = "):
            current = base
            base.setdefault("pkgbase", []).append(raw.split("=", 1)[1].strip())
            continue
        if raw.startswith("pkgname = "):
            name = raw.split("=", 1)[1].strip()
            current = packages.setdefault(name, {})
            current.setdefault("pkgname", []).append(name)
            continue
        if "=" in raw:
            key, value = raw.strip().split("=", 1)
            current.setdefault(key.strip(), []).append(value.strip())
    return base, packages


class PkgbuildContractTests(unittest.TestCase):
    def test_pkgbase_identity_and_version(self):
        self.assertEqual(scalar("pkgbase"), "terraphim-clients-bin")
        self.assertEqual(array("pkgname"), ["terraphim-agent-bin", "terraphim-grep-bin"])
        self.assertEqual(scalar("pkgver"), PKGVER)
        self.assertEqual(scalar("pkgrel"), "1")
        with CARGO.open("rb") as handle:
            workspace = tomllib.load(handle)["workspace"]["package"]["version"]
        self.assertEqual(
            PKGVER,
            workspace,
            "pkgver must track the workspace version so the package cannot lag a release",
        )

    def test_architectures_and_options(self):
        self.assertEqual(array("arch"), ["x86_64", "aarch64"])
        options = array("options")
        self.assertIn("!strip", options)
        self.assertIn("!debug", options)

    def test_sources_are_immutable_public_github_assets(self):
        for arch, expected in ASSETS.items():
            entries = array(f"source_{arch}")
            self.assertEqual(
                len(entries), 2, f"source_{arch} must carry exactly the two client archives"
            )
            resolved = {}
            for entry in entries:
                alias, _, url = entry.partition("::")
                resolved[expand(alias)] = expand(url)
            for binary, (alias, _sha) in expected.items():
                self.assertIn(alias, resolved, f"{arch}: missing source alias {alias}")
                self.assertEqual(
                    resolved[alias],
                    f"{RELEASE_BASE}/{alias}",
                    f"{arch}: {binary} must come from the immutable release URL",
                )

    def test_digests_match_the_release_sha256sums(self):
        for arch, expected in ASSETS.items():
            sums = array(f"sha256sums_{arch}")
            self.assertEqual(len(sums), 2)
            for index, (binary, (_alias, sha)) in enumerate(expected.items()):
                self.assertEqual(
                    sums[index],
                    sha,
                    f"{arch}/{binary}: digest drifted from the release SHA256SUMS",
                )
                self.assertRegex(sha, r"^[0-9a-f]{64}$")

    def test_each_output_owns_only_its_binary_license_and_receipt(self):
        code = code_only()
        license_prefix = D + "{pkgname}/"
        for name, (binary, license_file, spdx) in OUTPUTS.items():
            with self.subTest(package=name):
                body = package_function(name)
                self.assertTrue(body, f"missing package function for {name}")
                self.assertIn(f"provides=('{binary}')", body)
                self.assertIn(f"conflicts=('{binary}')", body)
                self.assertIn(f"license=('{spdx}')", body)
                self.assertIn(f"/usr/bin/{binary}", body)
                self.assertIn(
                    "/usr/share/licenses/" + license_prefix + license_file, body
                )
                self.assertIn(
                    f"package-manager.d/{binary}",
                    body,
                    "each output must drop a per-binary pacman receipt",
                )
                self.assertIn("pacman", body)
                other = next(b for _n, (b, _l, _s) in OUTPUTS.items() if b != binary)
                self.assertNotIn(
                    f"/usr/bin/{other}",
                    body,
                    "split outputs must not cross-install each other's executable",
                )
        self.assertNotIn("build()", code, "prebuilt archives must not be compiled")

    def test_srcinfo_matches_pkgbuild(self):
        self.assertTrue(SRCINFO.exists(), ".SRCINFO must be committed")
        base, packages = parse_srcinfo()
        self.assertEqual(base["pkgbase"], ["terraphim-clients-bin"])
        self.assertEqual(base["pkgver"], [PKGVER])
        self.assertEqual(base["pkgrel"], ["1"])
        self.assertEqual(sorted(base["arch"]), ["aarch64", "x86_64"])
        self.assertEqual(sorted(packages), sorted(OUTPUTS))
        for name, (_binary, _license_file, spdx) in OUTPUTS.items():
            self.assertEqual(packages[name]["license"], [spdx])
            self.assertEqual(packages[name]["provides"], [name.replace("-bin", "")])
            self.assertEqual(packages[name]["conflicts"], [name.replace("-bin", "")])
        for arch in ASSETS:
            self.assertEqual(len(base[f"source_{arch}"]), 2)
            self.assertEqual(len(base[f"sha256sums_{arch}"]), 2)

    @unittest.skipIf(os.geteuid() == 0, "makepkg refuses to run as root; verify.sh checks drift as the builder user")
    @unittest.skipUnless(shutil.which("makepkg"), "makepkg is only available on Arch")
    def test_srcinfo_is_not_stale(self):
        generated = subprocess.run(
            ["makepkg", "--printsrcinfo"],
            cwd=PKGDIR,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        self.assertEqual(
            generated,
            SRCINFO.read_text(),
            "run 'makepkg --printsrcinfo > .SRCINFO' after editing PKGBUILD",
        )


if __name__ == "__main__":
    unittest.main()
