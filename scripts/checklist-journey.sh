#!/usr/bin/env bash
# CI entry point for the checklist/concept contract.
#
# Checklist validation is product behaviour, not a build artefact: a checklist whose
# categories no knowledge-graph concept can credit is silently useless, and until now
# nothing noticed. This runs the contract, the end-to-end journey and a latency bench.
#
# Binary resolution: use the workspace build when it exists (CI builds it in the step
# before), otherwise whatever is on PATH, so the same script works on a dev box.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

if [ -x target/debug/terraphim-agent ]; then
  export TERRAPHIM_AGENT_BIN="$PWD/target/debug/terraphim-agent"
fi
if [ -x target/debug/terraphim-grep ]; then
  export TERRAPHIM_GREP_BIN="$PWD/target/debug/terraphim-grep"
fi

report="${CHECKLIST_REPORT:-target/checklist-journey.json}"
mkdir -p "$(dirname "$report")"
# stderr stays out of the report: merging it in made the JSON unparseable the moment the
# harness had anything to say (provenance, warnings), and a corrupt report reads as a
# crash rather than as the run it was.
python3 scripts/checklist-journey.py --json > "$report" 2> "${report%.json}.stderr.log"
rc=$?
cat "$report"
if [ -s "${report%.json}.stderr.log" ]; then
  echo "--- stderr ---" >&2
  cat "${report%.json}.stderr.log" >&2
fi
exit "$rc"
