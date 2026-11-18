#!/usr/bin/env bash
# Regression tests for normalise.sh (the #253 capture's normalisation).
#
# Run: bash docs/perf/2026-09-11/normalise_test.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "$HERE/normalise.sh"

fails=0
check() {
    local name="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        printf '  ok    %s\n' "$name"
    else
        printf '  FAIL  %s\n' "$name"
        printf '        expected: %q\n' "$expected"
        printf '        actual:   %q\n' "$actual"
        fails=$((fails + 1))
    fi
}

# The defect this guards: a UTF-8 glyph surrounded by ASCII must have the
# GLYPH removed and the ASCII kept. A sed byte class got this backwards on
# GNU sed, removing "A" and "B" and keeping the glyph.
out="$(printf 'A\u2705B\n' | normalise)"
check "a UTF-8 glyph between ASCII is removed, the ASCII kept" "AB" "$out"

# The latency line is dropped.
out="$(printf 'head\nSearch latency: 12.3 ms\ntail\n' | normalise)"
check "the self-reported latency line is dropped" "head
tail" "$out"

# Trailing blank lines are dropped, one trailing newline remains.
out="$(printf 'a\n\n\n' | normalise | od -c | head -1)"
check "trailing blank lines are dropped" "0000000   a  \n" "$out"

# Leading whitespace inside a line is preserved.
out="$(printf '  indented\n' | normalise)"
check "leading whitespace is preserved" "  indented" "$out"

# A file that is entirely blank normalises to nothing.
out="$(printf '\n\n' | normalise)"
check "an all-blank input normalises to empty" "" "$out"

# The em dash is non-ASCII and is removed with the other high bytes.
out="$(printf 'a\u2014b\n' | normalise)"
check "a non-ASCII em dash is removed" "ab" "$out"

if [ "$fails" -ne 0 ]; then
    echo "$fails normalisation test(s) failed" >&2
    exit 1
fi
echo "all normalisation tests passed"
