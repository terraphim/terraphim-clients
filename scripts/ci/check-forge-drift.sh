#!/usr/bin/env bash
# Fail when two git refs differ on a set of paths.
#
# Usage: check-forge-drift.sh REF_A REF_B [PATH...]
#
# Defaults to the reconciliation scope (.github scripts). Used by the
# forge-mirror workflow to prove GitHub and the Gitea mirror are identical
# (Gitea terraphim-clients#342).
set -euo pipefail

if [ "$#" -lt 2 ]; then
    echo "usage: check-forge-drift.sh REF_A REF_B [PATH...]" >&2
    exit 2
fi

ref_a="$1"; shift
ref_b="$1"; shift
paths=("$@")
if [ "${#paths[@]}" -eq 0 ]; then
    paths=(.github scripts)
fi

tmp_a="$(mktemp)"; tmp_b="$(mktemp)"
trap 'rm -f "$tmp_a" "$tmp_b"' EXIT

git ls-tree -r "$ref_a" -- "${paths[@]}" | sort > "$tmp_a"
git ls-tree -r "$ref_b" -- "${paths[@]}" | sort > "$tmp_b"

if diff -u "$tmp_a" "$tmp_b"; then
    echo "forge drift: ${ref_a} == ${ref_b} on: ${paths[*]}"
    exit 0
fi

echo "forge drift: ${ref_a} and ${ref_b} differ on: ${paths[*]}" >&2
echo "A Gitea-side change must be mirrored to GitHub first (or vice versa)." >&2
exit 1
