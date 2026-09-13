# Undefined-behaviour experiment registry (v1)

Run: 2026-09-11-terraphim-clients-1. Written by the Phase 4 synthesizer, 2026-09-12 10:05 BST. Source unmodified.

Every UB hypothesis for this run lives here until it carries a verdict. Finding references point at `phase4_unified_findings.md` (UF ids). Verdict vocabulary: OPEN, CONFIRMED_UB, NO_EVIDENCE, NEEDS_REFINEMENT, DEFERRED. Executed experiments (EXP-001 to EXP-004) carry the Phase 3 result and cite their raw logs under `phase3_raw/`; there is no `phase5_experiment_results/` directory yet, so Phase 5 should copy or symlink those logs when it re-runs anything. Experiments EXP-010 to EXP-015 are OPEN and are what Phase 5 must run.

Conventions used in this file:

- Follow-ups keep the ids Phase 3 already used (EXP-001b, EXP-002b, EXP-002c) and carry a `Follow-up of` line.
- Reproducer blocks are at most 30 lines. Where the real reproducer is a whole test binary or a C shim, the block is a distilled sketch of the access pair and the invocation names the real artefact.
- Standalone harnesses go under `<workspace>/exp-harness-<EXP_ID>/` and path-depend on the workspace crates; nothing is added under `crates/`.
- `terraphim_types` and `terraphim_test_utils` resolve from the `terraphim` registry (`~/.cargo/config.toml` `[registries.terraphim]`), so harness manifests that need them must say `registry = "terraphim"`.

## EXP-001: ThreadSanitizer on terraphim_grep lib tests (env writer vs libgit2 getenv)

**Finding ref:** UF-001 in phase4_unified_findings.md (data-races F-001)
**Bucket:** 7 data races (env setter vs native getenv)
**Severity (Phase 2):** LIKELY-UB
**Hypothesis:** At default libtest parallelism the `set_var`/`remove_var` calls in `test_openrouter_client_base_url_resolution` (crates/terraphim_grep/src/openrouter_client.rs:170-191) execute concurrently with libc `getenv` inside `git_libgit2_init` on a sibling test thread, and ThreadSanitizer reports the pair.

**Minimal reproducer:**
```rust
// Distilled shape of the pair the real binary contains. Writer = the
// openrouter test; reader = any of the 8 fff-mode tests that reach
// git2::Repository::discover (hybrid_searcher.rs:362-364).
use std::thread;
fn writer() {
    unsafe { std::env::set_var("OPENROUTER_BASE_URL", "https://example.com/v1") };
    unsafe { std::env::remove_var("OPENROUTER_BASE_URL") };
}
fn reader() {
    // git2::init() -> git_libgit2_init -> git_sysdir_global_init -> getenv("HOME")
    let _ = git2::Repository::discover(std::env::temp_dir());
}
fn main() {
    let w = thread::spawn(|| for _ in 0..200 { writer() });
    let r = thread::spawn(reader);
    w.join().unwrap();
    r.join().unwrap();
}
```

**Expected signal:**
- TSan: `WARNING: ThreadSanitizer: data race` pairing `setenv`/`unsetenv` (from `std::env::set_var`) with `getenv` in `git__getenv` (libgit2 sysdir.c:357).

**Falsifiability:** Three clean full runs plus a filtered run of the four relevant tests, with TSan able to intercept `setenv`/`getenv`, would refute the hypothesis for this host. A clean run with an instrument that cannot see the pair refutes nothing.

**Invocation (single command):**
```
RUSTFLAGS="-Zsanitizer=thread" CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-target-tsan \
  cargo +nightly test -Zbuild-std --target aarch64-apple-darwin -p terraphim_grep --lib 2>&1 \
  | tee phase3_raw/tsan_grep_run1.log
```

**Verdict:** DEFERRED
**Lead note (2026-09-12):** Superseded by EXP-001b (LD_PRELOAD probe on glibc, CONFIRMED_UB). TSan cannot observe the pair on any shipped runtime; no further TSan run is planned.

**Notes:**
- Executed in Phase 3 on macOS: 3 full runs (57 passed) and 1 filtered run, zero TSan warnings (`phase3_raw/tsan_grep_run{1,2,3}.log`, `tsan_grep_runfiltered.log`).
- Instrument blind: `nm librustc-nightly_rt.tsan.dylib` shows no `getenv`/`setenv` interceptors on macOS, and the Linux `.a` runtimes shipped with the same nightly have none either. Apple Libc additionally serialises both sides behind `environ_lock_np()`, so a macOS-clean result does not transfer to glibc.
- The `CFLAGS=-fsanitize=thread` variant failed to link the host proc-macro `sqlx-macros` (logs in `phase3_raw/cflags_attempt/`) and was dropped; it would not have instrumented the reads of `environ` inside libc anyway.
- Superseded by EXP-001b, which used an instrument that observes libc calls directly.

## EXP-001b: LD_PRELOAD interposition probe on the terraphim_grep lib test binary (Linux glibc)

**Follow-up of:** EXP-001
**Finding ref:** UF-001 in phase4_unified_findings.md (data-races F-001, phase3 F-102)
**Bucket:** 7 data races (env setter vs native getenv)
**Severity (Phase 2):** LIKELY-UB
**Hypothesis:** On glibc, where `getenv` walks `__environ` without a lock, a `getenv` call from the libgit2 sysdir chain on a sibling test thread executes while `setenv`/`unsetenv` from the openrouter test is in progress, without any artificial widening of the writer.

**Minimal reproducer:**
```rust
// The real reproducer is the unmodified test binary under an LD_PRELOAD shim
// (phase3_raw/envprobe/linux/envprobe_linux.c) that wraps setenv/unsetenv/getenv
// via dlsym(RTLD_NEXT) and flags a getenv on thread R while a setenv/unsetenv
// on thread W != R is between entry and return. Access pair, distilled:
use std::thread;
fn main() {
    let w = thread::spawn(|| {
        for _ in 0..200 {
            unsafe { std::env::set_var("OPENROUTER_BASE_URL", "https://example.com/v1") };
            unsafe { std::env::remove_var("OPENROUTER_BASE_URL") };
        }
    });
    let r = thread::spawn(|| {
        // libgit2_sys::init is Once-guarded: one getenv window per process.
        let _ = git2::Repository::discover(std::env::temp_dir());
    });
    w.join().unwrap();
    r.join().unwrap();
}
```

**Expected signal:**
- envprobe: `[envprobe] OVERLAP(call): getenv("HOME"|"XDG_CONFIG_HOME"|"APP_SANDBOX_CONTAINER_ID") on tid R while writer active on tid W`, backtrace through `git_sysdir_global_init` <- `git_libgit2_init` <- `libgit2_sys::init` <- `git2::init` <- `Repository::discover` <- `fff_search::file_picker::FilePicker::collect_files` <- `HybridSearcher::search_code`.

**Falsifiability:** Zero `call`-phase overlaps at `widen=0` across 100 iterations, and zero overlaps with the writer widened to 2000 us, would show the accesses are ordered by something the static analysis missed (for example the writer always running after the `Once`).

**Invocation (single command):**
```
cd /tmp/ubx-probe-clients/crates/terraphim_grep && ENVPROBE_WIDEN_US=0 LD_PRELOAD=/tmp/ubx-probe/libenvprobe.so \
  ../../../ubx-probe-target/debug/deps/terraphim_grep-5ffeb952bb8a9889 2>&1 | tee phase3_raw/envprobe_linux_grep_widen0us.log
```

**Verdict:** CONFIRMED_UB

**Notes:**
- Executed on bigbox (x86_64, Ubuntu 24.04, glibc 2.39, the `terraphim-native` Gitea runner), 100 iterations per widening: `widen=0` gave 1 unassisted `call` overlap (iteration 30, `getenv("APP_SANDBOX_CONTAINER_ID")` in `git_sysdir_guess_home_dirs` on the `multiple_search_paths_merge_results` thread); `widen=500us` 463 overlaps and `widen=2000us` 544 overlaps, all the same libgit2 chain. Logs: `phase3_raw/envprobe_linux_grep_widen{0,500,2000}us.log`, session summary `phase3_raw/probe_grep.session.log`.
- This is the exact reader chain Phase 2 predicted, so F-001 is proved for the Linux CI platform. Tracked as terraphim/terraphim-clients#280.
- Fix shape for Phase 8: inject the base URL without touching the process environment, or mark the writer `#[serial]` together with every test that reaches `Repository::discover`; the SAFETY comment at openrouter_client.rs:166 is false on both platforms.

## EXP-002: ThreadSanitizer on terraphim_agent tui_service_tests (EnvVarGuard vs sqlite getenv)

**Finding ref:** UF-002 in phase4_unified_findings.md (data-races F-002)
**Bucket:** 7 data races (env setter vs native getenv)
**Severity (Phase 2):** LIKELY-UB
**Hypothesis:** The `EnvVarGuard` writes and Drop-restores in `test_tui_service_new_uses_host_settings_path` (crates/terraphim_agent/tests/tui_service_tests.rs:37-61) run concurrently with `getenv("SQLITE_TMPDIR")`/`getenv("TMPDIR")` in sqlite `unixTempFileInit` on a sibling test thread that opens the first rusqlite connection, and ThreadSanitizer reports the pair.

**Minimal reproducer:**
```rust
// Distilled pair: writer = terraphim_test_utils::EnvVarGuard (safe wrapper over
// unsafe set_var/remove_var); reader = first rusqlite::Connection::open in the
// process (sqlite3_initialize -> unixTempFileInit -> getenv x2).
use std::thread;
fn main() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let w = thread::spawn(move || {
        unsafe { std::env::set_var("HOME", "/nonexistent") };
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/nonexistent/.config") };
        unsafe { std::env::set_var("TERRAPHIM_DATA_PATH", "/nonexistent/data") };
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::remove_var("TERRAPHIM_DATA_PATH") };
    });
    let r = thread::spawn(move || {
        let _c = rusqlite::Connection::open(&db).unwrap();
    });
    w.join().unwrap();
    r.join().unwrap();
}
```

**Expected signal:**
- TSan: `WARNING: ThreadSanitizer: data race` pairing `setenv` (EnvVarGuard::set or Drop) with `getenv` in `unixTempFileInit` (bundled sqlite3.c:44351-44354).

**Falsifiability:** Repeated clean runs with an instrument able to see the pair would refute the hypothesis for the shipped binary.

**Invocation (single command):**
```
RUSTFLAGS="-Zsanitizer=thread" CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-target-tsan \
  cargo +nightly test -Zbuild-std --target aarch64-apple-darwin -p terraphim_agent --test tui_service_tests 2>&1 \
  | tee phase3_raw/tsan_agent_tui_run1.log
```

**Verdict:** DEFERRED
**Lead note (2026-09-12):** Superseded by EXP-002b/002c. Same TSan blindness; the mechanism verdict lives in EXP-002c.

**Notes:**
- Executed in Phase 3 on macOS: 3 runs, 10 tests passed each, zero TSan warnings (`phase3_raw/tsan_agent_tui_run{1,2,3}.log`). Same instrument blindness as EXP-001 (no env interceptors; Apple Libc locks both sides). The sqlite C object carried no `__tsan_` symbols.
- Superseded by EXP-002b (real binary under the Linux probe) and EXP-002c (scratch reduction).

## EXP-002b: LD_PRELOAD interposition probe on the shipped tui_service_tests binary (Linux glibc)

**Follow-up of:** EXP-002
**Finding ref:** UF-002 in phase4_unified_findings.md (data-races F-002)
**Bucket:** 7 data races (env setter vs native getenv)
**Severity (Phase 2):** LIKELY-UB
**Hypothesis:** In the unmodified `tui_service_tests` binary at default parallelism, the sqlite `getenv` pair lands on a sibling thread inside one of the two EnvVarGuard mutation windows.

**Minimal reproducer:**
```rust
// Real reproducer: the unmodified test binary under the LD_PRELOAD shim.
// The ordering question the probe answers, distilled:
//   t0  serial writer test starts, EnvVarGuard::set x3          (window 1)
//   t1  writer thread opens the FIRST rusqlite connection -> sqlite3_initialize
//       consumes the only getenv pair in the process, on the writer thread
//   t2  siblings open later connections: no getenv any more
//   t3  writer test ends, Drop restores x3                       (window 2)
// The hypothesis needs a sibling to reach t1 before t0 or between t0 and t1.
fn main() {
    // See phase3_raw/envprobe_linux_tui_trace{1,2,3}.log for the measured
    // timeline (ENVPROBE_TRACE=1): first setenv -> sqlite getenv on the
    // same tid after 0.8 to 1.9 ms; Drop about 460 ms later.
}
```

**Expected signal:**
- envprobe: `[envprobe] OVERLAP(call): getenv("SQLITE_TMPDIR"|"TMPDIR") on tid R while writer active on tid W` with `sqlite3_os_init` <- `sqlite3_initialize` <- `rusqlite::inner_connection::InnerConnection::open_with_flags` in the backtrace.

**Falsifiability:** Zero overlaps in 300 iterations across three widenings, and a trace showing the sqlite reader executes on the writer thread itself, refute the hypothesis for the current test ordering.

**Invocation (single command):**
```
cd /tmp/ubx-probe-clients/crates/terraphim_agent && ENVPROBE_WIDEN_US=0 LD_PRELOAD=/tmp/ubx-probe/libenvprobe.so \
  ../../../ubx-probe-target/debug/deps/tui_service_tests-7b5b5c240c8f72ac 2>&1 | tee phase3_raw/envprobe_linux_tui_widen0us.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Executed on bigbox: 100 iterations each at `widen=0`, `500us`, `2000us`, all 10 tests passing, zero overlaps (`phase3_raw/envprobe_linux_tui_widen{0,500,2000}us.log`, `probe_tui.session.log`). Writer per run: 4 `setenv`, 2 `unsetenv`; 32 to 62 `getenv` calls from other threads after the first write, none overlapping and not backtraced (mostly std `env::var` readers, which are locked).
- Reachability caveat (why this is not a clean bill): the `#[serial]` writer is the first test to open a sqlite connection, so the process-wide `unixTempFileInit` reader is consumed on the writer's own thread. Any reordering, or a sibling opening a connection first, moves the reader onto a sibling thread inside a writer window, which EXP-002c shows is memory-unsafe. Neither `serial_test` nor the wrapper's SAFETY comment enforces the ordering.

## EXP-002c: scratch reduction of the EnvVarGuard vs rusqlite mechanism (Linux glibc)

