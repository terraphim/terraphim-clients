# Undefined-behaviour runbook

Permanent gate document for `/Users/alex/projects/terraphim/terraphim-clients`, a Rust workspace of 11 crates and six CLI binaries. Written 2026-09-12 from the Exhaustive undefined-behaviour audit recorded under `/Users/alex/projects/terraphim/terraphim-clients/.ub-exorcism/2026-09-11-terraphim-clients-1`, whose findings are summarised in `FINAL_UB_REPORT.md` in that directory.

You do not need to have read the audit to use this document. Two facts from it shape everything below. First, this workspace contains no `unsafe` in production source; the only six `unsafe` blocks it ever had were environment setters in tests, and the remediation removes all six. Second, that did not make it safe: the two confirmed production-path findings are both inside the `fff-search` dependency, whose own safety arguments the workspace's configuration and input defeat, and one of them was reproduced in the shipped `terraphim_mcp_server --sse` binary, which a single file truncation kills for every connected client. The gates here are therefore split between "keep our own code free of `unsafe`" and "keep watching what the dependencies do with theirs".

**If you read only one rule in this document, read the `fff-search` rule in section 7.** The mitigation for the worst finding is defeated by a routine `cargo update`, and it is defeated invisibly: the fix stays in the source and stops working.

Throughout, `$WS` means the audit workspace path given above, and paths starting `crates/` are relative to the project root.

---

## 1. Lints to enable workspace-wide

### The one the audit fixed on

Put this in the root `Cargo.toml`, after the `[workspace.package]` table:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
```

and this in each of the eleven member manifests, immediately before `[dependencies]`:

```toml
[lints]
workspace = true
```

**The crate-root attribute form is not sufficient, and this is the whole reason for the manifest mechanism.** Writing `#![forbid(unsafe_code)]` in `lib.rs` and `main.rs` covers fifteen files and misses every other crate root in the workspace: each file under `tests/`, each bench, each example and each `build.rs` is its own crate root and inherits nothing from the library. That is not hypothetical here. `crates/terraphim_mcp_server/tests/test_mcp_stdio.rs` held one of the six `unsafe` environment setters, and an attribute in `lib.rs` would never have seen it. The manifest `[lints]` table reaches every target in the crate.

Use `forbid`, not `deny`: `forbid` cannot be relaxed by an in-source `#[allow]`, which is the point of the gate.

A guard test keeps the mechanism honest. `crates/terraphim_agent/tests/ci_guards.rs` gains `every_member_inherits_workspace_lints`, which parses the root manifest's `[workspace] members` list and asserts that each member manifest contains `[lints]` with `workspace = true`. Without it, a new crate silently opts out of the policy. The `toml` dev-dependency is already present. Note that the GitHub workflow does not run `ci_guards` today; the Gitea native workflow does.

Macro-generated `unsafe` does not trip the lint, because built-in derives and standard macros are exempt through their spans. Proc-macro output is **not** exempt, so after the lint lands the pre-flight is:

```
cargo clippy --workspace --all-targets -- -D warnings
```

which fails loudly if any proc macro in the tree emits `unsafe` in a target nobody expanded by hand.

Two extensions that are part of the same lever:

- **`deny.toml`**: add `terraphim_test_utils` to `[bans].deny` with the reason "safe wrapper over `std::env::set_var`; the workspace forbids environment mutation in tests, inject values instead". The `cargo deny check` gate then enforces its absence. This matters because the lint is blind to safe wrappers over `unsafe`, which is exactly what that crate is.
- **Sequencing**: the lint commit cannot land before the commits that remove the six `unsafe` blocks and the eight wrapper call sites, or the workspace will not compile.

### Recommended additions the audit did not itself run

These map onto finding classes the audit confirmed, but no run in this audit enabled them, and lint names move between toolchain releases. Check each against the pinned toolchain (`rust-toolchain.toml`, currently 1.97.1) before wiring it into CI, and introduce them one at a time at `warn` before promoting to `deny`.

| Lint | Finding class it would have caught |
|---|---|
| `clippy::string_slice` | UF-009, UF-010, UF-011, UF-028: fixed-byte slicing of input-controlled strings, four confirmed panics |
| `clippy::await_holding_lock` | UF-019: lock guards held across awaits into persistence, search and an HTTP call |
| `clippy::undocumented_unsafe_blocks` | the SAFETY-comment discipline in section 5, for the rare deliberate exception |
| `clippy::indexing_slicing` (targeted, not workspace-wide) | the same byte-slicing family as `string_slice`, but it is noisy; scope it to the parsers |

