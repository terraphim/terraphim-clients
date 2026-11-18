"""Producer -> promoter -> reader round-trip for the published client inventory.

The release producer seals a stage of archives and candidate manifests. The
promoter publishes that stage and advances the stable pointers. The readers
(the R2 manifest health validator and the Rust strict manifest reader) must
accept exactly what the producer emitted, with no target invented and no
target missing.

This contract drives that whole chain with real fixture files: real tar.gz
archives in the producer's layout, the real producer scripts, the real
promoter, and the real readers over the promoted tree. Nothing reaches the
network. The three external command-line tools the promoter shells out to
(gh, wrangler, curl) are replaced by local executables that read and write a
scratch directory, which is the same hermetic substitution the promotion
contract uses. The fixtures carry no embedded signature: the zipsign signature
gate belongs to the producer's own stage, so what this contract proves is
that the promoter and the readers accept exactly the bytes the producer hands
over.

The published inventory has no Windows lane: five Unix targets for every
binary plus the universal Apple target for the two binaries that ship one.
Seventeen archives in total. No x86_64-pc-windows-msvc ZIP is produced,
advertised, promoted, or read, so a stage carrying one is rejected.
"""

import gzip
import hashlib
import io
import json
import os
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD_MANIFEST = ROOT / "scripts" / "build-manifest.sh"
BUILD_LEGACY = ROOT / "scripts" / "build-legacy-manifest.py"
PROMOTE = ROOT / "scripts" / "promote-release.sh"
VALIDATE_STAGE = ROOT / "scripts" / "validate-promotion-stage.py"
VALIDATE_R2 = ROOT / "scripts" / "validate-r2-manifests.py"

VERSION = "1.21.25"
SOURCE_SHA = "c" * 40
CORRELATION_ID = "terraphim-clients/release-1.21.25:342"
SOURCE_DATE_EPOCH = "1789689600"
ACTIVATION_VERSION = "1.21.15"

COMMON_TARGETS = (
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
)
INVENTORY = {
    "terraphim-agent": (*COMMON_TARGETS, "universal-apple-darwin"),
    "terraphim-cli": COMMON_TARGETS,
    "terraphim-grep": (*COMMON_TARGETS, "universal-apple-darwin"),
}
BINARIES = tuple(INVENTORY)
WINDOWS_TARGET = "x86_64-pc-windows-msvc"


