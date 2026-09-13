# Phase 2 findings — bucket 20: box-allocator

**Verdict: NO_SURFACE** (closed at Phase 0/2 with evidence; no subagent fan-out).

## Evidence

Signature sweep over `crates/` (excluding target and .ub-exorcism), run 2026-09-11:

```
rg -n --type rust -e 'Box::from_raw|Box::into_raw|GlobalAlloc|#\[global_allocator\]|alloc::alloc|Layout::' crates
```

Hit count: **10**

```
crates/terraphim_agent/tests/repl_integration_tests.rs:243:            let chunks = Layout::default()
crates/terraphim_agent/tests/repl_integration_tests.rs:310:            let chunks = Layout::default()
crates/terraphim_agent/tests/repl_integration_tests.rs:354:            let chunks = Layout::default()
crates/terraphim_agent/tests/repl_integration_tests.rs:395:            let chunks = Layout::default()
crates/terraphim_agent/tests/repl_integration_tests.rs:466:            let chunks = Layout::default()
crates/terraphim_agent/tests/repl_integration_tests.rs:499:            let chunks = Layout::default()
crates/terraphim_agent/tests/repl_integration_tests.rs:534:            let chunks = Layout::default()
crates/terraphim_agent/tests/repl_integration_tests.rs:578:            let chunks = Layout::default()
crates/terraphim_agent/src/main.rs:3020:                    let chunks = Layout::default()
crates/terraphim_agent/src/main.rs:3072:                        let chunks = Layout::default()
```

Corroboration: `scripts/rustonomicon-antipatterns.sh` reported 0 hits (phase2_raw/rustonomicon_antipatterns.txt) and `scripts/ast-grep-ub-patterns.sh` reported 0 hits for every bucket-specific pattern except the deliberately broad `alignment-repr-packed-field-ref` rule, whose hits are all on non-packed structs (`#[repr(packed)]` count is 0).

## Triage of hits

All 10 hits are `ratatui::layout::Layout::default()` (TUI layout constraints), not `std::alloc::Layout`. Verified: `rg -n 'use ratatui|ratatui::' crates/terraphim_agent/src/main.rs crates/terraphim_agent/tests/repl_integration_tests.rs` shows the import; there is no `use std::alloc` anywhere in `crates/`. Zero allocator-API sites.

## Findings

None. Bucket has no static surface in this workspace. Re-open if Phase 1 macro-expansion rows or a Phase 6 idea introduce a site.
