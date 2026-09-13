# Phase 3 dynamic findings (ThreadSanitizer runner)

Host: aarch64-apple-darwin (Darwin 25.6.0), rustc 1.98.0-nightly (bd08c9e71 2026-06-25), `-Zsanitizer=thread -Zbuild-std`, `CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-target-tsan`, `TSAN_OPTIONS="halt_on_error=0 report_signal_unsafe=0 history_size=4"`, default libtest parallelism (no `--test-threads`). Raw logs: `phase3_raw/tsan_<target>_run<N>.log`. Sources untouched.

## Instrument capability check (done before interpreting any run)

The load-bearing assumption of EXP-001/EXP-002 is that TSan can observe the `setenv`/`unsetenv` (writer) versus `getenv` (reader) pair. Both sides are executed inside uninstrumented libSystem code (`std::env::set_var` calls `libc::setenv` under std's `ENV_LOCK`; libgit2/sqlite call `libc::getenv`), so the only way TSan can see the accesses to `environ` is through runtime interceptors.

```
$ SR=$(rustc +nightly --print sysroot)/lib/rustlib/aarch64-apple-darwin/lib
$ nm $SR/librustc-nightly_rt.tsan.dylib | rg -c '^[0-9a-f]+ t _wrap_'
475                                # interceptors present in general (memcpy, strdup, pthread_*, ...)
$ nm $SR/librustc-nightly_rt.tsan.dylib | rg '_wrap_(getenv|setenv|unsetenv|putenv|clearenv)' | wc -l
0                                  # NO environment interceptors
$ nm $SR/librustc-nightly_rt.tsan.dylib | rg -i 'environ|getenv|setenv'
                 U __NSGetEnviron   # runtime's own use only (__sanitizer::GetEnv)
                 U _getenv
```

Consequence: on this host the macOS TSan runtime is **blind to the exact access pair that F-001 and F-002 predict**. A clean run therefore cannot be scored NO_EVIDENCE; the strongest honest verdict is NEEDS_REFINEMENT unless a report appears by some other route. Instrumenting the C code with `CFLAGS=-fsanitize=thread` would not change this: it instruments the *call* to `getenv`, not the reads of `environ` inside libc.

### CFLAGS attempt (recorded, then dropped)

First launch used `CFLAGS="-fsanitize=thread"` as drafted. Result for both targets, three consecutive attempts each: `error: could not compile sqlx-macros (lib)` with `ld: Undefined symbols: ___tsan_atomic8_load, ___tsan_atomic16_load/store, ___tsan_atomic32_load/store, ___tsan_atomic64_store ...` referenced from the bundled `libsqlite3-sys` and `ring` objects. Cause: `sqlx-macros` is a proc-macro (host dylib); the `cc` crate honours `CFLAGS` for host builds too, and host == target triple here, so the C objects picked up TSan instrumentation but the host link (no `-Zsanitizer`) has no TSan runtime. Logs: `phase3_raw/cflags_attempt/`. CFLAGS was dropped for all subsequent runs, as the lead permitted. C-code instrumentation therefore did **not** reach libgit2 or sqlite in the runs below, and (per the check above) could not have made the predicted pair visible anyway.

### Platform check: Apple Libc serialises environ access

Verified from apple-oss-distributions/Libc `stdlib/FreeBSD/getenv.c` and `setenv.c` (main branch, fetched 2026-09-12): `getenv()` wraps `__findenv_locked` in `environ_lock_np()`/`environ_unlock_np()`, and `setenv()`/`unsetenv()` wrap `__setenv_locked`/`__unsetenv_locked` in the same lock. So on macOS the writer (`std::env::set_var` -> `libc::setenv`) and the reader (`git__getenv` -> `getenv`, `unixTempFileInit` -> `getenv`) are mutually excluded at libc level. The remaining macOS hazard is only the classic use-after-free of the `char*` returned by `getenv` if another thread frees/replaces that entry afterwards (libgit2 copies immediately via `git_str_puts`; sqlite stores the pointer in `azTempDirs[]` and dereferences it later, which is an ASan-shaped question, not a TSan one).

F-001 and F-002 state their severity for the Linux CI runners (glibc `getenv` takes no lock). A macOS-clean result therefore does not transfer to the Linux claim, independent of the interceptor gap above.

## Linux runtimes checked too (for the refinement recommendation)

The rustup nightly also ships `librustc-nightly_rt.tsan.a` for `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu` under the same toolchain. `nm` on both: 1745 `__interceptor_*` symbols each, **zero** matching `(get|set|unset|put|clear)env`. compiler-rt's TSan (as shipped with this rustc) does not intercept the environment functions on Linux either, so a plain Linux TSan re-run is expected to be blind to the same pair. This contradicts the working assumption that Linux TSan would see it; see verdict table for the refinement that does not depend on it.

## EXP-001 (F-001): terraphim_grep --lib under TSan

Command per run (CFLAGS unset, see above):
```
RUSTFLAGS="-Zsanitizer=thread" CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-target-tsan \
  cargo +nightly test -Zbuild-std --target aarch64-apple-darwin -p terraphim_grep --lib
```
Binary: `deps/terraphim_grep-c20f6268f6f81f63`, links `@rpath/librustc-nightly_rt.tsan.dylib`, 12 `__tsan_*` references in the Rust code (instrumented). libgit2 object `build/libgit2-sys-d8b2e4a6c61a915d/out/build/*.o`: 0 `__tsan_` symbols (C not instrumented).

| run | log | tests | TSan warnings | wall |
|---|---|---|---|---|
| 1 | tsan_grep_run1.log | 57 passed, 0 failed | 0 | 126 s (includes build-std compile) |
| 2 | tsan_grep_run2.log | 57 passed, 0 failed | 0 | 123 s (rebuild after CFLAGS env change) |
| 3 | tsan_grep_run3.log | 57 passed, 0 failed | 0 | 4 s (test phase only) |
| filtered (4 tests: `test_openrouter_client_base_url_resolution search_without_thesaurus_uses_fff_mode default_build_greps_populated_directory multiple_search_paths_merge_results`) | tsan_grep_runfiltered.log | 4 passed, 0 failed | 0 | 5 s |

No `WARNING: ThreadSanitizer` of any kind (so nothing to classify as T-FP1..4 or UNRELATED).

## EXP-002 (F-002): terraphim_agent --test tui_service_tests under TSan

Command per run: as above with `-p terraphim_agent --test tui_service_tests`. build-std for the agent crate **succeeded** once CFLAGS was dropped (the only failure mode was the proc-macro host link described earlier). sqlite object `build/libsqlite3-sys-480c9d312d86877d/out/*sqlite3.o`: 0 `__tsan_` symbols; `libsqlite3-sys` is built with feature `bundled` (via sqlx-sqlite <- opendal services-sqlite <- terraphim_persistence).

| run | log | tests | TSan warnings | wall |
|---|---|---|---|---|
| 1 | tsan_agent_tui_run1.log | 10 passed, 0 failed | 0 | 273 s (includes compile; waited on the shared build-dir lock) |
| 2 | tsan_agent_tui_run2.log | 10 passed, 0 failed | 0 | 17 s |
| 3 | tsan_agent_tui_run3.log | 10 passed, 0 failed | 0 | 16 s |

No `WARNING: ThreadSanitizer` of any kind.

## EXP-001b: interposition probe (not TSan) on the built grep test binary

Because TSan cannot see the pair, a `DYLD_INSERT_LIBRARIES` shim (`envprobe.c`, source in `phase3_raw/envprobe/`) interposes `setenv`/`unsetenv`/`getenv` and records (a) every `getenv` executed on a thread other than the first writer thread after the first write, and (b) strict overlap: a `getenv` on thread R while a `setenv`/`unsetenv` on thread W != R is between entry and return. `ENVPROBE_WIDEN_US` makes the writer sleep inside its region; this enlarges a pre-existing window, it cannot create a cross-thread overlap that the schedule does not already permit. Shim self-test on a synthetic writer/reader pair detected overlap (1 strict overlap in 2000 writes). Run against `terraphim_grep-c20f6268f6f81f63` at default libtest parallelism, 10 iterations per mode, cwd `crates/terraphim_grep`. Logs: `phase3_raw/envprobe_grep_widen{0,500,2000}us.log`.

| widen | iterations | all 57 tests ok | getenv on a non-writer thread after first write (per run) | strict overlaps (total) |
|---|---|---|---|---|
| 0 us | 10 | 10/10 | 89-113 | 0 |
| 500 us | 10 | 10/10 | 98-112 | 7 |
| 2000 us | 10 | 10/10 | 89-113 | 4 |

Per run the writer executed `setenv` once and `unsetenv` twice (matches `test_openrouter_client_base_url_resolution`, openrouter_client.rs:170-191). All 11 strict overlaps had the same reader chain, and it is **not** the libgit2 chain F-001 names:

## F-101 (envprobe/terraphim_grep): getenv("OSLogRateLimit") / getenv("ACTIVITY_LOG_STDERR") on a sibling test thread while set_var/remove_var is in progress
**Tool:** envprobe (DYLD interposition; TSan blind) **File:line:** writer crates/terraphim_grep/src/openrouter_client.rs:170-191 (`test_openrouter_client_base_url_resolution`); reader crates/terraphim_grep/src/openrouter_client.rs `OpenRouterClient::new` -> reqwest `ClientBuilder::build` -> `reqwest::proxy::Matcher::system` -> hyper_util `proxy::matcher::Matcher::from_system` -> `mac::with_system` -> SystemConfiguration `SCDynamicStoreCreateWithOptions` -> libsystem_trace `_os_trace_init_slow` / `_os_trace_metadata_init` -> `getenv`, on the `test_into_llm_client_returns_trait_object` thread. **Severity:** UNRELATED to F-001's traced chain (Phase 2 row F lists "reqwest/hyper-util proxy env" as `env-read-std`, i.e. a locked reader; on macOS the `from_system` path is native `getenv` via SystemConfiguration and libsystem_trace, so that classification is wrong for this platform); on macOS it is **not memory-unsafe** because Apple Libc serialises both sides behind `environ_lock_np()` (see platform check). It is still a contract violation of `std::env::set_var` (a getenv reader on another thread that std cannot lock). On Linux hyper_util's `from_system` reads `HTTP_PROXY`-style variables via `std::env::var` (locked) and there is no SystemConfiguration/os_trace path, so this specific chain does not transfer to the CI runner. **Traceback (first 25 lines, widen=500 us, iteration with 1 overlap):**
```
[envprobe] OVERLAP: getenv("ACTIVITY_LOG_STDERR") on thread 24886892 while writer active on thread 24886893
0   libenvprobe.dylib                   my_getenv + 192
1   libsystem_trace.dylib               _os_trace_init_slow + 912
2   librustc-nightly_rt.tsan.dylib      wrap_dispatch_once + 256
3   librustc-nightly_rt.tsan.dylib      wrap_dispatch_once_f + 156
4   libsystem_trace.dylib               os_state_add_handler + 348
5   SystemConfiguration                 ____SCDynamicStoreInitialize_block_invoke + 116
6   librustc-nightly_rt.tsan.dylib      wrap_dispatch_once + 256
7   SystemConfiguration                 __SCDynamicStoreCreateInternal + 544
8   SystemConfiguration                 SCDynamicStoreCreateWithOptions + 68
9   terraphim_grep-c20f6268f6f81f63     system_configuration::dynamic_store::SCDynamicStore::create + 196
10  terraphim_grep-c20f6268f6f81f63     system_configuration::dynamic_store::SCDynamicStoreBuilder::build + 336
11  terraphim_grep-c20f6268f6f81f63     hyper_util::client::proxy::matcher::mac::with_system + 156
12  terraphim_grep-c20f6268f6f81f63     hyper_util::client::proxy::matcher::Builder::from_system + 48
13  terraphim_grep-c20f6268f6f81f63     hyper_util::client::proxy::matcher::Matcher::from_system + 36
14  terraphim_grep-c20f6268f6f81f63     reqwest::proxy::Matcher::system + 40
15  terraphim_grep-c20f6268f6f81f63     reqwest::async_impl::client::ClientBuilder::build + 512
16  terraphim_grep-c20f6268f6f81f63     terraphim_grep::openrouter_client::OpenRouterClient::new + 548
17  terraphim_grep-c20f6268f6f81f63     terraphim_grep::openrouter_client::tests::test_into_llm_client_returns_trait_object + 68
18  terraphim_grep-c20f6268f6f81f63     ...closure + 52
19  terraphim_grep-c20f6268f6f81f63     core::ops::function::FnOnce::call_once + 40
20  terraphim_grep-c20f6268f6f81f63     test::__rust_begin_short_backtrace
21  terraphim_grep-c20f6268f6f81f63     test::run_test_in_process::{{closure}}
22  terraphim_grep-c20f6268f6f81f63     std::panicking::catch_unwind::do_call
23  terraphim_grep-c20f6268f6f81f63     test::types::RunnableTest::run
24  libsystem_pthread.dylib             _pthread_start + 136
```
**Cross-refs:** F-001 (same writer; different reader), F-007 (logical race on the same variable). What this proves empirically: the writer's `SAFETY: test code, single-threaded, no concurrent env access` comment at openrouter_client.rs:166 is false in the real binary at default parallelism (about 100 `getenv` calls from other threads land after the write in every run, and strict overlap is reachable). What it does not prove: that the libgit2 `git__getenv` reader ever overlaps; that reader executed on the fff-mode test threads but never inside the writer's window in 30 iterations, consistent with its `Once` guard giving a single very short window.

## Verdict table

| experiment | verdict | runs | wall per run | CFLAGS reached C code? | notes |
|---|---|---|---|---|---|
| EXP-001 (F-001, terraphim_grep --lib) | NEEDS_REFINEMENT | 3 full + 1 filtered, all clean; plus 30 envprobe iterations | 126 s / 123 s / 4 s / 5 s | No (host proc-macro link fails with CFLAGS; dropped). Irrelevant anyway: the racing accesses are inside libSystem. | TSan blind to setenv/getenv on this host (no interceptors) and Apple Libc locks both sides, so a clean run is not evidence for the Linux claim. envprobe confirms cross-thread getenv concurrency with the writer (F-101) but via a different, macOS-only reader. |
| EXP-002 (F-002, tui_service_tests) | NEEDS_REFINEMENT | 3, all clean | 273 s / 17 s / 16 s | No (same) | Same blindness. build-std for terraphim_agent succeeded, so no reduction to a scratch binary was needed for compilation; the reduction is still recommended below for the reader-visibility problem. |

### Refinement recommended (what would actually decide F-001/F-002)

1. **Do not expect a Linux TSan re-run to help by itself.** The Linux TSan runtimes shipped with this nightly have no `getenv`/`setenv` interceptors either (checked above). If the lead has evidence that bigbox's compiler-rt differs, verify with `nm librustc-nightly_rt.tsan.a | rg 'interceptor_(get|set|unset)env'` before spending the run.
2. **Port the interposition probe to Linux (`LD_PRELOAD`)** and run the same two binaries on bigbox (x86_64-unknown-linux-gnu, glibc). glibc `getenv` takes no lock and `setenv` can `realloc` the `__environ` array, so a strict overlap there is a real memory-unsafe interleaving, not just a contract violation. The probe records the reader chain, which is what distinguishes the libgit2/sqlite chains from incidental readers like F-101. Source: `phase3_raw/envprobe/envprobe.c` (replace the `__interpose` section with `dlsym(RTLD_NEXT, ...)` wrappers; `pthread_threadid_np` -> `gettid`).
3. **Widen only the writer** (as here, `ENVPROBE_WIDEN_US`) rather than the reader, so the single-`Once` readers in libgit2 and sqlite keep their natural timing; run 100+ iterations, since the libgit2 window was never hit in 30 macOS iterations.
4. For F-002 specifically, a scratch binary that only exercises `EnvVarGuard` plus `rusqlite::Connection::open` on sibling threads (as the lead suggested) is still the cleanest way to force the sqlite `unixTempFileInit` reader into the window; combine it with the `LD_PRELOAD` probe or with ASan (glibc `setenv` growth path frees the old `__environ` array, which ASan can catch as a use-after-free if the reader is mid-scan).
5. Regardless of the outcome, F-101 already shows the SAFETY comment at openrouter_client.rs:166 is empirically false; the fix (make the writer test `#[serial]` and/or inject the base URL without touching the process environment) does not depend on the refinement.

## Linux refinement attempt on bigbox (2026-09-12, bigbox-local clock 10:35; bigbox's clock runs about one hour ahead of the mac that wrote the earlier sections, so all bigbox timestamps below are bigbox-local)

Step 1 of the lead's Linux plan (verify interceptors before building) was executed on `bigbox` (x86_64, Ubuntu glibc 2.39, rustc 1.100.0-nightly 67eda617e 2026-09-10):

```
$ nm ~/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/lib/librustc-nightly_rt.tsan.a | grep -c __interceptor_
1754
$ nm ... | grep -E "__interceptor_(getenv|setenv|unsetenv|putenv|clearenv)"
NONE
```

The Linux compiler-rt TSan runtime has no environment interceptors either. The writer (`libc::setenv` from `std::env::set_var`) and the readers (`getenv` inside libgit2 / sqlite) both execute in uninstrumented glibc, so a Linux TSan run of EXP-001/EXP-002 would be blind to the predicted pair exactly as the macOS run was. Per the plan, the clone and the TSan builds were **not** started.

### Final verdicts after the Linux check

| experiment | final verdict | reason |
|---|---|---|
| EXP-001 (F-001) | NEEDS_REFINEMENT | TSan cannot observe the pair on either platform. Evidence that the writer is concurrent with native `getenv` readers in the real binary exists (F-101, macOS, different reader). The libgit2 reader itself has not been caught in the window. |
| EXP-002 (F-002) | NEEDS_REFINEMENT | Same instrument blindness; no probe run yet against the tui binary. |

The only tools that can decide these two findings are ones that observe libc calls or libc memory directly: the `LD_PRELOAD` port of `phase3_raw/envprobe/envprobe.c` on bigbox (glibc `getenv` is unlocked, so a strict overlap there is a real unsynchronised read), ASan on the glibc `setenv` growth path, or Valgrind/Helgrind, which instruments libc too. A TSan run without environment interceptors adds nothing.

## Linux interposition probe on bigbox (EXP-001b, EXP-002b, EXP-002c)

Host: bigbox, x86_64, Ubuntu glibc 2.39, rustc 1.100.0-nightly (67eda617e 2026-09-10), 24 cores, load about 4. Clone of `task/254-ci-pipeline` at `/tmp/ubx-probe-clients`, plain (non-sanitised) debug test binaries built with `cargo +nightly test --no-run` into `/tmp/ubx-probe-target`. Probe: `phase3_raw/envprobe/linux/envprobe_linux.c`, an `LD_PRELOAD` shim wrapping `setenv`, `unsetenv` and `getenv` through `dlsym(RTLD_NEXT)`. It logs tid, name and `CLOCK_MONOTONIC` ns per call (with `ENVPROBE_TRACE=1`), and flags a `getenv` on thread R as an overlap when a `setenv`/`unsetenv` on thread W != R is in progress. Two phases are distinguished: **call** means the reader ran while the real glibc `setenv`/`unsetenv` was executing (no artificial widening involved); **pre** means the reader ran during the optional `ENVPROBE_WIDEN_US` sleep the shim inserts before the real call. Only `call` overlaps at `widen=0` are unassisted; `pre` counts show the schedule permits the two threads to be concurrent in that region. Overlap backtraces are captured with `backtrace()` and `dladdr`, symbolised afterwards with `addr2line` (`symbolise2.py`). Runner: `run_probe.sh`, default libtest parallelism, 100 iterations per widening (150 for the scratch binary). Session summaries: `phase3_raw/probe_{grep,tui,scratch}.session.log`. Note that std's own `env::var` also reaches libc `getenv` (under std's `ENV_LOCK`), so the "getenv on another thread" totals include locked std readers; only overlap blocks, which carry a backtrace, identify native readers.

