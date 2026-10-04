# Implementation Plan: Switch Coverage Lanes to `cargo llvm-cov nextest` + Preset SSL Cert Env

**Status:** Draft
**Canonical Path:** `docs/plans/design-coverage-nextest.md`
**Change Slug:** `coverage-nextest`
**Research:** `docs/plans/research-coverage-nextest.md`
**Gitea:** terraphim/terraphim-clients#313 (track #254, mitigation EXP-102 Lead addendum 2)
**Author:** Alex (via disciplined-design skill)
**Date:** 2026-09-15
**Scope:** CI-only (`.gitea/workflows/native-ci.yml`, `.github/workflows/ci.yml`, `BUILD.md`)
**Estimated Effort:** 0.5–1 working day (single PR)

---

## Overview

### Summary

Introduce the monorepo's first coverage lane and align it with EXP-102 Lead addendum 2. Two scoped edits:

1. `.gitea/workflows/native-ci.yml` and `.github/workflows/ci.yml` install `cargo-llvm-cov` and `cargo-nextest`, add `llvm-tools-preview`, preset `SSL_CERT_FILE` / `SSL_CERT_DIR` at the workflow `env:` level (guarded by `test -f` with a `::error::` annotation), and invoke `cargo llvm-cov nextest ...` (instead of any in-process `cargo llvm-cov ...`) to emit `lcov.info` as a workflow artefact.
2. `BUILD.md` documents the canonical coverage command alongside the existing test command so the ADF build-runner knowledge graph and any future native runner stay consistent.

