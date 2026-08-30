#!/usr/bin/env bash
# wait-for-mcp.sh MAX_SECONDS URL
# Polls URL every 500ms until it returns HTTP 200, then exits 0.
# Exits non-zero and prints elapsed time if MAX_SECONDS is exceeded.
set -euo pipefail

MAX_SECONDS="${1:-30}"
URL="${2:-http://localhost:8000/health}"

echo "Waiting up to ${MAX_SECONDS}s for MCP server at ${URL} ..."
start_ts=$(date +%s)

while true; do
    if curl --silent --fail --max-time 1 "${URL}" > /dev/null 2>&1; then
        elapsed=$(( $(date +%s) - start_ts ))
        echo "MCP server ready after ${elapsed}s"
        exit 0
    fi

    elapsed=$(( $(date +%s) - start_ts ))
    if [ "${elapsed}" -ge "${MAX_SECONDS}" ]; then
        echo "ERROR: MCP server not ready after ${MAX_SECONDS}s at ${URL}" >&2
        exit 1
    fi

    sleep 0.5
done
