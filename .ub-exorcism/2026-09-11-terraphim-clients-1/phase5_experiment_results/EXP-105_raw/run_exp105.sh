#!/bin/bash
# EXP-105 driver (bigbox). usage: run_exp105.sh <label> <iters_widen0> <iters_widen2000> [extra env...]
label=$1; n0=$2; n2000=$3; shift 3
B=/tmp/ubx-probe-target/release/terraphim-agent
SO=/tmp/ubx-probe/libenvprobe.so
L=/tmp/ubx-probe-logs
cd /tmp/ubx-probe-clients
for pair in "0:$n0" "2000:$n2000"; do
  widen=${pair%%:*}; iters=${pair##*:}
  out="$L/exp105_${label}_widen${widen}us.log"; : > "$out"
  for i in $(seq 1 $iters); do
    echo "=== ITER $i widen=$widen $(date '+%H:%M:%S')" >> "$out"
    env -u SSL_CERT_FILE -u SSL_CERT_DIR "$@" ENVPROBE_WIDEN_US=$widen ENVPROBE_TRACE=1 ENVPROBE_LOG="$out" LD_PRELOAD=$SO "$B" search "knowledge graph" --limit 1 >> "$out" 2>&1
    echo "=== ITER $i exit=$?" >> "$out"
  done
  echo "$label widen=$widen: iters=$iters set_ssl_cert_file=$(grep -c ' set SSL_CERT_FILE' "$out") pre=$(grep -c 'OVERLAP(pre)' "$out") call=$(grep -c 'OVERLAP(call)' "$out") nonzero_exit=$(grep -c '=== ITER [0-9]* exit=[1-9]' "$out")"
done
echo "=== PROBE $label DONE $(date '+%H:%M:%S')"
