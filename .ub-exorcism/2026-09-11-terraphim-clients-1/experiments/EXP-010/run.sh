#!/bin/bash
# EXP-010 runner: shim self-test, then 100 iterations of the terraphim_update lib
# test binary under the envprobe shim at widen 0 us and 500 us.
SO=/tmp/ubx-probe-scratch-src/libenvprobe.so
BIN=$(cat /tmp/ubx-exp010/test_binary.txt)
L=/tmp/ubx-exp010
cd /tmp/ubx-probe-clients
echo "=== EXP-010 start $(date "+%Y-%m-%d %H:%M:%S %Z") host=$(hostname) glibc=$(ldd --version | head -1)"
echo "=== clone HEAD $(git rev-parse HEAD) (crates/terraphim_update identical to 1120180)"
echo "=== binary $BIN"
echo "=== instruments: valgrind=$(command -v valgrind || echo ABSENT) ltrace=$(command -v ltrace || echo ABSENT) strace=$(command -v strace || echo ABSENT)"
echo "=== SELFTEST A: shim trace on resolver_selftest with RES_OPTIONS=ndots:2 LOCALDOMAIN=example.org"
: > $L/selftest_trace.log
RES_OPTIONS=ndots:2 LOCALDOMAIN=example.org ENVPROBE_TRACE=1 ENVPROBE_LOG=$L/selftest_trace.log LD_PRELOAD=$SO $L/resolver_selftest
echo "--- shim-logged getenv names in self-test:"; grep -o "get\S* \"[A-Za-z_]*\"\|getenv(\"[A-Za-z_]*\")" $L/selftest_trace.log | sort | uniq -c
echo "--- raw self-test log (first 40 lines):"; head -40 $L/selftest_trace.log
echo "=== SELFTEST B: nm/objdump check of what getaddrinfo binds to"
LIBC=$(ldd $L/resolver_selftest | awk "/libc.so/ {print \$3}")
echo "libc=$LIBC"; nm -D $LIBC | grep -E " (getenv|secure_getenv|__libc_secure_getenv)$" || true
echo "=== PROBE LOOP: widen 0 and 500 us, 100 iterations each"
for widen in 0 500; do
  out="$L/envprobe_linux_update_widen${widen}us.log"; : > "$out"
  t0=$(date +%s)
  for i in $(seq 1 100); do
    echo "=== ITER $i widen=$widen $(date "+%H:%M:%S")" >> "$out"
    ENVPROBE_WIDEN_US=$widen ENVPROBE_LOG="$out" LD_PRELOAD=$SO "$BIN" --test-threads=8 >> "$out" 2>&1
    echo "=== ITER $i exit=$?" >> "$out"
  done
  t1=$(date +%s)
  echo "update widen=$widen: pre=$(grep -c "OVERLAP(pre)" "$out") call=$(grep -c "OVERLAP(call)" "$out") ok=$(grep -c "test result: ok" "$out") failed_iters=$(grep -c "test result: FAILED" "$out") iters=100 seconds=$((t1-t0))"
  echo "--- summary lines (last 3):"; grep "\[envprobe\] summary" "$out" | tail -3
done
echo "=== EXP-010 DONE $(date "+%Y-%m-%d %H:%M:%S %Z")"
