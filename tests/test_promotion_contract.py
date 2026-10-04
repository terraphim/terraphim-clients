import hashlib
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "promote-release.sh"
ROLLBACK = ROOT / "scripts" / "rollback-release-pointers.sh"
VERSION = "1.21.15"
SOURCE_SHA = "a" * 40
CORRELATION_ID = "terraphim-clients/release-1.21.15:248"
COMMON_TARGETS = (
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
)


def executable(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def install_python_version_stub(tools: Path, accepted: bool) -> None:
    executable(
        tools / "python3",
        f'''#!{sys.executable}
import os, sys
if len(sys.argv) >= 3 and sys.argv[1] == "-c" and "sys.version_info" in sys.argv[2]:
    raise SystemExit({0 if accepted else 1})
os.execv({sys.executable!r}, [{sys.executable!r}, *sys.argv[1:]])
''',
    )


def prepare_complete_stage(root: Path) -> Path:
    stage_identity = f"client-release-stage-{VERSION}-{SOURCE_SHA}"
    staged = root / stage_identity
    assets = staged / "release-assets"
    manifests = staged / "manifests"
    assets.mkdir(parents=True)
    manifests.mkdir()
    sums = []
    for binary in ("terraphim-agent", "terraphim-cli", "terraphim-grep"):
        targets = list(COMMON_TARGETS)
        if binary != "terraphim-cli":
            targets.append("universal-apple-darwin")
        manifest_assets = {}
        for target in targets:
            suffix = ".zip" if target == "x86_64-pc-windows-msvc" else ".tar.gz"
            filename = f"{binary}-{VERSION}-{target}{suffix}"
            payload = f"signed-final-{binary}-{target}".encode()
            (assets / filename).write_bytes(payload)
            digest = hashlib.sha256(payload).hexdigest()
            sums.append(f"{digest}  {filename}\n")
            manifest_assets[target] = {
                "path": f"{binary}/{filename}",
                "sha256": digest,
                "size": len(payload),
            }
        candidate = {
            "assets": manifest_assets,
            "notes_url": f"https://example.invalid/v{VERSION}",
            "released_at": "2026-09-18T00:00:00Z",
            "version": VERSION,
        }
        (manifests / f"{binary}.v2.candidate.json").write_text(
            json.dumps(candidate, sort_keys=True) + "\n"
        )
        legacy = {
            **candidate,
            "assets": {key: value["path"] for key, value in manifest_assets.items()},
        }
        (manifests / f"{binary}.v1.candidate.json").write_text(
            json.dumps(legacy, sort_keys=True) + "\n"
        )
    (staged / "SHA256SUMS").write_text("".join(sorted(sums)))
    (staged / "provenance.json").write_text(
        json.dumps(
            {
                "archive_signatures": "embedded-zipsign-ed25519",
                "correlation_id": CORRELATION_ID,
                "release_tag": f"v{VERSION}",
                "source_sha": SOURCE_SHA,
                "stage_identity": stage_identity,
                "version": VERSION,
            },
            sort_keys=True,
        )
        + "\n"
    )
    return staged


def install_remote_tools(root: Path) -> tuple[Path, Path, Path, Path]:
    tools = root / "tools"
    gh_remote = root / "github-remote"
    r2_remote = root / "r2-remote"
    log = root / "calls.log"
    tools.mkdir()
    gh_remote.mkdir()
    r2_remote.mkdir()
    executable(
        tools / "gh",
        r'''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
remote = pathlib.Path(os.environ["GH_REMOTE"])
log = pathlib.Path(os.environ["CALL_LOG"])
if os.environ.get("ASSERT_NO_VERIFICATION_COPIES") == "1":
    scratch = pathlib.Path(os.environ["TMPDIR"])
    dirs = {"github", "github-readback", "r2-preflight", "r2-readback", "r2-immediate"}
    prefixes = ("pointer-terraphim-", "restore-terraphim-", "delete-terraphim-", "live-preflight-terraphim-")
    leaked = [path for path in scratch.rglob("*") if path.is_file() and (dirs.intersection(path.parts) or path.name.startswith(prefixes))]
    if leaked:
        with log.open("a") as handle: handle.write("scratch-leak " + " ".join(map(str, leaked)) + "\n")
        sys.exit(97)
if args[:2] == ["release", "view"]:
    with log.open("a") as handle: handle.write("gh-view\n")
    assets = [{"name": path.name, "size": path.stat().st_size, "state": "uploaded"} for path in sorted(remote.iterdir()) if path.is_file()]
    print(json.dumps({"assets": assets, "isDraft": os.environ.get("GH_DRAFT") == "1", "isPrerelease": os.environ.get("GH_PRERELEASE") == "1", "tagName": "v1.21.15"}))
elif args[:2] == ["release", "download"]:
    pattern = args[args.index("--pattern") + 1]
    destination = pathlib.Path(args[args.index("--dir") + 1])
    source = remote / pattern
    if not source.is_file(): sys.exit(4)
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination / pattern)
elif args[:2] == ["release", "upload"]:
    for value in args[3:]:
        if value == "--repo": break
        path = pathlib.Path(value)
        target = remote / path.name
        if target.exists(): sys.exit(22)
        shutil.copyfile(path, target)
        with log.open("a") as handle: handle.write(f"gh-upload {path.name}\n")
else:
    sys.exit(2)
''',
    )
    executable(
        tools / "wrangler",
        r'''#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
if os.environ.get("ASSERT_NO_VERIFICATION_COPIES") == "1":
    scratch = pathlib.Path(os.environ["TMPDIR"])
    dirs = {"github", "github-readback", "r2-preflight", "r2-readback", "r2-immediate"}
    prefixes = ("pointer-terraphim-", "restore-terraphim-", "delete-terraphim-", "live-preflight-terraphim-")
    leaked = [path for path in scratch.rglob("*") if path.is_file() and (dirs.intersection(path.parts) or path.name.startswith(prefixes))]
    if leaked:
        with pathlib.Path(os.environ["CALL_LOG"]).open("a") as handle: handle.write("scratch-leak " + " ".join(map(str, leaked)) + "\n")
        sys.exit(97)
if args[:3] not in (["r2", "object", "put"], ["r2", "object", "delete"]): sys.exit(2)
operation = args[2]
key = args[3].split("/", 1)[1]
failure = os.environ.get("FAIL_OBJECT_ONCE", os.environ.get("FAIL_POINTER_ONCE", os.environ.get("FAIL_DELETE_ONCE", "")))
marker = pathlib.Path(os.environ.get("FAILURE_MARKER", "/nonexistent"))
if key == failure and not marker.exists():
    marker.write_text("failed\n")
    sys.exit(19)
remote = pathlib.Path(os.environ["R2_REMOTE"]) / key
if operation == "put":
    source = pathlib.Path(args[args.index("--file") + 1])
    remote.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, remote)
    action = "r2-put"
else:
    if remote.exists(): remote.unlink()
    action = "r2-delete"
with pathlib.Path(os.environ["CALL_LOG"]).open("a") as handle: handle.write(f"{action} {key}\n")
''',
    )
    executable(
        tools / "curl",
        r'''#!/usr/bin/env python3
import os, pathlib, shutil, sys, urllib.parse
args = sys.argv[1:]
if os.environ.get("ASSERT_NO_VERIFICATION_COPIES") == "1":
    scratch = pathlib.Path(os.environ["TMPDIR"])
    dirs = {"github", "github-readback", "r2-preflight", "r2-readback", "r2-immediate"}
    prefixes = ("pointer-terraphim-", "restore-terraphim-", "delete-terraphim-", "live-preflight-terraphim-")
    leaked = [path for path in scratch.rglob("*") if path.is_file() and (dirs.intersection(path.parts) or path.name.startswith(prefixes))]
    if leaked:
        with pathlib.Path(os.environ["CALL_LOG"]).open("a") as handle: handle.write("scratch-leak " + " ".join(map(str, leaked)) + "\n")
        sys.exit(97)
url = next(value for value in args if value.startswith("http"))
key = urllib.parse.urlsplit(url).path.lstrip("/")
remote = pathlib.Path(os.environ["R2_REMOTE"])
source = remote / key
count_dir = pathlib.Path(os.environ["CURL_COUNT_DIR"])
count_file = count_dir / key.replace("/", "_")
count_dir.mkdir(parents=True, exist_ok=True)
count = int(count_file.read_text()) + 1 if count_file.exists() else 1
count_file.write_text(str(count))
if key == os.environ.get("APPEAR_ON_READ_KEY") and count == int(os.environ.get("APPEAR_ON_READ_NUMBER", "2")):
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_bytes(os.environ.get("APPEAR_BYTES", "different-race-winner").encode())
forced_key = os.environ.get("CURL_FORCED_KEY", "")
forced_status = os.environ.get("CURL_FORCED_STATUS", "")
forced_once = os.environ.get("CURL_FORCED_ONCE") == "1"
marker = pathlib.Path(os.environ.get("CURL_FAILURE_MARKER", str(count_dir / "forced-once")))
forced = forced_status and (forced_key in ("*", key)) and (not forced_once or not marker.exists())
if forced:
    if forced_once: marker.write_text("used\n")
    status = forced_status
    if "--output" in args: pathlib.Path(args[args.index("--output") + 1]).write_bytes(b"forced-response")
    sys.stdout.write(status)
    sys.exit(int(os.environ.get("CURL_FORCED_EXIT", "0")))
status = "200" if source.is_file() else "404"
if "--output" in args and source.is_file():
    destination = pathlib.Path(args[args.index("--output") + 1])
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
sys.stdout.write(status)
''',
    )
    return tools, gh_remote, r2_remote, log


def promotion_env(tools: Path, gh_remote: Path, r2_remote: Path, log: Path) -> dict[str, str]:
    env = os.environ.copy()
    env.update(
        {
            "PATH": f"{tools}:{env['PATH']}",
            "CALL_LOG": str(log),
            "GH_REMOTE": str(gh_remote),
            "R2_REMOTE": str(r2_remote),
            "CURL_COUNT_DIR": str(log.parent / "curl-counts"),
            "BASE_URL": "https://downloads.invalid",
        }
    )
    return env


def promotion_command(staged: Path) -> list[str]:
    return [
        str(SCRIPT), VERSION, str(staged), "terraphim-clients", SOURCE_SHA, CORRELATION_ID
    ]


def rollback_command(staged: Path) -> list[str]:
    return [
        str(ROLLBACK), VERSION, str(staged), SOURCE_SHA, CORRELATION_ID,
        "--authorized-pointers-only",
    ]


def seed_github(staged: Path, remote: Path) -> None:
    for path in (staged / "release-assets").iterdir():
        (remote / path.name).write_bytes(path.read_bytes())
    (remote / "SHA256SUMS").write_bytes((staged / "SHA256SUMS").read_bytes())


def seed_legacy_pointers(remote: Path) -> dict[str, bytes]:
    retained = {}
    for binary in ("terraphim-agent", "terraphim-cli", "terraphim-grep"):
        payload = json.dumps(
            {"assets": {"x86_64-unknown-linux-gnu": f"{binary}/{binary}-1.21.14-x86_64-unknown-linux-gnu.tar.gz"},
             "released_at": "2026-09-01T00:00:00Z", "version": "1.21.14"},
            sort_keys=True,
        ).encode()
        path = remote / binary / "stable.json"
        path.parent.mkdir(parents=True)
        path.write_bytes(payload)
        retained[binary] = payload
    return retained


def replace_live_pointers(remote: Path, version: str = "1.21.16") -> dict[Path, bytes]:
    live = {}
    for binary in ("terraphim-agent", "terraphim-cli", "terraphim-grep"):
        targets = list(COMMON_TARGETS)
        if binary != "terraphim-cli":
            targets.append("universal-apple-darwin")
        strict_assets = {}
        for target in targets:
            suffix = ".zip" if target == "x86_64-pc-windows-msvc" else ".tar.gz"
            path = f"{binary}/{binary}-{version}-{target}{suffix}"
            strict_assets[target] = {
                "path": path,
                "sha256": hashlib.sha256(f"{version}:{binary}:{target}".encode()).hexdigest(),
                "size": 1,
            }
        common = {
            "notes_url": f"https://example.invalid/v{version}",
            "released_at": "2026-09-19T00:00:00Z",
            "version": version,
        }
        documents = {
            "stable.json": {
                **common,
                "assets": {
                    target: metadata["path"]
                    for target, metadata in strict_assets.items()
                },
            },
            "stable-v2.json": {**common, "assets": strict_assets},
        }
        for pointer, document in documents.items():
            payload = (json.dumps(document, sort_keys=True) + "\n").encode()
            path = remote / binary / pointer
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(payload)
            live[path] = payload
    return live


class PromotionContract(unittest.TestCase):
    def test_operator_entrypoints_reject_python38_before_local_or_remote_work(self) -> None:
        for entrypoint in ("promote", "rollback"):
            with self.subTest(entrypoint=entrypoint), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                staged = prepare_complete_stage(root)
                tools, gh_remote, r2_remote, log = install_remote_tools(root)
                install_python_version_stub(tools, accepted=False)
                command = promotion_command(staged) if entrypoint == "promote" else rollback_command(staged)
                result = subprocess.run(
                    command,
                    env=promotion_env(tools, gh_remote, r2_remote, log),
                    text=True,
                    capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("ERROR: Python 3.9 or newer is required", result.stderr)
                self.assertFalse(log.exists(), "Python rejection must precede every remote query")

    def test_operator_entrypoints_accept_python39_gate(self) -> None:
        for entrypoint in ("promote", "rollback"):
            with self.subTest(entrypoint=entrypoint), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                staged = prepare_complete_stage(root)
                (staged / "provenance.json").unlink()
                tools, gh_remote, r2_remote, log = install_remote_tools(root)
                install_python_version_stub(tools, accepted=True)
                command = promotion_command(staged) if entrypoint == "promote" else rollback_command(staged)
                result = subprocess.run(
                    command,
                    env=promotion_env(tools, gh_remote, r2_remote, log),
                    text=True,
                    capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("Python 3.9 or newer", result.stderr)
                self.assertIn("provenance.json", result.stderr)
                self.assertFalse(log.exists(), "accepted gate must still validate locally before remote queries")

    def test_successful_promotion_releases_each_verification_copy_immediately(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            scratch = root / "operator-tmp"
            scratch.mkdir()
            env = promotion_env(tools, gh_remote, r2_remote, log)
            env.update(
                {
                    "TMPDIR": str(scratch),
                    "ASSERT_NO_VERIFICATION_COPIES": "1",
                }
            )
            result = subprocess.run(
                promotion_command(staged), env=env, text=True, capture_output=True
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("scratch-leak", log.read_text())
            self.assertEqual(list(scratch.iterdir()), [])

    def test_rollback_requires_explicit_pointers_only_authorization_flag(self) -> None:
        result = subprocess.run(
            [str(ROLLBACK), VERSION, "/nonexistent", SOURCE_SHA, CORRELATION_ID],
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("--authorized-pointers-only", result.stderr)

    def test_failed_immutable_upload_never_advances_stable_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            env = promotion_env(tools, gh_remote, r2_remote, log)
            env["FAIL_OBJECT_ONCE"] = f"terraphim-agent/manifests/v1/{VERSION}.json"
            env["FAILURE_MARKER"] = str(root / "failed-once")
            result = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            calls = log.read_text() if log.exists() else ""
            self.assertNotIn("stable.json", calls)
            self.assertNotIn("stable-v2.json", calls)

    def test_local_stage_and_provenance_fail_before_any_remote_query(self) -> None:
        mutations = (
            "missing",
            "duplicate",
            "source",
            "correlation",
            "version",
            "release-tag",
            "signature-scheme",
            "identity",
            "stage-path",
            "mixed-sums",
        )
        for mutation in mutations:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                staged = prepare_complete_stage(root)
                provenance = staged / "provenance.json"
                data = json.loads(provenance.read_text())
                if mutation == "missing":
                    provenance.unlink()
                elif mutation == "duplicate":
                    provenance.write_text(provenance.read_text().rstrip()[:-1] + ',"version":"1.21.15"}')
                elif mutation == "source":
                    data["source_sha"] = "b" * 40; provenance.write_text(json.dumps(data))
                elif mutation == "correlation":
                    data["correlation_id"] = "other-run"; provenance.write_text(json.dumps(data))
                elif mutation == "version":
                    data["version"] = "1.21.14"; provenance.write_text(json.dumps(data))
                elif mutation == "release-tag":
                    data["release_tag"] = "v1.21.14"; provenance.write_text(json.dumps(data))
                elif mutation == "signature-scheme":
                    data["archive_signatures"] = "different"; provenance.write_text(json.dumps(data))
                elif mutation == "identity":
                    data["stage_identity"] = "different-stage"; provenance.write_text(json.dumps(data))
                elif mutation == "stage-path":
                    renamed = root / "wrong-stage-directory"
                    staged.rename(renamed)
                    staged = renamed
                else:
                    rows = (staged / "SHA256SUMS").read_text().splitlines()
                    rows[0] = "0" * 64 + rows[0][64:]
                    (staged / "SHA256SUMS").write_text("\n".join(rows) + "\n")
                tools, gh_remote, r2_remote, log = install_remote_tools(root)
                result = subprocess.run(
                    promotion_command(staged),
                    env=promotion_env(tools, gh_remote, r2_remote, log),
                    text=True,
                    capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(log.exists(), "no remote query or write may precede provenance validation")

    def test_draft_and_prerelease_are_rejected_before_upload(self) -> None:
        for state in ("GH_DRAFT", "GH_PRERELEASE"):
            with self.subTest(state=state), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                staged = prepare_complete_stage(root)
                tools, gh_remote, r2_remote, log = install_remote_tools(root)
                env = promotion_env(tools, gh_remote, r2_remote, log)
                env[state] = "1"
                result = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("draft or prerelease", result.stderr)
                self.assertNotIn("upload", log.read_text())

    def test_second_invocation_skips_identical_immutables(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            env = promotion_env(tools, gh_remote, r2_remote, log)
            first = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            first_writes = [line for line in log.read_text().splitlines() if "upload" in line or "r2-put" in line]
            snapshot = (staged / "rollback-pointers" / "state.json").read_bytes()
            second = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
            self.assertEqual(second.returncode, 0, second.stderr)
            second_writes = [line for line in log.read_text().splitlines() if "upload" in line or "r2-put" in line]
            self.assertEqual(second_writes, first_writes)
            self.assertEqual((staged / "rollback-pointers" / "state.json").read_bytes(), snapshot)

    def test_partial_legacy_pointer_failure_recovers_on_rerun(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            env = promotion_env(tools, gh_remote, r2_remote, log)
            env["FAIL_POINTER_ONCE"] = "terraphim-grep/stable.json"
            env["FAILURE_MARKER"] = str(root / "failed-once")
            first = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
            self.assertNotEqual(first.returncode, 0)
            self.assertTrue((r2_remote / "terraphim-grep" / "stable-v2.json").is_file())
            second = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertTrue((r2_remote / "terraphim-grep" / "stable.json").is_file())

    def test_conflicting_immutable_fails_before_any_remote_write(self) -> None:
        for surface in ("github", "r2"):
            with self.subTest(surface=surface), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                staged = prepare_complete_stage(root)
                tools, gh_remote, r2_remote, log = install_remote_tools(root)
                first_asset = sorted((staged / "release-assets").iterdir())[0]
                if surface == "github":
                    (gh_remote / first_asset.name).write_bytes(b"conflicting")
                else:
                    candidate = json.loads((staged / "manifests" / "terraphim-agent.v2.candidate.json").read_text())
                    path = next(iter(candidate["assets"].values()))["path"]
                    remote = r2_remote / path
                    remote.parent.mkdir(parents=True)
                    remote.write_bytes(b"conflicting")
                result = subprocess.run(
                    promotion_command(staged),
                    env=promotion_env(tools, gh_remote, r2_remote, log),
                    text=True,
                    capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)
                calls = log.read_text() if log.exists() else ""
                self.assertNotIn("gh-upload", calls)
                self.assertNotIn("r2-put", calls)

    def test_every_non_404_and_transport_failure_fails_before_any_write(self) -> None:
        cases = (
            ("302", "0"),
            ("403", "0"),
            ("429", "0"),
            ("503", "22"),
            ("000", "7"),
            ("malformed", "0"),
        )
        for status, exit_code in cases:
            with self.subTest(status=status), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                staged = prepare_complete_stage(root)
                tools, gh_remote, r2_remote, log = install_remote_tools(root)
                seed_github(staged, gh_remote)
                path = "terraphim-agent/terraphim-agent-1.21.15-aarch64-apple-darwin.tar.gz"
                remote = r2_remote / path
                remote.parent.mkdir(parents=True)
                remote.write_bytes(b"already-present-different")
                env = promotion_env(tools, gh_remote, r2_remote, log)
                env.update({"CURL_FORCED_KEY": path, "CURL_FORCED_STATUS": status, "CURL_FORCED_EXIT": exit_code, "CURL_FORCED_ONCE": "1"})
                result = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                calls = log.read_text()
                self.assertNotIn("gh-upload", calls)
                self.assertNotIn("r2-put", calls)
                self.assertEqual(remote.read_bytes(), b"already-present-different")

    def test_object_appearing_different_at_immediate_recheck_is_not_overwritten(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            seed_github(staged, gh_remote)
            path = "terraphim-agent/terraphim-agent-1.21.15-aarch64-apple-darwin.tar.gz"
            env = promotion_env(tools, gh_remote, r2_remote, log)
            env["APPEAR_ON_READ_KEY"] = path
            result = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(f"r2-put {path}", log.read_text())
            self.assertEqual((r2_remote / path).read_bytes(), b"different-race-winner")

    def test_full_promotion_rollback_and_idempotent_rerun(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            retained = seed_legacy_pointers(r2_remote)
            env = promotion_env(tools, gh_remote, r2_remote, log)
            scratch = root / "operator-tmp"
            scratch.mkdir()
            env.update(
                {
                    "TMPDIR": str(scratch),
                    "ASSERT_NO_VERIFICATION_COPIES": "1",
                }
            )
            promoted = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
            self.assertEqual(promoted.returncode, 0, promoted.stderr)
            before_rollback = len(log.read_text().splitlines())
            rolled_back = subprocess.run(rollback_command(staged), env=env, text=True, capture_output=True)
            self.assertEqual(rolled_back.returncode, 0, rolled_back.stderr)
            for binary, payload in retained.items():
                self.assertEqual((r2_remote / binary / "stable.json").read_bytes(), payload)
                self.assertFalse((r2_remote / binary / "stable-v2.json").exists())
            rollback_calls = log.read_text().splitlines()[before_rollback:]
            legacy_restores = [index for index, line in enumerate(rollback_calls) if line.endswith("stable.json")]
            v2_deletes = [index for index, line in enumerate(rollback_calls) if line.startswith("r2-delete")]
            self.assertTrue(legacy_restores and v2_deletes)
            self.assertLess(max(legacy_restores), min(v2_deletes))
            writes = [line for line in log.read_text().splitlines() if line.startswith("r2-")]
            rerun = subprocess.run(rollback_command(staged), env=env, text=True, capture_output=True)
            self.assertEqual(rerun.returncode, 0, rerun.stderr)
            self.assertEqual([line for line in log.read_text().splitlines() if line.startswith("r2-")], writes)
            self.assertNotIn("scratch-leak", log.read_text())
            self.assertEqual(list(scratch.iterdir()), [])

    def test_stale_rollback_refuses_coherent_newer_release_without_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            seed_legacy_pointers(r2_remote)
            env = promotion_env(tools, gh_remote, r2_remote, log)
            promoted = subprocess.run(
                promotion_command(staged), env=env, text=True, capture_output=True
            )
            self.assertEqual(promoted.returncode, 0, promoted.stderr)
            newer = replace_live_pointers(r2_remote)
            before = len(log.read_text().splitlines())

            result = subprocess.run(
                rollback_command(staged), env=env, text=True, capture_output=True
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("live pointer was not written by this promotion", result.stderr)
            rollback_calls = log.read_text().splitlines()[before:]
            self.assertFalse(any(line.startswith(("r2-put", "r2-delete")) for line in rollback_calls))
            for path, payload in newer.items():
                self.assertEqual(path.read_bytes(), payload)

    def test_stale_rollback_global_preflight_prevents_partial_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            retained = seed_legacy_pointers(r2_remote)
            env = promotion_env(tools, gh_remote, r2_remote, log)
            promoted = subprocess.run(
                promotion_command(staged), env=env, text=True, capture_output=True
            )
            self.assertEqual(promoted.returncode, 0, promoted.stderr)
            foreign = r2_remote / "terraphim-grep" / "stable-v2.json"
            foreign.write_bytes(b'{"version":"1.21.16","foreign":true}')
            live_before = {
                path: path.read_bytes()
                for binary in ("terraphim-agent", "terraphim-cli", "terraphim-grep")
                for path in (
                    r2_remote / binary / "stable.json",
                    r2_remote / binary / "stable-v2.json",
                )
            }
            before = len(log.read_text().splitlines())

            result = subprocess.run(
                rollback_command(staged), env=env, text=True, capture_output=True
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("live pointer was not written by this promotion", result.stderr)
            rollback_calls = log.read_text().splitlines()[before:]
            self.assertFalse(any(line.startswith(("r2-put", "r2-delete")) for line in rollback_calls))
            for path, payload in live_before.items():
                self.assertEqual(path.read_bytes(), payload)
            self.assertEqual(
                {binary: (r2_remote / binary / "stable.json").read_bytes() for binary in retained},
                {binary: live_before[r2_remote / binary / "stable.json"] for binary in retained},
            )

    def test_rollback_repairs_partial_promotion_and_recovers_from_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            retained = seed_legacy_pointers(r2_remote)
            env = promotion_env(tools, gh_remote, r2_remote, log)
            env["FAIL_POINTER_ONCE"] = "terraphim-grep/stable.json"
            env["FAILURE_MARKER"] = str(root / "promotion-failed")
            partial = subprocess.run(promotion_command(staged), env=env, text=True, capture_output=True)
            self.assertNotEqual(partial.returncode, 0)
            env.pop("FAIL_POINTER_ONCE")
            env["FAIL_DELETE_ONCE"] = "terraphim-cli/stable-v2.json"
            env["FAILURE_MARKER"] = str(root / "rollback-failed")
            failed = subprocess.run(rollback_command(staged), env=env, text=True, capture_output=True)
            self.assertNotEqual(failed.returncode, 0)
            recovered = subprocess.run(rollback_command(staged), env=env, text=True, capture_output=True)
            self.assertEqual(recovered.returncode, 0, recovered.stderr)
            for binary, payload in retained.items():
                self.assertEqual((r2_remote / binary / "stable.json").read_bytes(), payload)
                self.assertFalse((r2_remote / binary / "stable-v2.json").exists())

    def test_rollback_handles_pre_promotion_pointer_absence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = prepare_complete_stage(root)
            tools, gh_remote, r2_remote, log = install_remote_tools(root)
            env = promotion_env(tools, gh_remote, r2_remote, log)
            self.assertEqual(subprocess.run(promotion_command(staged), env=env).returncode, 0)
            result = subprocess.run(rollback_command(staged), env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            for binary in ("terraphim-agent", "terraphim-cli", "terraphim-grep"):
                self.assertFalse((r2_remote / binary / "stable.json").exists())
                self.assertFalse((r2_remote / binary / "stable-v2.json").exists())


if __name__ == "__main__":
    unittest.main()
