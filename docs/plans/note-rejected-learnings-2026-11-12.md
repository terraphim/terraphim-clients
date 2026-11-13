# Research, Design, Verification and Validation: a rejected learning is never used (#388)

**Issue**: terraphim-clients#388
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect, explicit acceptance criteria, local change)

## Research

### What the code actually does

| Fact | Source |
|------|--------|
| `reject()` sets `suggestion_status = Rejected` and leaves `trust_level` alone | `shared_learning/store.rs` |
| The injector filters on `trust_level` only | `shared_learning/injector.rs` |
| The injector parses the trust level from the learning's **own frontmatter** | `shared_learning/injector.rs` (`Frontmatter.trust_level`) |
| The shared markdown file carries **no** status field at all | `shared_learning/markdown_store.rs` (`LearningFrontmatter`) |

So a rejected learning was still injected, and a file could promote itself: the
file's declared `trust_level` was taken at face value.

### The trust model, read from the code

`shared_learning/mod.rs` documents the levels:

- **L1** unverified, auto-captured,
- **L2** peer-validated: *applied 3+ times across 2+ agents with positive
  outcome*,
- **L3** human-approved.

L2 is therefore an **evidence** claim, and L3 an **approval** claim - not
something a file may assert about itself.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | A rejection must travel with the learning | The status has to be part of the shared markdown frontmatter |
| C2 | "Never injected, suggested or synced" | The rule must be one decision function all three use |
| C3 | A file may not promote itself | The effective trust follows status + evidence, not the declared level |
| C4 | L2 needs a success-rate floor | `meets_l2_criteria` grows one |

## Design

**One decision, shared by every consumer** (`shared_learning/types.rs`):

```rust
pub fn status_is_usable(status: SuggestionStatus) -> bool;      // Rejected => false
pub fn meets_l2_criteria(quality: &QualityMetrics) -> bool;       // applications, agents, success rate
pub fn effective_trust(status: SuggestionStatus, quality: Option<&QualityMetrics>) -> TrustLevel;
```

`effective_trust` is the trust a learning may actually be used at:

| Status | Effective trust |
|---|---|
| Rejected | L0 - never used |
| Pending | L2 when the evidence supports it, otherwise L1 |
| Approved | L3 - human approved |

The **declared** trust level in the file is deliberately not consulted.

**The rule is applied everywhere**:

- `markdown_store`: the status travels in the frontmatter (default `pending`),
  so a rejection is visible to every consumer, including another agent.
- `injector`: a rejected learning is skipped (`skipped_rejected` counter), and
  the trust filter uses the *effective* trust.
- `store::suggest`: rejected learnings are never suggested.
- `store::approve` / `store::promote_to_l3`: refuse to act on a rejected
  learning; `approve_after_rejection(id, reason)` is the explicit way to lift a
  rejection.
- `store::record_application`: no auto-promotion for a rejected learning.
- `wiki_sync`: a rejected learning is never synced.
- `L2_MIN_SUCCESS_RATE = 0.5` adds the success-rate floor the model asks for.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --all-targets --features cross-agent-injection` | 1218 passed, 0 failed |
| `cargo test -p terraphim_agent --all-targets` (default features) | 1212 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

New tests:

- `tests/shared_learning_trust.rs` - **property tests** over the whole
  (declared trust, status, evidence) space: a rejected learning is never usable
  and never carries trust; a pending learning reaches L2 only with evidence; an
  approved learning is L3. Plus a test that three *failed* applications across
  two agents are not peer validation.
- `store`: a rejected learning is not suggested; `approve` and `promote_to_l3`
  refuse it with an error that says so; `approve_after_rejection` approves it.
- `injector`: a rejected learning is not injected; a learning that only *claims*
  L3 is not injected, while the same learning with the evidence behind the claim
  is.
- `wiki_sync`: a rejected learning is not synced.

## Validation

| Acceptance criterion (issue #388) | Result |
|---|---|
| "proptest over (trust level, status, action): a Rejected learning is never injected, suggested or synced" | PASS - the property test covers (trust level, status, evidence); the three consumers have their own tests |
| "Filter Rejected in injector, `suggest()` and wiki sync" | PASS |
| "require an explicit override to promote a rejected learning" | PASS - `approve`/`promote_to_l3` refuse; `approve_after_rejection` is the override |
| "take trust from the store, never from frontmatter" | PASS for shared files - the effective trust is derived from the status and the recorded evidence, and the declared level is ignored |
| "add a success-rate floor to L2" | PASS - `L2_MIN_SUCCESS_RATE = 0.5` |

## Limits

- The demotion path (the issue's "no demotion path" remark) is **not** added
  here: a learning that stops being effective stays at its level until it is
  re-reviewed. That is a separate design question (what demotes a learning, and
  who decides) and is left for a dedicated change.
- `compute_risk` capping at 0.7 - mentioned in the issue - belongs to the
  *memory* subsystem (`memory_command.rs`), not to the shared-learning store, and
  is untouched.
- The evidence that backs an L2 claim travels in the file, so a peer could
  still overstate its own metrics. The injector now requires the metrics to be
  *present and consistent* rather than trusting the declared level, which is a
  strict improvement, but it is not a proof.
