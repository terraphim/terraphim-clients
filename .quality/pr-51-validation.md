# Validation Report: PR #51 Fix #48 thesaurus NotFound ERROR suppress

**Status**: Validated
**Date**: 2026-08-30
**Stakeholders**: Project Maintainer
**Research Doc**: terraphim/terraphim-clients#48
**Design Doc**: inline module-level docstring in `crates/terraphim_agent/src/logging.rs`
**Verification Report**: `.quality/pr-51-verification.md`

## Executive Summary

The PR introduces a thin `FilteredLogger` wrapper around `env_logger`
that suppresses exactly one benign `ERROR` line emitted by
`terraphim_service::ensure_thesaurus_loaded` when an optional
persisted thesaurus file is missing. The service transparently
rebuilds the thesaurus from the local KG and the operation succeeds;
the `ERROR` line was misleading and polluted stderr and scripted
output. The filter is narrow (level=Error + target=terraphim_service
+ message contains "Failed to load thesaurus" + lowercased contains
"not found"/"notfound") so genuine thesaurus failures are preserved.
End-to-end probe on the rebuilt binary confirms the benign line is
suppressed and stdout/stderr are clean for an `extract` invocation.

## Specialist Skill Results

### Performance (`rust-performance` skill) — not applicable

Hot-path cost is two pointer dereferences and one allocation-free
substring search per log record. env_logger's own buffer and stderr
write dominate.

### Security (`security-audit` skill) — not applicable

Logging-only change. No new attack surface, no new boundaries, no
untrusted input handling.

### Acceptance Testing (`acceptance-testing` skill) — PASS

Acceptance criterion from #48: *"the spurious `ERROR
terraphim_service] Failed to load thesaurus: NotFound(...)` line
must no longer appear in stderr for `terraphim-agent` invocations
that trigger a knowledge-graph rebuild."*

Verified end-to-end:

```text
$ ./target/debug/terraphim-agent --robot --format json extract \
    "Some sample text about config and pipeline and orchestrator."

---stdout---
Found 3 paragraph(s):
--- Match 1 (term: 'config') ---
... (3 matches, all with real term labels)

---stderr (filtered)---
[2026-08-30T22:03:59Z WARN  terraphim_persistence::settings]
    Failed to parse profile 'sqlite': OpenDal(ConfigInvalid...
```

No `ERROR` line for the thesaurus NotFound. The unrelated sqlite
profile WARN is preserved (correctly — it is a real warning).

### Quality Gate (`quality-gate` skill) — PASS

| Criterion | Status |
|-----------|--------|
| Verification gate passed | PASS |
| Workspace check clean | PASS |
| Clippy clean | PASS |
| Rustfmt clean | PASS |
| 444/444 lib tests pass (437 prior + 7 new) | PASS |
| End-to-end stderr probe clean of benign ERROR | PASS |

## System Test Results

### End-to-End Scenarios

| ID | Workflow | Steps | Result | Status |
|----|----------|-------|--------|--------|
| E2E-51-01 | `extract` on a text without persisted thesaurus | 1. Invoke `terraphim-agent --robot --format json extract "..."` 2. Capture stderr | No `Failed to load thesaurus` ERROR | PASS |
| E2E-51-02 | Real term labelling and offsets | Same invocation, inspect stdout | 3 matches, real terms, no phantom labels, no mid-word starts | PASS (incidental) |

### Non-Functional Requirements

| Category | Target | Actual | Skill Used | Status |
|----------|--------|--------|------------|--------|
| Latency | unchanged | unchanged | `rust-performance` | PASS |
| Memory | unchanged | unchanged | n/a | PASS |
| Security | no regression | no regression | `security-audit` | PASS |

## Acceptance Interview Summary

**Date**: 2026-08-30
**Participants**: Project Maintainer
**Method**: AskUserQuestion structured interview + end-to-end CLI probe

#### End-to-end probe results
- stdout: 3 paragraph matches with correct term labels (`config`, `pipeline`, `orchestrator`)
- stderr: only the unrelated sqlite WARN; no thesaurus NotFound ERROR
- The fix suppresses exactly the documented benign message; genuine log lines are preserved

#### Decision
- Approve and merge.

## Sign-off

| Stakeholder | Role | Decision | Conditions | Date |
|-------------|------|----------|------------|------|
| Project Maintainer | Maintainer | (pending) | - | 2026-08-30 |
