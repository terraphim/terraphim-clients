# Design: truthful `terraphim_grep` insufficient-result statistics

## Problem
Published `terraphim_grep` 1.21.1 can return non-empty `chunks` with `stats.chunks_returned = 0` when the sufficiency heuristic classifies fewer than three matches as `RlmInsufficient`. This violates the JSON result invariant and blocks release wrapper #3208.

## Decision
In `crates/terraphim_grep/src/lib.rs`, preserve returned chunks and KG concepts in the `Sufficiency::Insufficient` branch and derive counters from the actual vectors. Do not change sufficiency thresholds or reinterpret `RlmInsufficient`.

## Tests first
Extend the real CLI known-match regression so it asserts `stats.chunks_returned == chunks.len()`. Add/extend library coverage for `RlmInsufficient` with non-empty chunks and the same invariant. Run the targeted test before implementation and retain the expected RED.

## Acceptance
- Known-match JSON has at least one chunk.
- `stats.chunks_returned == chunks.len()` in every result branch.
- `stats.kg_hits == concepts.len()` when concepts are retained.
- Focused crate tests, fmt and clippy pass.
- Exact Release Guardian known-match smoke passes from the branch binary.

## Non-goals
No release version bump, publish, sufficiency threshold change, unrelated crate refactor, or dependency update in this PR.
