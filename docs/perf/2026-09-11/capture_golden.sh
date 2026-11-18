#!/usr/bin/env bash
# Capture the normalised golden outputs for the #253 workloads.
#
#   bash docs/perf/2026-09-11/capture_golden.sh            # capture and compare
#   bash docs/perf/2026-09-11/capture_golden.sh --update   # capture and bless
#
# Normalisation drops two things from the captured output:
#
#   - the self-reported "Search latency:" line, because it is timing, not
#     behaviour;
#   - every non-ASCII byte, because a golden is a document and documents in
#     this repository carry no emoji or other non-ASCII glyph.
#
# A workload that fails, or that produces empty output, aborts the capture. A
# missing binary must never be blessed as a passing golden.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GOLDEN_DIR="$HERE/golden"
WORKLOADS="$HERE/workloads.sh"

if [ ! -r "$WORKLOADS" ]; then
    echo "missing workloads file: $WORKLOADS" >&2
    exit 1
fi
# shellcheck source=/dev/null
source "$WORKLOADS"

# The workloads use a relative --paths, so run them from the repository root.
REPO_ROOT="$(git -C "$HERE" rev-parse --show-toplevel 2>/dev/null || echo "$HERE")"
cd "$REPO_ROOT"

unset OPENROUTER_API_KEY || true

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

WORKLOAD_NAMES=(GREP_CODE GREP_KG_MISS AGENT_SEARCH MEM_STARTUP)
RUNS=5

# Portable sha256: the capture host is macOS (shasum), CI is Linux (sha256sum).
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$@"
    else
        shasum -a 256 "$@"
    fi
}

normalise() {
    LC_ALL=C sed -E -e '/^Search latency:/d' -e 's/[\200-\377]//g'
}

capture_run() {
    local name="$1" run="$2" fn="$3"
    local raw="$STAGING/$name.r$run.raw"
    local out="$STAGING/$name.r$run"
    local err="$STAGING/$name.r$run.err"

    # The workload's own exit status, not a pipeline's.
    if ! "$fn" >"$raw" 2>"$err"; then
        echo "workload $name (run $run) failed:" >&2
        sed 's/^/    /' "$err" >&2
        exit 1
    fi
    normalise <"$raw" >"$out"
    if [ ! -s "$out" ]; then
        echo "workload $name (run $run) produced empty output; refusing to bless it" >&2
        exit 1
    fi
}

for name in "${WORKLOAD_NAMES[@]}"; do
    fn="W_$name"
    if ! command -v "$fn" >/dev/null 2>&1; then
        echo "no such workload: $fn" >&2
        exit 1
    fi
    for run in $(seq 1 "$RUNS"); do
        capture_run "$name" "$run" "$fn"
    done
done

# The majority of the five runs is the golden: agent-search scores drift
# between runs, the others are byte-stable.
: >"$STAGING/golden_checksums.txt"
for name in "${WORKLOAD_NAMES[@]}"; do
    files=()
    for run in $(seq 1 "$RUNS"); do
        files+=("$STAGING/$name.r$run")
    done
    best="$(sha256_of "${files[@]}" | awk '{print $1}' | sort | uniq -c | sort -rn | head -1)"
    count="${best%% *}"
    sha="${best##* }"
    for run in $(seq 1 "$RUNS"); do
        if [ "$(sha256_of <"$STAGING/$name.r$run")" = "$sha  -" ]; then
            cp "$STAGING/$name.r$run" "$STAGING/$name.golden"
            break
        fi
    done
    printf '%-13s majority=%s/%s\n' "$name" "$count" "$RUNS"
    (cd "$STAGING" && sha256_of "$name.golden" >>golden_checksums.txt)
done

if [ "${1:-}" = "--update" ]; then
    for name in "${WORKLOAD_NAMES[@]}"; do
        cp "$STAGING/$name.golden" "$GOLDEN_DIR/$name.golden"
    done
    cp "$STAGING/golden_checksums.txt" "$HERE/golden_checksums.txt"
    echo "goldens updated in $GOLDEN_DIR"
    exit 0
fi

echo
echo "=== comparing against the committed goldens ==="
status=0
for name in "${WORKLOAD_NAMES[@]}"; do
    if [ ! -r "$GOLDEN_DIR/$name.golden" ]; then
        echo "  $name: no committed golden"
        status=1
        continue
    fi
    if cmp -s "$STAGING/$name.golden" "$GOLDEN_DIR/$name.golden"; then
        echo "  $name: unchanged"
    else
        echo "  $name: DIFFERS from the committed golden"
        diff -u "$GOLDEN_DIR/$name.golden" "$STAGING/$name.golden" | head -20 || true
        status=1
    fi
done
exit "$status"
