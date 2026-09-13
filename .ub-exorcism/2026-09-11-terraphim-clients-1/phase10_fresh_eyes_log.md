# Phase 10 fresh-eyes log

Run: 2026-09-11-terraphim-clients-1. Project `/Users/alex/projects/terraphim/terraphim-clients`, branch `task/254-ci-pipeline`, HEAD 46bbc43. Reviewer: the Phase 10 fresh-eyes agent, 2026-09-12, 16:28 to 18:05 BST.

**SOLO review.** Multi-model triangulation was unavailable in this session. Every judgement below is one model's, cross-checked against primary sources (the tree at HEAD, the fff-search 0.8.4 registry sources, the Gitea API) rather than against a second reviewer. Weight it accordingly: the source re-derivations are reproducible from the commands recorded here; the judgement calls about verdict tokens are not triangulated.

No project source file was edited. No commit was made. No issue was created or closed. Artefact edits and Gitea comments are listed in full below.

## The three prompts as applied

The skill's fresh-eyes prompts are written for a code change. Adapted to an audit:

- **Pass A (self-critical read).** Read every artefact this run produced with fresh eyes. Look for obvious errors, stale claims, contradictions between files, verdicts unsupported by their own evidence, file:line citations that no longer match the tree, and experiments whose Falsifiability text does not discriminate. Fix what is found, in the artefacts only.
- **Pass B (trace the real code).** Pick source files around the confirmed findings and trace them through what imports them and what they import. Verify independently that each CONFIRMED_UB call path is real at HEAD and that each remediation reaches every affected call site. Re-derive, do not trust, the three claims the brief named.
- **Pass C (review the other agents' reasoning).** Audit the reasoning, not the prose: rubric arithmetic, whether the winner beats the runner-up on the stated axis, DEFERRED verdicts that are disguised OPEN, NO_EVIDENCE closed on a subset or blind run, Gitea bodies against the plan they cite, and untestable acceptance criteria.

Two rounds were run. Round 1 applied all three passes and produced 28 of the 38 numbered rows below (25 substantive plus 3 checks that came back clean). Round 2 re-read the same artefacts plus the phase 6 rollup, the phase 9 log, terraphim-ai#3378 and #313, which round 1 had not reached, and produced 10 more (A15 to A19, C8, C9, C13 to C15); its Pass B spot checks were clean. Neither round was clean, so the loop terminated on the "all three passes twice" condition rather than on two consecutive clean passes.

---

## Pass A findings

Severity: **high** = would mislead Phase 13 into wrong action; **medium** = a reader would draw a wrong conclusion; **low** = cosmetic or bookkeeping.

| # | Severity | Artefact | Finding | Disposition |
|---|---|---|---|---|
| A1 | medium | phase4_unified_findings.md | The `Counts:` paragraph says "35 unified findings", "CONFIRMED_UB 2, NO_EVIDENCE 0, OPEN 11, DEFERRED 22" and "LIKELY-UB 2, SUSPICIOUS 9". The table holds 44 rows with 4 CONFIRMED_UB and 4 NO_EVIDENCE. Written when the table had 35 rows and never updated as addenda 1 and 3 to 6 appended UF-036 to UF-044. The one paragraph a reader would quote halves the audit's own confirmed-finding count. | Fixed in place with a recount and a note saying why the old figures were wrong |
| A2 | high | phase8_remediation_plan.md s11 | The EXP-804 pass criterion lists the prose `unsafe` lines expected on a clean tree as `wiki_sync.rs:804,809`, `memory_command.rs:1032`, `modes/local.rs:89,294`, `command_system_integration_tests.rs:100`. Re-run at HEAD the real set is four lines: `wiki_sync.rs:804`, `memory_command.rs:1032`, `modes/local.rs:89`, `modes/local.rs:294`. `wiki_sync.rs:809` is `sync_learning_rejects_unsafe_page_name`, where `unsafe` is between underscores and fails the `\b` anchors; `command_system_integration_tests.rs:100` has no `unsafe` token. A fifth, `build.rs:12`, matches today and vanishes with commit 6. **As written the criterion fails on a correct tree.** | Fixed in place; also posted to #310 |
| A3 | medium | phase4 | UF-039's severity cell still reads LIKELY-UB and its title cell still gives the mechanism as `from_utf8_unchecked` at grep.rs:1710, both superseded by lead addendum 4 in the same file. A reader of the row alone gets the retracted mechanism. | Fixed in place |
| A4 | medium | phase4 | UF-038's file:line is `crates/terraphim-session-analyzer/src/matcher.rs`. No such file exists; it is `src/patterns/matcher.rs`, which is what the plan's D2.3 cites. | Fixed in place |
| A5 | medium | phase4 | UF-044 status is DEFERRED "needs an x86_64 host with disk", but EXP-301 has since run on bigbox (51,215 cases, `total_divergences=0`, zero ASan reports). | Status corrected to NO_EVIDENCE |
| A6 | medium | registry, EXP-301 | The block records the completed run in one bullet while two earlier bullets still say the host is unavailable and the work is "runnable in Phase 11". | Stale bullets marked superseded |
| A7 | high | registry, EXP-003c | Verdict reads NO_EVIDENCE over a run that executed zero tests. The block's own Falsifiability line says "Not falsifiable as run ... neither supports nor refutes", and its own final note says "A vacuous pass is recorded as NEEDS_REFINEMENT, not NO_EVIDENCE". Phase 4's Miri matrix always said NEEDS_REFINEMENT. | Verdict corrected to NEEDS_REFINEMENT |
| A8 | medium | registry, EXP-003f | Verdict is NO_EVIDENCE while the Notes still say strict-provenance is "still running" and symbolic-alignment is "PENDING. No log exists yet", and that "the lead will patch this block to NO_EVIDENCE when both pending axes complete". Both completed. | Stale bullets rewritten with the real results and the subset caveat |
| A9 | medium | phase4 | The Miri matrix cells for session-analyzer repeat those stale PENDINGs. | Fixed in place with the differing-test-set caveat |
| A10 | medium | phase4 | The "Experiment registry summary" table stops at EXP-015 and still carries pre-Phase-5 tokens (EXP-010 to EXP-015 all OPEN, EXP-003f OPEN). Sixteen experiments filed later appear nowhere. | Table extended to EXP-806; the token disagreements recorded rather than silently harmonised |
| A11 | low | phase4 | "Top 5 by severity" predates UF-039 and UF-040 and omits both production-path CONFIRMED_UBs. | Correction added above the original list, which is kept |
| A12 | medium | registry, EXP-202 | Verdict OPEN, but a 146-line log records a completed default-axis run (`MIRI_EXIT=0`) over 43 crafted inputs with no UB report, plus a lead RERUN section. No result was ever written into the block. | Annotated; no verdict assigned, that is the executor's call |
| A13 | medium | registry, EXP-203 | Verdict OPEN with no reason recorded. The log shows 308 inputs and `panics=0` but ends the axis with `MIRI_EXIT=` empty, so the exit status was never captured and the run is inadmissible. OPEN is right for a reason the block does not state. | Annotated with the real reason |
| A14 | medium | phase5_experiment_results/summary.md | Stops at round 2. EXP-201, 202, 203, 204 and 301 all have results in the same directory and appear in no round table; the EXP-301 result exists only inside its registry block. | Staleness note appended, including that the `_raw` directories for EXP-201, 205, 206, 207, 301 and 302 are empty |
| A15 | low | phase6 rollup | Round 2's I-229 cites `EXP-211`, which does not exist in the registry and never has. | Marked as a dangling reference |
| A16 | low | phase6 rollup | The "Promotions across the cycle" bullet lists four round 1 promotions; the counts table and the round 1 report both say two (I-03 and I-08). The two figures count different things. | Both readings recorded |
| A17 | low | phase6 rollup | R3 bullet still calls EXP-301 DEFERRED. | Corrected |
| A18 | low | phase9_gitea_log.md | Prose says "all 22 edges"; the table lists 21, and the graph confirms 21 new plus the pre-existing `#280 depends_on #252`. | Corrected |
| A19 | low | plan s13 | Instructs keeping "the canonical `cargo test` line (:15)" of BUILD.md. The command is on line 14; line 15 is the closing fence. | Corrected |

## Pass B findings

Method: `git diff --stat 1120180..46bbc43 -- crates/` is **empty**, so every `crates/` file:line in the plan is valid at HEAD in one shot. Editions were checked because the whole lint lever rests on them: root `Cargo.toml:19` is `edition = "2024"` and all eleven members carry `edition.workspace = true`, with `rust-toolchain.toml` at 1.97.1. `terraphim_grep`'s `default = ["llm", "code-search"]` puts both the UF-001 writer and the fff-search path in the default build, so section 1's exposure table stands.

**The three re-derivations the brief asked for.**

1. **UF-039: the zero-file budget closes every `Mmap::map` path — confirmed, by a wider margin than claimed.** `memmap2::Mmap::map` occurs exactly once in fff-search 0.8.4, at `types.rs:640` inside `get_cached_content`. With `max_files: 0` the budget guard fires on the first file (`0 >= 0`) before `absolute_path` at :635 and the map at :640. Its only callers are `get_content_for_search` (:665) and `warmup_mmaps` (`file_picker.rs:1925`), and `warmup_mmaps` is **dead in 0.8.4**: its sole call site, `scan.rs:317`, is commented out upstream under "Skipped as potentially unsafe - figure this out later". Budget retention holds: `has_explicit_cache_budget` is set from `options.cache_budget.is_some()` (`file_picker.rs:713-720`) and `collect_files` calls `reset()` rather than replacing (`:817-821`). The plan's warning against `zero()` and `max_file_size: 0` is correct: with `max_file_size: 0` the slow path rejects every non-empty file at `types.rs:669-671`. `from_overrides` genuinely cannot express `max_files: 0` (`:883-899`).
   - **Gap (medium), B1:** the budget removes fff-search's mapping, not all mapping in the process. `FrecencyTracker::open` (`crates/terraphim_mcp_server/src/lib.rs:110`) still opens an LMDB environment that memory-maps its data file, which is EXP-207's surface, and libgit2 maps pack files during the git-status lookup. Nothing in the plan or in #293 says so, and "we no longer mmap" is the natural misreading. Recorded in Delta 4 and posted to #293.

2. **UF-040: the guard sits ahead of every `FilePicker::new` — confirmed, with two qualifications.** Exactly four constructor sites at HEAD (`hybrid_searcher.rs:354`; `mcp_server/src/lib.rs:1274`, `:1372`, `:1485`), no `new_with_shared_state` anywhere, two dependent crates. The three MCP function heads are at :1264, :1349 and :1455, matching the touch list. `terraphim_mcp_server` does not depend on `terraphim_grep`, so the private-copy decision is forced.
   - **Gap (medium), B2:** all four sites use `..FilePickerOptions::default()` and none sets `follow_symlinks`. The default is `false` (`file_picker.rs:460`), so the helper's "matching `follow_symlinks: false`" is true today but rests on an upstream default the workspace never asserts. The `ci_guards` arm should also assert that no site sets `follow_symlinks: true`, or a future site follows a symlink into a subtree the pre-walk never visited. Posted to #296.
   - **Gap (low), B3:** `collect_files` calls `absolute_path` at `file_picker.rs:831` (inside the guarded region, so covered), and the two other fff-search entry points, `FrecencyTracker::open` and `QueryParser::new(AiGrepConfig)`, sit outside the guard but build no picker and touch no path arena. Neither is a real gap; the touch list should say so rather than leave it inferred.

3. **UF-001 and UF-002 alone do NOT remove every writer — the plan as a whole does.** Six `unsafe` blocks at HEAD, re-counted and matching `phase1_unsafe_surface_inventory.md` section A: `openrouter_client.rs:170`, `:177`, `:185`, `:189`, `terraphim_update/src/state.rs:131`, `mcp_server/tests/test_mcp_stdio.rs:15`. Commits 1 and 2 close four of six. Left standing are `state.rs:131`, `test_mcp_stdio.rs:15`, eight `terraphim_test_utils::set_env_var` call sites (`mcp_autocomplete_e2e_test.rs:22`, `:28`; `mcp_rolegraph_validation_test.rs:21`, `:27`, `:137`, `:284`, `:394`, `:618`) and the `self_update` dependency writer. Commits 3, 4, 5 and 9 close those, all already in section 10.
   - **Finding (low), B4:** no gap in the plan, but the claim must be read as "the plan", not "the two injections", and commit 7 cannot land before commits 1 to 6 or the workspace will not compile. Posted to #310.

**Other citations verified correct at HEAD** (spot checks, round 1 and round 2): `service.rs:31/:49/:78`; `state.rs:15-22`, `:42-53`, SAFETY at :130, `unsafe` at :131-137, ten tests all `#[serial]`; `tui_service_tests.rs:6`, `:9`, `:37`, `:40`, `:43`; `update/lib.rs:1712` `test_check_for_updates_auto`, no `#[serial]`; `patterns/matcher.rs:143`, `patterns/loader.rs:79-81`; fff-search `simd_path.rs:170-186` with `from_utf8_unchecked` at :186 and `PATH_BUF_SIZE = 4096` at :63, `types.rs:271-275` `absolute_path`, `types.rs:153-164` `write_dir_name` with the panicking slice at :163, `to_string_lossy` at `file_picker.rs:232`, `:352`, `:406`; `ci.yml:57/:64/:79`; `native-ci.yml:73/:84/:94`; `.config/nextest.toml:27-32`; `deny.toml:79-81`. Phase 4's UF-001 reader cite `hybrid_searcher.rs:362-364` and the plan's `:342` are not in conflict: :342 is the `search_code` head, :362-364 is `collect_files`, which is the libgit2 reader.

## Pass C findings

| # | Severity | Finding | Disposition |
|---|---|---|---|
| C1 | high | **EXP-105 closed UF-037 NO_EVIDENCE on an instrument the audit's own definition excludes.** Phase 4 defines NO_EVIDENCE as an experiment "able to see the predicted signal". EXP-105's own note says the shim sees only PLT `getenv` and is blind to the glibc-internal resolver reads that are the predicted reader — exactly the property for which EXP-001 was given NEEDS_REFINEMENT. Two experiments, same epistemic position, different tokens. The row's conclusion survives: it rests on `setenv=0` on the default path, which the shim *can* see. Only the "0 overlaps" arm is instrument-limited. | Annotated on the block and in a new phase 4 addendum; recommended NEEDS_REFINEMENT for the overlap arm only. Also posted to #302 |
| C2 | high | **EXP-003c: NO_EVIDENCE over a vacuous run** (see A7). Same class as C1 and a flat self-contradiction within one block. | Verdict corrected |
| C3 | medium | **Four DEFERRED rows are OPEN experiments blocked on infrastructure.** Phase 4's DEFERRED means safe-by-construction, awaiting a lockfile bump, or routed elsewhere. EXP-010-a (helgrind absent), EXP-205, EXP-206, EXP-207 (ASan corpus plus bigbox disk) and EXP-302 (bigbox disk) meet none of those. This matters because the phase exit criteria treat DEFERRED as closed. EXP-801 to EXP-806 are a fifth category again — not runnable until Phase 13 lands the fix — honestly labelled, but the taxonomy has no token for them. | Recorded in the phase 4 addendum |
| C4 | medium | **Rubric arithmetic:** UF-036 candidate C in its fork form scores 4+4+2+3+1 = 14; the table printed 13. The winner is unchanged (A, 18, on blast radius). The other five rubrics (UF-001, UF-002, UF-004, UF-039, UF-040) were re-summed and are all correct. | Corrected in the plan and in section 15; posted to #302 |
| C5 | medium | **UF-039's winner does not beat its runner-up on the stated axis.** The plan says "decided by correctness margin", but A and C both score 4 there. The axis that separates them is availability, which the rubric expresses as blast radius (A 3, C 2). Winner unchanged; the label was wrong. | Corrected in D2.1; section 0's table should be read the same way |
| C6 | low | **UF-040 hedges the same way** ("correctness margin among the options the workspace controls today"), which is availability wearing correctness's name. A scores 17 on release and 0 today; B wins because A is not available. The hedge is at least explicit. | Recorded, not changed |
| C7 | medium | **The Miri matrix is four partial results, not one matrix.** Symbolic-alignment covered 73 tests, default and strict-provenance 80, tree-borrows 128; `connectors::cursor` is skipped on every axis. No axis covered the crate. Related: EXP-003e is NO_EVIDENCE on 34 terraphim_grep tests whose 23 filtered ones are, by its own note, exactly the UF-001 surface — so it must not be cited against UF-001. | Caveat written into the phase 4 matrix row and the addendum |
| C8 | medium | **Acceptance-criteria count mismatch.** #296 and #297 say six hostile geometries; EXP-201 summarises its run as "seven hostile geometries and one control" while naming six hostile trees (6x249, 6x250, 6x251, 15x255, 7x200, 14x100) and two controls (2x250, 3x255). The issues match the named list, so the block's summary line looks like the error, but EXP-806 could be run one geometry short if nobody checks. | Posted to #296 for the author to settle against the log |
| C9 | low | **One experiment, three tokens.** EXP-010 is DEFERRED in its verdict line, NEEDS_REFINEMENT in its own Phase 5 note, and OPEN in phase 4. EXP-001 and EXP-002 are DEFERRED in the registry and NEEDS_REFINEMENT in phase 4 and the plan. Nothing turns on either (all are superseded by later experiments), but a reader cannot tell which file is authoritative. | Recorded in the phase 4 registry-summary note |
| C10 | none (checked clean) | **Gitea bodies match the plan they cite.** No body offers `ContentCacheBudget::zero()` or `max_file_size: 0` as the UF-039 mitigation, none cites grep.rs:1710 as the workspace mechanism, and #302 does not offer `#[serial]` as the UF-036 fix — the three retractions that would have misled Phase 13. | No action |
| C11 | none (checked clean) | **Acceptance criteria are testable.** The one that looked internal, EXP-805's `cached_count = 0` in #294, is observable from outside fff-search: `FilePicker::cache_budget()` is public (`file_picker.rs:539`), `ContentCacheBudget` is re-exported via `pub use types::*`, and `cached_count` is a `pub AtomicUsize`. | Confirmed on #294 |
| C13 | none (checked clean) | **terraphim-ai#3378 matches the plan it implements.** Read directly: open, label `type/bug`, cites #252 as parent with UF-003 as the wrapper and UF-002 as its manifestation, carries the EXP-014 and EXP-002c evidence, proposes making the three functions `unsafe fn` with a real contract, and states it is not a blocker of the local fix because the workspace decouples and bans the crate. The only divergence from plan 4.6 is cosmetic: the proposed title's "or delete the crate" alternative is in the body but not the title. | No action |
| C14 | low | **#313 carries plan section 6 candidate B, which was rejected as a remediation.** Its "preset SSL_CERT_FILE/SSL_CERT_DIR in CI" change is the 14-of-20 runner-up that lost to `#[ignore]` because it hides the writer and never reaches `cargo test` on a developer host. The body already says "Not a remediation of any finding", which is the right framing, so nothing is wrong; the risk is only that a later reader treats landing #313 as closing UF-036. | Recorded in a comment on #313; no change requested |
| C15 | low (flag to the lead, not fixed) | **Correcting EXP-003c to NEEDS_REFINEMENT re-opens one item in the phase 7 convergence tracker.** `phase7_needs_refinement_round_1.txt` held one entry and rounds 2 to 5 were empty; if the tracker greps NEEDS_REFINEMENT in the registry, the next snapshot will show one again. That is the truthful state of a vacuous run, not a regression and not new work: `terraphim_command_runtime` has no `unsafe`, no raw pointers and no UF row, and the item closes by authoring a harness test, not by finding UB. Phase 11 and 12 should read it that way. | Flagged only |
| C12 | none (checked clean) | **Dependency edges are real and correctly directed.** The graph gives `{"from": 280, "to": 288, "type": "depends_on"}`, i.e. "280 is blocked by 288", matching the log's and CLAUDE.md's reading. All 21 phase-9 edges are present; no inversion anywhere. | Confirmed in the phase 9 log |

## Artefact edits made

All within the workspace. No project source, no commits.

- `phase4_unified_findings.md`: recounted the `Counts:` paragraph (A1); corrected UF-038's path (A4), UF-044's status (A5) and UF-039's severity cell and mechanism note (A3); rewrote the session-analyzer Miri matrix cells and the command_runtime cell (A9, C7); extended the experiment registry summary from EXP-015 to EXP-806 and corrected the EXP-003f, EXP-010 to EXP-015 tokens (A10); added a Phase 10 correction above "Top 5 by severity" (A11); appended a "Phase 10 fresh-eyes addendum" covering C1, C3, C2 and C7.
- `UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md`: EXP-003c verdict NO_EVIDENCE to NEEDS_REFINEMENT with the reason (A7, C2); EXP-003f stale PENDING notes rewritten (A8); EXP-301 pre-run gate bullets marked superseded (A6); EXP-105 annotated with the instrument caveat (C1); EXP-202 and EXP-203 annotated (A12, A13). No block's required fields were touched, so the linter still passes.
- `phase8_remediation_plan.md`: UF-036 candidate C total 13 to 14, and the same figure in section 15 (C4); UF-039 deciding-axis label (C5); section 11's EXP-804 prose-line list (A2); section 13's BUILD.md line number (A19); appended "Delta 4" with the three re-derivations, gaps B1 to B4, and two residual items for the author.
- `phase6_idea_wizard_rollup.md`: marked the `EXP-211` dangling reference (A15); recorded both readings of the round 1 promotion count (A16); corrected the R3 EXP-301 token (A17); appended a Phase 10 note warning that round 1's I-03 carries the superseded UF-039 mechanism.
- `phase5_experiment_results/summary.md`: appended a staleness note (A14).
- `phase9_gitea_log.md`: corrected "22 edges" (A18); appended the independent graph and issue-body verification (C10, C11, C12).

## Gitea comments posted

Repository terraphim/terraphim-clients. Nothing was created, closed, relabelled or reassigned.

| Issue | Content |
|---|---|
| #293 | UF-039 mechanism and budget re-derived from fff-search sources and confirmed, including that `warmup_mmaps` is dead in 0.8.4; asked #295 to record that LMDB and libgit2 still map in this process (B1) |
| #294 | Confirmed the EXP-805 acceptance criteria are externally testable, naming the public accessor path for `cached_count` (C11) |
| #296 | Confirmed the guard covers all four `FilePicker::new` sites; asked for a `follow_symlinks: true` assertion in the ci_guards arm (B2); flagged the six-versus-seven geometry count (C8) |
| #302 | UF-036 rubric arithmetic correction, winner unchanged (C4); the EXP-105 instrument caveat (C1) |
| #310 | Edition and toolchain verification of the lint lever; the six-block writer count and the sequencing constraint (B4); the corrected EXP-804 prose-line criterion (A2) |
| #313 | Recorded that its CI SSL preset is plan section 6 candidate B, rejected as a remediation, so landing #313 does not close UF-036 (C14); confirmed the four lane citations against HEAD |

## Gate results

Run after each round; identical results both times.

| Gate | Result |
|---|---|
| `cargo fmt --check` (project) | exit 0, no output |
| `lint-experiment-designs.py` on the registry | `[OK] — all blocks well-formed`, exit 0 |
| `verify-phase-artifacts.sh <workspace> 4` | 2 present, 0 missing |
| `verify-phase-artifacts.sh <workspace> 8` | 1 present, 0 missing |
| `verify-phase-artifacts.sh <workspace> 9` | 1 missing: the script expects `phase9_beads_log.md`. This run's tracker is Gitea and the artefact is `phase9_gitea_log.md`, which exists and is complete. Tracker-name mismatch in the script, not a missing artefact. No file was created to satisfy it. |

No workspace-wide build was run and no crate was compiled; the two Miri processes and the bigbox experiment agent were left undisturbed.

## Clean / not clean, per pass

- **Round 1, Pass A: not clean** (13 findings). **Round 2, Pass A: not clean** (6 further findings, all in the phase 6 rollup and the phase 9 log, which round 1 had not reached).
- **Round 1, Pass B: not clean** (4 findings: B1 to B4; all three re-derivations confirmed the plan's claims, the findings are gaps in what the plan states, not errors in what it concludes). **Round 2, Pass B: clean** (spot checks of `state.rs`, `tui_service_tests.rs` and `update/lib.rs` matched every cited line; ten tests, ten `#[serial]`, as claimed).
- **Round 1, Pass C: not clean** (C1 to C7 and C10 to C12, three of which are checks that came back clean). **Round 2, Pass C: not clean** (C8 the geometry count, C9 the three-token disagreement, C13 the upstream issue check which came back clean, C14 the #313 framing, and C15 the convergence-tracker flag).

The loop terminated on the "all three passes twice" condition. Two consecutive clean passes were not reached: round 2 was clean on Pass B only.

## Report

**Findings per pass:** Pass A 19, Pass B 4, Pass C 15 rows of which 11 are substantive and 4 are checks that came back clean. Total 38 numbered rows, 34 of them substantive, across two rounds (round 1 contributed 28, round 2 contributed 10). Three are high severity, not four: A7 and C2 are the same defect seen from two angles (EXP-003c), so the distinct highs are A2, A7/C2 and C1. Twenty-five were fixed or annotated in the artefacts directly.

**Most serious.** The class of NO_EVIDENCE verdicts recorded on instruments that could not see the predicted signal, and specifically EXP-105 closing UF-037's overlap arm (C1). Phase 4 defines NO_EVIDENCE as requiring an instrument "able to see the predicted signal"; EXP-105's own note says the shim is blind to the glibc-internal resolver reads that constitute the predicted reader, which is precisely why EXP-001 received NEEDS_REFINEMENT. UF-037 is the only LIKELY-UB row on a shipped production path, so a token that reads as "we looked and it is clean" rather than "we could not look" is the error with the furthest reach, and it survived three convergence rounds. EXP-003c (C2) is the same class and is a flat self-contradiction inside a single block. The most operationally damaging single error is different and easier to fix: the EXP-804 pass criterion in plan section 11 (A2), which would have failed on a correct tree and made the headline lever look broken.

**One flag for the lead.** Correcting EXP-003c to NEEDS_REFINEMENT (C15) will make the next phase 7 convergence snapshot show one needs-refinement item where rounds 2 to 5 showed none. That is the honest state of a run that executed zero tests, not a regression and not new UB work; it closes by authoring a harness test for a 70-line crate that has no `unsafe` and no raw pointers.

**Did the audit's conclusions survive?** Yes, substantially. All four CONFIRMED_UB rows survive independent re-derivation from primary sources: UF-001 and UF-002's writers and readers are where the audit says they are, UF-039's single `Mmap::map` site and its budget-based closure check out at source and are in fact better supported than claimed (`warmup_mmaps` is dead code upstream), and UF-040's four constructor sites and the truncating `read_to_buf` are exactly as described. All six rubric winners stand; the one arithmetic error and the one mislabelled deciding axis change no outcome. The remediation plan reaches every affected call site, provided it is executed as a whole and in order. What did not survive is a layer of bookkeeping: the counts, the status tokens, the registry summary and several "pending" notes had drifted behind the evidence, in every case understating rather than overstating what the audit found. The two verdict-token errors (C1, C2) are real epistemic defects but neither changes a row's conclusion.
