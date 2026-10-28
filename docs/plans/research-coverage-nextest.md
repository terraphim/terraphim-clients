# Research: Switch Coverage Lanes to `cargo llvm-cov nextest` + Preset SSL Cert Env

**Status:** Draft
**Author:** Alex (via disciplined-research skill)
**Date:** 2026-09-15
**Gitea:** terraphim/terraphim-clients#313
**Slug:** `coverage-nextest`
**Scope:** CI-only (`.gitea/workflows/native-ci.yml`, `.github/workflows/ci.yml`)
**Related:** #254 (track), EXP-102 (Lead addendum 2 mitigation)

---

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energising? | Partial | The repo currently has **no coverage lane** in either CI workflow; this issue is mostly preventive/structural — making sure the first coverage lane uses `cargo llvm-cov nextest` from day one rather than the older `cargo llvm-cov` invocation that drives the test binary in-process. |
| Leverages strengths? | Yes | The `terraphim-native` runner already runs `cargo test --workspace --all-targets --no-fail-fast` and the GH `ubuntu-latest` runner runs a parallel `cargo test --workspace --lib`. Both can be promoted to `cargo llvm-cov nextest` without restructuring test code. |
| Meets real need? | Yes | `nextest` is already used in `terraphim-ai` (`cargo nextest run --target ... --workspace --exclude terraphim_agent --profile ci`) and is documented as "nextest is already installed in that" runner (research-session-test-parity-2026-09 §1224). Using it here aligns with the rest of the monorepo and the Lead addendum 2 mitigation for EXP-102 (certificate verification failures during in-process coverage runs that download crates from the Gitea registry). |

**Proceed:** Yes (3/3).

---

## 1. Problem Statement

### 1.1 What the issue actually says

> "ci: switch both coverage lanes from in-process cargo llvm-cov to cargo llvm-cov nextest; preset SSL_CERT_FILE/SSL_CERT_DIR in CI (Lead addendum 2, EXP-102 mitigation; #254 track)"

Two asks, both scoped to CI:

1. Replace any current in-process `cargo llvm-cov ...` invocation in the two CI workflows with `cargo llvm-cov nextest ...`.
2. Preset `SSL_CERT_FILE` / `SSL_CERT_DIR` in the CI env so that instrumented processes can reach the Gitea `cargo:` registry over HTTPS without tripping on a missing CA bundle. This is the "Lead addendum 2" mitigation referenced in EXP-102.

### 1.2 Why it matters

- `cargo llvm-cov` (without `nextest`) drives the test binary in-process via a `cargo test`-like wrapper. It cannot exploit nextest's per-test binary isolation, retry, slow-timeout, or feature-grouping, and it loses process-level coverage granularity that nextest enables.
- `cargo llvm-cov nextest` runs each test binary in its own process, which lets llvm-cov emit per-test `*.profraw` files with reliable attribution. It also inherits the `ci` nextest profile (slower timeouts, no fail-fast) already adopted in the wider monorepo.
- The `terraphim-native` runner sits on `bigbox`, which (per the HANDOVER comment in `native-ci.yml` line 9) carries host tooling (`zipsign`) at `/usr/local/bin/`. The certificate bundle that the runner uses for outbound HTTPS to `git.terraphim.cloud` is the same one `cargo install` already consumes when pulling the workspace's `[patch.crates-io]` registry sources from `https://git.terraphim.cloud/api/packages/terraphim/cargo/`. Without `SSL_CERT_FILE` / `SSL_CERT_DIR`, llvm-cov's child processes fall back to rustls's compiled-in webpki roots, which are not always in sync with the runner's host CA bundle, and EXP-102 manifested as spurious handshake failures on instrumented test runs.

### 1.3 Out of scope

