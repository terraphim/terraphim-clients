# PR #84 Validation Report

**PR**: terraphim/terraphim-agents#91 (parent audit) → terraphim/terraphim-clients#84 (this repo)
**Title**: `ci: run all workspace targets in test gate`
**Branch**: `fix/91-all-targets-test-gate`
**Validated against**: `terraphim/terraphim-agents#91` ("audit: --lib-only test gate regression across Terraphim repos (2026-07-31 family)")
**Validator**: PR merge campaign agent
**Date**: 2026-08-31

---

## 1. Acceptance Criteria (from terraphim-agents#91 + PR #84 body)

| AC | Source | Result |
|----|--------|--------|
| AC-1: replace `cargo test --workspace --lib --no-fail-fast` with `cargo test --workspace --all-targets --no-fail-fast` in the active native runner gate | PR body | MET |
| AC-2: leave `conf.d/terraphim-clients.toml` line 60 unchanged (the disabled build-runner `--lib` is out of scope per "1 PR per repo") | PR body | MET (no diff to that file) |
| AC-3: workflow yaml must remain valid and the final `cargo test` step must contain `--all-targets` and not `--lib` | PR body, yaml.safe_load | MET |
| AC-4: no source code touched; only `.gitea/workflows/native-ci.yml` modified | `git diff --stat` | MET (1 file, 5 lines) |
| AC-5: after merge, `cargo test --workspace --all-targets --no-fail-fast` must succeed on the live runner | terraphim-agents#91 + PR body claim | **NOT MET** — CI run 29222 fails with 57 unique test failures (job 61703 exit 101) |
| AC-6: "does not, on spot-reads, destabilise the pipeline" | PR body claim | **NOT MET** — see verification §4 |

AC-1 through AC-4 are mechanical and green. AC-5 and AC-6 are empirical and red.

---

## 2. Performance Review

Workflow file change is one line of build semantics; no performance regression risk in CI execution itself. The `--all-targets` switch does add compilation cost (integration test binaries) but that is the intended scope of the change.

Local baseline (main @ `572ae18`, macOS workstation):
- `cargo test --workspace --all-targets --no-fail-fast` → 14m 22s (862s), 41 failures
- This includes feature gates (`--features enrichment` not exercised here; that flag is added separately on line 18)

CI baseline (PR #84 branch @ `076c1d5`, Linux runner):
- `cargo test --workspace --all-targets --no-fail-fast` exit 101, 57 failures observed (timing truncated in the captured log)

No new performance hotspot identified.

---

## 3. Security Review

- No code change, no new dependencies, no new permissions.
- Workflow file only flips a flag on an existing step.
- No surface area for security regression.

---

## 4. Defect Register (with ownership and follow-up)

| ID | Defect | Owner | Follow-up |
|----|--------|-------|-----------|
| D-PR84-01 | PR body claim about pipeline stability is empirically false on this runner | PR #84 author | Update PR body or amend the change to include the necessary test gating |
| D-PR84-02 | `is_ci_environment()` does not recognise the Gitea runner; CI-skip logic in `replace_feature_tests.rs` and `server_mode_tests.rs` does not fire on `terraphim-gitea-runner` | `terraphim_agent/tests/*` authors | Either add `env: CI: true` to the workflow test step OR extend the helper to probe `~/.local/share/terraphim-gitea-runner` or `GITEA_ACTIONS=true` |
| D-PR84-03 | `terraphim_mcp_server/tests/*` tests assume `TERRAPHIM_MCP_SERVER_BIN` is set; the workflow does not produce or export the binary before the test step | `terraphim_mcp_server` test authors | Either (a) add a `cargo build -p terraphim_mcp_server` step that exports the binary path; or (b) add `#[ignore]` with a stated reason; or (c) auto-build and resolve via `env!("CARGO_BIN_EXE_terraphim_mcp_server")` (the precedent set by PR #121 / #135 for `terraphim_server`) |
| D-PR84-04 | `replace_feature_tests.rs` looks for `<workspace_root>/docs/src/kg` but the fixture lives at `<workspace_root>/crates/terraphim_agent/docs/src/kg` | `terraphim_agent/tests/replace_feature_tests.rs` author | Update the path; the file already does `manifest_path.parent().and_then(\|p\| p.parent())` — change `workspace_root.join("docs/src/kg")` to `manifest_path.join("docs/src/kg")` |
| D-PR84-05 | "Other infra-bound suites gate on missing services via presence checks" — claim is overstated; at least 30 tests panic or error without checking | PR #84 author | Update PR body |

---

## 5. Stakeholder Sign-off

Sign-off requires:

1. **Author acknowledgement** that AC-5 / AC-6 do not hold in the current state and either of the three merge paths in verification §6 is acceptable.
2. **Maintainer decision** on path (1) vs (2) vs (3) from the verification report. Path (1) (merge PR #135 first) is the lowest-risk option because it is verified-mergeable on its own branch.
3. **Issue tracking** for the remaining follow-up defects (D-PR84-02 through D-PR84-05). At minimum D-PR84-04 (wrong KG path) and D-PR84-03 (missing MCP server binary) should be filed as Gitea issues before PR #84 merges, because both are reproducible on `main` once `--all-targets` lands and would otherwise block every subsequent PR.

---

## 6. Validation Verdict

**CONDITIONAL — DO NOT MERGE AS-IS.**

The change is mechanically correct and minimal. It is blocked by pre-existing test failures that the change exposes (rather than introduces). Merging without addressing those failures would regress `main` from green to red on `native-ci / build (push)`.

The right sequence is:

1. Land PR #135 (gates 5 server-binary-dependent tests with `#[ignore]`).
2. File follow-up issues for D-PR84-02, D-PR84-03, D-PR84-04.
3. Either (a) extend PR #84 with `env: CI: true` + minimal `#[ignore]` for tests that lack CI-skip logic, or (b) defer PR #84 until D-PR84-02/03/04 are resolved in separate PRs.
4. Re-run CI on PR #84. If green, merge with a release note acknowledging that `--all-targets` was previously hidden behind `--lib`.

Only then close terraphim/terraphim-clients#108 with the campaign summary.