Both workflows keep the existing `cargo test ...` lanes intact — coverage is additive, not a replacement. The native lane covers `--workspace --all-targets` (matching today's `cargo test`); the GH lane stays `--workspace --lib` (GH has no registry creds for the `terraphim_server`-dependent integration tests).

### Approach

Mirror the `zipsign` host-tooling precedent in `native-ci.yml` line 9 for installing `cargo-llvm-cov` and `cargo-nextest` to `/usr/local` (so the runner's `CARGO_HOME` quirk is irrelevant). Use `taiki-e/install-action@cargo-llvm-cov` and `taiki-e/install-action@nextest` on the GH side, which is the idiom already used by `terraphim-ai`. Keep every step's literal first token as `cargo` (or one of the allowlisted GH Actions `uses:` keys) to honour the runner command allowlist documented in `crates/terraphim_agent/tests/ci_guards.rs:11`.

### Scope

**In scope (vital few):**

1. Install coverage toolchain on both runners (`cargo-llvm-cov`, `cargo-nextest`, `llvm-tools-preview`).
2. Preset `SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt` and `SSL_CERT_DIR=/etc/ssl/certs` on both workflows' `env:` blocks, with a `test -f` guard and `::error::` annotation on miss.
3. Replace any in-process `cargo llvm-cov ...` invocation with `cargo llvm-cov nextest ...` for the coverage lane on each workflow.
4. Emit `lcov.info` as a workflow artefact (Gitea Actions and GH Actions both support `actions/upload-artifact`).
5. Append a "Coverage (optional)" section to `BUILD.md`.

**Out of scope:**

- Codecov / Coveralls upload of `lcov.info` (no issue asks for it; PR comments and threshold gating are follow-ups).
- Threshold gating (`--fail-under-lines ...`).
- Replacing `cargo test` with `cargo nextest` for non-coverage lanes (Lane A from `design-session-test-suite-2026-09.md` is the home for that).
- Any change to `[patch.crates-io]` or `terraphim-types 1.21.x` registry pins.
- Adding a coverage lane to a third workflow (nightly / release).
- Modifying `crates/terraphim_server` or `terraphim-ai` consumers.

**Avoid At All Cost:**

- **Never set `SSL_CERT_FILE=/dev/null`.** That is the silent "disable verification" anti-pattern; EXP-102's failure mode is exactly the kind of regression it would mask. Always point at a real CA bundle path.
- **Never introduce a shell `if` / `then` / `fi` as the literal first token of a step on the native runner.** The runner command policy rejects it (`native-ci.yml:19-20` documents the `test`-only conditional primitive). Use `test -f X && ... || { echo "::error::..."; exit 1; }` instead.
- **Never install `cargo-llvm-cov` to `~/.cargo/bin`.** The runner does not always have `~/.cargo/bin` on `PATH`; host tooling belongs at `/usr/local/bin/` (the `zipsign` precedent at `native-ci.yml:9`).
- **Never drop `GITEA_TOKEN` from the coverage step's env.** The instrumented `cargo nextest` re-invokes cargo for the `[patch.crates-io]` registry sources; without the token the Gitea `cargo:` registry handshake fails — a regression of EXP-102 itself.
- **Never replace the existing `cargo test ...` lanes with coverage-only.** Coverage is additive; tests must keep running so failures are visible even when the coverage toolchain is broken.
- **Never pin `cargo-llvm-cov` or `cargo-nextest` without `--locked`.** Pinning prevents silent reinstall churn and is the same discipline the existing `cargo install --locked --git ...` step at `native-ci.yml:45` follows.
- **No mocks in the coverage report.** The report must come from actually running the workspace tests under instrumentation (global policy from `~/.claude/Claude.md`).
- **No emoji** in workflow comments or `::error::` annotations; British English in prose.

### Reality Adjustments vs the Research Artefact

| # | Research assumption | Verified reality | Consequence |
|---|---------------------|------------------|-------------|
| 1 | "the issue introduces the first coverage lane" | Confirmed: grep for `llvm-cov` / `coverage` / `cargo cov` across `.gitea/workflows/`, `.github/workflows/`, `BUILD.md`, and `scripts/` returns zero hits in CI; only the `memory_bench` fixture JSONL mentions `cargo llvm-cov` (a historical failed-command recording, not a CI invocation). | Coverage is genuinely new in CI; the design must be self-contained and not assume a pre-existing lane to "switch". The issue title's "switch" is forward-looking ("when we add it, use nextest"). |
| 2 | `cargo-llvm-cov` / `cargo-nextest` not installed on either runner | Confirmed: no install step exists in either workflow. | Add install steps at the top of each job; match the `zipsign` precedent (`/usr/local`) on native, `taiki-e/install-action` on GH. |
| 3 | `terraphim-native` runner carries host tooling at `/usr/local/bin/zipsign` | Confirmed by `native-ci.yml:9-11` comment. Runner CA-bundle path undocumented in this repo (HANDOVER is authoritative per `native-ci.yml:9`). | Design presumes `/etc/ssl/certs/ca-certificates.crt` (Debian/Ubuntu) with a `test -f` guard; native runner is assumed Debian-family until HANDOVER says otherwise. See Open Item 2. |
| 4 | Runner allowlist restricts steps to `cargo` and `test` | Confirmed by `crates/terraphim_agent/tests/ci_guards.rs:11`. | All step first-tokens are `cargo` or the allowlisted GH Actions `uses:` keys; coverage step uses `cargo llvm-cov nextest` (which is a `cargo` subcommand). |
| 5 | `terraphim-ai` already uses nextest with the `ci` profile | Confirmed by `research-coverage-nextest.md` §2.1 row 5 and the cross-reference to `research-session-test-parity-2026-09.md` §1224. | Native install step can rely on the same `--locked` version of `cargo-nextest` that `terraphim-ai` uses. |
| 6 | `ubuntu-latest` lacks `llvm-tools-preview` | Standard GH Actions `dtolnay/rust-toolchain@stable` does not install it by default. | Add `rustup component add llvm-tools-preview` (or rely on `taiki-e/install-action@cargo-llvm-cov` which adds it). |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|---|---|---|
| Convert both workflows' `cargo test` lanes in place to `cargo llvm-cov nextest` | The issue says "switch" but the title is forward-looking (no existing coverage lane). Replacing `cargo test` would make a coverage-toolchain regression also break the test signal. | Silent loss of test gating when llvm-cov breaks |
| Use `cargo-llvm-cov` (no nextest) for speed | Loses per-test process isolation, retry, slow-timeout profile, and process-level coverage granularity that nextest enables. Defeats the issue's primary ask. | Real-world coverage is meaningfully worse |
| Add a separate Codecov uploader now | Out of issue scope; threshold gating needs a baseline first (chicken-and-egg). | Scope creep, partial fix |
| Probe the cert chain with `openssl s_client` inside the workflow | Runner allowlist rejects `openssl` as a step first-token (`ci_guards.rs:11`). | Step failure, false sense of security |
| Set `SSL_CERT_FILE=/dev/null` to suppress EXP-102 in a hurry | Silent TLS bypass; defeats the entire point of the fix. | Future merge nightmare |
| Use `cargo install --git ...` instead of `--locked` crates.io pin | `--git` pulls the latest HEAD every install, defeating reproducibility and matching the `cargo install --locked --git ... v1.21.3` precedent poorly. | Non-deterministic installs, drift |
| Add coverage lane as a third job on `native-ci.yml` | The native runner is single-job; expanding to a matrix without confirming runner capacity is speculation. Open Item 4. | Possible queue contention |

### Simplicity Check

**What if this could be easy?** It is: every step is a plain `cargo` invocation; the install steps follow an established precedent; the cert guard is a one-liner. **Senior-engineer test:** passes — no new abstractions, no "just in case" features, no premature threshold gating.

**Nothing speculative:** every install step has a precedent (`zipsign` for native, `taiki-e/install-action` for GH); the cert path is the same one `cargo install` already implicitly trusts; no new test code, fixtures, or workflow jobs beyond the documented additive coverage lane.

---

## Architecture

### Component / Data Flow

```
[.gitea/workflows/native-ci.yml]         [.github/workflows/ci.yml]
   jobs.build.steps:                         jobs.build.steps:
     ┌─────────────────────────────────┐      ┌─────────────────────────────────┐
     │ env: SSL_CERT_FILE, SSL_CERT_DIR│      │ env: SSL_CERT_FILE, SSL_CERT_DIR│
     │  (inherited by all steps)       │      │  (inherited by all steps)       │
     └─────────────────────────────────┘      └─────────────────────────────────┘
                       │                                       │
   ┌───────────────────┴───────────────────┐   ┌───────────────┴───────────────────┐
   │ cargo install cargo-llvm-cov --locked │   │ taiki-e/install-action@cargo-llvm-cov│
   │ cargo install cargo-nextest  --locked │   │ taiki-e/install-action@nextest       │
   │ rustup component add llvm-tools-preview│  │ (adds llvm-tools-preview internally) │
   └───────────────────────────────────────┘   └───────────────────────────────────┘
                       │                                       │
            ┌──────────┴──────────┐                ┌──────────┴──────────┐
            │ cargo llvm-cov      │                │ cargo llvm-cov      │
            │   nextest           │                │   nextest           │
            │   --workspace       │                │   --workspace       │
            │   --all-targets     │                │   --lib             │
            │   --no-fail-fast    │                │   --no-fail-fast    │
            │   --lcov            │                │   --lcov            │
            │   --output-path     │                │   --output-path     │
            │     lcov.info       │                │     lcov.info       │
            └─────────┬───────────┘                └─────────┬───────────┘
                      │                                      │
                      ▼                                      ▼
            ┌─────────────────────┐                ┌─────────────────────┐
            │ actions/upload-     │                │ actions/upload-     │
            │   artifact lcov.info│                │   artifact lcov.info│
            └─────────────────────┘                └─────────────────────┘
```

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|---|---|---|
| Install coverage toolchain to `/usr/local/bin/` on native, not `~/.cargo/bin` | Mirrors `zipsign` precedent (`native-ci.yml:9`); runner `CARGO_HOME` does not always include `~/.cargo/bin`. | User-local install (invisible to runner) |
| Preset `SSL_CERT_FILE` at workflow `env:` level (inherited by all steps), not per-step | Coverage-instrumented subprocesses inherit env; per-step would be brittle and miss the `cargo install` re-fetch. | Per-step `env:` blocks |
| Add `cargo llvm-cov nextest` as a new step beside the existing `cargo test` lane, not in place of it | Test signal must stay visible even when coverage toolchain breaks. The issue's "switch" is forward-looking (no prior lane). | Replace `cargo test` with coverage |
| Use `test -f $SSL_CERT_FILE \|\| { echo "::error::..."; exit 1; }` (allowlist-safe) instead of shell `if`/`then` | Runner command policy rejects shell keywords as first token (`ci_guards.rs:11`). | `if [ -f ... ]; then ...; fi` |
| GH side uses `taiki-e/install-action` not `cargo install` | The `taiki-e` action is the in-monorepo idiom, adds `llvm-tools-preview` automatically, and survives runner image churn better than `cargo install`. | `cargo install` (slower, fragile) |
| Native lane keeps `--workspace --all-targets` (matches existing `cargo test`); GH lane keeps `--workspace --lib` | Native has Gitea registry creds; GH does not — lib-only avoids the `terraphim_server` integration tests that need `TERRAPHIM_SERVER_BIN`. | Symmetric `--all-targets` (breaks GH) |
| Emit `lcov.info` and upload as workflow artefact, but no Codecov upload | Issue scope is the lane and the cert env, not publishing. PR comments and threshold gating are explicit follow-ups. | Codecov upload (out of scope) |
| Pin `cargo-llvm-cov` and `cargo-nextest` with `--locked` to the crates.io latest, not `--git` | Matches the deterministic-install discipline of `native-ci.yml:45` (`cargo install --locked --git ... v1.21.3`); crates.io `--locked` is sufficient for tooling crates. | `--git` install (non-deterministic) |
| No threshold gating (`--fail-under-lines`) | First coverage lane in the monorepo; baseline does not exist. A threshold would fail CI from day one until the team hand-tunes it. | `--fail-under-lines 80` (CI red until tuned) |

---

## Expected Lifecycle Artefacts

| Artefact | Path | Required? |
|---|---|---|
| Research | `docs/plans/research-coverage-nextest.md` | Done (#313) |
| Design | `docs/plans/design-coverage-nextest.md` (this doc) | Yes |
| Verification (in-PR, committed at #327) | `docs/plans/verification-coverage-nextest.md` | Yes (committed alongside the code) |
| Verification (post-merge smoke evidence; the "Close gate") | `docs/verification/verification-report-coverage-nextest.md` | Yes (closes the gate after the lanes run on `main`) |
| Validation | `docs/plans/validation-coverage-nextest.md` | Yes (round-3 re-validation committed at #327; supersedes the round-1 verdict) |

---

## File Changes

### New Files
None. The change is entirely additive to two existing workflows and one doc file; no new source files, tests, or fixtures.

### Modified Files
| File | Changes |
|---|---|
| `.gitea/workflows/native-ci.yml` | (a) Add `env:` keys `SSL_CERT_FILE: /etc/ssl/certs/ca-certificates.crt` and `SSL_CERT_DIR: /etc/ssl/certs`. (b) Add a `Check host CA bundle` step directly after the existing `Check host tooling (zipsign)` step, with the same `test -f X \|\| { echo "::error::..."; exit 1; }` shape. (c) Add an install step for `cargo-llvm-cov` and `cargo-nextest` to `/usr/local` and `rustup component add llvm-tools-preview` before the coverage step. (d) Add a new coverage step that runs `cargo llvm-cov nextest --workspace --all-targets --no-fail-fast --lcov --output-path lcov.info`. (e) Add `actions/upload-artifact@v4` to publish `lcov.info`. (f) Refs comment `#313` block above the new steps. |
| `.github/workflows/ci.yml` | (a) Add a pinned `taiki-e/install-action@v2` step with `with: tool: cargo-llvm-cov@v<local-version>,nextest@v<local-version>` (the tool versions must match the locally-installed toolchain; `crates/terraphim_agent/tests/ci_guards.rs::coverage_tool_pinning_matches_local_toolchain` fires on drift). (b) Add `env:` keys `SSL_CERT_FILE` / `SSL_CERT_DIR` at the workflow level. (c) Add a `Check host CA bundle` step mirroring the native lane's `test -f $SSL_CERT_FILE || { echo ::error::...; exit 1; }` guard (the GH runner image ships `ca-certificates` by default but the guard is required by the Acceptance Criteria). (d) Add a new `Coverage (cargo llvm-cov nextest)` step beside the existing `cargo test --workspace --lib --no-fail-fast` step (which stays in place — see "Avoid At All Cost" rule and the round-1 P1 fix at `9adbbeaa`). (e) Add `actions/upload-artifact@v4` to publish `lcov.info` as `lcov-gh`. (f) Refs comment `#313` block above the new steps. |
| `BUILD.md` | Append a `## Coverage (optional)` section documenting the `cargo llvm-cov nextest` command for both runners, with a short note on `SSL_CERT_FILE` for the native runner. British English, no emoji. |

### Deleted Files
None.

### Diff Sketch (illustrative, not final)

```yaml
# .gitea/workflows/native-ci.yml (additive)
name: native-ci
on:
  push:
  workflow_dispatch:
jobs:
  build:
    runs-on: terraphim-native
    env:
      # #313: preset SSL cert env so instrumented subprocesses can reach
      # git.terraphim.cloud over HTTPS (EXP-102 Lead addendum 2).
      # Inherited by every step; see "Check host CA bundle" below.
      SSL_CERT_FILE: /etc/ssl/certs/ca-certificates.crt
      SSL_CERT_DIR: /etc/ssl/certs
    steps:
      - name: Check host tooling (zipsign)
        run: |
          test -x /usr/local/bin/zipsign && /usr/local/bin/zipsign --version || { echo "::error::zipsign not found on PATH. Install on the runner host: sudo install -m 0755 ~/.cargo/bin/zipsign /usr/local/bin/zipsign (see gitea-infrastructure HANDOVER.md, 'Host Tooling'). Refs #106"; exit 1; }
      # #313: guard the CA bundle path the workflow just exported.
      # The runner command policy rejects shell `if`/`then`/`fi` as the
      # literal first token; use `test` (the only conditional primitive
      # on the allowlist) with `||` chaining. Never SSL_CERT_FILE=/dev/null.
      - name: Check host CA bundle
        run: |
          test -f "$SSL_CERT_FILE" || { echo "::error::CA bundle not found at $SSL_CERT_FILE (EXP-102). Install ca-certificates on the runner host or set SSL_CERT_FILE to a real path. Refs #313"; exit 1; }
      # #313: install coverage toolchain to /usr/local (mirrors the zipsign
      # precedent above; ~/.cargo/bin is not always on the runner PATH).
      # --locked pins cargo-llvm-cov and cargo-nextest to the crates.io
      # latest matching the workspace's Cargo.lock hash.
      - name: Install coverage toolchain
        run: |
          cargo install cargo-llvm-cov --locked --root /usr/local && \
          cargo install cargo-nextest --locked --root /usr/local && \
          rustup component add llvm-tools-preview
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo build --workspace
      - run: cargo install --locked --git https://git.terraphim.cloud/terraphim/terraphim-ai --tag v1.21.3 --root /tmp/terraphim_server_install --config 'registries.terraphim.index="sparse+https://git.terraphim.cloud/api/packages/terraphim/cargo/"' --config 'registry.global-credential-providers=["cargo:token"]' --bin terraphim_server terraphim_server
      - run: TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test --workspace --all-targets --no-fail-fast
      - run: TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test -p terraphim_agent --test cross_mode_consistency_test -- --nocapture
      - run: TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test -p terraphim_agent --test integration_tests -- --nocapture
      - run: TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test -p terraphim_agent --test kg_ranking_integration_test -- --nocapture
      - run: cargo clippy -p terraphim_sessions --features enrichment -- -D warnings
      - run: cargo test -p terraphim_sessions --features enrichment --lib --no-fail-fast
      - run: cargo test -p terraphim_sessions --all-features --no-fail-fast
      - run: cargo test -p terraphim_agent --test packaged_install_graph_regression -- --nocapture
      - run: cargo test -p terraphim_agent --test ci_guards -- --nocapture
      # #313: first coverage lane in the monorepo. nextest runs each test
      # binary in its own process so llvm-cov can attribute per-test
      # coverage. --workspace --all-targets mirrors the existing cargo
      # test lane; --no-fail-fast matches it. GITEA_TOKEN is inherited
      # from the runner env so the [patch.crates-io] registry fetch works.
      - name: Coverage (cargo llvm-cov nextest)
        run: |
          TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server \
            cargo llvm-cov nextest --workspace --all-targets --no-fail-fast --lcov --output-path lcov.info
      - uses: actions/upload-artifact@v4
        with:
          name: lcov-native
          path: lcov.info
```

```yaml
# .github/workflows/ci.yml (additive)
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
  workflow_dispatch:

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1
  # #313: preset SSL cert env. ubuntu-latest's default bundle lives at
  # this path; presetting is harmless and uniform with native-ci.yml.
  SSL_CERT_FILE: /etc/ssl/certs/ca-certificates.crt
  SSL_CERT_DIR: /etc/ssl/certs

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      # #313: install cargo-llvm-cov and cargo-nextest via taiki-e's
      # install-action, pinned to the v2 action tag and specific tool
      # versions so a transitive regression on the crates we depend on
      # cannot silently flip the coverage lane (matches the
      # cargo install --locked discipline on the native lane). The pin
      # values must match the locally-installed toolchain on the dev box;
      # coverage_tool_pinning_matches_local_toolchain in
      # crates/terraphim_agent/tests/ci_guards.rs fires if they drift.
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-llvm-cov@v0.8.5,nextest@v0.9.144
      - uses: Swatinem/rust-cache@v2
      # #313: guard the CA bundle path the workflow just exported. Same
      # shape as the native lane (test -f ... || { echo ::error::...; exit 1; })
      # but with the ubuntu-latest default bundle path. cargo-llvm-cov
      # installs llvm-tools-preview via rustup on first run.
      - name: Check host CA bundle
        run: |
          test -f "$SSL_CERT_FILE" || { echo "::error::CA bundle not found at $SSL_CERT_FILE (EXP-102). Install ca-certificates on the runner host or set SSL_CERT_FILE to a real path. Refs #313"; exit 1; }
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo clippy -p terraphim_sessions --features enrichment -- -D warnings
      - run: cargo build --workspace
      # #313: the existing --workspace --lib cargo test lane stays in
      # place so the test signal is visible even if the coverage
      # toolchain breaks; the design's "Avoid At All Cost" rule above
      # explicitly forbids replacing it. The round-1 implementation
      # replaced it; commit 9adbbeaa restored it. Future readers: do
      # NOT collapse this step back into the coverage step.
      - run: cargo test --workspace --lib --no-fail-fast
      # #313: additive coverage lane. nextest runs each test binary in
      # its own process so llvm-cov can attribute per-test coverage;
      # --workspace --lib matches the test lane above; --no-fail-fast
      # matches the prior lane. GH has no Gitea registry creds so --lib
      # is the safe target set.
      - name: Coverage (cargo llvm-cov nextest)
        run: cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path lcov.info
      - uses: actions/upload-artifact@v4
        with:
          name: lcov-gh
          path: lcov.info
      - run: cargo test -p terraphim_sessions --features enrichment --lib --no-fail-fast
      - run: cargo test -p terraphim_grep --test default_feature_smoke
      - run: cargo test -p terraphim_agent --test packaged_install_graph_regression -- --nocapture
```

### BUILD.md Addendum

```markdown
## Coverage (optional)

The first coverage lane runs under nextest so per-test process isolation
is preserved. `SSL_CERT_FILE` must point at a real CA bundle path on the
runner host (see gitea-infrastructure HANDOVER.md, 'Host Tooling').

```bash
# terraphim-native (Gitea Actions)
cargo install cargo-llvm-cov --locked --root /usr/local
cargo install cargo-nextest --locked --root /usr/local
rustup component add llvm-tools-preview
TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server \
  cargo llvm-cov nextest --workspace --all-targets --no-fail-fast --lcov --output-path lcov.info

# ubuntu-latest (GitHub Actions)
cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path lcov.info
```
```

---

## API Design

No new public APIs. The change is CI-only and does not touch any Rust source.

### Workflow "API" (step signatures)

The two new step shapes introduced are:

```yaml
# native-ci.yml
- name: Check host CA bundle
  run: |
    test -f "$SSL_CERT_FILE" || { echo "::error::CA bundle not found at $SSL_CERT_FILE (EXP-102). Install ca-certificates on the runner host or set SSL_CERT_FILE to a real path. Refs #313"; exit 1; }

# Three separate install steps so a transient network failure on one
# install can be retried without rerunning the others. `cargo install
# --locked` is idempotent on warm caches.
- name: Install cargo-llvm-cov (pinned via Cargo.lock)
  run: cargo install cargo-llvm-cov --locked --root /usr/local
- name: Install cargo-nextest (pinned via Cargo.lock)
  run: cargo install cargo-nextest --locked --root /usr/local
- name: Add llvm-tools-preview component
  run: rustup component add llvm-tools-preview

- name: Coverage (cargo llvm-cov nextest)
  run: |
    TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server \
      cargo llvm-cov nextest --workspace --all-targets --no-fail-fast --lcov --output-path lcov.info

- uses: actions/upload-artifact@v4
  with:
    name: lcov-native
    path: lcov.info
```

```yaml
# ci.yml
# The pin values must match the locally-installed toolchain on the dev box;
# coverage_tool_pinning_matches_local_toolchain in
# crates/terraphim_agent/tests/ci_guards.rs fires on drift.
- uses: taiki-e/install-action@v2
  with:
    tool: cargo-llvm-cov@v0.8.5,nextest@v0.9.144

- name: Check host CA bundle
  run: |
    test -f "$SSL_CERT_FILE" || { echo "::error::CA bundle not found at $SSL_CERT_FILE (EXP-102). Install ca-certificates on the runner host or set SSL_CERT_FILE to a real path. Refs #313"; exit 1; }

# The existing --workspace --lib cargo test lane stays in place so the
# test signal is visible even if the coverage toolchain breaks. Do NOT
# collapse this step back into the coverage step (Avoid At All Cost
# rule above). The round-1 implementation collapsed it; commit 9adbbeaa
# restored it.
- run: cargo test --workspace --lib --no-fail-fast

- name: Coverage (cargo llvm-cov nextest)
  run: cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path lcov.info

- uses: actions/upload-artifact@v4
  with:
    name: lcov-gh
    path: lcov.info
```

### Environment Surface (added)

```yaml
# both workflows
env:
  SSL_CERT_FILE: /etc/ssl/certs/ca-certificates.crt
  SSL_CERT_DIR: /etc/ssl/certs
```

### Error Types

No new errors. Workflow failure surfaces as a non-zero step exit code plus an `::error::` annotation that GH/Gitea Actions render in the run UI.

---

## Test Strategy

### CI Lane Tests (no new Rust tests)

The coverage lane is itself the verification: `cargo llvm-cov nextest` runs the existing test suites under instrumentation. A green coverage run on `main` proves the lane works.

### Workflow Lint (manual / reviewer checklist)

| Check | How |
|---|---|
| No shell `if` / `then` / `fi` as literal first token on native | `terraphim-grep "^[\\s-]*run: \\|if " .gitea/workflows/native-ci.yml` returns only the `test -f` and `test -x` forms |
| No `SSL_CERT_FILE=/dev/null` anywhere | `terraphim-grep "/dev/null" .gitea/workflows/ .github/workflows/` — should match only fixture JSONLs |
| Both workflows preset `SSL_CERT_FILE` | `terraphim-grep "SSL_CERT_FILE:" .gitea/workflows/ .github/workflows/` — should return both |
| Both workflows install or use `cargo-llvm-cov` | `terraphim-grep "cargo-llvm-cov\|install-action@cargo-llvm-cov" .gitea/workflows/ .github/workflows/` |
| Both workflows invoke `cargo llvm-cov nextest` | `terraphim-grep "cargo llvm-cov nextest" .gitea/workflows/ .github/workflows/` |
| `BUILD.md` documents the coverage command | reviewer reads the new "Coverage (optional)" section |

### Workflow Smoke (post-merge)

Run both workflows on a throwaway branch after the change merges and verify:

1. Both jobs complete green.
2. `lcov.info` artefact is downloadable from the run UI.
3. `lcov.info` contains lines beginning with `SF:` for workspace crates (use `head -20 lcov.info`).
4. No `::error::` annotation appears in the run log.
5. No `cargo install` step times out (the `--locked` pin avoids needless reinstall churn).

### Unit / in-crate

None. No Rust code changes.

### Property / regression

None. No behaviour change to existing tests.

### Integration

None. No new integration tests; the existing `cargo test --workspace --all-targets` suite is reused under nextest.

---

## Implementation Steps

### PR-1 — Coverage lane + SSL cert env (issue #313)
**Files:** `.gitea/workflows/native-ci.yml`, `.github/workflows/ci.yml`, `BUILD.md`
**Tests:** workflow lint (above checklist); smoke run on a throwaway branch.
**Estimated:** 0.5–1 day.
**Rollback:** revert the merge commit. No data migrations, no flag flips, no registry changes.

### Sub-steps within PR-1 (commit-level)

1. **Commit 1: BUILD.md doc-only addendum.** Lowest-risk first; documents intent before code.
2. **Commit 2: `.gitea/workflows/native-ci.yml` additions.** New steps + `env:` block; reuses the `zipsign` shape.
3. **Commit 3: `.github/workflows/ci.yml` additions.** `taiki-e` install-actions + coverage step + `env:` block.
4. **Commit 4: smoke run + artefact capture.** Manual, not a code commit; record the artefact SHA on the PR.

Each commit is independently green if the workflow lints clean; the smoke run is the final gate.

### Rollback Plan

- **Single PR revert:** `git revert <merge-sha>` — restores both workflow files and `BUILD.md` to their pre-#313 state. No registry, lockfile, or Cargo.toml changes to unwind.
- **Partial rollback (if only one runner regresses):** revert just the failing workflow file; the other runner's lane stays green and is independently useful.
- **Toolchain-only rollback (if `cargo install cargo-llvm-cov` flapped):** remove the install + coverage steps but keep the `env:` block. The cert env is independently valuable for any future cargo subprocess that hits HTTPS (EXP-102 mitigation standalone).

### Verification (post-merge)

| Check | Method | Owner |
|---|---|---|
| Workflow lint passes (no shell keywords, no `/dev/null`, both envs preset) | reviewer reads diff + runs `terraphim-grep` smoke | PR reviewer |
| Native runner green | merge → watch run UI | Alex |
| GH runner green | merge → watch run UI | Alex |
| `lcov.info` artefact present on both runs | download from run UI, `head -20 lcov.info` shows `SF:` lines | Alex |
| No `::error::` annotations | grep run log | Alex |
| BUILD.md mentions coverage | reviewer reads | PR reviewer |

### Open Items

| Item | Status | Owner |
|---|---|---|
| Is there an existing coverage lane the issue title refers to that was missed? | Research grep across `.gitea/workflows/`, `.github/workflows/`, `BUILD.md`, `scripts/` returned zero hits. The title's "switch" is forward-looking. | Confirm with Alex before merge. |
| `terraphim-native` runner OS assumption (Debian vs RHEL family) | `/etc/ssl/certs/ca-certificates.crt` is the Debian/Ubuntu path; `/etc/pki/tls/certs/ca-bundle.crt` is RHEL. The HANDOVER referenced at `native-ci.yml:9` is authoritative. If the runner is RHEL, the path must change. | Alex — verify via HANDOVER before merge. If RHEL, edit the env value to the RHEL path. |
| Should the coverage step consume `--no-clean` to preserve `*.profraw` artefacts across re-runs for diff coverage? | Out of scope per the issue title; first lane runs single-shot. | Defer to follow-up issue if needed. |
| Should the native lane split into two jobs (one for tests, one for coverage) to run in parallel? | The native runner is single-job; splitting needs runner capacity confirmation. | Defer until a coverage-toolchain regression justifies the cost. |
| EXP-102 also has a "use http instead of https" workaround that should be overridden by this fix | Worth reading EXP-102 directly when accessible; not blocking #313. | Alex |
| Codecov upload + threshold gating | Explicit follow-up; out of scope. | New issue after first green coverage run establishes baseline |

### Acceptance Criteria

- Both `.gitea/workflows/native-ci.yml` and `.github/workflows/ci.yml` install or use `cargo-llvm-cov` and `cargo-nextest`, with `llvm-tools-preview` present on the GH side.
- Both workflows preset `SSL_CERT_FILE` and `SSL_CERT_DIR` at the workflow `env:` level, with a `test -f` guard that emits a `::error::` annotation if the path is missing.
- Both workflows invoke `cargo llvm-cov nextest ...` (not in-process `cargo llvm-cov ...`) for the coverage step.
- The native coverage step preserves `--workspace --all-targets`; the GH coverage step preserves `--workspace --lib`.
- Both workflows upload `lcov.info` via `actions/upload-artifact@v4`.
- The existing `cargo test ...` lanes remain in place and continue to pass.
- No shell `if` / `then` / `fi` as literal first token on the native runner.
- No `SSL_CERT_FILE=/dev/null` anywhere.
- `BUILD.md` documents the coverage command for both runners.
- Workflow comments use British English and contain no emoji.
- First coverage run on `main` produces a downloadable `lcov.info` artefact with workspace crate coverage lines.

---

## Approval

- [ ] Alex confirms the research open question (whether a hidden coverage lane was missed).
- [ ] Alex confirms `terraphim-native` runner CA-bundle path (Debian vs RHEL) via HANDOVER.
- [ ] Gate: workflow lint checklist above passes on the PR diff.
- [ ] Gate: both CI workflows green on the PR.
- [ ] Gate: `lcov.info` artefact downloadable from both runs.
- [ ] Gate: `docs/verification/verification-report-coverage-nextest.md` captures the post-merge smoke evidence (Close gate; written after the lanes run on `main`; the in-PR `docs/plans/verification-coverage-nextest.md` carries the pre-merge evidence).