- Adding a coverage lane to a third workflow (e.g. nightly) — only the existing two (`native-ci.yml`, `ci.yml`) are in scope.
- Changing test code, fixtures, or `cargo test` invocations in the workspace.
- Replacing `cargo test` with `cargo nextest` for non-coverage lanes (Lane A from `design-session-test-suite-2026-09` already proposed this for `terraphim_sessions`, but that is a separate effort).
- Publishing the coverage report to Codecov/Coveralls — the issue does not request artefact upload.
- Modifying `[patch.crates-io]` or `terraphim-types 1.21.x` registry pins.

---

## 2. Current State Analysis

### 2.1 What exists today

| Path | Has coverage lane? | Test driver | Notes |
|------|--------------------|-------------|-------|
| `.gitea/workflows/native-ci.yml` (terraphim-native runner on bigbox) | **No.** Grep for `llvm-cov`, `cargo cov`, `coverage` returns zero hits. | `cargo test --workspace --all-targets --no-fail-fast` plus focused re-runs for `cross_mode_consistency_test`, `integration_tests`, `kg_ranking_integration_test`, plus `cargo test -p terraphim_sessions --all-features`. | Installs `terraphim_server` from the v1.21.3 git tag with `--locked` and an explicit `terraphim` registry credential provider. Reuses the workspace's `.cargo/config.toml` is *not* possible — `cargo install` runs in an isolated context. |
| `.github/workflows/ci.yml` (ubuntu-latest) | **No.** Same grep is empty. | `cargo test --workspace --lib --no-fail-fast` + enrichment-feature + grep smoke + packaged install regression. | No `cargo install` of `terraphim_server`; relies on `--lib` only because the workspace has no GH-hosted secrets for the registry. |
| `BUILD.md` | Documents the canonical CI command set; no coverage lane. | `cargo test --workspace --no-fail-fast`. | Authoritative command set for both the ADF build-runner and the future native runner. |
| `crates/terraphim_agent/tests/fixtures/memory_bench/{queries,corpus,jsonl}` | Mentions `cargo llvm-cov` only as a *historical failed-command* that the memory bench records; not a real invocation. | n/a. | n/a. |
| `crates/terraphim_agent/commands/test.md`, `record_demo.sh`, `demo_script.sh` | User-facing "test --coverage" flag inside the agent command system; not a CI lane. | n/a. | Out of scope. |

The only place in the monorepo where `cargo nextest` is already part of CI is `terraphim-ai` (`.github/workflows/rust-build.yml` — see the `Install cargo-nextest` and `Run basic tests` steps), where the nextest `ci` profile is used for the full workspace with `terraphim_agent` excluded. There is **no coverage lane there either**, so this issue is genuinely introducing the first coverage lane in the entire monorepo.

### 2.2 Code locations that touch the surface

- `/Users/alex/projects/terraphim/terraphim-clients/.gitea/workflows/native-ci.yml` — the Gitea Actions workflow that runs on `terraphim-native`. Already sets `TERRAPHIM_SERVER_BIN` env var inline on the relevant test steps.
- `/Users/alex/projects/terraphim/terraphim-clients/.github/workflows/ci.yml` — the GitHub-style workflow, ubuntu-latest. Smaller subset of tests.
- `/Users/alex/projects/terraphim/terraphim-clients/BUILD.md` — the canonical command set. If a coverage lane is added, this file should mention the new command so the ADF build-runner and any future native runner stay in sync.
- `/Users/alex/projects/terraphim/terraphim-clients/Cargo.toml` — workspace `[patch.crates-io]` block. Coverage tools do not change this, but any new env vars (e.g. `SSL_CERT_FILE`) must not collide with the existing `CARGO_*` set used by `cargo install` steps in `native-ci.yml` (lines 41–48: `--config 'registries.terraphim.index=...'` and `--config 'registry.global-credential-providers=...'`).
- `/Users/alex/projects/terraphim/terraphim-clients/crates/terraphim_agent/tests/ci_guards.rs` (line 11) — establishes that "every other terraphim repo's `native-ci` runs `cargo` and nothing else" and that the runner allowlist rejects any program that is not `cargo`. This constrains what we can put in a coverage step: `cargo llvm-cov` (a `cargo` subcommand) and `cargo nextest` are both on the allowlist; arbitrary scripts are not.

