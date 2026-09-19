#!/usr/bin/env python3
"""Derive a deterministic pre-1.21.15 manifest from a strict v2 candidate."""

from __future__ import annotations

import json
import os
import pathlib
import re
import sys
import tempfile
from typing import NoReturn


def fail(message: str) -> NoReturn:
    raise SystemExit(f"ERROR: {message}")


def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: build-legacy-manifest.py STRICT_INPUT LEGACY_OUTPUT")
    source = pathlib.Path(sys.argv[1])
    output = pathlib.Path(sys.argv[2])
    if not source.is_file() or source.is_symlink():
        fail("strict candidate must be a regular file")
    if output.name in {"stable.json", "stable-v2.json"}:
        fail("candidate builder refuses to write a live stable pointer")
    try:
        strict = json.loads(source.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(f"invalid strict candidate: {error}")
    if set(strict) != {"assets", "notes_url", "released_at", "version"}:
        fail("strict candidate top-level keys are not exact")
    if not isinstance(strict["version"], str) or not re.fullmatch(
        r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", strict["version"]
    ):
        fail("invalid stable version")
    if not isinstance(strict["assets"], dict) or not strict["assets"]:
        fail("strict candidate assets must be a non-empty object")
    legacy_assets: dict[str, str] = {}
    for target, asset in sorted(strict["assets"].items()):
        if not isinstance(target, str) or not isinstance(asset, dict):
            fail("invalid strict asset entry")
        if set(asset) != {"path", "sha256", "size"}:
            fail(f"{target}: strict asset keys are not exact")
        path = asset["path"]
        digest = asset["sha256"]
        size = asset["size"]
        if not isinstance(path, str) or path.startswith("/") or ".." in pathlib.PurePosixPath(path).parts:
            fail(f"{target}: unsafe asset path")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            fail(f"{target}: invalid sha256")
        if isinstance(size, bool) or not isinstance(size, int) or size <= 0:
            fail(f"{target}: invalid size")
        legacy_assets[target] = path
    legacy = {
        "assets": legacy_assets,
        "notes_url": strict["notes_url"],
        "released_at": strict["released_at"],
        "version": strict["version"],
    }
    encoded = (json.dumps(legacy, sort_keys=True, indent=2) + "\n").encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary_name = tempfile.mkstemp(dir=output.parent, prefix=f".{output.name}.")
    temporary = pathlib.Path(temporary_name)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary, 0o644)
        os.replace(temporary, output)
    finally:
        temporary.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
