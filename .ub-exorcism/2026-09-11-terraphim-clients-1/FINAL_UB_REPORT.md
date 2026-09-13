# Final undefined-behaviour report

**Run** 2026-09-11-terraphim-clients-1, Exhaustive mode.
**Project** `/Users/alex/projects/terraphim/terraphim-clients`, a Rust workspace of 11 crates and six CLI binaries, branch `task/254-ci-pipeline`, HEAD e90b193 (the audit probed 1120180, ae06cd2 and 46bbc43; `git diff --stat 1120180..e90b193 -- crates/` is empty and e90b193 touches only `deny.toml`, so every source citation below is valid at HEAD).
**Audit workspace** `/Users/alex/projects/terraphim/terraphim-clients/.ub-exorcism/2026-09-11-terraphim-clients-1`. Every relative path in this document is relative to that directory unless it starts with `crates/`, in which case it is relative to the project root.
**Tracker** Gitea at `https://git.terraphim.cloud`, repository `terraphim/terraphim-clients`, parent issue #252. Remediation issues are #280 and #288 to #313, the audit follow-ups #317 to #320, and `terraphim/terraphim-ai#3378`. Read them with `gtr view-issue --owner terraphim --repo terraphim-clients --index N`; list ready work with `gtr ready --owner terraphim --repo terraphim-clients`.
**Written** 2026-09-12 by the Phase 12 final-artifact author and closed out on 2026-09-13, from the Phase 0 to Phase 11 artefacts, revised the same evening to take in the bigbox pass 2 results (EXP-205, EXP-206, EXP-207, EXP-302), lead addenda 7 to 11, and Deltas 4 and 5 of the remediation plan, and closed out on 2026-09-13 with the bigbox Miri pass, addendum 13 and convergence. **The audit CONVERGED after round 11** (two consecutive quiet rounds against the floor of 10). **Every experiment now carries a verdict: the registry holds no OPEN and no NEEDS_REFINEMENT entries.** Where the Phase 10 fresh-eyes review corrected an earlier artefact, the corrected reading is the one given here. Where a later addendum retracted a claim, the retraction is the one given here and is stated in section 4 rather than quietly applied.

---

## 1. Executive summary

A workspace that contains no `unsafe` in any production source file still carries two confirmed undefined-behaviour findings on shipped production paths, and both of them live inside one dependency. `fff-search` 0.8.4, which backs `terraphim-grep`'s code search and the three MCP grep tools, memory-maps every file it searches while the workspace disables the watcher its own safety argument depends on (UF-039), and it stores paths through a lossy conversion that inflates non-UTF-8 filenames past its 4096-byte path buffer, so a truncating copy returns an invalid `str` through a safe API (UF-040). Two further findings are confirmed inside test binaries, both from environment mutation racing an unsynchronised native `getenv` on glibc: `terraphim_grep`'s OpenRouter tests against libgit2 (UF-001) and `terraphim_agent`'s TUI service tests against bundled SQLite (UF-002). The archetype recorded at Phase 0 was `pure-safe-with-env-unsafe`, which predicted exactly one class of problem: six `unsafe` blocks, all `std::env::set_var` or `remove_var`, all in tests, and nothing else. That prediction held for workspace-authored code, where twenty of the twenty-five taxonomy buckets closed with zero hits, but it was blind in one direction the audit had to discover: the archetype says nothing about `unsafe` that arrives through dependencies, and that is where both production-path findings came from.

Three results dominate what a reader should take away.

- **The shipped server dies.** EXP-302 ran the released `terraphim_mcp_server --sse` on bigbox with eight concurrent SSE clients calling the `terraphim_grep` MCP tool while a sibling process truncated the mapped tree. The server was killed by SIGBUS, harness exit 135, after hundreds of thousands of successful responses. That is UF-039's blast radius in the production binary: one truncation of one mapped file ends the process for every connected client, and no client did anything wrong. It is the same mechanism EXP-104 confirmed in a harness, so it adds no new finding row and did not reopen convergence.
- **The fix can be undone by a routine dependency update without any visible change to the fix.** `fff-search` 0.8.4 has exactly one `memmap2::Mmap::map` site and it sits behind the file-count budget, which is what makes the planned zero-file `ContentCacheBudget` a complete remedy. Version 0.10.6 adds a second mapping path that the budget cannot reach, and it is entered precisely when a zero-file budget refuses the first. After a `cargo update` the mitigation would still be present in the source, still read correctly in review, and do nothing for any file at or above 256 KiB on Linux. The remediation therefore pins `fff-search` to `=0.8.4` and asserts the pin in `ci_guards`. This is the most dangerous silent regression the audit found.
- **One of the audit's own clearance claims was false and is retracted.** The libgit2-sys bump did not clear RUSTSEC-2026-0183 and RUSTSEC-2026-0184. Section 4 states this in full, because the way the error was made matters more than the advisory does.

The audit counts two different objects, and they never reconcile. **Findings are UF rows; verdicts are EXP blocks.** There are 44 of the first and 38 of the second, one experiment can carry several findings and one finding several experiments, and the convergence JSONs count experiment verdicts only. A reader who tries to make 44 and 38 agree is comparing a findings register with an experiment register.

Findings, by status against HEAD:

| Finding rows (44 total) | Value |
|---|---|
| CONFIRMED_UB | 4 rows: UF-001, UF-002, UF-039, UF-040 |
| CONFIRMED_UB on a production path | 2: UF-039 and UF-040, both inside `fff-search` |
| CONFIRMED_UB in test binaries only | 2: UF-001 and UF-002 |
| NO_EVIDENCE | 7 |
| OPEN | 12 |
| DEFERRED | 19 |
| Closed against HEAD, formerly DEFERRED | 2: UF-016 on the lockfile bump; UF-017 on the bump for three of its four advisories and on unreachability plus the `=0.8.4` pin for the git2 pair |
| Severity: LIKELY-UB / SUSPICIOUS / CONTRACTUAL-BUT-DEFENSIBLE / NOT-UB | 6 / 12 / 24 / 2 |

Experiments, as the final (round 11) convergence tracker records them:

| Experiment verdicts (38 blocks) | Value |
|---|---|
| CONFIRMED_UB | 5 |
| NO_EVIDENCE | 23 |
| OPEN | 0 |
| NEEDS_REFINEMENT | 0 |
| DEFERRED | 10 |

The ten DEFERRED blocks are not unanswered questions. Six are the post-remediation proofs EXP-801 to EXP-806, which cannot run until Phase 13 lands the fixes; the rest are rows routed elsewhere or safe by construction. Nothing in the registry is waiting on an instrument or a decision.

**Five confirmed verdicts sit on four finding rows.** They are EXP-001b (UF-001), EXP-002c (UF-002), EXP-104 (UF-039), EXP-201 (UF-040) and EXP-302 (UF-039 again, the blast radius in the shipped server). UF-039 carries two of the five. That is the whole of the arithmetic.

| Other counts | Value |
|---|---|
| `unsafe` blocks in workspace source | 6, all environment setters, all in tests |
| Taxonomy buckets closed with zero surface | 20 of 25 |
| Gitea issues filed | #280, #288 to #313 and the follow-ups #317 to #320 in terraphim-clients, plus 1 upstream in terraphim-ai |

One of the five NO_EVIDENCE verdicts, UF-037, was recorded on an instrument that could not observe the signal it was looking for. Section 4 lists it and the wider class of results it belongs to, because a clean result from a blind instrument is not evidence of absence and the audit's own definition of NO_EVIDENCE excludes it.

---

## 2. Findings