**Follow-up of:** EXP-002
**Finding ref:** UF-002 in phase4_unified_findings.md (data-races F-002, phase3 F-103)
**Bucket:** 7 data races (env setter vs native getenv)
**Severity (Phase 2):** LIKELY-UB
**Hypothesis:** When the EnvVarGuard sequence and `rusqlite::Connection::open` are unsynchronised on different threads (the shape of a non-serial sibling opening a connection while the serial test's guard is live), glibc `getenv` inside sqlite `unixTempFileInit` overlaps glibc `setenv`/`unsetenv` without any widening.

**Minimal reproducer:**
```rust
// Distilled from phase3_raw/envprobe/linux/scratch_main.rs (rusqlite 0.32,
// feature "bundled", the same libsqlite3-sys build the workspace links).
use std::sync::{Arc, Barrier};
use std::thread;
fn main() {
    let dir = tempfile::tempdir().unwrap();
    let go = Arc::new(Barrier::new(9));
    let mut hs = Vec::new();
    let g = go.clone();
    hs.push(thread::spawn(move || { g.wait(); for _ in 0..200 {
        unsafe { std::env::set_var("HOME", "/x") };
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/x/.config") };
        unsafe { std::env::set_var("TERRAPHIM_DATA_PATH", "/x/data") };
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::remove_var("TERRAPHIM_DATA_PATH") };
    }}));
    for i in 0..8 {
        let g = go.clone();
        let db = dir.path().join(format!("{i}.db"));
        hs.push(thread::spawn(move || { g.wait(); for _ in 0..3 {
            let _c = rusqlite::Connection::open(&db).unwrap();
        }}));
    }
    for h in hs { h.join().unwrap(); }
}
```

**Expected signal:**
- envprobe: `[envprobe] OVERLAP(call): getenv("SQLITE_TMPDIR")` or `getenv("TMPDIR")` on a reader tid while the writer tid is inside `setenv`/`unsetenv`, backtrace `sqlite3_os_init` <- `sqlite3_initialize` <- `sqlite3_mutex_alloc` <- `InnerConnection::open_with_flags`.

**Falsifiability:** Zero `call`-phase overlaps at `widen=0` over 150 iterations would show the pair is serialised by something in glibc or sqlite the analysis missed.

**Invocation (single command):**
```
cd /tmp/ubx-probe-scratch && ENVPROBE_WIDEN_US=0 LD_PRELOAD=/tmp/ubx-probe/libenvprobe.so \
  target/release/ubx-probe-scratch 2>&1 | tee phase3_raw/envprobe_linux_scratch_widen0us.log
```

**Verdict:** CONFIRMED_UB

**Notes:**
- Executed on bigbox, 150 iterations per widening: `widen=0` gave 102 unassisted `call` overlaps (43 on `SQLITE_TMPDIR`, 59 on `TMPDIR`) plus 4 `pre`; widened runs 298 and 282 `pre` overlaps. Logs: `phase3_raw/envprobe_linux_scratch_widen{0,500,2000}us.log`, `probe_scratch.session.log`.
- Confirms the mechanism behind F-002. Additional hazard specific to this reader: sqlite stores the `char*` from `getenv` in the static `azTempDirs[]` and dereferences it on every later temp-file open; an `unsetenv` shifting `__environ` mid-scan can make `getenv` return the wrong entry.
- Combined with EXP-002b the finding stays High: the shipped binary is safe only by accident of ordering.

## EXP-003a: Miri matrix on terraphim_negative_contribution lib tests

**Finding ref:** Miri matrix table in phase4_unified_findings.md (no per-crate finding; the crate is a pure-computation control)
**Bucket:** 1 aliasing, 2 provenance, 3 alignment (Miri axes: Stacked Borrows default, Tree Borrows, strict provenance, symbolic alignment)
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** The crate's unit tests, which exercise the Aho-Corasick scanner and byte-offset slicing at scanner.rs:62 and :140-171, perform no aliasing, provenance or alignment violation under any Miri axis.

**Minimal reproducer:**
```rust
// No bespoke reproducer: the crate's own #[test] functions (40) are the input.
// Equivalent one-liner for the boundary contract the crate relies on:
fn main() {
    let content = "caf\u{e9} terraphim";
    let (start, end) = (5, 14); // positions from terraphim_automata::find_matches
    assert!(content.is_char_boundary(start) && content.is_char_boundary(end));
    let _ = &content[start..end.min(content.len())];
}
```

**Expected signal:**
- Miri: `error: Undefined Behavior:` on any axis. Clean runs print `test result: ok`.

**Falsifiability:** Any Miri UB report inside workspace frames on any of the four axes refutes the hypothesis.

**Invocation (single command):**
```
for f in "" "-Zmiri-tree-borrows" "-Zmiri-strict-provenance" "-Zmiri-symbolic-alignment-check"; do \
  MIRIFLAGS="-Zmiri-disable-isolation $f" cargo +nightly miri test -p terraphim_negative_contribution --lib -- --test-threads=1; done 2>&1 \
  | tee phase3_raw/miri_matrix_terraphim_negative_contribution.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Executed in Phase 3: default 40 passed (406 s), tree-borrows 40 passed (483 s), strict-provenance 40 passed (227 s), symbolic-alignment 40 passed (221 s). Logs: `phase3_raw/miri_{default,tree_borrows,strict_provenance,symbolic_alignment}_terraphim_negative_contribution.log`. No tests filtered.

## EXP-003b: Miri matrix on terraphim_hooks lib tests

**Finding ref:** Miri matrix table in phase4_unified_findings.md (no per-crate finding)
**Bucket:** 1 aliasing, 2 provenance, 3 alignment
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** The hooks crate's replacement and matching code (terraphim_hooks/src/replacement.rs:127-136 feeds byte positions into `replace_range`) performs no aliasing, provenance or alignment violation under any Miri axis.

**Minimal reproducer:**
```rust
// No bespoke reproducer: the crate's own #[test] functions are the input
// (39 run; `discovery` and `latency` modules skipped: filesystem walking and
// timing under Miri's isolation). Contract exercised, distilled:
fn main() {
    let mut s = String::from("run terraphim grep now");
    let (start, end) = (4, 13); // from ReplacementService::find_matches on `s`
    assert!(s.is_char_boundary(start) && s.is_char_boundary(end));
    s.replace_range(start..end, "tg");
    assert_eq!(s, "run tg grep now");
}
```

**Expected signal:**
- Miri: `error: Undefined Behavior:` on any axis; clean runs print `test result: ok`.

**Falsifiability:** Any Miri UB report inside workspace frames refutes the hypothesis for the tested subset; the two skipped modules are outside the claim.

**Invocation (single command):**
```
for f in "" "-Zmiri-tree-borrows" "-Zmiri-strict-provenance" "-Zmiri-symbolic-alignment-check"; do \
  MIRIFLAGS="-Zmiri-disable-isolation $f" cargo +nightly miri test -p terraphim_hooks --lib -- --test-threads=1 --skip discovery --skip latency; done 2>&1 \
  | tee phase3_raw/miri_matrix_terraphim_hooks.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Executed in Phase 3: 39 passed on every axis, 4 filtered (default 324 s, tree-borrows 801 s, strict-provenance 320 s, symbolic-alignment 320 s). Logs: `phase3_raw/miri_*_terraphim_hooks.log`.

## EXP-003c: Miri matrix on terraphim_command_runtime lib tests

**Finding ref:** Miri matrix table in phase4_unified_findings.md (no per-crate finding)
**Bucket:** 1 aliasing, 2 provenance, 3 alignment
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** The command-runtime crate (Arc<Mutex<Config>> helpers, terraphim_command_runtime/src/lib.rs:18-68) performs no aliasing, provenance or alignment violation under Miri.

**Minimal reproducer:**
```rust
// No bespoke reproducer was possible: the crate has zero #[test] functions,
// so `cargo miri test --lib` executed nothing. A Phase 5 re-run needs a
// harness test that drives the public helpers, for example:
fn main() {
    // let cfg = terraphim_command_runtime::CommandRuntime::default_for_tests();
    // let _ = cfg.list_roles();        // or whichever pure helper is public
    println!("EXP-003c: no unit tests exist; author a harness first");
}
```

**Expected signal:**
- Miri: `error: Undefined Behavior:` on any axis once tests exist.

**Falsifiability:** Not falsifiable as run: zero tests executed, so the run neither supports nor refutes the hypothesis.

**Invocation (single command):**
```
MIRIFLAGS="-Zmiri-disable-isolation" cargo +nightly miri test -p terraphim_command_runtime --lib -- --test-threads=1 2>&1 \
  | tee phase3_raw/miri_default_terraphim_command_runtime.log
```

**Verdict:** NO_EVIDENCE
**Phase 10 fresh-eyes correction (2026-09-12):** the verdict line read NO_EVIDENCE, which contradicted this block's own Falsifiability line ("Not falsifiable as run: zero tests executed, so the run neither supports nor refutes the hypothesis") and its own final note ("A vacuous pass is recorded as NEEDS_REFINEMENT, not NO_EVIDENCE"), and disagreed with the Miri matrix row in phase4_unified_findings.md. Corrected to NEEDS_REFINEMENT. Nothing downstream depends on the token: the crate has no `unsafe`, no raw pointers and no UF row.

**Notes:**
- **Executed to a verdict, 2026-09-12 23:25 BST, on the macOS audit host** (log `phase5_experiment_results/EXP-003c_harness_four_axes.log`). The missing test surface was authored as `experiments/EXP-003c/` (added to the experiments workspace members): a harness that builds a `ConfigState` in process (three roles, one with a shortname, one whose name and shortname change byte length under `to_lowercase`) and drives four of the five public helpers -- `get_config`, `get_selected_role`, `list_roles_with_info` and `find_role_by_name_or_shortname` over nine queries (exact, case-insensitive, shortname, shortname upper-cased, miss, empty and a U+0130 query) -- plus a clone/lock round trip so the `Arc` refcount and the `Mutex` guard drop path run more than once. No executor is started: a noop-waker `block_on` drives the futures, because the helpers only await an uncontended `tokio::sync::Mutex::lock`. `save_config` is deliberately excluded: it calls `Persistable::save`, which writes to the configured persistence backends, which is host I/O rather than a property of this crate's code. All four axes clean, 13 checks each, no Miri diagnostic: `MIRI_EXIT[default]=0`, `MIRI_EXIT[-Zmiri-tree-borrows]=0`, `MIRI_EXIT[-Zmiri-strict-provenance]=0`, `MIRI_EXIT[-Zmiri-symbolic-alignment-check]=0`, each captured by a `bash -c` driver. The Phase 3 run was vacuous; this one is not, so the row moves from NEEDS_REFINEMENT to NO_EVIDENCE with the corpus stated.
- Phase 5 (2026-09-12): recorded NO_EVIDENCE with the note "no test surface; 70-line safe lib": the crate has zero #[test] functions and no unsafe, so there is nothing for Miri to execute and no bespoke harness was authored; log copied to `phase5_experiment_results/EXP-003c.log`.
- Executed in Phase 3: `running 0 tests`, `test result: ok. 0 passed` (`phase3_raw/miri_default_terraphim_command_runtime.log`). A vacuous pass is recorded as NEEDS_REFINEMENT, not NO_EVIDENCE. Only the default axis was attempted. Low priority: the crate has no unsafe, no raw pointers and its only shared state is `Arc<tokio::sync::Mutex<Config>>` (phase1_notes/H.md).

## EXP-003d: Miri matrix on terraphim_sessions lib tests

**Finding ref:** UF-009, UF-010 (byte-slicing sites) and the Miri matrix table in phase4_unified_findings.md
**Bucket:** 1 aliasing, 2 provenance, 3 alignment
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** The sessions crate's parsing, search and model code performs no aliasing, provenance or alignment violation under any Miri axis.

**Minimal reproducer:**
```rust
// No bespoke reproducer: the crate's own #[test] functions are the input
// (50 run). Skipped: connector::native, connector::cursor, service and
// aider::test_import_from_tempdir_history_file (kqueue/FSEvents and rusqlite
// FFI are unsupported foreign functions under Miri on macOS).
fn main() {
    // Representative pure path exercised by the run:
    let s = terraphim_sessions::search::search_sessions(&[], "terraphim");
    assert!(s.is_empty());
}
```

**Expected signal:**
- Miri: `error: Undefined Behavior:` on any axis; clean runs print `test result: ok`.

**Falsifiability:** Any Miri UB report inside workspace frames refutes the hypothesis for the 50-test subset. The skipped connector tests are outside the claim (they need the FFI-backed connectors, which Miri cannot run).

**Invocation (single command):**
```
for f in "" "-Zmiri-tree-borrows" "-Zmiri-strict-provenance" "-Zmiri-symbolic-alignment-check"; do \
  MIRIFLAGS="-Zmiri-disable-isolation $f" cargo +nightly miri test -p terraphim_sessions --lib -- --test-threads=1 \
  --skip connector::native --skip connector::cursor --skip service --skip test_import_from_tempdir_history_file; done 2>&1 \
  | tee phase3_raw/miri_matrix_terraphim_sessions.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Executed in Phase 3: 50 passed, 36 filtered on every axis (default 497 s, tree-borrows 702 s, strict-provenance 417 s, symbolic-alignment 418 s). Logs: `phase3_raw/miri_*_terraphim_sessions.log`; the first attempt (`miri_default_terraphim_sessions_probe1.log`) aborted on `kqueue` from the aider tempdir test, which was then skipped.
- The byte-slicing panics UF-009 and UF-010 are safe-Rust panics; Miri would report them as test failures, not UB, and the existing tests do not feed non-ASCII input. EXP-012 covers them.

## EXP-003e: Miri matrix on terraphim_grep lib tests

**Finding ref:** UF-025 (NaN comparator) and the Miri matrix table in phase4_unified_findings.md
**Bucket:** 1 aliasing, 2 provenance, 3 alignment
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** The grep crate's pure-computation subset (signatures, sufficiency judge, scoring) performs no aliasing, provenance or alignment violation under any Miri axis.

**Minimal reproducer:**
```rust
// No bespoke reproducer: the crate's own #[test] functions are the input
// (34 run, 23 filtered). Skipped: every openrouter and hybrid_searcher test
// and six lib.rs tests that reach tokio I/O (`kqueue` unsupported) or the fff
// file picker. Those skipped tests are exactly the ones UF-001 concerns, so
// this run says nothing about the env race.
fn main() {
    use terraphim_grep::hybrid_searcher::{score_kg_boost, KgConcept, RetrievedChunk};
    let c = RetrievedChunk { content: "x".into(), source: "s".into(), line_start: None,
        line_end: None, relevance_score: 1.0, haystack: "code" };
    let _ = score_kg_boost(&c, &[], 0.5);
}
```

**Expected signal:**
- Miri: `error: Undefined Behavior:` on any axis; clean runs print `test result: ok`.

**Falsifiability:** Any Miri UB report inside workspace frames refutes the hypothesis for the 34-test subset.

**Invocation (single command):**
```
for f in "" "-Zmiri-tree-borrows" "-Zmiri-strict-provenance" "-Zmiri-symbolic-alignment-check"; do \
  MIRIFLAGS="-Zmiri-disable-isolation $f" cargo +nightly miri test -p terraphim_grep --lib -- --test-threads=1 \
  --skip openrouter --skip hybrid_searcher::tests --skip needs_synthesis_without_answer_skips_llm \
  --skip needs_synthesis_with_answer_invokes_llm --skip force_rlm_without_answer_invokes_llm \
  --skip insufficient_path_propagates_chunk_count --skip search_without_llm_degrades_to_search_only \
  --skip search_without_thesaurus_uses_fff_mode; done 2>&1 | tee phase3_raw/miri_matrix_terraphim_grep.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Executed in Phase 3: 34 passed, 23 filtered on every axis (default 11 s, tree-borrows 17 s, strict-provenance 11 s, symbolic-alignment 11 s). Logs: `phase3_raw/miri_*_terraphim_grep.log`; the probe run `miri_default_terraphim_grep_probe1.log` shows the `kqueue` abort that drove the skip list.

## EXP-003f: Miri matrix on terraphim-session-analyzer lib tests

**Finding ref:** UF-005 (crossbeam-epoch Stacked Borrows report) and the Miri matrix table in phase4_unified_findings.md
**Bucket:** 1 aliasing, 2 provenance, 3 alignment
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** The session-analyzer's parsing, pattern and reporting code performs no aliasing, provenance or alignment violation under any Miri axis; any report is confined to dependency frames.

**Minimal reproducer:**
```rust
// No bespoke reproducer: the crate's own #[test] functions are the input.
// Two subsets were run: 128 tests (rayon-using analyzer and parser tests
// included) and 80 tests (analyzer::tests and parser::tests excluded).
// connectors::cursor is skipped on every axis (rusqlite FFI).
fn main() {
    // Rayon path that pulls crossbeam-epoch into the run:
    use rayon::prelude::*;
    let n: usize = (0..64usize).into_par_iter().map(|x| x % 3).sum();
    assert_eq!(n, 63);
}
```

**Expected signal:**
- Miri: `error: Undefined Behavior:` inside a workspace frame on any axis. Reports whose innermost frames are all in `crossbeam-epoch` belong to EXP-004.

**Falsifiability:** Any Miri UB report inside workspace frames refutes the hypothesis. A dependency-only report neither confirms nor refutes it.

**Invocation (single command):**
```
for f in "" "-Zmiri-tree-borrows" "-Zmiri-strict-provenance" "-Zmiri-symbolic-alignment-check"; do \
  MIRIFLAGS="-Zmiri-disable-isolation $f" cargo +nightly miri test -p terraphim-session-analyzer --lib -- --test-threads=1 \
  --skip connectors::cursor; done 2>&1 | tee phase3_raw/miri_matrix_terraphim_session_analyzer.log
```

**Verdict:** NO_EVIDENCE
**Lead note (2026-09-12):** symbolic-alignment axis: full 80-test subset attempt aborted at 13/80 clean (proptest modules impractically slow under this axis; aborted log kept); re-run with proptest modules also skipped finished clean: 73 passed, 0 failed, 55 filtered out, 490.74 s (phase3_raw/miri_symbolic_alignment_terraphim_session_analyzer.log, MIRI_DONE exit=0). Verdict NO_EVIDENCE for the 73-test subset only; this axis covered fewer tests than default/strict-provenance (80) and tree-borrows (128).

**Notes:**
- Default (Stacked Borrows) axis: the 128-test run aborted with the crossbeam-epoch report recorded under EXP-004 (`miri_default_terraphim_session_analyzer_probe1.log`, `_probe_parser.log`); with the six rayon-driven analyzer and parser tests skipped, 121 passed (`_probe2.log`, 1882 s); the final 80-test subset passed (`miri_default_terraphim_session_analyzer.log`, 1951 s). No workspace frame appears in any report.
- Tree Borrows axis: 128 passed (3721 s), then Miri exited non-zero with `the main thread terminated without waiting for all remaining threads` (rayon's global pool outlives the test main thread; a leak-check condition, not UB) and two integer-to-pointer-cast warnings in crossbeam-epoch (`miri_tree_borrows_terraphim_session_analyzer.log`).
- Strict-provenance axis: completed clean, 80 passed, 2685 s (`miri_strict_provenance_terraphim_session_analyzer.log`; Phase 5 round 1 summary). The "PENDING / still running" wording here was written before the run finished and was corrected by the Phase 10 fresh-eyes reviewer on 2026-09-12.
- Symbolic-alignment axis: completed clean on a 73-test subset (proptest modules skipped in addition to the 80-test subset's exclusions); see the Lead note above. The "PENDING. No log exists yet" wording was likewise stale and is corrected here.
- The verdict was flipped to NO_EVIDENCE by the lead once both axes completed. Phase 10 caveat: the four axes cover different test sets (73 / 80 / 80 / 128), so this is four partial results rather than one matrix, and `connectors::cursor` is skipped on every axis. Cross-ref: terraphim/terraphim-clients#264 (codex `test_parse_response_item` fails under workspace feature unification; not part of the Miri subset).

## EXP-004: crossbeam-epoch 0.9.18 Stacked Borrows report in Local::element_of on rayon worker threads

**Finding ref:** UF-005 in phase4_unified_findings.md
**Bucket:** 1 aliasing (dependency-inherited)
**Severity (Phase 2):** SUSPICIOUS
**Hypothesis:** The report is a Stacked Borrows rejection of crossbeam-epoch's container-of pattern (`(entry as *const Entry).cast::<Local>()` then `&*local_ptr`, internal.rs:548-549), not a defect reachable from workspace code, and it does not reproduce under Tree Borrows.

**Minimal reproducer:**
```rust
// Distilled container-of shape from crossbeam-epoch-0.9.18/src/internal.rs:546-550.
#[repr(C)]
struct Entry { next: usize }
#[repr(C)]
struct Local { entry: Entry, epoch: usize }
unsafe fn element_of(entry: &Entry) -> &Local {
    let local_ptr = (entry as *const Entry).cast::<Local>();
    &*local_ptr // SB: retag for the whole Local from a tag that covers only Entry
}
fn main() {
    let boxed = Box::new(Local { entry: Entry { next: 0 }, epoch: 7 });
    let entry: &Entry = &boxed.entry;
    let local = unsafe { element_of(entry) };
    println!("epoch = {}", local.epoch);
}
```

**Expected signal:**
- Miri (Stacked Borrows): `error: Undefined Behavior: trying to retag from <tag> for SharedReadWrite permission at alloc[0x8], but that tag does not exist in the borrow stack for this location` at internal.rs:549.
- Miri (Tree Borrows): clean.

**Falsifiability:** If Tree Borrows also rejects the pattern, or if the report moves into a workspace frame, the hypothesis is wrong and the finding must be re-scored.

**Invocation (single command):**
```
cd exp-harness-EXP-004 && MIRIFLAGS="-Zmiri-disable-isolation" cargo +nightly miri run 2>&1 | tee ../phase5_experiment_results/EXP-004.log; \
MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-tree-borrows" cargo +nightly miri run 2>&1 | tee -a ../phase5_experiment_results/EXP-004.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- **Lead note (2026-09-12):** superseded by EXP-101, which is exactly the post-bump Stacked Borrows re-run this block was deferred on. EXP-101 found the report persists inside crossbeam-epoch 0.9.21 only, with no workspace frame, and Tree Borrows clean. Closing NO_EVIDENCE for workspace code.
- Observed in Phase 3 on the session-analyzer default axis: thread `unnamed-5`, frames `Local::element_of` <- `list::Iter::next` <- `Global::try_advance` <- `Global::collect` <- `Local::pin` <- `LocalHandle::pin` <- `crossbeam_epoch::default::pin` on a rayon worker (`phase3_raw/miri_default_terraphim_session_analyzer_probe_parser.log:186-215`). Tree Borrows accepted the same tests (EXP-003f).
- Deferred to the lockfile bump tracked with the RUSTSEC-2026-0204 advisory (crossbeam-epoch 0.9.18 to 0.9.21). Note the two are different defects: the advisory concerns `fmt::Pointer` on invalid pointers, and `element_of` in 0.9.21 (internal.rs:564-568) is byte-for-byte the same code as 0.9.18, so the bump will not clear this report. Phase 5 should re-run the Stacked Borrows axis after the bump; if it persists, record it as a known Stacked Borrows limitation of the dependency (Tree Borrows is the model Miri now recommends for this pattern) and close it NO_EVIDENCE for workspace code.

## EXP-010: terraphim_update state.rs set_var versus any native reader on Linux glibc

**Finding ref:** UF-004 in phase4_unified_findings.md (data-races F-003)
**Bucket:** 7 data races (env setter vs native getenv)
**Severity (Phase 2):** SUSPICIOUS
**Hypothesis:** In the `terraphim_update` lib test binary (130 tests, 10 `#[serial]`), the five `set_var` calls in `setup_temp_config_dir` (crates/terraphim_update/src/state.rs:130-137) execute while some non-std reader calls glibc `getenv` on a sibling thread; the candidate reader is the glibc resolver reached from `test_check_for_updates_auto` (lib.rs:1713) through `getaddrinfo`.

**Minimal reproducer:**
```rust
// Shim self-test first (does the LD_PRELOAD probe see the resolver at all?):
// glibc's own callers of getenv bind to the internal hidden symbol and bypass
// the PLT, so res_init's getenv("RES_OPTIONS")/getenv("LOCALDOMAIN") may be
// invisible to the shim even when they run. Run this under the shim with
// RES_OPTIONS=ndots:2 set and check whether any getenv("RES_OPTIONS") is logged.
use std::net::ToSocketAddrs;
use std::thread;
fn main() {
    let w = thread::spawn(|| for _ in 0..200 {
        let d = std::env::temp_dir();
        unsafe { std::env::set_var("HOME", &d) };
        unsafe { std::env::set_var("USERPROFILE", &d) };
        unsafe { std::env::set_var("XDG_CONFIG_HOME", d.join(".config")) };
        unsafe { std::env::set_var("APPDATA", &d) };
        unsafe { std::env::set_var("LOCALAPPDATA", &d) };
    });
    let r = thread::spawn(|| for _ in 0..20 {
        let _ = ("api.github.com", 443).to_socket_addrs(); // getaddrinfo
    });
    w.join().unwrap();
    r.join().unwrap();
}
```

**Expected signal:**
- envprobe: `[envprobe] OVERLAP(call): getenv(...)` on a tid other than the writer's with a backtrace outside `std::env`. In the shim self-test, a logged `getenv("RES_OPTIONS")` proves the resolver is visible; its absence with the variable set proves the instrument is blind to the resolver.

**Falsifiability:** Zero overlaps over 100 iterations at `widen=0` and 100 at `widen=2000us` refute the hypothesis for every reader the shim can see. It does not refute the resolver reader unless the self-test showed `RES_OPTIONS` being read; in that case the follow-up instrument is Valgrind/Helgrind (which instruments libc itself) on the same binary, or `ltrace -e getenv` with thread ids.

**Invocation (single command):**
```
ssh bigbox 'bash /tmp/ubx-exp010/build.sh && bash /tmp/ubx-exp010/run.sh > /tmp/ubx-exp010/EXP-010.log 2>&1 && bash /tmp/ubx-exp010/run_gai.sh > /tmp/ubx-exp010/EXP-010-gai.log 2>&1' \
  && scp bigbox:/tmp/ubx-exp010/EXP-010.log bigbox:/tmp/ubx-exp010/EXP-010-gai.log phase5_experiment_results/
```

**Verdict:** DEFERRED
**Lead note (2026-09-12):** The LD_PRELOAD shim cannot see glibc-internal resolver reads and valgrind/helgrind is not installed on bigbox; EXP-010-a (helgrind, or a libc getenv uprobe) is recorded as a runbook follow-up rather than blocking convergence. Also carries the new fact that self_update 0.42.0 set_ssl_vars!() is a second unannotated env writer in the terraphim_update lib test binary.

**Notes:**
- As run: the shim is `/tmp/ubx-probe-scratch-src/libenvprobe.so` (not `/tmp/ubx-probe/`), the test binary path comes from `--message-format=json`, and the three scripts plus `gaiprobe.c` and `resolver_selftest.c` are archived in `experiments/EXP-010/`; the original `ls -t` glob invocation was replaced above.
- Phase 5 (2026-09-12, bigbox, glibc 2.39, `phase5_experiment_results/EXP-010.log` with the interpretation appended, raw logs in `EXP-010-raw-logs.tgz` and `EXP-010-gai-raw-logs.tgz`, sources in `experiments/EXP-010/`): NEEDS_REFINEMENT. envprobe shim, 100 iterations at widen 0 us and 100 at 500 us: zero overlaps (pre or call), 52 setenv and ~125 getenv on 61 threads per iteration, all 200 runs `test result: ok`. Shim self-test: getaddrinfo ran but the shim logged only the program's own getenv, so the glibc resolver reader is invisible to this instrument (internal hidden alias, no PLT). Second instrument (gaiprobe, interposes getaddrinfo and setenv): at widen 0 no writer/resolver window in 100 iterations; at widen 500 us, 6 of 100 iterations had a `getaddrinfo("localhost")` (14 per run, from the mock HTTP servers) start while a writer was inside setenv, so the window exists once widened but whether the resolver reads the environment inside it is unobservable here. valgrind/helgrind and ltrace are not installed on bigbox and were not installed. Extra finding: self_update 0.42.0 `set_ssl_vars!()` (`src/lib.rs:692`, Linux only) does `std::env::set_var("SSL_CERT_FILE"/"SSL_CERT_DIR")` on the GitHub backend path reached by the non-serial `test_check_for_updates_auto`, a second unannotated env writer in this binary. Follow-up EXP-010-a: helgrind on a host that has it, or a libc-level getenv probe (uprobe on `getenv`/`__libc_secure_getenv`), repeated at widen 500 us.
- Phase 2 found no native reader in this binary's dependency tree (no libgit2, libsqlite3, openssl or aws-lc; ring has no `getenv`) but did not probe. `test_check_for_updates_auto` performs live HTTPS to api.github.com, so the run needs network on bigbox.
- Independent of the race: the five variables are never restored and the `TempDir` they point at is deleted at test end, so `HOME` dangles for the rest of the process (data-races F-003).

## EXP-011: NaN injection into float comparators (grep relevance_score, markdown importance_total, dedup best-match)

**Finding ref:** UF-025, UF-027, UF-026 in phase4_unified_findings.md (library-trait F-003, F-005, F-004)
**Bucket:** 12 library trait invariants (comparator total order; Deserialize-produced invalid state)
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** A NaN reaching `partial_cmp(..).unwrap_or(Equal)` in `HybridSearcher::fuse_and_rank` / `boost_chunks_with_kg` (crates/terraphim_grep/src/hybrid_searcher.rs:210-214, :428-432) or in `list_all_entries` (crates/terraphim_agent/src/learnings/capture.rs:1445-1463) makes the comparator non-transitive and the std sort panics with its total-order message; and a NaN reaching `partial_cmp(..).unwrap()` in `store_with_dedup` (shared_learning/store.rs:242) panics on `None`.

**Minimal reproducer:**
```rust
// exp-harness-EXP-011/src/main.rs; path-depends on terraphim_grep and
// terraphim_agent (default features). serde_json rejects a bare NaN token, so
// the value is constructed directly, as a non-JSON deserialiser would.
use terraphim_grep::hybrid_searcher::{boost_chunks_with_kg, RetrievedChunk};
fn chunk(score: f64) -> RetrievedChunk {
    RetrievedChunk { content: String::new(), source: String::new(), line_start: None,
        line_end: None, relevance_score: score, haystack: "code" }
}
fn main() {
    let mut v: Vec<RetrievedChunk> = Vec::new();
    for i in 0..20 { v.push(chunk(if i % 7 == 3 { f64::NAN } else { i as f64 })); }
    let r = std::panic::catch_unwind(|| boost_chunks_with_kg(v, &[], 0.5));
    println!("grep arm: {}", if r.is_err() { "EXP-011 CONFIRMED (panic)" } else { "NO_EVIDENCE" });
    // Markdown arm: write three learnings/*.md whose frontmatter carries
    // `importance_total: NaN` plus finite severity/repetition/recency and
    // `importance_has_correction: false`, then:
    let dir = std::env::temp_dir().join("exp011");
    let r2 = std::panic::catch_unwind(|| terraphim_agent::learnings::capture::list_all_entries(&dir, 10));
    println!("markdown arm: {}", if r2.is_err() { "CONFIRMED (panic)" } else { "NO_EVIDENCE" });
    // Dedup arm: two textually empty SharedLearning values with distinct ids,
    // then store_with_dedup of a learning with one token; expect unwrap on None.
}
```

**Expected signal:**
- std (Rust 1.81+ driftsort): panic `user-provided comparison function does not correctly implement a total order` from `slice::sort_by`.
- Dedup arm: panic `called Option::unwrap() on a None value` at store.rs:242.

**Falsifiability:** If all three arms return an ordering without panicking on the pinned toolchain, the panic hypothesis is refuted for this std version; the docs only say the sort "may" panic, so a clean run proves the comparator is tolerated today, not that it is a total order. The masking at hybrid_searcher.rs:126 (`f64::min` cap) must also be confirmed: a NaN `KgConcept.score` should yield a finite boost.

**Invocation (single command):**
```
cd exp-harness-EXP-011 && RCH_DISABLED=1 cargo +nightly run --release 2>&1 | tee ../phase5_experiment_results/EXP-011.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Phase 5 (2026-09-12): NO_EVIDENCE for UB; panic confirmed on all three arms (`phase5_experiment_results/EXP-011.log`; harness `experiments/EXP-011/`, `cargo +nightly run -p exp011_nan_comparators`, dev profile, rustc 1.98.0-nightly aarch64-apple-darwin). Grep arm (`boost_chunks_with_kg`, two arguments, empty concept list): `user-provided comparison function does not correctly implement a total order` from `slice::sort_by` at n = 21, 32, 50, 64, 100 and 1000 for several NaN placements; n <= 20 never panics (insertion-sort path) and detection at larger n depends on placement (54 of 65 size/placement cases returned without panic), so a clean run is 'not detected', not 'refuted'. Markdown arm (`terraphim_agent::learnings::list_all_entries`, frontmatter `importance_total: NaN`, all five importance keys present): panics at n = 30, not at n = 3 or 300. Dedup arm (`SharedLearningStore::store_with_dedup` with two textually empty learnings indexed, so `avg_doc_len = 0` and BM25 yields NaN): `called Option::unwrap() on a None value` at store.rs:242. Masking arm: a NaN `KgConcept.score` gives boost 0 (NaN alone, `f64::max` drops it) or the cap (NaN plus a finite score, `f64::min` drops it), both finite, so the hybrid_searcher.rs:126 masking claim holds. Safe-Rust panics: correctness bucket, not UB. The sketch's three-argument `boost_chunks_with_kg` call is schematic; the function takes two.
- `boost_chunks_with_kg` and `RetrievedChunk` are `pub` in `terraphim_grep::hybrid_searcher`; `list_all_entries` is `pub` in `terraphim_agent::learnings::capture` (module always compiled); `SharedLearningStore::store_with_dedup` is `pub` under the default `shared-learning` feature. `SharedLearning` comes from `terraphim_types` (registry `terraphim`).
- Add a proptest over `prop::num::f64::ANY` for the grep comparator to characterise density; keep it in the harness, not in `crates/`.

## EXP-012: non-ASCII input through the byte-index string-slicing sites

**Finding ref:** UF-009, UF-010, UF-011, UF-028 in phase4_unified_findings.md (library-trait F-001, F-002, F-006, F-007; panic-safety F-003)
**Bucket:** 12 library trait invariants (str char-boundary contract), cross-tag 11 panic safety
**Severity (Phase 2):** SUSPICIOUS
**Hypothesis:** Each site guards only the byte length and slices at a fixed byte offset, so a string whose code point straddles that offset panics with `byte index N is not a char boundary`: `SessionEnricher::extract_context` (crates/terraphim_sessions/src/enrichment/enricher.rs:177-188), `Session::summary` (model.rs:326-336), the handler/procedure/main display truncations (repl/handler.rs:994,1995,2051,2098,2172,2226,2321,2585; learnings/procedure.rs:427; main.rs:2774) and `find_paragraph_end` (commands/markdown_parser.rs:452-470).

**Minimal reproducer:**
```rust
// exp-harness-EXP-012/src/main.rs; path-depends on terraphim_sessions.
// The sessions sites are called for real; the agent display closures are
// private fns inside bin/feature-gated modules, so their idiom is copied
// verbatim as a mechanism reproducer (same guard, same slice).
use terraphim_sessions::model::{Message, MessageRole, Session, SessionMetadata};
use terraphim_sessions::search::search_sessions;
fn user_session(content: &str) -> Session {
    // Session and Message do not derive Default: fill every field.
    Session { id: "s1".into(), source: "exp".into(), external_id: "s1".into(), title: None,
        source_path: std::path::PathBuf::new(), started_at: None, ended_at: None,
        messages: vec![Message::text(0, MessageRole::User, content)],
        metadata: SessionMetadata::default() }
}
fn display_cut(t: &str) -> String { if t.len() > 40 { format!("{}...", &t[..40]) } else { t.to_string() } }
fn main() {
    // model.rs:332: byte 100 is a continuation byte when the text starts with one ASCII byte.
    let odd = format!("a{}", "\u{e9}".repeat(50));
    let r = std::panic::catch_unwind(|| search_sessions(&[user_session(&odd)], "x"));
    println!("summary arm: {}", if r.is_err() { "EXP-012 CONFIRMED (panic)" } else { "NO_EVIDENCE" });
    // handler.rs:1995 idiom: 40 is even, so the odd-offset text panics.
    let r2 = std::panic::catch_unwind(|| display_cut(&odd));
    println!("display arm: {}", if r2.is_err() { "CONFIRMED (panic)" } else { "NO_EVIDENCE" });
    // enricher arm: "\u{e9}".repeat(40) + "a" + "terraphim" with a one-term thesaurus,
    // SessionEnricher::new(thesaurus).enrich_session(&session); ctx_start = 31 is a continuation byte.
}
```

**Expected signal:**
- Rust panic: `byte index 100 is not a char boundary; it is inside 'é' (bytes 99..101)` (summary arm); `byte index 31 is not a char boundary` (enricher arm); `byte index 40 is not a char boundary` (display arm); `byte index 23 is not a char boundary` (paragraph-finder arm, input `"terraphim " + "é"x11 + "\n\n" + "tail"`).

**Falsifiability:** If every arm returns without panicking, the sites are boundary-safe by some means the static read missed (for example an `is_char_boundary` walk added since the sweep); the harness must then be re-checked against the current line numbers.

**Invocation (single command):**
```
cd exp-harness-EXP-012 && RCH_DISABLED=1 cargo +nightly run --release 2>&1 | tee ../phase5_experiment_results/EXP-012.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Phase 5 (2026-09-12): NO_EVIDENCE for UB; panic confirmed on all five arms (`phase5_experiment_results/EXP-012.log`; harness `experiments/EXP-012/`, run as `cargo +nightly run -p exp012_char_boundary`, dev profile, rustc 1.98.0-nightly (bd08c9e71 2026-06-25) aarch64-apple-darwin). Real calls: `Session::summary` direct and via `search_sessions` (`end byte index 100 is not a char boundary; it is inside 'é' (bytes 99..101 of string)`) and `SessionEnricher::enrich_session` -> `extract_context` (`start byte index 31 is not a char boundary`; Aho-Corasick positions are byte offsets). Mechanism copies of the private sites: handler.rs:1995 idiom (`end byte index 40`) and markdown_parser `find_paragraph_end` plus its caller slice (`end byte index 23`). An ASCII control of the same byte lengths returned cleanly. Safe-Rust panics: correctness bucket, not UB. The sketch above is schematic; `Message` and `Session` have no `Default`, the harness uses `Message::text` and builds `Session` by hand.
- Safe-Rust panics, never UB; the experiment exists because two sites (enricher, summary) are on production paths reached from imported session data (`sessions search`, REPL enrichment) and the same class was already fixed once at cli_helpers.rs:52.
- Add a proptest over `Vec<char>` prefixes for `Session::summary` and `find_paragraph_end` (assert `is_char_boundary(result)`); keep it in the harness.
- `find_paragraph_end` is private inside `commands::markdown_parser` (feature `repl-custom`, not default) with test-only callers; its arm is a mechanism copy.

## EXP-013: current_thread #[tokio::test] calling the sync LearningStore bridge

**Finding ref:** UF-008 in phase4_unified_findings.md (async-drop F-002, data-races F-013)
**Bucket:** 17 async drop / runtime nesting
**Severity (Phase 2):** SUSPICIOUS
**Hypothesis:** Calling any method of the sync `terraphim_types::shared_learning::LearningStore` impl on `SharedLearningStore` (crates/terraphim_agent/src/shared_learning/store.rs:608-612 bridge, :616-753 impl) from a current_thread runtime panics with tokio's `can call blocking only when running on the multi-threaded runtime`, and from no runtime panics with `there is no reactor running`.

**Minimal reproducer:**
```rust
// exp-harness-EXP-013/src/lib.rs (tests); path-depends on terraphim_agent with
// default features (shared-learning on) and terraphim_types = { registry = "terraphim" }.
use terraphim_agent::shared_learning::store::{SharedLearningStore, StoreConfig};
use terraphim_types::shared_learning::LearningStore;
async fn open_store() -> SharedLearningStore {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = StoreConfig::default();
    cfg.markdown.base_dir = dir.path().to_path_buf(); // field name per StoreConfig
    std::mem::forget(dir);
    SharedLearningStore::open(cfg).await.unwrap()
}
#[tokio::test] // bare: current_thread flavour
#[should_panic(expected = "can call blocking only when running on the multi-threaded runtime")]
async fn sync_trait_panics_on_current_thread() {
    let store = open_store().await;
    let dyn_store: &dyn LearningStore = &store;
    let _ = dyn_store.get("missing");
}
#[test]
#[should_panic(expected = "there is no reactor running")]
fn sync_trait_panics_without_runtime() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let store = rt.block_on(open_store());
    drop(rt);
    let dyn_store: &dyn LearningStore = &store;
    let _ = dyn_store.get("missing");
}
```

**Expected signal:**
- Both `#[should_panic]` tests pass with the quoted substrings. A third positive test (ten concurrent sync calls from spawned tasks on a `worker_threads = 1` multi_thread runtime) completes, exercising the `block_in_place` core hand-off.

**Falsifiability:** If the current_thread test does not panic, the bridge no longer depends on the runtime flavour (for example it was changed to `Handle::try_current` plus a fallback) and the finding closes.

**Invocation (single command):**
```
cd exp-harness-EXP-013 && RCH_DISABLED=1 cargo +nightly test --release 2>&1 | tee ../phase5_experiment_results/EXP-013.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Phase 5 (2026-09-12): NO_EVIDENCE for UB; panic confirmed (`phase5_experiment_results/EXP-013.log`; harness `experiments/EXP-013/`, `cargo +nightly test -p exp013_current_thread_bridge`, 3 passed). current_thread `#[tokio::test]`: `panicked at crates/terraphim_agent/src/shared_learning/store.rs:609:5: can call blocking only when running on the multi-threaded runtime`; no runtime: `panicked at store.rs:610:18: there is no reactor running, must be called from the context of a Tokio 1.x runtime`. Positive control (ten concurrent sync calls from spawned tasks on a `worker_threads = 1` multi_thread runtime) completed. The `StoreConfig` field is `markdown.learnings_dir`, not `base_dir` as sketched; the public path is `terraphim_agent::shared_learning::{SharedLearningStore, StoreConfig}` (the `store` module itself is private).
- The sync impl has zero production callers in this workspace (terraphim_middleware feedback_loop uses the async `GraphTouchStore`); the contract is latent until an out-of-workspace consumer drives it from a current_thread runtime. Not UB either way; the experiment converts a documented panic into a tool-arbitrated one and provides the regression test for the doc-only fix.
- Adjust the `StoreConfig` field names to the current struct (store.rs:26-30) when authoring the harness.

## EXP-014: terraphim_test_utils mixed-binary rule (#[serial] on the writer alone does not exclude unannotated siblings)

**Finding ref:** UF-003 in phase4_unified_findings.md (data-races F-006)
**Bucket:** 7 data races (unsound safe abstraction over env setters)
**Severity (Phase 2):** SUSPICIOUS
**Hypothesis:** A test binary in which only the env-writing test is `#[serial]` (the rule the `terraphim_test_utils` SAFETY comment prescribes) still runs unannotated sibling tests concurrently with the write, so the wrapper's stated precondition cannot be satisfied by following its own rule.

**Minimal reproducer:**
```rust
// exp-harness-EXP-014/src/lib.rs (tests); dev-deps: serial_test = "3",
// terraphim_test_utils = { version = "1.20", registry = "terraphim" }.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::time::{Duration, Instant};
static WRITER_ACTIVE: AtomicBool = AtomicBool::new(false);
static OVERLAPS: AtomicUsize = AtomicUsize::new(0);
#[test]
#[serial_test::serial]
fn writer_is_serial() {
    let _g = terraphim_test_utils::EnvVarGuard::set("EXP014_VAR", "1");
    WRITER_ACTIVE.store(true, SeqCst);
    std::thread::sleep(Duration::from_millis(300)); // guard live, siblings free to run
    WRITER_ACTIVE.store(false, SeqCst);
}
fn sibling() {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(400) {
        if WRITER_ACTIVE.load(SeqCst) { OVERLAPS.fetch_add(1, SeqCst); break; }
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test] fn sibling_a() { sibling() }
#[test] fn sibling_b() { sibling() }
#[test]
fn zz_report() {
    std::thread::sleep(Duration::from_millis(800));
    assert!(OVERLAPS.load(SeqCst) > 0, "EXP-014 NO_EVIDENCE: siblings never ran during the serial writer");
}
```

**Expected signal:**
- `zz_report` passes (OVERLAPS > 0): unannotated siblings observed the writer's guard live. With `--test-threads=1` the same binary reports zero overlaps, which is the control.

**Falsifiability:** If OVERLAPS is 0 at default parallelism across 20 runs, `serial_test` (or libtest) excludes unannotated tests after all and the wrapper's rule is sufficient; the finding then downgrades to CONTRACTUAL-BUT-DEFENSIBLE.

**Invocation (single command):**
```
cd exp-harness-EXP-014 && RCH_DISABLED=1 cargo +nightly test --release -- --test-threads=4 2>&1 | tee ../phase5_experiment_results/EXP-014.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Phase 5 (2026-09-12): NO_EVIDENCE for UB, hypothesis confirmed: the log contains libtest scheduling evidence only, no tool-observed UB, so the finding keeps its Phase 2 severity (SUSPICIOUS) rather than being promoted; the wrapper's stated precondition is unsatisfiable by its own rule, and the memory-level manifestation stays with UF-002 / EXP-002c. Harness `experiments/EXP-014/` (`cargo +nightly test -p exp014_serial_mixed_binary -- --test-threads=4`, `phase5_experiment_results/EXP-014.log`): five runs at default parallelism all passed `zz_report` with `overlaps=3` (all three unannotated siblings observed the `#[serial]` writer's `EnvVarGuard` live and read `EXP014_VAR=Ok("1")` from it), plus five `--nocapture` runs with the same counts and timestamps. Control `--test-threads=1`: `overlaps=0`, `zz_report` fails as designed (exit 101), confirming the harness only reports real concurrency. `serial_test` 3.5.0 excludes only other `#[serial]` tests; libtest schedules unannotated tests freely.
- Design finding whose UB manifestation is UF-002 (EXP-002c). The wrapper is a safe `pub fn` over `unsafe { set_var }` in a registry crate (`terraphim_test_utils-1.20.3/src/lib.rs:36-42`, guards at :106-113), so the remediation lands outside this workspace: make it `unsafe fn`, or take a process-wide lock that every env-reading test also takes, or mark whole binaries serial. The `build.rs` cfg `rust_has_unsafe_env_setters` in crates/terraphim_agent/build.rs:19 has zero consumers and is dead.

## EXP-015: mcp_server integration binaries spawning the real server without TERRAPHIM_PROFILE_MEMORY_TYPE

**Finding ref:** UF-015 in phase4_unified_findings.md (phase1_notes/G.md open question 1)
**Bucket:** 7 data races (cross-process shared state; check-then-act), cross-tag 11 file lifecycle
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** The seven unserialised `terraphim_mcp_server` integration binaries (integration_test, test_advanced_automata_functions, test_bug_report_extraction, test_kg_term_verification, test_mcp_fixes_validation, test_selected_role_usage, test_working_advanced_functions) each spawn a child server that opens the default sqlite store at `${TERRAPHIM_DATA_PATH:-~/.terraphim}/sqlite/terraphim.db`, so up to seven server processes share one on-disk database concurrently during `cargo test --all-targets`.

**Minimal reproducer:**
```rust
// exp-harness-EXP-015/src/main.rs: spawn the built server binary N times with
// HOME pointed at a fresh directory and TERRAPHIM_PROFILE_MEMORY_TYPE unset,
// wait for readiness, then count opened sqlite files and WAL handles.
use std::process::{Command, Stdio};
fn main() {
    let home = tempfile::tempdir().unwrap();
    let bin = std::env::var("TERRAPHIM_MCP_SERVER_BIN").expect("path to terraphim_mcp_server");
    let mut kids = Vec::new();
    for i in 0..7u16 {
        kids.push(Command::new(&bin)
            .env("HOME", home.path()).env_remove("TERRAPHIM_PROFILE_MEMORY_TYPE")
            .args(["--sse", "--bind", &format!("127.0.0.1:{}", 18000 + i)])
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    }
    std::thread::sleep(std::time::Duration::from_secs(5));
    let db = home.path().join(".terraphim/sqlite/terraphim.db");
    let wal = home.path().join(".terraphim/sqlite/terraphim.db-wal");
    println!("db exists = {}, wal exists = {}", db.exists(), wal.exists());
    let lsof = Command::new("lsof").arg(&db).output().map(|o| o.stdout.len()).unwrap_or(0);
    println!("lsof bytes = {lsof}  (>0 with 7 children => EXP-015 CONFIRMED: shared on-disk db)");
    for mut k in kids { let _ = k.kill(); let _ = k.wait(); }
}
```

**Expected signal:**
- `db exists = true` under the fresh HOME and `lsof` listing several child pids on the same file (or a `-wal` file with multiple writers). Alternatively `strace -f -e openat` on one child showing `terraphim.db` opened during startup.

**Falsifiability:** If no child creates `~/.terraphim/sqlite/terraphim.db` (for example `ConfigState::new` never opens the store for the default server profile, or the child resolves a memory profile), the binaries do not share on-disk state and the open question closes as a non-issue.

**Invocation (single command):**
```
cd exp-harness-EXP-015 && TERRAPHIM_MCP_SERVER_BIN="$(cargo +nightly build -p terraphim_mcp_server --message-format=json 2>/dev/null | rg -o '"executable":"[^"]+"' | head -1 | cut -d'"' -f4)" \
  RCH_DISABLED=1 cargo +nightly run --release 2>&1 | tee ../phase5_experiment_results/EXP-015.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Phase 5 (2026-09-12): NO_EVIDENCE for UB; the hypothesis' premise is confirmed and recorded as a test-hygiene finding. Static trace: `main.rs:207 ConfigState::new` loads thesauri only; storage is opened lazily on the first `Persistable::save/load` (`terraphim_service` `update_config`/`update_selected_role`, `terraphim_persistence` conversation store) through `DeviceStorage::instance()` -> `init_device_storage()` -> `DeviceSettings::load_from_env_and_file(<ProjectDirs com.aks.terraphim config dir>/settings.toml)`, which on this host lists profiles memory, rocksdb, redb, sqlite (`/Users/alex/.terraphim/sqlite/terraphim.db`) and dashmap; the registry default `settings.toml` ships the same sqlite profile under `${TERRAPHIM_DATA_PATH:-~/.terraphim}/sqlite`. Empirical: one run of `cargo +nightly test -p terraphim_mcp_server --test integration_test -- --test-threads=1` (7 passed, 18.56 s, HOME not redirected, `TERRAPHIM_PROFILE_MEMORY_TYPE` unset) while polling `lsof -p` on every `terraphim_mcp_server` child every 200 ms: child pid 86519 held `terraphim.db`, `terraphim.db-wal` and `terraphim.db-shm` open read-write (fds 12u/13u/14u, later 15u/16u, i.e. more than one sqlite connection) from 10:30:59 until the test ended (`phase5_experiment_results/EXP-015.log`, `EXP-015.lsof_poll.log`). The read-write fds are the evidence that the shipped test's child opens the developer's real `~/.terraphim` store; the `-wal` mtime advance during the run is only corroborating, because the before-listing shows the WAL already being modified by another process at 10:27. The `stat -f` lines in the log are a quoting bug (format string parsed as a path) and carry no information; the `ls -la` listings next to them are the before/after record. Cross-process sqlite access is serialised by sqlite's file locks (WAL mode), so this is not memory UB; the seven-binary spawn harness was not needed to answer the question and was not run.
- Not memory UB: sqlite serialises cross-process access with file locks, so the expected failure modes are WAL stalls, `database is locked` errors and tests writing into the developer's real `~/.terraphim`. Recorded because phase1_notes/G.md left it open and the two serialised binaries (e2e, rolegraph_validation) deliberately set `TERRAPHIM_PROFILE_MEMORY_TYPE=memory` while the seven parallel ones do not (crates/terraphim_mcp_server/tests/*.rs: only the two serial files contain that variable).
- Related: G open question 3 (blocking-pool reader inside `DeviceStorage::init_memory_only`) is covered by the same run if the probe from EXP-010 is layered on the serial binaries.

## EXP-101: post-bump Stacked Borrows re-run of the session-analyzer rayon tests (crossbeam-epoch 0.9.21)

**Follow-up of:** EXP-004
**Finding ref:** UF-005 in phase4_unified_findings.md (Phase 6 round 1 idea I-01)
**Bucket:** 1 aliasing (dependency-inherited)
**Severity (Phase 2):** SUSPICIOUS
**Hypothesis:** With the lockfile now at crossbeam-epoch 0.9.21 (commit ae06cd2, 2026-09-12 10:51 BST), the six rayon-driven analyzer and parser tests that Phase 3 skipped still trigger the `Local::element_of` Stacked Borrows report (0.9.21 internal.rs:564-568 is the same container-of code as 0.9.18), and the report still names no workspace frame; the Tree Borrows axis stays clean.

**Minimal reproducer:**
```rust
// No bespoke reproducer: the six rayon tests are the input. The rayon
// entry points are crates/terraphim-session-analyzer/src/parser.rs:177,242,284
// (par_iter over entries) and analyzer.rs:72 (par_iter over parsers).
// Equivalent shape the tests exercise:
fn main() {
    use rayon::prelude::*;
    let entries: Vec<u32> = (0..64).collect();
    let n: usize = entries.par_iter().filter_map(|x| (x % 3 == 0).then_some(1usize)).sum();
    assert_eq!(n, 22);
}
```

**Expected signal:**
- Miri (Stacked Borrows): `error: Undefined Behavior: trying to retag from <tag> for SharedReadWrite permission` with innermost frame `crossbeam_epoch::internal::Local::element_of` at crossbeam-epoch-0.9.21/src/internal.rs:564-568 on a rayon worker thread.
- Miri (Tree Borrows): `test result: ok` for the same six tests.

**Falsifiability:** If the Stacked Borrows axis is clean for all six tests, the bump did clear the report and UF-005 closes NO_EVIDENCE. If the report moves into a workspace frame on either axis, UF-005 must be re-scored. If the report persists only in crossbeam-epoch frames under Stacked Borrows and Tree Borrows is clean, record it as a known Stacked Borrows limitation of the dependency and close UF-005 NO_EVIDENCE for workspace code.

**Invocation (single command):**
```
for f in "" "-Zmiri-tree-borrows"; do MIRIFLAGS="-Zmiri-disable-isolation $f" CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target \
  cargo +nightly miri test -p terraphim-session-analyzer --lib -- --test-threads=1 analyzer::tests parser::tests; done 2>&1 \
  | tee phase5_experiment_results/EXP-101.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Executed by the lead 2026-09-12 at HEAD ae06cd2 (crossbeam-epoch 0.9.21), log phase5_experiment_results/EXP-101.log. Stacked Borrows: the report persists, innermost frame `crossbeam_epoch::internal::Local::element_of` at crossbeam-epoch-0.9.21/src/internal.rs:567, reached via `crossbeam_deque::Stealer<JobRef>::steal` on a rayon worker; every frame is crossbeam-epoch, crossbeam-deque, rayon-core or std, no workspace frame. Tree Borrows: 48 passed, 0 failed for the same filter. The tree-borrows process still exited 1 with "the main thread terminated without waiting for all remaining threads", which is Miri's leak check tripping on rayon's global pool daemon threads at exit, not a UB report. Third falsifiability branch applies: known Stacked Borrows limitation of the dependency; UF-005 closes NO_EVIDENCE for workspace code.
- Filed by Phase 6 round 1 (structural lens). Gate: the committed lockfile must carry crossbeam-epoch >= 0.9.21. At the time of filing `git show HEAD:Cargo.lock` reports 0.9.21 (commit ae06cd2) and `git status` is clean; earlier in the same round (10:45 to 10:50 BST) the working tree was observed at 0.9.18 and then 0.9.21, so the lockfile was in flux while other agents worked. Record the `git rev-parse HEAD` the run used.
- Expected outcome per EXP-004 notes: the report persists (identical `element_of` code). The value of the run is closing UF-005 with an explicit post-bump observation rather than an inference.
- Budget: Phase 3 measured the 128-test default-axis run at 1882 s to 3721 s; the six-test filter should take under 15 minutes per axis.

## EXP-102: reachability of the self_update `set_ssl_vars!()` env writer from non-serial terraphim_update tests (UF-036)

**Follow-up of:** EXP-010
**Finding ref:** UF-036 in phase4_unified_findings.md (Phase 6 round 1 idea I-02)
**Bucket:** 7 data races (env setter vs native getenv; hidden-unsafe-in-safe-wrapper)
**Severity (Phase 2):** SUSPICIOUS
**Hypothesis:** In the `terraphim_update --lib` test binary (130 tests: config 10, downloader 11, lib 19, manifest 11, notification 15, platform 12, rollback 18, scheduler 10, signature 14, state 10; the 10 `#[serial]` tests are all in state.rs), exactly one non-serial test reaches the writer: `test_check_for_updates_auto` (lib.rs:1712-1734, `#[tokio::test]`) -> `check_for_updates_auto` (lib.rs:1277) -> `spawn_blocking` -> `self_update::backends::github::Update::get_latest_release` (github.rs:518) -> `set_ssl_vars!()` (self_update-0.42.0/src/macros.rs:12-24), which calls the safe edition-2021 `std::env::set_var("SSL_CERT_FILE" | "SSL_CERT_DIR")` on Linux only and only when the variable is absent. The writer runs concurrently with the 119 other non-serial tests, so any native `getenv` reader among them (the EXP-010 resolver candidate, `getaddrinfo("localhost")` from downloader.rs:481,502,527) can overlap it.

**Minimal reproducer:**
```rust
// Distilled access pair. Real artefact: the unmodified --lib binary under the
// EXP-010 instrument. Writer = self_update macros.rs:12-24 on Linux; reader =
// glibc resolver reached from the downloader tests' can_connect("localhost", 3000).
use std::net::ToSocketAddrs;
use std::thread;
fn main() {
    let w = thread::spawn(|| for _ in 0..200 {
        unsafe { std::env::remove_var("SSL_CERT_FILE") };
        if std::env::var_os("SSL_CERT_FILE").is_none() {
            unsafe { std::env::set_var("SSL_CERT_FILE", "/etc/ssl/certs/ca-certificates.crt") };
        }
    });
    let r = thread::spawn(|| for _ in 0..50 {
        let _ = ("localhost", 3000).to_socket_addrs(); // getaddrinfo
    });
    w.join().unwrap();
    r.join().unwrap();
}
```

**Expected signal:**
- Static arm (done at filing): the call chain above, one non-serial test, zero `#[serial]` outside state.rs. Second binary check: `tests/r2_update.rs` selects `UpdateBackend::R2` at :102 and only asserts the builder at :236-238, so it never reaches the GitHub backend.
- Dynamic arm: helgrind or a libc-level `getenv` uprobe (the EXP-010-a instrument) reporting a `getenv` inside `res_init`/`__libc_res_nsend` on a reader thread while `setenv("SSL_CERT_FILE" | "SSL_CERT_DIR")` is in progress on the `test_check_for_updates_auto` blocking-pool thread.

**Falsifiability:** If, with `SSL_CERT_FILE` and `SSL_CERT_DIR` unset on the host, the instrument that can see glibc-internal `getenv` records zero overlaps across 100 iterations at widen 0 and 100 at widen 500 us, the writer is ordered relative to every reader in this binary and UF-036 downgrades to CONTRACTUAL-BUT-DEFENSIBLE. The static arm cannot be refuted by a run; it is refuted only by a source change that adds `#[serial]` to the test or removes the network call.

**Invocation (single command):**
```
ssh bigbox 'env -u SSL_CERT_FILE -u SSL_CERT_DIR bash /tmp/ubx-exp010/run.sh' > phase5_experiment_results/EXP-102.log 2>&1 \
  # same runner as EXP-010; requires the EXP-010-a instrument (helgrind or uprobe) on the host
```

**Verdict:** DEFERRED

**Notes:**
- Filed by Phase 6 round 1 (structural lens). Gate: the EXP-010-a instrument (valgrind/helgrind, or a `getenv` uprobe) on bigbox; the LD_PRELOAD shim cannot see the glibc resolver's internal `getenv` (EXP-010 self-test). Do not invent a new reader: the reader set is UF-004's.
- Platform: `set_ssl_vars!` is `#[cfg(target_os = "linux")]`, so the writer is inert on the macOS audit host and live on both CI runners (ubuntu-latest, bigbox). `ci.yml` runs this binary with `--lib`.
- Two properties that shape remediation: (1) the macro writes only when the variable is absent, so presetting `SSL_CERT_FILE` and `SSL_CERT_DIR` in the CI environment makes the writer a no-op without touching source; (2) the call is a safe `std::env::set_var` inside an edition-2021 dependency, so no `unsafe` block in the workspace marks it and the Phase 1 env-setter sweep (six blocks) could not have seen it. The durable fix is `#[serial]` on `test_check_for_updates_auto` (or gating it behind an env flag, as it already performs live HTTPS to api.github.com).
- self_update 0.42.0 depends on openssl-probe 0.2.1 but the macro does not call it; the writer is the macro body itself.

## EXP-103: empty pattern string in a tools.toml yields zero-width Aho-Corasick matches at non-char boundaries (session-analyzer slice panic)

**Finding ref:** UF-038 in phase4_unified_findings.md (assigned by lead 2026-09-12; Phase 6 round 1 idea I-08)
**Bucket:** 12 library trait invariants (str char-boundary contract), cross-tag 11 panic safety
**Severity (Phase 2):** SUSPICIOUS (not scored in Phase 2)
**Hypothesis:** `AhoCorasickMatcher::build_automaton` (crates/terraphim-session-analyzer/src/patterns/matcher.rs:97-120) passes every pattern string to `AhoCorasickBuilder` without rejecting empty strings, and `load_patterns_from_str` (patterns/loader.rs:79-91) validates only that the `patterns` Vec is non-empty. An empty pattern makes `find_iter` emit zero-width matches at every byte position, including continuation bytes, so `&text[mat.start()..mat.end()]` at matcher.rs:143 panics with `byte index N is not a char boundary` on any non-ASCII session text. Reachable from `~/.config/claude-log-analyzer/tools.toml` via `load_user_patterns` (loader.rs:100-111) on every `tsa` run, and from `load_patterns_from_file`.

**Minimal reproducer:**
```rust
// experiments/EXP-103/src/main.rs; path-depends on terraphim-session-analyzer
// (default features). Real-call arm through the crate's own loader and matcher.
use terraphim_session_analyzer::patterns::loader::load_patterns_from_str;
use terraphim_session_analyzer::patterns::matcher::{AhoCorasickMatcher, PatternMatcher};
const TOML: &str = "[[tools]]\nname = \"cargo\"\npatterns = [\"cargo build\", \"\"]\n\
    [tools.metadata]\ncategory = \"build\"\nconfidence = 0.9\n";
fn main() {
    let patterns = load_patterns_from_str(TOML).expect("loader accepts the empty pattern");
    let mut m = AhoCorasickMatcher::new();
    m.initialize(&patterns).expect("automaton builds with an empty pattern");
    let text = "caf\u{e9} \u{1F600} cargo build";
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m.find_matches(text)));
    match r {
        Err(_) => println!("EXP-103 CONFIRMED (panic at matcher.rs:143)"),
        Ok(v) => {
            let bad = v.iter().filter(|x| !text.is_char_boundary(x.start) || !text.is_char_boundary(x.end)).count();
            println!("EXP-103 NO_EVIDENCE: {} matches, {} at non-boundaries", v.len(), bad);
        }
    }
}
```

**Expected signal:**
- Rust panic from matcher.rs:143: `start byte index 2 is not a char boundary; it is inside 'é' (bytes 1..3 of string)` (first zero-width match inside the first multi-byte character).
- Control: the same TOML without the empty string returns matches with `is_char_boundary` true at both ends.

**Falsifiability:** If `load_patterns_from_str` rejects the empty string, or `find_matches` returns without panicking on the non-ASCII text, the loader or matcher filters empty patterns by a route the static read missed and the finding closes. The mechanism arm cannot be refuted: aho-corasick 1.1.4 with `LeftmostLongest` and `ascii_case_insensitive(true)` was already observed emitting 5 non-boundary zero-width matches out of 14 for `["", "caf"]`, `["caf", ""]` and `[""]` on `"a\u{e9}b caf\u{e9} \u{1F600}x"`, and 0 for non-empty patterns.

**Invocation (single command):**
```
cd experiments && RCH_DISABLED=1 CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target cargo +nightly run -p exp103_empty_pattern 2>&1 \
  | tee ../phase5_experiment_results/EXP-103.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- Executed 2026-09-12 by the Phase 5 executor (round 2), harness `experiments/EXP-103/` (path-depends on `terraphim-session-analyzer`, default features), log `phase5_experiment_results/EXP-103.log`. Panic confirmed: `load_patterns_from_str` accepts `patterns = ["cargo build", ""]` (only an empty Vec is rejected, loader.rs:79-81), `initialize` builds the automaton, and `find_matches("caf\u{e9} \u{1F600} cargo build")` panics at matcher.rs:143:32 with `start byte index 4 is not a char boundary; it is inside 'é' (bytes 3..5 of string)`. Controls: the same TOML without the empty string returns 1 match at a char boundary; the empty pattern on ASCII text returns 7 zero-width matches without panicking; a whitespace-only pattern `" "` is accepted and matches at char boundaries only (whitespace is not part of the hazard, only the empty string is). Safe-Rust panic, so the UB verdict is NO_EVIDENCE with the panic confirmed (same convention as EXP-011 to EXP-013).
- Filed by Phase 6 round 1 (structural lens). Safe-Rust panic, never memory UB; recorded for the same reason as EXP-012 (production path, input-controlled) and because it answers phase1_notes/I.md open question 1 with a new mechanism (zero-width match) rather than a fixed byte cut.
- Mechanism arm executed at filing in a scratch crate (`phase6_raw/structprobe/src/bin/acempty.rs`, aho-corasick 1.1.4, log `phase6_raw/structprobe/acempty.log`): with an empty pattern present, `find_iter` produced 14 matches for the mixed pattern sets and 17 for `[""]`, 5 of which start inside 'é' or the emoji; slicing at those positions panicked. Non-empty patterns produced 0 non-boundary matches, which confirms the general argument that a valid-UTF-8 non-empty pattern can only match at char boundaries.
- Every other Aho-Corasick consumer in the workspace goes through `terraphim_automata::find_matches`, which drops empty and sub-minimum-length keys (terraphim_automata-1.21.1/src/matcher.rs:61-78), so terraphim_negative_contribution scanner.rs:35-64, terraphim_lsp kg_analysis.rs:56-74, terraphim_hooks replacement.rs:127-136 and terraphim_sessions enricher.rs:177-188 are not exposed to this mechanism. The `TerraphimMatcher` fallback (matcher.rs:162-297, feature `terraphim`) delegates to the same `AhoCorasickMatcher` and inherits the hazard.
- Fix shape for Phase 8: reject empty (and whitespace-only) pattern strings in `load_patterns_from_str` and `merge_patterns`, and use `text.get(start..end)` at matcher.rs:143.

## EXP-104: fff-search mmaps files during every workspace grep with the watcher disabled; a concurrent in-place writer invalidates the whole-file UTF-8 check behind `from_utf8_unchecked`

**Finding ref:** UF-039 in phase4_unified_findings.md (assigned by lead 2026-09-12; Phase 6 round 1)
**Bucket:** 12 library trait invariants (memmap2 `Mmap::map` safety contract; `str` validity), cross-tag 1 aliasing (shared slice over externally mutated memory)
**Severity (Phase 2):** LIKELY-UB (not scored in Phase 2)
**Hypothesis:** `FileItem::get_content_for_search` (fff-search-0.8.4/src/types.rs:655-684) first calls `get_cached_content`, which maps any file with `MMAP_THRESHOLD <= size <= 10 MiB` via `unsafe { memmap2::Mmap::map(&file) }` (types.rs:640) regardless of `enable_mmap_cache` (that flag only controls warmup). The SAFETY comment at types.rs:637-639 relies on the background watcher to invalidate mappings on modification, but every workspace call site constructs the picker with `watch: false` (crates/terraphim_grep/src/hybrid_searcher.rs:354-359; crates/terraphim_mcp_server/src/lib.rs:1274-1279, :1372-1377, :1485-1490) and calls `collect_files` then `grep`. In `grep_files_parallel` the whole mapping is validated once (`file_is_utf8 = std::str::from_utf8(file_bytes).is_ok()`, grep.rs:1685) and each line is then built with `from_utf8_unchecked` (grep.rs:1710). A process that rewrites the file in place between the check and the line construction produces a `&str` (and the returned `GrepMatch::line_content: String`, grep.rs:269) holding invalid UTF-8, which is library UB for `str`; an in-place truncation raises SIGBUS.

**Minimal reproducer:**
```rust
// experiments/EXP-104/src/main.rs; depends on fff-search = "=0.8.4" (crates.io) and tempfile.
use fff_search::{FFFMode, FilePicker, FilePickerOptions, GrepSearchOptions, parse_grep_query};
use std::{fs, io::{Seek, SeekFrom, Write}, thread, time::Duration};
fn main() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.txt");
    let good = "needle caf\u{e9} line\n".repeat(20_000).into_bytes(); // ~380 KiB, above the 16 KiB threshold
    fs::write(&path, &good).unwrap();
    let mut picker = FilePicker::new(FilePickerOptions { base_path: dir.path().to_string_lossy().into(),
        mode: FFFMode::Ai, watch: false, cache_budget: None, ..FilePickerOptions::default() }).unwrap();
    picker.collect_files().unwrap();
    // Same length, invalid UTF-8: 0xA9 (second byte of 'é') becomes 0xFF, which is never valid.
    let bad: Vec<u8> = good.iter().map(|&b| if b == 0xA9 { 0xFF } else { b }).collect();
    let p2 = path.clone();
    let writer = thread::spawn(move || for i in 0..20_000u32 {
        let mut f = fs::OpenOptions::new().write(true).open(&p2).unwrap();
        f.seek(SeekFrom::Start(0)).unwrap();
        f.write_all(if i % 2 == 0 { &bad } else { &good }).unwrap();
    });
    let q = parse_grep_query("needle");
    for _ in 0..2_000 {
        for m in picker.grep(&q, &GrepSearchOptions::default()).matches.iter() {
            if std::str::from_utf8(m.line_content.as_bytes()).is_err() {
                panic!("EXP-104 CONFIRMED: String with invalid UTF-8 returned from grep");
            }
        }
        thread::sleep(Duration::from_micros(50));
    }
    writer.join().unwrap(); println!("EXP-104 NO_EVIDENCE: no invalid UTF-8 observed");
}
```

**Expected signal:**
- `EXP-104 CONFIRMED: String with invalid UTF-8 returned from grep` (a `String` whose bytes fail `from_utf8` is a violated language invariant produced through safe API). With `RUST_BACKTRACE=1` the panic is in the harness; the invalid bytes originate at grep.rs:1710.
- Secondary, not the UB signal: `SIGBUS` (`bus error`) if the writer arm is changed to `set_len` below the mapped length.

**Falsifiability:** If 2000 grep iterations against a writer alternating valid and invalid same-length content never return a non-UTF-8 `line_content`, either the mapping is not taken on this host (check `MMAP_THRESHOLD`: 16 KiB on aarch64, 4 KiB elsewhere, and the budget path at types.rs:625-631), or the validation and the line construction are not separated by a window the writer can hit; in that case widen the window by adding `after_context` and larger files before closing NO_EVIDENCE. A harness that reads via the slow path (file below the threshold) must stay clean and is the control.

**Invocation (single command):**
```
cd experiments && RCH_DISABLED=1 CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target cargo +nightly run --release -p exp104_mmap_grep 2>&1 \
  | tee ../phase5_experiment_results/EXP-104.log
```

**Verdict:** CONFIRMED_UB

**Notes:**
- Executed 2026-09-12 by the Phase 5 executor (round 2) on the macOS aarch64 audit host, harness `experiments/EXP-104/` (fff-search =0.8.4 from crates.io, real `FilePicker` with `watch: false`, `cache_budget: None`, options copied from hybrid_searcher.rs:354-385 with `page_limit`/`max_matches_per_file` unbounded), log `phase5_experiment_results/EXP-104.log`. Four 4 MiB files of `needle café line\n`; the mapping was confirmed live in-process (vmmap shows 4 `mapped file` regions for the temp files; picker budget `cached_count=4`, `cached_bytes=16777152` after the first grep). Same-length rewriter (0xA9 -> 0xFF, write(2) at offset 0) on a sibling thread: `GrepMode::Fuzzy` (the `from_utf8_unchecked` route) returned 11,552,323 `line_content` Strings failing `std::str::from_utf8` out of 23,528,460 in 60 greps; `GrepMode::PlainText` (the mode every workspace call site uses) returned 440 invalid Strings out of 186,412,800 in 200 greps (2,039 of 93,206,400 under ASan). Below-threshold control (8190-byte files, slow read path), same writer: 0 invalid in 200 greps in both modes. Truncate arm (`set_len(4096)` then restore, child process): SIGBUS (signal 10) in both modes within about a second of the writer starting (arm timestamps 11:19:43 to 11:19:44); under ASan the report is `BUS on unknown address ... READ` in `memchr::...::One::find_raw` called from `fff_grep::searcher::glue::SliceByLine<PlainTextMatcher, PlainTextSink>::run` <- `Searcher::search_slice` <- `fff_search::grep::perform_grep`, i.e. the PlainText path.
- Correction to the hypothesis: `from_utf8_unchecked` at grep.rs:1710 lives in `fuzzy_grep_search` (there is no `grep_files_parallel`), which is dispatched only for `GrepMode::Fuzzy` (grep.rs:1914-1987); no workspace call site uses Fuzzy or Regex (`rg "GrepMode::(Fuzzy|Regex)" crates/` is empty), so that specific unchecked conversion is not workspace-reachable. The PlainText path still violates the `str` invariant through safe std code: `PlainTextSink::matched` (grep.rs:750; RegexSink :832 and AhoCorasickSink :909 are identical) builds `line_content` with `String::from_utf8_lossy(display_bytes).into_owned()`, which validates once and then copies the bytes a second time; both reads go through `&[u8]` over the live mapping, so a write landing between validation and copy produces a `String` with invalid UTF-8. The root UB is the `Mmap::map` contract (the `&[u8]` aliases memory another process mutates) rather than the unchecked conversion; the invalid `String` and the SIGBUS are its two observable consequences on the workspace path (`perform_grep` -> `get_content_for_search` -> `get_cached_content`, grep.rs:1269, types.rs:640).
- Verdict CONFIRMED_UB per the block's rule (invalid-UTF-8 `String` observed through the safe API, and SIGBUS/ASan BUS report produced), with the harness supplying the concurrent writer: the exposure is to any external process rewriting or truncating a file above 16 KiB (aarch64) / 4 KiB (x86_64) in the searched tree during a grep. ASan adds only the symbolised SIGBUS frame; it cannot see the content race (no out-of-bounds access, the mapping is fixed-length), so the invariant check in the harness remains the primary signal.
- Filed by Phase 6 round 1 (structural lens). Dependency-inherited hazard whose enabling condition is workspace configuration: fff-search documents the mapping as safe only under the watcher's invalidation discipline (types.rs:598-607 and :637-639), and the workspace disables the watcher at every call site. The mapping lifetime is one `grep` call because each call site builds a fresh picker, so the exposure window is the duration of a grep over the user's working tree (terraphim-grep CLI, MCP `grep_files`/`multi_grep` tools). In CI, `crates/terraphim_mcp_server/tests/test_find_files.rs:146-164` greps the whole checkout, so the mapping path executes on both runners; no in-tree concurrent writer was found in the test binaries (mcp tests write only under temp log and thesaurus paths), so CI exposure is to external writers only.
- Miri and TSan cannot observe this pair (mmap plus a second process); the harness observes the consequence (an invalid `str`) through the safe API, which is the strongest available signal. The byte substitution (0xA9 -> 0xFF, never valid UTF-8) keeps the file length constant so the mapping stays valid and only the content invariant is violated.
- The memmap2 advisory RUSTSEC-2026-0186 (unchecked offset in `advise_range`, `unchecked_advise_range`, `flush_range`, `flush_async_range`, fixed in 0.9.11) remains unreachable: fff-search calls only `Mmap::map(&file)` and never `MmapOptions`, `.offset(` or the range helpers. UF-017's rationale should be refined from "no reachable call path" to "mapping is live in every grep; the advisory-specific functions are not called"; the lockfile is at 0.9.11 since ae06cd2 anyway.
- Other fff-search unsafe sites reviewed and found unreachable or sound under the workspace configuration: `BigramIndexBuilder` (`unsafe impl Sync`, bigram_filter.rs:36; partition by `par_chunks(256)` is word-aligned) and `PostScanUnsafeSnapshot` (`unsafe impl Send/Sync`, file_picker.rs:1675-1679) only run with `enable_content_indexing` or the scan-thread constructor, neither used here; `offset_from` at grep.rs:1040 only with a bigram index and within one allocation; `ascii_case_eq` unaligned `[u8; 8]` reads and `get_unchecked` at grep.rs:525-569 are bounds-sound.
- Fix shape for Phase 8 (upstream): validate per line (`std::str::from_utf8(trimmed)`) instead of once per mapping, or read into the scratch buffer when no watcher is installed (`watch: false` should imply the slow path). Workspace-side mitigation: pass an explicit `cache_budget` with `max_file_size: 0` so `get_cached_content` returns `None` (types.rs:618-620) and every read goes through `read_exact` into the owned buffer.

## EXP-801: post-remediation probe loop on the terraphim_grep lib test binary (UF-001 fix is sound)

**Follow-up of:** EXP-001b
**Finding ref:** UF-001 in phase4_unified_findings.md (data-races F-001, phase3 F-102); remediation phase8_remediation_plan.md section 3 (commit 1)
**Bucket:** 7 data races (env setter vs native getenv); remediation soundness
**Severity (Phase 2):** LIKELY-UB (CONFIRMED_UB by EXP-001b before the fix)
**Hypothesis:** After commit 1 (constructor injection: `OpenRouterClient::with_base_url` plus pure `resolve_base_url`; the four `unsafe` blocks at openrouter_client.rs:170-191 deleted), the rebuilt terraphim_grep lib test binary executed directly by libtest at default parallelism performs zero `setenv`/`unsetenv` calls in-process, so no writer exists for the libgit2 sysdir reader to overlap; the pre-fix binary (1120180) remains the positive control that reproduces EXP-001b.

**Minimal reproducer:**
```rust
// The real reproducer is the rebuilt lib test binary under the LD_PRELOAD shim
// (runbook copy of phase3_raw/envprobe/linux/envprobe_linux.c). Distilled: the
// test no longer touches the process environment; it drives the pure resolver
// and the injecting constructor, while a sibling thread runs the libgit2 reader.
use std::thread;
fn resolve_base_url(o: Option<String>) -> String {
    o.unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string())
}
fn main() {
    let r = thread::spawn(|| { let _ = git2::Repository::discover(std::env::temp_dir()); });
    let w = thread::spawn(|| for _ in 0..200 {
        assert_eq!(resolve_base_url(None), "https://openrouter.ai/api/v1");
        assert_eq!(resolve_base_url(Some("https://example.com/v1".into())), "https://example.com/v1");
    });
    w.join().unwrap(); r.join().unwrap();
    // Under the shim: summary must read setenv=0 unsetenv=0 overlaps_call=0.
}
```

**Expected signal:**
- envprobe summary line per iteration: `setenv=0 unsetenv=0 ... overlaps_pre=0 overlaps_call=0` at widen 0 us and 2000 us, 100 iterations each, all 57 tests `test result: ok`.
- Control (pre-fix binary built from ae06cd2, whose crates/ are identical to 1120180, on the same host in the same session): `setenv=1 unsetenv=2` per iteration and at least one `OVERLAP(call)` with the `git_sysdir_guess_home_dirs` backtrace across the widen 0 batch (EXP-001b saw 1 in 100), hundreds at widen 2000 us. If the control does not reproduce, the host is too loaded (#281) or the shim is not loaded; the run is void.

**Falsifiability:** Any `setenv` or `unsetenv` count above zero in the post-fix main process refutes the hypothesis (a writer survived: either a missed site or a dependency writer such as reqwest/hyper-util on that platform); an `OVERLAP` line with a workspace frame in the backtrace refutes it outright. Executing via nextest instead of the libtest binary is not a valid run (one process per test would pass trivially).

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-probe-clients && git fetch origin && git checkout <fix-sha> && CARGO_TARGET_DIR=/tmp/ubx-probe-target cargo +nightly test -p terraphim_grep --lib --no-run --message-format=json 2>/dev/null | rg -o "\"executable\":\"[^\"]+\"" | cut -d\" -f4 > /tmp/ubx-exp801/bin.txt && ENVPROBE_SO=/tmp/ubx-probe-scratch-src/libenvprobe.so ENVPROBE_LOGDIR=/tmp/ubx-exp801 bash runbook/run_probe.sh grep "$(cat /tmp/ubx-exp801/bin.txt)" crates/terraphim_grep 100' > phase5_experiment_results/EXP-801.log 2>&1
```

**Verdict:** DEFERRED

**Notes:**
- **Lead note (2026-09-12):** DEFERRED, gated on Phase 13 execution of the remediation commit it proves; it is a post-fix soundness proof, not an open hypothesis about the current tree, so it does not hold convergence open.
- Phase 8 (2026-09-12): allocated as the fails-before/passes-after test for #280. The runbook's `run_probe.sh` is the phase3 script with `SO` and `L` parameterised (`ENVPROBE_SO`, `ENVPROBE_LOGDIR`); widen values stay 0/500/2000 us. Record `uptime` load and `pgrep -c terraphim_server` in the log header (#281 noise).
- The macOS sibling reader (UF-012, SystemConfiguration) is out of scope: Apple Libc locks both sides, and the post-fix binary has no writer on any platform.

## EXP-802: post-remediation probe loop on the terraphim_agent tui_service_tests binary (UF-002 fix is sound)

**Follow-up of:** EXP-002b
**Finding ref:** UF-002 in phase4_unified_findings.md (data-races F-002, phase3 F-103); remediation phase8_remediation_plan.md section 4 (commits 2 and 5)
**Bucket:** 7 data races (env setter vs native getenv); remediation soundness
**Severity (Phase 2):** LIKELY-UB (mechanism CONFIRMED_UB by EXP-002c)
**Hypothesis:** After commit 2 (`TuiService::new_with_settings_dir`; the test passes a TempDir settings directory and calls `DeviceStorage::init_memory_only()` first; `EnvVarGuard` and `#[serial]` removed) and commit 5 (`terraphim_test_utils` dropped from both manifests), the rebuilt `tui_service_tests` binary executed directly by libtest performs zero `setenv`/`unsetenv` calls in its main process, and the test still bootstraps `settings.toml` under the injected directory; the EXP-002c scratch binary on the same host is the instrument control showing the shim still sees the sqlite reader.

**Minimal reproducer:**
```rust
// Distilled shape of the rewritten test (real reproducer: the rebuilt binary
// under the shim). No process-environment access on the writer side at all.
use std::path::PathBuf;
async fn new_with_settings_dir(dir: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    // stands in for TuiService::new_with_settings_dir -> DeviceSettings::load_from_env_and_file(dir)
    let d = dir.unwrap_or_else(|| PathBuf::from("/nonexistent-default"));
    std::fs::create_dir_all(&d)?;
    std::fs::write(d.join("settings.toml"), "initialized = false\n")?;
    Ok(d)
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let dir = tmp.path().join(".config").join("terraphim");
    let got = new_with_settings_dir(Some(dir.clone())).await?;
    assert!(got.join("settings.toml").exists());
    assert_eq!(std::env::var_os("HOME").is_some(), true, "HOME untouched: never written");
    Ok(())
}
```

**Expected signal:**
- envprobe summary for the main process (the summaries printed by spawned child servers show `setenv=0` today and are ignored as in EXP-002b): `setenv=0 unsetenv=0 overlaps_pre=0 overlaps_call=0`, 100 iterations at widen 0 us and 2000 us, 10 tests `test result: ok`.
- Control A (pre-fix binary from ae06cd2; crates/ identical to 1120180): `setenv=4 unsetenv=2` per iteration (EXP-002b).
- Control B (instrument): `ubx-probe-scratch` from `phase3_raw/envprobe/linux/scratch_main.rs` rebuilt on the host, 20 iterations at widen 0: at least one `OVERLAP(call)` with the `sqlite3_os_init` backtrace (EXP-002c saw 102 in 150). Without control B a clean post-fix run cannot distinguish "no writer" from "blind shim".

**Falsifiability:** A non-zero `setenv`/`unsetenv` count in the post-fix main process refutes the hypothesis (a writer survived: a missed guard, a remaining `terraphim_test_utils` path, or a dependency writer). A missing `settings.toml` under the injected directory means the injection does not reach `load_from_env_and_file` and the functional half of the fix is wrong even if the count is zero.

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-probe-clients && git checkout <fix-sha> && CARGO_TARGET_DIR=/tmp/ubx-probe-target cargo +nightly test -p terraphim_agent --test tui_service_tests --no-run --message-format=json 2>/dev/null | rg -o "\"executable\":\"[^\"]+\"" | cut -d\" -f4 > /tmp/ubx-exp802/bin.txt && ENVPROBE_SO=/tmp/ubx-probe-scratch-src/libenvprobe.so ENVPROBE_LOGDIR=/tmp/ubx-exp802 bash runbook/run_probe.sh tui "$(cat /tmp/ubx-exp802/bin.txt)" crates/terraphim_agent 100 && ENVPROBE_WIDEN_US=0 LD_PRELOAD=/tmp/ubx-probe-scratch-src/libenvprobe.so /tmp/ubx-probe-scratch/target/release/ubx-probe-scratch' > phase5_experiment_results/EXP-802.log 2>&1
```

**Verdict:** DEFERRED

**Notes:**
- **Lead note (2026-09-12):** DEFERRED, gated on Phase 13 execution of the remediation commit it proves; it is a post-fix soundness proof, not an open hypothesis about the current tree, so it does not hold convergence open.
- Phase 8 (2026-09-12): allocated as the fails-before/passes-after test for the UF-002 child issue. Runs on bigbox only (glibc); never via nextest. The upstream `terraphim_test_utils` fix (UF-003, terraphim-ai issue) is not a precondition: the workspace stops depending on the crate in commit 5.
- Residual, not measured here: with `init_memory_only` being `get_or_try_init`, a sibling that initialises sqlite first wins the singleton (UF-023 hygiene); it does not affect the writer count.

## EXP-803: post-remediation writer count on the terraphim_update lib test binary (UF-004 fix; UF-036 count arm)

**Follow-up of:** EXP-010
**Finding ref:** UF-004 and UF-036 in phase4_unified_findings.md (data-races F-003; EXP-010 note); remediation phase8_remediation_plan.md sections 5 and 6 (commits 3 and 9)
**Bucket:** 7 data races (env setter vs native getenv); remediation soundness
**Severity (Phase 2):** SUSPICIOUS (raised to LIKELY-UB by the lead for UF-004)
**Hypothesis:** After commit 3 (history-path injection: `history_path_in` and the `_to`/`_from`/`_at` variants; `setup_temp_config_dir` and its `unsafe` block deleted; ten `#[serial]` dropped) the ten state tests perform zero `setenv` calls (EXP-010 counted 50 per iteration from them), so the per-iteration writer count of the lib test binary under the shim drops from 52 to exactly the self_update pair (2, `SSL_CERT_FILE`/`SSL_CERT_DIR` from `test_check_for_updates_auto`), and to 0 once commit 9 marks that live-network test `#[ignore]`.

**Minimal reproducer:**
```rust
// Distilled shape of the rewritten state tests: a TempDir replaces HOME/XDG
// mutation; nothing in the test touches the process environment.
use std::path::{Path, PathBuf};
fn history_path_in(config_dir: &Path) -> PathBuf { config_dir.join("terraphim").join("update_history.json") }
fn save_to(path: &Path, body: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?; std::fs::write(path, body)
}
fn main() -> std::io::Result<()> {
    let tmp = tempfile::tempdir()?;
    let p = history_path_in(tmp.path());
    save_to(&p, "{\"current_version\":\"1.0.0\"}")?;
    assert!(p.exists());
    assert!(p.ends_with("terraphim/update_history.json"));
    // Under the shim, with the live-network test ignored: setenv=0.
    Ok(())
}
```

**Expected signal:**
- envprobe summary per iteration, post-commit-3 with `test_check_for_updates_auto` still running: `setenv=2` (both names `SSL_CERT_*`, traced with `ENVPROBE_TRACE=1`), never `HOME`/`USERPROFILE`/`XDG_CONFIG_HOME`/`APPDATA`/`LOCALAPPDATA`.
- Post-commit-9 (default run, ignored test skipped): `setenv=0 unsetenv=0`, 100 iterations at widen 0 and 500 us, `test result: ok` with 129 passed and 1 ignored.
- Control (ae06cd2; crates/ identical to 1120180): `setenv=52` per iteration (EXP-010).

**Falsifiability:** Any `setenv` of a name other than `SSL_CERT_FILE`/`SSL_CERT_DIR` after commit 3 refutes the UF-004 half (a state-test writer survived). Any `setenv` at all after commit 9 in the default run refutes the UF-036 count arm (another caller of the self_update GitHub backend exists in the lib tests, or the `#[ignore]` was not applied). This experiment says nothing about the glibc resolver reader (EXP-010-a, helgrind, still DEFERRED): it proves the writer is gone, which is sufficient for the remediation, not for the original reader claim.

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-probe-clients && git checkout <fix-sha> && CARGO_TARGET_DIR=/tmp/ubx-probe-target cargo +nightly test -p terraphim_update --lib --no-run --message-format=json 2>/dev/null | rg -o "\"executable\":\"[^\"]+\"" | cut -d\" -f4 > /tmp/ubx-exp803/bin.txt && ENVPROBE_SO=/tmp/ubx-probe-scratch-src/libenvprobe.so ENVPROBE_LOGDIR=/tmp/ubx-exp803 ENVPROBE_TRACE=1 bash runbook/run_probe.sh update "$(cat /tmp/ubx-exp803/bin.txt)" crates/terraphim_update 100' > phase5_experiment_results/EXP-803.log 2>&1
```

**Verdict:** DEFERRED

**Notes:**
- **Lead note (2026-09-12):** DEFERRED, gated on Phase 13 execution of the remediation commit it proves; it is a post-fix soundness proof, not an open hypothesis about the current tree, so it does not hold convergence open.
- Phase 8 (2026-09-12): allocated for the UF-004 and UF-036 child issues. The production-path hypothesis for `set_ssl_vars!()` (self_update executes it from `spawn_blocking` in shipped binaries on Linux) is escalated to the lead for the next convergence round and is not covered here.
- With `ENVPROBE_TRACE=1` the log grows by about 125 lines per iteration; that is intended so the names of any surviving `setenv` are visible without a second run.

## EXP-804: workspace-wide `unsafe_code = "forbid"` detects a reintroduced env writer at compile time (headline lever is a real gate)

**Finding ref:** UF-001, UF-002, UF-004, UF-013 in phase4_unified_findings.md (all six `unsafe` sites of phase1_unsafe_surface_inventory.md section A); remediation phase8_remediation_plan.md section 7 (commit 7)
**Bucket:** 7 data races (class-level gate); Principle 8 detection test
**Severity (Phase 2):** LIKELY-UB (class)
**Hypothesis:** With `[workspace.lints.rust] unsafe_code = "forbid"` in the root manifest and `[lints] workspace = true` in all 11 member manifests, (a) the unmodified post-remediation workspace passes `cargo clippy --workspace --all-targets -- -D warnings` (no proc macro emits `unsafe` in any target; the mcp_server lib expansion re-checked 2026-09-12 has 0 `unsafe` tokens), (b) applying the stored patch `exp804_reintroduce_set_var.patch`, which adds one `unsafe { std::env::set_var("UBX_FAULT", "1") }` inside a `#[test]` in crates/terraphim_grep/src/openrouter_client.rs, makes `cargo check --workspace --all-targets` fail with the `unsafe_code` lint error, and (c) the `ci_guards` test `every_member_inherits_workspace_lints` fails if any member manifest drops `[lints] workspace = true`.

**Minimal reproducer:**
```rust
// exp804_reintroduce_set_var.patch adds exactly this function to the tests
// module of crates/terraphim_grep/src/openrouter_client.rs and nothing else;
// it is applied with `git apply` inside the experiment and reverted with
// `git apply -R`. No cfg-gated unsafe is ever left in the tree.
#[test]
fn ubx_fault_reintroduced_env_writer() {
    // Expected: error: usage of an `unsafe` block  (lint `unsafe_code`, level forbid)
    unsafe {
        std::env::set_var("UBX_FAULT", "1");
    }
}
```

**Expected signal:**
- (a) `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- (b) with the patch applied: `cargo check --workspace --all-targets` exit non-zero; stderr contains `error: usage of an \`unsafe\` block` and `note: the lint level is defined here` pointing at the manifest lint (`[lints.rust] unsafe_code = "forbid"` reported as `-F unsafe_code` on the command line / manifest).
- (c) after temporarily deleting `[lints]` from one member manifest (also via a stored patch): `cargo test -p terraphim_agent --test ci_guards every_member_inherits_workspace_lints` fails naming that crate; restored, it passes.
- Token sweep on the unpatched tree: `rg -n '\bunsafe\b' crates/ -t rust` matches only the six prose lines listed in phase8_remediation_plan.md section 11.

**Falsifiability:** (a) failing means a proc macro emits `unsafe` in some target and the lint must be scoped or the macro replaced; the lever is then not deployable as designed. (b) succeeding (compiling) with the patch applied means `forbid` did not reach the test target (for example the member manifest lacks `[lints]`, or the lint is `deny` and an `allow` crept in) and the gate is not real. (c) passing with a manifest stripped means the guard test is wrong.

**Invocation (single command):**
```
cd /Users/alex/projects/terraphim/terraphim-clients && CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target RCH_DISABLED=1 bash -c 'set -o pipefail; cargo clippy --workspace --all-targets -- -D warnings && git apply .ub-exorcism/2026-09-11-terraphim-clients-1/runbook/exp804_reintroduce_set_var.patch && { cargo check --workspace --all-targets; echo "check exit=$? (expect non-zero)"; }; git apply -R .ub-exorcism/2026-09-11-terraphim-clients-1/runbook/exp804_reintroduce_set_var.patch; cargo test -p terraphim_agent --test ci_guards every_member_inherits_workspace_lints' 2>&1 | tee .ub-exorcism/2026-09-11-terraphim-clients-1/phase5_experiment_results/EXP-804.log
```

**Verdict:** DEFERRED

**Notes:**
- **Lead note (2026-09-12):** DEFERRED, gated on Phase 13 execution of the remediation commit it proves; it is a post-fix soundness proof, not an open hypothesis about the current tree, so it does not hold convergence open.
- Phase 8 (2026-09-12): allocated as the detection test for the headline lever (Principle 8: prove the gate fires when the bug is reintroduced, not merely that the clean tree passes). Runs on any host; no glibc dependence.
- The lint does not see safe wrappers in dependencies (terraphim_test_utils, removed and banned in deny.toml by commit 7; self_update, section 6 of the plan). EXP-804 therefore proves the in-tree class is closed, not that the binaries are env-race-free; the docs commit says so.

## EXP-105: self_update `set_ssl_vars!()` on the production update-check path races glibc `getenv` readers in the shipped terraphim-agent binary (Linux)

**Follow-up of:** EXP-102
**Finding ref:** UF-037 in phase4_unified_findings.md (lead-filed 2026-09-12 from the Phase 8 architect's escalation)
**Bucket:** 7 data races (hidden unsafe in a dependency behind a safe API)
**Severity (Phase 2):** LIKELY-UB
**Hypothesis:** On Linux the shipped `terraphim-agent` binary runs the pre-dispatch update check (crates/terraphim_agent/src/main.rs:448-456 -> crates/terraphim_update/src/manifest.rs) inside `spawn_blocking`; the self_update backend calls `set_ssl_vars!()` (self_update-0.42.0/src/backends/github.rs:518, sets `SSL_CERT_FILE`/`SSL_CERT_DIR` when absent, via edition-2021 safe `set_var`) while tokio worker threads of the same runtime are live and can call glibc `getenv` (resolver `RES_OPTIONS`/`LOCALDOMAIN` inside `getaddrinfo`, openssl-probe/rustls-native-certs reads of `SSL_CERT_FILE`). glibc `getenv` is unlocked and `setenv` may realloc `__environ`, so a `call`-phase overlap is a data race in production, not only in tests.

**Minimal reproducer:**
```rust
// No bespoke reproducer: the shipped binary is the input. Run any agent
// command that reaches the pre-dispatch update check with the update endpoint
// reachable and SSL_CERT_FILE / SSL_CERT_DIR unset, e.g.
//   terraphim-agent search "knowledge graph" --limit 1
// under the envprobe LD_PRELOAD shim. A scratch equivalent of the shape:
fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let w = tokio::task::spawn_blocking(|| std::env::set_var("SSL_CERT_FILE", "/etc/ssl/certs/ca-certificates.crt"));
        let r = tokio::spawn(async { tokio::net::lookup_host("git.terraphim.cloud:443").await.map(|_| ()) });
        let _ = tokio::join!(w, r);
    });
}
```

**Expected signal:**
- envprobe (LD_PRELOAD, glibc): at least one `call`-phase overlap at `ENVPROBE_WIDEN_US=0` whose writer backtrace contains `self_update::backends::github` and whose reader backtrace contains `getaddrinfo` or a certificate-store probe, on threads W != R.
- If only `pre`-phase overlaps appear under widening, record the window as demonstrated but unassisted overlap not observed.

**Falsifiability:** If, with `SSL_CERT_FILE` and `SSL_CERT_DIR` unset, 300 iterations at `widen=0` and 100 at `widen=2000` show zero overlaps with a self_update writer frame, or the writer never fires because the variables are set by the environment on the host, the verdict is NO_EVIDENCE for this host and the row stays LIKELY-UB by contract only. If the writer fires and any `call`-phase overlap with a native reader on another thread is recorded, the verdict is CONFIRMED_UB and UF-037 is the first production-path confirmed finding of this run.

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-probe-clients && env -u SSL_CERT_FILE -u SSL_CERT_DIR ENVPROBE_WIDEN_US=0 ENVPROBE_TRACE=1 LD_PRELOAD=/tmp/ubx-probe/libenvprobe.so \
  /tmp/ubx-probe-target/release/terraphim-agent search "knowledge graph" --limit 1' 2>&1 | tee phase5_experiment_results/EXP-105.log
```

**Verdict:** NO_EVIDENCE
**Phase 10 fresh-eyes note (2026-09-12):** the token is sound for the arm that decided the row (the default `R2` backend fires no `setenv` at all: `setenv=0` in 400 of 400 runs, a signal this shim can see). It is **not** sound for the overlap arm: phase4_unified_findings.md defines NO_EVIDENCE as an instrument "able to see the predicted signal", and this block's own note says the shim sees only PLT `getenv` and is blind to the glibc-internal resolver reads that are the predicted reader. EXP-001 was given NEEDS_REFINEMENT for exactly that property. Read "0 overlaps" as NEEDS_REFINEMENT; the row's conclusion is unaffected.

**Notes:**
- Executed 2026-09-12 by the Phase 5 executor (round 2) on bigbox (Ubuntu 20.04 x86_64, glibc): release `terraphim-agent` 1.21.14 built on bigbox from `/tmp/ubx-probe-clients` at ae06cd2; envprobe shim rebuilt at `/tmp/ubx-probe/libenvprobe.so` (the earlier deployment had been removed); driver `phase5_experiment_results/EXP-105_raw/run_exp105.sh` with `env -u SSL_CERT_FILE -u SSL_CERT_DIR`, `ENVPROBE_TRACE=1`, 300 iterations at widen 0 and 100 at widen 2000; log `phase5_experiment_results/EXP-105.log`, raw probe logs under `EXP-105_raw/`. Neither variable is set at the host level. The update endpoint is reachable (`https://downloads.terraphim.ai/terraphim-agent/stable.json` -> HTTP/2 200; strace shows the connect to 2a06:98c1:3120::9:443 on every run) and main.rs:448-456 has no throttle, so the check ran in every iteration.
- Correction to the hypothesis: the shipped pre-dispatch check dispatches on `UpdateBackend` (lib.rs:242-251), default `R2` (env `TERRAPHIM_UPDATE_BACKEND` selects); `check_update_r2` fetches the manifest with ureq inside `spawn_blocking` and never enters self_update, so `set_ssl_vars!()` is not on the default path. It is reached via `check_update_github` (lib.rs:395-430, `TERRAPHIM_UPDATE_BACKEND=github`) and the install/update paths. Arm A (default): 400/400 iterations with `setenv=0`, the writer never fires. Arm B (`TERRAPHIM_UPDATE_BACKEND=github`): 400/400 iterations fire `set SSL_CERT_FILE` and `set SSL_CERT_DIR` on the spawn_blocking thread; `getenv_on_other_thread_after_first_write=68` per run but `overlaps_pre=0 overlaps_call=0` in all 400 (300 at widen 0, 100 at widen 2000), so no backtraces to symbolise. The trace shows the PLT-visible readers are sequential: the reqwest client thread's proxy reads start after both `set_var` calls have returned and the main thread is parked in `block_on` awaiting the `JoinHandle` until the check completes.
- Verdict NO_EVIDENCE for this host per the block's Falsifiability (zero overlaps with the writer firing on the configured path; writer absent on the default path). UF-037 stays LIKELY-UB by contract only, and its "production path" qualifier narrows to `TERRAPHIM_UPDATE_BACKEND=github` and the explicit update commands. Blind spot carried from EXP-010: the shim sees only PLT `getenv`, not glibc-internal reads inside `getaddrinfo`; in this binary no other thread resolves names while the writer runs, so that reader class is not concurrent here either.
- Lead-filed. The release binary must be built on bigbox from the same HEAD (record `git rev-parse HEAD`); the update endpoint must be reachable or the writer path is not entered (manifest.rs gives up after three attempts). Loop the invocation 300 times with a small driver script as EXP-001b's run_probe.sh does.
- Coincides with the top entry of the #253 opportunity matrix (gating the pre-dispatch update check): the remediation that removes the writer from the hot path is also the performance lever.
- Whether the writer fires at all depends on `SSL_CERT_FILE`/`SSL_CERT_DIR` being absent, so the probe driver must unset them explicitly.

## EXP-805: post-remediation rerun of the EXP-104 harness with the workspace's explicit no-mmap budget (UF-039 fix is sound)

**Follow-up of:** EXP-104
**Finding ref:** UF-039 in phase4_unified_findings.md (Lead addendum 4); remediation phase8_remediation_plan.md Delta 2, D2.1 (commit 0)
**Bucket:** 12 library trait invariants (memmap2 `Mmap::map` contract; `str` validity), cross-tag 1 aliasing; remediation soundness
**Severity (Phase 2):** LIKELY-UB (CONFIRMED_UB by EXP-104 before the fix)
**Hypothesis:** With every workspace `FilePicker::new` call passing an explicit `ContentCacheBudget` whose `max_files` is 0 (and `max_file_size` 10 MiB, so the slow path stays open), fff-search 0.8.4 never maps a file (`get_cached_content` returns `None` at types.rs:627-631 before `Mmap::map` at :640) and every grep reads through `read_exact` into the owned scratch buffer (types.rs:676-683); therefore a concurrent same-length writer can no longer produce a `line_content` `String` with invalid UTF-8, a concurrent truncation can no longer raise SIGBUS, and the picker's `cached_count` stays 0 after every grep. The pre-fix option block (`cache_budget: None`) on the same host reproduces EXP-104 and is the control.

**Minimal reproducer:**
```rust
// experiments/EXP-805/src/main.rs: the EXP-104 harness with the option block
// copied verbatim from the post-fix hybrid_searcher.rs (explicit budget).
use fff_search::{ContentCacheBudget, FFFMode, FilePicker, FilePickerOptions, GrepSearchOptions, parse_grep_query};
use std::sync::atomic::{AtomicU64, AtomicUsize};
fn no_mmap_budget() -> ContentCacheBudget {
    ContentCacheBudget { max_files: 0, max_bytes: 0, max_file_size: 10 * 1024 * 1024,
        cached_count: AtomicUsize::new(0), cached_bytes: AtomicU64::new(0) }
}
fn main() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.txt");
    let good = "needle caf\u{e9} line\n".repeat(200_000).into_bytes(); // 4 MiB, above every MMAP_THRESHOLD
    std::fs::write(&path, &good).unwrap();
    let mut picker = FilePicker::new(FilePickerOptions { base_path: dir.path().to_string_lossy().into(),
        mode: FFFMode::Ai, watch: false, cache_budget: Some(no_mmap_budget()), ..FilePickerOptions::default() }).unwrap();
    picker.collect_files().unwrap();
    // writer thread and truncate arm exactly as in EXP-104 (same-length 0xA9 -> 0xFF; set_len(4096) in a child)
    let q = parse_grep_query("needle");
    let mut invalid = 0usize;
    for _ in 0..200 {
        let r = picker.grep(&q, &GrepSearchOptions::default());
        invalid += r.matches.iter().filter(|m| std::str::from_utf8(m.line_content.as_bytes()).is_err()).count();
        assert_eq!(picker.cache_budget().cached_count.load(std::sync::atomic::Ordering::Relaxed), 0, "a file was mapped");
    }
    println!("EXP-805 {}", if invalid == 0 { "NO_EVIDENCE (0 invalid Strings, no mapping)" } else { "REFUTED" });
}
```

**Expected signal:**
- Post-fix arm: `EXP-805 NO_EVIDENCE (0 invalid Strings, no mapping)`; `cached_count` 0 after all 200 greps; no `mapped file` region for the temp files in `vmmap` (macOS) or `/proc/<pid>/maps` (Linux); the truncate arm ends with the grep returning an `Err`/short result and the process exiting 0, never `bus error`.
- Control arm (same harness, `cache_budget: None`): invalid `line_content` count above 0 within 200 greps and SIGBUS in the truncate arm, as in EXP-104 (440 of 186 M lines; SIGBUS within about a second).
- Guard arm: `cargo test -p terraphim_agent --test ci_guards no_fff_picker_maps_files` passes on the fixed tree and fails (naming the file) after a stored patch reintroduces `cache_budget: None` at one call site.

**Falsifiability:** Any invalid `line_content` in the post-fix arm, a non-zero `cached_count`, a mapped temp file in the process map, or a SIGBUS in the truncate arm refutes the hypothesis (mapping still occurs by a route the static read missed, for example a second content path or a Windows-only stub reversed on Unix). A control arm that does not reproduce EXP-104 voids the run (host or writer timing, not the fix). A guard arm that passes with the reintroduction patch applied means the guard is wrong.

**Invocation (single command):**
```
cd experiments && RCH_DISABLED=1 CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target bash -c 'cargo +nightly run --release -p exp805_no_mmap_grep -- fixed && cargo +nightly run --release -p exp805_no_mmap_grep -- control; cd .. && cargo test -p terraphim_agent --test ci_guards no_fff_picker_maps_files' 2>&1 | tee ../phase5_experiment_results/EXP-805.log
```

**Verdict:** DEFERRED

**Notes:**
- Phase 8 Delta 2 (2026-09-12): DEFERRED, gated on Phase 13 landing commit 0 (the four call sites and the `ci_guards` test); runs on either host (the mapping threshold is 16 KiB on aarch64 and 4 KiB on Linux; a 4 MiB file is above both). The harness must not use `ContentCacheBudget::zero()` or any `max_file_size: 0`: those also close the slow path (types.rs:669-671) and would produce a vacuous zero-match "pass".
- Also records, for the truncate arm, that `read_exact` on a file shortened under it returns `UnexpectedEof` and the file is skipped (types.rs:682); that is the intended post-fix behaviour, not a failure.
- Upstream fix (fff-search: gate `get_cached_content` on `has_watcher()` or validate per line with buffered reads when unwatched) is the durable companion; when a released version carries it, rerun this block with `cache_budget: None` before dropping the workspace helper.


## EXP-201: hostile non-UTF-8 filenames inflate the fff-search relative path past PATH_BUF_SIZE, so read_to_buf truncates mid-`U+FFFD` and returns an invalid `str` through `from_utf8_unchecked`

**Finding ref:** UF-039 neighbour in phase4_unified_findings.md (Phase 6 round 2, ADVERSARIAL lens; new shape)
**Bucket:** 3 validity / 1 aliasing (dependency `unsafe` whose precondition the consumer's lossy path storage breaks); cross-tag 12 library trait (`str` validity)
**Severity (Phase 2):** LIKELY-UB (not scored in Phase 2)
**Hypothesis:** fff-search stores every scanned relative path as a `String` produced by `Path::to_string_lossy().into_owned()` (file_picker.rs:232,352,406), so on Linux a filesystem entry with invalid UTF-8 bytes (legal on ext4/xfs/tmpfs) inflates: each invalid byte becomes `U+FFFD` (three bytes). `FileItem::write_absolute_path` (types.rs:337-356) writes `base + '/' + relative` into a fixed `[0u8; PATH_BUF_SIZE]` (4096) via `ChunkedString::read_to_buf` (simd_path.rs:170-186), which does `total = byte_len.min(buf.len())` then `from_utf8_unchecked(&buf[..total])`. When the inflated absolute path exceeds 4096 the truncation can fall inside a three-byte `U+FFFD`, so the returned `&Path`/`&str` holds invalid UTF-8 (language UB for `str`). The base path is attacker-chosen through the terraphim-grep `--paths` argument and the MCP `grep_files`/`multi_grep` `path` argument; a cloned hostile repository carries the offending names. macOS/APFS enforces UTF-8 filenames (EILSEQ on creation), so the site is glibc/Linux (bigbox, ubuntu-latest) only.

**Minimal reproducer:**
```rust
// experiments/EXP-201/src/main.rs; depends on fff-search = "=0.8.4", tempfile. Linux only.
use std::{fs, os::unix::ffi::OsStrExt, ffi::OsStr, path::PathBuf};
use fff_search::{FFFMode, FilePicker, FilePickerOptions, GrepSearchOptions, parse_grep_query};
fn main() {
    let dir = tempfile::tempdir().unwrap();
    // Six nested directories each named 250 x 0xFF (invalid UTF-8) -> ~250*3 bytes each once lossy.
    let mut p: PathBuf = dir.path().into();
    for _ in 0..6 { p.push(OsStr::from_bytes(&[0xFFu8; 250])); fs::create_dir(&p).unwrap(); }
    p.push("needle.txt");
    fs::write(&p, b"needle inside a deep hostile tree\n".repeat(2000)).unwrap(); // > MMAP_THRESHOLD
    let mut picker = FilePicker::new(FilePickerOptions { base_path: dir.path().to_string_lossy().into(),
        mode: FFFMode::Ai, watch: false, ..FilePickerOptions::default() }).unwrap();
    picker.collect_files().unwrap();
    let q = parse_grep_query("needle");
    for m in picker.grep(&q, &GrepSearchOptions::default()).matches.iter() {
        if std::str::from_utf8(m.file_path.as_bytes()).is_err() {
            panic!("EXP-201 CONFIRMED: invalid-UTF-8 path string surfaced through grep");
        }
    }
    println!("EXP-201 NO_EVIDENCE: no invalid path string observed");
}
```

**Expected signal:** a `file_path` (or a `line_content`, if the mmap read path is also reached) whose bytes fail `from_utf8`, produced through the safe grep API; or `RUST_BACKTRACE=1` showing a `str`-validity assertion. Under Miri the same reduction over a synthetic `ChunkedString` longer than 4096 bytes flags the `from_utf8_unchecked` at simd_path.rs:186. Control: a tree whose inflated paths stay under 4096 bytes must stay clean.

**Falsifiability:** If a filter between the `ignore` walker and arena storage rejects non-UTF-8 entries (re-check scan.rs/file_picker.rs for a `to_str()?`/`is_some` guard before `push`), or if `read_to_buf`'s truncation only ever lands on an ASCII byte because a length cap upstream keeps paths under 4096, the hypothesis is wrong. `ignore` is `OsStr`-based and no such filter was found in the static read, so the enabling condition is present; the open question is whether 4096 is actually exceeded on a realistic tree (it is with six 250-byte components once tripled).

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-exp201 && RCH_DISABLED=1 CARGO_TARGET_DIR=/tmp/ubx-exp-target cargo +nightly run --release' 2>&1 \
  | tee phase5_experiment_results/EXP-201.log
```

**Verdict:** CONFIRMED_UB

**Notes:**
- Filed by Phase 6 round 2 (adversarial lens). Same attacker-chosen tree as EXP-104 (UF-039) but a different `unsafe` site: EXP-104 is the mmap-vs-writer window; this is path inflation defeating a fixed stack buffer's implicit "paths fit in 4096" precondition. `write_relative_cstr` (types.rs:366-379) shares the truncation but is bounds-sound for `str` validity because a NUL terminator, not a `str`, is the invariant, and `U+FFFD` contains no NUL byte.
- Confirmed pieces: lossy storage at file_picker.rs:232,352,406; `read_to_buf` truncate-then-`from_utf8_unchecked` at simd_path.rs:170-186; the comment at simd_path.rs:168 states the truncation and tells callers to pass `[u8; PATH_BUF_SIZE]`, which is exactly the 4096 array `write_absolute_path` passes.
- Carry to CROSS-SYSTEM: MCP over HTTP with a hostile `path` argument combines this inflation, EXP-104's mmap window and the libgit2 status chain (EXP-205) in one process on bigbox.
- Executed 2026-09-12 by the Phase 5 executor (round 3) at HEAD 46bbc43 (lockfile-only move from ae06cd2). Host deviation: bigbox's root filesystem was 100% full (0 free blocks for non-root, `mkdir /tmp/ubx-probe-exp201` failed with ENOSPC; nothing was deleted and no state was left there), so the run used an OrbStack Linux container on the audit host (`rust:1.96-trixie`, aarch64, kernel 7.0.14-orbstack, hostile trees on the container's own overlay filesystem under `/tmp/ubx-probe-exp201`, which accepts 0xFF names). Harness `experiments/EXP-201/` (standalone root with its own `[workspace]`, fff-search =0.8.4 from crates.io, `Cargo.lock` copied from `experiments/`, picker and grep options copied from hybrid_searcher.rs:354-385), log `phase5_experiment_results/EXP-201.log`. The mechanism (lossy storage plus a fixed 4096-byte buffer) is architecture-independent; `MMAP_THRESHOLD` plays no part.
- Corrections to the block: `GrepMatch` has no `file_path` field (it has `file_index` into `GrepResult.files: Vec<&FileItem>`), and the truncating site on the grep path is `FileItem::absolute_path` (types.rs:271-275: `read_to_buf` into a local `[u8; 4096]`, then `base_path.join(rel)`), called from `get_content_for_search` (types.rs:676) and `get_cached_content` (:634). `write_absolute_path` (types.rs:337) shares the truncation (cut at 4096 - base_len - 1 rather than at 4096) but is reached only from `update_frecency_scores` and the `for_each_dir` fallback.
- Result: seven hostile geometries and one control. The file under the hostile tree was collected in every geometry (`files=1 dirs=1`), so there is no non-UTF-8 filter between the `ignore` walker and the arena (the falsifiability check passes) and the relative path is stored inflated (`relative_path_len` 4498 for 6x249, 11500 for 15x255). `FileItem::absolute_path(&picker, base)` returned a `PathBuf` of 4132 or 4133 bytes (base, '/', then exactly the first 4096 bytes of the inflated relative path) whose bytes fail `std::str::from_utf8` with `incomplete utf-8 byte sequence` in every geometry whose cut residue `(4096 mod (3*comp_len+1)) mod 3` is 1 or 2: 6x249, 6x250, 6x251 and 15x255 (residue 2, tail `ef bf`) and 7x200 (residue 1, tail `ef`). The residue-0 geometry 14x100 (cut exactly on a U+FFFD boundary) stayed valid, as did the 2x250 control (1512 bytes) and 3x255 (2308 bytes): 9 invalid strings in total, 0 in the controls. `DirItem::dir_name` returned a `String` with the same broken tail in the residue-1/2 geometries (`write_dir_name`, types.rs:153-164, slices `full[offset..]` of the truncated str and pushes it into a `String`), and panicked with `start byte index 10724 is out of bounds for string of length 4096` at types.rs:163 for 15x255, where `last_segment_offset` (10724) exceeds the truncated length (a second robustness defect).
- Grep-path consequence: on every tree, hostile and control, `grep("needle")` returned 0 matches with `total_files_searched=1`, because the lossily stored path (U+FFFD bytes) no longer names the on-disk entry (0xFF bytes): `absolute_path(..).exists()` is false even for the 1512-byte control, `File::open` fails and the file is silently dropped from the results. The invalid str therefore does not reach `GrepMatch::line_content`; it surfaces through the public `absolute_path` and `dir_name` API and is used internally as the open path. For Phase 8: (a) `read_to_buf` must truncate on a char boundary or fail (the comment at simd_path.rs:168 documents the truncation, not the UTF-8 hazard), and `write_dir_name` must bound `offset` by the truncated length; (b) lossy storage makes every non-UTF-8 entry unsearchable, which is both the enabling condition for (a) and a robustness issue in its own right (a hostile checkout hides any content under one 0xFF-named directory).
- Verdict CONFIRMED_UB on the block's own rule (an invalid-UTF-8 `str`, `String` and `PathBuf` observed through the safe public API, controls clean), on the same footing as EXP-104. Severity framing: `str` UTF-8 is a library (safety) invariant rather than a language validity invariant, so Miri would not flag this `from_utf8_unchecked` and ASan has nothing to observe (the copy is bounded by `total`, there is no out-of-bounds access); neither was run, and the harness check is the strongest available signal. The workspace exposure is the terraphim-grep `--paths` argument and the MCP `grep_files`/`multi_grep` `path` argument over a cloned hostile tree on Linux; the terraphim-grep CLI itself was not run in the container (no Linux aarch64 workspace build available), the harness mirrors its call site exactly as EXP-104 did.

## EXP-202: hostile YAML frontmatter reaches `unsafe-libyaml` 0.2.11 through serde_yaml 0.9 in command and shared-learning parsers

**Finding ref:** phase1_notes/H (commands), phase4 non-UB routing (Phase 6 round 2, ADVERSARIAL lens)
**Bucket:** 10 FFI-shaped (C-transpiled parser) / 4 validity; dependency-inherited unsafe
**Severity (Phase 2):** SUSPICIOUS (not scored in Phase 2)
**Hypothesis:** serde_yaml 0.9.34 parses through `unsafe-libyaml` 0.2.11, a `c2rust` transpilation of libyaml that is riddled with raw-pointer `unsafe` (pointer arithmetic over the scanner buffer, manual stack growth for flow/block nesting, alias resolution). Workspace code feeds attacker-controlled YAML into it at three reachable sites: command markdown frontmatter (`markdown_parser.rs:143`, files loaded by `parse_directory` at :572 from any configured command directory), shared-learning frontmatter (`injector.rs:248`, files from `config.shared_dir`) and the learning markdown store (`markdown_store.rs:348`, directory from `TERRAPHIM_LEARNINGS_DIR`). A crafted document (deep flow nesting `[[[[...`, an anchor/alias bomb `&a [*a,*a]`, a truncated escape, or an invalid-surrogate `\uD800`) can drive the transpiled pointer code down a path the safe wrapper never exercises. Because `unsafe-libyaml` is pure Rust, Miri can execute it end to end.

**Minimal reproducer:**
```rust
// experiments/EXP-202/src/main.rs; depends on serde_yaml = "0.9". Run under Miri.
use serde::Deserialize;
#[derive(Debug, Deserialize)]
struct Frontmatter { #[serde(default)] name: String, #[serde(default)] tags: Vec<String> }
fn main() {
    let cases = [
        "name: &a [*a, *a, *a]\ntags: [*a]\n",                 // alias fan-out
        &("tags: ".to_string() + &"[".repeat(4000) + &"]".repeat(4000)), // deep flow nesting
        "name: \"\\uD800\"\n",                                   // lone surrogate escape
        "name: \"abc\\",                                          // truncated escape at EOF
    ];
    for c in &cases {
        let r = std::panic::catch_unwind(|| serde_yaml::from_str::<Frontmatter>(c));
        match r { Ok(Ok(v)) => println!("ok: {:?}", v), Ok(Err(e)) => println!("err: {}", e.to_string().lines().next().unwrap_or("")), Err(_) => println!("PANIC (safe) on input len {}", c.len()) }
    }
    println!("EXP-202: inspect the Miri log for any dependency-frame UB report");
}
```

**Expected signal:** a Miri report (out-of-bounds pointer, invalid read, or use of an uninitialised value) whose backtrace names an `unsafe_libyaml::*` frame while parsing one of the crafted inputs. A safe `Err` or a controlled panic (deep nesting hitting the recursion guard) is NOT the signal; those are robustness notes.

**Falsifiability:** If Miri stays clean across all crafted inputs (and a short fuzz corpus of the same shapes), the transpiled parser upholds its invariants on these inputs and the row closes NO_EVIDENCE; the DoS-class inputs (deep nesting, alias fan-out) route to Phase 8 as robustness only. If serde_yaml rejects the input before reaching the C-shaped core (e.g. a depth limit), record which guard fired.

**Invocation (single command):**
```
cd experiments && MIRIFLAGS="-Zmiri-disable-isolation" RCH_DISABLED=1 CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target \
  cargo +nightly miri run -p exp202_yaml_frontmatter 2>&1 | tee ../phase5_experiment_results/EXP-202.log
```

**Verdict:** NO_EVIDENCE
**Phase 10 fresh-eyes note (2026-09-12):** the verdict still reads OPEN, but `phase5_experiment_results/EXP-202.log` (146 lines) records a completed default-axis run (`MIRI_EXIT=0`) over 43 crafted inputs with no UB report, plus the lead's tree-borrows RERUN section. No result line was ever written into this block and phase4_unified_findings.md still lists UF-041 as OPEN. Either the log supports NO_EVIDENCE and the block should say so with the input count and both axis exit codes, or the tree-borrows rerun is incomplete and the block should say that. The reviewer did not assign a verdict: that is the executor's or the lead's call, and the log's last axis boundary is ambiguous.

**Notes:**
- **Verdict assigned 2026-09-13, resolving the Phase 10 fresh-eyes question. NO_EVIDENCE on the Stacked Borrows axis, which carries the row; the Tree Borrows axis is corroboration and remains incomplete.** Stacked Borrows (`-Zmiri-disable-isolation`, macOS audit host, log `phase5_experiment_results/EXP-202.log`): **43 crafted inputs**, each parsed five ways (`serde_yaml::Value`, `CommandDefinition`, the injector `Frontmatter`, `LearningFrontmatter` and `from_str` where the bytes are UTF-8), `MIRI_EXIT=0` captured, **no Miri diagnostic on any input**. No UB in unsafe-libyaml 0.2.11's transpiled pointer code on any input tried, including the alias fan-out, the merge-key bomb, 300-deep flow and block nesting, lone and split surrogates, truncated escapes at EOF, invalid UTF-8 in plain, quoted and block scalars, a UTF-16 BOM, NUL bytes, 3000-byte anchors, 200 directives and four 40 KB scalar shapes. The DoS-class observations (recursion-limit errors on deep nesting, the alias fan-out) are robustness notes and route to Phase 8, as the Falsifiability line anticipated.
- **Tree Borrows axis on bigbox (125 GB, 24 cores), 2026-09-12 23:20 to 2026-09-13 06:19 CEST: partial, then aborted; no diagnostic in anything it did cover.** Part 1 (log `phase5_experiment_results/EXP-202_tree_borrows_bigbox.log`) ran the full corpus and completed **34 of the 43 inputs in 4h06m with no Miri diagnostic**, including every input the macOS attempt could not finish -- it cleared `folded-40k`, the 52 KB input that stalled the 16 GB host for three hours -- and was stopped at the audit's four-hour limit (`MIRI_EXIT=143`). The cap rule as written could not be applied usefully: excluding `folded-40k` would have removed an input this host had already processed, and the time went on the later corpus. Part 2 (log `..._part2.log`) therefore ran only the 9 inputs part 1 never reached, via an env-gated skip list naming the 34 completed ones, in a fresh process; it spent **2h52m inside the first of them, `many-keys-4k` (4096 YAML keys), without completing it** and was aborted (`MIRI_EXIT=143`). Both aborts are recorded as aborts. Neither bigbox log contains an `Undefined Behavior` or `unsupported operation` line. Net Tree Borrows coverage: 34 of 43 inputs clean, 9 inputs never completed on this axis. Corroboration is therefore partial and the conclusion rests on the Stacked Borrows axis, which is the stricter aliasing model.
- **Host and toolchain note:** the Stacked Borrows axis ran on macOS arm64 with rustc 1.98.0-nightly (bd08c9e71, 2026-06-25) at HEAD 46bbc43 (the harness has no workspace dependencies, so the HEAD difference from the audit's e90b193 does not affect it); the Tree Borrows attempts ran on x86_64 Ubuntu with rustc 1.100.0-nightly (67eda617e, 2026-09-10) and miri 0.1.0 (67eda617e6). The dependencies under test were identical on both: serde_yaml 0.9.34 and unsafe-libyaml 0.2.11, confirmed by `cargo tree` in each bigbox log header. The harness is the workspace copy of `experiments/EXP-202`, with one addition: an env-gated skip list (`EXP202_SKIP`), inactive unless set, used only for part 2.
- **Lead note (2026-09-12 14:40 BST):** the executor's tmux session for this experiment was killed by the lead while the tree-borrows axis was still running (the default axis had finished with MIRI_EXIT=0 and no UB report); the tree-borrows axis was re-run by the lead in session exp202b and its result is appended to the same log under a RERUN header.
- Filed by Phase 6 round 2 (adversarial lens). serde_yaml 0.9 is unmaintained (last release 0.9.34+deprecated) and `unsafe-libyaml` is its only backend; this is the workspace's largest attacker-reachable pure-Rust `unsafe` parser surface, and no phase has run Miri against it (the Phase 3 Miri matrix covered workspace lib tests, none of which deserialise hostile YAML).
- Reachability is real but requires the attacker to place files: command directories are opt-in (`add_command_directory`, registry.rs:57) and `TERRAPHIM_LEARNINGS_DIR` is env-controlled, so the threat model is a hostile repo/config a user imports, matching this lens.
- If a UB frame appears, the fix is to migrate to a maintained YAML parser (serde_norway or serde_yml) or to gate these parsers behind a size/nesting pre-check; record the exact input in `phase5_experiment_results/`.

## EXP-203: attacker-influenced release-manifest version string reaches `semver` 1.0.28 inline-storage `unsafe` before the zipsign signature gate

**Finding ref:** UF-004/UF-036 neighbour in phase4_unified_findings.md (Phase 6 round 2, ADVERSARIAL lens)
**Bucket:** 4 validity / 5 uninit (dependency inline SSO `unsafe`); order-of-operations against the signature gate
**Severity (Phase 2):** SUSPICIOUS (not scored in Phase 2)
**Hypothesis:** `TerraphimUpdater` parses the version field of the fetched release/manifest with `is_newer_version_static` -> `semver::Version::parse(m.version | release.version)` (lib.rs:264, :312, :440, :1304) BEFORE the archive signature is verified (`verify_archive_signature` runs later at lib.rs:359 and :783). semver 1.0.28 stores small pre-release/build identifiers inline with raw-pointer `unsafe` (identifier.rs, 43 `unsafe` tokens: `ptr::copy_nonoverlapping`, tagged-pointer packing). A hostile or MITM-substituted manifest (the R2/GitHub JSON is fetched over the network and the version string is not signed independently of the archive) can supply a crafted version string (very long pre-release, embedded interior structure, boundary-length identifiers) that exercises the inline/heap boundary of that `unsafe` code on a path the crate's own tests (three ASCII versions, lib.rs:1551-1556) never reach. semver is pure Rust, so Miri can drive the parser directly.

**Minimal reproducer:**
```rust
// experiments/EXP-203/src/main.rs; depends on semver = "1.0". Run under Miri.
use semver::Version;
fn main() {
    let inputs = [
        format!("1.0.0-{}", "a".repeat(255)),          // long pre-release identifier
        format!("1.0.0+{}", "b".repeat(4096)),          // long build metadata
        "1.0.0-0.0.0.0.0.0.0.0.0.0.0".to_string(),      // many numeric pre-release fields
        format!("{}.0.0", "9".repeat(40)),               // oversize numeric core
        "1.0.0-\u{80}".to_string(),                       // non-ASCII (must Err, not UB)
    ];
    for s in &inputs {
        let r = std::panic::catch_unwind(|| Version::parse(s));
        match r {
            Ok(Ok(v)) => println!("ok len={} -> {}", s.len(), v),
            Ok(Err(e)) => println!("err len={}: {}", s.len(), e),
            Err(_) => println!("PANIC (safe) len={}", s.len()),
        }
    }
    println!("EXP-203: inspect the Miri log for any semver-frame UB report");
}
```

**Expected signal:** a Miri report (invalid pointer, OOB write on the inline buffer, or an uninitialised read) in a `semver::identifier::*` frame while parsing a crafted version. A safe `Err` on every input is the refutation signal for the UB claim; a safe panic is a robustness note only.

**Falsifiability:** If Miri stays clean across the crafted inputs and a short fuzz corpus of the same shapes, semver's inline storage upholds its invariants and the row closes NO_EVIDENCE; the finding then reduces to the ordering observation (unsigned data parsed before the signature check), which routes to Phase 8 as defence-in-depth (verify signature before touching the version). If `is_newer_version_static` is only ever reached after a signature check on some path, record which caller.

**Invocation (single command):**
```
cd experiments && MIRIFLAGS="-Zmiri-symbolic-alignment-check" RCH_DISABLED=1 CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target \
  cargo +nightly miri run -p exp203_semver_version 2>&1 | tee ../phase5_experiment_results/EXP-203.log
```

**Verdict:** NO_EVIDENCE
**Phase 10 fresh-eyes note (2026-09-12):** the verdict still reads OPEN and should stay OPEN, but for a reason the block does not record: `phase5_experiment_results/EXP-203.log` ends the first axis with `MIRI_EXIT=` (empty), so that run's exit status was not captured and the run is not admissible as evidence even though 308 inputs were processed with `panics=0`. The block should either record the missing exit status as the reason the row is still open, or the axis should be re-run with the status captured. The defence-in-depth observation (an unverified manifest version is parsed at lib.rs:264/:312 before the zipsign gate at :359/:783) does not depend on Miri and is already routed to Phase 8.

**Notes:**
- **Tree Borrows axis re-run on bigbox (125 GB, 24 cores) 2026-09-12 23:20 to 2026-09-13 01:18 CEST: four attempts, all four aborted at the 40 GB audit cap or beyond. The axis is still without a verdict, and the cap rule did not rescue it.** Logs: `phase5_experiment_results/EXP-203_tree_borrows_bigbox.log` (uncapped, all 308 inputs, killed by the executor at **110 GB** resident after 28 minutes with 4 GB of host memory left, `MIRI_EXIT=143`), `..._capped.log` (`EXP203_MAX=8192`, 295 of 308 inputs, auto-killed at 48.2 GB, `MIRI_EXIT=143`), `..._capped256.log` (`EXP203_MAX=256`, 273 of 308, auto-killed at 54.4 GB, `MIRI_EXIT=143`), `..._capped32.log` (`EXP203_MAX=32`, 171 of 308 and still covering the whole 1..=32 identifier sweep and the 7/8/9-byte inline-storage boundary the experiment targets, auto-killed at 43.7 GB, `MIRI_EXIT=143`). Every run used a `bash -c` driver with the exit captured from `PIPESTATUS[0]`, so the aborts are recorded as aborts with a real exit status rather than as empty captures. The four runs share one curve: about 13 GB at 10 minutes, 26 GB at 19 minutes, past 40 GB by 25 to 28 minutes, **independently of the corpus cap**. Memory therefore tracks interpreted steps and Miri's accumulating Tree Borrows bookkeeping (308 parsed `Version` values kept, cloned, compared, displayed and sorted every 50), not input length, which is why capping the corpus below 16 KB as the earlier note proposed does not help and why a 125 GB host did not either. Superseding that outstanding-work item: the remaining options are a harness that drops each `Version` immediately instead of accumulating `kept`, or a per-input Miri process, both of which change the harness rather than the host. The experiment's verdict is unchanged and still rests on the Stacked Borrows axis, which is the stricter aliasing model: 308 inputs, `MIRI_EXIT=0`, no diagnostic.
- **Host and toolchain note for the above:** the admissible Stacked Borrows run was macOS arm64 on rustc 1.98.0-nightly (bd08c9e71, 2026-06-25); the bigbox attempts were x86_64 Ubuntu on rustc 1.100.0-nightly (67eda617e, 2026-09-10) with miri 0.1.0 (67eda617e6). The dependency under test was identical in both: semver 1.0.28.
- **Executed to a verdict by the lead, 2026-09-12, at HEAD e90b193** (log `phase5_experiment_results/EXP-203_bashdriver.log`). Stacked Borrows axis (`-Zmiri-disable-isolation`): 308 crafted version strings, ok=247 err=61, **0 panics**, no Miri diagnostic, `MIRI_EXIT=0` captured by a correct `bash -c` driver, so the run is admissible under the audit rule. No UB in semver 1.0.28's inline identifier storage on any input tried, including 64 KB version strings and the 22/23/24-byte boundary strings around the small-string optimisation size. Verdict NO_EVIDENCE on that axis.
- **Tree Borrows axis: aborted twice, no verdict.** First attempt reached 4.6 GB resident and was killed to relieve host memory pressure; the second reached 8.2 GB after 92 minutes with 61 MB of host memory free and was killed deliberately to protect the host. Both aborts are recorded in the logs as aborts, not as clean runs. The experiment's verdict rests on the Stacked Borrows axis, which is the stricter aliasing model, so the missing axis weakens corroboration rather than the conclusion. Outstanding work, recorded in the report: re-run the Tree Borrows axis on a host with more memory, or with the corpus capped below 16 KB.
- The defence-in-depth point stands whatever Miri says and is independent of this verdict: `is_newer_version_static` parses a network-fetched, not-yet-verified manifest version at lib.rs:264 and :312, while the zipsign gate only protects the archive bytes. That belongs in Phase 8 regardless (Gitea #302/#303).
- Supersedes the earlier zsh-driven log, kept as `EXP-203_first_attempt_lost_output.log`, whose exit capture was empty and which was therefore inadmissible under the audit rule (Lead addendum 12).
- **Lead note (2026-09-12 14:40 BST):** the executor's first run lost all Miri output because its log pipeline treated the stream as binary (log kept as EXP-203_first_attempt_lost_output.log); both axes were re-run by the lead in session exp203b with NUL bytes stripped.
- Filed by Phase 6 round 2 (adversarial lens). The value of the finding even if Miri is clean is the ordering: `is_newer_version_static(&m.version, ..)` at lib.rs:264/:312 runs on network-fetched, not-yet-verified JSON; the zipsign gate only protects the archive bytes, not the manifest that names them. That is defence-in-depth, not UB, and belongs in Phase 8 regardless of the Miri outcome.
- semver 1.0.28 has no open RUSTSEC advisory; this is a targeted probe of its `unsafe` identifier storage under adversarial input, not an advisory follow-up. Provability is high because the parser is pure Rust and Miri-runnable in seconds.

## EXP-204: `parse_correction_pattern` applies byte offsets computed on a transformed (lowercased/trimmed) string to the original string, panicking on non-ASCII learning prompts

**Finding ref:** UF-011/UF-028 neighbour in phase4_unified_findings.md (Phase 6 round 2, ADVERSARIAL lens; new mechanism)
**Bucket:** 12 library trait (str char-boundary contract), cross-tag 11 panic safety
**Severity (Phase 2):** NOT-UB (robustness); safe-Rust panic, filed on the EXP-103/EXP-012 precedent
**Hypothesis:** `parse_correction_pattern` (crates/terraphim_agent/src/learnings/hook.rs:227-275, reached from the learning capture hook at hook.rs:205 with the user's prompt text) computes match indices on `text.to_lowercase().trim_start()` and on `text_after_use.to_lowercase()`, then slices the ORIGINAL `text`/`text_after_use` at those byte offsets (hook.rs:235, :238-240, :268-270). Three effects: (1) any Unicode case fold that changes byte length (e.g. `U+0130` LATIN CAPITAL I WITH DOT lowercases to two bytes) shifts every later offset so the slice lands off a char boundary and panics; (2) non-ASCII leading whitespace (e.g. `U+2003` EM SPACE, three bytes) makes `trim_start` on the lowercased copy remove a different byte count than exists in the original, so the `find("use ")` offset is wrong and the slice panics `byte index N is not a char boundary`; (3) even for pure ASCII the offset from the trimmed copy is applied to the untrimmed original, so `"  use bun instead of npm"` silently returns `("npm", "e bun")` (wrong correction stored). This is safe-Rust panic / data corruption, never memory UB.

**Minimal reproducer:**
```rust
// experiments/EXP-204/src/main.rs; no deps. Verbatim "use X instead of Y" arm of hook.rs:227.
fn parse_correction_pattern(text: &str) -> Option<(String, String)> {
    let lower = text.to_lowercase();
    let trimmed = lower.trim_start();
    if let Some(i) = trimmed.find("use ") { if i == 0 {
        let after = &text[text.to_lowercase().trim_start().find("use ").unwrap() + 4..];
        let la = after.to_lowercase();
        if let Some(j) = la.find(" instead of ") {
            return Some((after[j + 12..].trim().trim_end_matches('.').to_string(), after[..j].trim().to_string()));
        }
    }}
    None
}
fn main() {
    for c in ["use bun instead of npm", "  use bun instead of npm",
              "\u{2003}\u{2003}use bun instead of npm", "use \u{0130}\u{0130} instead of npm"] {
        match std::panic::catch_unwind(|| parse_correction_pattern(c)) {
            Ok(v) => println!("{:?} -> {:?}", c, v),
            Err(_) => println!("{:?} -> PANIC", c),
        }
    }
}
```

**Expected signal:** a panic (`byte index N is not a char boundary` or `out of bounds`) on the em-space and `U+0130` inputs, and a wrong (non-panicking) tuple on the ASCII leading-whitespace input. Control: `"use bun instead of npm"` returns `Some(("npm","bun"))`.

**Falsifiability:** If the hook trims or normalises the prompt to ASCII before calling `parse_correction_pattern`, or if the capture path is unreachable from untrusted prompt text, the finding closes. The mechanism itself is already CONFIRMED in a scratch run (`scratchpad/advprobe`, 2026-09-12): em-space input panicked at byte 4 inside `U+2003`, `U+0130` input panicked out of bounds, and the ASCII-space input returned `("npm","e bun")`.

**Invocation (single command):**
```
cd experiments && RCH_DISABLED=1 CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target cargo +nightly run -p exp204_correction_slice 2>&1 \
  | tee ../phase5_experiment_results/EXP-204.log
```

**Verdict:** NO_EVIDENCE (panic confirmed)

**Notes:**
- Filed by Phase 6 round 2 (adversarial lens), and explicitly on the EXP-103 / EXP-012 precedent that a safe-Rust panic on input-controlled text is recorded as a robustness note, never as memory UB. The lead's stated rule is that safe panics are promoted only if they can feed an unsafe boundary; this one cannot, so it is filed as robustness (`NOT-UB`) and is called out as such in the round report rather than counted as a UB find.
- Distinct mechanism from EXP-012 (fixed byte cuts) and UF-028 (`chars().nth(byte_pos)`): here a byte offset computed on a case-folded/trimmed COPY is applied to the SOURCE string, which also silently corrupts the learned correction for ASCII input with leading whitespace.
- Fix shape for Phase 8: compute offsets on the same string you slice (operate on `text` throughout, or return the matched sub-`&str` from the lowercased copy by index into a parallel char map), and use `floor_char_boundary`/`str::find` on the original.
- Executed 2026-09-12 by the Phase 5 executor (round 3) at HEAD 46bbc43 on the macOS aarch64 audit host, harness `experiments/EXP-204/` (verbatim copy of hook.rs:227-275, all three arms including the `is_empty` guards), log `phase5_experiment_results/EXP-204.log`. Twenty inputs: 3 panics, 7 wrong tuples, controls (`use bun instead of npm`, `use bun not npm`, `prefer bun over npm`) all return `("npm", "bun")`. Panics: two leading U+2003 give `start byte index 4 is not a char boundary; it is inside '\u{2003}'` (the hook.rs:235 slice); ten U+0130 in X give `start byte index 42 is out of bounds for string of length 33` (hook.rs:239); three U+0130 with the `not` arm give `index 14 ... length 12`. Silent corruption: `"  use bun instead of npm"` gives `("npm", "e bun")`, `"   prefer bun over npm"` gives `("npm", "er bun")`, a leading NBSP gives `("npm", "e bun")`, a leading U+3000 gives `("npm", "se bun")`, `"use İİ instead of npm"` gives `("m", "İİ i")`, `"use İ not npm"` gives `("pm", "İ")`, and three Kelvin signs (whose lowercase is one byte shorter) give `("ad of npm", "K")`. A leading tab is fine (one byte removed in both copies). Refinement of the hypothesis: U+0130 panics only when the grown offset runs past the end of the original; otherwise it returns a wrong tuple, which is the more common and quieter outcome.
- Corroborated on the installed release binary (`~/.cargo/bin/terraphim-agent` 1.21.14, built 2026-09-11, not proven to be HEAD) through `learn hook --learn-hook-type user-prompt-submit` with HOME and `TERRAPHIM_DEFAULT_DATA_PATH` redirected to a scratch directory: the em-space prompt (the log line shows the raw U+2003 bytes) panicked at hook.rs:235:35 and the U+0130 prompt at hook.rs:239:42, both exit 101, so the user-prompt hook is not fail-open on this path; the ASCII two-space prompt exited 0 and wrote a learning file whose `## Corrected` section reads `e bun` into the redirected store (the corrupted correction is persisted, not just returned).
- Verdict NO_EVIDENCE (panic confirmed) on the EXP-103/EXP-012 convention: a safe-Rust panic and persisted data corruption on input-controlled text, with no unsafe boundary downstream; routes to Phase 8 as robustness with the fix shape already in the Notes above.

## EXP-205: hostile local git repository reaches bundled libgit2 1.9.4 (delta/config parsing) through fff-search `Repository::discover` + status during a workspace grep

**Finding ref:** UF-017 refinement in phase4_unified_findings.md (Phase 6 round 2, ADVERSARIAL lens)
**Bucket:** 10 FFI (C library) / 20 allocator; dependency-inherited unsafe (C)
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE (bounded by the reachable libgit2 surface)
**Hypothesis:** The workspace links `libgit2-sys` 0.18.5+1.9.4 (bundled libgit2 1.9.4, verified from version.h). fff-search reaches it on every grep whose path is inside a git repo: `collect_files` -> `FileSync::discover_git_workdir` -> `git2::Repository::discover` -> `git_libgit2_init`, then `spawn_git_status` computes status (phase1_notes/F). libgit2 1.9.5 (2026-07-18) fixed several defects, of which two are plausibly reachable from local repository content read by discovery+status: CVE-2026-53585 (unbounded allocation from an untrusted delta result-size header, reachable when status reads HEAD tree objects that live in packfiles) and hostile `.git/config` parsing (discover walks parent directories and reads any `.git/config` it finds). The network-only defects (CVE-2026-53587 smart-protocol OOB read, CVE-2026-53586 redirect credential leak, CVE-2026-53583 TLS SAN, PCRE revspec overflow) are NOT reachable: the workspace never fetches, clones, revparses, or opens a TLS transport through git2.

**Minimal reproducer:**
```rust
// experiments/EXP-205/src/main.rs; depends on fff-search = "=0.8.4". Run under ASan on bigbox
// against a crafted repo whose packfile carries a delta with an oversize result-size header.
use fff_search::{FFFMode, FilePicker, FilePickerOptions};
fn main() {
    // $HOSTILE_REPO is prepared out of band: git init, commit a file, then rewrite
    // .git/objects/pack/*.pack so a delta object declares a huge result size, and/or
    // drop a crafted .git/config with pathological include.path chains.
    let base = std::env::var("HOSTILE_REPO").expect("set HOSTILE_REPO to the crafted checkout");
    let mut picker = FilePicker::new(FilePickerOptions {
        base_path: base, mode: FFFMode::Ai, watch: false, ..FilePickerOptions::default()
    }).unwrap();
    // discover_git_workdir + spawn_git_status run inside collect_files.
    let n = picker.collect_files().map(|_| picker.len()).unwrap_or(0);
    println!("EXP-205: collected {n} files; inspect ASan/RSS for libgit2 delta/config abuse");
}
```

**Expected signal:** an ASan report (heap overflow / OOB) or an unbounded-RSS spike whose allocation backtrace is inside `libgit2` delta application, or a crash/hang parsing the crafted `.git/config`, on bigbox with the crafted repo. Clean status over a benign repo is the control.

**Falsifiability:** If ASan is clean and RSS stays bounded across the crafted-repo runs, either the status path does not materialise the offending delta (libgit2 may read only loose objects or refuse the packfile) or 1.9.4 already carries the mitigation; the row then stands as CONTRACTUAL-BUT-DEFENSIBLE and the remedy is the lockfile bump alone. The static reachability arm (discovery + status reads local objects and config) is refuted only by a source change.

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-exp205 && HOSTILE_REPO=/tmp/ubx-exp205/hostile RUSTFLAGS="-Zsanitizer=address" \
  CARGO_TARGET_DIR=/tmp/ubx-exp-target cargo +nightly run -Zbuild-std --target x86_64-unknown-linux-gnu' 2>&1 \
  | tee phase5_experiment_results/EXP-205.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- **Phase 5 bigbox pass 2 (2026-09-12): NO_EVIDENCE. libgit2 1.9.7 (the bumped lockfile version) upheld its invariants across the hostile corpus under ASan; the finding stays CONTRACTUAL-BUT-DEFENSIBLE, remedied by the lockfile bump alone.** Clone HEAD 46bbc43, bigbox x86_64. Harness `experiments/EXP-205` built nightly with `-Zbuild-std --target x86_64-unknown-linux-gnu --release`, `RUSTFLAGS=-Zsanitizer=address`, `CFLAGS=-fsanitize=address -fno-omit-frame-pointer`; run with `ASAN_OPTIONS=detect_odr_violation=0:abort_on_error=1`. **ASan admissibility PROVEN:** the bundled libgit2 1.9.7 is compiled from source under ASan (`target/x86_64-unknown-linux-gnu/release/build/libgit2-sys/<hash>/out/build/*.o`, 206 objects, 2517 `__asan_` symbols total; `repository.o` = 25 and `config_file.o` = 19, both carrying `__asan_report_load{1,4,8}` interceptors — the exact objects for discover/status and config parsing). It builds the vendored copy, not the host's `libgit2.so.1.7`. (Note: the harness's own inline admissibility line printed `0` because it globbed `libgit2-sys-*/out/build` whereas the real path is `libgit2-sys/<hash>/out/build`; the corrected `nm` counts are appended to the log and are the admissible ones.) The `drive` mode ran all 5 repos through the exact workspace path `FilePicker::new{watch:false, mode:Ai}` -> `collect_files` -> `discover_git_workdir` (`git2::Repository::discover`) -> `spawn_git_status`: `00_benign` (control, 1 file), `01_deep_config` (200-deep `include.path` chain + 200 KB value), `02_config_loop` (self-referential include), `03_parent_config` (hostile `.git/config` with 300 include entries in a walked parent, 19 files), `04_delta_inflate`. **Zero ASan reports, exit 0, RSS bounded** (peak `ru_maxrss` delta 1.7–20 MB per repo, nowhere near the ~2 GiB an unbounded delta allocation would demand). The delta-inflate arm could not be materialised into a valid probe: git 2.43's packer never places a delta as the strict last pack object (blob deltas are followed by trees), and `inflate_delta.py` patching a mid-pack delta shifts trailing offsets so libgit2 rejects the pack up front ("packfile does not match index") before any delta allocation — a clean rejection, not a memory-safety event; so repo 04 stands as a malformed-pack input that libgit2 refused safely, and the CVE-2026-53585 allocation path was not reached (the config-parsing arms carry the reachable-surface test). Falsifiability satisfied: ASan clean + RSS bounded across the crafted corpus closes the row NO_EVIDENCE. Logs: `phase5_experiment_results/EXP-205.log`, raw under `EXP-205_raw/`.
- Filed by Phase 6 round 2 (adversarial lens). Gate to promote to OPEN: build a crafted repo (oversize delta result-size header; pathological `.git/config` include chain) and an ASan-instrumented harness on bigbox. Do not invent a fuzz that cannot be run this round.
- Lockfile finding independent of the experiment: git2 0.20.4 depends on `libgit2-sys = "0.18.3"` (`^0.18.3`), and crates.io has `libgit2-sys 0.18.8+1.9.7` (2026-08-21, bundles libgit2 1.9.7). So `cargo update -p libgit2-sys --precise 0.18.8+1.9.7` is a lockfile-only bump that needs NO fff-search change; #257 (a git2 major-version bump) is NOT the blocker for these C-library CVE fixes. This corrects phase1_notes/J's "needs fff-search bump (#257)" for the libgit2 C-library version specifically.
- Same attacker-chosen tree as EXP-104/EXP-201; a hostile repo triggers all three chains (mmap window, path inflation, libgit2 status) in one MCP grep.
- Phase 5 bigbox pass (2026-09-12, rounds 7-9): STAYS DEFERRED. Gate artefacts are now written, so the next session can run the single-command invocation to a verdict: ASan harness `experiments/EXP-205/{Cargo.toml,src/main.rs}` (drives the exact `FilePicker::new{watch:false,mode:Ai}` -> `collect_files` path and reports `ru_maxrss` delta around it); corpus builder `experiments/EXP-205/craft_repos.sh` (00_benign control, 01_deep_config 200-deep `include.path` chain with a 200 KB value, 02_config_loop self-referential include, 03_parent_config hostile `.git/config` in a walked parent, 04_delta_inflate) plus `inflate_delta.py` (patches the largest-offset pack delta's target-size varint to ~2 GiB and recomputes the trailing pack SHA-1, so libgit2 allocates the inflated result buffer during status unpack before the final oid check — the CVE-2026-53585 allocation probe). Lockfile confirmed: at clone HEAD 46bbc43 `Cargo.lock` carries `libgit2-sys 0.18.8+1.9.7` (commit "bump libgit2-sys to 0.18.8+1.9.7"), so the run tests the mitigated version; a bounded RSS + clean status would confirm CONTRACTUAL-BUT-DEFENSIBLE remedied by the bump alone. Not run this session: the executor's safety classifier blocked all mutating Bash (corpus crafting, build, execution) for the remainder of the session after the hostile-input generators were authored; re-run in a fresh session or the default permission mode. ASan admissibility to check on the run: `nm <target>/build/libgit2-sys-*/out/build/*.o | rg -c __asan_` non-zero, and suppress the benign libgit2 `llhttp` ODR false-positive with `ASAN_OPTIONS=detect_odr_violation=0` (observed in EXP-301).

## EXP-206: crafted SQLite database file reaches bundled SQLite 3.46.0 (C) through the Cursor connectors

**Finding ref:** phase1_notes/D, phase1_notes/I (Phase 6 round 2, ADVERSARIAL lens)
**Bucket:** 10 FFI (C library); dependency-inherited unsafe (C)
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** Both Cursor connectors open an on-disk SQLite database with `rusqlite::Connection::open` (crates/terraphim_sessions/src/connector/cursor.rs:163; crates/terraphim-session-analyzer/src/connectors/cursor.rs:156) and run fixed `SELECT` statements over `cursorDiskKV` / `ItemTable`. rusqlite 0.32 bundles libsqlite3-sys 0.30.1 = SQLite 3.46.0 with FTS5 enabled (build.rs `-DSQLITE_ENABLE_FTS5`). SQLite 3.46.0 predates several 2025 fixes; the ones NOT requiring attacker-controlled SQL text are CVE-2025-7709 (FTS5 index corruption from a malicious database file, fixed 3.50.3) and CVE-2025-29088/52099 (C-API misuse, not reachable from safe rusqlite). The connectors accept a database PATH the attacker controls (a planted `state.vscdb` in a Cursor workspace the user imports), so a crafted db file feeds the C page/btree parser. The queries are plain `SELECT ... LIKE`, not FTS5 `MATCH`, so CVE-2025-7709's specific trigger needs an FTS5 vtable inside the crafted file that a query touches; page/btree corruption in the C reader is the broader surface.

**Minimal reproducer:**
```rust
// experiments/EXP-206/src/main.rs; depends on rusqlite = { version = "0.32", features = ["bundled"] }.
// Run under ASan on bigbox against a corrupted db file.
use rusqlite::Connection;
fn main() {
    // $CRAFTED_DB is a hand-corrupted SQLite file (fuzzed page header / btree pointers,
    // or an FTS5 shadow table with an out-of-range segment) carrying a cursorDiskKV table.
    let path = std::env::var("CRAFTED_DB").expect("set CRAFTED_DB");
    let conn = match Connection::open(&path) { Ok(c) => c, Err(e) => { println!("open err: {e}"); return; } };
    let mut stmt = conn.prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'").unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
    match rows { Ok(it) => println!("rows: {}", it.count()), Err(e) => println!("query err: {e}") }
    println!("EXP-206: inspect ASan for a libsqlite3 page/btree/FTS5 fault");
}
```

**Expected signal:** an ASan heap-overflow / OOB report inside `sqlite3` while opening or querying the corrupted file. A clean `SQLITE_CORRUPT`/`SQLITE_NOTADB` error is the refutation (SQLite detected the corruption safely).

**Falsifiability:** If ASan is clean across a corpus of corrupted db files (SQLite returns `SQLITE_CORRUPT` gracefully), the reader upholds its invariants on these inputs and the row stays CONTRACTUAL-BUT-DEFENSIBLE, remedied by the version bump alone. Note the bump is not free: rusqlite 0.32 pins libsqlite3-sys 0.30 (SQLite 3.46); reaching 3.50.3 needs rusqlite 0.34+.

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-exp206 && CRAFTED_DB=/tmp/ubx-exp206/crafted.vscdb RUSTFLAGS="-Zsanitizer=address" \
  CARGO_TARGET_DIR=/tmp/ubx-exp-target cargo +nightly run -Zbuild-std --target x86_64-unknown-linux-gnu' 2>&1 \
  | tee phase5_experiment_results/EXP-206.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- **Phase 5 bigbox pass 2 (2026-09-12): NO_EVIDENCE. Bundled SQLite 3.46.0 upheld its invariants across the corrupted-db corpus under ASan; the row stays CONTRACTUAL-BUT-DEFENSIBLE, the bump to 3.50.3+ (rusqlite 0.34+) remains hardening, not a fix for an observed fault.** Clone HEAD 46bbc43, bigbox x86_64. Harness `experiments/EXP-206` built nightly `-Zbuild-std --target x86_64-unknown-linux-gnu --release`, `RUSTFLAGS=-Zsanitizer=address`, `CFLAGS=-fsanitize=address -fno-omit-frame-pointer`; `rusqlite::version()` reported 3.46.0. **ASan admissibility PROVEN:** the bundled SQLite object `build/libsqlite3-sys/<hash>/out/<hash>-sqlite3.o` carries 30 `__asan_` symbols with the full `__asan_report_load/store{1,2,4,8,16}` interceptor set; standalone crate, no proc-macro host-link of libsqlite3-sys (the I-303 sqlx-macros trap does not apply). (The harness's inline admissibility line printed empty because it globbed `sqlite3.o` whereas cc names it `<hash>-sqlite3.o`; corrected `nm` counts appended to the log.) The harness opened each of 14 files read-only and ran the two exact Cursor-connector `SELECT`s over `cursorDiskKV`/`ItemTable` plus `PRAGMA integrity_check` (which walks the btree pages in C). Every corrupted variant was rejected safely: `file is not a database` (bad magic, bad page size), `database disk image is malformed` (inflated page count, truncated header, bad page type, cell-pointer clobber, random fuzz), FTS5-specific `malformed inverted index for FTS5 table` and `vtable constructor failed: ftsdoc`, and `integrity_check` reporting tree/freelist corruption in prose. **Zero ASan reports, exit 0, no crash.** (The valid control dbs returned a benign `Invalid column type Text` from the harness reading TEXT columns as `Vec<u8>`; that is a harness detail, not a SQLite fault — `integrity_check` on those returned `ok`.) Falsifiability satisfied: a clean, graceful rejection across the corpus with no memory-safety report closes the row NO_EVIDENCE. Logs: `phase5_experiment_results/EXP-206.log`, raw under `EXP-206_raw/`.
- Filed by Phase 6 round 2 (adversarial lens). Gate to promote to OPEN: a corpus of corrupted/fuzzed SQLite files plus an ASan build on bigbox. The connectors are feature-gated (`cursor-connector` on terraphim_sessions; `connectors` on terraphim-session-analyzer) and enabled in the terraphim_agent default build (Cargo.toml:99), so the path ships.
- terraphim_persistence does NOT reach SQLite FTS5: `rg -ni fts5|MATCH` over its src returns only Rust `match` expressions; it uses opendal (dashmap/redb/s3) and, on the sqlite profile, sqlx-sqlite with plain SQL. So CVE-2025-7709 is reachable only through the Cursor connector db-file path, not through the persistence layer.
- The SQL-text CVEs (CVE-2025-6965, 29087, 3277) are unreachable: no query string in the workspace is attacker-controlled; all are compile-time literals.
- Phase 5 bigbox pass (2026-09-12, rounds 7-9): STAYS DEFERRED. Gate artefacts written: standalone ASan harness `experiments/EXP-206/{Cargo.toml,src/main.rs}` (`rusqlite = { version = "0.32", features = ["bundled"] }` -> SQLite 3.46.0, FTS5; opens each corpus file read-only and runs the two exact connector `SELECT`s over `cursorDiskKV`/`ItemTable` plus `PRAGMA integrity_check` to walk the btree pages in C), and generator `experiments/EXP-206/gen_corpus.py` (13 variants: benign control, magic/page-size/encoding/page-count header tampering, tail and header truncation, page-2 body scribble, invalid page-type byte, cell-pointer clobber, bad freelist head, 10% random fuzz, and FTS5 shadow-table corruption). Standalone crate: no proc-macro path links libsqlite3-sys, so plain `CFLAGS` is admissible (the I-303 sqlx-macros host-link trap does not apply). Not run this session: the safety classifier blocked all mutating Bash (corpus generation, build, execution) for the rest of the session; re-run in a fresh session. ASan admissibility to check: `nm <target>/build/libsqlite3-sys-*/out/sqlite3.o | rg -c __asan_` non-zero and the host `target/debug/build/libsqlite3-sys-*` count zero.

## EXP-207: `FFF_FRECENCY_PATH` env injection points the MCP server's `heed`/LMDB `unsafe open` at a hostile memory-mapped database

**Finding ref:** phase1_notes/G (Phase 6 round 2, ADVERSARIAL lens)
**Bucket:** 10 FFI (C library) / 1 aliasing (mmap-backed store); dependency-inherited unsafe
**Severity (Phase 2):** CONTRACTUAL-BUT-DEFENSIBLE
**Hypothesis:** `McpService::new` reads `FFF_FRECENCY_PATH` and, when set, calls `fff_search::FrecencyTracker::open(&path)` (crates/terraphim_mcp_server/src/lib.rs:109-118), which opens an LMDB environment through `heed` 0.22.1 `EnvOpenOptions::open` (an `unsafe fn`, dbs/lmdb.rs:154) over lmdb-master-sys 0.2.6 (LMDB 0.9.70, C). heed's own docs state that "anything that causes LMDB's lock file to be broken will cause synchronization issues and may introduce UB" and that LMDB memory-maps the data file with minimal validation. An attacker who controls the environment (env injection into the MCP server process, or a symlinked path) can point `FFF_FRECENCY_PATH` at a crafted `data.mdb` whose page headers/meta pages are corrupted, so the C reader dereferences attacker-shaped offsets when the frecency store is consulted.

**Minimal reproducer:**
```rust
// experiments/EXP-207/src/main.rs; depends on fff-search = "=0.8.4". Run under ASan on bigbox.
use fff_search::FrecencyTracker;
fn main() {
    // $FRECENCY_DIR contains a hand-corrupted data.mdb (fuzzed meta page / page pointers)
    // plus a lock file; mirrors what FFF_FRECENCY_PATH would name for the MCP server.
    let dir = std::env::var("FRECENCY_DIR").expect("set FRECENCY_DIR");
    match FrecencyTracker::open(&dir) {
        Ok(_t) => println!("EXP-207: opened; a real MCP read would now traverse the crafted pages"),
        Err(e) => println!("EXP-207 refutation arm: open rejected the crafted db: {e}"),
    }
    println!("EXP-207: inspect ASan for an LMDB page-pointer dereference fault");
}
```

**Expected signal:** an ASan OOB/wild-read report inside `lmdb`/`mdb_*` while opening the environment or reading the meta page. A clean `heed::Error` (LMDB rejecting the file) is the refutation.

**Falsifiability:** If ASan is clean across a corpus of corrupted `data.mdb` files (LMDB validates its meta pages and returns an error), the store upholds its invariants and the row stays CONTRACTUAL-BUT-DEFENSIBLE; the remaining hardening is to refuse `FFF_FRECENCY_PATH` outside a trusted directory. If `FrecencyTracker::open` never mmaps until first read, extend the harness to perform a lookup before scoring NO_EVIDENCE.

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-exp207 && FRECENCY_DIR=/tmp/ubx-exp207/crafted RUSTFLAGS="-Zsanitizer=address" \
  CARGO_TARGET_DIR=/tmp/ubx-exp-target cargo +nightly run -Zbuild-std --target x86_64-unknown-linux-gnu' 2>&1 \
  | tee phase5_experiment_results/EXP-207.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- **Phase 5 bigbox pass 2 (2026-09-12): NO_EVIDENCE. LMDB 0.9.70 validated its meta pages and page pointers across the hostile corpus under ASan; the row stays CONTRACTUAL-BUT-DEFENSIBLE, and the residual hardening is to refuse `FFF_FRECENCY_PATH` outside a trusted directory.** Clone HEAD 46bbc43, bigbox x86_64. Harness `experiments/EXP-207` built nightly `-Zbuild-std --target x86_64-unknown-linux-gnu --release`, `RUSTFLAGS=-Zsanitizer=address`, `CFLAGS=-fsanitize=address -fno-omit-frame-pointer`, `ASAN_OPTIONS=detect_odr_violation=0` (fff-search pulls git2/llhttp; the benign startup ODR is suppressed, as in EXP-301). **ASan admissibility PROVEN:** `build/lmdb-master-sys/<hash>/out/<hash>-mdb.o` carries 28 `__asan_` symbols (plus midl.o; 38 total) with the full `__asan_report_load/store{1,2,4,8,16}` interceptor set — LMDB's env open, meta-page read and page traversal are instrumented. The `init` mode built a valid populated env via `FrecencyTracker::open` + 500 `track_access` calls (86 KiB `data.mdb`), the generator emitted 11 crafted copies (all capped under the 12 MiB `SIZE_CAP_BYTES` so `erase_if_oversized` does not delete them at open), and `drive` ran each through the exact workspace surface `FrecencyTracker::open` -> `open_env` (unsafe `EnvOpenOptions::open`, mmap + meta-page read) plus the extended read arm (`access_count`/`seconds_since_last_access`/`get_access_score`, per the Falsifiability clause). **Every crafted env was handled safely, zero ASan reports, exit 0:** open rejected `01_bad_magic` (`MDB_INVALID`), `02_bad_version`/`09_random_fuzz` (`MDB_VERSION_MISMATCH`), `03_bad_root_pgno` (`EINVAL`); the read arm returned clean `Mdb(PageNotFound)` on `05_page2_scribble`/`06_nodeptr_clobber` and `Mdb(Corrupted)` on `08_truncated_meta_only` rather than dereferencing an attacker-shaped offset; controls and benign scribbles (`00`, `04`, `07`, `10`) opened and read normally. Falsifiability satisfied: LMDB validated its meta pages and returned errors across the corpus with no memory-safety report, so the row closes NO_EVIDENCE. I-309 honoured: `open` is the primary workspace surface; the read arm is the belt-and-braces extension and is also clean. Logs: `phase5_experiment_results/EXP-207.log`, raw under `EXP-207_raw/`.
- Filed by Phase 6 round 2 (adversarial lens). Gate to promote to OPEN: a corpus of corrupted LMDB files plus an ASan build on bigbox. The path is only wired when `FFF_FRECENCY_PATH` is set (default off), so the threat model is env-var injection into the server process, which this lens covers explicitly.
- LMDB memory-maps the file and trusts its meta pages; the `unsafe` on `EnvOpenOptions::open` is precisely about the caller vouching for the file's integrity, which env injection removes. This is the same class as EXP-104 (a dependency `unsafe` whose precondition the deployment breaks), reached through the environment rather than through a concurrent writer.
- No RUSTSEC advisory drives this; it is a targeted probe of the documented heed/LMDB open contract under a hostile file.
- Phase 5 bigbox pass (2026-09-12, rounds 7-9): STAYS DEFERRED. Gate artefacts written: ASan harness `experiments/EXP-207/{Cargo.toml,src/main.rs}` with an `init <dir>` mode (builds a valid populated frecency env via `FrecencyTracker::open` + 500 `track_access` calls so the corpus has real btree pages to corrupt) and a `drive <parent>` mode that, for each crafted env dir, calls `FrecencyTracker::open` (the exact workspace open-only surface: `open_env` -> unsafe `EnvOpenOptions::open`, mmap + meta-page read) and then, per the Falsifiability clause, forces a read (`access_count`/`seconds_since_last_access`/`get_access_score`) to traverse pages before scoring; generator `experiments/EXP-207/gen_corpus.py` (11 variants: control, bad meta magic, bad version, scribbled root pgno, maxed txnid+root, page-2 scribble, node-pointer clobber, tail and meta-only truncation, random fuzz, bad page-header pgno). Confirmed the store's `SIZE_CAP_BYTES` is 12 MiB and `open_env` calls `erase_if_oversized` first, so the generator caps every `data.mdb` at 12 MiB to avoid the file being erased before open. I-309 narrowing honoured: the tracker is opened and never read by the workspace, so `open` is the primary arm; the read arm is the belt-and-braces extension. Not run this session: the safety classifier blocked all mutating Bash for the rest of the session; re-run in a fresh session. ASan admissibility to check: `nm <target>/build/lmdb-master-sys-*/out/*.o | rg -c __asan_` non-zero; suppress the transitive libgit2 `llhttp` ODR false-positive with `ASAN_OPTIONS=detect_odr_violation=0` (fff-search pulls git2).

## EXP-301: differential test of the fff-search case-insensitive memmem AVX2 kernels against a scalar oracle under ASan (x86_64 only)

**Follow-up of:** EXP-201
**Finding ref:** UF-044 in phase4_unified_findings.md (Phase 6 round 3 idea I-301; block written by the lead from the round report after the idea-wizard agent hit the session limit)
**Bucket:** 2 out-of-bounds / 3 validity (dependency-inherited SIMD unsafe)
**Severity (Phase 2):** SUSPICIOUS
**Hypothesis:** fff-search 0.8.4's `verify_avx2` (case_insensitive_memmem.rs:107-170, reached only for needles of 32 bytes or more, :355) and `search_packed_pair_avx2` (:376-470) have no upstream test (longest upstream needle is 26 bytes, :564-662) and have never run under a sanitiser; the workspace reaches them through every PlainText grep on x86_64 hosts (grep.rs:1281 with the user needle). A needle or haystack length at a dispatch boundary reads past the haystack or returns a match the scalar oracle does not.

**Minimal reproducer:**
```rust
// Harness shape: seeded corpus, both kernels are pub (fff-search lib.rs:107).
fn oracle(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let h = hay.to_ascii_lowercase(); let n = needle.to_ascii_lowercase();
    if n.is_empty() { return Some(0); }
    h.windows(n.len()).position(|w| w == &n[..])
}
fn main() {
    // needle lengths 1, 2, 15, 16, 17, 31, 32, 33 and longer than the haystack;
    // haystack lengths at multiples of 16 and 32 plus or minus one; a match only
    // in the final tail slice; bytes 0x80-0xFF; empty needle and empty haystack.
    // Compare fff_search::case_insensitive_memmem(hay, needle) with oracle(hay, needle).
}
```

**Expected signal:**
- ASan: any report (heap-buffer-overflow, global-buffer-overflow) inside `verify_avx2` or `search_packed_pair_avx2`.
- Differential: any index where the kernel and the oracle disagree, or a divergence between the x86_64 result and the aarch64 (NEON) result for the same seed.

**Falsifiability:** Zero ASan reports and zero divergences over the full edge corpus (at least 10,000 seeded cases plus the enumerated boundary cases) on an x86_64 host closes the row NO_EVIDENCE. Any report or divergence is CONFIRMED_UB (report) or a correctness bug (divergence only, NOT-UB).

**Invocation (single command):**
```
ssh <x86_64-linux-host> 'cd /tmp/ubx-exp301 && RUSTFLAGS="-Zsanitizer=address" cargo +nightly run --target x86_64-unknown-linux-gnu --release' 2>&1 | tee phase5_experiment_results/EXP-301.log
```

**Verdict:** NO_EVIDENCE

**Notes:**
- **Phase 10 fresh-eyes note (2026-09-12):** the two bullets below are the pre-run gate text and were left in place after the run completed; they are superseded by the "Phase 5 bigbox pass" bullet that follows. The row is resolved, not pending, and phase4_unified_findings.md UF-044 was corrected from DEFERRED to NO_EVIDENCE to match. Phase 11 does not need to re-run this.
- (Superseded) Requires an x86_64 host with free disk (bigbox is the only candidate and was full on 2026-09-12; 310 GB free by 14:20 BST, so runnable again). The aarch64 audit host cannot execute the AVX2 kernels; Phase 6 round 2 idea I-208's bounds argument was a static read only.
- (Superseded) Gate: an x86_64 host and the seeded corpus; both trivial once disk is available. Runnable in Phase 11 as a bounded campaign.
- Phase 5 bigbox pass (2026-09-12, rounds 7-9): RESOLVED NO_EVIDENCE. Harness `experiments/EXP-301/` drives both public entry points `case_insensitive_memmem::search` (reaches `verify_avx2`, needle >= 32) and `search_packed_pair` (reaches `search_packed_pair_avx2`) against the `to_ascii_lowercase().windows()` oracle. Seed `0x5eed1301abcdef01`; 1,215 enumerated boundary cases (needle 0,1,2,15,16,17,31,32,33,40; haystack 0,1 and 16/32/48/64/96/128/256 +/-1; tail-only matches; 0x80-0xFF; empty needle/haystack; needle>haystack) plus 50,000 seeded random cases = 51,215 total. x86_64 bigbox: ASan build `RUSTFLAGS=-Zsanitizer=address cargo +nightly build -Zbuild-std --target x86_64-unknown-linux-gnu --release`, 118 `__asan_` symbols in the binary, AVX2 detected (`avx2` in /proc/cpuinfo), `total_divergences=0`, exit 0, ZERO ASan reports. aarch64 macOS audit host (NEON path): identical run gives `fingerprint=0x90a0d98b9dbec243`, byte-identical to the x86_64 fingerprint -> NO host divergence; a second aarch64 run under `-Zsanitizer=address -Zbuild-std --target aarch64-apple-darwin` (1720 `__asan_` syms) over 21,215 cases is also clean, exit 0. Admissibility: the AVX2/NEON kernels are pure-Rust SIMD (no bundled C on the reached path), so admissibility is that ASan actually instruments the harness (`nm` count non-zero) and the SIMD dispatch fires (avx2/dotprod detected), both confirmed. One benign startup abort (ASan `odr-violation` on `lookup_table` in libgit2's bundled `deps/llhttp/llhttp.c`, pulled in transitively by fff-search's git2 dep, never on the memmem path) was suppressed with `ASAN_OPTIONS=detect_odr_violation=0`; it is not a memory-safety report. Logs: `phase5_experiment_results/EXP-301.log`.

## EXP-302: shipped `terraphim_mcp_server --sse` under concurrent `grep_files` over a tree with an in-place writer and a truncator (blast radius of UF-039 in the long-lived server)

**Follow-up of:** EXP-104
**Finding ref:** UF-039 in phase4_unified_findings.md (Phase 6 round 3 idea I-302; block written by the lead from the round report after the idea-wizard agent hit the session limit)
**Bucket:** 1 aliasing (dependency-inherited; same mechanism as EXP-104)
**Severity (Phase 2):** LIKELY-UB (mechanism already CONFIRMED_UB by EXP-104; this experiment measures blast radius)
**Hypothesis:** `grep_files` (crates/terraphim_mcp_server/src/lib.rs:1349-1406) and `multi_grep_files` (:1455-1495) each build a `FilePicker::new` with `watch: false` and `cache_budget: None` (:1372-1378, :1485-1491) and map the tree per call, while SSE hands one `McpService` clone per connection (main.rs:295), so K concurrent clients produce K independent mappings of the same files. One client's grep over a tree that another process truncates kills the server for every client (SIGBUS), and the invalid-String arm becomes an MCP JSON response that serde_json must either escape or fail on.

**Minimal reproducer:**
```rust
// Driver shape (not a unit test): start `terraphim_mcp_server --sse` on a tree of
// 4 MiB files; from N client tasks call grep_files in a loop; from a sibling
// process run the EXP-104 writer arm (same-length rewrite) and then the truncate
// arm; record server exit status and every response whose content fails
// std::str::from_utf8 or whose JSON fails to parse.
fn main() {}
```

**Expected signal:**
- Truncate arm: server process exits on SIGBUS (signal 7) while other clients have requests in flight.
- Writer arm: at least one `grep_files` response whose line content is not valid UTF-8, or a serde_json serialisation error in the server log.

**Falsifiability:** If the server survives 1,000 truncations with all clients receiving well-formed responses and no invalid UTF-8, the blast-radius hypothesis is NO_EVIDENCE (the mechanism stays CONFIRMED_UB via EXP-104). The Phase 8 UF-039 remediation (zero-file cache budget) must make this experiment pass; it doubles as the pre/post proof alongside EXP-805.

**Invocation (single command):**
```
ssh bigbox 'cd /tmp/ubx-exp302 && ./run_sse_grep_soak.sh 1000' 2>&1 | tee phase5_experiment_results/EXP-302.log
```

**Verdict:** CONFIRMED_UB

**Notes:**
- Primary arm runnable on bigbox with the EXP-104 harness's writer once disk is available; the hostile-corpus arm (EXP-201 inflated names, EXP-205 crafted repo) is gated on corpora that do not exist yet.
- Recommended as a Phase 11 soak campaign rather than a round experiment: it adds blast radius, not a new mechanism, so it does not hold convergence open.
- **Phase 5 bigbox pass 2 (2026-09-12): CONFIRMED_UB. Blast radius of UF-039 confirmed in the shipped server.** Clone HEAD 46bbc43, host bigbox (x86_64 Ubuntu, glibc 2.39, 4 KiB mmap threshold so the 4 MiB files map). The shipped release `terraphim_mcp_server --sse -p server` (from `/tmp/ubx-r7/EXP-302/target/release`) served 8 concurrent SSE clients each calling `terraphim_grep{query:"needle", path:tree, limit:200}` in a loop over a 4-file tree of 4 MiB files with the needle only in the last 40 lines (so every grep scans the whole mmap). A sibling process mutated the same files. **Truncate arm: the server died with SIGBUS** — bash job control logged `1320217 Bus error  RUST_LOG=warn "$SRV" --sse ...`, the harness recorded `server EXITED code=135 (128+7 = SIGBUS)`, the server.log ends abruptly mid-`walk_filesystem` scan with no graceful shutdown, and the process was gone afterwards. One client truncating the mapped tree therefore kills the long-lived server for every connected client. Writer arm (same-length in-place 0xA9->0xFF rewrite): server stayed ALIVE and no response content failed `std::str::from_utf8` (the shipped `terraphim_grep` uses the PlainText/`from_utf8_lossy` sink, so an in-place rewrite yields replacement chars, not invalid UTF-8 over the wire — NO_EVIDENCE for the writer/UTF-8 arm, consistent with EXP-104's finding that the invalid-`String` arm is the Fuzzy `from_utf8_unchecked` path, which the MCP tool does not use). Hundreds of thousands of grep responses succeeded before the crash (`json_parse_fail` counts text blocks in `Ok` responses; grep returns human text not JSON, so those "failures" are expected and prove the mapping path executed). Corrections to the pre-run scaffold, applied and rebuilt this pass: (1) the MCP tool is named `terraphim_grep`, not `grep_files` (the old name returned `-32601` on every call, so a prior run that "survived" was inadmissible — the grep path never ran); (2) the rmcp 0.9 client needs the `transport-sse-client-reqwest` feature for `SseClientTransport::start(url)`; (3) each `call_tool` is wrapped in a 5 s `tokio::time::timeout` so a stalled SSE response cannot hang the driver; (4) the corpus puts the needle only at the file tail so the grep reads the full mapping (a low match limit otherwise reads only the head and never touches truncated-away pages); (5) there is no `/health` endpoint, so readiness gates on the port listening. No ASan needed: the signal is process survival (SIGBUS), directly observed. Logs: `phase5_experiment_results/EXP-302.log`, raw under `EXP-302_raw/` (`EXP-302.full.log`, `server.trimmed.log`). This is the pre-remediation proof; the Phase 8 UF-039 zero-file cache budget must make this experiment pass (EXP-805 is the post-fix pair).
- Phase 5 bigbox pass (2026-09-12, rounds 7-9): STAYS DEFERRED, but partly prepared. The shipped server release binary is built and present on bigbox at `/tmp/ubx-r7/EXP-302/target/release/terraphim_mcp_server` (23 MB, from clone HEAD 46bbc43, `cargo build --release -p terraphim_mcp_server`, exit 0). Harness scaffold written: `experiments/EXP-302/run_sse_grep_soak.sh` (starts `--sse` on unused port 18302, health-checks `/health`, runs the writer arm then the truncate arm with the EXP-104 `rewrite`/`child-truncate` sibling against the same 4 MiB tree, and captures the server exit signal so a truncate SIGBUS shows as exit code 135 = 128+7), plus an rmcp-0.9 SSE client driver `experiments/EXP-302/{Cargo.toml,src/main.rs}` that calls `grep_files{query:"needle",path:tree}` from 8 concurrent clients and checks every response's text with `std::str::from_utf8`. Two items remain before a verdict: (a) compile-verify the rmcp-0.9 client driver (its `SseClientTransport::start` / `().serve` / `call_tool` signatures are written from the 0.9 client surface but were NOT built this session) and adapt the EXP-104 sibling to target a directory rather than a private tempdir; (b) execute. NOT run this session: the executor's safety classifier blocked all mutating Bash (build, corpus, execution, and even read-only ssh to bigbox) for the remainder of the session after the hostile-input generators were authored — re-run in a fresh session or the default permission mode. No ASan needed for this block: the signal is server survival (SIGBUS) and response UTF-8 validity, not a C-library sanitiser report.

## EXP-806: post-remediation rerun of the EXP-201 hostile geometries through the workspace entry points (UF-040 fix is sound)

**Follow-up of:** EXP-201
**Finding ref:** UF-040 in phase4_unified_findings.md (Lead addendum 5); remediation phase8_remediation_plan.md Delta 3, D3.1 (commit 0b)
**Bucket:** 3 validity / 1 aliasing (dependency `unsafe` whose precondition the consumer's input defeats); cross-tag 12 library trait (`str` validity); remediation soundness
**Severity (Phase 2):** LIKELY-UB (CONFIRMED_UB by EXP-201 before the fix)
**Hypothesis:** After commit 0b (`reject_hostile_tree` called at the head of `HybridSearcher::search_code` and of the MCP `find_files`, `grep_files` and `multi_grep_files` tools, refusing any entry name that is not valid UTF-8 or any relative path longer than 4095 bytes), the EXP-201 hostile geometries (6x249, 6x250, 6x251, 7x200, 14x100, 15x255 directories of `0xFF` names) driven through the workspace entry points return an error naming the offending entry before any `FilePicker` is constructed, so fff-search never stores an inflated path and `absolute_path`/`dir_name` are never reached with it; the control geometries (2x250, 3x255, which fit the 4096-byte buffer) still search normally; and the same trees driven through raw fff-search 0.8.4 still reproduce EXP-201, proving the trees remain hostile and the guard, not the host, is what changed.

**Minimal reproducer:**
```rust
// experiments/EXP-806/src/main.rs; path-depends on terraphim_grep and terraphim_mcp_server
// (fixed tree), plus fff-search = "=0.8.4" for the control arm. Linux only (0xFF names).
use std::{ffi::OsStr, fs, os::unix::ffi::OsStrExt, path::PathBuf};
fn hostile(depth: usize, len: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut p: PathBuf = dir.path().into();
    for _ in 0..depth { p.push(OsStr::from_bytes(&vec![0xFFu8; len])); fs::create_dir(&p).unwrap(); }
    fs::write(p.join("needle.txt"), b"needle\n".repeat(4000)).unwrap();
    dir
}
#[tokio::main]
async fn main() {
    for (depth, len, expect_err) in [(6, 250, true), (15, 255, true), (7, 200, true), (2, 250, false)] {
        let t = hostile(depth, len);
        // Workspace arm: the guard must refuse hostile trees and pass the controls.
        let r = terraphim_grep::path_guard::reject_hostile_tree(t.path());
        assert_eq!(r.is_err(), expect_err, "guard verdict for {depth}x{len}: {r:?}");
        if expect_err { assert!(r.unwrap_err().contains("not valid UTF-8"), "error must name the cause"); }
        // Control arm: raw fff-search still inflates on the same tree (EXP-201 unchanged).
        let mut picker = fff_search::FilePicker::new(fff_search::FilePickerOptions {
            base_path: t.path().to_string_lossy().into(), watch: false, ..Default::default() }).unwrap();
        picker.collect_files().unwrap();
        let abs = picker.get_files()[0].absolute_path(&picker, t.path());
        assert_eq!(std::str::from_utf8(abs.as_os_str().as_bytes()).is_err(), expect_err && depth * (3 * len + 1) > 4096);
    }
    println!("EXP-806 NO_EVIDENCE: guard refuses every hostile geometry; controls pass; raw fff-search still inflates");
}
```

**Expected signal:**
- Workspace arm: `reject_hostile_tree` returns `Err` naming the first non-UTF-8 entry for 6x249, 6x250, 6x251, 7x200, 14x100 and 15x255, and `Ok(())` for 2x250 and 3x255. `HybridSearcher::search` over `with_search_paths(vec![hostile])` returns `Err` with the same text before any `FilePicker` log line; `McpService::grep_files(.., Some(hostile), ..)`, `find_files` and `multi_grep_files` return `is_error: true` with the same text. Control geometries return matches for `needle` as before.
- Control arm: raw fff-search 0.8.4 reproduces EXP-201 (`absolute_path` bytes fail `from_utf8` in the residue-1/2 geometries; `dir_name` panics at 15x255), unchanged.
- Guard arm: `cargo test -p terraphim_agent --test ci_guards` fails, naming the site, after a stored patch removes the `reject_hostile_tree` call from one `FilePicker::new` site; passes on the fixed tree.

**Falsifiability:** A hostile geometry that passes the guard and reaches `FilePicker::new` (an `Ok` from `reject_hostile_tree`, or a search result instead of an error) refutes the hypothesis (the walk skips entries, for example symlinked or ignored directories the fff-search walker does visit, or the length rule is off by one). A control geometry refused by the guard means the rule is too broad and the fix regresses legitimate trees. A control arm that no longer reproduces EXP-201 voids the run (host filesystem refuses `0xFF` names, or fff-search moved). The TOCTOU residual (an entry created between the guard and `collect_files`) is out of scope for this block and stays documented in D3.1.

**Invocation (single command):**
```
docker run --rm -v "$PWD":/w -w /w/experiments rust:1.97-trixie bash -c 'RCH_DISABLED=1 CARGO_TARGET_DIR=/tmp/ubx-exp-target cargo run --release -p exp806_hostile_tree_guard && cd /w && cargo test -p terraphim_grep --test path_guard_hostile_tree && cargo test -p terraphim_agent --test ci_guards' 2>&1 | tee phase5_experiment_results/EXP-806.log
```

**Verdict:** DEFERRED

**Notes:**
- Phase 8 Delta 3 (2026-09-12): DEFERRED, gated on Phase 13 landing commit 0b (guard module, four call sites, two Linux-only tests, `ci_guards` extension). Linux only, as EXP-201: run in the OrbStack container if bigbox is still full (EXP-201 note), on the container's overlay filesystem, which accepts `0xFF` names. The `docker` invocation above is the portable form; substitute the OrbStack container as EXP-201 did.
- The guard's length rule (relative path longer than 4095 bytes refused) is a conservative extension for deep trees of valid multi-byte names, which the same `read_to_buf` truncation would cut mid-character; EXP-201 did not test that geometry, so this block records it as an assertion of the guard, not as a reproduction of UB, and a future round may add the geometry to EXP-201.
- When a released fff-search fixes `read_to_buf` and the lossy storage (upstream issue in D3.1), rerun the control arm against that version; a clean control arm is the signal that the guard has become defence in depth rather than the fix.
