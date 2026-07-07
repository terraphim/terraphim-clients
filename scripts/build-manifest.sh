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

# Build the assets JSON object: { "<target>": "<bin>/<filename>", ... }
assets=$(cd "$artifacts_dir" && ls -1 "${bin}-${version}-"*.tar.gz 2>/dev/null | while read -r f; do
    # strip prefix "<bin>-<version>-" and suffix ".tar.gz" to get the target
    tgt="${f#"${bin}-${version}-"}"
    tgt="${tgt%.tar.gz}"
    # escape for JSON
    printf '    "%s": "%s/%s"' "$tgt" "$bin" "$f"
done | paste -sd, -)

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