### EXP-001b: terraphim_grep --lib (binary `terraphim_grep-5ffeb952bb8a9889`, cwd `crates/terraphim_grep`)

| widen | iterations | all 57 tests ok | overlaps `pre` | overlaps `call` | wall for the batch |
|---|---|---|---|---|---|
| 0 us | 100 | 100/100 | 0 | **1** | about 2 s |
| 500 us | 100 | 100/100 | 463 | 0 | about 1 s |
| 2000 us | 100 | 100/100 | 544 | 0 | about 2 s |

Logs: `phase3_raw/envprobe_linux_grep_widen{0,500,2000}us.log`. Writer per run: 1 `setenv` and 2 `unsetenv` of `OPENROUTER_BASE_URL` from `test_openrouter_client_base_url_resolution` (openrouter_client.rs:170-191, the only writer in the binary). All 994 backtraced overlap blocks parsed by the symboliser (1 + 454 + 539; the per-run counters total 1 + 463 + 544 = 1008, the difference being header lines interleaved with test stdout) are the libgit2 chain that F-001 traced statically; no other native reader appeared on Linux (the macOS-only SystemConfiguration reader of F-101 does not exist here). Reader signatures across the widened runs: `git__getenv` <- `git_sysdir_guess_xdg_dirs` (`XDG_CONFIG_HOME`), `git__getenv` <- `git_sysdir_guess_home_dirs` (`HOME`), and a direct `getenv("APP_SANDBOX_CONTAINER_ID")` in `git_sysdir_guess_home_dirs`, all under `git_sysdir_global_init` <- `init_common` <- `git_runtime_init` <- `git_libgit2_init` <- `libgit2_sys::init` (`Once`) <- `git2::init` <- `Repository::discover` <- `fff_search::file_picker::FileSync::discover_git_workdir` <- `FilePicker::collect_files` <- `HybridSearcher::search_code`.

