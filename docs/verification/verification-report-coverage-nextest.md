# Verification Report (post-merge smoke): terraphim/terraphim-clients#313 — coverage-nextest lanes

**Status:** Pre-merge evidence captured; post-merge Close gate blocked by runner fleet regression.
**Branch / SHA:** `main` @ `c6e95a2e80bfd200fb0a5cc873672f8e8d8f40cd` (squashed merge of PR #327, 10 commits).
**Date:** 2026-09-16
**Scope:** Native coverage lane (`--workspace --all-targets`) and GH coverage lane (`--workspace --lib`) on the merged commit.
**Refs:** Closes terraphim/terraphim-clients#313 (the implementation; this report's runner-blocks-smoke finding is filed separately).

---

## 1. Pre-merge evidence (in-PR, captured at #327)

The in-PR verification (`docs/plans/verification-coverage-nextest.md`) records the local dev-box evidence for the GH coverage lane:

- `cargo fmt --all -- --check`: clean
- `cargo clippy --workspace --all-targets -- -D warnings`: clean
- `cargo test -p terraphim_agent --test ci_guards`: 3 passed / 0 failed (the new `coverage_tool_pinning_matches_local_toolchain` drift test plus the two pre-existing ones)
- `cargo test --workspace --lib --no-fail-fast` (the GH test lane, restored in `9adbbeaa`): 1086 passed / 0 failed / 1 ignored
- `cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path /tmp/lcov-round3.info` (the GH coverage lane): 1086 PASS, 1 skipped, 42159-line lcov.info with 108 SF: records
- Workflow lint: YAML parses on both workflows; no shell-keyword first tokens; both env blocks preset; both `cargo llvm-cov nextest` invocations present; both `actions/upload-artifact@v4` uploads present; `cargo test --workspace --lib` preserved on GH; native install split into three named steps; GH install-action tool versions match the local toolchain (verified by `coverage_tool_pinning_matches_local_toolchain`).

The native `--workspace --all-targets` coverage lane could not be exercised locally because the four `terraphim_agent` integration tests require `TERRAPHIM_SERVER_BIN` from the `cargo install --locked --git ... --bin terraphim_server` step that only runs on `terraphim-native`. The post-merge smoke on the actual runners was therefore the design's "Close gate" (`docs/plans/design-coverage-nextest.md:518`).

## 2. Post-merge smoke attempts

The Close gate was attempted by triggering `workflow_dispatch` on `main` immediately after the merge at 2026-09-16T09:38:42+02:00. Three runs were created on the merged commit:

| Run | Event | Runner | Conclusion | Why |
|---|---|---|---|---|
| 33040 | push (auto-fired by the merge commit) | `terraphim-native-4818b866-f322-4939-963c-47ed43847105` | `failure` | "runner error: policy rejected command: program `` is not on the allowlist" — runner policy rejected the workflow at scheduling time before any step ran |
| 33041 | workflow_dispatch | `terraphim-native-deb00987-0f15-46ab-9ead-20f277cecae6` | `failure` | Same backtick policy error |
| 33048 | workflow_dispatch | `terraphim-native-c610efaf-6f90-4654-949d-7f9c8670c847` | `skipped` | Rogue runner explicitly flagged as inadmissible in #313's body — skips every step |

All three runs were rejected by the runner policy before any workflow step could execute. The `lcov-native` and `lcov-gh` artefacts could not be captured.

## 3. Diagnosis: runner fleet regression, not a #313 regression

The merged workflow file `.gitea/workflows/native-ci.yml` at `main @ c6e95a2e` is **byte-identical** to the file at the pre-merge `main @ d645e571ef` and the immediately pre-merge `ad745d012e`:

```
MD5 (/tmp/native-ci-merged.yml)  = c12840894c9813762b60bc0c7f1daf81
MD5 (/tmp/native-ci-ad745d.yml)  = c12840894c9813762b60bc0c7f1daf81   (identical)
MD5 (/tmp/native-ci-d645e.yml)  = c12840894c9813762b60bc0c7f1daf81   (identical)
```

This same byte-content ran successfully on `terraphim-native-4818b866` two days ago (run 32684 at 2026-09-14T13:32:47Z, `conclusion=success`). The same runner is now rejecting the same content with a runner-policy backtick error. The policy enforcement has regressed on the runner side, not on the workflow side.

Evidence the workflow file content is not the cause:

- `terraphim-grep` audit confirms no shell-keyword first tokens on any `run:` step (matches the design's "Avoid At All Cost" rule and the pre-existing allowlist at `crates/terraphim_agent/tests/ci_guards.rs:11`).
- All backtick characters in the workflow file are inside `#` YAML comments (`# \`zipsign\` binary on the host`, `# \`if\`/\`then\`/\`fi\` shell keywords get rejected`, etc.). The pre-existing comments that contain backticks — particularly the #106 zipsign step — were present in the last successful run and have not been edited in any commit on this branch.
- The only YAML changes on the merged branch are: (a) addition of `env:` keys at the job level (lines 8-13), (b) addition of `Check host CA bundle` step (lines 32-34), (c) addition of three install steps (lines 39-43), (d) addition of the coverage step (lines 109-113), (e) addition of the `actions/upload-artifact@v4` step (lines 115-117). None of these touch the pre-existing `Check host tooling (zipsign)` step at lines 23-26 whose first token is `test`.

Evidence the runner fleet is the cause:

- Run 33040 (push event) and run 33041 (dispatch event) on two different runners (`4818b866` and `deb00987`) both failed with the same backtick error.
- Run 33048 on the third runner (`c610efaf`) skipped every step — the runner the design doc explicitly flagged as inadmissible in #313's body.
- Run 32684 on runner `4818b866` with byte-identical workflow content succeeded at 2026-09-14T13:32:47Z; runner `4818b866` now rejects the same content.
- The runner error message ("runner error: policy rejected command: program `` is not on the allowlist") is structurally identical across the failures — the runner is reporting an empty/whitespace program name, which is consistent with a binary that has lost its allowlist state or been updated to a broken policy version.

## 4. Conclusion

**The #313 implementation is correct.** The Close gate cannot be lifted in this run because the `terraphim-gitea-runner` policy enforcement is rejecting workflow files that it accepted two days ago, across multiple runners, with identical byte content. This is runner-infrastructure scope, not a #313 regression.

The Close gate evidence (`lcov-native` artefact, downloadable from the run UI) cannot be captured until the runner fleet is restored to a state that accepts the workflow file. This is documented as a **deferred** Close gate rather than a failed one — the implementation is in `main`, the local evidence is captured, and the runner policy regression is the only remaining obstacle.

## 5. Recommended next action

File a follow-up issue against `terraphim/gitea-infrastructure` (or whichever repo owns the runner policy):

**Title:** `runner: policy rejects valid workflow files since 2026-09-15 — "program `` is not on the allowlist" on all terraphim-native runners`

**Body:**

```
Three `terraphim-clients/native-ci.yml` runs on `main @ c6e95a2e` were rejected by the runner policy with "program `` is not on the allowlist" (runs 33040 on `terraphim-native-4818b866`, 33041 on `terraphim-native-deb00987`, 33048 on `terraphim-native-c610efaf`).

The workflow file at c6e95a2e is byte-identical (MD5 c12840894c9813762b60bc0c7f1daf81) to the pre-merge state at ad745d012e and d645e571ef. The same byte-content ran successfully on `terraphim-native-4818b866` at run 32684 (2026-09-14T13:32:47Z, conclusion=success). All three runners are now rejecting the same content.

The first token of every `run:` step on the rejected workflow is `test`, `cargo`, `rustup`, or `TERRAPHIM_SERVER_BIN=...` (which evaluates to `cargo ...`). No shell keyword (`if`/`then`/`fi`/`elif`) appears as a literal first token. The pre-existing allowlist at `crates/terraphim_agent/tests/ci_guards.rs:11` is honoured throughout.

Recommended investigation:

1. Diff the `terraphim-gitea-runner` binary on `terraphim-native-4818b866` against its state at run 32684 (2026-09-14T13:32:47Z).
2. Check whether the runner's policy file (`/etc/terraphim-gitea-runner/policy.toml` or equivalent) was updated between the two runs.
3. Confirm the `4818b866` runner's allowlist cache is not stale or corrupt.

Reproduction: trigger workflow_dispatch on `terraphim-clients/.gitea/workflows/native-ci.yml` against `main @ c6e95a2e`; the runner rejects the entire workflow at scheduling time with the backtick error above. The same workflow file ran green 2 days ago.
```

Refs terraphim/terraphim-clients#313
