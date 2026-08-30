#!/usr/bin/env bash
#
# Tests for scripts/ci/check-no-duplicate-terraphim.sh.
#
# Duplicate resolutions arise transitively and cannot be contrived on demand --
# cargo refuses outright to resolve a *direct* dependency that would conflict --
# so detection is tested against recorded `cargo tree -d` output captured from
# the real #112 failure. The live path is covered by the check running in CI.
#
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
GATE="$HERE/../ci/check-no-duplicate-terraphim.sh"
pass=0; fail=0
check() { if [ "$2" -eq "$3" ]; then printf '  ok    %s\n' "$1"; pass=$((pass+1));
          else printf '  FAIL  %s (expected %s, got %s)\n' "$1" "$2" "$3"; fail=$((fail+1)); fi; }

CHECK_TREE_FIXTURE="$HERE/fixtures/tree-duplicates.txt" "$GATE" >/dev/null 2>&1
check "duplicate terraphim crates are rejected" 1 $?

CHECK_TREE_FIXTURE="$HERE/fixtures/tree-clean.txt" "$GATE" >/dev/null 2>&1
check "third-party duplicates alone are accepted" 0 $?

# the failure message must name the offending crate, not just say "duplicates"
out=$(CHECK_TREE_FIXTURE="$HERE/fixtures/tree-duplicates.txt" "$GATE" 2>&1)
case "$out" in *terraphim_config*) r=0 ;; *) r=1 ;; esac
check "failure output names the offending crate" 0 $r

# thiserror is duplicated in the fixture and must not be reported
case "$out" in *thiserror*) r=1 ;; *) r=0 ;; esac
check "third-party duplicates are not reported as failures" 0 $r

printf '\ncheck-no-duplicate-terraphim tests: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
