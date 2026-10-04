# Validation: terraphim/terraphim-clients#313 — coverage-nextest (round-3 re-validation)

**Status:** Passed.
**Validator:** Alex (via disciplined-validation skill)
**Branch:** `task/313-coverage-nextest`
**Base:** `main`
**Date:** 2026-09-16
**Scope:** CI-only — `.gitea/workflows/native-ci.yml`, `.github/workflows/ci.yml`, `BUILD.md`, `crates/terraphim_agent/tests/ci_guards.rs`, plan artefacts under `docs/plans/`.
**Supersedes:** the prior validation at the same path that reported `passed: false` against the round-1 head (before `9adbbeaa`). The round-1 P1 and the two highest-impact P2 findings have been addressed; this round-3 re-validation re-runs the acceptance criteria against the current head and includes the round-2 structural review's five P2 findings.

---

## What changed since the round-1 validation

| Round | Commit | What it changed |
|---|---|---|
| 1 | `c3e723a` (workflow output) | Initial GH lane. Replaced `cargo test --workspace --lib` with the coverage step. |
| 2 | `9adbbeaa` | Restored the GH `cargo test --workspace --lib --no-fail-fast` step; pinned `taiki-e/install-action@v2` with explicit `tool: cargo-llvm-cov@v0.6.16,nextest@v0.9.144`; added the `Check host CA bundle` step on GH. |
| 3 | this branch (pending) | Split the native install into three steps (P2-1); added `coverage_tool_pinning_matches_local_toolchain` ci_guards test and bumped the GH pin to `cargo-llvm-cov@v0.8.5` so the local + GH toolchains agree (P2-2); reconciled the design's `Diff Sketch` and `Modified Files` table to match the additive pattern (P2-3). |

---

## Acceptance Criteria Audit (mapped from Gitea #313 + design `§Acceptance Criteria`)

The design (`docs/plans/design-coverage-nextest.md` §"Acceptance Criteria", lines 456-462 in the original numbering) and the research (`docs/plans/research-coverage-nextest.md` §5) define acceptance criteria. Evidence per criterion, against the round-3 head:

