#!/usr/bin/env python3
"""Validate a sealed client release stage and emit its immutable R2 object plan."""

from __future__ import annotations

import datetime
import hashlib
import json
import pathlib
import re
import sys
import urllib.parse
from typing import NoReturn


COMMON_TARGETS = {
    "aarch64-apple-darwin": ".tar.gz",
    "aarch64-unknown-linux-musl": ".tar.gz",
    "x86_64-apple-darwin": ".tar.gz",
    "x86_64-pc-windows-msvc": ".zip",
    "x86_64-unknown-linux-gnu": ".tar.gz",
    "x86_64-unknown-linux-musl": ".tar.gz",
}
TARGET_SETS = {
    "terraphim-agent": {**COMMON_TARGETS, "universal-apple-darwin": ".tar.gz"},
    "terraphim-cli": COMMON_TARGETS,
    "terraphim-grep": {**COMMON_TARGETS, "universal-apple-darwin": ".tar.gz"},
}
STABLE_VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
SOURCE_SHA = re.compile(r"[0-9a-f]{40}")
CORRELATION = re.compile(r"[A-Za-z0-9._:/@+-]{1,128}")


def fail(message: str) -> NoReturn:
    raise SystemExit(f"ERROR: {message}")


def reject_duplicates(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: pathlib.Path):
    if not path.is_file() or path.is_symlink():
        fail(f"{path}: must be a regular file")
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates
        )
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as error:
        fail(f"{path}: invalid JSON: {error}")


def hash_file(path: pathlib.Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            size += len(chunk)
            digest.update(chunk)
    return size, digest.hexdigest()


def validate_identity(
    version: str, staged: pathlib.Path, expected_source_sha: str, correlation_id: str
) -> None:
    if STABLE_VERSION.fullmatch(version) is None:
        fail(f"invalid stable version {version!r}")
    if SOURCE_SHA.fullmatch(expected_source_sha) is None:
        fail("expected source SHA must be 40 lowercase hexadecimal characters")
    if CORRELATION.fullmatch(correlation_id) is None:
        fail("correlation ID contains unsafe characters or exceeds 128 characters")
    stage_identity = f"client-release-stage-{version}-{expected_source_sha}"
    if staged.name != stage_identity:
        fail(f"stage directory basename must be {stage_identity!r}")
    provenance = load_json(staged / "provenance.json")
    expected = {
        "archive_signatures": "embedded-zipsign-ed25519",
        "correlation_id": correlation_id,
        "release_tag": f"v{version}",
        "source_sha": expected_source_sha,
        "stage_identity": stage_identity,
        "version": version,
    }
    if provenance != expected:
        fail("provenance.json does not exactly match the authorized release identity")


def load_sums(path: pathlib.Path) -> dict[str, str]:
    if not path.is_file() or path.is_symlink():
        fail(f"{path}: must be a regular file")
    result: dict[str, str] = {}
    for line_number, line in enumerate(path.read_text(encoding="ascii").splitlines(), 1):
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.+-]+)", line)
        if match is None:
            fail(f"{path}:{line_number}: malformed checksum row")
        digest, name = match.groups()
        if name in result:
            fail(f"{path}:{line_number}: duplicate checksum name {name}")
        result[name] = digest
    if not result:
        fail(f"{path}: checksum set is empty")
    return result


