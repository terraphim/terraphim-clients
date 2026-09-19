#!/usr/bin/env python3
"""Create a byte-reproducible ZIP from an explicit ordered member list."""

from __future__ import annotations

import datetime
import os
import pathlib
import stat
import sys
import tempfile
import zipfile
from typing import NoReturn


def fail(message: str) -> NoReturn:
    raise SystemExit(f"ERROR: {message}")


def main() -> None:
    if len(sys.argv) < 6:
        fail(
            "usage: create-deterministic-zip.py SOURCE_DATE_EPOCH ROOT OUTPUT MEMBER..."
        )
    epoch_arg, root_arg, output_arg, *members = sys.argv[1:]
    try:
        epoch = int(epoch_arg)
        timestamp = datetime.datetime.fromtimestamp(epoch, datetime.timezone.utc)
    except (ValueError, OverflowError, OSError) as error:
        fail(f"invalid SOURCE_DATE_EPOCH: {error}")
    if timestamp.year < 1980:
        fail("ZIP timestamps require SOURCE_DATE_EPOCH in 1980 or later")

    root = pathlib.Path(root_arg)
    output = pathlib.Path(output_arg)
    if not root.is_dir() or output.name in members:
        fail("invalid package root or output path")
    if len(members) != len(set(members)):
        fail("duplicate ZIP member names")

    inputs = []
    for member in members:
        pure = pathlib.PurePosixPath(member)
        if pure.is_absolute() or len(pure.parts) != 1 or pure.name != member:
            fail(f"unsafe ZIP member name: {member!r}")
        path = root / member
        if not path.is_file() or path.is_symlink() or path.stat().st_size <= 0:
            fail(f"member must be a non-empty regular file: {member}")
        inputs.append((member, path))

    output.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary_arg = tempfile.mkstemp(
        dir=output.parent, prefix=f".{output.name}.", suffix=".tmp"
    )
    os.close(fd)
    temporary = pathlib.Path(temporary_arg)
    try:
        date_time = timestamp.timetuple()[:6]
        with zipfile.ZipFile(
            temporary,
            "w",
            compression=zipfile.ZIP_DEFLATED,
            compresslevel=9,
            strict_timestamps=True,
        ) as bundle:
            for member, path in inputs:
                mode = stat.S_IMODE(path.stat().st_mode)
                info = zipfile.ZipInfo(member, date_time=date_time)
                info.create_system = 3
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (stat.S_IFREG | mode) << 16
                bundle.writestr(
                    info,
                    path.read_bytes(),
                    compress_type=zipfile.ZIP_DEFLATED,
                    compresslevel=9,
                )
        with temporary.open("rb") as handle:
            os.fsync(handle.fileno())
        os.chmod(temporary, 0o644)
        os.replace(temporary, output)
        directory_fd = os.open(output.parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    finally:
        temporary.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
