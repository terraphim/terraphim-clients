#!/usr/bin/env bash
#
# Generate a per-binary release manifest (stable.json) for the R2 backend.
#
# Usage:
#   scripts/build-manifest.sh <version> <bin_name> <artifacts_dir>
#
# Emits the manifest on stdout. The assets map is built from every file
# matching <bin_name>-<version>-<target>.tar.gz in <artifacts_dir>; the target
# triple is extracted and mapped to "bin/<filename>" (the R2 object key).
#
set -euo pipefail

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