## F-102 (envprobe-linux/terraphim_grep): unassisted cross-thread `getenv` inside `git_libgit2_init` while glibc `setenv`/`unsetenv` executes on the openrouter test thread
**Tool:** envprobe (LD_PRELOAD, glibc 2.39) **File:line:** writer crates/terraphim_grep/src/openrouter_client.rs:170-191 (`std::env::set_var`/`remove_var("OPENROUTER_BASE_URL")`, SAFETY comment at :166); reader libgit2 sysdir.c `git_sysdir_guess_home_dirs` -> `getenv`, reached from crates/terraphim_grep/src/hybrid_searcher.rs (`search_code` -> fff-search `collect_files` -> `Repository::discover` -> `git2::init`) on the `hybrid_searcher::tests::multiple_search_paths_merge_results` thread. **Severity:** CONFIRMED_UB. This is a `call`-phase overlap at `widen=0` (iteration 30 of 100): the reader executed glibc `getenv`, which walks `__environ` without a lock, while the writer thread was inside glibc `setenv`/`unsetenv`, which modify the `__environ` array (and may `realloc` it) under glibc's internal lock that `getenv` does not take. That is an unsynchronised read concurrent with a write of the same object, a data race under the C11 rules the `std::env::set_var` contract cites. The predicted pair from F-001 is therefore reproduced on the Linux CI platform without any artificial widening; with the writer widened by 500 us the same chain overlapped in 89 of 100 iterations, and by 2000 us in 92 of 100 (several reader calls per iteration, one per sysdir variable). The C11 data race F-001 cites is the absence of any happens-before edge between the conflicting accesses to `__environ`; that is a property of the binary at default parallelism regardless of hit rate, and the probe's job was only to show the accesses are in fact unordered and conflicting, which both the unassisted `call` overlap and the widened runs do. "Linux CI platform" here means glibc: `.github/workflows/ci.yml` runs on `ubuntu-latest` and `.gitea/workflows/native-ci.yml` on the `terraphim-native` runner, which is bigbox itself (Ubuntu 24.04, glibc 2.39), the host these probes ran on. **Traceback (first 25 lines, symbolised):**
```
[envprobe] OVERLAP(call): getenv("APP_SANDBOX_CONTAINER_ID") on tid 290269 while writer active on tid 290279
  #1 libenvprobe.so getenv
  #2 terraphim_grep-5ffeb952bb8a9889 git_sysdir_guess_home_dirs
  #3 terraphim_grep-5ffeb952bb8a9889 git_sysdir_guess_global_dirs
  #4 terraphim_grep-5ffeb952bb8a9889 git_sysdir_global_init
  #5 terraphim_grep-5ffeb952bb8a9889 init_common
  #6 terraphim_grep-5ffeb952bb8a9889 git_runtime_init
  #7 terraphim_grep-5ffeb952bb8a9889 git_libgit2_init
  #8 terraphim_grep-5ffeb952bb8a9889 libgit2_sys::init::{closure#0}
  #9 terraphim_grep-5ffeb952bb8a9889 <std::sync::once::Once>::call_once::<libgit2_sys::init::{closure#0}>::{closure#0}
  #10 terraphim_grep-5ffeb952bb8a9889 <std::sys::sync::once::futex::Once>::call
  #11 terraphim_grep-5ffeb952bb8a9889 <std::sync::once::Once>::call_once::<libgit2_sys::init::{closure#0}>
  #12 terraphim_grep-5ffeb952bb8a9889 libgit2_sys::init
  #13 terraphim_grep-5ffeb952bb8a9889 git2::init
  #14 terraphim_grep-5ffeb952bb8a9889 <git2::repo::Repository>::discover::<&std::path::Path>
  #15 terraphim_grep-5ffeb952bb8a9889 <fff_search::file_picker::FileSync>::discover_git_workdir
  #16 terraphim_grep-5ffeb952bb8a9889 <fff_search::file_picker::FilePicker>::collect_files
  #17 terraphim_grep-5ffeb952bb8a9889 <terraphim_grep::hybrid_searcher::HybridSearcher>::search_code::{closure#0}
  #18 terraphim_grep-5ffeb952bb8a9889 <terraphim_grep::hybrid_searcher::HybridSearcher>::search::{closure#0}
  #19 terraphim_grep-5ffeb952bb8a9889 terraphim_grep::hybrid_searcher::tests::multiple_search_paths_merge_results::{closure#0}
  #20 terraphim_grep-5ffeb952bb8a9889 <core::pin::Pin<&mut dyn core::future::future::Future<Output = ()>> as core::future::future::Future>::poll
  #21 terraphim_grep-5ffeb952bb8a9889 <core::pin::Pin<&mut core::pin::Pin<&mut dyn core::future::future::Future<Output = ()>>> as core::future::future::Future>::poll
  #22 terraphim_grep-5ffeb952bb8a9889 <tokio::runtime::scheduler::current_thread::CoreGuard>::block_on::{closure#0}::{closure#0}
  #23 terraphim_grep-5ffeb952bb8a9889 tokio::task::coop::with_budget
  #24 terraphim_grep-5ffeb952bb8a9889 <tokio::runtime::scheduler::current_thread::Context>::enter
  #25 terraphim_grep-5ffeb952bb8a9889 <tokio::runtime::scheduler::current_thread::CoreGuard>::block_on::{closure#0}
```
Raw block: `phase3_raw/envprobe_linux_grep_widen0us.log` line 1889 (iteration 30). **Cross-refs:** F-001 (proved), F-101 (macOS sibling of the same writer), F-007.

