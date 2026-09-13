# Phase 11 soak designs (Exhaustive mode)

Run: 2026-09-11-terraphim-clients-1. Written by the lead, 2026-09-12. Project archetype: pure-safe-with-env-unsafe (six `unsafe` blocks, all `std::env::set_var`/`remove_var` in tests; zero `unsafe` in production workspace code). Confirmed findings at the time of writing: UF-001, UF-002 (test-binary env races), UF-039 and UF-040 (production-path, both inside the `fff-search` dependency's `unsafe`).

## Why these campaigns and not the skill's generic four

The skill's default menu is a 24-hour fuzz of previously-UB-bearing APIs, multi-day Miri over the whole suite under every MIRIFLAGS combination, 10^4 loom iterations against racy concurrency code, and Shuttle where loom times out. Three of those four do not fit this project and saying so is part of the design:

- **Loom and Shuttle are inapplicable.** Loom models `unsafe` concurrency primitives (atomics, `UnsafeCell`, hand-rolled locks). This workspace has none: its concurrency is `tokio` tasks, `rayon` `par_iter` and `std` channels, all safe-Rust. The two confirmed races are libc-level (`setenv` versus `getenv`), which loom cannot model because it does not model libc. No campaign.
- **Whole-suite multi-day Miri is low-yield but cheap enough to keep, in a bounded form.** Phase 3 already ran the four MIRIFLAGS axes over the workspace lib tests; EXP-101 showed the only report is inside `crossbeam-epoch` on the Stacked Borrows axis and is clean under Tree Borrows. A multi-day repeat would re-derive that. Campaign C keeps the *new* part only: the axes that were run on a reduced test set.
- **A generic 24-hour fuzz finds panics, not UB,** because there is no `unsafe` in workspace code for a malformed input to corrupt. Campaign D is therefore bounded and framed as a robustness campaign, with its UB value stated honestly as near zero.

The two campaigns that can still surface memory-unsafety (A and B) are the ones given the most budget.

## Preconditions for every campaign

1. **bigbox disk.** The root filesystem hit 100 percent on 2026-09-12 at about 10:38 BST and took the Gitea database down with it. Before any campaign: `df --output=avail -BG / | tail -1` must show at least 60 GB free. Every campaign uses a target directory under `/tmp/ubx-soak/<campaign>/target` and deletes it at the end; only sources, corpora and logs are kept, and logs are copied back into this workspace.
2. **#281.** The 62 orphaned `terraphim_server` processes must be killed by an operator before campaigns B and D, which bind ports and open the same sqlite stores.
3. **ASan admissibility.** Any campaign whose signal is an ASan report must prove the ASan runtime actually instrumented the C library under test (see the recipe in `phase6_idea_wizard_round_3.md`, section "Notes for Phase 5"). A run that did not instrument is inadmissible, not negative.
4. **Dispatch.** The skill's `rch` path is not used here: every earlier dynamic run in this audit used `ssh bigbox` plus `tmux`, and introducing a second dispatch mechanism at the soak stage would make the results harder to reproduce. Each campaign below gives its `tmux` session name.

---

## Campaign A: env-probe loop across all three env-writing test binaries

**Rationale.** This is the only campaign that can still produce a *new* memory-unsafe interleaving. The `LD_PRELOAD` interposition shim (`phase3_raw/envprobe/linux/envprobe_linux.c`) is the instrument that decided UF-001 and UF-002 when TSan proved blind (the Rust-shipped TSan runtime has no `getenv`/`setenv` interceptors on either platform). EXP-001b saw one unassisted `call`-phase overlap in 100 iterations; a soak at three orders of magnitude more iterations measures the true rate and may catch reader chains that a 100-iteration run misses.

**Host:** bigbox (glibc 2.39; Apple Libc serialises `getenv`/`setenv` via `environ_lock_np`, so macOS cannot produce the signal).

**Targets:** the three env-writing test binaries: `terraphim_grep --lib`, `terraphim_agent tui_service_tests`, `terraphim_update --lib`. Run each libtest binary **directly**, never through nextest: nextest gives one process per test, which destroys the cross-test race by construction (Lead addendum 2).

**Procedure.** 5,000 iterations per binary at `ENVPROBE_WIDEN_US=0`, then 500 at 500 and 500 at 2000 for the widened arms, at default libtest parallelism. Record per iteration: `setenv`/`unsetenv` count, `getenv`-on-another-thread count, `call`-phase overlaps with backtrace, `pre`-phase overlaps. Symbolise every overlap backtrace with `symbolise2.py`.

**Command.**
```
ssh bigbox 'tmux new-session -d -s soak-a "cd /tmp/ubx-soak/A && ./run_probe.sh 5000 0 && ./run_probe.sh 500 500 && ./run_probe.sh 500 2000"'
```

**Signal.** Any `call`-phase overlap at `widen=0` whose reader backtrace names a chain not already recorded in `phase3_dynamic_findings.md` (libgit2 `git_sysdir_guess_home_dirs`, sqlite `unixTempFileInit`) is a new finding: append an EXP block and loop back to Phase 8.

**Expected outcome.** Confirmation at a measured rate, no new chain. Budget about 6 hours.

**Post-remediation second pass.** After Phase 13 lands the injection commits, the same campaign must show `setenv=0` in the main process for all three binaries. That is the acceptance criterion of EXP-801, EXP-802 and EXP-803 and of Gitea issues #288, #291 and #300.

---

## Campaign B: MCP server SSE soak under a concurrent writer (EXP-302)

**Rationale.** UF-039's mechanism is already CONFIRMED_UB by EXP-104; what is unmeasured is blast radius in the long-lived server, where one client's grep over a tree another process truncates takes the process down for every client. Phase 6 round 3 recommended this as a soak rather than a round experiment for exactly that reason.

**Host:** bigbox (4 KiB `MMAP_THRESHOLD`, so far more files map than on the aarch64 audit host).

**Procedure.** Start the shipped `terraphim_mcp_server --sse` on a tree of several hundred files above the mapping threshold. Drive 8 concurrent clients calling `grep_files` and `multi_grep_files` in a loop for 4 hours. A sibling process runs the EXP-104 writer arm (same-length in-place rewrite) continuously and the truncate arm at randomised intervals, 1,000 truncations total. Record: server exit signal, per-request latency, every response whose content fails `std::str::from_utf8`, every JSON parse failure client-side, and the count of requests in flight at each server death.

**Command.**
```
ssh bigbox 'tmux new-session -d -s soak-b "cd /tmp/ubx-soak/B && ./run_sse_grep_soak.sh 14400 8 1000"'
```

**Signal.** Server death on SIGBUS with other clients in flight (blast radius confirmed); invalid-UTF-8 or unparseable MCP responses (data-integrity impact).

**Post-remediation second pass.** With the UF-039 zero-file `ContentCacheBudget` in place the same campaign must complete with zero server deaths and zero invalid responses. This is the strongest available proof of commit 0 and belongs on Gitea issue #294.

**Dependency.** Do not start before #281's orphans are killed.

---

## Campaign C: Miri completion pass over the test sets earlier axes skipped

**Rationale.** Not a fishing expedition: it closes a *known* gap in the evidence rather than repeating work. The Phase 3 Miri matrix does not cover the same tests on every axis. For `terraphim_session_analyzer`: default and strict-provenance ran 80 tests, tree-borrows ran 128, and symbolic-alignment ran 73 after the proptest modules were skipped for being impractically slow. The four cells are therefore not comparable, which is recorded in `phase4_unified_findings.md` but not resolved.

**Host:** the macOS audit host (Miri is platform-independent for this purpose and bigbox's disk is the scarcer resource).

**Procedure.** Run the proptest and rayon modules that each axis skipped, one axis at a time, with `-Zmiri-ignore-leaks` (rayon's global pool daemon threads otherwise make the process exit 1 even when every test passes) and a reduced `PROPTEST_CASES` (16) so the symbolic-alignment axis finishes. Sequential, never two Miri processes at once: each takes hours and the audit host has already been pushed into memory pressure by two concurrent Miri runs.

**Command.**
```
for axis in "" -Zmiri-tree-borrows -Zmiri-strict-provenance -Zmiri-symbolic-alignment; do
  PROPTEST_CASES=16 MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks $axis" \
  CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-soak-c cargo +nightly miri test \
    -p terraphim-session-analyzer --lib -- --test-threads=1 proptest rayon
done
```

**Signal.** Any UB report naming a workspace frame. A report naming only `crossbeam-epoch` repeats EXP-101 and is not new.

**Expected outcome.** Clean, and the Miri matrix in the final report becomes honest (every cell the same test set, or the difference stated per cell). Budget 8 to 12 hours unattended.

**Executed 2026-09-12 23:30 BST on bigbox** (not the macOS host this design names: the macOS host is 16 GB and two Tree Borrows axes had already failed on it; Miri is platform-independent for this purpose). Source: a detached git worktree at e90b193. Log: `phase11_artifacts/campaign_C.log`. Driver: `bash -c` with the exit status captured from `PIPESTATUS[0]`.

The skipped sets were derived rather than guessed: the tree-borrows run covered 128 tests and is a strict superset of the other three, so the deltas are 48 (default), 48 (strict-provenance) and 55 (symbolic-alignment). A native preflight confirmed the 48-name `--exact` filter selects exactly 48 of 128, so no cell is vacuous.

Run as one batch per axis, every axis aborts on its first test, because Miri aborts the process on the first report. Each delta test was therefore also run in its own Miri process. Result: 43 of 48 pass on default, 43 of 48 on strict-provenance, 50 of 55 on symbolic-alignment, so **every axis now covers the same 123 of 128 tests** (tree-borrows already covered 128).

The 5 tests that no Stacked-Borrows-based axis can cover are identical on all three axes: `analyzer::tests::test_analyze_drops_sessions_without_target_file_operations`, `parser::tests::test_extract_agent_invocations_ignores_non_task_tool_use`, `parser::tests::test_extract_file_operations`, `parser::tests::test_extract_file_operations_ignores_unknown_tool_and_missing_path` and `parser::tests::test_parse_task_invocation`. All five drive `SessionAnalyzer::analyze`, whose `par_iter` (`crates/terraphim-session-analyzer/src/analyzer.rs:70`) starts rayon's global pool; on a rayon worker thread crossbeam-epoch 0.9.21 then either violates Stacked Borrows at `internal.rs:567` (default and symbolic-alignment) or performs an integer-to-pointer cast at `atomic.rs:204` that strict-provenance refuses as an unsupported operation. Every erroring frame is a dependency frame; the workspace appears only in Miri's "called indirectly due to this code" note. That is a dependency limitation on the EXP-101 precedent, not CONFIRMED_UB, and it is why tree-borrows can cover all 128. `-Zmiri-ignore-leaks` was set throughout as designed; it was necessary but not sufficient, since these five abort on the crossbeam-epoch report rather than on a leak.

**Outcome: NO_EVIDENCE of workspace UB, and the matrix caveat is now precise** rather than "the cells are not comparable": all four axes cover the same 123 tests, and the 5 remaining tests are named with the reason each axis cannot run them.

---

## Campaign D: bounded fuzz of the parsers that take untrusted bytes

**Rationale and honest expectation.** There is no `unsafe` in workspace code, so this campaign cannot find memory-unsafety in the workspace; it finds panics, which are robustness defects, not UB. It is included because three separate panics have already been confirmed by hand (EXP-011, EXP-012, EXP-013, EXP-103, EXP-204) in the same family of byte-slicing parsers, which is strong evidence that the family is under-tested. Its UB value is only in the dependency layer: a crafted input that reaches `unsafe-libyaml`, `semver`'s inline storage, SQLite, LMDB or libgit2 could produce real UB, and those targets are run under ASan.

**Host:** bigbox, ASan where the target reaches a C library.

**Targets (four `cargo fuzz` targets, 6 hours each):**
1. The session JSONL parsers (`terraphim-session-analyzer` connectors: codex, opencode, claude), the source of the confirmed byte-slicing panics. Structured `Arbitrary` input: a JSONL document with controlled multi-byte characters at cut offsets.
2. `tools.toml` and thesaurus loading into the Aho-Corasick matcher (EXP-103's family), including empty and whitespace-only patterns.
3. YAML frontmatter into `serde_yaml` 0.9 and thence `unsafe-libyaml`, under ASan (the Miri arm of this is EXP-202; fuzzing covers inputs Miri is too slow to try).
4. Release-manifest JSON and version strings into `terraphim_update`, including the `semver` parse that happens before the zipsign gate (EXP-203's family), under ASan.

**Command.**
```
ssh bigbox 'tmux new-session -d -s soak-d "cd /tmp/ubx-soak/D && for t in sessions_jsonl tools_toml yaml_frontmatter update_manifest; do cargo +nightly fuzz run $t -- -max_total_time=21600; done"'
```

**Signal.** An ASan report is UB and loops back to Phase 8. A panic is a robustness finding: file it on the existing pattern (a Gitea issue on the EXP-103/#304 model), do not record it as UB.

**Note.** The workspace has no fuzz targets today; authoring these four is itself a deliverable and belongs in `UB_RUNBOOK.md` as a permanent gate.

---

## Campaign E (conditional): AVX2 differential under ASan (EXP-301)

Runs only if the round 7-9 bigbox pass did not already reach a verdict on EXP-301. Same design as the EXP-301 block: the `fff-search` case-insensitive memmem kernels against a scalar oracle, 10,000-plus seeded cases plus the enumerated boundary geometries, under ASan on x86_64, with the aarch64 NEON result as the divergence arm. Budget 1 hour; it is in the soak list only because it needs an x86_64 host, not because it is long.

---

## Summary table

| Campaign | Can find UB | Host | Budget | Blocked by |
|---|---|---|---|---|
| A env-probe loop x3 binaries | Yes (the only new-interleaving source) | bigbox | 6 h | disk |
| B MCP SSE under writer and truncator | Blast radius of a confirmed finding | bigbox | 4 h | disk, #281 |
| C Miri completion over skipped test sets | Yes (closes an evidence gap) | macOS | 8-12 h | nothing |
| D bounded fuzz of untrusted parsers | Only via C dependencies, under ASan | bigbox | 24 h | disk, fuzz targets do not exist yet |
| E AVX2 differential | Yes (untested SIMD `unsafe`) | bigbox (x86_64) | 1 h | disk; skip if EXP-301 already resolved |

**Exit criterion for Phase 11:** every campaign finished or timed out with a recorded outcome, and no CONFIRMED_UB finding without a Gitea remediation issue. At the time of writing all four confirmed findings have one (#280, #290, #293, #296).

## Correction (2026-09-13): capping the corpus does not bound Tree Borrows memory

Anything in this document or elsewhere in the audit suggesting that a Tree Borrows run can be made to fit by capping input size is **wrong**, and the evidence is EXP-203's four aborted attempts on bigbox: uncapped reached 110 GB, then caps of 8192, 256 and 32 bytes were killed at 48.2, 54.4 and 43.7 GB respectively. All four share one memory curve, about 13 GB at ten minutes and past 40 GB by 25 to 28 minutes, independent of the cap. Memory tracks interpreted steps and Miri's accumulating Tree Borrows state, not input length.

The correct remedies are to drop each value immediately rather than accumulating results, or to run one Miri process per input, which is what made campaign C tractable.

**Host-safety rule for any future Tree Borrows campaign on bigbox:** an automatic kill polling at 10 seconds or tighter. The uncapped run went from 33 GB to 110 GB between two 45-second polls and left the host with 4 GB available while it also hosts the Gitea PostgreSQL container; a 20-second poll still overshot to 48 and 54 GB.
