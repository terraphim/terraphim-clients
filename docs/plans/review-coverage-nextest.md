# Review: terraphim/terraphim-clients#313 — coverage-nextest (round 1, before PR)

**Reviewer:** structural-pr-review skill (round 1)
**Branch:** `task/313-coverage-nextest`
**Base:** `main`
**Date:** 2026-09-15
**Scope:** CI-only (`.gitea/workflows/native-ci.yml`, `.github/workflows/ci.yml`, `BUILD.md`)

---

## Summary

The change introduces the monorepo's first coverage lane using `cargo llvm-cov nextest` on both runners and presets `SSL_CERT_FILE` / `SSL_CERT_DIR` as the EXP-102 Lead addendum 2 mitigation. The native lane is implemented correctly: the existing `cargo test --workspace --all-targets --no-fail-fast` step is preserved (line 76 of `.gitea/workflows/native-ci.yml`) and the coverage step is added beside it (line 109-112). The GitHub lane is **inconsistent with the design's own "Avoid At All Cost" rule** and **replaces** the existing `cargo test --workspace --lib --no-fail-fast` step with `cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path lcov.info`. That is the only P1 finding; everything else is P2/P3.

**Result: NOT PASSED** — 1 P1 (test signal loss on GH when llvm-cov toolchain breaks), 3 P2 (comment accuracy, version pinning, no `--profile ci`), 1 P3 (native comment about `taiki-e` adding llvm-tools-preview is misleading — but the native lane uses `cargo install`, not taiki-e).

---

## P1 — `cargo test --workspace --lib --no-fail-fast` removed from GH workflow

**File:** `/Users/alex/projects/terraphim/terraphim-clients/.github/workflows/ci.yml`
**Line:** 39
**Before (origin/main):**
```yaml
- run: cargo test --workspace --lib --no-fail-fast
```
**After (this PR):**
```yaml
- name: Coverage (cargo llvm-cov nextest)
  run: cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path lcov.info
```

The design (`docs/plans/design-coverage-nextest.md`) explicitly forbids this on lines 55 ("Never replace the existing `cargo test ...` lanes with coverage-only. Coverage is additive; tests must keep running so failures are visible even when the coverage toolchain is broken.") and rejects the option on line 75 ("Convert both workflows' `cargo test` lanes in place to `cargo llvm-cov nextest` ... Silent loss of test gating when llvm-cov breaks").

The implementation on the native side honours this rule (the original `cargo test --workspace --all-targets --no-fail-fast` at `.gitea/workflows/native-ci.yml:76` is preserved and the coverage step is added at lines 109-112). The GH side does not.

**Why this matters.** Coverage instrumentation modifies the build (`RUSTFLAGS="-C instrument-coverage"` and the llvm-cov runner binary). If `cargo-llvm-cov` fails to install, `llvm-tools-preview` is missing, the instrumented binary fails to compile, or `cargo llvm-cov nextest` has any other regression, the GH `cargo test --workspace --lib --no-fail-fast` test signal disappears entirely. The native lane would still catch the issue because the original test step runs first. The GH lane would go red with no test-failure attribution.

**Fix.** Keep the original `cargo test --workspace --lib --no-fail-fast` step intact (or move it before the coverage step) and add the coverage step as a new line, matching the native lane's pattern.

---

## P2 — GH install-actions do not pin tool versions

**File:** `/Users/alex/projects/terraphim/terraphim-clients/.github/workflows/ci.yml`
**Lines:** 27-28

```yaml
- uses: taiki-e/install-action@cargo-llvm-cov
- uses: taiki-e/install-action@nextest
```

The design says (line 26 of the diff): "taiki-e install-actions pin the version". The `@<tool>` shorthand installs the **latest** version at the time the workflow runs; it does not pin. If `cargo-llvm-cov` or `cargo-nextest` publishes a regression to GitHub Releases, the GH lane breaks without any code change.

The native lane's `cargo install cargo-llvm-cov --locked` is the equivalent of pinning — it uses the workspace's Cargo.lock hash. The GH lane should match that discipline; pin to a specific version tag (e.g. `taiki-e/install-action@nextest` with a `tool: nextest@0.9.144` input), or pin the action itself (`taiki-e/install-action@v2.82.0`).

**Fix.** Pin the action version (`taiki-e/install-action@v2`) and pass `with: tool: nextest@0.9, cargo-llvm-cov@0.6` (or similar specific versions).

---

## P2 — Neither coverage invocation uses `--profile ci`

