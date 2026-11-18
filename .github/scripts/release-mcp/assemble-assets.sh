#!/usr/bin/env bash
# Assemble the terraphim_mcp_server release assets and checksums.txt.
#
# Usage: assemble-assets.sh INPUT_DIR OUTPUT_DIR
#
# INPUT_DIR must contain exactly the six raw binaries (any sub-directories from
# artifact download are flattened); OUTPUT_DIR receives them with mode 0755 and
# a checksums.txt generated with `sha256sum -- *` over exactly those files.
# Fails closed on a missing, extra or empty file.
set -euo pipefail

input_dir="${1:?input directory required}"
output_dir="${2:?output directory required}"

expected=(
  terraphim_mcp_server-aarch64-apple-darwin
  terraphim_mcp_server-aarch64-unknown-linux-musl
  terraphim_mcp_server-universal-apple-darwin
  terraphim_mcp_server-x86_64-apple-darwin
  terraphim_mcp_server-x86_64-unknown-linux-gnu
  terraphim_mcp_server-x86_64-unknown-linux-musl
)

mkdir -p "$output_dir/assets"
# Flatten: artifact downloads place each file under its artifact directory.
shopt -s globstar nullglob dotglob
for file in "$input_dir"/**; do
  [ -f "$file" ] || continue
  cp -- "$file" "$output_dir/assets/$(basename -- "$file")"
done
shopt -u globstar nullglob dotglob

actual="$(cd "$output_dir/assets" && printf '%s\n' * | LC_ALL=C sort)"
want="$(printf '%s\n' "${expected[@]}" | LC_ALL=C sort)"
if [ "$actual" != "$want" ]; then
  echo "ERROR: asset set mismatch" >&2
  echo "expected:" >&2; printf '  %s\n' "${expected[@]}" >&2
  echo "actual:" >&2; printf '%s\n' "$actual" | sed 's/^/  /' >&2
  exit 1
fi

for name in "${expected[@]}"; do
  test -s "$output_dir/assets/$name" || { echo "ERROR: $name is empty" >&2; exit 1; }
  chmod 0755 "$output_dir/assets/$name"
done

# The glob runs inside the assets directory; checksums.txt is written outside
# it so it can never list itself.
(cd "$output_dir/assets" && sha256sum -- *) > "$output_dir/checksums.txt"
(cd "$output_dir/assets" && sha256sum -c ../checksums.txt)