---

## 2. Miri in CI

Run Miri over the crates whose tests are pure computation. Four axes, each as its own job so a failure names the model that rejected:

```
MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks"                          cargo +nightly miri test -p <crate>
MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks -Zmiri-tree-borrows"      cargo +nightly miri test -p <crate>
MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks -Zmiri-strict-provenance" cargo +nightly miri test -p <crate>
MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks -Zmiri-symbolic-alignment-check" cargo +nightly miri test -p <crate>
```

The fourth flag is `-Zmiri-symbolic-alignment-check`, which is what the Phase 3 logs record. The Phase 11 soak design writes it as `-Zmiri-symbolic-alignment`, which is not a flag; use the spelling above.

Crates that ran clean in the audit and are worth keeping in the matrix: `terraphim_negative_contribution`, `terraphim_hooks`, `terraphim_sessions`, `terraphim_grep`, `terraphim-session-analyzer`. `terraphim_command_runtime` has zero `#[test]` functions, so a Miri job over it is vacuous and must not be recorded as a pass; write a harness test first.

**Three traps, all recorded from real runs.**

1. **rayon makes Miri exit 1 even when every test passes.** rayon's global pool daemon threads outlive `main`, so Miri's leak check reports "the main thread terminated without waiting for all remaining threads" and the process exits non-zero. That is not a UB report. Either pass `-Zmiri-ignore-leaks` as above, or have CI read the `test result:` line rather than the exit status. Do not "fix" it by skipping the rayon tests: the tree-borrows axis covering 128 tests including rayon is the most complete cell the audit produced.
2. **The crossbeam-epoch reports on rayon tests are a dependency limitation, not your bug, and they cost you exactly five tests.** Any test that drives a `par_iter` starts rayon's global pool, after which crossbeam-epoch 0.9.21 either violates Stacked Borrows at `internal.rs:567` in `Local::element_of` or, on the strict-provenance axis, performs an integer-to-pointer cast at `atomic.rs:204` that the axis refuses as unsupported. The code is byte-for-byte identical in 0.9.21, so upgrading will not clear it. **Every erroring frame is a dependency frame**; workspace code appears only in Miri's "called indirectly due to this code" note, naming the line that started the pool. Tree Borrows covers those tests because it does not flag the pattern. If these are the only reports, the run is clean for workspace purposes. Treat any report that genuinely names a workspace frame in the backtrace as a real finding.

3. **Capture the exit status in a shell that has the array you are reading, and treat an empty capture as fatal.** Two of this audit's Miri drivers wrote `MIRI_EXIT=${PIPESTATUS[0]}` and ran under tmux, which inherits the user's login shell. That shell is zsh, and **`PIPESTATUS` is bash-only**: zsh spells it `pipestatus` and indexes it from one, so the capture came out empty and the run became inadmissible under the audit's own rule that an unscored run is not a clean run. Check it on your host with:

   ```
   zsh -c 'false | true; echo "PIPESTATUS0=[${PIPESTATUS[0]}] pipestatus1=[${pipestatus[1]}]"'
   ```

   **Write every Miri driver, and every probe harness driver, as `bash -c`**, especially inside tmux where the shell is whatever the user happens to log in with. Under zsh the status is `${pipestatus[1]}`, one-indexed.

   **An empty `MIRI_EXIT` makes the run inadmissible regardless of what the log says**, and grepping the log for "Undefined Behavior" is supporting evidence, not a substitute. That rule was tested against the temptation to relax it: a log-based substitute was proposed in this audit for two runs whose captures were lost, and rejected, because weakening a stated rule so that an unscoreable run becomes scoreable is the failure this document exists to prevent. A log check is still worth running as corroboration, and the markers worth grepping are `error: Undefined Behavior`, `error: test failed`, and `process didn't exit successfully` (a cargo continuation line, so no `error: ` prefix). Where the exit status is trustworthy the two agree: EXP-101's failing tree-borrows axis carries all three markers, EXP-202's passing default axis carries none.