### 2.3 Existing behaviour

- Both CI workflows already invoke cargo with the `terraphim` registry (Gitea) for build/test. The native-ci workflow in particular uses `cargo install --locked --git https://git.terraphim.cloud/terraphim/terraphim-ai --tag v1.21.3 --config 'registries.terraphim.index="sparse+https://git.terraphim.cloud/api/packages/terraphim/cargo/"' --config 'registry.global-credential-providers=["cargo:token"]'` (line 45). That is the same surface that EXP-102 hit: the `cargo install` path, after spawning instrumented child processes, must reach the Gitea registry over HTTPS using the runner's CA bundle.
- `terraphim-native` is self-hosted (label `runs-on: terraphim-native`). It carries host tooling at `/usr/local/bin/zipsign` and uses a CARGO_HOME that is not on `~/.cargo/bin`. CA-bundle location is host-specific and not documented in this repo.
- `ubuntu-latest` runners use the runner image's default CA bundle at `/etc/ssl/certs/ca-certificates.crt`. They do *not* need `SSL_CERT_FILE` unless the image's bundle is wrong, but presetting it is harmless and uniform across both runners.

### 2.4 Risks

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| `cargo-llvm-cov` is not installed on the `terraphim-native` runner. | High — neither workflow installs it today. | Add `cargo install cargo-llvm-cov --locked --root /usr/local` (matching the `zipsign` precedent in line 9) before the coverage step. Use `--locked` to pin to the version used elsewhere in the monorepo. |
| `cargo-nextest` is not installed on `ubuntu-latest`. | Medium — terraphim-ai uses self-hosted linux, not ubuntu-latest. | Add `cargo install cargo-nextest --locked --root /usr/local` (or use `taiki-e/install-action@nextest` for the GH side). |
| `cargo llvm-cov nextest` requires `llvm-tools-preview` (`llvm-cov` + `llvm-profdata`). | High on `ubuntu-latest`. | Add `rustup component add llvm-tools-preview` (the same component `taiki-e/install-action@cargo-llvm-cov` would install). |
| `cargo llvm-cov nextest` instruments all binaries including the integration tests that shell out to `terraphim_server`. The instrumented `cargo test` step (line 50 of `native-ci.yml`) currently exports `TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server`. The `cargo` registry credential provider flow needs `GITEA_TOKEN` in env; coverage instrumented processes inherit env, but if any spawned subprocess spawns another shell that calls `cargo` it may re-fetch crates. | Medium. | Keep `GITEA_TOKEN` exported on the coverage step. Pass `--no-clean` only if cross-process artefacts interfere. |
| Coverage step fails on the `terraphim_session-analyzer` crate or the `terraphim-negative_contribution` crate because they have unusual feature sets. | Low — both are workspace members, so `--workspace` covers them. | Document `--workspace` to make coverage exhaustive. |
| The runner's `cargo install` is rate-limited or sandboxed; running `cargo install cargo-llvm-cov` may be slow. | Low — the runner already does one `cargo install --locked --git ...` per build. | Pin a specific version to avoid reinstall churn. |
| Presetting `SSL_CERT_FILE=/dev/null` (a known "disable verification" anti-pattern) by accident. | Low — but the failure mode is silent and dangerous. | Use the concrete path `/etc/ssl/certs/ca-certificates.crt` (Debian/Ubuntu) and `/etc/pki/tls/certs/ca-bundle.crt` (RHEL-style) as a fallback chain via an `if [ -f ... ]` guard, with a clear `::error::` if neither exists. **Never** use `/dev/null`. |
| The first coverage run produces a noisy diff and reviewers expect a coverage floor (threshold). | Medium. | The issue title says "switch" — the simplest interpretation is to not introduce a threshold and just emit a report. Optionally follow up with a threshold in a separate issue. |

### 2.5 Constraints

