#!/bin/bash
cd /tmp/ubx-probe-clients
export CARGO_TARGET_DIR=/tmp/ubx-probe-target
date "+%Y-%m-%d %H:%M:%S %Z"
cargo +nightly test --no-run -p terraphim_update --lib --message-format=json 2>/tmp/ubx-exp010/build.stderr | python3 -c "
import sys, json
for line in sys.stdin:
    try: m = json.loads(line)
    except Exception: continue
    if m.get(\"reason\") == \"compiler-artifact\" and m.get(\"executable\") and m[\"target\"][\"name\"] == \"terraphim_update\" and \"lib\" in m[\"target\"][\"kind\"]:
        print(m[\"executable\"])
" > /tmp/ubx-exp010/test_binary.txt
echo "BUILD_DONE exit=${PIPESTATUS[0]}"; cat /tmp/ubx-exp010/test_binary.txt; tail -3 /tmp/ubx-exp010/build.stderr
date "+%Y-%m-%d %H:%M:%S %Z"