### EXP-002b: terraphim_agent --test tui_service_tests (binary `tui_service_tests-7b5b5c240c8f72ac`, cwd `crates/terraphim_agent`)

| widen | iterations | all 10 tests ok | overlaps `pre` | overlaps `call` | wall for the batch |
|---|---|---|---|---|---|
| 0 us | 100 | 100/100 | 0 | 0 | 52 s |
| 500 us | 100 | 100/100 | 0 | 0 | 56 s |
| 2000 us | 100 | 100/100 | 0 | 0 | 68 s |

Logs: `phase3_raw/envprobe_linux_tui_widen{0,500,2000}us.log` (the tests spawn child processes that inherit the shim; their summaries show `setenv=0` and are ignored). Main-process writer per run: 4 `setenv` and 2 `unsetenv` (`HOME`, `XDG_CONFIG_HOME`, `TERRAPHIM_DATA_PATH`, then the restores on Drop), matching `EnvVarGuard` in tui_service_tests.rs:37-61; 32 to 62 `getenv` calls from other threads land after the first write in every run. **Zero overlaps in 300 iterations.** Three traced runs (`envprobe_linux_tui_trace{1,2,3}.log`, `ENVPROBE_TRACE=1`) explain why: sqlite's `getenv("SQLITE_TMPDIR")`/`getenv("TMPDIR")` pair executes exactly once per process, and in all three runs it executed **on the writer thread itself**, 0.8 to 1.9 ms after the guard's first `setenv` and about 460 ms before the guard's Drop. The `#[serial]` writer test is the first test to open a sqlite connection, so `sqlite3_initialize` (and thus `unixTempFileInit`) is consumed on the writer's own thread, and the sibling tests' later opens no longer read the environment. **Verdict for EXP-002b: NO_EVIDENCE**, with the reachability caveat that this depends on test scheduling: if any non-serial sibling ever opened its connection before the serial test's first `set_var`, or the serial test were reordered, the sqlite reader would land on a sibling thread inside the two writer windows (see EXP-002c for what happens then). The other `getenv` calls observed on sibling threads after the first write (`HOME`, `XDG_CONFIG_HOME`, `XDG_*`, `RIPGREP_CONFIG_PATH`, `RUST_MIN_STACK`, `WSL_DISTRO_NAME`) never overlapped and were not backtraced, so they are unclassified (most are std `env::var` readers, which are locked against the writer).

