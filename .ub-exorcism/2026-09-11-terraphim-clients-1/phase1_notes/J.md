# Module: J dependency-unsafe-surface

## Purpose
Catalogue the unsafe and UB-relevant surface the workspace inherits from dependencies, since the workspace's own crate code has only 6 unsafe blocks (all env setters, section A).

## Unsafe-surface tally
Own code: 0 unsafe outside section A. Dependency tally: cargo-geiger per crate, see phase2_raw/geiger_<crate>.json (run 2026-09-11; results appended in Phase 2 dependency findings when the run completes; it was queued behind the shared build lock).

## Advisory surface (cargo-audit 2026-09-11, phase2_raw/cargo_audit.txt; cargo-deny with deny.toml from #254)
UB-class (unsound) advisories reaching the resolved graph:
| Advisory | Crate | Chain into workspace | Reachable code path? |
|---|---|---|---|
| RUSTSEC-2026-0204 | crossbeam-epoch 0.9.18 | crossbeam-deque -> ignore -> fff-search -> terraphim_file_search / terraphim_middleware -> mcp_server, agent, cli | fmt::Pointer on invalid Atomic/Shared; only if a dependency formats an epoch pointer. Fixed by 0.9.21 lockfile bump |
| RUSTSEC-2026-0190 | anyhow 1.0.102 | opendal -> terraphim_config -> cli, agent, command_runtime | Error::downcast_mut; grep for downcast_mut in own code = 0. Fixed by 1.0.104 |
| RUSTSEC-2026-0221 | event-listener 5.4.1 | sqlx-core -> sqlx -> opendal | !Send tag crossing threads via StackSlot; sqlx internal. Fixed by 5.4.2 |
| RUSTSEC-2026-0253 | lru 0.18.0 | transitive | UAF on panic inside LruCache::pop; only if a value's Drop panics. Fixed by 0.18.4 |
| RUSTSEC-2026-0186 | memmap2 0.9.10 | transitive | unchecked pointer offset. Fixed by 0.9.11 |
| RUSTSEC-2026-0183/0184 | git2 0.20.4 | fff-search | Remote::list and BlameHunk signature; neither called by workspace code (rg 'Remote::list|BlameHunk' = 0). Needs fff-search bump (#257) |
DoS-class: h2 0.4.15 (#254 bump), quick-xml 0.37/0.38 (#258), rmcp 0.9.1 DNS rebinding (#256). Lockfile-only, not in resolved graph: rsa, quinn-proto.

## Send/Sync-relevant dependency notes
- event-listener advisory is a Send/Sync soundness hole inside sqlx; it is not reachable through any manual unsafe impl in this workspace, but it means tokio tasks touching sqlx via opendal could in theory move a !Send value. Treated as CONTRACTUAL-BUT-DEFENSIBLE pending the bump.

## Licence finding (not UB, recorded for completeness)
html2md GPL-3.0+ via terraphim_middleware into Apache-2.0 binaries: terraphim/terraphim-ai#3374.

## Open questions
- Does any workspace or first-party terraphim_* code call `Arc::downcast`/`Error::downcast_mut` on anyhow errors? (rg in workspace = 0; terraphim_service not checked.)

## cargo-geiger results (appended 2026-09-12)
cargo-geiger completed for 2 of 11 crates (the rest failed on the runner-side build lock churn and empty JSON; rerun is low value given the static sweeps). Available: terraphim_update: 198 packages in graph, 118 use unsafe, 30 forbid unsafe, own unsafe 0; terraphim-session-analyzer: 126 packages, 54 use unsafe, 15 forbid, own 0. Consistent with the Phase 1 finding that all workspace-owned unsafe is the six env-setter blocks. Raw: phase2_raw/geiger_<crate>.json.
