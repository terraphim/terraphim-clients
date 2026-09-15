#!/usr/bin/env python3
"""Validate and extract review-bound, prebuilt release inputs."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
import tarfile


MAX_BINARY_BYTES = 512 * 1024 * 1024
BINARIES = ("terraphim-agent", "terraphim-cli", "terraphim-grep")
UNIX_TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
)


class ValidationError(ValueError):
    """Raised when a release input bundle violates its reviewed contract."""


def expected_binary_names() -> set[str]:
    """Return the exact supported release-input inventory."""
    names = {
        f"{binary}-{target}"
        for binary in BINARIES
        for target in UNIX_TARGETS
    }
    names.update(
        f"{binary}-x86_64-pc-windows-msvc.exe" for binary in BINARIES
    )
    return names


def load_expected_hashes(contract_path: pathlib.Path) -> dict[str, str]:
    """Load and structurally validate binary hashes from a release contract."""
    contract = json.loads(contract_path.read_text())
    binaries = contract.get("binaries")
    if not isinstance(binaries, list):
        raise ValidationError("contract binaries must be a list")
    expected: dict[str, str] = {}
    for item in binaries:
        if not isinstance(item, dict):
            raise ValidationError("contract binary entries must be objects")
        name = item.get("name")
        digest = item.get("sha256")
        if not isinstance(name, str) or not isinstance(digest, str):
            raise ValidationError("contract binary name and sha256 must be strings")
        if name in expected:
            raise ValidationError(f"duplicate contract binary: {name}")
        if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise ValidationError(f"invalid SHA-256 for contract binary: {name}")
        expected[name] = digest
    if set(expected) != expected_binary_names():
        missing = sorted(expected_binary_names() - set(expected))
        extra = sorted(set(expected) - expected_binary_names())
        raise ValidationError(f"contract inventory mismatch: missing={missing}, extra={extra}")
    return expected


def validate_and_extract(
    contract_path: pathlib.Path,
    archive_path: pathlib.Path,
    destination: pathlib.Path,
) -> None:
    """Validate an archive against its contract and extract regular files only."""
    expected = load_expected_hashes(contract_path)
    if destination.exists():
        raise ValidationError(f"destination already exists: {destination}")
    destination.mkdir(parents=True)

    with tarfile.open(archive_path, "r:gz") as bundle:
        members = bundle.getmembers()
        names = [member.name for member in members]
        if len(names) != len(set(names)):
            raise ValidationError("duplicate staging archive member")
        if set(names) != set(expected):
            missing = sorted(set(expected) - set(names))
            extra = sorted(set(names) - set(expected))
            raise ValidationError(f"staging inventory mismatch: missing={missing}, extra={extra}")
        for member in members:
            if not member.isfile():
                raise ValidationError(f"non-regular staging member rejected: {member.name}")
            if member.size <= 0 or member.size > MAX_BINARY_BYTES:
                raise ValidationError(f"invalid staging member size: {member.name}")
            source = bundle.extractfile(member)
            if source is None:
                raise ValidationError(f"cannot read staging member: {member.name}")
            target = destination / member.name
            digest = hashlib.sha256()
            with source, target.open("xb") as output:
                while chunk := source.read(1024 * 1024):
                    digest.update(chunk)
                    output.write(chunk)
            target.chmod(0o755)
            if digest.hexdigest() != expected[member.name]:
                raise ValidationError(f"digest mismatch: {member.name}")


def main() -> int:
    """Run the command-line validator."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--contract", type=pathlib.Path, required=True)
    parser.add_argument("--archive", type=pathlib.Path, required=True)
    parser.add_argument("--destination", type=pathlib.Path, required=True)
    args = parser.parse_args()
    try:
        validate_and_extract(args.contract, args.archive, args.destination)
    except (OSError, json.JSONDecodeError, tarfile.TarError, ValidationError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