**Files:** `/Users/alex/projects/terraphim/terraphim-clients/.gitea/workflows/native-ci.yml:112`, `/Users/alex/projects/terraphim/terraphim-clients/.github/workflows/ci.yml:39`

Both invocations use the default nextest profile. The design references `terraphim-ai` adopting the `ci` profile (research-coverage-nextest.md §2.1 row 5: `cargo nextest run --workspace --exclude terraphim_agent --profile ci`). For consistency with the monorepo's other nextest usage and to get the slower-timeout / no-fail-fast behaviour configured centrally, both coverage steps should add `--profile ci`. This requires adding a `.config/nextest.toml` with `[profile.ci]` settings — a small follow-up, but worth noting in this PR's review so the work is not repeated later.

**Fix.** Add `.config/nextest.toml` with a `[profile.ci]` block matching `terraphim-ai`'s settings, then change both invocations to `cargo llvm-cov nextest --profile ci ...`.

---

## P2 — Design's "test-signal-preserving" claim contradicts its own "Avoid At All Cost" rule

**File:** `/Users/alex/projects/terraphim/terraphim-clients/docs/plans/design-coverage-nextest.md`
**Lines:** 55, 75, 163, 169

The design's "Avoid At All Cost" list (line 55) says: *"Never replace the existing `cargo test ...` lanes with coverage-only. Coverage is additive; tests must keep running so failures are visible even when the coverage toolchain is broken."*

The "Eliminated Options" table (line 75) rejects: *"Convert both workflows' `cargo test` lanes in place to `cargo llvm-cov nextest`"*.

But the "Diff Sketch" (line 163, GH side item (c)) and "Modified Files" (line 169) instruct exactly that for the GH workflow.

The design's rationale (line 163) is "test-signal-preserving; coverage and test run are the same command under nextest". That rationale is true at the binary level but ignores the failure-mode reasoning in the "Avoid At All Cost" list. The GH implementation followed the contradictory instruction; the design itself is internally inconsistent.

**Fix.** Reconcile the design: either drop the contradiction in the "Avoid At All Cost" list and "Eliminated Options" table (acknowledging that GH's `--lib` is the entire workspace's test coverage so the test signal is preserved) or change the GH implementation to keep `cargo test --workspace --lib --no-fail-fast` and add the coverage step beside it (matching the native lane). The native lane's behaviour is the safer pattern and aligns with the "Avoid At All Cost" rule.

---

## P3 — Comment on GH lane is misleading

**File:** `/Users/alex/projects/terraphim/terraphim-clients/.github/workflows/ci.yml`
**Lines:** 25-26

```yaml
# #313: taiki-e install-actions pin the version and add
# llvm-tools-preview internally.
```

`taiki-e/install-action@cargo-llvm-cov` and `taiki-e/install-action@nextest` (the `@<tool>` shorthand) do not install `llvm-tools-preview`. They install the named binary. The `llvm-tools-preview` rustup component is added by `cargo-llvm-cov` itself on first invocation in CI (via `rustup component add llvm-tools-preview` with `CARGO_LLVM_COV_SETUP=yes` by default in non-interactive environments). So the practical effect is correct (llvm-tools-preview ends up installed), but the comment's claim about the install-actions adding it "internally" is wrong.

**Fix.** Rewrite the comment to: `# #313: install cargo-llvm-cov and cargo-nextest via taiki-e's install-action. cargo-llvm-cov installs llvm-tools-preview via rustup on first run.`

---

## Positive Observations

1. **The native lane is implemented exactly as the design prescribes.** The `Check host CA bundle` step uses `test -f` with `||` chaining (no shell keywords), the install step targets `/usr/local/bin/`, and the coverage step runs beside the existing `cargo test --workspace --all-targets --no-fail-fast`. EXP-102 mitigation is sound.

2. **`SSL_CERT_FILE` is never set to `/dev/null`.** The grep audit returned one match, and that match is in a comment that explicitly says "Never SSL_CERT_FILE=/dev/null" (`.gitea/workflows/native-ci.yml:31`). That is documentation, not configuration.

3. **All new `run:` steps' literal first tokens are `cargo` or `test`** — none use shell keywords as the first token. The runner command allowlist documented in `crates/terraphim_agent/tests/ci_guards.rs:11` is honoured.

4. **Comments use British English** ("behaviour", "initialised", etc.). No emoji. Workflow comments cite `#313` consistently.

