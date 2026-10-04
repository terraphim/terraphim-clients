# Validation Report: PR #44 Fix #2721 insufficient KG propagation

**Status**: Validated
**Date**: 2026-08-30
**Stakeholders**: Project Maintainer
**Research Doc**: terraphim/terraphim-ai#2721
**Design Doc**: n/a (bug fix; pattern mirrored from existing `Sufficient` branch)
**Verification Report**: `.quality/pr-44-verification.md`

## Executive Summary

The PR restores data symmetry between the `Sufficient` and `Insufficient`
branches of `TerraphimGrep::search()`: both now report actual chunk
counts, KG concept counts, and KG concept lists. The bug had been
silently hiding KG boost activity from callers of `terraphim-grep`
when the result count was below `min_results`. The fix is minimal,
mirrors an existing pattern, and is regression-tested.

## Specialist Skill Results

### Performance (`rust-performance` skill) — not applicable

No performance budgets are impacted. The change replaces a constant
zero with a `Vec::len()` call on a vector already in memory. The
Sufficient branch already does this exact work, so the new behaviour
is at parity with the previously-correct path.

### Security (`security-audit` skill) — not applicable

No security boundaries touched. The change only affects which fields
of `GrepResult` are populated, not which code is reached.

### Acceptance Testing (`acceptance-testing` skill) — PASS

Acceptance criterion from terraphim-ai#2721: *"the Insufficient path
must propagate the same KG concept information that the Sufficient path
does."*

Verified by the new regression test:

```text
running 1 test
test tests::insufficient_path_propagates_chunk_count ... ok
```

Forcing `Insufficient` with a 2-file corpus (below default
`min_results: 3`) yields:

- `stats.chunks_returned == chunks.len()` (was 0 before fix)
- `stats.kg_hits == concepts.len()` (was 0 before fix)
- `concepts == hybrid_results.kg_concepts` (was empty vec before fix)

### Requirements Traceability (`requirements-traceability` skill)

| Requirement | Acceptance Scenario | Evidence | Stakeholder | Status |
|-------------|--------------------|----------|-------------|--------|
| #2721: propagate `chunks_returned` in Insufficient | 2-file corpus | new regression test passes | Project Maintainer | Accepted |
| #2721: propagate `kg_hits` in Insufficient | 2-file corpus | new regression test passes | Project Maintainer | Accepted |
| #2721: propagate `concepts` in Insufficient | 2-file corpus | new regression test passes | Project Maintainer | Accepted |

### Quality Gate (`quality-gate` skill) — PASS

| Criterion | Status |
|-----------|--------|
| Verification gate passed | PASS |
| Workspace check (`cargo check --workspace --all-features`) | PASS |
| Clippy clean | PASS |
| Rustfmt clean | PASS |
| Regression test green | PASS |
| Hygiene cleanup committed in scope | PASS |

## System Test Results

### End-to-End Scenarios

| ID | Workflow | Steps | Result | Status |
|----|----------|-------|--------|--------|
| E2E-44-01 | Insufficient path with sparse corpus | 1. Build 2-file corpus 2. Run search 3. Inspect stats | All three stats reflect real data | PASS |

### Non-Functional Requirements

| Category | Target | Actual | Skill Used | Status |
|----------|--------|--------|------------|--------|
| Latency | unchanged | unchanged | `rust-performance` | PASS |
| Memory | unchanged | unchanged | n/a | PASS |
| Security | no regression | no regression | `security-audit` | PASS |
| Compile time | unchanged | unchanged | n/a | PASS |

## Acceptance Interview Summary

**Date**: 2026-08-30
**Participants**: Project Maintainer
**Method**: AskUserQuestion structured interview

#### Decision
- Approve and merge.

#### Conditions
- None.

## Defect Register

| ID | Description | Origin Phase | Severity | Resolution | Status |
|----|-------------|--------------|----------|------------|--------|
| D-PR44-01 | Tracked SQLite cache files | Phase 3 | Low | Removed in commit `29e4c94` | Closed |
| D-PR44-02 | rustfmt violation in new test | Phase 3 | Low | Fixed in commit `6b5ea6f` | Closed |

## Sign-off

| Stakeholder | Role | Decision | Conditions | Date |
|-------------|------|----------|------------|------|
| Project Maintainer | Maintainer | Approved | None | 2026-08-30 |

## Gate Checklist

- [x] Performance validated (n/a, no budgets affected)
- [x] Security validated (n/a, no boundaries touched)
- [x] UAT scenario executed (1/1)
- [x] Requirements traceability complete (3/3)
- [x] Quality gate report produced
- [x] Stakeholder interview completed
- [x] All critical/high defects resolved
- [x] Formal sign-off received
- [x] Ready for production merge

## Next Step

Proceed to merge via `gtr merge-pull --owner terraphim --repo terraphim-clients --index 44 --delete-branch`.
