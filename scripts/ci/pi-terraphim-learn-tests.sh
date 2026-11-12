#!/usr/bin/env bash
# Functional test for the pi-terraphim-learn plugin package (#467).
#
# The native runner's command policy allows only `cargo` and repository
# scripts, so this wrapper is the entry point for both lanes. When node is not
# installed on the runner the test is skipped with a notice rather than failing
# the lane for a missing runtime; the GitHub lane has node and runs it.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1

if ! command -v node >/dev/null 2>&1; then
  echo "::notice::node is not installed on this runner; skipping the pi-terraphim-learn functional test"
  exit 0
fi

node --test packages/pi-terraphim-learn/index.test.mjs
