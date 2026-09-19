#!/usr/bin/env bash
# Build a deterministic, integrity-bearing candidate release manifest.
#
# Usage: build-manifest.sh VERSION BINARY ARTIFACTS_DIR OUTPUT.candidate.json
#
# This script deliberately cannot write stable.json or stable-v2.json. Stable
# promotion is a separately authorized operation performed by promote-release.sh.
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 VERSION BINARY ARTIFACTS_DIR OUTPUT.candidate.json" >&2
  exit 2
fi

: "${SOURCE_DATE_EPOCH:?SOURCE_DATE_EPOCH must identify the immutable source timestamp}"

python3 - "$1" "$2" "$3" "$4" <<'PY'
import datetime
import hashlib
import json
import os
import pathlib
import re
import sys
import tempfile

version, binary, artifacts_arg, output_arg = sys.argv[1:]
artifacts_dir = pathlib.Path(artifacts_arg)
output = pathlib.Path(output_arg)

if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
    sys.exit(f"invalid stable version: {version!r}")

common_targets = {
    "aarch64-apple-darwin": ".tar.gz",
    "aarch64-unknown-linux-musl": ".tar.gz",
    "x86_64-apple-darwin": ".tar.gz",
    "x86_64-pc-windows-msvc": ".zip",
    "x86_64-unknown-linux-gnu": ".tar.gz",
    "x86_64-unknown-linux-musl": ".tar.gz",
}
target_sets = {
    "terraphim-agent": {**common_targets, "universal-apple-darwin": ".tar.gz"},
    "terraphim-grep": {**common_targets, "universal-apple-darwin": ".tar.gz"},
    "terraphim-cli": common_targets,
}
if binary not in target_sets:
    sys.exit(f"unsupported release binary: {binary!r}")
if not artifacts_dir.is_dir():
    sys.exit(f"artifacts directory does not exist: {artifacts_dir}")
if output.name in {"stable.json", "stable-v2.json"}:
    sys.exit("candidate builder refuses to replace a stable pointer; use authorized promotion")

expected = target_sets[binary]
assets = {}
consumed = set()
for target, suffix in sorted(expected.items()):
    filename = f"{binary}-{version}-{target}{suffix}"
    path = artifacts_dir / filename
    if not path.is_file() or path.is_symlink():
        sys.exit(f"missing required regular artifact: {filename}")
    size = path.stat().st_size
    if size <= 0:
        sys.exit(f"artifact is empty: {filename}")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    assets[target] = {
        "path": f"{binary}/{filename}",
        "sha256": digest,
        "size": size,
    }
    consumed.add(filename)

# Any other archive bearing this binary's prefix is a duplicate target,
# wrong-version artifact, wrong extension, or unsupported target. Reject the
# whole candidate rather than silently publishing an open-ended set.
own_archives = {
    path.name
    for path in artifacts_dir.iterdir()
    if path.is_file()
    and path.name.startswith(f"{binary}-")
    and (path.name.endswith(".tar.gz") or path.name.endswith(".zip"))
}
extras = sorted(own_archives - consumed)
if extras:
    sys.exit(f"unexpected or duplicate artifacts for {binary}: {', '.join(extras)}")

try:
    epoch = int(os.environ["SOURCE_DATE_EPOCH"])
    released_at = datetime.datetime.fromtimestamp(
        epoch, datetime.timezone.utc
    ).strftime("%Y-%m-%dT%H:%M:%SZ")
except (KeyError, ValueError, OverflowError, OSError) as error:
    sys.exit(f"invalid SOURCE_DATE_EPOCH: {error}")

manifest = {
    "version": version,
    "released_at": released_at,
    "assets": assets,
    "notes_url": (
        "https://github.com/terraphim/terraphim-clients/releases/tag/"
        f"v{version}"
    ),
}
encoded = (
    json.dumps(manifest, sort_keys=True, indent=2, ensure_ascii=False) + "\n"
).encode("utf-8")

output.parent.mkdir(parents=True, exist_ok=True)
fd, temporary_name = tempfile.mkstemp(
    dir=output.parent, prefix=f".{output.name}.", suffix=".tmp"
)
temporary = pathlib.Path(temporary_name)
try:
    with os.fdopen(fd, "wb") as handle:
        handle.write(encoded)
        handle.flush()
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
PY
