#!/bin/bash
SO=/tmp/ubx-exp010/libgaiprobe.so
BIN=$(cat /tmp/ubx-exp010/test_binary.txt)
L=/tmp/ubx-exp010
cd /tmp/ubx-probe-clients
echo "=== EXP-010 gaiprobe start $(date "+%Y-%m-%d %H:%M:%S %Z")"
echo "=== self-test: resolver_selftest under gaiprobe (expect getaddrinfo=1, setenv=0)"
GAIPROBE_LOG=$L/gai_selftest.log LD_PRELOAD=$SO $L/resolver_selftest; cat $L/gai_selftest.log
for widen in 0 500; do
  out="$L/gaiprobe_update_widen${widen}us.log"; : > "$out"; t0=$(date +%s)
  for i in $(seq 1 100); do
    echo "=== ITER $i widen=$widen" >> "$out"
    GAIPROBE_WIDEN_US=$widen GAIPROBE_LOG="$out" LD_PRELOAD=$SO "$BIN" --test-threads=8 >> "$out" 2>&1
    echo "=== ITER $i exit=$?" >> "$out"
  done
  t1=$(date +%s)
  echo "gaiprobe widen=$widen: windows_setenv=$(grep -c "WINDOW(setenv)" "$out") windows_gai=$(grep -c "WINDOW(getaddrinfo)" "$out") ok=$(grep -c "test result: ok" "$out") iters=100 seconds=$((t1-t0))"
  echo "--- summary lines (last 2):"; grep "\[gaiprobe\] summary" "$out" | tail -2
  echo "--- getaddrinfo call count distribution across iterations:"; grep -o "getaddrinfo=[0-9]*" "$out" | sort | uniq -c
done
echo "=== EXP-010 gaiprobe DONE $(date "+%Y-%m-%d %H:%M:%S %Z")"