### EXP-002c: scratch reduction of F-002 (binary `ubx-probe-scratch`, release, standalone crate under `/tmp/ubx-probe-scratch`)

Source: `phase3_raw/envprobe/linux/scratch_main.rs` and `Cargo.toml` (rusqlite 0.32, `bundled`, i.e. the same libsqlite3-sys build the workspace uses). One writer thread performs the `EnvVarGuard` sequence (`set_var("HOME"|"XDG_CONFIG_HOME"|"TERRAPHIM_DATA_PATH")`, then `remove_var` x2) 200 times; 8 sibling threads each call `rusqlite::Connection::open` three times on their own file; all threads start behind a barrier. No synchronisation between writer and readers, mirroring a non-serial sibling that opens a connection while the serial test's guard is live.

| widen | iterations | all runs ok | overlaps `pre` | overlaps `call` | wall for the batch |
|---|---|---|---|---|---|
| 0 us | 150 | 150/150 | 4 | **102** | about 1 s |
| 500 us | 150 | 150/150 | 298 | 0 | 85 s |
| 2000 us | 150 | 150/150 | 282 | 0 | 314 s |

Logs: `phase3_raw/envprobe_linux_scratch_widen{0,500,2000}us.log`. All 106 backtraced overlaps at `widen=0` are `getenv("SQLITE_TMPDIR")` (43 call) and `getenv("TMPDIR")` (59 call) from `sqlite3_os_init` <- `sqlite3_initialize` <- `sqlite3_mutex_alloc` <- `rusqlite::inner_connection::InnerConnection::open_with_flags` on a reader thread, while the writer thread was inside glibc `setenv`/`unsetenv`. (The 4 `pre` counts at `widen=0` are readers that arrived in the few instructions between the shim recording the writer tid and entering the real call; they are not artificial.)