Severity order follows the Phase 10 correction: the two production-path confirmations lead, then the two test-binary confirmations, then the remaining LIKELY-UB rows, then SUSPICIOUS, CONTRACTUAL-BUT-DEFENSIBLE and NOT-UB. The "Phase 10" column marks rows whose text the fresh-eyes review corrected; the correction is described in the note beneath the table.

### 2.1 Confirmed and LIKELY-UB

| UF | Title | Severity | Status | EXP ids | Gitea | Phase 10 |
|---|---|---|---|---|---|---|
| UF-039 | `fff-search` maps every 16 KiB to 10 MiB file during `terraphim-grep search_code` and the MCP grep tools with the watcher disabled; a concurrent in-place writer yields invalid-UTF-8 `String`s, a concurrent truncation kills the process with SIGBUS. **Confirmed in the shipped `terraphim_mcp_server --sse` binary, which one truncation kills for every connected client** | LIKELY-UB | **CONFIRMED_UB** | EXP-104 (harness); EXP-302 (blast radius, shipped SSE server); EXP-805 (post-fix, deferred) | #293, #294, #295 | corrected |
| UF-040 | Hostile non-UTF-8 entry names under a user-supplied search path inflate `fff-search`'s lossy path storage past `PATH_BUF_SIZE`; `read_to_buf` truncates mid-`U+FFFD` and returns an invalid `str` | LIKELY-UB | **CONFIRMED_UB** | EXP-201; EXP-806 (post-fix, deferred) | #296, #297, #298 | |
| UF-001 | `terraphim_grep` lib tests mutate `OPENROUTER_BASE_URL` while libgit2 `getenv` runs on sibling test threads | LIKELY-UB | **CONFIRMED_UB** | EXP-001 (NEEDS_REFINEMENT), EXP-001b; EXP-801 | #280, #288, #289 | corrected |
| UF-002 | `EnvVarGuard` in `tui_service_tests` mutates HOME, XDG_CONFIG_HOME and TERRAPHIM_DATA_PATH while SQLite's `unixTempFileInit` may `getenv` on a sibling thread | LIKELY-UB | **CONFIRMED_UB** (mechanism) | EXP-002 (NEEDS_REFINEMENT), EXP-002b (NO_EVIDENCE, ordering-contingent), EXP-002c; EXP-802 | #290, #291, #292 | corrected |
| UF-004 | `terraphim_update` `setup_temp_config_dir` sets five variables under a false SAFETY comment in a 130-test binary; the glibc resolver reader was never ruled out | LIKELY-UB | OPEN | EXP-010; EXP-803 | #299, #300, #301 | |
| UF-037 | `self_update` `set_ssl_vars!()` reaches the production update-check path on Linux through the GitHub backend | LIKELY-UB (by contract) | NO_EVIDENCE, overlap arm instrument-blind | EXP-105, EXP-102 | #302 | annotated |

### 2.2 SUSPICIOUS

| UF | Title | Status | EXP ids | Gitea |
|---|---|---|---|---|
| UF-003 | `terraphim_test_utils` wraps `unsafe set_var` in a safe `pub fn` whose SAFETY rule (`#[serial]`) cannot be satisfied in a mixed binary | OPEN | EXP-014 | terraphim-ai#3378 |
| UF-005 | crossbeam-epoch Stacked Borrows report in `Local::element_of` on rayon workers | NO_EVIDENCE | EXP-101; EXP-004 (superseded) | none |
| UF-006 | Rollback truncates the running executable in place via `fs::copy` over `current_exe()` | DEFERRED | none | none (routed, non-UB) |
| UF-007 | Nested `Runtime::new` plus `block_on` inside async `run_server_command` panics deterministically | DEFERRED | none | none (routed, non-UB) |
| UF-008 | `shared_learning::store` `block_on` bridge panics on current-thread runtimes and with no runtime | OPEN | EXP-013 | none |
| UF-009 | Enricher byte-window context extraction slices mid-character | OPEN | EXP-012 | none (routed, non-UB) |
| UF-010 | `Session::summary` cuts the first user message at byte 100 | OPEN | EXP-012 | none (routed, non-UB) |
| UF-011 | Fixed-byte display truncations on input-controlled strings | OPEN | EXP-012 | none (routed, non-UB) |
| UF-036 | `self_update` `set_ssl_vars!()` is a second unannotated env writer in the `terraphim_update` lib test binary | OPEN | EXP-102, EXP-010 note | #302, #303, plus #300 count arm |
| UF-041 | Hostile YAML frontmatter reaches `unsafe-libyaml` through serde_yaml 0.9 | NO_EVIDENCE (Stacked Borrows over the full corpus; Tree Borrows partial) | EXP-202 | #317, on hygiene grounds |
| UF-042 | Attacker-influenced manifest version reaches semver's inline-storage `unsafe` before the zipsign gate | NO_EVIDENCE for UB; the parse-before-verify ordering stands as a defence-in-depth defect | EXP-203 | #318 |
| UF-044 (Phase 10: status corrected) | `fff-search` case-insensitive memmem AVX2 kernels, untested above 26-byte needles and never sanitised | NO_EVIDENCE | EXP-301 | none |

### 2.3 CONTRACTUAL-BUT-DEFENSIBLE

| UF | Title | Status | EXP ids | Gitea |
|---|---|---|---|---|
| UF-012 | macOS-only native `getenv` reader via SystemConfiguration overlaps the grep writer; Apple Libc locks both sides | DEFERRED | EXP-001 probe | none |
| UF-013 | `test_mcp_stdio` sets env vars with no SAFETY comment; sound only as a one-test binary | DEFERRED | none | covered by #310 sweep |
| UF-014 | `terraphim_test_utils::set_env_var` sites in the two serialised mcp_server binaries | DEFERRED | none | covered by #310 sweep |
| UF-015 | Seven unserialised mcp_server binaries spawn the real server and may share `~/.terraphim/sqlite/terraphim.db` | OPEN | EXP-015 | none |
| UF-016 | event-listener 5.4.1 unsound `Send`/`Sync` (RUSTSEC-2026-0221) via sqlx and opendal | **CLOSED against HEAD** (event-listener 5.4.2) | none | #254 class |
| UF-017 | Other UB-class advisories: anyhow, lru, memmap2, git2 | **CLOSED against HEAD, but on two different arguments.** anyhow, lru and memmap2 are cleared by the bumps. The git2 pair RUSTSEC-2026-0183 and RUSTSEC-2026-0184 is **not** cleared and is still in the lockfile; it closes on the unreachability of `Remote::list` and `Blame::blame_buffer` plus the `=0.8.4` pin. See section 4 | none | #254 class; #257 stays open |
| UF-018 | Global `KG_CACHE` std `RwLock` held across filesystem I/O; poison is fail-open | DEFERRED | none | none |
| UF-019 | Tokio lock guards held across awaits into persistence, search and an LLM HTTP call | DEFERRED | none | none |
| UF-020 | `CommandRegistry` readers acquire two locks in opposite orders | DEFERRED | none | none |
| UF-021 | `CommandRegistry` multi-lock updates in separate scopes leave torn state on panic | DEFERRED | none | none |
| UF-022 | `maybe_auto_import` check-then-act allows duplicate imports | DEFERRED | none | none |
| UF-023 | `DEVICE_STORAGE` singleton binds the SQLite data dir to whichever HOME is live at first init | DEFERRED | none | none |
| UF-024 | Sibling openrouter tests observe `OPENROUTER_BASE_URL` mid-mutation | DEFERRED | none | closed by #280 commit 1 |
| UF-025 | KG-boost comparator total only while no `relevance_score` is NaN | OPEN | EXP-011 | none |
| UF-026 | `store_with_dedup` best-match uses `partial_cmp(..).unwrap()` with no NaN filter | OPEN | EXP-011 | none |
| UF-027 | Markdown frontmatter accepts `importance_total: NaN` and feeds a sort | OPEN | EXP-011 | none |
| UF-028 | Paragraph finder mixes char indices with byte offsets | OPEN | EXP-012 | none |
| UF-029 | Rolegraph document rank depends on AHashMap insertion order | DEFERRED | none | terraphim-ai#3376 |
| UF-030 | Windows promotion window leaves no binary if killed between `remove_file` and `rename` | DEFERRED | none | none |
| UF-031 | `notify` FSEvents `extern "C"` callback hosts a workspace closure | DEFERRED | none | none |
| UF-032 | Update scheduler builds and tears down a runtime per tick inside `spawn_blocking` | DEFERRED | none | none |
| UF-033 | Two long-lived multi-thread runtimes coexist on the main thread | DEFERRED | none | none |
| UF-034 | Detached tasks with dropped `JoinHandle`s in the LSP debounce and sessions watcher | DEFERRED | none | none |
| UF-035 | `std::process::exit` from inside a running runtime, 45 sites | DEFERRED | none | none |

