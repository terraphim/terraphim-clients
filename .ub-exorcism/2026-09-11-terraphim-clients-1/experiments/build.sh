#!/bin/bash
export CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target
export RCH_DISABLED=1
cd "$(dirname "$0")"
date '+%Y-%m-%d %H:%M:%S %Z'
cargo +nightly build --workspace --tests 2>&1
echo "BUILD_DONE exit=${PIPESTATUS[0]}"
date '+%Y-%m-%d %H:%M:%S %Z'