Budget honestly, and budget memory separately from time. The audit measured a 128-test default-axis run at between 31 and 62 minutes. The symbolic-alignment axis is much slower still and needs a reduced `PROPTEST_CASES` (16 worked) to finish at all. Run axes sequentially; two concurrent Miri processes put the audit host into memory pressure.

**Tree Borrows memory does not track input size, and capping the corpus will not save you.** One experiment aborted four times, at 110 GB uncapped and then at 48.2, 54.4 and 43.7 GB under input caps of 8192, 256 and 32 bytes. All four traced the same curve, roughly 13 GB at ten minutes and past 40 GB by 25 to 28 minutes, because the memory tracks interpreted steps and the accumulating permission state, not the length of the input. If a Tree Borrows run will not fit, the fixes that work are to **drop each value as soon as it is checked rather than accumulating results**, and to **run one process per input**. Shrinking the inputs is not one of them.

**Host-safety rule for any Tree Borrows campaign.** The same run went from 33 GB to 110 GB between two 45-second polls, and a 20-second poll still overshot to 48 and 54 GB. Any such campaign needs an automatic kill polling at **10 seconds or tighter**, and a declared memory ceiling before it starts. Do not run one on a host that also serves anything the team depends on: on the audit's own host this nearly took out the Gitea PostgreSQL container, which a disk-full incident the previous day had already restarted once.

Finally, **record which tests each axis actually ran, and run the awkward ones one process at a time.** A batch Miri run aborts on the first failure, so a single uncoverable test hides every test behind it and the axis silently reports a smaller set than you think. The audit's first matrix looked like one table but was four partial results, 80 tests on two axes, 128 on another and 73 on the fourth. Re-running each skipped test in its own Miri process fixed it: all three Stacked-Borrows-based axes then covered the same 123 of 128 tests in `terraphim-session-analyzer`, Tree Borrows covered 128, and the five that no Stacked-Borrows-based axis can cover are the same five every time, for the dependency reason in trap 2. A cell without a test count is not comparable to any other cell, and per-test isolation is what makes a complete count affordable.

---

## 3. No loom, no Shuttle

Say this plainly so nobody spends a week on it. Loom models `unsafe` concurrency primitives: atomics, `UnsafeCell`, hand-rolled locks. This workspace has none. Its concurrency is tokio tasks, rayon `par_iter` and standard-library channels, all safe Rust. The two confirmed races in this codebase are libc-level, `setenv` against `getenv` on glibc, and loom does not model libc, so it cannot represent the bug even in principle. Shuttle inherits the same limitation. Neither tool is applicable here, and neither should be added to CI.

The instrument that does model these races is in section 6.

---

## 4. Fuzz corpora, sanitiser admissibility, and the soak recipes

**The workspace has no fuzz targets today.** Authoring these four is itself a deliverable. Their honest expected value: they will find panics, not undefined behaviour, because there is no `unsafe` in workspace code for a malformed input to corrupt. Two of the four are worth more than that, because they reach C libraries through dependencies, and those two run under AddressSanitizer.

| Target | Input | Why | Sanitiser |
|---|---|---|---|
| `sessions_jsonl` | A JSONL document with controlled multi-byte characters at cut offsets, driven into the session-analyzer connectors (codex, opencode, claude) | This is the family that produced every confirmed byte-slicing panic | none needed |
| `tools_toml` | `tools.toml` and thesaurus content into the Aho-Corasick matcher, including empty and whitespace-only patterns | An empty pattern yields zero-width matches at non-char boundaries and panics | none needed |
| `yaml_frontmatter` | YAML frontmatter through serde_yaml 0.9 into `unsafe-libyaml` | The largest attacker-reachable `unsafe` parser surface in the tree, a C-to-Rust transpiled parser. Miri's Stacked Borrows axis found nothing over a 43-input hostile corpus, but that is a sample, not a proof, and the crate is unmaintained. Migration is tracked as #317 | **ASan** |
| `update_manifest` | Release-manifest JSON and version strings into the update path, including the semver parse that happens before the signature gate | semver has inline storage `unsafe`, and the parse precedes verification. Miri found nothing over 308 crafted versions, so the residual issue is the ordering rather than the parser; fixing the ordering is tracked as #318 | **ASan** |

Preserve the corpora in the repository and keep the crashers as regression inputs. Six hours per target is the budget the audit's soak design allocated.

### The ASan admissibility procedure

