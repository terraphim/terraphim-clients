#!/usr/bin/env python3
"""Qualify an immutable release stage before populating makepkg's source cache."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import platform
from pathlib import Path
import re
import shutil
import sys
import tempfile

REPOSITORY = "terraphim/terraphim-clients"
WORKFLOW = ".github/workflows/release-binaries.yml"
ROOT = Path(__file__).resolve().parents[1]
CONTRACT_KEYS = {
    "version",
    "source_sha",
    "control_sha",
    "correlation_id",
    "run_id",
    "run_attempt",
    "artifact_id",
    "artifact_name",
    "artifact_digest",
    "files",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON field: {key}")
        result[key] = value
    return result


def read_json(path: Path):
    require(
        path.is_file() and not path.is_symlink(), "JSON input must be a regular file"
    )
    require(path.stat().st_size <= 4 * 1024 * 1024, "JSON input exceeds bound")
    return json.loads(path.read_text(), object_pairs_hook=unique_object)


def contract(path: Path) -> dict:
    value = read_json(path)
    require(
        isinstance(value, dict) and set(value) == CONTRACT_KEYS,
        "unexpected contract fields",
    )
    for name in ("source_sha", "control_sha"):
        require(
            isinstance(value[name], str)
            and re.fullmatch(r"[0-9a-f]{40}", value[name]) is not None,
            f"invalid {name}",
        )
    require(
        isinstance(value["version"], str)
        and re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", value["version"])
        is not None,
        "invalid version",
    )
    require(
        isinstance(value["correlation_id"], str)
        and re.fullmatch(r"[A-Za-z0-9._:/@+-]{1,128}", value["correlation_id"])
        is not None,
        "invalid correlation",
    )
    for name in ("run_id", "run_attempt", "artifact_id"):
        require(type(value[name]) is int and value[name] > 0, f"invalid {name}")
    require(
        value["artifact_name"]
        == f"client-release-stage-{value['version']}-{value['source_sha']}",
        "artifact identity mismatch",
    )
    require(
        isinstance(value["artifact_digest"], str)
        and re.fullmatch(r"sha256:[0-9a-f]{64}", value["artifact_digest"]) is not None,
        "invalid artifact digest",
    )
    require(
        isinstance(value["files"], dict) and 17 < len(value["files"]) <= 128,
        "invalid stage inventory",
    )
    for name, digest in value["files"].items():
        require(
            isinstance(name, str)
            and re.fullmatch(r"[A-Za-z0-9_.+-]+(?:/[A-Za-z0-9_.+-]+)*", name)
            is not None
            and all(p not in (".", "..") for p in name.split("/")),
            "unsafe inventory path",
        )
        require(
            isinstance(digest, str)
            and re.fullmatch(r"[0-9a-f]{64}", digest) is not None,
            "invalid file digest",
        )
    return value


def release_mode(value: dict, status: int, release: dict) -> str:
    if status == 404:
        return "stage"
    require(status == 200, "release lookup must return 200 or 404")
    require(
        release.get("tag_name") == f"v{value['version']}"
        and release.get("draft") is False
        and release.get("prerelease") is False,
        "release is not the intended stable release",
    )
    assets = release.get("assets")
    require(isinstance(assets, list), "missing release assets")
    names = [a.get("name") for a in assets]
    require(len(names) == len(set(names)), "duplicate release asset")
    expected = {Path(p).name for p in value["files"] if p.startswith("release-assets/")}
    require(
        expected <= set(names), "published release is incomplete; no stage fallback"
    )
    for asset in assets:
        if asset.get("name") in expected:
            require(
                asset.get("browser_download_url")
                == f"https://github.com/{REPOSITORY}/releases/download/v{value['version']}/{asset['name']}",
                "unexpected public asset URL",
            )
    return "public"


def validate_metadata(value: dict, run: dict, artifact: dict) -> None:
    require(
        type(run.get("id")) is int
        and type(run.get("run_attempt")) is int
        and run.get("id") == value["run_id"]
        and run.get("run_attempt") == value["run_attempt"],
        "run/attempt mismatch",
    )
    require(
        run.get("path") == WORKFLOW
        and run.get("event") == "workflow_dispatch"
        and run.get("head_branch") == "main",
        "wrong producer workflow",
    )
    require(
        run.get("status") == "completed" and run.get("conclusion") == "success",
        "producer has not succeeded",
    )
    require(
        run.get("head_sha") == value["control_sha"], "producer control source mismatch"
    )
    repo, head_repo = run.get("repository", {}), run.get("head_repository", {})
    require(
        repo.get("full_name") == REPOSITORY
        and head_repo.get("full_name") == REPOSITORY
        and type(repo.get("id")) is int
        and repo.get("id") == head_repo.get("id"),
        "wrong producer repository",
    )
    require(
        artifact.get("id") == value["artifact_id"]
        and artifact.get("name") == value["artifact_name"]
        and artifact.get("digest") == value["artifact_digest"]
        and artifact.get("expired") is False,
        "artifact identity/digest mismatch",
    )
    parent = artifact.get("workflow_run", {})
    require(
        parent.get("id") == value["run_id"]
        and parent.get("head_sha") == value["control_sha"]
        and parent.get("head_branch") == "main"
        and parent.get("repository_id") == repo["id"]
        and parent.get("head_repository_id") == repo["id"],
        "artifact producer mismatch",
    )


def prepare(
    value: dict, stage: Path, run: dict, artifact: dict, cache: Path, arch: str
) -> dict:
    validate_metadata(value, run, artifact)
    native = {"arm64": "aarch64"}.get(platform.machine(), platform.machine())
    require(
        arch in ("x86_64", "aarch64") and arch == native,
        "unsupported or mismatched native architecture",
    )
    require(
        stage.is_dir()
        and not stage.is_symlink()
        and stage.name == value["artifact_name"],
        "stage directory mismatch",
    )
    actual = {}
    actual_dirs = set()
    expected_dirs = {
        parent.as_posix()
        for name in value["files"]
        for parent in Path(name).parents
        if parent != Path(".")
    }
    for base, dirs, files in os.walk(stage):
        actual_dirs.update(
            (Path(base) / name).relative_to(stage).as_posix() for name in dirs
        )
        for name in dirs + files:
            p = Path(base) / name
            require(not p.is_symlink(), "symlink in stage")
        for name in files:
            p = Path(base) / name
            require(p.is_file(), "non-regular stage file")
            require(p.stat().st_size <= 512 * 1024 * 1024, "stage file exceeds bound")
            with p.open("rb") as stream:
                digest = hashlib.file_digest(stream, "sha256").hexdigest()
            actual[p.relative_to(stage).as_posix()] = digest
    require(
        actual_dirs == expected_dirs,
        "stage directory set differs from reviewed inventory",
    )
    require(
        actual == value["files"], "stage files differ from exact reviewed inventory"
    )
    spec = importlib.util.spec_from_file_location(
        "promotion_stage", ROOT / "scripts/validate-promotion-stage.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    with tempfile.TemporaryDirectory() as tmp:
        module.validate_stage(
            value["version"],
            stage,
            value["source_sha"],
            value["correlation_id"],
            Path(tmp) / "plan.tsv",
        )
    require(
        cache.is_dir() and not cache.is_symlink(),
        "cache directory must exist and be real",
    )
    selected = [
        f"{binary}-{value['version']}-{arch}-unknown-linux-musl.tar.gz"
        for binary in ("terraphim-agent", "terraphim-grep")
    ]
    require(
        not any(cache.glob("*.tar.gz")) and not any(cache.glob("*.zip")),
        "source cache is not empty",
    )
    result = {}
    for name in selected:
        source = stage / "release-assets" / name
        with (cache / name).open("xb") as dst, source.open("rb") as src:
            shutil.copyfileobj(src, dst)
        with (cache / name).open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        require(
            digest == value["files"][f"release-assets/{name}"],
            "cached archive failed readback",
        )
        result[name] = digest
    return {
        "version": value["version"],
        "source_sha": value["source_sha"],
        "run_id": value["run_id"],
        "run_attempt": value["run_attempt"],
        "architecture": arch,
        "cached": result,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("contract", type=Path)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("identity")
    metadata = commands.add_parser("metadata")
    metadata.add_argument("run", type=Path)
    metadata.add_argument("artifact", type=Path)
    mode = commands.add_parser("mode")
    mode.add_argument("status", type=int)
    mode.add_argument("release", type=Path)
    prep = commands.add_parser("prepare")
    prep.add_argument("stage", type=Path)
    prep.add_argument("run", type=Path)
    prep.add_argument("artifact", type=Path)
    prep.add_argument("cache", type=Path)
    prep.add_argument("arch")
    args = parser.parse_args()
    value = contract(args.contract)
    if args.command == "identity":
        for key in ("version", "run_id", "artifact_id", "artifact_name"):
            print(f"{key}={value[key]}")
    elif args.command == "metadata":
        validate_metadata(value, read_json(args.run), read_json(args.artifact))
        print("producer_metadata=verified")
    elif args.command == "mode":
        print("mode=" + release_mode(value, args.status, read_json(args.release)))
    else:
        print(
            json.dumps(
                prepare(
                    value,
                    args.stage,
                    read_json(args.run),
                    read_json(args.artifact),
                    args.cache,
                    args.arch,
                ),
                sort_keys=True,
                indent=2,
            )
        )


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        sys.exit(f"ERROR: {error}")
