#!/bin/bash
# Runs EXP-011 .. EXP-014 sequentially; each log is self-contained.
export CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target
export RCH_DISABLED=1
export RUST_BACKTRACE=0
cd "$(dirname "$0")"
R=../phase5_experiment_results
hdr() { echo "=== $1 start $(date '+%Y-%m-%d %H:%M:%S %Z') host=$(hostname) $(rustc +nightly --version) target=$(rustc +nightly -vV | awk '/host/ {print $2}')"; }
{ hdr EXP-011; cargo +nightly run -p exp011_nan_comparators 2>&1; echo "=== exit=$?"; } > $R/EXP-011.log 2>&1
{ hdr EXP-012; cargo +nightly run -p exp012_char_boundary 2>&1; echo "=== exit=$?"; } > $R/EXP-012.log 2>&1
{ hdr EXP-013; cargo +nightly test -p exp013_current_thread_bridge -- --nocapture 2>&1; echo "=== exit=$?"; } > $R/EXP-013.log 2>&1
{ hdr EXP-014
  for i in 1 2 3 4 5; do echo "--- run $i: default parallelism (--test-threads=4)"; cargo +nightly test -p exp014_serial_mixed_binary -- --test-threads=4 2>&1; echo "--- run $i exit=$?"; done
  echo "--- control: --test-threads=1 (zz_report is expected to FAIL here: no concurrency, so no overlap)"
  cargo +nightly test -p exp014_serial_mixed_binary -- --test-threads=1 2>&1; echo "--- control exit=$?"
} > $R/EXP-014.log 2>&1
echo "ALL_DONE $(date '+%H:%M:%S')" > $R/run_011_014.done
