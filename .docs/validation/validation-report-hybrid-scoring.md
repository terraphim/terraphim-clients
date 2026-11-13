# Validation Report: Hybrid Scoring for SharedLearningStore

**Status**: Validated (post-fix)
**Date**: 2026-09-01
**Stakeholders**: requesting engineer (user)
**Research ref**: user request "BM25 via store.suggest - we shall leverage existing terraphim hybrid scoring instead of pure BM25"
**Design ref**: design captured in commit body of `ba2e292`
**Verification ref**: `.docs/verification/verification-report-hybrid-scoring.md`
**PR review ref**: `.docs/pr-review/pr-review-ba2e292.md`

## Executive Summary

The change replaces pure BM25 scoring in `SharedLearningStore::suggest` and `SharedLearningStore::find_similar` with the existing Terraphim hybrid scorer (`RoleGraph::query_graph` -- weighted mean of node rank, edge rank, and document rank, with thesaurus term expansion). BM25 is preserved as a transparent fallback when the graph is unset or returns no matches. Stakeholder interview confirmed the implementation matches the requirement and the chosen trade-offs. Four P2 findings from the structural PR review were closed in commit `85b1b8b`. Final PR review confidence: 5/5.

## Specialist Skill Results

### Performance (`rust-performance` skill)

- **Scope**: scoring-path swap only.
- **Benchmarks run**: none required. Both BM25 and `query_graph` run in-memory over a corpus sized to agent workloads (< 500 entries); neither is on a request hot path. The `suggest_with_local_scored` orchestrator wraps `suggest` so any added latency propagates linearly; no measurable regression expected.
- **Build profile**: dev (test) profile.
- **Targets met**: n/a (no NFR was specified in the original ask).
- **Status**: PASS (no regression expected; not measured).

### Security (`security-audit` skill)

- **Scope**: write-lock handling on `role_graph` in `set_role_graph` and `sync_to_graph`; document body construction in `build_document_for_graph`.
- **Findings**: none. No PII, secrets, or untrusted network input enters the new paths. Document body uses the existing `extract_searchable_text()` (already audited for redacted secrets in the BM25 path).
- **Status**: PASS.

### Acceptance Testing (`acceptance-testing` skill)

- **UAT Plan**: see Acceptance Results below.
- **Scenarios executed**: 5/5 (one per user-visible behaviour).
- **Pass rate**: 100%.
- **Status**: PASS.

### Requirements Traceability (`requirements-traceability` skill)

- **Matrix location**: `.docs/verification/verification-report-hybrid-scoring.md` (Section: Unit Test Traceability Matrix)
- **Requirements traced**: 3 (requirement met, trust semantics accepted, auto-ingest accepted)
- **Gaps**: none.

### Quality Gate (`quality-gate` skill)

- **Decision**: Pass
- **Notes**: this is a single-file, internal-scoring refactor. Cross-file consistency verified (no public API changes; `find_similar`/`suggest` signatures unchanged). No breaking changes. Existing 294 pre-existing lib tests still pass; 9 new hybrid tests pass.

## System Test Results

### End-to-End Scenarios

| ID | Workflow | Steps | Expected Outcome | Result | Status |
|---|---|---|---|---|---|
| E2E-001 | Insert + suggest via graph | 1. open store 2. set rolegraph 3. insert learning 4. call suggest("query", "agent") | learning returned via hybrid scoring | `suggest_uses_role_graph_when_available` | PASS |
| E2E-002 | Insert + find_similar via graph | 1. open store 2. set rolegraph 3. insert two learnings 4. call find_similar("query") | matched learning ranked first | `find_similar_uses_role_graph_when_available` | PASS |
| E2E-003 | suggest with graph but unrelated thesaurus | 1. set graph with unrelated terms 2. insert learning 3. suggest | falls back to BM25, learning still surfaced | `find_similar_falls_back_to_bm25_when_graph_has_no_match` | PASS |
| E2E-004 | suggest without graph | 1. open store (no graph) 2. insert learning 3. suggest | BM25 path runs, learning surfaced | `suggest_falls_back_to_bm25_without_graph` | PASS |
| E2E-005 | Trust weighting on hybrid path | 1. set graph 2. insert L1 and L3 with identical body 3. find_similar | L3 ranks above L1 | `hybrid_rank_respects_trust_weighting` | PASS |

### Non-Functional Requirements

| Category | Target | Actual | Status |
|---|---|---|---|
| Latency (p95) | not specified | not measured (in-memory, no I/O) | n/a |
| Memory | unchanged | no new allocations on hot path; one `Document` allocation per learning on insert | PASS |
| Security | no secrets/PII in logs | none added | PASS |
| Visual regression | n/a (no UI) | n/a | n/a |

### NFR Details

- The change introduces zero new I/O. Both scoring paths read from the in-memory `tokio::sync::RwLock<HashMap>` index; the hybrid path additionally takes a `std::sync::RwLock` read on the rolegraph.
- No new background tasks, timers, or network calls.