**Any experiment whose signal is an ASan report must first prove ASan instrumented the C under test.** A run that did not instrument is inadmissible, not negative. This audit had four such experiments and the check caught a real problem in three of them, so treat it as a step, not a formality.

1. **Build with the sanitiser reaching both languages.** `RUSTFLAGS=-Zsanitizer=address` and `-Zbuild-std` for the Rust side, `CFLAGS=-fsanitize=address -fno-omit-frame-pointer` for the vendored C. Without `-Zbuild-std` the standard library is uninstrumented and the coverage is partial.
2. **Find the real object path, then `nm` it.** The naive glob `build/<lib>-sys-*/out/*.o` **is wrong and silently prints zero**; it is what the bigbox harnesses used and it made their own inline checks report no instrumentation on builds that were in fact instrumented. The real shape is:

   ```
   <target>/<triple>/release/build/<lib>-sys/<hash>/out/[<hash>-]<name>.o
   ```

   `nm` the object that actually carries the code under test, not an arbitrary one, and require a non-zero `__asan_` count with load and store interceptors present:

   ```
   nm <that object> | rg -c __asan_
   ```

3. **Know which object to name.** Reference values measured in this audit, useful as a sanity check on your own build:

   | Library | Object under test | `__asan_` symbols |
   |---|---|---|
   | libgit2 1.9.7 (discover and status) | `repository.o` | 25 |
   | libgit2 1.9.7 (config parsing) | `config_file.o` | 19 |
   | SQLite 3.46.0 | `sqlite3.o` | 30 |
   | LMDB 0.9.70 | `mdb.o` | 28 |

   libgit2 builds 206 objects carrying 2,517 `__asan_` symbols in total; a total that large with a zero count on the specific object means you named the wrong file.
4. **Suppress the one known false positive.** Anything that links libgit2 produces an ODR violation report at startup from its bundled llhttp. Run with `ASAN_OPTIONS=detect_odr_violation=0:abort_on_error=1`. Do not suppress anything else; every other report is real until proven otherwise.
5. **Record the counts in the log next to the verdict.** A clean ASan result whose admissibility is not recorded cannot be re-read later, which is how three of this audit's results nearly became unusable.

### The MCP soak recipe, and the four traps that void a run

The EXP-302 soak drove eight concurrent SSE clients through the shipped server. Four details decide whether such a run means anything, and three of them fail silently rather than erroring.

- **The tool is named `terraphim_grep`.** The older name `grep_files` returns `-32601` on every call, the grep path never executes, and the soak looks like a clean pass. A run using the wrong name is inadmissible.
- **The rmcp 0.9 client needs the `transport-sse-client-reqwest` feature** for `SseClientTransport::start`.
- **Put the needle at the tail of each corpus file**, so the grep reads the whole mapping rather than returning from the first page.
- **Give every `call_tool` a client-side timeout.** Without one a stalled SSE response hangs the driver and hides the result you are measuring.

---

## 5. The SAFETY comment template, and the rule this audit established

Under `unsafe_code = "forbid"` there is normally nothing to document. A SAFETY comment is needed only where one target deliberately drops `[lints] workspace = true`, which also means updating the `ci_guards` manifest test to allow that exception by name. Treat that as a reviewed decision, not a local one.

```rust
// SAFETY: <the precondition, stated as something a reader can check>.
// Upheld here by: <the mechanism in THIS file that makes it true>.
// Broken by: <the change that would silently invalidate it>.
// Verified by: <the test or experiment that fails if it is broken>.
```

**The rule this audit established: a SAFETY comment that prescribes `#[serial]` for environment mutation is wrong.** It reads plausibly and it does not work. `serial_test` excludes only other tests that are themselves annotated `#[serial]`. It says nothing about unannotated siblings in the same binary, and libtest runs those on other threads at default parallelism. EXP-014 measured it: across five runs at four test threads, all three unannotated siblings observed the annotated writer's environment guard live, three overlaps every time. A future test added without the annotation reintroduces the race with no diff to the SAFETY comment and no compiler complaint.

The only sound arguments for mutating the process environment are:

- **No other thread exists yet.** Before any thread is spawned, in `main`, before a runtime is built.
- **The binary holds exactly one test**, ideally `harness = false`. This one is fragile: the invariant is unguarded unless a test counts the tests, and one of the audit's findings survives only on this basis.
- **The variable is for a child process.** Then it is not an argument at all: use `Command::env` on the child and do not touch the parent's environment.

