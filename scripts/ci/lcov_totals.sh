#!/usr/bin/env bash
#
# Print lcov line-hit totals for the CI log (Refs #328).
#
# The Gitea native runner does not execute `uses:` marketplace steps
# (terraphim_github_runner workflow/parser.rs skips them), so the native
# coverage lane cannot upload lcov.info via actions/upload-artifact. This
# script is the durable replacement signal: a one-line totals summary in
# the run log. Invoked from the workflow as `bash ./scripts/ci/...`
# because the runner's command policy allowlists only the literal first
# token of a step (`bash`, `cargo`, `test`; Refs #118, #328).
#
# Usage: bash ./scripts/ci/lcov_totals.sh <lcov.info>

set -euo pipefail

file="${1:?usage: lcov_totals.sh <lcov.info>}"

if [ ! -s "$file" ]; then
    echo "::error::$file is missing or empty; the coverage lane produced no lcov output"
    exit 1
fi

awk '
    # lcov records are "LF:<n>" / "LH:<n>" with no space, so split on ":".
    /^LF:/ { split($0, a, ":"); lf += a[2] + 0 }
    /^LH:/ { split($0, a, ":"); lh += a[2] + 0 }
    END {
        if (lf == 0) {
            printf "lcov totals: 0 instrumented lines (malformed lcov?)\n"
        } else {
            printf "lcov totals: %d/%d lines hit (%.2f%%)\n", lh, lf, 100.0 * lh / lf
        }
    }
' "$file"

printf 'lcov records: %s files\n' "$(grep -c '^SF:' "$file")"
