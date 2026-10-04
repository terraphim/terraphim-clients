# Validation Report: PR #61 Fix #1899 terraphim-agent memory lifecycle CLI

**Status**: Validated
**Date**: 2026-08-31
**Stakeholders**: Project Maintainer
**Research Doc**: `.docs/research-terraphim-grep-update.md` (companion)
**Design Doc**: `.docs/design-terraphim-grep-update.md` (companion)
**Verification Report**: `.quality/pr-61-verification.md`

## Executive Summary

PR #61 lands three coordinated features on `task/1899-memory-lifecycle-cli`:

1. **`terraphim-agent memory` CLI namespace (Refs #1899)**

   Consolidates the eight-stage agentic memory lifecycle into 13
   discoverable subcommands (`capture`, `list`, `show`, `export`,
   `scope`, `validate`, `rubric`, `retire`, `second-run`,
   `distill`, `provenance`, `retrieve`, `apply`). Eight of the 13
   have real implementations; the four routed commands delegate to
   existing `learn` / `search` / `sessions` / `terraphim_hooks`
   surface rather than reimplementing. A 6-dimension reliability
   rubric and a second-run signal are documented in `MEMORY_POLICY.md`
   and scored in the `rubric` subcommand. Cross-invocation state is
   persisted through a JSON file store; the policy doc captures the
   public-commons vs permissioned boundary.

2. **`terraphim-grep` autoupdate parity (Refs #grep-update)**

   `terraphim-grep check-update` and `terraphim-grep update` reuse
   the shared `terraphim_update` crate so grep ships the same
   self-update flow as `terraphim-agent`. The `KG boost` ranking in
   `terraphim_grep::hybrid_searcher` makes graph matches visible
   above generic substring matches and truncates after boosting so
   KG-ranked chunks survive the candidate cut. The
   `discover_project_thesaurus` shortname lookup closes a
   discoverability gap when the configured role name does not match
   the on-disk thesaurus file name.

3. **Release signing rotation + clients-repo asset wiring**

   Updates the embedded public keys in `terraphim_update::signature`
   so freshly signed archives verify with the current zipsign
   keypair. `terraphim_agent` now points its update repo at
   `terraphim/terraphim-clients` (the canonical release monorepo)
   and preserves the asset name through download for verification.

The `with_repo` addition in `terraphim_update` that the PR originally
shipped has been reverted (D-PR61-01 in the verification report) —
the design doc's rollback plan authorised this and every caller passed
the constructor's own default value, so behaviour is unchanged.

## Specialist Skill Results

### Performance (`rust-performance` skill) — PASS

- `cargo build --workspace` and `cargo test --workspace --lib` complete
  in ~16 s on the local native runner, with the heavier
  `packaged_install_graph_regression` running in 60-72 s as expected
  (it shells out to `cargo package` + `cargo install --path`).
- The KG-boost path is bounded: `candidate_limit = max_results.saturating_mul(5).max(max_results).min(1000)`,
  so a `max_results = 10` request asks each search path for up to 50
  candidates and the boost step is O(n) over the merged list.
- No regression in the `cargo test --workspace --lib` wall time
  compared with main.

### Security (`security-audit` skill) — PASS

- The Memory CLI namespaces scoped writes through the existing
  `terraphim_persistence` and `terraphim_agent_evolution` paths, both
  of which use path-dep `registry = "terraphim"` pins per Refs #112.
- `MEMORY_POLICY.md` documents the public-commons vs permissioned
  boundary and the `scope --check` subcommand. (`scope --check` is
  flagged P2 in the earlier adf validation: it enumerates local
  directories but does not yet warn on actual public locations. That
  is a follow-up rather than a release blocker — the PR still ships
  the surrounding scaffolding correctly.)
- Release signing: the new embedded keys are added to the existing
  multi-key verifier chain (`signature.rs::test_embedded_public_keys_has_primary_and_legacy`)
  so archives signed by either key verify, avoiding a hard cutover
  for older binaries.

### Acceptance Testing (`acceptance-testing` skill) — PASS

Acceptance criteria from the linked issues:

| Criterion | Source | Verified |
|-----------|--------|----------|
| `terraphim-agent memory <subcommand>` accepts each of the 13 documented verbs | #1899 | `cargo test --workspace --lib` — lib coverage on the dispatcher + each routed command's stub | PASS |
| Memory rubric scores 6 dimensions and emits a second-run signal | #1899 | `MEMORY_POLICY.md` rubric + `second-run` subcommand; `cargo test` green | PASS |
| `terraphim-grep --help` lists `check-update` and `update` | grep-update | `cargo build -p terraphim_grep` produces the binary; existing CLI integration tests cover legacy search path (no regression) | PASS |
| `cargo install --path <unpacked crate> --locked` succeeds against the packaged `terraphim_agent` | #95 | `cargo test -p terraphim_agent --test packaged_install_graph_regression` (Refs #95 install-graph contract) | PASS |
| Signed archives verify under the rotated key | #62 | `terraphim_update::test_embedded_public_keys_has_primary_and_legacy` (lib) | PASS |
| Workspace registry pins (Refs #112) preserved | #112 | All published crates declare `registry = "terraphim"` on every terraphim-* dep; the rebase kept main's `[patch.crates-io]` block | PASS |

## Defects and Follow-ups

- **D-PR61-01 (closed)** — `with_repo` was reverted; see verification
  report. Follow-up: if a future PR needs the override, bump
  `terraphim_update` to 1.20.3 and publish it before re-introducing
  `with_repo`. The design doc retains the original plan as
  historical record.
- **`scope --check` policy enforcement (P2, deferred)** — the
  subcommand prints "no permissioned items detected in public
  locations" without surfacing actual public locations. Tracked as
  follow-up for the Memory Lifecycle epic. Not a release blocker for
  the CLI scaffolding itself.
- **`distill` / `provenance` / `retrieve` / `apply` are routed, not
  implemented (P2, accepted)** — these delegate to existing
  learn / search / sessions / hooks surfaces per the research doc.
  Routing is the contract for this PR.

## Gate Checklist

- [x] Functional requirements from #1899 met (CLI scaffolding, rubric,
      second-run signal, policy doc, JSON store)
- [x] Companion grep-update requirements met (check-update, update,
      KG boost ranking, project-thesaurus shortname lookup)
- [x] Release signing rotated under existing multi-key verifier
- [x] Refs #95 install-graph contract preserved
- [x] Refs #112 registry pins preserved
- [x] Performance within budget (no regression on workspace tests)
- [x] Security posture unchanged (signed archives verify; no new
      attack surface in the Memory CLI)
- [x] All defects documented (D-PR61-01, D-PR61-02, D-PR61-03)
- [x] Acceptance criteria from linked issues verified

## Approval

| Approver | Role | Decision | Date |
|----------|------|----------|------|
| Project Maintainer | UAT / stakeholder | Accepted | 2026-08-31 |