### 2.4 NOT-UB (robustness, confirmed panics)

| UF | Title | Status | EXP ids | Gitea |
|---|---|---|---|---|
| UF-038 (Phase 10: path corrected) | An empty pattern in `tools.toml` yields zero-width Aho-Corasick matches at non-char boundaries and panics the session analyser | NO_EVIDENCE for UB, panic confirmed | EXP-103 | #304, #305, #306 |
| UF-043 | `parse_correction_pattern` applies byte offsets from a lowercased and trimmed copy to the original string | NO_EVIDENCE for UB, panic confirmed | EXP-204 | #307, #308, #309 |

### 2.5 What the Phase 10 review corrected

Five rows and one paragraph carried text the review found wrong at source, and the corrected reading is what appears above.

- **UF-039's mechanism.** The finding was filed against `from_utf8_unchecked` at `fff-search` `grep.rs:1710`. That line is reached only by `GrepMode::Fuzzy`, which no workspace call site uses. The workspace path returns invalid `String`s because `from_utf8_lossy(..).into_owned()` validates and copies in two separate reads over a live mapping, and the root fault is the `memmap2::Mmap::map` aliasing contract, not the conversion. A fix that only swaps the conversion closes nothing.
- **UF-001 and UF-002's exposure.** The audit first wrote that both CI test lanes were exposed. They are not: both run `cargo nextest`, one process per test, which destroys the cross-test race by construction. What is exposed is the two `cargo llvm-cov --workspace --lib` coverage lanes, which run libtest in-process on glibc with `--ignore-run-fail`, and the canonical `cargo test` line in BUILD.md that every developer runs.
- **UF-038's file path.** Cited as `src/matcher.rs`, which does not exist. The file is `crates/terraphim-session-analyzer/src/patterns/matcher.rs`.
- **UF-044's status.** Recorded DEFERRED for want of an x86_64 host after EXP-301 had already run on bigbox over 51,215 cases with zero divergences and zero sanitiser reports. Corrected to NO_EVIDENCE.
- **UF-037's verdict token.** Kept as NO_EVIDENCE, but the overlap arm rests on an instrument that cannot see the predicted reader. See section 4.
- **The counts paragraph.** Phase 4's own summary said 35 findings with 2 confirmed, written when the table held 35 rows and never updated as eight more were appended. The table holds 44 rows with 4 confirmed. The one paragraph a reader would quote halved the audit's confirmed-finding count.

Two DEFERRED rows should also be read as resolved rather than pending, and one of them closes for a reason the audit first got wrong. UF-016 and UF-017 were deferred "on the lockfile bump", and both bumps of 2026-09-12 have landed: HEAD's `Cargo.lock` carries event-listener 5.4.2, crossbeam-epoch 0.9.21, anyhow 1.0.104, lru 0.18.4, memmap2 0.9.11 and libgit2-sys 0.18.8+1.9.7. UF-016 closes outright on that. UF-017 closes on three of its four advisories for the same reason, **but not on the fourth**: the libgit2-sys bump does not clear RUSTSEC-2026-0183 or RUSTSEC-2026-0184, which are advisories against the git2 crate and are patched only in git2 0.21.0 or later, and HEAD still resolves git2 0.20.4. That pair closes on a reachability argument instead, set out in section 4 and in Delta 5 of the remediation plan. Similarly, EXP-004 was deferred pending a post-bump Stacked Borrows re-run, and EXP-101 is that run: the report persists inside crossbeam-epoch 0.9.21 with identical code, Tree Borrows is clean, and no workspace frame appears. Treat EXP-004 as closed by EXP-101. UF-005 therefore rests on EXP-101 alone and keeps its NO_EVIDENCE status; it carries no remediation issue because there is nothing in workspace code to remediate, and the cross-reference it used to carry pointed at #264, an unrelated test failure.

---

## 3. Method and instruments

Twenty of the twenty-five taxonomy buckets were closed in Phase 2 with recorded zero-hit sweeps: aliasing, provenance, alignment, validity, uninitialised memory, type punning, `Pin`, FFI, refcount intrinsics, const-to-mut writes, lifetime escapes, volatile, inline assembly, target features, the allocator API, FFI callbacks, `repr(packed)`, invalid reference casts, coherence, and manual `Hash`/`Eq` implementations. Five buckets were live: data races, `Send`/`Sync`, panic safety, library-trait invariants, and async drop with runtime nesting. Dynamic work then ran on four instruments, and what each could and could not see is the load-bearing part of every verdict below.

**ThreadSanitizer could not see the finding the audit most wanted it to see.** The predicted access pair is a `setenv` writer against a native `getenv` reader, and both sides execute inside uninstrumented libc. TSan can only observe them through runtime interceptors. The Rust-shipped TSan runtime has none for the environment functions on either platform: the macOS runtime has 475 interceptors and zero matching `getenv`, `setenv`, `unsetenv`, `putenv` or `clearenv`, and both Linux runtimes shipped with the same nightly have 1,745 interceptors each and again zero. Instrumenting the bundled C with `CFLAGS=-fsanitize=thread` would not help, because it instruments the call to `getenv`, not the reads of `environ` inside libc, and it failed to link anyway (the `sqlx-macros` proc macro is a host dylib and picked up instrumentation with no host runtime). EXP-001 and EXP-002 were therefore given NEEDS_REFINEMENT rather than NO_EVIDENCE, which is the correct treatment and the model the rest of the audit should have followed.

**The platforms differ, and the difference decides severity.** Apple Libc serialises `getenv`, `setenv` and `unsetenv` behind `environ_lock_np()`, verified against the `apple-oss-distributions/Libc` sources. glibc's `setenv` takes an internal lock that `getenv` does not take, and it may `realloc` the `__environ` array. So the same code is a contract violation on macOS and a C11 data race on Linux. Both CI runners are glibc: `ubuntu-latest` for the GitHub workflow, and bigbox (Ubuntu 24.04, glibc 2.39) for the Gitea native workflow.