def executable(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def write_archive(path: Path, binary: str, payload: bytes) -> None:
    """Write a deterministic tar.gz holding the binary and both licences."""
    entries = (
        (binary, payload, 0o755),
        ("LICENSE-Apache-2.0", b"Apache License 2.0\n", 0o644),
        ("LICENSE-MIT", b"MIT License\n", 0o644),
    )
    with path.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as bundle:
                for name, content, mode in entries:
                    info = tarfile.TarInfo(name)
                    info.size = len(content)
                    info.mode = mode
                    info.mtime = 0
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    bundle.addfile(info, io.BytesIO(content))


def build_release_assets(directory: Path) -> list[str]:
    """Create the producer's asset set and return the archive file names."""
    names: list[str] = []
    for binary in BINARIES:
        for target in INVENTORY[binary]:
            name = f"{binary}-{VERSION}-{target}.tar.gz"
            write_archive(
                directory / name,
                binary,
                f"sealed-payload-{binary}-{target}\n".encode(),
            )
            names.append(name)
    return names


def build_candidate_manifests(staged: Path) -> None:
    """Run the producer's manifest builders over the staged assets."""
    manifests = staged / "manifests"
    manifests.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment["SOURCE_DATE_EPOCH"] = SOURCE_DATE_EPOCH
    for binary in BINARIES:
        strict = manifests / f"{binary}.v2.candidate.json"
        built = subprocess.run(
            [
                str(BUILD_MANIFEST),
                VERSION,
                binary,
                str(staged / "release-assets"),
                str(strict),
            ],
            env=environment,
            text=True,
            capture_output=True,
        )
        assert built.returncode == 0, built.stderr
        derived = subprocess.run(
            [
                "python3",
                str(BUILD_LEGACY),
                str(strict),
                str(manifests / f"{binary}.v1.candidate.json"),
            ],
            text=True,
            capture_output=True,
        )
        assert derived.returncode == 0, derived.stderr


def seal_stage(root: Path) -> Path:
    """Assemble the stage exactly as the release producer seals it."""
    staged = root / f"client-release-stage-{VERSION}-{SOURCE_SHA}"
    assets = staged / "release-assets"
    assets.mkdir(parents=True)
    names = build_release_assets(assets)
    build_candidate_manifests(staged)
    rows = []
    for name in names:
        digest = hashlib.sha256((assets / name).read_bytes()).hexdigest()
        rows.append(f"{digest}  {name}\n")
    (staged / "SHA256SUMS").write_text("".join(sorted(rows)))
    (staged / "expected-assets.txt").write_text("".join(f"{name}\n" for name in sorted(names)))
    (staged / "provenance.json").write_text(
        json.dumps(
            {
                "archive_signatures": "embedded-zipsign-ed25519",
                "correlation_id": CORRELATION_ID,
                "release_tag": f"v{VERSION}",
                "source_sha": SOURCE_SHA,
                "stage_identity": f"client-release-stage-{VERSION}-{SOURCE_SHA}",
                "version": VERSION,
            },
            sort_keys=True,
        )
        + "\n"
    )
    return staged


def install_stub_clis(tools: Path, github: Path, objects: Path) -> None:
    """Install local stand-ins for the external CLIs the promoter invokes."""
    tools.mkdir()
    github.mkdir()
    objects.mkdir()
    executable(
        tools / "gh",
        r'''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
remote = pathlib.Path(os.environ["GITHUB_REMOTE"])
if args[:2] == ["release", "view"]:
    assets = [
        {"name": path.name, "size": path.stat().st_size, "state": "uploaded"}
        for path in sorted(remote.iterdir())
        if path.is_file()
    ]
    print(json.dumps({
        "assets": assets,
        "isDraft": False,
        "isPrerelease": False,
        "tagName": os.environ["RELEASE_TAG"],
    }))
elif args[:2] == ["release", "upload"]:
    for value in args[3:]:
        if value == "--repo":
            break
        source = pathlib.Path(value)
        shutil.copyfile(source, remote / source.name)
elif args[:2] == ["release", "download"]:
    pattern = args[args.index("--pattern") + 1]
    destination = pathlib.Path(args[args.index("--dir") + 1])
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(remote / pattern, destination / pattern)
else:
    sys.exit(2)
''',
    )
    executable(
        tools / "wrangler",
        r'''#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
if args[:3] not in (["r2", "object", "put"], ["r2", "object", "delete"]):
    sys.exit(2)
key = args[3].split("/", 1)[1]
remote = pathlib.Path(os.environ["R2_REMOTE"]) / key
if args[2] == "put":
    source = pathlib.Path(args[args.index("--file") + 1])
    remote.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, remote)
else:
    remote.unlink(missing_ok=True)
''',
    )
    executable(
        tools / "curl",
        r'''#!/usr/bin/env python3
import os, pathlib, shutil, sys, urllib.parse
args = sys.argv[1:]
url = next(value for value in args if value.startswith("http"))
key = urllib.parse.urlsplit(url).path.lstrip("/")
remote = pathlib.Path(os.environ["R2_REMOTE"]) / key
if "--output" in args and remote.is_file():
    destination = pathlib.Path(args[args.index("--output") + 1])
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(remote, destination)
print("200" if remote.is_file() else "404")
''',
    )


def promoter_environment(tools: Path, github: Path, objects: Path) -> dict[str, str]:
    environment = os.environ.copy()
    environment.update(
        {
            "PATH": f"{tools}:{environment['PATH']}",
            "BASE_URL": "https://downloads.invalid",
            "GITHUB_REMOTE": str(github),
            "R2_REMOTE": str(objects),
            "RELEASE_TAG": f"v{VERSION}",
        }
    )
    return environment


class ReleaseTargetInventoryContract(unittest.TestCase):
    def test_producer_stage_promotes_and_the_published_tree_is_healthy(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = seal_stage(root)
            tools = root / "tools"
            github = root / "github-remote"
            objects = root / "r2-remote"
            install_stub_clis(tools, github, objects)
            environment = promoter_environment(tools, github, objects)

            gated = subprocess.run(
                [
                    sys.executable,
                    str(VALIDATE_STAGE),
                    VERSION,
                    str(staged),
                    SOURCE_SHA,
                    CORRELATION_ID,
                    str(root / "r2-objects.tsv"),
                ],
                text=True,
                capture_output=True,
            )
            self.assertEqual(gated.returncode, 0, gated.stderr)
            plan = (root / "r2-objects.tsv").read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(plan), 23)

            promoted = subprocess.run(
                [
                    str(PROMOTE),
                    VERSION,
                    str(staged),
                    "terraphim-clients",
                    SOURCE_SHA,
                    CORRELATION_ID,
                ],
                env=environment,
                text=True,
                capture_output=True,
            )
            self.assertEqual(promoted.returncode, 0, promoted.stderr)

            for binary in BINARIES:
                strict = json.loads(
                    (objects / binary / "stable-v2.json").read_text(encoding="utf-8")
                )
                legacy = json.loads(
                    (objects / binary / "stable.json").read_text(encoding="utf-8")
                )
                self.assertEqual(set(strict["assets"]), set(INVENTORY[binary]))
                self.assertNotIn(WINDOWS_TARGET, strict["assets"])
                self.assertNotIn(WINDOWS_TARGET, legacy["assets"])
                self.assertEqual(set(legacy["assets"]), set(INVENTORY[binary]))
                for target, asset in strict["assets"].items():
                    published = objects / asset["path"]
                    self.assertTrue(published.is_file(), asset["path"])
                    payload = published.read_bytes()
                    self.assertEqual(len(payload), asset["size"])
                    self.assertEqual(hashlib.sha256(payload).hexdigest(), asset["sha256"])
                    self.assertEqual(legacy["assets"][target], asset["path"])

            healthy = subprocess.run(
                [
                    "python3",
                    str(VALIDATE_R2),
                    "--base-url",
                    objects.as_uri(),
                    "--activation-version",
                    ACTIVATION_VERSION,
                ],
                text=True,
                capture_output=True,
            )
            self.assertEqual(healthy.returncode, 0, healthy.stderr)
            self.assertIn("verified terraphim-agent", healthy.stdout)

    def test_the_promoted_inventory_is_exactly_seventeen_windows_free_assets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = seal_stage(root)
            assets = staged / "release-assets"
            names = sorted(path.name for path in assets.iterdir())
            self.assertEqual(len(names), 17)
            self.assertFalse([name for name in names if name.endswith(".zip")])
            self.assertFalse([name for name in names if WINDOWS_TARGET in name])

            for binary in BINARIES:
                strict = json.loads(
                    (staged / "manifests" / f"{binary}.v2.candidate.json").read_text(
                        encoding="utf-8"
                    )
                )
                self.assertEqual(set(strict["assets"]), set(INVENTORY[binary]))
                for target, asset in strict["assets"].items():
                    self.assertEqual(
                        asset["path"],
                        f"{binary}/{binary}-{VERSION}-{target}.tar.gz",
                    )

    def test_the_promoter_rejects_a_windows_asset_in_the_produced_stage(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = seal_stage(root)
            windows = (
                staged
                / "release-assets"
                / f"terraphim-agent-{VERSION}-{WINDOWS_TARGET}.zip"
            )
            write_archive(windows, "terraphim-agent", b"windows-payload\n")
            tools = root / "tools"
            github = root / "github-remote"
            objects = root / "r2-remote"
            install_stub_clis(tools, github, objects)
            environment = promoter_environment(tools, github, objects)

            result = subprocess.run(
                [
                    str(PROMOTE),
                    VERSION,
                    str(staged),
                    "terraphim-clients",
                    SOURCE_SHA,
                    CORRELATION_ID,
                ],
                env=environment,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("do not describe the same exact set", result.stderr)
            self.assertFalse((objects / "terraphim-agent" / "stable-v2.json").exists())

    def test_the_promoter_rejects_a_stage_that_is_missing_a_published_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = seal_stage(root)
            missing = (
                staged
                / "release-assets"
                / f"terraphim-cli-{VERSION}-x86_64-unknown-linux-musl.tar.gz"
            )
            missing.unlink()
            tools = root / "tools"
            github = root / "github-remote"
            objects = root / "r2-remote"
            install_stub_clis(tools, github, objects)
            environment = promoter_environment(tools, github, objects)

            result = subprocess.run(
                [
                    str(PROMOTE),
                    VERSION,
                    str(staged),
                    "terraphim-clients",
                    SOURCE_SHA,
                    CORRELATION_ID,
                ],
                env=environment,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("local asset must be a regular file", result.stderr)
            self.assertFalse((objects / "terraphim-cli" / "stable-v2.json").exists())

    def test_the_old_published_v1_legacy_pointer_stays_healthy(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            legacy_targets = (*COMMON_TARGETS, "universal-apple-darwin")
            for binary in BINARIES:
                directory_path = root / binary
                directory_path.mkdir()
                assets = {
                    target: f"{binary}/{binary}-1.21.14-{target}.tar.gz"
                    for target in legacy_targets
                }
                (directory_path / "stable.json").write_text(
                    json.dumps(
                        {
                            "assets": assets,
                            "released_at": "2026-09-01T00:00:00Z",
                            "version": "1.21.14",
                        },
                        sort_keys=True,
                    )
                    + "\n",
                    encoding="utf-8",
                )

            healthy = subprocess.run(
                [
                    "python3",
                    str(VALIDATE_R2),
                    "--base-url",
                    root.as_uri(),
                    "--activation-version",
                    ACTIVATION_VERSION,
                ],
                text=True,
                capture_output=True,
            )
            self.assertEqual(healthy.returncode, 0, healthy.stderr)
            self.assertIn("migration-pending terraphim-agent", healthy.stdout)


if __name__ == "__main__":
    unittest.main()