| # | Criterion | Status | Evidence (file:line) |
|---|-----------|--------|---------------------|
| 1 | Both workflows install or use `cargo-llvm-cov` and `cargo-nextest`; `llvm-tools-preview` present | PASS | Native: `native-ci.yml:39-43` — three separate install steps (`Install cargo-llvm-cov`, `Install cargo-nextest`, `Add llvm-tools-preview`). GH: `ci.yml:28-30` — `taiki-e/install-action@v2` with `tool: cargo-llvm-cov@v0.8.5,nextest@v0.9.144`. `llvm-tools-preview` is added on first invocation of `cargo-llvm-cov` via `rustup component add`. |
| 2 | Both workflows preset `SSL_CERT_FILE` and `SSL_CERT_DIR` at the workflow `env:` level | PASS | Native: `native-ci.yml:8-13`. GH: `ci.yml:12-15`. Real CA bundle path on Debian/Ubuntu runners; not `/dev/null`. |
| 2a | Both workflows include a `test -f` guard with `::error::` annotation on miss | PASS | Native: `native-ci.yml:32-34`. GH: `ci.yml:36-40` (added in round-2 `9adbbeaa`). Both use the allowlist-safe `test -f X \|\| { echo ::error::...; exit 1; }` shape. |
| 3 | Both workflows invoke `cargo llvm-cov nextest ...` (not in-process `cargo llvm-cov ...`) | PASS | Native: `native-ci.yml:109-113`. GH: `ci.yml:55-56`. Both use the `nextest` subcommand. |
| 4 | Native preserves `--workspace --all-targets`; GH preserves `--workspace --lib` | PASS | Native: `--workspace --all-targets`. GH: `--workspace --lib`. GH rationale (no Gitea registry creds for `terraphim_server`-dependent integration tests) documented at `ci.yml:51-53`. |
| 5 | Both workflows upload `lcov.info` via `actions/upload-artifact@v4` | PASS | Native: `native-ci.yml:115-117` (`name: lcov-native`, `path: lcov.info`). GH: `ci.yml:58-60` (`name: lcov-gh`, `path: lcov.info`). |
| 6 | Existing `cargo test ...` lanes remain in place and continue to pass | PASS | Native: `cargo test --workspace --all-targets` (line 76) and four focused re-runs (lines 84-86, 89, 94, 99) preserved. GH: `cargo test --workspace --lib --no-fail-fast` (line 50) preserved; `cargo test -p terraphim_sessions --features enrichment --lib` (line 62), `cargo test -p terraphim_grep --test default_feature_smoke` (line 64), `cargo test -p terraphim_agent --test packaged_install_graph_regression` (line 66) preserved. Local `cargo test --workspace --lib --no-fail-fast` reports 1086 passed / 0 failed / 1 ignored (the existing `#[ignore]` on `terraphim_cli` binary's default-features probe). |
| 7 | No shell `if` / `then` / `fi` as literal first token on native | PASS | `terraphim-grep`-style audit confirms; no `if`/`then`/`fi`/`elif` first tokens. The `Check host tooling (zipsign)` and `Check host CA bundle` steps use `test -x` and `test -f` with `\|\|` chaining exclusively. |
| 8 | No `SSL_CERT_FILE=/dev/null` anywhere | PASS | Only match in the diff is the comment "Never SSL_CERT_FILE=/dev/null" at `native-ci.yml:31` (the design's forbidden-pattern reminder). |
| 9 | `BUILD.md` documents the coverage command for both runners | PASS | `BUILD.md` lines 17-32. British English, no emoji, real `SSL_CERT_FILE` paths. |
| 10 | Comments use British English, no emoji | PASS | Comments reviewed: "behaviour", "centre", "artefact" (where applicable), "presetting", "instrumented subprocesses", "deterministic-install". No emoji (`terraphim-grep --haystack code '\p{Extended_Pictographic}'` returns zero matches in the changed files). |
| 11 | First coverage run produces a downloadable `lcov.info` artefact with workspace crate coverage lines | PASS (local smoke); UNVERIFIED on actual runner (post-merge gate) | Local `cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path /tmp/lcov-round3.info`: 1086 PASS, 42159-line lcov.info, 108 SF: records. The post-merge smoke on `terraphim-native` is required for the `--workspace --all-targets` lane and the `lcov-native` artefact download (cannot be exercised locally because the four `terraphim_agent` integration tests need `TERRAPHIM_SERVER_BIN` from the `cargo install --git ...` step that only runs on the native runner). |
| 12 | Pinned version discipline — native uses `cargo install --locked`; GH uses `taiki-e/install-action@<tool>` | PASS | Native: `cargo install cargo-llvm-cov --locked --root /usr/local`, `cargo install cargo-nextest --locked --root /usr/local`. GH: `taiki-e/install-action@v2` with `tool: cargo-llvm-cov@v0.8.5,nextest@v0.9.144` (bumped from `v0.6.16` after `coverage_tool_pinning_matches_local_toolchain` in `crates/terraphim_agent/tests/ci_guards.rs` fired the drift assertion). The drift test is now in `ci_guards.rs` and runs as part of `cargo test -p terraphim_agent --test ci_guards`, so a future local upgrade that does not bump the GH pin fails CI. |
| 13 | Round-2 structural review findings are resolved | PASS | P1 (GH cargo test step replaced) fixed in `9adbbeaa`. P2 (unpinned install-action, missing GH CA guard, misleading comment) fixed in `9adbbeaa`. P2 (single install chain) fixed in round-3 by splitting into three steps. P2 (lockfile-decoupled GH pin) fixed in round-3 by the `coverage_tool_pinning_matches_local_toolchain` test. P2 (design doc contradicts head) fixed in round-3 by updating `Diff Sketch` and `Modified Files` to match the additive pattern. |

---

## Static Gates

### `cargo fmt --all -- --check`
PASS. Exit 0.

### `cargo clippy --workspace --all-targets -- -D warnings`
PASS. Exit 0. Only pre-existing manifest warnings (`terraphim_grep/Cargo.toml` and `terraphim_agent/Cargo.toml` each declare both `license` and `license-file`; the `rustls-webpki` patch in `Cargo.lock` is not used in the crate graph).

### `cargo test --workspace --lib --no-fail-fast` (GH lane)
PASS. 1086 passed / 0 failed / 1 ignored. The ignored test is `terraphim_cli`'s default-features probe (`#[ignore]` attribute); not a fail-fast bypass.

### `cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path /tmp/lcov-round3.info` (GH coverage lane)
PASS. 1086 PASS, 1 skipped. `lcov.info` is 42159 lines with 108 SF: records.

### `cargo test -p terraphim_agent --test ci_guards -- --nocapture` (new drift test)
PASS. 3 passed / 0 failed (`no_duplicate_terraphim_crates`, `publish_gate_tests_pass`, `coverage_tool_pinning_matches_local_toolchain`).

### Workflow lint
PASS for both YAMLs: 20 steps on native / 15 on GH, no shell-keyword first tokens, both `cargo llvm-cov nextest` invocations present, both `actions/upload-artifact@v4` uploads present, GH `cargo test --workspace --lib` preserved, native install split into 3 steps, no `SSL_CERT_FILE=/dev/null` matches outside the forbidden-pattern reminder comment.

---

## Round-2 P2 findings (resolved)

| # | Finding | Resolution |
|---|---------|-----------|
| P2-1 | Single `&&`-chained install step can fail-fast on transient network errors | Split into three named steps in `native-ci.yml:39-43`: `Install cargo-llvm-cov (pinned via Cargo.lock)`, `Install cargo-nextest (pinned via Cargo.lock)`, `Add llvm-tools-preview component`. A transient network failure on one install can now be retried without rerunning the others. |
| P2-2 | GH install-action tool versions decoupled from the workspace's `Cargo.lock` hash | Added `coverage_tool_pinning_matches_local_toolchain` in `crates/terraphim_agent/tests/ci_guards.rs`. The test reads the GH `with: tool:` block from `.github/workflows/ci.yml`, runs `cargo llvm-cov --version` and `cargo nextest --version` locally, strips the leading `v` from the GH pin to match the local output format, and asserts equality. Negative path verified: flipping the pin from `v0.8.5` to `v0.6.16` fires the assertion with an actionable message. Bumped the GH pin to `cargo-llvm-cov@v0.8.5,nextest@v0.9.144` to match the local toolchain. |
| P2-3 | Design doc's `Diff Sketch` and `Modified Files` still describe the replaced-step pattern | Updated `design-coverage-nextest.md` lines 169 (Modified Files GH row), lines 244-289 (GH Diff Sketch — including the additive test step restored and an explicit "do NOT collapse this step" comment for future readers), and the Workflow API block to show the three-step native install and the additive GH test+coverage pattern. Cross-referenced `9adbbeaa` so the history is preserved. |
| P2-4 | Stale `validation-coverage-nextest.md` verdict citing round-1 P1 | This document supersedes the prior validation. The round-1 P1 and the two highest-impact round-1 P2s are confirmed fixed against the round-2 head; the round-2 P2s are confirmed fixed against the round-3 head. |
| P2-5 | Design's Lifecycle Artefacts table names only `docs/verification/verification-report-coverage-nextest.md`; the PR commits at `docs/plans/verification-coverage-nextest.md` | Updated in round-3 (see `design-coverage-nextest.md` Lifecycle Artefacts table): both paths are now named with the labels "in-PR verification (committed at #327)" and "post-merge smoke evidence (closes the gate)". The post-merge doc is intentionally absent from the PR; it is written after the lanes run on `main`. |

---

## Deferred Product Validation (Recorded)

The change is **CI-only**: no Rust source touched (other than the `ci_guards` drift test, which is itself a CI gate, not a behaviour change), no new APIs, no user-visible behaviour change. The verification report confirms (`docs/plans/verification-coverage-nextest.md`) the same scope.

- **First-coverage-run smoke on `terraphim-native`.** Locally exercised the GH coverage command (`--workspace --lib`) end-to-end with 1086 PASS / 1 skipped / 42159-line lcov.info / 108 SF: records. The native coverage command (`--workspace --all-targets`) was **not** exercised locally because the dev box lacks `TERRAPHIM_SERVER_BIN` for the four `terraphim_agent` integration tests. **Owner: Alex. Gate: post-merge smoke on a throwaway branch; write `docs/verification/verification-report-coverage-nextest.md` with the downloaded `lcov-native` artefact SHA and the SF: count.**
- **`terraphim-native` runner OS assumption (Debian vs RHEL).** The design (`design-coverage-nextest.md:67`, Open Item 2) presumes `/etc/ssl/certs/ca-certificates.crt` (Debian/Ubuntu). If the runner is RHEL-based, the path must change to `/etc/pki/tls/certs/ca-bundle.crt`. The HANDOVER referenced at `native-ci.yml:9` is the authoritative source. **Owner: Alex. Gate: confirm via HANDOVER before merge. If RHEL, edit the env value to the RHEL path; the `test -f` guard will catch the mismatch early.**
- **Codecov upload + threshold gating.** Out of scope per the design (`design-coverage-nextest.md:88`, Open Item 6). The first coverage run establishes a baseline; threshold tuning and Codecov upload are follow-up issues.

---

## Reality Checks

- **No mocks in any test.** `cargo llvm-cov nextest` runs real test binaries under instrumentation. `coverage_tool_pinning_matches_local_toolchain` invokes the real `cargo llvm-cov` and `cargo nextest` binaries. No mock, no fixture stub, no fake source file.
- **No timeout escalation.** All gates use the workspace's default test profile; no `--test-threads` or per-test timeout was raised. The `--no-fail-fast` flag is preserved.
- **British English in workflow comments and `BUILD.md`.** Comments reviewed: "behaviour", "centre", "artefact", "presetting", "instrumented subprocesses", "deterministic-install", "transient", "idempotent", "mutable", "autodetect". No American-English slips found in the diff.
- **No emoji** in any diff line.
- **Runner command allowlist honoured.** All `run:` step first tokens are `cargo`, `test`, or `TERRAPHIM_SERVER_BIN=...` (which evaluates to `cargo ...`). No `if` / `then` / `fi` / `elif` / `for` / `while` first tokens. The allowlist at `crates/terraphim_agent/tests/ci_guards.rs:11` is satisfied.
- **Git history is preserved** (no force-push, no rebase over `main` since the branch was cut; conventional-commit messages match the design's commit-level plan; three round-2/round-3 follow-up commits added on top).
- **No timeout in command line** (global policy from `~/.claude/Claude.md`).
- **`SSL_CERT_FILE=/dev/null` audit clean** — only one match in the entire diff and it is the comment "Never SSL_CERT_FILE=/dev/null" at `native-ci.yml:31` (the forbidden-pattern reminder).

---

## Verdict

**passed.** All acceptance criteria from `design-coverage-nextest.md` satisfied against the round-3 head. Round-1 P1 and round-1 P2s (unpinned install-actions, missing GH CA guard) fixed in `9adbbeaa`. Round-2 P2s (single install chain, lockfile-decoupled GH pin, design doc contradiction, stale validation, doc path mismatch) fixed in this round-3 batch.

**Evidence:**

- `.gitea/workflows/native-ci.yml:8-13` (env preset, real CA bundle path)
- `.gitea/workflows/native-ci.yml:32-34` (Check host CA bundle step)
- `.gitea/workflows/native-ci.yml:39-43` (three named install steps; --locked against Cargo.lock)
- `.gitea/workflows/native-ci.yml:76` (existing `cargo test --workspace --all-targets` preserved)
- `.gitea/workflows/native-ci.yml:109-117` (coverage step additive; lcov-native artefact)
- `.github/workflows/ci.yml:12-15` (env preset)
- `.github/workflows/ci.yml:28-30` (pinned `taiki-e/install-action@v2` with tool versions matching local)
- `.github/workflows/ci.yml:36-40` (Check host CA bundle step)
- `.github/workflows/ci.yml:50` (existing `cargo test --workspace --lib` restored in `9adbbeaa`)
- `.github/workflows/ci.yml:55-56` (coverage step additive; same `--workspace --lib` as test lane)
- `.github/workflows/ci.yml:58-60` (upload-artifact correct)
- `BUILD.md:17-32` (Coverage section correct)
- `crates/terraphim_agent/tests/ci_guards.rs` (new `coverage_tool_pinning_matches_local_toolchain` test, drift assertion verified negative path)
- `docs/plans/design-coverage-nextest.md:169` (Modified Files GH row reconciled with head)
- `docs/plans/design-coverage-nextest.md:244-289` (GH Diff Sketch reconciled)
- `docs/plans/design-coverage-nextest.md` Workflow API block (three-step native install; additive GH test+coverage)

---

## Next Actions

1. **Push round-3 commit(s) via Gitea Contents API** (HTTPS `git push` 401s on cached `osxkeychain` credentials; this is the same workaround used for `9adbbeaa`).
2. **Post a "Round-3 P2 fixes applied" status comment** on PR #327 and issue #313 summarising the five P2 fixes and re-running this validation's evidence.
3. **Wait for the merge gate (human).** The post-merge smoke on both runners is the design's Close gate and cannot be exercised locally. The lanes are designed to fail loudly (test signal preserved if coverage toolchain breaks; CA bundle guard emits a `::error::` annotation if the bundle path is missing; coverage tool pinning fails CI on drift).
4. **After merge**, write `docs/verification/verification-report-coverage-nextest.md` with the downloaded `lcov-native` artefact SHA and the SF: count.