`#[serial]` is never one of them.

---

## 6. The env-probe instrument

This is the only instrument that decided the environment races, because ThreadSanitizer cannot. The Rust-shipped TSan runtime has no interceptors for `getenv`, `setenv`, `unsetenv`, `putenv` or `clearenv` on either macOS or Linux, verified by symbol count on both, so both sides of the access pair execute in uninstrumented libc where TSan cannot see them. Instrumenting the bundled C would not help either: it instruments the call to `getenv`, not the reads of `environ` inside libc.

**Sources.** All under `$WS`:

| File | Role |
|---|---|
| `phase3_raw/envprobe/linux/envprobe_linux.c` | the Linux `LD_PRELOAD` shim, 87 lines, wraps `setenv`, `unsetenv` and `getenv` through `dlsym(RTLD_NEXT, ...)` |
| `phase3_raw/envprobe/linux/run_probe.sh` | the iteration driver |
| `phase3_raw/envprobe/linux/symbolise2.py` | resolves overlap backtraces with `addr2line` |
| `phase3_raw/envprobe/linux/scratch_main.rs` and its `Cargo.toml` | the control binary: it exercises the environment guard plus a SQLite connection on sibling threads, and its job is to prove the shim can still see a known reader on this host, so a clean production result means "no writer" rather than "blind instrument" |
| `phase3_raw/envprobe/envprobe.c` | the macOS `DYLD_INSERT_LIBRARIES` variant, kept for completeness; macOS cannot produce the memory-unsafe signal because Apple Libc serialises both sides |

**Before anything else, edit the driver once.** As archived, `run_probe.sh` hard-codes the shim path on line 4 and the log directory on line 5. Change them to `SO=${ENVPROBE_SO:-/tmp/ubx-probe/libenvprobe.so}` and `L=${ENVPROBE_LOGDIR:-/tmp/ubx-probe-logs}`, then store that edited copy as the permanent instrument. Without this edit the commands below load no shim at all and report a silent, meaningless "clean".

**Build and run**, from the project root, on a glibc host:

```
mkdir -p /tmp/ubx-probe /tmp/ubx-probe-logs
gcc -shared -fPIC -O2 -o /tmp/ubx-probe/libenvprobe.so \
    $WS/phase3_raw/envprobe/linux/envprobe_linux.c -ldl

# Locate the libtest binary; do not glob target/debug/deps by mtime.
cargo +nightly test -p terraphim_grep --lib --no-run --message-format=json 2>/dev/null \
  | rg -o '"executable":"[^"]+"' | cut -d'"' -f4 > /tmp/bin.txt

ENVPROBE_SO=/tmp/ubx-probe/libenvprobe.so ENVPROBE_LOGDIR=/tmp/ubx-probe-logs \
  bash $WS/phase3_raw/envprobe/linux/run_probe.sh grep "$(cat /tmp/bin.txt)" crates/terraphim_grep 100
```

Run the driver under `bash`, as written above, not under the shell tmux gives you: the same `PIPESTATUS` trap that spoiled two Miri runs in this audit applies to any harness that captures a pipeline's status, and zsh is the login shell here. The compile line is recovered from the shim's own source, not from a recorded build; confirm it links on your host before trusting a result. The last argument of the driver is a working directory it changes into, so the command must run from the project root for `crates/terraphim_grep` to resolve. The driver loops the binary over three widenings (0, 500 and 2000 microseconds) and prints per-widening counts of `pre` overlaps, `call` overlaps and passing runs.

**Environment knobs.** `ENVPROBE_WIDEN_US` makes the writer sleep inside its region before the real call; `ENVPROBE_LOG` sets the log path, defaulting to stderr; `ENVPROBE_TRACE=1` logs every call, which is how you find out which thread consumed a once-per-process reader.

**Reading the output.** A `call` overlap means the reader executed while the real glibc `setenv` was running. A `pre` overlap means it arrived during the artificial sleep. **Only `call` overlaps at widening 0 are unassisted evidence.** Widening enlarges a window the schedule already permits; it cannot create one that does not exist, so widened counts show the threads *can* be concurrent, not that they *were*.

**Two constraints that decide whether the run means anything.**

