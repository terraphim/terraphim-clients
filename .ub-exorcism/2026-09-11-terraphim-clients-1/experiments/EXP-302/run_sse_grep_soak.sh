#!/bin/bash
# EXP-302 orchestration: shipped terraphim_mcp_server --sse under concurrent
# grep_files with a sibling in-place writer (same-length rewrite) then truncator.
# UNTESTED THIS SESSION (safety classifier blocked execution). Design is complete.
#
# Prereqs on bigbox (already satisfied this session):
#   - server release binary built at /tmp/ubx-r7/EXP-302/target/release/terraphim_mcp_server
#   - EXP-104 harness binary (rewrite / child-truncate arms) built from experiments/EXP-104
#   - EXP-302 SSE driver built from experiments/EXP-302 (compile-verify first)
#
# usage: run_sse_grep_soak.sh <iterations>
set -u
ITERS="${1:-1000}"
PORT=18302
TREE=/tmp/ubx-r7/EXP-302/tree
SRV=/tmp/ubx-r7/EXP-302/target/release/terraphim_mcp_server
DRIVER=/tmp/ubx-r7/EXP-302/driver/target/release/exp302_sse_driver   # adjust to build path
EXP104=/tmp/ubx-r7/EXP-104/target/release/exp104_mmap_grep            # adjust to build path
LOG=/tmp/ubx-r7/EXP-302/EXP-302.log

# 1. Build a tree of 4 MiB files with 'needle café' lines (the EXP-104 corpus shape).
rm -rf "$TREE"; mkdir -p "$TREE"
python3 - "$TREE" <<'PY'
import sys, os
tree = sys.argv[1]
line = "needle café line\n".encode()   # 'é' = C3 A9, same as EXP-104
n = (4*1024*1024)//len(line)
for i in range(4):
    with open(os.path.join(tree, f"big{i}.txt"), "wb") as f:
        f.write(line*n)
print("tree built:", tree)
PY

{
  echo "=== EXP-302 SSE grep soak ==="
  echo "date: $(date -u +%FT%TZ)"
  echo "HEAD(clone): $(cd /tmp/ubx-probe-clients && git rev-parse HEAD)"
  echo "server: $SRV"
  echo "uptime: $(uptime)"
  echo "orphans(terraphim_server): $(pgrep -fc terraphim_server)"
  echo "iterations: $ITERS  port: $PORT  clients: 8"
} | tee "$LOG"

# 2. Start the server on an unused port.
RUST_LOG=warn "$SRV" --sse --bind "127.0.0.1:$PORT" >> "$LOG" 2>&1 &
SRV_PID=$!
echo "server pid $SRV_PID" | tee -a "$LOG"
# wait for /health
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then break; fi
  sleep 1
done

run_arm() { # <arm-name> <writer-cmd...>
  local arm="$1"; shift
  echo "=== arm: $arm ===" | tee -a "$LOG"
  # driver in background (drives grep_files from 8 clients for 60s)
  "$DRIVER" "$PORT" "$TREE" 8 60 >> "$LOG" 2>&1 &
  local DRV=$!
  # sibling mutator against the SAME tree files
  ( cd "$TREE" && "$@" ) >> "$LOG" 2>&1 &
  local MUT=$!
  wait "$DRV"
  kill "$MUT" 2>/dev/null
  # is the server still alive?
  if kill -0 "$SRV_PID" 2>/dev/null; then
    echo "arm $arm: server still alive" | tee -a "$LOG"
  else
    wait "$SRV_PID"; local st=$?
    echo "arm $arm: server EXITED code=$st (128+7=135 => SIGBUS)" | tee -a "$LOG"
    return 1
  fi
}

# Writer arm: EXP-104 rewrite (same-length 0xA9->0xFF flip). Needs the EXP-104
# binary pointed at $TREE; adapt EXP-104 to take a dir, or copy files in.
# Truncate arm: shrink the mapped files under live greps (SIGBUS probe).
run_arm writer "$EXP104" rewrite plain "$ITERS" || true
run_arm truncate "$EXP104" child-truncate plain "$ITERS" || true

# 3. Stop the server.
kill "$SRV_PID" 2>/dev/null; wait "$SRV_PID" 2>/dev/null
echo "DONE_EXP302" | tee -a "$LOG"
