#!/usr/bin/env bash
#
# Build a deterministic, integrity-bearing release manifest.
#
# Two forms, selected by the trailing argument:
#
#   Legacy stdout (3 args) -- used by the v1.21.14 release finalizer
#     (scripts/sign-macos-binary.sh and .github/workflows/finalize-prebuilt-release.yml):
#       scripts/build-manifest.sh <version> <bin_name> <artifacts_dir> > stable.json
#
#     Emits the v1 (path-only assets) manifest on stdout. The asset map is the
#     seven unix targets (aarch64 + x86_64 + universal Apple, three Linux
#     musl/gnu). Windows is omitted on purpose: the tagged v1.21.14 updater
#     cannot verify ZIP signatures, so the finalizer deliberately keeps
#     Windows manifests blank until a later client release restores signed
#     Windows automatic updates. Fail-closed: every required asset must
#     exist; any missing target exits non-zero so CI never publishes a
#     partial manifest.
#
#   Strict candidate (4 args) -- used by the v1.21.16 release producer
#     (.github/workflows/release-binaries.yml seal-release-stage):
#       scripts/build-manifest.sh <version> <bin_name> <artifacts_dir> <output.candidate.json>
#
#     Writes a v2 (object-valued assets with sha256 + size) candidate to the
#     fourth argument. The candidate schema is strict: every advertised
#     target for the binary must be present and well-formed (Windows zip
#     included for agent and grep, omitted for cli), no extra or wrong-version
#     archives may appear, and the filename must encode the exact version
#     and target. The candidate builder refuses to overwrite a stable
#     pointer (stable.json / stable-v2.json) -- stable promotion is a
#     separately authorized promote-release.sh operation.
#
# SOURCE_DATE_EPOCH is mandatory in the 4-arg mode (deterministic
# released_at); the 3-arg legacy mode uses the current wall clock because the
# finalizer captures the published manifest's released_at at promotion time,
# not at build time.
#
set -euo pipefail

usage() {
    cat >&2 <<'EOF'
Usage:
  scripts/build-manifest.sh VERSION BINARY ARTIFACTS_DIR
      Emit the v1 (legacy stdout) manifest -- used by the v1.21.14
      release finalizer.
  scripts/build-manifest.sh VERSION BINARY ARTIFACTS_DIR OUTPUT.candidate.json
      Write a v2 strict candidate manifest -- used by the v1.21.16
      producer's seal-release-stage step.
EOF
    exit 2
}

if [ "$#" -eq 3 ]; then
    legacy_mode=true
elif [ "$#" -eq 4 ]; then
    legacy_mode=false
else
    usage
fi

if [ "$legacy_mode" = true ]; then
    version="$1"
    bin="$2"
    artifacts_dir="$3"

    release_url="https://github.com/terraphim/terraphim-clients/releases/tag/v${version}"

    unix_targets=(
        aarch64-apple-darwin
        x86_64-apple-darwin
        universal-apple-darwin
        x86_64-unknown-linux-gnu
        x86_64-unknown-linux-musl
        aarch64-unknown-linux-musl
    )

    assets=""
    for target in "${unix_targets[@]}"; do
        filename="${bin}-${version}-${target}.tar.gz"
        [ -f "$artifacts_dir/$filename" ] || {
            echo "ERROR: missing manifest asset: $artifacts_dir/$filename" >&2
            exit 1
        }
        entry=$(printf '    "%s": "%s/%s"' "$target" "$bin" "$filename")
        if [ -n "$assets" ]; then
            assets="$assets,"$'\n'
        fi
        assets="$assets$entry"
    done

    cat <<EOF
{
  "version": "${version}",
  "released_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "assets": {
${assets}
  },
  "notes_url": "${release_url}"
}
EOF
    exit 0
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