- **Drive the libtest binary directly, never through nextest.** nextest gives one process per test, which destroys a cross-test race by construction, so a nextest run passes trivially and proves nothing. Any regression test built on this instrument must execute the binary itself.
- **Run it on glibc.** On macOS, Apple Libc serialises `getenv` and `setenv` behind `environ_lock_np()`, so the pair is a contract violation but not memory-unsafe, and the probe will not show the signal you are looking for. bigbox (Ubuntu, glibc 2.39) is the reference host.

**What the shim cannot see.** It intercepts PLT `getenv` only. Calls the C library makes to its own internal symbol are invisible to it, and that includes the glibc resolver reading `RES_OPTIONS` and `LOCALDOMAIN` inside `getaddrinfo`. A zero-overlap result against a predicted resolver reader is "could not look", not "looked and it was clean". Seeing that reader needs helgrind or a libc-level `getenv` uprobe; valgrind is not currently installed on bigbox.

**Which lanes are exposed.** The two CI test lanes run `cargo nextest`, one process per test, and are **not** exposed to in-process environment races. Both coverage lanes run plain `cargo llvm-cov --workspace --lib` in-process on glibc with `--ignore-run-fail`, so they **are** exposed and a crash there would not even turn the lane red. The canonical `cargo test --workspace --no-fail-fast` line in BUILD.md is exposed too. Note this asymmetry when you read a green pipeline: the lanes that could show the race are the ones that hide their own failures.

---

## 7. If you change X, re-run experiment Y

Every EXP block named here is in `$WS/UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md`, with its hypothesis, its falsifiability clause and a single-command invocation.

