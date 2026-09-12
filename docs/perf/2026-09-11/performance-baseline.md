# Performance baseline: terraphim-grep search and terraphim-agent search and memory

Refs terraphim/terraphim-clients#253. Captured 2026-09-12 (see `preconditions.txt` for the exact time, load and host).

## Build

| Item | Value |
|---|---|
| Commit | 1120180 (main) |
| Profile | `release` (opt-level 3, codegen-units 1, lto off) with `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` for symbolised profiles; no Cargo.toml change |
| Target dir | isolated (`/private/tmp/claude-501/ubx-target-perf`), never `~/.cargo/bin` binaries |
| Host | Apple Silicon, 11 cores, macOS (details in `preconditions.txt`) |
| Load at capture | 5.0 one-minute average on 11 cores; background: one single-threaded Miri run and rust-analyzer |

## Workloads (`workloads.sh`)

| Name | Command | What it measures |
|---|---|---|
| GREP_CODE | `terraphim-grep 'fn main' --haystack code --search-only -n 50 --paths crates` | fff-search enhanced grep over this repo, no thesaurus |
| GREP_KG | `terraphim-grep 'haystack' --haystack code --search-only -n 20 --paths <terraphim-ai>/crates --thesaurus ~/.config/terraphim/thesaurus.json` | knowledge-graph path with a 219-entry thesaurus |
| AGENT_SEARCH | `terraphim-agent search 'knowledge graph' --limit 10` | offline search, auto-routed role over a markdown vault; the only workload with substantial work |
| MEM_RETRIEVE | `terraphim-agent memory retrieve 'gitea'` | startup plus rolegraph build; the store holds 5 test items, so it does not measure retrieval |

Golden outputs (`golden/`, `golden_checksums.txt`) are byte-stable across 5 runs for the grep and memory workloads once the self-reported latency line is dropped. AGENT_SEARCH is not pinnable: its integer scores drift between runs because of an upstream rolegraph counter bug (terraphim/terraphim-ai#3376); ranks are stable in this corpus but can invert on ties.

## Results (`hyperfine.json`, 3 warm-up, 10 runs)

| Workload | Mean | Min | Max | User | System |
|---|---|---|---|---|---|
| GREP_CODE | 15.2 ms ± 0.6 | 14.2 | 16.0 | 9.7 ms | 34.6 ms |
| GREP_KG | 36.4 ms ± 0.6 | 35.8 | 38.0 | 21.5 ms | 54.9 ms |
| AGENT_SEARCH | 617.3 ms ± 29.6 | 597.7 | 693.7 | 324.2 ms | 295.9 ms |
| MEM_RETRIEVE | 177.4 ms ± 33.2 | 138.4 | 230.2 | 11.1 ms | 12.5 ms |

## Hotspot 1: the update check before every agent command

MEM_RETRIEVE spends 177 ms of wall time for 24 ms of CPU, with the widest variance of any workload (138 to 230 ms). The agent performs an HTTPS update check before dispatching every subcommand (`crates/terraphim_agent/src/main.rs:448-456`), including the Claude Code hook subcommands. The differential (`hyperfine_no_update.md`) points the manifest URL at a closed local port, which the updater treats as a fetch failure and retries with backoff (`crates/terraphim_update/src/manifest.rs:20,156,182`: 3 attempts, 500 ms doubling):

| Workload | Reachable endpoint | Unreachable endpoint |
|---|---|---|
| MEM_RETRIEVE | 177 ms | 1574 ms |
| AGENT_SEARCH | 617 ms | 2645 ms |

So every agent invocation pays roughly 150 ms of network round trip when online and about 1.5 s when the endpoint is unreachable (offline laptop, blocked network, or the CDN being slow). For a hook that fires on every tool call, that is the whole latency budget.

## Hotspot 2: agent search CPU

AGENT_SEARCH is CPU-bound (620 ms CPU for 617 ms wall) and stable. The samply profile (`agent_search.profile.json`, open with `samply load`) shows the work on the main thread with tokio workers nearly idle. From the traced call chain in terraphim-ai#3376: every document is inserted into all 11 role graphs by terraphim_middleware, then the queried role's graph is discarded and rebuilt from scratch and the documents re-inserted, so each search does 12 rolegraph inserts per document where one is needed.

## Opportunity matrix (score = impact x confidence / effort; implement only >= 2.0)

| Hotspot | Impact | Confidence | Effort | Score | Isomorphism |
|---|---|---|---|---|---|
| Gate the pre-dispatch update check: skip for hook, learn hook, guard and memory apply subcommands; persist last-check time and honour the 24 h interval already in `UpdateConfig`; never retry inside a one-shot command | 5 | 5 (measured) | 2 | 12.5 | Output unchanged; only a side effect (a network request and a notification line) is removed or deferred |
| Remove the discarded first indexing pass (terraphim_middleware indexer/mod.rs:190 then terraphim_service search.rs:740) | 4 | 5 (traced) | 2 (upstream, one loop) | 10 | No score change for the queried role |
| Fix rolegraph counter seeding (rolegraph lib.rs:1235) and add a document-id tie-break | 3 (correctness) | 5 | 1 | 15 | Scores change deterministically; ordering may differ only where it is already unstable |
| Grep: per-query thesaurus clone (`hybrid_searcher.rs:177`) | 2 | 3 | 2 | 3 | None; borrow instead of clone |

The first row is the lever for this repo (the other two live in terraphim-ai). It also removes the retry storm that makes offline hooks take 1.5 s.

## Regression protocol

Before any change: `cargo test`, then extend a regression test for the changed path. After: `cargo test`, `sha256sum -c golden_checksums.txt` over fresh outputs (`capture_golden.sh`), then re-run `hyperfine` with the same workloads and compare against `hyperfine.json` on the same machine at comparable load. Criterion benches (`hybrid_search`, `search_nfr`) are run separately with `--save-baseline` once the machine is quiet; they are not part of this capture because the machine was shared during the audit.
