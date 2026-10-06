#!/usr/bin/env python3
"""Validate a terraphim_mcp_server release tag against the crate version.

Usage: check-tag.py TAG METADATA_JSON

TAG must be `terraphim_mcp_server-v<version>`. METADATA_JSON is the output of
`cargo metadata --no-deps --format-version 1`. The tag version must equal the
crate version, or be that version with a semantic-version pre-release suffix
(for example `1.21.18-rc.1`), because the pre-release channel tags release
candidates of a crate version before it is final. Prints `crate_version=<v>`
and `version=<tag version>` on success.
"""

from __future__ import annotations

import json
import re
import sys

PACKAGE = "terraphim_mcp_server"
TAG_PATTERN = re.compile(
    rf"^{PACKAGE}-v(?P<version>(?P<core>(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))"
    r"(?:-(?P<pre>(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?)$"
)
# SemVer 2.0.0 without build metadata: numeric pre-release identifiers have no
# leading zeros and no identifier is empty. The workflow's shell guard is only
# a coarse character filter; this is the authoritative grammar.


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    tag, metadata_path = argv[1], argv[2]
    match = TAG_PATTERN.fullmatch(tag)
    if match is None:
        print(f"ERROR: tag {tag!r} is not {PACKAGE}-v<semver>", file=sys.stderr)
        return 1
    with open(metadata_path, encoding="utf-8") as handle:
        packages = {p["name"]: p["version"] for p in json.load(handle)["packages"]}
    crate_version = packages.get(PACKAGE)
    if crate_version is None:
        print(f"ERROR: package {PACKAGE} not found in cargo metadata", file=sys.stderr)
        return 1
    if match.group("core") != crate_version:
        print(
            f"ERROR: tag version {match.group('version')!r} does not match "
            f"crate version {crate_version!r}",
            file=sys.stderr,
        )
        return 1
    print(f"crate_version={crate_version}")
    print(f"version={match.group('version')}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