- **Runner command allowlist.** `native-ci.yml` line 15 establishes that the runner inspects the literal first token and rejects anything outside the allowlist. The only conditional primitive on the allowlist is `test`. `cargo` (and therefore `cargo llvm-cov`, `cargo nextest`) is fine; arbitrary shell like `wget`, `curl`, `openssl` is not. This rules out scripting an HTTP probe to verify the cert chain inside the workflow; the cert path is set and assumed correct.
- **`GITEA_TOKEN` injection.** The native-ci runner already exports `GITEA_TOKEN` (used by the `cargo install --git` step). It must remain exported for any coverage step that triggers a fetch.
- **No mocks in tests** (global policy from `~/.claude/Claude.md`). Coverage tooling cannot use synthetic `*.profraw` shims; the report must come from actually running the workspace tests under instrumentation.
- **No CLI timeout.** (global policy). Any `cargo install` step must rely on nextest's slow-timeout profile (already used by terraphim-ai) rather than bumping the runner timeout.
- **British English** in workflow comments and documentation.
- **No emoji.** `::error::` GH annotation syntax is fine; emoji are not.

---

## 3. Proposed Approach (for the design phase)

### 3.1 Diff summary

1. **`.gitea/workflows/native-ci.yml`**:
   - Pre-step: `cargo install cargo-llvm-cov --locked --root /usr/local && cargo install cargo-nextest --locked --root /usr/local && rustup component add llvm-tools-preview`.
   - Preset env at the workflow `env:` level (so every step inherits it):
     ```yaml
     SSL_CERT_FILE: /etc/ssl/certs/ca-certificates.crt
     SSL_CERT_DIR: /etc/ssl/certs
     ```
     with a `test -f $SSL_CERT_FILE || { echo "::error::CA bundle not found at $SSL_CERT_FILE"; exit 1; }` guard that follows the `zipsign` precedent (line 18).
   - Add a new step after the existing `cargo test --workspace --all-targets` line (or replace the test step for the coverage lane; **issue title says "switch" so the existing test lane stays, the coverage lane is added beside it**):
     ```bash
     cargo llvm-cov nextest --workspace --all-targets --no-fail-fast --lcov --output-path lcov.info
     ```
     This emits `lcov.info` which can be archived as a Gitea Actions artefact.
   - Add a focused re-coverage step for the integration tests that depend on `TERRAPHIM_SERVER_BIN`, mirroring lines 56–63:
     ```bash
     TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server \
       cargo llvm-cov nextest -p terraphim_agent --test cross_mode_consistency_test --no-report
     ```
     followed by `cargo llvm-cov report --lcov --output-path lcov.integration.info`. (Or merge into a single report — design choice.)

2. **`.github/workflows/ci.yml`**:
   - Use `taiki-e/install-action@cargo-llvm-cov` and `taiki-e/install-action@nextest` (these are the GH Actions idioms already used by other Terraphim repos and have rust-toolchain `stable` baked in).
   - Preset `SSL_CERT_FILE: /etc/ssl/certs/ca-certificates.crt` on `env:`.
   - Switch the existing `cargo test --workspace --lib` step to `cargo llvm-cov nextest --workspace --lib --lcov --output-path lcov.info`. This keeps the `--lib` constraint (GH has no registry creds for the integration tests that need `terraphim_server`).

3. **`BUILD.md`**:
   - Append a "Coverage (optional)" section documenting the `cargo llvm-cov nextest` command so the ADF build-runner knowledge graph and any future native runner stay consistent.

### 3.2 What this approach does *not* do

- It does not add a Codecov upload. If the team wants PR comments later, that is a separate issue.
- It does not change any `cargo test` invocation that does not currently produce coverage.
- It does not introduce thresholds. Threshold gating (e.g. `--fail-under-lines 80`) is a follow-up.

### 3.3 Verification plan

