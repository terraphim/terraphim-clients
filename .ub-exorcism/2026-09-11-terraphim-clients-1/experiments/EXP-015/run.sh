#!/bin/bash
# EXP-015: run the shipped terraphim_mcp_server integration_test binary once while
# polling open files of every terraphim_mcp_server child for the default sqlite
# store. HOME is NOT redirected on purpose: the point is to observe what the
# shipped test does to the developer's real ~/.terraphim.
export CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target
export RCH_DISABLED=1
REPO=/Users/alex/projects/terraphim/terraphim-clients
W=$REPO/.ub-exorcism/2026-09-11-terraphim-clients-1
OUT=$W/phase5_experiment_results/EXP-015.log
DB=$HOME/.terraphim/sqlite/terraphim.db
POLL=$W/phase5_experiment_results/EXP-015.lsof_poll.log
{
echo "=== EXP-015 start $(date '+%Y-%m-%d %H:%M:%S %Z') host=$(hostname) $(rustc +nightly --version)"
echo "=== env: TERRAPHIM_PROFILE_MEMORY_TYPE=${TERRAPHIM_PROFILE_MEMORY_TYPE-<unset>} TERRAPHIM_SETTINGS_PATH=${TERRAPHIM_SETTINGS_PATH-<unset>} TERRAPHIM_DATA_PATH=${TERRAPHIM_DATA_PATH-<unset>}"
echo "=== settings file consulted by terraphim_persistence on this host: $HOME/Library/Application Support/com.aks.terraphim/settings.toml"
rg -n "^\[profiles|^type|connection_string|datadir" "$HOME/Library/Application Support/com.aks.terraphim/settings.toml"
echo "=== db before: $(stat -f '%N size=%z mtime=%Sm' "$DB" 2>&1)"; ls -la "$HOME/.terraphim/sqlite/" 2>&1
echo "=== building test binary"
cd $REPO && cargo +nightly test -p terraphim_mcp_server --test integration_test --no-run 2>&1 | tail -3
echo "=== poller started $(date '+%H:%M:%S')"
: > "$POLL"
(
  while [ ! -f /tmp/claude-501/exp015.stop ]; do
    pids=$(pgrep -f 'terraphim_mcp_server' | tr '\n' ',' | sed 's/,$//')
    if [ -n "$pids" ]; then
      ts=$(date '+%H:%M:%S.%N' 2>/dev/null || date '+%H:%M:%S')
      lsof -p "$pids" 2>/dev/null | awk -v ts="$ts" '/terraphim\.db|\.terraphim\/|com\.aks\.terraphim/ {print ts, $1, $2, $4, $NF}' >> "$POLL"
    fi
    /bin/sleep 0.2
  done
) &
POLLER=$!
rm -f /tmp/claude-501/exp015.stop
echo "=== running: cargo +nightly test -p terraphim_mcp_server --test integration_test -- --nocapture --test-threads=1"
cd $REPO && cargo +nightly test -p terraphim_mcp_server --test integration_test -- --nocapture --test-threads=1 2>&1 | rg -v "^\s*$" | tail -120
echo "=== test exit=${PIPESTATUS[0]}"
touch /tmp/claude-501/exp015.stop; wait $POLLER 2>/dev/null
echo "=== db after: $(stat -f '%N size=%z mtime=%Sm' "$DB" 2>&1)"; ls -la "$HOME/.terraphim/sqlite/" 2>&1
echo "=== lsof poll: $(wc -l < "$POLL") matching lines; distinct (command,pid,file):"
awk '{print $2, $3, $5}' "$POLL" | sort | uniq -c | sort -rn | head -40
echo "=== distinct pids that had terraphim.db open: $(awk '/terraphim\.db/ {print $3}' "$POLL" | sort -u | tr '\n' ' ')"
echo "=== EXP-015 DONE $(date '+%Y-%m-%d %H:%M:%S %Z')"
} > "$OUT" 2>&1
