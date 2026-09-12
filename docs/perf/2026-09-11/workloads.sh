# Perf workloads for #253. Binaries: /private/tmp/claude-501/ubx-target-perf/release (commit 1120180, profile release + CARGO_PROFILE_RELEASE_DEBUG=line-tables-only).
W_GREP_CODE="/private/tmp/claude-501/ubx-target-perf/release/terraphim-grep 'fn main' --haystack code --search-only -n 50 --paths crates"
W_GREP_KG="/private/tmp/claude-501/ubx-target-perf/release/terraphim-grep 'haystack' --haystack code --search-only -n 20 --paths /Users/alex/projects/terraphim/terraphim-ai/crates --thesaurus /Users/alex/.config/terraphim/thesaurus.json"
W_AGENT_SEARCH="/private/tmp/claude-501/ubx-target-perf/release/terraphim-agent search 'knowledge graph' --limit 10"
W_MEM_RETRIEVE="/private/tmp/claude-501/ubx-target-perf/release/terraphim-agent memory retrieve 'gitea'"