- Re-read both workflow files and confirm no shell-keyword first token (`if`, `then`, `fi`) is used outside of `test` (per the runner allowlist precedent).
- Confirm `cargo-llvm-cov` version is pinned via `--locked` to avoid drift.
- Confirm `SSL_CERT_FILE` resolves on both runners (a non-fatal `test -f` probe with a `::error::` annotation on miss).
- Confirm `lcov.info` is uploaded as a workflow artefact (Gitea Actions supports `actions/upload-artifact@v4`).
- Smoke-test locally: `cargo llvm-cov nextest --workspace --all-targets --no-fail-fast --lcov --output-path /tmp/lcov.info` on a developer machine. Expect ~5–10 min build, ~2–3 min test, single `lcov.info` artefact.

---

## 4. Open Questions

1. **Is there an existing coverage lane the title is referring to that I missed?** Grep for `llvm-cov`, `coverage`, `cargo cov` returned zero hits in both workflows and in `BUILD.md`. The only `cargo llvm-cov` references are in test fixture JSONLs (memory bench corpus). If the issue title is forward-looking ("when we add the lane, use this pattern"), the wording in the title is a stub. Worth confirming with Alex.
2. **Which runner image is `terraphim-native`?** The `runs-on: terraphim-native` label hides the OS. The CA-bundle path assumption (`/etc/ssl/certs/ca-certificates.crt`) may need adjustment if the runner is RHEL-based (`/etc/pki/tls/certs/ca-bundle.crt`). The HANDOVER referenced in `native-ci.yml` line 9 is the authoritative source.
3. **Should the coverage report use the `--json` summary output for Gitea PR comments?** Out of scope per the issue title, but a natural follow-up.
4. **Does the `terraphim_session-analyzer` crate (added recently) interact with `cargo-llvm-cov`'s instrumentation correctly?** It has a `reporter.rs` and uses `tempfile` in tests; instrumentation should be transparent, but worth a one-line smoke test.
5. **EXP-102 mitigation is partial.** This issue addresses the certificate side. EXP-102 may also have a "use http instead of https" workaround that should be overridden by this fix. Worth reading EXP-102 directly when accessible.

---

## 5. Acceptance Criteria

- Both `.gitea/workflows/native-ci.yml` and `.github/workflows/ci.yml` install `cargo-llvm-cov` and `cargo-nextest` (or use a pre-installed binary).
- Both workflows preset `SSL_CERT_FILE` (and `SSL_CERT_DIR`) at the workflow `env:` level, with a `test -f` guard that emits a `::error::` annotation if the path is missing.
- Both workflows invoke `cargo llvm-cov nextest ...` instead of `cargo llvm-cov ...` for any coverage step. (`cargo test` is unchanged for non-coverage steps.)
- The coverage step emits an `lcov.info` artefact uploaded via `actions/upload-artifact`.
- No new test failures introduced; existing `cargo test --workspace --all-targets` continues to pass.
- `BUILD.md` documents the coverage command alongside the existing test command.
- Comments in the workflows use British English and contain no emoji.
- A follow-up issue tracks Codecov upload (or equivalent) and threshold gating, if those are wanted.

---

## 6. Cross-references

- `docs/plans/design-session-test-suite-2026-09.md` §3.3 / Lane A — proposed `cargo nextest run -p terraphim_sessions` (separate lane, not in scope here).
- `docs/plans/research-session-test-parity-2026-09.md` §1224 — confirms `terraphim-ai` runs nextest with the `ci` profile; "nextest is already installed in that" runner.
- `crates/terraphim_agent/tests/ci_guards.rs` line 11 — establishes the runner command allowlist constraint (`cargo` and `test` only).
- `.gitea/workflows/native-ci.yml` line 9 — `zipsign` host-tooling precedent that the new install steps should follow in spirit (install to `/usr/local`).
- `cargo-llvm-cov` documentation: `cargo llvm-cov nextest` is the supported entry point for nextest-based coverage (`https://github.com/taiki-e/cargo-llvm-cov`).
- nextest + llvm-cov integration guide (`https://nexte.st/docs/integrations/test-coverage/`).
- terraphim-ai `.github/workflows/rust-build.yml` — the existing in-monorepo reference for nextest-on-CI.