## F-103 (envprobe-linux/scratch): sqlite `unixTempFileInit` `getenv` on a sibling thread inside a glibc `setenv`/`unsetenv` window when the EnvVarGuard sequence and `Connection::open` are unsynchronised
**Tool:** envprobe (LD_PRELOAD, glibc 2.39) **File:line:** reader libsqlite3-sys bundled sqlite3.c `unixTempFileInit` (`azTempDirs[0] = getenv("SQLITE_TMPDIR"); azTempDirs[1] = getenv("TMPDIR");`) via `sqlite3_os_init` from `rusqlite::Connection::open`; writer the `EnvVarGuard` sequence from terraphim_test_utils (`set_var`/`remove_var` of `HOME`, `XDG_CONFIG_HOME`, `TERRAPHIM_DATA_PATH`) as used at crates/terraphim_agent/tests/tui_service_tests.rs:37-61. **Severity:** CONFIRMED_UB for the mechanism (unassisted `call`-phase overlap in 102 of 150 iterations when the reader lands on a sibling thread), **not reproduced in the shipped tui binary** (EXP-002b) because the reader is consumed on the writer thread first. Note the additional hazard specific to this reader: sqlite stores the `char*` returned by `getenv` in the static `azTempDirs[]` and dereferences it on every later temp-file open, so a concurrent `setenv` that replaces the entry leaves sqlite holding a pointer into memory glibc will never free (glibc leaks old values by design), but a later `unsetenv` shifting `__environ` mid-scan can make `getenv` return the wrong entry. **Traceback (first 12 lines, symbolised):**
```
[envprobe] OVERLAP(call): getenv("SQLITE_TMPDIR") on tid 282608 while writer active on tid 282618
  #1 libenvprobe.so getenv
  #2 ubx-probe-scratch sqlite3_os_init
  #3 ubx-probe-scratch sqlite3_initialize
  #4 ubx-probe-scratch sqlite3_mutex_alloc
  #5 ubx-probe-scratch <rusqlite::inner_connection::InnerConnection>::open_with_flags
  #6 ubx-probe-scratch std::sys::backtrace::__rust_begin_short_backtrace::<ubx_probe_scratch::main::{closure#2}, ()>
  #7 ubx-probe-scratch <std::thread::lifecycle::spawn_unchecked<...>::{closure#1} as FnOnce<()>>::call_once
  #8 ubx-probe-scratch <std::sys::thread::unix::Thread>::new::thread_start
  #9 libc.so.6+0x9caa4 (addr2line without libc debug info reports pthread_condattr_setpshared; this is the pthread start trampoline)
  #10 libc.so.6 __clone
```
**Cross-refs:** F-002 (mechanism proved in reduction; concrete binary NO_EVIDENCE), F-006, F-008, G (same wrapper).

