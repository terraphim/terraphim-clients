# Research Document: A ci_guards arm that rejects new environment-mutation sites

**Status**: Approved (Phase 1 gate passed)
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Reviewers**: self-evaluated with the KLS 6-dimension gate (see Gate Record)

## Executive Summary

The confirmed env-mutation shape spread twice in four weeks (6 -> 14 unsafe
blocks under `crates/`) because both landings carried a plausible SAFETY
comment. `forbid(unsafe_code)` (#310) is the real fix but cannot land until
every site is closed. This issue asks for the cheap interim stop: a
`ci_guards` arm that fails when a **new** env-mutation site appears, seeded
with an allow-list of the sites that still exist. After #492 and #493 landed
(the grep crate's ten sites and the MCP stdio site's one), the remaining
inventory is **5 files, 23 sites**, which is what the allow-list must be
seeded with.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | The same pattern has now landed twice undetected by review; this is the second recurrence. |
| Leverages strengths? | Yes | The repository already has `ci_guards.rs` (1070 lines) and a crate walker in it (`workspace_rust_sources`). |
| Meets real need? | Yes | #375 states the acceptance criteria explicitly, and #310 cannot land first. |

**Proceed**: Yes (3/3)

## Problem Statement

### Description

Nothing fails when a new `unsafe { std::env::set_var(...) }` block, or a new
use of the safe wrapper `terraphim_test_utils::set_env_var` /
`remove_env_var` / `EnvVarGuard`, is added under `crates/`. The wrapper is
*safe* API over `unsafe set_var`, so `forbid(unsafe_code)` is structurally
blind to it.

### Impact

- The remediation train (14 -> 0 sites) can be undone by one line, silently.
- The wrapper pattern is the one that will spread next, because it looks safe.

### Success Criteria

1. The arm passes on `main` with the current sites allow-listed.
2. It fails, naming file and line, for a new site of either form.
3. It fails when an allow-list entry names a site that no longer exists, so
   the list cannot rot.

## Current State Analysis

### Existing Implementation

`crates/terraphim_agent/tests/ci_guards.rs` already contains:

| Component | Location | Purpose |
|-----------|----------|---------|
| `workspace_root()` | line 25 | workspace root from `CARGO_MANIFEST_DIR` |
| `workspace_rust_sources()` | line 666 | recursive `.rs` walker under `crates/`, **excluding the guard file itself** (so the guard may quote the literals it forbids) |
| `no_duplicate_terraphim_crates` | line 44 | the manifest-walking precedent |

### Inventory of environment-mutation sites (post #492/#493)

| File | Needle | Count |
|------|--------|-------|
| `crates/terraphim_update/src/state.rs` | `env::set_var(` | 5 |
| `crates/terraphim_update/tests/managed_mode.rs` | `env::set_var(` / `env::remove_var(` | 2 / 1 |
| `crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs` | `set_env_var(` | 6 |
| `crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs` | `set_env_var(` | 5 |
| `crates/terraphim_agent/tests/tui_service_tests.rs` | `EnvVarGuard` | 4 |

Total: 23 sites in 5 files. The issue's "14 direct blocks + 11 wrapper uses"
predates this session's #492/#493, which removed 11 of them.

### Data Flow

`cargo test -p terraphim_agent --test ci_guards` -> `workspace_rust_sources()`
-> per-file scan for the five needles -> compare with the allow-list ->
assert no new site and no stale entry.

### Integration Points

- `native-ci.yml` runs `cargo test -p terraphim_agent --test ci_guards` and
  the workspace `--all-targets` lane, so the arm runs on both lanes.
- The arm must not spawn processes (the native runner's command policy allows
  only `cargo` and repo scripts).

## Constraints

### Technical Constraints

- The guard file quotes the forbidden literals, so it must exclude itself from
  the scan. `workspace_rust_sources()` already does that.
- The `toml` dev-dependency is present in `terraphim_agent`, so member
  manifests can be parsed without a new dependency.
- The allow-list must not rot: an entry whose site disappeared must fail.

### Business Constraints

- The arm passes on `main` today; it may not block the current remediation
  train.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Runtime | < 1 s | walker over `crates/` (the existing `every_file_picker_refuses_the_mmap_cache` guard does the same) |
| False positives | 0 | needles are call-shaped, not word-shaped |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| Detect **both** forms (direct and wrapper) | The wrapper is the form that will spread next | #375: `forbid(unsafe_code)` cannot see it |
| Allow-list must shrink monotonically | Otherwise the arm becomes a rubber stamp | #375 acceptance: "fails when an allow-list entry names a site that no longer exists" |
| No process spawning | The native runner's command policy rejects it | `native-ci.yml` policy comments |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Fixing the remaining 23 sites | Separate issues (#374 for the HOME writer, the rest under the sweep) |
| `forbid(unsafe_code)` workspace-wide | #310; blocked until the sites are closed |
| `deny.toml` / `cargo deny` | No `deny.toml` on `main` and no workflow step runs it (#375) |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `ci_guards.rs` | The arm lives there | Low |
| `terraphim_agent` dev-dependency `toml` | Manifest parsing | Low |

### External Dependencies

None.

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| False positive on a legitimate mention (e.g. a comment) | Medium | Medium | Needle is call-shaped (`set_env_var(`), and the guard file is excluded |
| The allow-list rots silently | Low | High | The arm fails on stale entries |
| The manifest assertion fails today because members still depend on `terraphim_test_utils` | High | Low | Assert it only once the wrapper sites are gone; until then, report the state instead of failing |

### Open Questions

1. Should the manifest assertion fail while wrapper sites remain? The issue's
   acceptance says "once the UF-014 sweep has removed it", so it is
   conditional; the arm reports the state and fails only when the allow-list
   has no wrapper entries left.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|------------|
| `workspace_rust_sources()` is reusable as-is | It already excludes the guard file and skips `target` | The arm would match its own literals | Yes (read) |
| Count-based allow-list entries are acceptable | The issue asks for entries per site; a count is a precise encoding of "N sites here" | A reviewer may prefer one entry per site | Partially - documented in the design |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| (A) allow-list of (file, needle, count) | Compact, detects additions and removals | Chosen |
| (B) allow-list of (file, line number) | Precise, rots on every edit above it | Rejected |
| (C) allow-list of (file, exact source line) | Precise and readable, but a formatting change breaks it | Rejected |

## Research Findings

### Key Insights

1. The guard file already excludes itself from the crate walk, so quoting the
   forbidden literals inside it is safe - the pattern `every_file_picker_refuses_the_mmap_cache`
   already relies on.
2. The inventory shrank from 25 to 23 sites in this session (10 in
   `terraphim_grep` and 1 in the MCP stdio test were removed by #492/#493),
   so the allow-list starts smaller than the issue's estimate.
3. The wrapper form lives in an external crate (`terraphim_test_utils`), so
   this repository can only guard *call sites*, not the definition - which is
   why the manifest assertion matters.

### Relevant Prior Art

- `every_file_picker_refuses_the_mmap_cache` (same file): a source-scanning
  guard with an explanatory doc comment and a per-site message.
- `no_duplicate_terraphim_crates` (same file): the manifest-walking precedent.

### Technical Spikes Needed

None - the pieces exist.

## Recommendations

### Proceed/No-Proceed

Proceed. The arm is one test, seeded with a measured inventory, and it turns
the remediation train into something that cannot silently regress.

### Scope Recommendations

- One test, `no_new_environment_mutation_sites`, appended to `ci_guards.rs`.
- Allow-list as `(file, needle, count)` triples.
- Manifest assertion conditional on the wrapper sites being gone.

### Risk Mitigation Recommendations

- Prove the detector works: add a unit test that feeds the scanner a synthetic
  file list with an unlisted site.

## Next Steps

1. Phase 2: design the arm (allow-list shape, messages, manifest rule).
2. Phase 3: implement, verify, validate against #375's acceptance list.

## Gate Record: Phase 1 (Research) Quality Evaluation

| Dimension | Score | Justification |
|-----------|-------|---------------|
| Physical | 4/5 | Structured sections; the inventory table is the evidence core. |
| Empirical | 4/5 | Counts come from a scan of the tree, not from the issue text, which is now stale. |
| Syntactic | 4/5 | Constraints, vital few and eliminations line up with the recommendations. |
| Semantic | 4/5 | The guard's domain (source scanning, manifests, CI policy) is described accurately. |
| Pragmatic | 4/5 | Names the arm, the allow-list shape and the proof obligation. |
| Social | 3/5 | No external reviewer; the interpretation of the conditional manifest rule is the author's. |

**Average**: 3.8/5 | **Minimum**: 3/5 (Social) -> **CONDITIONAL PASS**
(the requester's confirmation was requested on #375 before implementation).

## Next Steps After Approval

Phase 2 design, Phase 3 implementation, then verification and validation
against the six acceptance criteria of #375.