def validate_stage(
    version: str,
    staged: pathlib.Path,
    expected_source_sha: str,
    correlation_id: str,
    plan_path: pathlib.Path,
) -> None:
    validate_identity(version, staged, expected_source_sha, correlation_id)
    asset_dir = staged / "release-assets"
    manifest_dir = staged / "manifests"
    if not asset_dir.is_dir() or asset_dir.is_symlink():
        fail("release-assets must be a real directory")
    if not manifest_dir.is_dir() or manifest_dir.is_symlink():
        fail("manifests must be a real directory")
    sums = load_sums(staged / "SHA256SUMS")
    expected_local_assets: set[str] = set()
    object_rows: list[tuple[str, pathlib.Path]] = []
    for binary, targets in TARGET_SETS.items():
        v2_path = manifest_dir / f"{binary}.v2.candidate.json"
        v1_path = manifest_dir / f"{binary}.v1.candidate.json"
        v2 = load_json(v2_path)
        v1 = load_json(v1_path)
        for path, data in ((v2_path, v2), (v1_path, v1)):
            if not isinstance(data, dict) or set(data) != {
                "assets",
                "notes_url",
                "released_at",
                "version",
            }:
                fail(f"{path}: unexpected top-level keys")
            if data["version"] != version:
                fail(f"{path}: version mismatch")
            if not isinstance(data["assets"], dict) or set(data["assets"]) != set(targets):
                fail(f"{path}: target set mismatch")
            notes = urllib.parse.urlsplit(data["notes_url"])
            if notes.scheme != "https" or not notes.netloc:
                fail(f"{path}: notes_url must be absolute HTTPS")
            try:
                datetime.datetime.strptime(data["released_at"], "%Y-%m-%dT%H:%M:%SZ")
            except (TypeError, ValueError) as error:
                fail(f"{path}: released_at must be UTC RFC3339 seconds: {error}")
        if (v1["version"], v1["notes_url"], v1["released_at"]) != (
            v2["version"],
            v2["notes_url"],
            v2["released_at"],
        ):
            fail(f"{v1_path}: metadata does not correlate with v2")
        for target, asset in v2["assets"].items():
            if not isinstance(asset, dict) or set(asset) != {"path", "sha256", "size"}:
                fail(f"{v2_path}: {target}: unexpected asset keys")
            if not isinstance(asset["sha256"], str) or re.fullmatch(
                r"[0-9a-f]{64}", asset["sha256"]
            ) is None:
                fail(f"{v2_path}: {target}: invalid sha256")
            if (
                not isinstance(asset["size"], int)
                or isinstance(asset["size"], bool)
                or asset["size"] <= 0
            ):
                fail(f"{v2_path}: {target}: invalid size")
            filename = f"{binary}-{version}-{target}{targets[target]}"
            expected_path = f"{binary}/{filename}"
            if asset["path"] != expected_path:
                fail(f"{v2_path}: {target}: asset path mismatch")
            if v1["assets"][target] != expected_path:
                fail(f"{v1_path}: {target}: legacy path does not correlate with v2")
            local = asset_dir / filename
            if not local.is_file() or local.is_symlink():
                fail(f"{v2_path}: {target}: local asset must be a regular file")
            size, digest = hash_file(local)
            if size != asset["size"]:
                fail(f"{v2_path}: {target}: size mismatch")
            if digest != asset["sha256"]:
                fail(f"{v2_path}: {target}: checksum mismatch")
            if sums.get(filename) != digest:
                fail(f"{staged / 'SHA256SUMS'}: {filename}: manifest/checksum mismatch")
            expected_local_assets.add(filename)
            object_rows.append((expected_path, local))
        object_rows.extend(
            (
                (f"{binary}/manifests/v2/{version}.json", v2_path),
                (f"{binary}/manifests/v1/{version}.json", v1_path),
            )
        )
    actual_v2 = {path.name for path in manifest_dir.glob("*.v2.candidate.json")}
    actual_v1 = {path.name for path in manifest_dir.glob("*.v1.candidate.json")}
    if actual_v2 != {f"{binary}.v2.candidate.json" for binary in TARGET_SETS}:
        fail("strict candidate manifest set mismatch")
    if actual_v1 != {f"{binary}.v1.candidate.json" for binary in TARGET_SETS}:
        fail("legacy candidate manifest set mismatch")
    actual_assets = {
        path.name for path in asset_dir.iterdir() if path.is_file() and not path.is_symlink()
    }
    if actual_assets != expected_local_assets or set(sums) != expected_local_assets:
        fail("release assets, manifests, and SHA256SUMS do not describe the same exact set")
    if len(object_rows) != len({row[0] for row in object_rows}):
        fail("duplicate R2 immutable object path")
    plan_path.write_text(
        "".join(f"{key}\t{local}\n" for key, local in sorted(object_rows)),
        encoding="utf-8",
    )


def main() -> None:
    if len(sys.argv) != 6:
        fail(
            "usage: validate-promotion-stage.py VERSION STAGED_DIR "
            "EXPECTED_SOURCE_SHA CORRELATION_ID PLAN_PATH"
        )
    validate_stage(
        sys.argv[1],
        pathlib.Path(sys.argv[2]).resolve(),
        sys.argv[3],
        sys.argv[4],
        pathlib.Path(sys.argv[5]),
    )


if __name__ == "__main__":
    main()
