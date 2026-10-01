#!/usr/bin/env bash
#
# Build the committed memory benchmark fixture (terraphim-clients#259, step 1
# of #255) from a directory of captured learnings.
#
# Usage:
#   scripts/build_memory_fixture.sh [learnings_dir] [out_dir]
#
#   learnings_dir  directory holding learning-*.md and correction-*.md files;
#                  required as $1 or via TERRAPHIM_LEARNINGS_DIR (no default:
#                  the capture directory is private and must be named
#                  explicitly)
#   out_dir        where corpus.jsonl and queries.jsonl are written
#                  (default: crates/terraphim_agent/tests/fixtures/memory_bench)
#
# The selection, redaction and query derivation live in
# crates/terraphim_agent/examples/build_memory_fixture.rs so that the capture
# module's own redactor (terraphim_agent::learnings::redact_secrets) is
# applied. This wrapper builds that example, runs it, and prints the SHA-256
# of corpus.jsonl for the fixture README.
#
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
learnings_dir="${1:-${TERRAPHIM_LEARNINGS_DIR:-}}"
out_dir="${2:-$repo_root/crates/terraphim_agent/tests/fixtures/memory_bench}"

if [ -z "$learnings_dir" ]; then
    echo "usage: scripts/build_memory_fixture.sh <learnings_dir> [out_dir]" >&2
    echo "       (or set TERRAPHIM_LEARNINGS_DIR)" >&2
    exit 2
fi
if [ ! -d "$learnings_dir" ]; then
    echo "learnings directory not found: $learnings_dir" >&2
    exit 1
fi

cd "$repo_root"
cargo run --quiet --locked -p terraphim_agent --example build_memory_fixture -- \
    "$learnings_dir" "$out_dir"

corpus="$out_dir/corpus.jsonl"
if command -v shasum >/dev/null 2>&1; then
    hash="$(shasum -a 256 "$corpus" | cut -d' ' -f1)"
else
    hash="$(sha256sum "$corpus" | cut -d' ' -f1)"
fi
echo "corpus.jsonl SHA-256: $hash"
echo "Record that value in $out_dir/README.md; tests/memory_fixture_integrity.rs asserts it."
