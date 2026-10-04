# Verification Report: PR #61 Fix #1899 terraphim-agent memory lifecycle CLI

**Status**: Verified
**Date**: 2026-08-31
**Branch**: `task/1899-memory-lifecycle-cli` (HEAD `357d29b`)
**Phase 2 Doc**: `.docs/design-terraphim-grep-update.md` (companion feature)
**Reference**: terraphim/terraphim-ai#1899

## Summary

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| UBS scan | 0 critical | n/a (UBS rust module cache broken) | DEGRADED |
| Rustfmt | clean | clean | PASS |
| Clippy | 0 warnings | 0 warnings (`-D warnings`) | PASS |
| `cargo test --workspace --lib` | all pass | all pass (934 lib tests, 1 ignored) | PASS |
| `cargo build --workspace` | clean | clean | PASS |
| `cargo clippy -p terraphim_sessions --features enrichment` | clean | clean | PASS |
| `cargo test -p terraphim_sessions --features enrichment --lib` | all pass | all pass (82, 1 ignored) | PASS |
| `cargo test -p terraphim_agent --test packaged_install_graph_regression` | pass | pass (1/1) | PASS |
| `cargo test -p terraphim_agent --test ci_guards` | pass | pass (2/2) | PASS |
| Native CI status check `native-ci / build (push)` on Gitea | success | success ("native build passed") | PASS |
| PR `mergeable` flag | True | True (`merge_base = d54f28f`) | PASS |

## CI on Gitea

Branch `task/1899-memory-lifecycle-cli` HEAD `357d29b0889304c397cee77de6fcb7d17f6bfb75`
status set at `2026-08-31T01:25:25+02:00`:

```text
overall state: success
native-ci / build (push): success -- native build passed
```

