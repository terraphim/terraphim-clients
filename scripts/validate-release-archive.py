#!/usr/bin/env python3
"""Fail-closed layout, mode, filename, and architecture validation."""

from __future__ import annotations

import pathlib
import re
import stat
import sys
import tarfile
import tempfile
import zipfile
from typing import NoReturn

from validate_release_binary import validate_binary


COMMON_TARGETS = {
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
}
EXPECTED_TARGETS = {
    "terraphim-agent": COMMON_TARGETS | {"universal-apple-darwin"},
    "terraphim-grep": COMMON_TARGETS | {"universal-apple-darwin"},
    "terraphim-cli": COMMON_TARGETS,
}


def fail(message: str) -> NoReturn:
    raise SystemExit(f"ERROR: {message}")


def identify(filename: str, version: str) -> tuple[str, str, str]:
    for binary, targets in EXPECTED_TARGETS.items():
        prefix = f"{binary}-{version}-"
        if not filename.startswith(prefix):
            continue
        remainder = filename.removeprefix(prefix)
        if remainder.endswith(".tar.gz"):
            target, extension = remainder.removesuffix(".tar.gz"), ".tar.gz"
        elif remainder.endswith(".zip"):
            target, extension = remainder.removesuffix(".zip"), ".zip"
        else:
            fail(f"unsupported archive extension: {filename}")
        if target not in targets:
            fail(f"unsupported target {target!r} for {binary}")
        expected_extension = ".zip" if target == "x86_64-pc-windows-msvc" else ".tar.gz"
        if extension != expected_extension:
            fail(f"wrong archive extension for {target}: {extension}")
        return binary, target, extension
    fail(f"filename does not encode a supported binary/version: {filename}")


def validate_tar(archive: pathlib.Path, executable: str) -> bytes:
    with tarfile.open(archive, "r:gz") as bundle:
        members = bundle.getmembers()
        names = [member.name for member in members]
        if any(pathlib.PurePosixPath(name).is_absolute() or ".." in pathlib.PurePosixPath(name).parts for name in names):
            fail(f"{archive.name}: unsafe archive member path")
        if len(names) != len(set(names)):
            fail(f"{archive.name}: duplicate archive members")
        expected = {executable, "LICENSE-Apache-2.0", "LICENSE-MIT"}
        if set(names) != expected:
            fail(f"{archive.name}: layout {set(names)!r} != {expected!r}")
        by_name = {member.name: member for member in members}
        if any(not member.isfile() or member.size <= 0 for member in members):
            fail(f"{archive.name}: every member must be a non-empty regular file")
        if stat.S_IMODE(by_name[executable].mode) != 0o755:
            fail(f"{archive.name}: executable mode must be 0755")
        for license_name in ("LICENSE-Apache-2.0", "LICENSE-MIT"):
            if stat.S_IMODE(by_name[license_name].mode) != 0o644:
                fail(f"{archive.name}: license mode must be 0644")
        extracted = bundle.extractfile(by_name[executable])
        if extracted is None:
            fail(f"{archive.name}: executable could not be read")
        return extracted.read()


def validate_zip(archive: pathlib.Path, executable: str) -> bytes:
    with zipfile.ZipFile(archive) as bundle:
        infos = bundle.infolist()
        names = [info.filename for info in infos]
        if any(pathlib.PurePosixPath(name).is_absolute() or ".." in pathlib.PurePosixPath(name).parts for name in names):
            fail(f"{archive.name}: unsafe archive member path")
        if len(names) != len(set(names)):
            fail(f"{archive.name}: duplicate archive members")
        expected = {executable, "LICENSE-Apache-2.0", "LICENSE-MIT"}
        if set(names) != expected:
            fail(f"{archive.name}: layout {set(names)!r} != {expected!r}")
        by_name = {info.filename: info for info in infos}
        if any(info.create_system != 3 or info.is_dir() or info.file_size <= 0 for info in infos):
            fail(f"{archive.name}: every member must be a non-empty regular file")
        for name, info in by_name.items():
            raw_mode = info.external_attr >> 16
            if stat.S_IFMT(raw_mode) != stat.S_IFREG:
                fail(f"{archive.name}: {name}: member must be a regular file")
            expected_mode = 0o755 if name == executable else 0o644
            if stat.S_IMODE(raw_mode) != expected_mode:
                fail(f"{archive.name}: {name}: mode must be {expected_mode:04o}")
        return bundle.read(executable)


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: validate-release-archive.py VERSION ARCHIVE")
    version, archive_arg = sys.argv[1:]
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
        fail(f"invalid stable version {version!r}")
    archive = pathlib.Path(archive_arg)
    if not archive.is_file() or archive.is_symlink() or archive.stat().st_size <= 0:
        fail(f"archive is not a non-empty regular file: {archive}")
    binary, target, extension = identify(archive.name, version)
    executable = binary + (".exe" if extension == ".zip" else "")
    payload = (
        validate_zip(archive, executable)
        if extension == ".zip"
        else validate_tar(archive, executable)
    )
    with tempfile.TemporaryDirectory() as directory:
        path = pathlib.Path(directory) / executable
        path.write_bytes(payload)
        path.chmod(0o755)
        validate_binary(path, target)


if __name__ == "__main__":
    main()