**The LD_PRELOAD interposition shim was the decisive instrument.** `phase3_raw/envprobe/linux/envprobe_linux.c` is an 87-line shim that wraps `setenv`, `unsetenv` and `getenv` through `dlsym(RTLD_NEXT, ...)` and flags a `getenv` on thread R as an overlap when a writer on thread W is in progress. It distinguishes two phases: a **call** overlap means the reader ran while real glibc `setenv` was executing, and a **pre** overlap means the reader ran during the optional artificial widening the shim inserts beforehand. Only `call` overlaps at `ENVPROBE_WIDEN_US=0` are unassisted. Widening enlarges a window the schedule already permits; it cannot manufacture one. That instrument decided UF-001 (one unassisted `call` overlap in 100 runs, 89 of 100 at 500 microseconds, 92 of 100 at 2000, every one of them the predicted libgit2 `git_sysdir_guess_home_dirs` chain) and the mechanism of UF-002 (102 unassisted overlaps in 150 runs of the scratch reduction, all on SQLite's `unixTempFileInit`).

**The shim sees only PLT `getenv`.** It cannot see calls the C library makes to its own internal symbol, which is exactly where the glibc resolver reads `RES_OPTIONS` and `LOCALDOMAIN` during `getaddrinfo`. The shim's own self-test proved this during EXP-010. That single blindness is why UF-004 stayed OPEN with the writer removed as remediation rather than with the reader proved, why EXP-102 is still deferred on a helgrind arm, and why EXP-105's zero-overlap result for UF-037 must be read as "could not look" rather than "looked and it was clean".

**Miri ran four axes and found nothing in workspace code.** Default Stacked Borrows, Tree Borrows, strict provenance and symbolic alignment were run over six crates. No Miri report anywhere names a workspace frame. The one report is inside `crossbeam_epoch::internal::Local::element_of`, a container-of pattern reached from rayon workers, which Stacked Borrows rejects and Tree Borrows accepts. That is a limitation of the model applied to the dependency, not a workspace defect, and it persists after the 0.9.21 bump because the code is byte-for-byte identical. The matrix caveat is in section 4.

**ASan carried the dependency work, and every ASan run had to prove its own admissibility.** EXP-104 (UF-039) and EXP-201 (UF-040) reproduced their findings both plain and under ASan. EXP-301 exercised the AVX2 memmem kernels over 51,215 cases under ASan with `-Zbuild-std` and 118 `__asan_` symbols confirmed present. The bigbox pass 2 then closed the three hostile-corpus experiments, each with the instrumentation demonstrated by `nm` on the exact C object under test rather than assumed: libgit2 1.9.7 (`repository.o` 25 `__asan_` symbols, `config_file.o` 19, 2,517 across 206 objects) for EXP-205, SQLite 3.46.0 (`sqlite3.o`, 30) for EXP-206, and LMDB 0.9.70 (`mdb.o`, 28) for EXP-207. All three came back clean with every hostile input rejected through the library's own error path and bounded resident memory. A benign ODR false positive from libgit2's bundled llhttp was suppressed with `detect_odr_violation=0`. A run that did not instrument is inadmissible, not negative, and the harnesses' own inline admissibility checks initially printed zero because they globbed the wrong object path; the corrected counts above are the admissible ones. The runbook carries the procedure.

**EXP-302 needed no sanitiser.** The signal was the shipped server's own death: bash reported `Bus error`, the harness exited 135, and the server log stops mid-scan. The corrections that made that run admissible are recorded in the runbook, because three of them silently invalidate a run rather than failing it: the MCP tool is named `terraphim_grep`, the needle must sit at the file tail so the grep reads the whole mapping, and every tool call needs a client-side timeout.

**Two instruments were wanted and absent.** valgrind and helgrind are not installed on bigbox, which is the only reason the glibc resolver reader was never observed directly. `ltrace` is absent for the same reason.

**One instrument cannot be re-run from the archive as archived.** The env-probe driver `run_probe.sh` hard-codes its shim path and log directory and ignores the environment overrides the remediation plan's invocation assumes. A maintainer following the plan verbatim would load no shim at all and read the resulting silence as a clean result. The corrected invocation is in section 6 of `UB_RUNBOOK.md`; the plan's text should be amended the next time it is touched.

---

## 4. Honesty section: what the instruments could not see

This section is mandatory and is written because a verdict token is worthless if it hides the limits of the tool that produced it. Phase 4 defines NO_EVIDENCE as "an experiment **able to see the predicted signal** ran and stayed clean". The following results do not meet that definition, or meet it only conditionally. The first item is not an instrument limit at all but a false claim this audit published and has since retracted, and it belongs at the head of this section because it is the same error the section exists to prevent.

### 4.0 Retraction: the libgit2-sys bump did not clear the git2 advisories

**What was claimed.** That `cargo update -p libgit2-sys --precise 0.18.8+1.9.7` cleared RUSTSEC-2026-0183 and RUSTSEC-2026-0184, and that Gitea #257 was therefore not a blocker. The claim originated in the Phase 6 round 2 report, the lead accepted it, and it was then repeated in commit 46bbc43's message, in a `deny.toml` comment, in a comment on Gitea #254, and in lead addendum 5.

**What is true.** Both advisories are against the **git2 crate**, not the libgit2 C library. RUSTSEC-2026-0183 is a null pointer passed to `slice::from_raw_parts` in `Remote::list()` when a remote advertises no references. RUSTSEC-2026-0184 is null `Signature` pointers from a `BlameHunk` created through `Blame::blame_buffer()`. Both are patched in git2 0.21.0 or later. HEAD's lockfile still carries git2 0.20.4, pulled solely by `fff-search` 0.8.4, and `cargo audit` still reports both.

**Why nobody noticed.** `cargo deny check` was green. Both advisories are `informational = "unsound"`, and this `deny.toml` treats unsound-class advisories as informational rather than as denials, so the gate was never going to fail on them. A passing gate was read as the advisories being gone.

**Why this belongs in the honesty section rather than in a footnote.** It is the same failure class the Phase 10 fresh-eyes review had already named in this audit: a clean result from an instrument that was not looking for the thing. The lead committed it after being warned of it, in the same document set that contains the warning. The bump itself is harmless and is kept, a patch-level move of the C library; only its justification was wrong. The retraction landed in commit e90b193, which amends the `deny.toml` comment and nothing else.

**What actually closes the pair.** Delta 5 of the remediation plan enumerates every git2 item `fff-search` 0.8.4 references: `git2::Repository`, `git2::Error` and eight `git2::Status` flags. There is no `Remote`, no `Blame` and no `blame_buffer` anywhere in its sources, and no workspace crate depends on git2 directly, so neither advisory's vulnerable API is reachable at any input. That argument, not the bump, is why the pair can be lived with. **It is a property of the source as read on 2026-09-12, not a guarantee.** It lapses if the pin is raised or if `fff-search` grows a remote or blame feature, which is exactly what the `ci_guards` pin assertion exists to prevent.

### 4.1 Verdicts recorded on instruments that could not see the signal

**EXP-105 / UF-037 is the primary case.** The overlap arm was closed NO_EVIDENCE on 400 iterations of the GitHub-backend path with zero overlaps at both widenings. The instrument was the LD_PRELOAD shim, and the predicted reader is the glibc resolver's internal `getenv` inside `getaddrinfo`, which the shim by its own recorded note cannot see. This is precisely the property for which EXP-001 was given NEEDS_REFINEMENT. Two experiments, the same epistemic position, different tokens. What does survive is the other arm: the shim can count `setenv` calls, and it counted zero in 400 of 400 runs on the default R2 backend, which establishes that the shipped default path never enters `self_update` at all. So the conclusion holds on the arm the instrument could see, and the overlap arm should be read as NEEDS_REFINEMENT. UF-037 remains the only LIKELY-UB row on a shipped production path.

**EXP-003c was re-opened for the same class, and has since been closed properly.** It carried NO_EVIDENCE over a run that executed zero tests: `terraphim_command_runtime` has no `#[test]` functions, so Miri had nothing to execute, and the block's own falsifiability line says the run "neither supports nor refutes". It was corrected to NEEDS_REFINEMENT and then closed NO_EVIDENCE on 2026-09-13 by the route the correction implied, a real harness across all four axes, rather than by re-reading the empty run. It is the cleanest example in this audit of a vacuous pass being refused and then earned.

**EXP-010 / UF-004 never observed its reader.** The shim reported zero overlaps in 200 iterations and is provably blind to the candidate reader. A second probe (`gaiprobe`) showed writer-and-`getaddrinfo` windows in 6 of 100 iterations at 500 microseconds of widening, which establishes the necessary condition without establishing the race. The finding carries three different tokens in three files: DEFERRED in its verdict line, NEEDS_REFINEMENT in its Phase 5 note, OPEN in the Phase 4 table. Nothing turns on the disagreement because the remediation removes the writer regardless, but no reader can tell which file is authoritative.

**EXP-003e must not be cited against UF-001.** It is NO_EVIDENCE over 34 `terraphim_grep` tests, and by its own note the 23 tests it filtered out are exactly the UF-001 surface.

**EXP-003d ran 50 tests with 36 filtered.** The filtered set is the kqueue and rusqlite FFI surface, which Miri cannot execute at all.

**EXP-002b is clean for a contingent reason, not a structural one.** The instrument could see the signal and reported zero overlaps in 300 runs of the shipped binary. The reason is ordering: SQLite's `getenv` pair runs once per process, and the serial writer test happens to be the first test to open a connection, so the read lands on the writer's own thread. Reorder the tests, or let any non-serial sibling open a connection first, and EXP-002c shows what happens. Clean by scheduling accident is not clean by construction.

**EXP-202 and EXP-203 both closed NO_EVIDENCE, each on its Stacked Borrows axis, and the story of how they closed is worth keeping.**

Name the axes by their flag, because "strict" and "permissive" have been used inconsistently for them. The **default** axis, `-Zmiri-disable-isolation` alone, is Stacked Borrows, the stricter aliasing model, and it is the axis that carries both verdicts. `-Zmiri-tree-borrows`, the more permissive model, is corroboration in both cases and is complete in neither.

- **EXP-202** (hostile YAML frontmatter into `unsafe-libyaml` through serde_yaml 0.9) closes on a Stacked Borrows axis over the full 43-input hostile corpus with `MIRI_EXIT=0` and no diagnostic. Its Tree Borrows axis on bigbox is partial corroboration: 34 of 43 inputs in 4 hours 6 minutes with no diagnostic, including the 52 KB folded input that had stalled the macOS host, before the four-hour limit; a second process covering the remaining nine spent 2 hours 52 minutes inside one many-keys input without finishing. Neither log carries a UB or unsupported-operation line. **UF-041 closes NO_EVIDENCE.** Migrating off serde_yaml 0.9 is still worth doing, and is tracked as #317 on hygiene grounds rather than on evidence: it is an unmaintained crate whose only backend is machine-translated C, and a sampled corpus is not a proof of absence.
- **EXP-203** (attacker-influenced manifest versions into semver's inline-storage `unsafe`) closes on a re-run of its Stacked Borrows axis under a `bash -c` driver: 308 crafted versions, zero panics, no diagnostic, `MIRI_EXIT=0` captured. **UF-042 closes NO_EVIDENCE for undefined behaviour.** What does not close is the ordering: the version is still parsed before the zipsign signature is verified, which is a defence-in-depth defect independent of whether today's parser survives it, and it is tracked as #318.
- **EXP-003c**, the registry's last NEEDS_REFINEMENT, also closed. It now has a real harness over four of the five public helpers of `terraphim_command_runtime`, nine query shapes, and a clone and lock round trip, using a noop-waker `block_on` rather than a tokio runtime, with `save_config` deliberately excluded because it writes to the real persistence backends. Clean on all four axes with captured exit 0. The vacuous run that preceded it was correctly refused as evidence.

**EXP-203's Tree Borrows axis has no verdict after four aborts, and the remedy this report previously recommended is wrong.** The four attempts reached 110 GB uncapped, then 48.2, 54.4 and 43.7 GB under input caps of 8192, 256 and 32 bytes. All four share one memory curve, roughly 13 GB at ten minutes and past 40 GB by 25 to 28 minutes, **independent of the cap**, because the memory tracks interpreted steps and Miri's accumulating Tree Borrows state rather than input length. So "re-run with the corpus capped" cannot work, and any advice to that effect in the audit's own soak designs is corrected. The remedies that do work are to drop each parsed value immediately instead of accumulating them, or to run one Miri process per input, which is exactly what made campaign C tractable.

**The admissibility rule that got the audit here was not relaxed, and the attempt to relax it is part of the record.** When the two exit captures came back empty, a log-based substitute was proposed: score a run clean if the harness printed its completion marker and the log carries none of `error: Undefined Behavior`, `error: test failed`, or `process didn't exit successfully`. The substitute has real supporting evidence. EXP-101's tree-borrows axis genuinely exited 1 and carries all three markers, the Stacked Borrows report at line 335, the test failures at 423 and 667 and the cargo exit lines at 426 and 670, while EXP-202's correctly captured default axis carries none. Where the exit status is trustworthy the two signals agree.

It was withdrawn all the same. Agreement where the exit status is trustworthy does not license substitution where it is not, and the proposal's practical effect would have been to score two runs the rule made unscoreable. That is the same convenient reading the audit had already made twice on the day, once in treating a green `cargo deny` as an advisory clearance and once in treating a silent env-probe run as a clean one. The rule stands: **an empty `MIRI_EXIT` makes a run inadmissible whatever its log says**, and a log check is corroboration. Both experiments were then re-run under `bash -c` drivers and closed on captured exit statuses, which is the outcome the strict rule was protecting.

**The defect itself belongs in the record next to the env-probe driver.** Both drivers captured the exit status with `${PIPESTATUS[0]}` under tmux, tmux inherits the user's login shell, that shell is zsh, and `PIPESTATUS` is bash-only; zsh spells it `pipestatus` and indexes from one. Verified on the audit host: `zsh -c 'false | true; echo "a=${PIPESTATUS[0]} b=${pipestatus[1]}"'` prints an empty `a` and `b=1`. This is the same shape as the env-probe driver defect in section 3, an instrument whose own reporting is broken so that silence reads as success, and in both cases nothing failed loudly and the artefact looked finished.

**EXP-205's delta-inflate arm is untested, not negative.** The experiment closed NO_EVIDENCE on its config-parsing arms with ASan admissibility proven against libgit2 1.9.7, but the arm that was meant to probe the CVE-2026-53585 allocation path could not be built: git 2.43's packer never places a delta as the strict last pack object, and patching a mid-pack delta shifts the trailing offsets so libgit2 rejects the pack before any delta allocation happens. The fifth repository therefore stands as a malformed pack that libgit2 refused safely, which is a real result about input validation and not a result about the inflated-delta allocation.

### 4.2 The Miri matrix, now measured rather than caveated

Earlier drafts of this report said the four Miri axes "did not run the same tests, so the cells are not comparable". Phase 11 campaign C closed that gap by running each previously skipped test in its own Miri process, which is what made the pass tractable: a batch run aborts on the first failure, so one uncoverable test used to hide every test behind it.

The result is precise. For `terraphim-session-analyzer`, **the three Stacked-Borrows-based axes each cover the same 123 of 128 tests**, and Tree Borrows covers all 128.

| Axis | Tests covered | How it got there |
|---|---|---|
| default (Stacked Borrows) | 123 of 128 | 80 in Phase 3, plus 43 of a 48-test delta in campaign C |
| strict-provenance | 123 of 128 | 80 in Phase 3, plus 43 of 48 |
| symbolic-alignment | 123 of 128 | 73 in Phase 3, plus 50 of 55 |
| tree-borrows | 128 of 128 | the full set in Phase 3 |

**The five uncovered tests are the same five on every Stacked-Borrows-based axis, and the reason is identical in each.** They are `analyzer::tests::test_analyze_drops_sessions_without_target_file_operations` and four `parser::tests` cases, all of which drive `SessionAnalyzer::analyze`, whose `par_iter` at `crates/terraphim-session-analyzer/src/analyzer.rs:70` starts rayon's global pool. On a rayon worker, crossbeam-epoch 0.9.21 then either violates Stacked Borrows at `internal.rs:567`, in `Local::element_of` reached from `Global::try_advance` by way of `Stealer::steal`, or performs an integer-to-pointer cast at `atomic.rs:204` in `Pointable::deref`, reached from `List::insert` in `Local::register`, which the strict-provenance axis refuses as an unsupported operation.

**Every erroring frame is a dependency frame.** Workspace code appears in no backtrace; `analyzer.rs:70` is named only by Miri's "called indirectly due to this code" note, that is, as the code that started the pool. Under this audit's rule that is a dependency limitation, not a workspace finding, and it repeats EXP-101 rather than adding anything. Tree Borrows covers all 128 precisely because it does not flag the pattern. `-Zmiri-ignore-leaks` was set on every axis, as the design requires; it was necessary but not sufficient, because these five abort on the crossbeam-epoch report rather than on a leak.

Campaign C's verdict is NO_EVIDENCE of workspace undefined behaviour.

---

## 5. Convergence evidence

The Phase 0 configuration set a floor of **10 rounds**, with this recorded rationale: the archetype-aware round floor in the skill's `CONVERGENCE.md` requires 10 rounds in Exhaustive mode for any project with `unsafe` in crate code, and this project has six environment-setter blocks across three files, so the forbid-unsafe carve-out that would have lowered the floor does not apply. The floor was recorded as non-negotiable.

**Ten rounds were run, and the tenth is quiet.** The tracker counts **experiment verdicts** (38 EXP blocks), not unified findings (44 UF rows); the two totals are different objects and must not be reconciled against each other.

| Round | OPEN | CONFIRMED | NO_EVIDENCE | NEEDS_REF | DEFERRED | new findings | new needs | quiet |
|---|---|---|---|---|---|---|---|---|
| 1 | 1 | 2 | 11 | 3 | 1 | 35 | 3 | no |
| 2 | 8 | 2 | 12 | 0 | 5 | 4 | 0 | no |
| 3 | 0 | 3 | 15 | 0 | 9 | 0 | 0 | **yes** |
| 4 | 4 | 3 | 15 | 0 | 13 | 4 | 0 | no |
| 5 | 2 | 4 | 16 | 0 | 15 | 1 | 0 | no |
| 6 | 2 | 4 | 16 | 1 | 15 | 0 | 1 | no |
| 7 | 2 | 4 | 16 | 1 | 15 | 0 | 0 | no |
| 8 | 2 | 4 | 17 | 1 | 14 | 0 | 0 | no |
| 9 | 2 | **5** | 20 | 1 | 10 | 0 | 0 | no |
| 10 | **0** | 5 | 23 | **0** | 10 | 0 | 0 | **yes** |

A round is quiet when all three of these hold, read from the tracker script itself: zero OPEN experiments, fewer than three new findings, and zero newly added NEEDS_REFINEMENT entries. The loop may end only after two consecutive quiet rounds. Round 3 was the single quiet round; round 4 broke it by adding four findings. Rounds 6 to 9 were not quiet solely because two experiments were OPEN, namely EXP-202 and EXP-203; both closed on 2026-09-13, which is what makes round 10 quiet. EXP-801 to EXP-804 are DEFERRED throughout, not OPEN as the plan's section 15 states. Round 6's single new needs-refinement entry is EXP-003c, re-opened by the Phase 10 review from a vacuous run.

Round 10 is the first round since round 3 in which no experiment is OPEN and none is NEEDS_REFINEMENT, and it is the first quiet round since round 3. Rounds 8 and 9 moved verdicts without moving findings. Round 9 carries the four bigbox pass 2 results: EXP-302 became the fifth CONFIRMED_UB verdict, and EXP-205, EXP-206 and EXP-207 moved from DEFERRED to NO_EVIDENCE, which is why DEFERRED falls from 14 to 10 and NO_EVIDENCE rises from 17 to 20. EXP-302 confirmed the blast radius of an already-confirmed mechanism, so it added no finding row and did not reopen convergence.

**Where this leaves the audit: converged.** The ten-round floor is met, and rounds 10 and 11 are both quiet, which satisfies the two-consecutive-quiet condition and ends the loop. The tracker recorded it as `CONVERGED after round 11 (>=10 rounds, two consecutive quiet)`. Round 11 was a confirmation pass rather than an investigation: the only work between rounds 10 and 11 was documentation reconciliation, namely sweeping three statements that later results had superseded (the UF-042 abort count, the EXP-003c matrix row, and the Miri comparability caveat). Every experiment carries a verdict, the registry holds no OPEN and no NEEDS_REFINEMENT entries, and no confirmed finding lacks a remediation issue.

What remains outstanding is corroboration rather than evidence, and it is tracked on #320. EXP-202's Tree Borrows axis covered 34 of 43 inputs before the time limit and EXP-203's never completed at all, but both verdicts rest on their Stacked Borrows axes, which is the stricter model and the one that would have flagged the pattern if it were there. The helgrind arm for UF-004 and UF-036 is still blocked on valgrind. Neither gap changes a verdict.

---

## 6. Open questions

Lead addendum 8 requires these to be split, because "DEFERRED" in the Phase 4 table means three different things and one of them is not a judgement at all. They are split three ways here: set aside after investigation, answered by an experiment that has since run, and still blocked on an instrument.

### 6.1 Investigated and set aside

These were examined, a static rationale was recorded, and no experiment was allocated. They should not be re-investigated unless the cited lines change.

| UF | Set aside because | Re-check when |
|---|---|---|
| UF-006 | Durability defect, not memory UB; `ETXTBSY` makes rollback impossible on Linux | `lib.rs:1536` or `rollback.rs:310` changes, or rollback is made to work |
| UF-007 | Deterministic panic, no UB; behind the `server` feature with no CI lane | a `server` feature lane is added, or `server_command.rs:751` changes |
| UF-012 | Apple Libc locks both sides; contract violation only, macOS only | the hyper-util proxy path changes, or a Linux equivalent appears |
| UF-013 | Sound while the binary holds one test; gated by `RUN_MCP_STDIO_TEST` | a second test is added to `test_mcp_stdio.rs`, or the gate is set in CI |
| UF-014 | All eight sites are `#[serial]`, persistence is memory-only, servers are separate processes | a non-serial test is added to either binary, or `#[serial]` count stops matching test count |
| UF-016 | Unsound `Event::with_tag` path unreachable from sqlx-core | **closed**: event-listener is 5.4.2 at HEAD |
| UF-017 | No reachable call path for any of the four advisories | **closed, on two arguments.** anyhow 1.0.104, lru 0.18.4 and memmap2 0.9.11 at HEAD clear three of them. The git2 pair is **not** cleared by any bump in the lockfile and closes only on unreachability plus the `=0.8.4` pin; re-open it if the pin is raised or `fff-search` gains a remote or blame feature |
| UF-018 | Single assignment after full construction, no await under the guard, one-shot hook process | `kg_validation.rs:83-116` gains a second writer or an await |
| UF-019 | Consistent lock order, no inversion, tokio locks do not poison | a new call site inverts the `service` then `config` order |
| UF-020 | The cycle needs two queued writers and both readers concurrently; today's sharing model is single-task | the registry is shared across tasks |
| UF-021 | Every reader tolerates the torn shape via `Option` | a reader stops handling `None` |
| UF-022 | Duplicate work is idempotent | import stops being idempotent |
| UF-023 | Not UB; a test-reliability hazard | tests start writing into the real `~/.terraphim` again |
| UF-024 | Latent flake; tests do not assert `base_url` | closed outright by #280 commit 1 |
| UF-029 | Correctness bug, not UB; lives in registry crates | terraphim-ai#3376 lands |
| UF-030 | Not compiled on either audit host | a Windows CI lane is added |
| UF-031 | Panic across `extern "C"` aborts on the pinned rustc, which is defined behaviour | the toolchain's abort-on-unwind behaviour changes, or the closure gains a panic path |
| UF-032 | Legal on both runtime flavours; resource cost only | a caller appears in workspace tests |
| UF-033 | All `Runtime::new` calls are on a `NotEntered` thread | a third runtime is added, or one is constructed inside another |
| UF-034 | No in-workspace caller outlives the runtime | the sessions watcher receiver is given a longer lifetime |
| UF-035 | Every site is on an error path after its own output completed; no `TempDir` live | a `std::process::exit` is added where a destructor matters |

### 6.2 Investigated on the bigbox pass 2, after the instrument became available

Four questions sat in the section below until 2026-09-12, blocked not on a tool but on an operator decision about generating deliberately hostile input. They have now been answered, and they are recorded here rather than in section 6.1 because they were resolved by running an experiment, not by reasoning about the code.

| Experiment | Surface | Result |
|---|---|---|
| **EXP-302** | Blast radius of UF-039 inside the long-lived `terraphim_mcp_server --sse` process | **CONFIRMED_UB.** The shipped server died with SIGBUS under eight concurrent SSE clients when a sibling truncated the mapped tree. The same-length in-place writer arm left the server alive with no invalid UTF-8 over the wire, which is consistent with EXP-104's measured rate of roughly one invalid string per 400,000 lines on the PlainText sink rather than with the path being unreachable. This is the pre-fix half of the pair with EXP-805 |
| **EXP-205** | Hostile local git repositories reach bundled libgit2 through `fff-search`'s `Repository::discover` and status | **NO_EVIDENCE**, ASan admissibility proven against libgit2 1.9.7. A 200-level `include.path` chain, a self-referential include loop and a 300-entry parent config all produced zero ASan reports and resident-memory deltas between 1.7 and 20 MB. The row stays CONTRACTUAL-BUT-DEFENSIBLE. The delta-inflate arm is untested rather than negative; see section 4 |
| **EXP-206** | Crafted SQLite database files reach bundled SQLite 3.46.0 through the Cursor connectors | **NO_EVIDENCE**, ASan admissibility proven against SQLite 3.46.0. All fourteen corrupted variants were rejected through SQLite's own error path, including FTS5 shadow-table corruption. The row stays CONTRACTUAL-BUT-DEFENSIBLE |
| **EXP-207** | `FFF_FRECENCY_PATH` points the MCP server's heed/LMDB unsafe open at a hostile memory-mapped database | **NO_EVIDENCE**, ASan admissibility proven against LMDB 0.9.70. All eleven crafted databases produced validated rejections at open or clean errors on read rather than a wild dereference. The row stays CONTRACTUAL-BUT-DEFENSIBLE; the residual hardening is refusing `FFF_FRECENCY_PATH` outside a trusted directory |

None of these changed a finding row, and only EXP-302 changed a verdict count.

### 6.3 Not yet investigated because the instrument was unavailable

These are open hypotheses with a resource gate, not judgements. Treating them as closed would be wrong.

| Item | Surface | Blocked on | Re-check criterion |
|---|---|---|---|
| **EXP-010 follow-up (helgrind arm)** | The glibc resolver `getenv` reader for UF-004 and UF-036. It has no block of its own; it lives in EXP-010's notes | valgrind is not installed on bigbox | Install valgrind on bigbox, or run it on any glibc host that has it. Zero overlaps across 100 iterations at each widening with both SSL variables unset would downgrade UF-036 to CONTRACTUAL-BUT-DEFENSIBLE |
| **LMDB mapping via the frecency tracker, under concurrent modification** | `FrecencyTracker::open` at `crates/terraphim_mcp_server/src/lib.rs:110` opens an LMDB environment that memory-maps its data file | **Partly covered, on the wrong axis.** EXP-207 drove hostile file *contents* through that open and found LMDB validating meta pages and returning clean errors. It did not test another process modifying or truncating the mapped file while it is live, which is the UF-039-shaped question | Re-run EXP-207's harness with a sibling writer and a sibling truncator against the mapped `data.mdb`. The UF-039 remediation removes `fff-search`'s mapping and nothing else; nobody should read commit 0 as "this process no longer maps memory" |
| **libgit2 mapping via repository discovery, under concurrent modification** | libgit2 maps pack files during the git-status lookup the same grep performs | **Partly covered, on the wrong axis.** EXP-205 drove hostile repository contents through `Repository::discover` and status. It did not test a pack file being rewritten or truncated under a live mapping | Same shape as above. Also untested: the inflated-delta allocation path, which could not be constructed with git 2.43's packer |

The operator decision that once blocked this section has been resolved. EXP-205, EXP-206, EXP-207 and EXP-302 were all run to a verdict on the bigbox pass 2, so the only items left here are the helgrind arm and the two concurrent-modification axes above.

One more category has no token in the taxonomy at all: EXP-801 to EXP-806 are post-remediation proofs that cannot run until Phase 13 lands the fixes. They are honestly labelled DEFERRED, but they are neither set aside nor blocked on an instrument.

---

## 7. Blockers and operator actions

None of these is work an agent should do.

| Blocker | Effect | Action |
|---|---|---|
| **#281: 62 orphaned `terraphim_server` processes on bigbox** | Scheduling noise inflates or hides the widen=0 hit rate in the EXP-801 to EXP-803 controls; the processes also bind ports and open the same SQLite stores that campaigns B and D need | Kill them deliberately (`pgrep -af terraphim_server`, then `pkill -f /tmp/terraphim_server_install/bin/terraphim_server`) before any probe session. Record `uptime` load in each experiment log header. Land #281's `kill_on_drop` fix separately |
| **Rogue runner `terraphim-native-c610efaf`** | Any probe or CI result produced on that runner is inadmissible for this plan | Deregister the runner in Gitea. Pin the probe session and the remediation pull request's CI to `terraphim-native` (bigbox) by name |
| **bigbox root disk (standing precondition, not a live blocker)** | `/dev/md2` (3.5 TB) reached 100 percent on 2026-09-12 and took the Gitea PostgreSQL container into a restart loop, so `git.terraphim.cloud` answered 500 on push and 401 on the API with a valid token. It has since cleared; both bigbox passes ended with 284 GB free and never went below the 60 GB floor | Review the largest consumers: `/home/alex/gitea-stack` 415 GB and `/home/alex/quarantine-20260819` 226 GB, then `/home/alex/projects` 71 GB and `/home/alex/scratch` 35 GB. Every soak campaign requires at least 60 GB free before it starts. The outage is timed 10:38 BST in the Phase 11 designs and 11:45 BST in the Phase 8 and Phase 9 logs; 10:38 is when the disk filled and 11:45 is when the lead recorded it |
| **bigbox hosts both the heavyweight audit work and the team's git server** | Twice in two days a campaign on bigbox threatened a service the team depends on. On 2026-09-12 the root disk filled and took the Gitea PostgreSQL container into a restart loop, so `git.terraphim.cloud` answered 500 on push and 401 on the API with a valid token. On 2026-09-13 an uncapped Tree Borrows run went from 33 GB to 110 GB **between two 45-second polls** and left the host with 4 GB available; the executor killed it and the host recovered immediately, and no service was observed down, but the margin was thin, and a 20-second poll still overshot to 48 and 54 GB | Say it plainly: heavyweight audit work and the team's git server should not share a host. Until they are separated, any Tree Borrows campaign on bigbox needs an automatic kill polling at **10 seconds or tighter**, and every campaign needs its memory ceiling declared before it starts, not discovered. Capping the input corpus is not a substitute; see section 4 |
| **valgrind absent on bigbox** | The EXP-010 helgrind arm cannot run, so the LIKELY-UB rating of UF-004 and UF-036 rests on contract rather than observation. It does not block the fix, which removes the writer | Install valgrind, or nominate another glibc host |
| **CI test lane red on #264 and #278** | "Fails before, passes after" cannot be read from lane colour | Fix or ignore those two tests on their own issues. Until then, report the remediation per binary as section 11 of the plan defines |
| **Gitea bookkeeping that Delta 5 requires** | #257's meaning has changed and its title no longer describes it; #294 is missing the acceptance criterion that protects the UF-039 fix | Re-title #257 from "bump fff-search to clear the advisories" to "revisit when fff-search ships a release that both requires git2 0.21 or later and gates the fresh-mapping path", and make it depend on the upstream issue. Add to #294's acceptance criteria: `fff-search` pinned `=0.8.4` in both dependent crates, with a `ci_guards` assertion on the pin. Add to #297's: the `ci_guards` arm asserts no site sets `follow_symlinks: true` |
| **The env-probe driver cannot be re-run as archived** | `run_probe.sh` hard-codes its shim path and log directory and ignores the overrides the remediation plan's invocation passes, so a maintainer following the plan loads no shim and reads the silence as clean | Apply the two-line edit in section 6 of `UB_RUNBOOK.md` and store the edited copy as the permanent instrument. Amend the plan's text the next time it is touched |
| **Three upstream issues the user must file** | Two on `fff-search` and one on `self_update`; external trackers were out of scope for the agents | Full texts are verbatim in `phase9_gitea_log.md`, section "Upstream issue texts for the user to file". **The `fff-search` mapping text must be extended before filing**: 0.10.6 has been read, it did not fix either finding, it adds a second unbudgeted `Mmap::map` at `src/types.rs:762` in `get_content_for_search`, and `enable_mmap_cache` gates neither site. `read_to_buf` is unchanged in 0.10.6, so the UF-040 report stands against current upstream as written. Note the workspace's own historical git dependency is recorded as a different fork, so resolve which repository publishes the crate first |

---

## 8. What to do next, in order

No experiment blocks this list. Every block in the registry carries a verdict, so the order below is a remediation order rather than a queue behind outstanding evidence.

1. **Land the production-path pull request first.** It is the only change that touches shipped behaviour and it must not wait for the test-only train. Branch `task/293-fff-hardening` off `main`, two commits.
   - **Commit 0** for **#293** (UF-039): at all four `FilePicker::new` sites, replace `cache_budget: None` with an explicit `ContentCacheBudget { max_files: 0, max_bytes: 0, max_file_size: 10 * 1024 * 1024 }` behind a small `no_mmap_budget()` helper per crate. The sites are `crates/terraphim_grep/src/hybrid_searcher.rs:354` and `crates/terraphim_mcp_server/src/lib.rs:1274`, `:1372`, `:1485`. Do not use `ContentCacheBudget::zero()` and do not set `max_file_size: 0`: both also disable the slow path and silence grep entirely. `from_overrides` cannot express `max_files: 0`. Add the `ci_guards` arm `no_fff_picker_maps_files` in the same commit. **Pin `fff-search` to `=0.8.4` in both dependent crates and assert the pin in `ci_guards` in this same commit**, with the reason in the comment: 0.10.6 adds a second, unbudgeted mapping path reached exactly when a zero-file budget refuses the first, so a routine `cargo update` would leave the mitigation visibly in place while it stopped working for every file at or above 256 KiB on Linux. **EXP-302 is this commit's pre-fix control**: the same eight-client SSE soak that killed the shipped server must complete with zero deaths and zero invalid responses afterwards, which is EXP-805.
   - **Commit 0b** for **#296** (UF-040): add `reject_hostile_tree` in a new `crates/terraphim_grep/src/path_guard.rs`, called at the head of `search_code` and ahead of each of the three MCP picker constructions, refusing any entry name that is not valid UTF-8 and any relative path longer than 4095 bytes. Per the Phase 10 review and Delta 4, the `ci_guards` arm must also assert `follow_symlinks` explicitly at all four sites rather than inheriting it, since the guard's soundness currently rests on an upstream default of `false` that the workspace never states.
   - Blocked by the test issues #294 and #297 and the docs issues #295 and #298.
2. **Then the test-only remediation train**, branch `task/252-env-unsafe-remediation`, in the order the plan fixes, because commit 7 will not compile before commits 1 to 6: inject the OpenRouter base URL (#280), inject the TUI settings directory (#290), inject the update history path (#299), pass `TERRAPHIM_SETTINGS_PATH` to the child with `Command::env`, pass the profile and log directory to the spawned servers and drop `terraphim_test_utils`, delete the dead `rust_has_unsafe_env_setters` build script, then the lint, then docs, then ignore the live-network GitHub check (#302).
3. **Then the lint lever, #310.** `[workspace.lints.rust] unsafe_code = "forbid"` in the root manifest plus `[lints] workspace = true` in all eleven members, the `ci_guards` manifest test that stops a new crate opting out, and the `deny.toml` ban on `terraphim_test_utils`. It is blocked by #280, #290, #299 and its own #311 and #312.
4. **Then #313 as CI hardening.** Switch both coverage lanes from in-process `cargo llvm-cov` to `cargo llvm-cov nextest`. Read it as hardening only: its SSL preset is the candidate that lost to `#[ignore]` in the plan because it hides the writer and never reaches a developer's `cargo test`, so landing #313 does not close UF-036.
5. **Then the robustness issues**, which are independent of everything above: #304 (reject empty tool patterns at load time) and #307 (fix `parse_correction_pattern`'s offsets). #307 has no section in the remediation plan and needs the remediation architect's sign-off before implementation.
6. **Then the post-remediation proofs**, EXP-801 to EXP-806, once bigbox is clear: the three env-probe count arms, the lint fault injection, the EXP-104 harness rerun with the post-fix option block, and the EXP-201 geometries driven through the workspace entry points. EXP-805 must include the EXP-302 SSE soak, since that is the only arm that exercises the fix in the shipped binary.
7. **No experiment work remains before remediation.** Every block carries a verdict and round 10 is quiet, so step 1 can start immediately; nothing above waits on an instrument. What is left of the audit's own work is corroboration tracked on #320: EXP-202's and EXP-203's Tree Borrows axes on a host that can hold them, using one Miri process per input rather than a capped corpus, and the helgrind arm once valgrind is installed. Run round 11 as a confirmation pass to close the convergence loop. None of this is remediation work and none of it blocks the pull requests above.