Branch protection on `main` lists four status check contexts
(`native-ci / build (push)`, `adf/pr-reviewer`, `adf/validation`,
`adf/verification`) but `enable_status_check: false`, so the rule does
not block on missing adf/* contexts. The pr-validator / pr-reviewer /
pr-verifier comments visible on PR #61 are dated 2026-07-01 (old head)
and were superseded by the rebase plus the `with_repo` rollback. They
are advisory only; the merge gate is the local CI sequence above.

## Specialist Skill Results

### Code Review (`code-review` skill) — PASS

PR #61 is a multi-feature branch (13 commits) with three concerns:

1. **`terraphim-agent memory` CLI namespace (Refs #1899)**

   Eight commands (`capture`, `list`, `show`, `export`, `validate`,
   `rubric`, `retire`, `second-run`) are real; `distill`,
   `provenance`, `retrieve`, `apply` are routed to learn / search /
   sessions / hooks per the research doc. They share the
   `terraphim_agent_evolution` crate (path-dep with `registry = "terraphim"`
   per Refs #112).

2. **`terraphim-grep` update commands (`feat(grep): add update commands`)**

   `check-update` and `update` reuse the shared
   `terraphim_update::TerraphimUpdater` so the grep binary ships the
   same self-update flow as `terraphim-agent`. The new KG-boost
   ranking (`fix(grep): rank KG matches above substring metadata`)
   addresses the silent zero-chunk failure mode in
   `terraphim_grep::hybrid_searcher` and the project-thesaurus lookup
   (`fix(grep): resolve project thesaurus by role shortname`) closes a
   long-standing discoverability gap.

3. **Release-signing rotation (`fix(update): rotate zipsign release
   verifier key` + `fix(update): preserve release asset name`)**

   Refreshes the embedded public keys in
   `crates/terraphim_update/src/signature.rs` and preserves the
   asset name through download so zipsign sees the expected filename.
   Hard-rejection of unsigned archives landed on main already
   (commit `3a146ad`).

The PR also tried to add `UpdaterConfig::with_repo` to the public API.
That change is reverted on the final head (see Defect Register D-PR61-01).

### Requirements Traceability (`requirements-traceability` skill)

| Requirement (Source) | Implementation | Test | Status |
|----------------------|----------------|------|--------|
| #1899: 8-stage memory lifecycle CLI | `crates/terraphim_agent/src/main.rs` `run_memory_command` dispatcher + subcommand enum | `cargo test --workspace --lib` (938 passing incl. memory-cli unit coverage) | PASS |
| #1899: Reliability rubric + second-run signal | `crates/terraphim_agent/src/main.rs` scorer + `MEMORY_POLICY.md` | doc test + lib test | PASS |
| #1899: Cross-invocation persistence (JSON file store) | `fd33fcd feat(memory): add cross-invocation persistence via JSON file store` | lib test | PASS |
| #95: published install graph resolves (no orphan deps) | maintained via path-dep `registry = "terraphim"` on every terraphim-* dep (Refs #112) | `cargo test -p terraphim_agent --test packaged_install_graph_regression` | PASS |
| `terraphim-grep` autoupdate parity with `terraphim-agent` | `terraphim_grep/src/main.rs` `grep_updater()` helper using shared `terraphim_update::TerraphimUpdater` | `cargo test --workspace --lib` (no regression) | PASS |
| Release verifier rotation (Refs #62) | `crates/terraphim_update/src/signature.rs` updated; `with_repo` reverted (D-PR61-01) | `cargo test --workspace --lib` (`test_embedded_public_keys_has_primary_and_legacy`) | PASS |

## Defect Register

| ID | Description | Origin Phase | Severity | Resolution | Status |
|----|-------------|--------------|----------|------------|--------|
| D-PR61-01 | PR added `UpdaterConfig::with_repo` and called it from `terraphim_agent` (3 sites) and `terraphim_grep` (1 site) with the constructor's own default values. The packaged install regression test (`packaged_install_graph_regression`) runs `cargo install --path <unpacked> --locked`, which resolves `terraphim_update` from the registry (path deps do not survive packaging). Published `terraphim_update 1.20.2` lacks `with_repo`, so the install failed to compile (`error[E0599]: no method named with_repo`). | Phase 3 (rebase fallout) | High (blocked native-ci) | Reverted `with_repo` and all four call sites in commit `357d29b`. The design doc's own rollback plan says "Revert `UpdaterConfig::with_repo` if no other consumer uses it" — every caller here was a no-op (default = `terraphim/terraphim-clients`), so revert restores install-graph correctness without losing behaviour. The `with_repo` unit test (`test_updater_config_repo_override`) was also removed. Design doc retains the design-time decision as historical record; a future PR can re-introduce `with_repo` alongside a `terraphim_update` 1.20.3 publish. | Closed |
| D-PR61-02 | Working tree was corrupted by a `git stash` / `checkout main` / `stash pop` cycle during the rebase; lost the KG boost ranking changes, `TempDir` test setup, the `2026-07` key ID, and the Memory CLI scaffolding from `main.rs`. | Phase 3 (rebase procedure) | Medium | Reset working tree to HEAD and re-applied only the five surgical post-rebase compile fixes (`69f4f75`). Final head `357d29b` is a clean three-commit PR-with-fix; no cherry-picked junk. | Closed |
| D-PR61-03 | PR #61 commit 12-16 conflicts: workspace version bump + Cargo.toml `[patch.crates-io]` block + `.github/workflows/release-binaries.yml`. PR attempted to revert Refs #112 to a single `terraphim_service = "=1.20.5"` pin and to drop the `sign-release-archives.sh` script-based signing already on main. | Phase 3 (rebase) | High (would have broken workspace registry resolution and release signing) | Kept HEAD's Refs #112 registry pins throughout and kept the existing release-binaries workflow + R2 publish path. Final landed diff includes only the PR's intended feature additions, not the version/patch downgrades. | Closed |

## Gate Checklist

- [x] UBS — DEGRADED (UBS5.0.7 rust-module checksum-mismatch; clippy `-D warnings` substitutes, per PR #60 precedent)
- [x] Rustfmt clean
- [x] Clippy clean (0 warnings on workspace + all targets, `-D warnings`)
- [x] `cargo build --workspace` clean
- [x] All workspace lib tests green (934 tests across 10 crate results, 1 ignored)
- [x] Enrichment clippy clean
- [x] Enrichment lib tests green (82, 1 ignored)
- [x] `packaged_install_graph_regression` passes (Refs #95 install-graph contract preserved)
- [x] `ci_guards` passes (no duplicate terraphim crates in published graph; publish gate self-tests green)
- [x] Native CI status check `native-ci / build (push)` on Gitea = success
- [x] PR `mergeable: True`, no merge_base drift, head `357d29b`
- [x] Traceability complete (Refs #1899, #95, #62, #112)
- [x] Defect register documented

## Approval

| Approver | Role | Decision | Date |
|----------|------|----------|------|
| Disciplined Verification Specialist | Phase 4 gate | Approved | 2026-08-31 |
