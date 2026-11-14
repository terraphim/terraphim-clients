#!/usr/bin/env bash
# The agent CLI performance gate (#437).
#
# The native runner's command policy accepts `cargo`, `bash <checked-in
# script>` and `test` as the first token of a workflow step, so the workflow
# calls this script with one line and the policy is satisfied.
#
# The script turns the gate on: the ordinary test lane measures the commands
# but does not assert on wall-clock numbers, because a busy machine would
# make those assertions meaningless, while this lane - which runs on a
# dedicated runner, between the other lanes - asserts the ceilings.
#
# The measured binary is the dev-profile binary the lane above built. The
# budgets for that profile live in docs/benchmarks/agent-cli-budgets.json;
# the release profile's budgets are measured against a release build and
# published on the pull request.
set -euo pipefail

cd "$(dirname "$0")/../.." || exit 1

# The coverage lane's instrumentation wrapper must not wrap this build.
export RUSTC_WRAPPER=""
export RUSTC_WORKSPACE_WRAPPER=""

# The gate is on: the ceilings are asserted, not only measured.
export TERRAPHIM_PERF_GATE=1

cargo test -p terraphim_agent --test perf_gate -- --nocapture
