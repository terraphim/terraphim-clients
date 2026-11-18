# shellcheck shell=bash
# Perf workloads for #253.
#
# Each workload is a shell function so the capture script can run it and read
# its exit status directly. A command string would have to be `eval`ed, and an
# `eval`ed pipeline reports the exit status of its last stage (sed) - so a
# missing binary becomes a successful, empty capture that then gets blessed.
#
# PERF_BIN_DIR points at the release build of the audited commit; the default
# is the capture host's build. Override it to re-capture elsewhere.
PERF_BIN_DIR="${PERF_BIN_DIR:-/private/tmp/claude-501/ubx-target-perf/release}"
PERF_TERRAPHIM_AI_CRATES="${PERF_TERRAPHIM_AI_CRATES:-/Users/alex/projects/terraphim/terraphim-ai/crates}"
PERF_THESAURUS="${PERF_THESAURUS:-/Users/alex/.config/terraphim/thesaurus.json}"

W_GREP_CODE() {
    "$PERF_BIN_DIR/terraphim-grep" 'fn main' --haystack code --search-only -n 50 --paths crates
}

# The knowledge-graph path with a 219-entry thesaurus. On the capture corpus
# this produced zero KG hits and fell back to SearchOnly, so it measures a KG
# MISS, not KG ranking.
W_GREP_KG_MISS() {
    "$PERF_BIN_DIR/terraphim-grep" 'haystack' --haystack code --search-only -n 20 \
        --paths "$PERF_TERRAPHIM_AI_CRATES" --thesaurus "$PERF_THESAURUS"
}

# The only workload with substantial work. Its integer scores drift between
# runs (terraphim/terraphim-ai#3376), so it is not pinnable.
W_AGENT_SEARCH() {
    "$PERF_BIN_DIR/terraphim-agent" search 'knowledge graph' --limit 10
}

# Startup plus rolegraph build. The store holds 5 test items and the query
# retrieves none, so this measures STARTUP, not retrieval.
W_MEM_STARTUP() {
    "$PERF_BIN_DIR/terraphim-agent" memory retrieve 'gitea'
}