## Final verdict table (after Linux probe)

| experiment | verdict | evidence | runs |
|---|---|---|---|
| EXP-001 (TSan, macOS) | NEEDS_REFINEMENT (superseded by EXP-001b) | instrument blind to the pair | 3 + filtered, clean |
| EXP-001b (envprobe, Linux glibc) | **CONFIRMED_UB** (F-102) | 1 unassisted `call` overlap in 100 iterations, reader is the libgit2 sysdir chain F-001 predicted; 1007 further overlaps with the writer widened, all the same chain | 300 |
| EXP-002 (TSan, macOS) | NEEDS_REFINEMENT (superseded by EXP-002b/c) | instrument blind to the pair | 3, clean |
| EXP-002b (envprobe, Linux, real tui binary) | **NO_EVIDENCE** with reachability caveat | 0 overlaps in 300 iterations; sqlite's single `getenv` pair executes on the writer thread about 1 ms after the guard's first `setenv`, so no sibling ever reads during a writer window under current ordering | 300 + 3 traced |
| EXP-002c (envprobe, Linux, scratch reduction) | **CONFIRMED_UB** for the mechanism (F-103) | 102 unassisted `call` overlaps in 150 iterations, reader is sqlite `unixTempFileInit` | 450 |

Implications for the fix list: F-001 should be closed by removing the process-environment mutation from `test_openrouter_client_base_url_resolution` (inject the base URL) or by marking that test `#[serial]` together with every test that reaches `Repository::discover`; the SAFETY comment at openrouter_client.rs:166 is demonstrably false on both platforms. F-002 stays High on the strength of F-103: the binary is only safe today because the serial test happens to be the first sqlite opener, and neither `serial_test` nor the `EnvVarGuard` SAFETY comment enforces that.
