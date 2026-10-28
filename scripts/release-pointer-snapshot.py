#!/usr/bin/env python3
"""Create or validate the retained pre-promotion stable-pointer snapshot."""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import sys
from typing import NoReturn


BINARIES = ("terraphim-agent", "terraphim-cli", "terraphim-grep")
POINTERS = tuple(
    f"{binary}/{name}"
    for binary in BINARIES
    for name in ("stable.json", "stable-v2.json")
)


def fail(message: str) -> NoReturn:
    raise SystemExit(f"ERROR: {message}")


def reject_duplicates(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def digest(path: pathlib.Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            value.update(chunk)
    return value.hexdigest()


def expected_identity(version: str, source_sha: str, correlation_id: str) -> dict:
    if re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version) is None:
        fail("invalid version")
    if re.fullmatch(r"[0-9a-f]{40}", source_sha) is None:
        fail("invalid source SHA")
    if re.fullmatch(r"[A-Za-z0-9._:/@+-]{1,128}", correlation_id) is None:
        fail("invalid correlation ID")
    return {
        "correlation_id": correlation_id,
        "source_sha": source_sha,
        "stage_identity": f"client-release-stage-{version}-{source_sha}",
        "version": version,
    }


def create(
    version: str,
    source_sha: str,
    correlation_id: str,
    snapshot: pathlib.Path,
    rows_path: pathlib.Path,
) -> None:
    rows = {}
    for line in rows_path.read_text(encoding="utf-8").splitlines():
        fields = line.split("\t")
        if len(fields) != 3 or fields[0] in rows:
            fail("pointer capture rows are malformed or duplicated")
        rows[fields[0]] = (fields[1], fields[2])
    if set(rows) != set(POINTERS):
        fail("pointer capture rows are not the exact pointer set")
    pointers = {}
    for object_path in POINTERS:
        state, relative = rows[object_path]
        if state == "absent" and relative == "-":
            pointers[object_path] = {"file": None, "present": False, "sha256": None}
            continue
        expected_relative = f"objects/{object_path}"
        if state != "present" or relative != expected_relative:
            fail(f"{object_path}: invalid captured state")
        local = snapshot / relative
        if not local.is_file() or local.is_symlink():
            fail(f"{object_path}: captured bytes are not a regular file")
        pointers[object_path] = {
            "file": relative,
            "present": True,
            "sha256": digest(local),
        }
    state = expected_identity(version, source_sha, correlation_id) | {"pointers": pointers}
    (snapshot / "state.json").write_text(
        json.dumps(state, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def validate(
    version: str,
    source_sha: str,
    correlation_id: str,
    snapshot: pathlib.Path,
    plan_path: pathlib.Path,
) -> None:
    state_path = snapshot / "state.json"
    if not state_path.is_file() or state_path.is_symlink():
        fail("rollback pointer snapshot has no regular state.json")
    try:
        state = json.loads(
            state_path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates
        )
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as error:
        fail(f"invalid rollback pointer state: {error}")
    expected = expected_identity(version, source_sha, correlation_id)
    if not isinstance(state, dict) or set(state) != set(expected) | {"pointers"}:
        fail("rollback pointer state keys are not exact")
    for key, value in expected.items():
        if state[key] != value:
            fail(f"rollback pointer state {key} mismatch")
    pointers = state["pointers"]
    if not isinstance(pointers, dict) or set(pointers) != set(POINTERS):
        fail("rollback pointer state does not contain the exact pointer set")
    plan = []
    for object_path in POINTERS:
        item = pointers[object_path]
        if not isinstance(item, dict) or set(item) != {"file", "present", "sha256"}:
            fail(f"{object_path}: snapshot item keys are not exact")
        if item["present"] is False:
            if item["file"] is not None or item["sha256"] is not None:
                fail(f"{object_path}: absent snapshot item carries bytes")
            plan.append((object_path, "absent", "-"))
            continue
        expected_relative = f"objects/{object_path}"
        if item["present"] is not True or item["file"] != expected_relative:
            fail(f"{object_path}: invalid present snapshot item")
        if not isinstance(item["sha256"], str) or re.fullmatch(
            r"[0-9a-f]{64}", item["sha256"]
        ) is None:
            fail(f"{object_path}: invalid retained digest")
        local = snapshot / expected_relative
        if not local.is_file() or local.is_symlink() or digest(local) != item["sha256"]:
            fail(f"{object_path}: retained pointer bytes differ from state")
        plan.append((object_path, "present", str(local)))
    plan_path.write_text(
        "".join(f"{path}\t{state_name}\t{local}\n" for path, state_name, local in plan),
        encoding="utf-8",
    )


def main() -> None:
    if len(sys.argv) != 7 or sys.argv[1] not in {"create", "validate"}:
        fail(
            "usage: release-pointer-snapshot.py create|validate VERSION SOURCE_SHA "
            "CORRELATION_ID SNAPSHOT_DIR ROWS_OR_PLAN_PATH"
        )
    mode, version, source_sha, correlation_id, snapshot, path = (
        sys.argv[1],
        sys.argv[2],
        sys.argv[3],
        sys.argv[4],
        pathlib.Path(sys.argv[5]),
        pathlib.Path(sys.argv[6]),
    )
    if mode == "create":
        create(version, source_sha, correlation_id, snapshot, path)
    else:
        validate(version, source_sha, correlation_id, snapshot, path)


if __name__ == "__main__":
    main()
