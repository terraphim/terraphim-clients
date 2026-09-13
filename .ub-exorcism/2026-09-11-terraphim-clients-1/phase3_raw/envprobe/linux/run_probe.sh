#!/bin/bash
# usage: run_probe.sh <label> <binary> <cwd> <iterations> [extra env...]
label=$1; bin=$2; wd=$3; iters=$4; shift 4
SO=/tmp/ubx-probe-scratch-src/libenvprobe.so
L=/tmp/ubx-probe-logs
cd "$wd"
for widen in 0 500 2000; do
  out="$L/envprobe_linux_${label}_widen${widen}us.log"; : > "$out"
  for i in $(seq 1 $iters); do
    echo "=== ITER $i widen=$widen $(date '+%H:%M:%S')" >> "$out"
    env "$@" ENVPROBE_WIDEN_US=$widen ENVPROBE_LOG="$out" LD_PRELOAD=$SO "$bin" >> "$out" 2>&1
    echo "=== ITER $i exit=$?" >> "$out"
  done
  echo "$label widen=$widen: pre=$(grep -c 'OVERLAP(pre)' "$out") call=$(grep -c 'OVERLAP(call)' "$out") ok=$(grep -c -E 'test result: ok|^scratch ok' "$out") iters=$iters"
done
echo "=== PROBE $label DONE"
