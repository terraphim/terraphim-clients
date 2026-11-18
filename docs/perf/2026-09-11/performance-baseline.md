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
| GREP_KG_MISS | `terraphim-grep 'haystack' --haystack code --search-only -n 20 --paths <terraphim-ai>/crates --thesaurus ~/.config/terraphim/thesaurus.json` | the knowledge-graph path with a 219-entry thesaurus; on this corpus it produced **zero KG hits** and fell back to `SearchOnly`, so it measures a KG **miss**, not KG ranking |
| AGENT_SEARCH | `terraphim-agent search 'knowledge graph' --limit 10` | offline search, auto-routed role over a markdown vault; the only workload with substantial work |
| MEM_STARTUP | `terraphim-agent memory retrieve 'gitea'` | startup plus rolegraph build; the store holds 5 test items and the query retrieves none, so it measures **startup**, not retrieval |

Golden outputs (`golden/`, `golden_checksums.txt`) are byte-stable across 5 runs for the grep and memory workloads once the self-reported latency line and the non-ASCII status glyph are dropped. The scenarios are named for what they actually measure: `GREP_KG_MISS` is a KG miss and `MEM_STARTUP` is startup. AGENT_SEARCH is not pinnable: its integer scores drift between runs because of an upstream rolegraph counter bug (terraphim/terraphim-ai#3376); ranks are stable in this corpus but can invert on ties.

## Results (`hyperfine.json`, 3 warm-up, 10 runs)

| Workload | Mean | Min | Max | User | System |
|---|---|---|---|---|---|
| GREP_CODE | 15.2 ms ± 0.6 | 14.2 | 16.0 | 9.7 ms | 34.6 ms |
| GREP_KG_MISS | 36.4 ms ± 0.6 | 35.8 | 38.0 | 21.5 ms | 54.9 ms |
| AGENT_SEARCH | 617.3 ms ± 29.6 | 597.7 | 693.7 | 324.2 ms | 295.9 ms |
| MEM_STARTUP | 177.4 ms ± 33.2 | 138.4 | 230.2 | 11.1 ms | 12.5 ms |

## Hotspot 1: the update check before every agent command

MEM_STARTUP spends 177 ms of wall time for 24 ms of CPU, with the widest variance of any workload (138 to 230 ms). The agent performs an HTTPS update check before dispatching every subcommand (`crates/terraphim_agent/src/main.rs:448-456`), including the Claude Code hook subcommands.

### What the measurements show

The differential (`hyperfine_connection_refused.md`) points the manifest URL at a **closed local port**. The updater treats that as a fetch failure and retries with backoff (`crates/terraphim_update/src/manifest.rs:263,297`: 3 attempts, `500 ms` doubling, and a **15 s** timeout per attempt):

| Workload | Reachable endpoint | Update check pointed at a closed port |
|---|---|---|
| MEM_STARTUP | 177 ms | 1574 ms |
| AGENT_SEARCH | 617 ms | 2645 ms |

Two things follow from this, and one thing does not.

**Established - the retry backoff, on a connection-refused endpoint.** The differential costs about **1.4 s** on top of the online run for MEM_STARTUP (1574 - 177 = 1397 ms) and about **2.0 s** for AGENT_SEARCH (2645 - 617 = 2028 ms). The backoff itself is 500 ms + 1000 ms of sleeping; the remainder is the refused connections themselves. The two ends of that range bracket the retry path rather than measuring one fixed cost.

**Not established - the online cost of the update check.** This capture has **no update-disabled control**: there is no run with the check switched off, so its cost cannot be separated from the rest of the command. The gap between MEM_STARTUP's 177 ms of wall time and its 24 ms of CPU is 153 ms of *unaccounted* time - it may be network round trip, TLS handshake, DNS, or the updater's own work - and nothing here separates them. Attributing that gap to the network would be an inference from wall time minus CPU time rather than a measurement, so this document puts no figure on the online cost of the update check.

**Not established - "unreachable" covers two failure modes.** A **connection-refused** endpoint (a closed local port: the case measured here) fails immediately, so the cost is the retry backoff, about 1.4-2.0 s. A **blackholed** endpoint - a dropped packet, a silent firewall, a slow CDN - instead waits out the per-attempt timeout of **15 s** (`crates/terraphim_update/src/manifest.rs:263`), so the same code path costs tens of seconds. The two differ by more than an order of magnitude and must not be quoted as one number.

**The differential's raw data is not committed.** `hyperfine_connection_refused.md` is a summary; the raw Hyperfine JSON for the differential runs, and the exact invocation and environment, are not in this tree, so the differential cannot be re-derived from what is committed here.

## Hotspot 2: agent search CPU

AGENT_SEARCH is CPU-bound (620 ms CPU for 617 ms wall) and stable. A samply profile of that run was taken on the capture host and shows the work on the main thread with tokio workers nearly idle, but **the profile file is not committed to this tree** and neither are the exact Hyperfine invocations, so neither the profile nor the benchmark run can be reproduced from what is here alone. The traced call chain below is the reproducible part. From the traced call chain in terraphim-ai#3376: every document is inserted into all 11 role graphs by terraphim_middleware, then the queried role's graph is discarded and rebuilt from scratch and the documents re-inserted, so each search does 12 rolegraph inserts per document where one is needed.

## Opportunity matrix (score = impact x confidence / effort; implement only >= 2.0)

| Hotspot | Impact | Confidence | Effort | Score | Isomorphism |
|---|---|---|---|---|---|
| Gate the pre-dispatch update check: skip for hook, learn hook, guard and memory apply subcommands; persist last-check time and honour the 24 h interval already in `UpdateConfig`; never retry inside a one-shot command | 5 | 4 (the retry backoff is measured; the online cost is not) | 2 | 10 | Output unchanged; only a side effect (a network request and a notification line) is removed or deferred |
| Remove the discarded first indexing pass (terraphim_middleware indexer/mod.rs:190 then terraphim_service search.rs:740) | 4 | 5 (traced) | 2 (upstream, one loop) | 10 | No score change for the queried role |
| Fix rolegraph counter seeding (rolegraph lib.rs:1235) and add a document-id tie-break | 3 (correctness) | 5 | 1 | 15 | Scores change deterministically; ordering may differ only where it is already unstable |
| Grep: per-query thesaurus clone (`hybrid_searcher.rs:177`) | 2 | 3 | 2 | 3 | None; borrow instead of clone |

The first row is the lever for this repo (the other two live in terraphim-ai). It also removes the retry storm that the connection-refused differential measures at roughly 1.4-2.0 s per agent invocation.

## Regression protocol

Before any change: `cargo test`, then extend a regression test for the changed path. After: `cargo test`, then `bash docs/perf/2026-09-11/normalise_test.sh` and `bash docs/perf/2026-09-11/capture_golden.sh`, which re-captures the five runs into a temporary directory and compares the majority output against the committed goldens, failing loudly on any difference and refusing to capture at all if a workload fails or produces empty output. `bash docs/perf/2026-09-11/capture_golden.sh --update` re-blesses the goldens when a change is intended. Then re-run `hyperfine` with the same workloads and compare against `hyperfine.json` on the same machine at comparable load. Criterion benches (`hybrid_search`, `search_nfr`) are run separately with `--save-baseline` once the machine is quiet; they are not part of this capture because the machine was shared during the audit.
