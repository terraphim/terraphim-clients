#!/usr/bin/env python3
"""Validate and stage the exact canonical Linux binary bytes without mutation."""

from __future__ import annotations

import hashlib
import os
import pathlib
import shutil
import sys
import tempfile

from validate_release_binary import validate_binary


BINARIES = ("terraphim-agent", "terraphim-cli", "terraphim-grep")
TARGETS = (
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
)


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit("usage: stage-canonical-linux.py SOURCE_DIR OUTPUT_DIR SUMS_FILE")
    source_dir, output_dir, sums_path = map(pathlib.Path, sys.argv[1:])
    inputs = []
    for target in TARGETS:
        for binary in BINARIES:
            source = source_dir / f"{binary}-{target}"
            validate_binary(source, target)
            inputs.append(source)
    if output_dir.exists():
        raise SystemExit(f"ERROR: output already exists: {output_dir}")
    output_dir.parent.mkdir(parents=True, exist_ok=True)
    temporary = pathlib.Path(tempfile.mkdtemp(prefix=f".{output_dir.name}.", dir=output_dir.parent))
    try:
        rows = []
        for source in inputs:
            destination = temporary / source.name
            shutil.copyfile(source, destination)
            if source.read_bytes() != destination.read_bytes():
                raise SystemExit(f"ERROR: canonical copy differs: {source.name}")
            digest = hashlib.sha256(destination.read_bytes()).hexdigest()
            rows.append(f"{digest}  {destination.name}\n")
        os.replace(temporary, output_dir)
        temporary = pathlib.Path()
        sums_path.write_text("".join(sorted(rows)), encoding="utf-8")
    finally:
        if temporary != pathlib.Path() and temporary.exists():
            shutil.rmtree(temporary)


if __name__ == "__main__":
    main()