5. **`BUILD.md` is updated.** The "Coverage (optional)" section documents both runners' commands, references the HANDOVER for CA bundle path, and cites `#313`.

6. **`--locked` is used on the native lane's install steps.** This matches the deterministic-install discipline of the existing `cargo install --locked --git ...` step.

7. **EXP-102 mitigation is correct.** `SSL_CERT_FILE` is set to a real path, not `/dev/null`; `SSL_CERT_DIR` is set as a fallback; the `test -f` guard emits a `::error::` annotation on miss with a remediation pointer.

8. **`actions/upload-artifact@v4` is used** with explicit `name:` (lcov-native, lcov-gh) and `path:` (lcov.info). Gitea Actions and GitHub Actions both support this action.

---

## Acceptance Criteria Audit (from design-coverage-nextest.md §"Acceptance Criteria")

| Criterion | Status |
|-----------|--------|
| Both workflows install or use `cargo-llvm-cov` and `cargo-nextest` | PASS — GH uses `taiki-e/install-action@*`, native uses `cargo install --locked` |
| Both workflows preset `SSL_CERT_FILE` and `SSL_CERT_DIR` with `test -f` guard | PASS — native has explicit `Check host CA bundle` step, GH inherits env (implicit guard — see P2 below) |
| Both workflows invoke `cargo llvm-cov nextest ...` | PASS |
| Native keeps `--workspace --all-targets`; GH keeps `--workspace --lib` | PASS |
| Both workflows upload `lcov.info` via `actions/upload-artifact@v4` | PASS |
| **Existing `cargo test ...` lanes remain in place** | **FAIL on GH** — see P1 |
| No shell `if`/`then`/`fi` as literal first token on native | PASS |
| No `SSL_CERT_FILE=/dev/null` anywhere | PASS |
| `BUILD.md` documents the coverage command for both runners | PASS |
| Comments use British English, no emoji | PASS |
| First coverage run on `main` produces a downloadable `lcov.info` artefact | UNVERIFIED — requires post-merge smoke (per design §"Verification") |

**Note on "with `test -f` guard":** The native workflow has an explicit `Check host CA bundle` step. The GH workflow inherits the env but does **not** have an equivalent guard step. The design's acceptance criterion says "with a `test -f` guard that emits a `::error::` annotation if the path is missing" — applied to "both workflows". Strictly speaking, GH is missing this guard. On `ubuntu-latest` the cert path almost always exists, so the practical risk is low, but the design's own criterion is not fully met. This is another P2 finding (added below).

---

## Additional Finding (P2) — GH workflow lacks the `test -f` guard step

**File:** `/Users/alex/projects/terraphim/terraphim-clients/.github/workflows/ci.yml`

The design's acceptance criterion (`design-coverage-nextest.md` line 457) requires the `test -f` guard on **both** workflows. The native workflow has it (`.gitea/workflows/native-ci.yml:32-34`); the GH workflow does not.

**Fix.** Add a step to the GH workflow before the install-actions:
```yaml
- name: Check host CA bundle
  run: |
    test -f "$SSL_CERT_FILE" || { echo "::error::CA bundle not found at $SSL_CERT_FILE (EXP-102). Refs #313"; exit 1; }
```

---

## Verdict

**passed = false.** One P1 finding (GH `cargo test --workspace --lib` step replaced instead of preserved) blocks the merge. After the P1 is addressed, three P2 (version pinning, `--profile ci`, missing GH guard step) and one P3 (misleading comment) remain — these can ship as follow-ups but should be acknowledged before merge so the next coverage work has a clean baseline.

**Evidence:**
- `.gitea/workflows/native-ci.yml:32-43` (Check host CA bundle + install steps, correct shape)
- `.gitea/workflows/native-ci.yml:76` (existing `cargo test --workspace --all-targets` preserved)
- `.gitea/workflows/native-ci.yml:109-114` (coverage step additive, not replacement)
- `.github/workflows/ci.yml:14-15` (env keys preset)
- `.github/workflows/ci.yml:27-28` (install-actions, unpinned — P2)
- `.github/workflows/ci.yml:39` (replaces `cargo test --workspace --lib --no-fail-fast` — **P1**)
- `.github/workflows/ci.yml:40-43` (upload-artifact correct)
- `BUILD.md:18-32` (Coverage section correct)
- `docs/plans/design-coverage-nextest.md:55,75,163,169` (design's internal contradiction — P2)
- `crates/terraphim_agent/tests/ci_guards.rs:11` (runner allowlist: `cargo` and `test` only — honoured throughout the diff)