| If you change this | Re-run this | Because |
|---|---|---|
| Any `FilePicker::new` call site, or add a fifth one | **EXP-805** and the `ci_guards` arm `no_fff_picker_maps_files` | There are exactly four sites: `crates/terraphim_grep/src/hybrid_searcher.rs:354` and `crates/terraphim_mcp_server/src/lib.rs:1274`, `:1372`, `:1485`. Each must carry the explicit zero-file content-cache budget, or the process starts memory-mapping files that other processes can rewrite underneath it |
| Any `FilePicker::new` site, again | **EXP-806** and the guard-call arm | Each site must also sit behind `reject_hostile_tree`. A new site added without it reopens the invalid-`str` path. Assert `follow_symlinks` explicitly rather than inheriting the upstream default |
| Upgrade `fff-search` past 0.8.4 | **Do not.** See the hard rule below this table. If it is ever done anyway: **EXP-104, EXP-201, EXP-301**, then **EXP-805 and EXP-806**, with files above 256 KiB in every corpus | The version is pinned `=0.8.4` and the pin is asserted in `ci_guards`. Both confirmed production-path findings live in this crate, and 0.10.6 makes one of them worse while appearing to leave the fix intact |
| Add any `std::env::set_var` or `remove_var` anywhere | **EXP-801, EXP-802, EXP-803** for the binary you touched, and **EXP-804** for the lint | Under `forbid(unsafe_code)` this should be a compile error. If it compiled, the lint is not reaching that target, which is itself the finding. The probe's acceptance criterion is `setenv=0` in the main process |
| Add a new test binary that reads the environment | **EXP-801 to EXP-803** pattern against the new binary, and check the `#[serial]` count | A reader added to a binary that already has a writer recreates the confirmed race with no diff to any SAFETY comment. Prefer injecting the value over reading the environment at all |
| Upgrade `crossbeam-epoch` | **EXP-101** (the Stacked Borrows axis over the rayon tests) | The container-of report is expected to persist; the point is to confirm no workspace frame has appeared and that Tree Borrows is still clean |
| Upgrade `rayon` | the Miri matrix, all four axes | The leak-check behaviour at exit and the set of tests each axis can complete both depend on the pool implementation |
| Upgrade `serde_yaml` (or migrate off it: 0.9 is deprecated, and #317 tracks the migration) | **EXP-202** and the `yaml_frontmatter` fuzz target | The finding is the transpiled C parser behind it. EXP-202 closed NO_EVIDENCE on Stacked Borrows over the hostile corpus, so this is hygiene rather than a live defect, but a migration to a maintained pure-Rust parser closes the row outright and is the better outcome |
| Upgrade `semver` | **EXP-203** and the `update_manifest` fuzz target | The finding is the inline-storage `unsafe`, reached by an attacker-influenced version string parsed **before** the signature gate. EXP-203 closed NO_EVIDENCE for undefined behaviour over 308 crafted versions, so the live defect is the ordering, not the parser. Fixing the ordering, verify then parse (#318), is worth more than any re-run |
| Upgrade `rusqlite` or `libsqlite3-sys` | **EXP-206** and **EXP-002c** | EXP-206 is the corrupted-database corpus. EXP-002c is the environment-race mechanism against SQLite's one-shot `getenv` pair, whose position in start-up can move with the version |
| Upgrade `libgit2-sys` | **EXP-205** | The hostile-repository corpus, which closed NO_EVIDENCE against libgit2 1.9.7 with ASan admissibility proven. HEAD is at 0.18.8+1.9.7; a downgrade or a bundled-source change reopens it. Note this crate is not what the git2 advisories are about; see section 8 |
| Upgrade `self_update` | **EXP-102** and **EXP-105** | Its `set_ssl_vars!` macro calls `set_var` from library code on Linux. Check whether the upgrade gated or removed it; if not, the ignored live-network test and the upstream issue both still stand |
| Change the CI runner, the container image or the libc | the whole env-probe set | Every environment-race verdict is glibc-specific. On a musl or macOS runner the signal changes character entirely |
| Switch a coverage lane to nextest | note it here and in BUILD.md | It closes the in-process exposure, including for dependency writers the lint cannot see. It does not close any finding |

---

### The hard rule: `fff-search` stays at `=0.8.4`

This is the single most dangerous silent regression the audit found, and it is dangerous precisely because a careful reviewer would miss it.

**What the mitigation depends on.** `fff-search` 0.8.4 contains exactly one `memmap2::Mmap::map` call, at `src/types.rs:640` inside `get_cached_content`, and it sits behind the `max_files` and `max_bytes` budget checks. That is the only reason a zero-file `ContentCacheBudget` is a complete fix: with `max_files: 0` the budget guard fires on the first file, before the mapping. The one other caller, `warmup_mmaps`, is dead in 0.8.4 because its sole call site is commented out upstream.

**What 0.10.6 changes.** It adds a **second** mapping site at `src/types.rs:762`, inside `get_content_for_search`, gated only by `FRESH_MMAP_THRESHOLD` and `max_file_size`. The budget cannot reach it. Worse, it is entered exactly when `get_cached_content` returns `None`, which is what a zero-file budget makes it do. So after the upgrade the mitigation would send every file at or above `FRESH_MMAP_THRESHOLD`, which is 256 KiB on Linux and 1 MiB on macOS, straight into an unbudgeted mapping. **The fix would still be in the source, still read correctly in review, and do nothing.** The upstream SAFETY comment still rests on the same unsatisfiable precondition, that a watcher invalidates on modification, for a picker built by `FilePicker::new`, which installs no watcher.

**The UF-040 guard is needed at every version.** `read_to_buf` in 0.10.6 is functionally identical to 0.8.4: it copies `byte_len.min(buf.len())` bytes and returns `core::str::from_utf8_unchecked`. `PATH_BUF_SIZE` is still 4096, and the lossy path storage is unchanged. No upgrade removes the need for `reject_hostile_tree`.

**Therefore:**

- The version is pinned `=0.8.4` in both dependent crates and `ci_guards` asserts the pin. Do not relax either.
- A future upgrade requires an upstream fix that gates **both** map sites. There is no configuration workaround: the only budget field reaching the fresh path is `max_file_size`, and setting it low enough to suppress the mapping also refuses those files to grep.
- `enable_mmap_cache` gates neither site, at either version. Do not reach for it.
- Raising the pin also lapses the reachability argument that closes the git2 advisories in section 8, so the two decisions are one decision.

---

## 8. Dependency hygiene

`deny.toml` at the project root is the gate. Its policy: advisories and yanked crates block the pipeline, unmaintained and notice-level advisories warn, and every `ignore` entry carries its reason and the issue that owns its removal. **`yanked = "deny"` is restored** after the two lockfile bumps of 2026-09-12.

Those two bumps cleared much of what the audit found, though not as much as the audit first claimed. The first took crossbeam-epoch to 0.9.21, anyhow to 1.0.104, event-listener to 5.4.2, lru to 0.18.4 and memmap2 to 0.9.11, cleared the h2 advisory, and removed a yanked spin 0.9.8. The second was lockfile-only and took libgit2-sys to 0.18.8+1.9.7, a patch-level move of the bundled C library from libgit2 1.9.4 to 1.9.7. It is harmless and it is kept, but **it did not clear RUSTSEC-2026-0183 or RUSTSEC-2026-0184**, which the audit's own commit message, `deny.toml` comment and a Gitea comment all said it did. That claim is retracted in commit e90b193 and in section 4 of `FINAL_UB_REPORT.md`.

### The rule that failure produced: a green `cargo deny check` is not an advisory clearance

Both git2 advisories are `informational = "unsound"`, and this `deny.toml` treats unsound-class advisories as informational rather than denials. The gate was therefore never going to fail on them, and its passing was mistaken for their absence. **Unsound-class advisories are invisible to this gate by configuration.** Check them with `cargo audit`, which still reports both at HEAD, and never infer an advisory's disappearance from a green pipeline.

### Why the git2 pair is nonetheless acceptable, and the condition on that

Both advisories are API-specific. RUSTSEC-2026-0183 needs a call to `Remote::list()`; RUSTSEC-2026-0184 needs a `BlameHunk` obtained from `Blame::blame_buffer()`. The complete set of git2 items `fff-search` 0.8.4 references is `git2::Repository`, `git2::Error` and eight `git2::Status` flags. `Remote`, `Blame` and `blame_buffer` appear nowhere in its sources, and no workspace crate depends on git2 directly, so `fff-search` is the only consumer and neither vulnerable path is reachable at any input. The advisories are patched in git2 0.21.0 or later, first reachable through `fff-search` 0.10.6, which is the version that breaks the UF-039 mitigation, so the workspace stays where it is.

**This is a property of the source as read on 2026-09-12, not a guarantee.** It lapses if the pin is raised or if `fff-search` grows a remote or blame feature. The `ci_guards` pin assertion is what stops that happening silently, and it is why the pin is not cosmetic. Record the enumerated git2 surface above in `deny.toml` as a comment so no future reader has to re-derive it; it does not need a new ignore entry, because unsound-class advisories are already informational here.

What remains ignored, and why each entry is there:

| Ignored | Reason it cannot be cleared locally | Removal condition |
|---|---|---|
| RUSTSEC-2026-0195 and RUSTSEC-2026-0194 (quick-xml) | Two lockfile copies, 0.37 through `self_update` and 0.38 through `opendal`, both below the fixed 0.41. Neither is a direct dependency | Upstream releases in `self_update` and `opendal` |
| RUSTSEC-2026-0189 (rmcp 0.9.1) | Reaching 1.4.0 or later is a major upgrade of the MCP server's transport. The DNS-rebinding issue affects the streamable HTTP server only | The rmcp major upgrade, tracked as a child of the dependency issue |
| RUSTSEC-2025-0141 (bincode 1.x) | Unmaintained notice, transitive, no maintained drop-in | A maintained replacement appears upstream |
| RUSTSEC-2025-0119 (number_prefix) | Unmaintained notice, arrives through indicatif | indicatif drops it |
| RUSTSEC-2024-0436 (paste) | Unmaintained notice, proc macro, compile-time only | A transitive dependency drops it |
| RUSTSEC-2024-0370 (proc-macro-error) | Unmaintained notice, compile-time only | Same |

One licence exception is recorded separately: html2md is GPL-3.0-or-later and reaches two Apache-2.0 crates through a registry dependency. That is an upstream licence-compatibility problem, not a local one, and the exception should be removed when the upstream crate drops html2md.

Two bans are active: `openssl-sys` (workspace policy is rustls everywhere, and native TLS breaks the static release matrix), and `terraphim_test_utils` should be added by the lint commit, because it is a safe wrapper over `std::env::set_var` whose documented safety rule cannot be satisfied.

**The standing discipline.** Every ignore entry is temporary by construction: it names its removal condition and it is removed as soon as that condition is met. When the audit's own deferred rows were re-read at the end, two of them (the unsound `Send`/`Sync` advisory and the four other UB-class advisories) had already had their conditions met by the bumps above and nobody had closed them. Re-read the ignore list whenever `Cargo.lock` changes, not only when `cargo deny` complains.