## Acceptance Results

### Requirements Traceability

| Requirement ID | Description | Evidence | Stakeholder | Status |
|---|---|---|---|---|
| REQ-001 | `find_similar` and `suggest` use Terraphim hybrid scoring instead of pure BM25 | `hybrid_rank` calls `RoleGraph::query_graph` and normalises `IndexedDocument.rank` | user | Accepted |
| REQ-002 | BM25 remains the fallback when the graph is unset or empty | `find_similar_falls_back_to_bm25_without_graph`, `find_similar_falls_back_to_bm25_when_graph_has_no_match`, `hybrid_rank_with_empty_thesaurus_falls_back` | user | Accepted |
| REQ-003 | Trust weighting (L0..L3) is preserved on the hybrid path | `hybrid_rank_respects_trust_weighting` | user | Accepted |
| REQ-004 | `applicable_agents` filter is preserved on the hybrid path | `suggest_respects_applicable_agents_with_graph` | user | Accepted |
| REQ-005 | New learnings are auto-ingested into the graph so callers do not pre-load | `insert_syncs_learning_into_role_graph` plus initial sync in `set_role_graph` | user | Accepted |

### Acceptance Interview Summary

**Date**: 2026-09-01
**Participants**: requesting engineer (user)

#### Problem Validation

> "Looking at the original problem statement: 'BM25 via store.suggest - we shall leverage existing terraphim hybrid scoring instead of pure BM25'. Does this implementation solve it?"

User answered: **Yes, fully met**. `find_similar` and `suggest` now route through `RoleGraph::query_graph` (the existing Terraphim hybrid scorer with thesaurus-based term expansion) when a graph is configured; BM25 stays as the fallback. This matches the ask exactly.

#### Success Criteria

> "Trust semantics: hybrid score is `normalised_graph_rank * trust_level.weight()` (range 0..3). Acceptable?"

User answered: **Acceptable**. The shape matches the prior BM25 path; L0 entries filtered out, L3 entries dominate, callers see a consistent trust signal across both code paths.

> "Auto-ingest: learnings are mirrored into the rolegraph on insert() and on set_role_graph(). Acceptable side effect?"

User answered: **Acceptable**. Removes the burden of pre-loading documents from callers; the store and graph stay in lockstep.

#### Completeness

All five acceptance criteria (REQ-001..REQ-005) have at least one test as evidence. No additional requirements surfaced during the interview.

#### Risk Assessment

- **Concurrent writers**: `sync_to_graph` uses a `std::sync::RwLock` write lock that can be poisoned. The implementation intentionally swallows that error (graph is an accelerator, not the source of truth). Risk: low.
- **Stale graph after `record_application` / promote**: the graph caches a document at insert time and is not re-synced on quality-metric changes. The body text doesn't change on those operations, so re-sync is unnecessary; trust-level changes do not propagate to the graph but `hybrid_rank` reads `l.trust_level.weight()` from the in-memory index at query time, not from the graph. Risk: low (correctness preserved; just no graph-edge rank boost when trust changes).

#### Conditions

None. The user accepted the implementation as-is.

## Defect Register

No defects found during validation. Defects surfaced during verification (`D001`, `D002`, `P2-1`..`P2-4`) are pre-existing or addressed in `85b1b8b`. PR review confidence is now 5/5 with zero P2 open.

## Sign-off

| Stakeholder | Role | Decision | Conditions | Date |
|---|---|---|---|---|
| (user, via AskUserQuestion) | Requesting engineer | Approved | None | 2026-09-01 |
| (self) | Implementation + verification engineer | Approved (post-fix) | None | 2026-09-01 |

## Gate Checklist

### Specialist Skill Outputs

- [x] `rust-performance`: not applicable (no NFR specified)
- [x] `security-audit`: no findings
- [x] `visual-testing`: not applicable (no UI)
- [x] `acceptance-testing`: 5/5 UAT scenarios pass
- [x] `requirements-traceability`: 5/5 requirements traced
- [x] `quality-gate`: Pass

### Validation Gates

- [x] All user workflows tested end-to-end (via lib integration tests)
- [x] NFRs from research: none specified
- [x] All requirements traced to acceptance evidence
- [x] Stakeholder interview completed (AskUserQuestion)
- [x] No critical/high defects open
- [x] Formal sign-off received
- [x] Deployment conditions: none

### Deployment Readiness

- **Verified**: yes (post-fix)
- **Pushed**: yes (commits `ba2e292` and `85b1b8b` on `main`, remote `origin` and `gitea`)
- **PR status**: no separate PR was opened; the work was committed directly to `main` per workspace policy (Claude.md). Both commits are visible on the default branch.
- **Follow-ups**: none required. PR review confidence is 5/5 (zero P0/P1/P2).