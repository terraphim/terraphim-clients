# Phase 2 findings: bucket 7 data-races (including "env-setter vs getenv")

Run: 2026-09-11-terraphim-clients-1. Sweeper: static-bucket-sweeper (data-races). Written 2026-09-11 23:51 BST.
Host: darwin arm64, 11 logical CPUs (`sysctl -n hw.ncpu`), rustc 1.97.1, workspace edition 2024.
Source is unmodified. Search tooling: `rg`, `fd`, `ast-grep`. No experiments were executed in this phase; every "Draft experiment" is a proposal.

Citation note: the 1.97.1 toolchain has no `rust-src` component on this host, so std line numbers below are from `~/.rustup/toolchains/1.96.0-aarch64-apple-darwin/lib/rustlib/src/rust/library/std/src/`. Registry crate versions cited are the ones cargo resolves for the binary in question (`cargo tree -p <crate> -e normal,dev --offline`); note that `terraphim_agent` resolves `terraphim_config`, `terraphim_settings` and `terraphim_persistence` at 1.20.2 from the `terraphim` registry (`~/.cargo/registry/src/git.terraphim.cloud-72dcd9d4c11821d8/`), not the crates.io 1.20.4 copies that phase 1 section C cited.

## 0. Inputs filtered from phase 1

Rows tagged data-race / env-mutation / env-read-native / concurrency / shared-service-state / check-then-act in `phase1_unsafe_surface_inventory.md` and notes A, B, C, E, F, G:

| Source | Row | Bucket tag |
|---|---|---|
| A, E | crates/terraphim_update/src/state.rs:131-137 `set_var` x5 | env-mutation, data-race |
| A, F | crates/terraphim_grep/src/openrouter_client.rs:170,177,185,189 `set_var`/`remove_var` | env-mutation, data-race |
| A, G | crates/terraphim_mcp_server/tests/test_mcp_stdio.rs:15-20 `set_var` | env-mutation |
| C | crates/terraphim_agent/tests/tui_service_tests.rs:37,40,43 + Drop -> terraphim_test_utils-1.20.3 `set_var`/`remove_var` | data-race |
| G | 8 `terraphim_test_utils::set_env_var` sites in tests/mcp_autocomplete_e2e_test.rs and tests/mcp_rolegraph_validation_test.rs | env-mutation (hidden) |
| F | hybrid_searcher.rs:341-402 -> fff-search -> git2 -> libgit2 `getenv` | env-read-native |
| F | openrouter_client.rs:51 + reqwest/hyper-util proxy env | env-read-std |
| B | registry.rs:25-37, service.rs:16, kg_validation.rs:28, repl/handler.rs:1909, shared_learning/store.rs:136,140,608 | concurrency |
| D | sessions/service.rs:18,22 (`maybe_auto_import` check-then-act), redaction.rs:55 OnceLock | concurrency, check-then-act |
| E | scheduler.rs:299, tests/manifest.rs:76-98 atomics; scheduler `is_running` plain bool | concurrency (test) |
| G | mcp_server/src/lib.rs:99,372,418,510,788; main.rs:295 clone-per-connection | concurrency, shared-service-state |
| H | terraphim_lsp server.rs:18-19,46-73; terraphim_cli service.rs:20,266,296,335; terraphim_command_runtime lib.rs:18-68 | concurrency |

## 1. The race model used for every verdict

Two facts anchor the env verdicts, both verified in source:

1. **The write side is locked inside std only.** `std::env::set_var` -> `sys::env::unix::setenv` takes `ENV_LOCK.write()` and then calls `libc::setenv` (std `sys/env/unix.rs:67` static, `:132` and `:140` for setenv/unsetenv). Every `std::env::var`/`var_os` reader takes `ENV_LOCK.read()` (`:70`). `std::process::Command::spawn` also takes `env_read_lock()` before dereferencing `environ` (`sys/process/unix/unix.rs:93`, `:763-764`), and tokio's `Command::spawn` delegates to `self.std.spawn()` (tokio-1.52.3 `src/process/mod.rs:865`). So all Rust-side readers, including subprocess spawns, are serialised against the write: a concurrent Rust-side read is a *logical* race (observing an intermediate value), never a memory race.
2. **Foreign readers are outside that lock, and std says so.** `std/src/env.rs:367-389` (the `# Safety` section of `set_var`): "you must ensure that there are no other threads concurrently writing or *reading*(!) the environment through functions or global variables other than the ones in this module ... most C libraries, including libc itself, do not advertise which functions read from the environment". A libc `getenv` that scans `environ` while `setenv`/`unsetenv` rewrites it is an unsynchronised read/write of shared memory: a data race under the C memory model on every platform, and the Linux glibc runner used by `.github/workflows/ci.yml:26` (`cargo test --workspace --lib`) is the configuration where `getenv` is documented to scan `environ` without a lock. I did not read Apple's Libc and make no claim about its internals.

Verdict scale: MUST-BE-UB (a native reader is guaranteed to overlap the write in every run) / LIKELY-UB (native reader and writer are in the same process with no ordering constraint; overlap is scheduling-dependent) / SUSPICIOUS (contract violated, reader status unresolved) / CONTRACTUAL-BUT-DEFENSIBLE (contract violated as written, but every reader found is behind std's lock or sequenced after the write).

Harness model common to all binaries (verified in phase 1, no `harness = false`, no `--test-threads`, no `RUST_TEST_THREADS`, no `nextest.toml`): libtest runs each test on its own spawned thread with up to `available_parallelism()` (11 here) tests in flight; `serial_test` 3.5.0 `#[serial]` excludes only other `#[serial]`/`#[parallel]` tests in the same process.

## 2. Env-mutation sites: race partners per binary

### 2.1 terraphim_grep lib test binary (writer: openrouter_client.rs:170,177,185,189)

(a) **Concurrent tests.** 57 tests, zero `#[serial]`, `serial_test` not a dependency. All 56 siblings may run concurrently with `test_openrouter_client_base_url_resolution` (`crates/terraphim_grep/src/openrouter_client.rs:164-193`), whose SAFETY comment at :166 ("test code, single-threaded, no concurrent env access") is false on both counts.

(b) **Native reader: yes, verified chain.**

- `HybridSearcher::search_code` (`crates/terraphim_grep/src/hybrid_searcher.rs:341-365`): builds `fff_search::FilePicker` (`:353-360`) then calls `picker.collect_files()` (`:362-364`).
- fff-search-0.8.4 `src/file_picker.rs:796` `collect_files` -> `:800` `FileSync::discover_git_workdir(&self.base_path)` -> `:1695` `git2::Repository::discover(base_path)`.
- git2-0.20.4 `src/repo.rs:248-250` `discover` calls `crate::init()` first; `src/lib.rs:748-756` `init` is `Once`-guarded and calls `raw::init()`.
- libgit2-sys-0.18.5+1.9.4 `lib.rs:4708-4716` `init`: `Once` -> `git_libgit2_init()`.
- `libgit2/src/libgit2/libgit2.c:34-58` `git_libgit2_init` runs the init list including `git_sysdir_global_init` (`:46`).
- `libgit2/src/libgit2/sysdir.c:453-465` `git_sysdir_global_init` loops over every `guess` fn eagerly. `git_sysdir_guess_home_dirs` (`:324-368`) calls `getenv("APP_SANDBOX_CONTAINER_ID")` (`:347`) and `git__getenv(out, "HOME")` (`:357`); `git_sysdir_guess_xdg_dirs` (`:375-412`) calls `git__getenv(&env, "XDG_CONFIG_HOME")` (`:402`) and `"HOME"` (`:405`).
- `libgit2/src/util/util.c:786-788` (non-Windows) `git__getenv` is `const char *val = getenv(name);` with no lock.

Reached by 8 lib tests (all `#[tokio::test]` current-thread, so the call runs on the libtest worker thread): `src/lib.rs:607,640,673,741,792,840` and `src/hybrid_searcher.rs:484,526`. `git2::init` is `Once`, so only the first of the eight to reach `collect_files` performs the native reads; the window is once per process and ordering-dependent.

Rust-side (locked) readers of the mutated variable: `openrouter_client.rs:51` via tests at :158, :196, :205 (logical race only; they do not assert `base_url`). Other locked readers on the code-search path: `ignore` (`XDG_CONFIG_HOME`, `HOME` via std), rayon (`RAYON_NUM_THREADS`), tempfile (`TMPDIR`), hyper-util proxy vars.

(c) **Verdict: LIKELY-UB.** Unsynchronised libc `getenv` and std `setenv`/`unsetenv` run in the same process on different threads with nothing ordering them. Whether the two overlap in a given run depends on which of the eight tests wins the `Once` and when the openrouter test is scheduled. Not MUST because overlap is not guaranteed in every execution.

### 2.2 terraphim_agent `tui_service_tests` binary (writer: terraphim_test_utils-1.20.3 via EnvVarGuard)

Writer: `crates/terraphim_agent/tests/tui_service_tests.rs:33-61` `test_tui_service_new_uses_host_settings_path` (`#[tokio::test] #[serial]`) calls `EnvVarGuard::set` three times (`:37` HOME, `:40` XDG_CONFIG_HOME, `:43` TERRAPHIM_DATA_PATH). `terraphim_test_utils-1.20.3/src/lib.rs:36-42` `set_env_var` is `unsafe { std::env::set_var }` under `cfg(rust_has_unsafe_env_setters)`, which that crate's `build.rs` always emits (edition 2024 is hard-coded true at build.rs:25-29). The three guards restore on drop (`lib.rs:106-113`), so there are six mutation points: three at test start, three at test end after a full `TuiService::new` (which itself bootstraps settings and builds every role graph, so the two windows are well separated in time).

(a) **Concurrent tests.** 10 tests; only the writer is `#[serial]` (`:13-243` attribute list: 9 bare `#[tokio::test]`). The nine siblings run on their own libtest threads concurrently with both mutation windows.

(b) **Native reader: yes, verified chain, on the sibling test threads.**

- Four of the nine siblings reach `TuiService::get_thesaurus` (`crates/terraphim_agent/src/service.rs:462-464`) -> `TerraphimService::ensure_thesaurus_loaded`: `test_tui_service_autocomplete` (test :95 -> `autocomplete` :619-626), `test_tui_service_replace_matches` (test :123 -> `replace_matches` :658-665), `test_tui_service_find_matches` (test :194 -> `find_matches` :644-650), and `test_tui_service_search` (test :70-72 -> `search_with_role` :431 -> `TerraphimService::search`, terraphim_service-1.21.1 `src/search.rs:201` -> `:741` `self.ensure_thesaurus_loaded(&role.name)` on the TerraphimGraph branch). The embedded config's selected role is "Terraphim Engineer" with `RelevanceFunction::TerraphimGraph` (terraphim_config-1.20.2 `src/lib.rs:666`, `:610`). Not counted: `test_tui_service_summarize` (:150) goes through `chat` (:674-678) and `build_llm_from_role` (:562), which fails before any thesaurus read when the embedded role has no LLM; `test_tui_service_update_selected_role` (:257) reaches `Config::save` only through `tokio::spawn` (terraphim_service-1.21.1 `src/lib.rs:204-211`) on a current_thread runtime, and tokio's `block_on` loop (tokio-1.52.3 `src/runtime/scheduler/current_thread/mod.rs:205-224`) runs spawned tasks only when the main future returns Pending, so that save is scheduling-dependent and may never execute.
- terraphim_service-1.21.1 `src/thesaurus.rs:393`: `rolegraph_value.lock().await.thesaurus.clone().load().await` runs unconditionally whenever the role has a rolegraph, which `ConfigState::new` (terraphim_config-1.20.2 `src/lib.rs:1121`, local-KG branch `:1219-1252`) guarantees for Terraphim Engineer.
- `Persistable::load` -> `load_config` -> `DeviceStorage::instance()` (terraphim_persistence-1.20.2 `src/lib.rs:237`, `:73-81`) -> `init_device_storage` (`:129-143`, reads `TERRAPHIM_SETTINGS_PATH` via std at `:131`) -> `DeviceSettings::load_from_env_and_file` (terraphim_settings-1.20.2 `src/lib.rs:221`; default profiles include `sqlite` at `default/settings.toml:12-13`, path `${TERRAPHIM_DATA_PATH:-~/.terraphim}/sqlite/terraphim.db` expanded through `std::env::var` at `src/lib.rs:158`) -> `settings::parse_profiles` (`src/lib.rs:197` -> `src/settings.rs:349`).
- `settings.rs:277-314` `Scheme::Sqlite` arm calls `ensure_sqlite_table_exists` (`:95-140`), which does `rusqlite::Connection::open(db_path)` (`:110`) **on the calling thread** (no spawn_blocking); rusqlite-0.32.1 `src/inner_connection.rs:88` -> `sqlite3_open_v2`. Feature `sqlite` is in terraphim_persistence's `default` set (Cargo.toml:46-50, :63-66) and cargo resolves it on for terraphim_agent (`cargo tree -e features` line 2122).
- libsqlite3-sys-0.30.1 `sqlite3/sqlite3.c`: `sqlite3_open_v2` -> `sqlite3_initialize` -> `sqlite3_os_init` -> `unixTempFileInit()` (`:46631`) -> `:44351-44354` `azTempDirs[0] = getenv("SQLITE_TMPDIR"); azTempDirs[1] = getenv("TMPDIR");`. Plain libc `getenv`, no lock, once per process (sqlite's own init is idempotent and `DEVICE_STORAGE` is an `AsyncOnceCell`, `lib.rs:55`).
- After the table check, `parse_profiles` probes every operator with `op.write("test", ..)`/`op.read("test")` (`settings.rs:176-179`), so the opendal sqlite backend also connects (`opendal-0.54.1 src/services/sqlite/backend.rs:185-188`, lazily via sqlx) on an sqlx worker thread (sqlx-sqlite-0.8.6 `src/connection/worker.rs:106`); by then `sqlite3_initialize` has already run, so no further `getenv`. Note that sqlite keeps the raw `getenv` pointers in `azTempDirs` and dereferences them on every later temp-file creation (`:44360-44397`); this is only a dangling-pointer hazard if `TMPDIR`/`SQLITE_TMPDIR` are themselves mutated, which no test in this workspace does.

Locked (Rust-side) readers concurrent with the writer, for the logical-race record: `DeviceSettings::new` (`terraphim_settings-1.20.2 src/lib.rs:98`, `ProjectDirs::from` at `:161,:198` -> dirs-sys-0.5.0 `var_os("HOME")`; `std::env::var("HOME")` at `:163,:204`) from every `TuiService::new_with_embedded_defaults` (`crates/terraphim_agent/src/service.rs:216-231` -> `ConfigBuilder::new_with_id` -> terraphim_config-1.20.2 `src/lib.rs:565-568` `DeviceSettings::new()`); the Ripgrep haystack `rg` subprocess (terraphim_middleware-1.21.0 `src/command/ripgrep.rs:284`, tokio -> std spawn under `env_read_lock`); `TERRAPHIM_DATA_PATH` interpolation at settings `:158`.

(c) **Verdict: LIKELY-UB.** Same structure as 2.1 but with a stronger reachability argument: four of the nine non-serial tests execute the native `getenv` path on their own thread (whichever is first through `DEVICE_STORAGE`), all ten tests start within the same scheduling burst on an 11-CPU host, and the writer has two mutation windows. Not run in CI today (`ci.yml` runs `--lib` only; `native-ci.yml:54` runs `--all-targets`, so the Gitea runner does execute it).

### 2.3 terraphim_update lib test binary (writer: state.rs:131-137)

(a) **Concurrent tests.** 130 tests; the 10 in `state.rs` are `#[serial]` (`:144-312`), the other 120 are not and run concurrently with `setup_temp_config_dir` (`crates/terraphim_update/src/state.rs:127-140`). The SAFETY comment at `:130` ("runs in isolation before spawning other threads") is false: libtest workers are already executing sibling tests.

(b) **Native reader: none found.** Checked for this binary's dependency tree (`cargo tree -p terraphim_update -e normal,dev --offline`):

- No libgit2-sys, no libsqlite3-sys, no openssl/native-tls, no aws-lc-sys in the tree. `-sys` crates present: core-foundation-sys, dirs-sys 0.4.1, libbz2-rs-sys, zstd-sys, ring 0.17.14.
- ring 0.17.14: `rg -l getenv` over the crate source returns nothing.
- dirs-sys-0.4.1 `src/lib.rs:34` `env::var_os("HOME")` (std lock) with `getpwuid_r` fallback (`:52`; reads passwd, not `environ`).
- chrono 0.4.45 + iana-time-zone 0.1.65 are linked, but `rg 'Local::now\(\)' crates/*/src` finds no call in any workspace crate, and self_update-0.42.0 / dialoguer-0.12.0 have none either. jiff is used only as `Timestamp::now()` (`config.rs:7`, `platform.rs:369,390`, `state.rs:187,206`), a clock read.
- console 0.15/0.16 (via dialoguer/self_update) read `TERM`, `NO_COLOR`, `LANG` via `std::env`. ureq 2.12.1 proxy discovery reads `*_proxy` via `std::env::var` (`src/proxy.rs:77-90`).
- tokio `spawn_blocking` threads (lib.rs:263-1283, scheduler.rs:250) run self_update/ureq/rustls/ring code; none of it was found to call `getenv`.
- No `std::process::Command` in `src/`; `tempfile` and `std::env::temp_dir` are std-locked.

Residual not verified dynamically: `test_check_for_updates_auto` (`lib.rs:1713`) performs a live HTTPS request through self_update -> ureq -> rustls (ring); DNS resolution goes through `std::net::ToSocketAddrs` -> `getaddrinfo`, which std's own `set_var` doc at env.rs:383-385 names explicitly as a function that "may read the environment without going through this module". I did not read the libc resolver sources; resolv.conf(5) documents environment overrides for the resolver, which is why the read cannot be ruled out. That is a plausible native reader on Linux CI (`ci.yml:26` runs `--lib` for the workspace, so this binary runs there) whenever the network test actually resolves `api.github.com` while a `state.rs` test is inside its `set_var` block.

(c) **Verdict: SUSPICIOUS.** The SAFETY claim is false and soundness rests on a property the comment does not state (all readers go through std's lock). Every reader identified in source is locked, which would make it CONTRACTUAL-BUT-DEFENSIBLE, but the live `getaddrinfo` path in `test_check_for_updates_auto` is a resolver-library read that I could not rule out by source inspection, so it stays one notch up. Independent of UB: HOME, USERPROFILE, XDG_CONFIG_HOME, APPDATA, LOCALAPPDATA are never restored, and the `TempDir` they point at is deleted at each test's end, so `platform.rs:287,402` (`env::var("HOME")` at `platform.rs:112`) and `platform.rs:74` can observe a non-existent HOME for the remainder of the process.

### 2.4 terraphim_mcp_server `test_mcp_stdio` binary (writer: test_mcp_stdio.rs:15-20)

(a) One test in the binary (`:6`), so one libtest worker plus the parked main thread. (b) No native reader: the only other thread-creating call is `Command::spawn` at `:24-32`, sequenced after the write on the same thread and taking `env_read_lock` itself. Gated by `RUN_MCP_STDIO_TEST=1` (`:10`), which neither CI workflow exports. (c) **Verdict: CONTRACTUAL-BUT-DEFENSIBLE.** No SAFETY comment; the `:14` comment is descriptive.

### 2.5 terraphim_mcp_server `set_env_var` sites (tests/mcp_autocomplete_e2e_test.rs:22,28; tests/mcp_rolegraph_validation_test.rs:21,27,137,284,394,618)

(a) Every test in both binaries is `#[serial]` (8/8 and 4/4 per section G), so no sibling test thread is alive during a write. (b) Threads left over from earlier tests: each `#[tokio::test]` runtime is dropped at test end and `Runtime::drop` joins its blocking pool; child servers are separate processes; `DeviceStorage::init_memory_only()` (`e2e:30`, `rolegraph:28`) builds only the memory operator, so no sqlite init and no sqlx worker thread. `TokioChildProcess` spawns go through std's `env_read_lock`. (c) **Verdict: CONTRACTUAL-BUT-DEFENSIBLE.** Residual: the `#[serial]` attribute lives on the tests while the mutation lives one level down in a shared setup fn, so a future non-serial test calling that helper silently breaks the invariant.

## 3. Findings

## F-001: `set_var`/`remove_var` in terraphim_grep lib tests races libc `getenv` inside `git_libgit2_init`

**File:line:** crates/terraphim_grep/src/openrouter_client.rs:170-172, :177-179, :185-187, :189-191 (writer); crates/terraphim_grep/src/hybrid_searcher.rs:362-364 -> fff-search-0.8.4/src/file_picker.rs:800,1695 -> git2-0.20.4/src/repo.rs:250, src/lib.rs:748 -> libgit2-sys-0.18.5+1.9.4/lib.rs:4708 -> libgit2/src/libgit2/libgit2.c:46 -> sysdir.c:347,357,402,405 -> util.c:788 (reader).
**Kind:** unsynchronised libc `getenv` concurrent with `setenv`/`unsetenv` (std `ENV_LOCK` covers only the write side).
**Bucket(s):** 7 (+ ffi-transitive, safety-comment-accuracy).
**Severity:** High (memory-unsafe read of `environ` on the Linux CI runner path `.github/workflows/ci.yml:26`; probability per run is low because the reader is `Once`).
**Static evidence:** Writer SAFETY comment at :166 reads `// SAFETY: test code, single-threaded, no concurrent env access.`; the binary has 57 tests, no `#[serial]`, default libtest parallelism. Reader chain quoted in section 2.1(b); the terminal read is `const char *val = getenv(name);` (util.c:788) with no synchronisation. Reached by 8 tests: src/lib.rs:607,640,673,741,792,840; src/hybrid_searcher.rs:484,526. std's contract (env.rs:374-389) forbids exactly this.
**Draft experiment:**
```
# ThreadSanitizer, instrumenting the vendored libgit2 C too (cc crate honours CFLAGS):
RUSTFLAGS="-Zsanitizer=thread" CFLAGS="-fsanitize=thread" \
  cargo +nightly test -Zbuild-std --target aarch64-apple-darwin -p terraphim_grep --lib
# Expected: "WARNING: ThreadSanitizer: data race" naming setenv/unsetenv (from std::env::set_var
# on the openrouter test thread) against getenv in git__getenv (sysdir.c:357) on another test thread.
# Cheap differential (no sanitiser): loop 200x at default parallelism vs --test-threads=1 and vs a
# filtered run "test_openrouter_client_base_url_resolution search_without_thesaurus_uses_fff_mode";
# absence of a crash is not evidence of safety (single Once window). Miri cannot run this FFI path.
```
**Cross-refs:** A (openrouter rows), F (reachability table tests 2-4, 7-9, 14-15), bucket ffi (libgit2 init), F-007 (logical race on the same variable).

## F-002: `EnvVarGuard` in tui_service_tests races libc `getenv` inside sqlite `unixTempFileInit`

**File:line:** crates/terraphim_agent/tests/tui_service_tests.rs:37,40,43 and implicit Drop at :61 -> terraphim_test_utils-1.20.3/src/lib.rs:39,61,106-113 (writer); crates/terraphim_agent/src/service.rs:462-464,626,650,665 and :431 -> terraphim_service-1.21.1/src/search.rs:741 -> src/thesaurus.rs:393 -> terraphim_persistence-1.20.2/src/lib.rs:237,73,129,197 -> src/settings.rs:277-314,95-110 -> rusqlite-0.32.1/src/inner_connection.rs:88 -> libsqlite3-sys-0.30.1/sqlite3/sqlite3.c:46631,44351-44354 (reader).
**Kind:** unsynchronised libc `getenv` (`SQLITE_TMPDIR`, `TMPDIR`) concurrent with `setenv`/`unsetenv` of HOME, XDG_CONFIG_HOME, TERRAPHIM_DATA_PATH.
**Bucket(s):** 7 (+ ffi-transitive, safety-comment-accuracy, hidden-unsafe-in-safe-wrapper).
**Severity:** High. Four of nine non-serial tests (search, autocomplete, find_matches, replace_matches) execute the reader on their own thread; the writer has two mutation windows; the binary runs under `.gitea/workflows/native-ci.yml:54`.
**Static evidence:** The writer test is `#[serial]` but its nine siblings are bare `#[tokio::test]` (`:13-243`); `serial_test` does not exclude unannotated tests. The library wrapper's SAFETY comment (`lib.rs:37-38`, "Tests using this should be marked with #[serial] to ensure isolation") gives a rule that cannot satisfy its own stated precondition in a mixed binary. Reader chain in section 2.2(b); the terminal reads are `azTempDirs[0] = getenv("SQLITE_TMPDIR"); azTempDirs[1] = getenv("TMPDIR");` executed from `sqlite3_os_init` on the thread that first calls `rusqlite::Connection::open` (settings.rs:110, not wrapped in `spawn_blocking`). `ensure_thesaurus_loaded` calls `.load()` unconditionally at thesaurus.rs:393 for any role with a rolegraph, which the embedded "Terraphim Engineer" role has after `ConfigState::new`.
**Draft experiment:**
```
RUSTFLAGS="-Zsanitizer=thread" CFLAGS="-fsanitize=thread" \
  cargo +nightly test -Zbuild-std --target aarch64-apple-darwin -p terraphim_agent --test tui_service_tests
# Expected: TSan report pairing setenv (EnvVarGuard::set / Drop) with getenv in unixTempFileInit
# (sqlite3.c:44352-44353) on a sibling test thread; a second report is possible against
# sqlite3_open_v2's SQLITE_FORCE_PROXY_LOCKING read only if SQLITE_OPEN_AUTOPROXY were set (it is not).
# Cheap differential: `--test-threads=1` vs default over 100 iterations; filtered pair
# "test_tui_service_new_uses_host_settings_path test_tui_service_autocomplete".
# libsqlite3-sys is built with feature "bundled" in this graph (cargo tree -e features lines 2144-2154),
# so CFLAGS reaches the sqlite3.c compile and the getenv frame will be symbolised.
```
**Cross-refs:** C (notable pattern 1), G (same wrapper), F-006, F-008.

## F-003: terraphim_update `setup_temp_config_dir` violates the `set_var` contract with a false SAFETY claim; no native reader found, HOME left dangling

**File:line:** crates/terraphim_update/src/state.rs:130-137.
**Kind:** env-mutation with 120 concurrent std-locked readers; unresolved resolver-library reader on the live-network test.
**Bucket(s):** 7 (+ safety-comment-accuracy, test-hygiene).
**Severity:** Medium (SUSPICIOUS; no confirmed out-of-lock reader).
**Static evidence:** `// SAFETY: This test runs in isolation before spawning other threads` precedes five `set_var` calls; the binary has 130 tests of which only the 10 in this module are `#[serial]`. All readers found are `std::env::var`/`var_os` (platform.rs:112 via tests :287,:402; lib.rs:159,167,183 via :1569-1609; tempfile TMPDIR; dirs-sys HOME). Native candidates ruled out: ring (no `getenv`), no libgit2/sqlite/openssl in tree, no `chrono::Local` anywhere in the workspace. Unresolved: `test_check_for_updates_auto` (lib.rs:1713) resolves `api.github.com` through `getaddrinfo` inside a `spawn_blocking` thread; std's own `set_var` doc names DNS lookups as an out-of-module reader. Values are never restored and the `TempDir` is deleted at test end, so HOME points at a removed directory for the rest of the process.
**Draft experiment:**
```
RUSTFLAGS="-Zsanitizer=thread" cargo +nightly test -Zbuild-std --target x86_64-unknown-linux-gnu \
  -p terraphim_update --lib   # on the Linux CI image, network allowed
# Expected: clean if the resolver does not touch environ; a report pairing setenv with getenv inside
# res_init/getaddrinfo would upgrade this to LIKELY-UB. Also run with --test-threads=1 as control.
# Logical check: run `test_get_config_dir` after any state.rs test with `--test-threads=1
# --skip nothing` and assert the returned path exists (it will not once the TempDir is gone).
```
**Cross-refs:** A, E (lib test thread model table), F-007 pattern (no restore on panic).

## F-004: test_mcp_stdio `set_var` without a SAFETY comment; sound only by being a one-test binary

**File:line:** crates/terraphim_mcp_server/tests/test_mcp_stdio.rs:15-20.
**Kind:** env-mutation, single-threaded by construction.
**Bucket(s):** 7.
**Severity:** Low (CONTRACTUAL-BUT-DEFENSIBLE).
**Static evidence:** One `#[test]` in the file; `Command::spawn` at :24-32 follows the write on the same thread and takes std's env read lock. Comment at :14 describes intent, not safety. Gated by `RUN_MCP_STDIO_TEST` (:10); not exported by any workflow. Adding a second test to this file would silently create the F-001 shape.
**Draft experiment:** none needed; a one-line `cargo test -p terraphim_mcp_server --test test_mcp_stdio -- --list` confirms the single test.
**Cross-refs:** A, G.

## F-005: `terraphim_test_utils::set_env_var` sites in mcp_server integration tests are serialised, but the invariant is enforced one level away from the mutation

**File:line:** crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs:22,28; tests/mcp_rolegraph_validation_test.rs:21,27,137,284,394,618.
**Kind:** hidden `unsafe` env mutation behind a safe fn; all callers `#[serial]`.
**Bucket(s):** 7 (+ hidden-unsafe-in-safe-wrapper).
**Severity:** Low (CONTRACTUAL-BUT-DEFENSIBLE).
**Static evidence:** Every test in both binaries is `#[serial]`; `init_memory_only` builds only the memory operator (no sqlite, no sqlx thread); child servers are separate processes; previous tests' tokio runtimes are dropped (blocking pool joined) before the next serial test starts. The mutation sits inside a shared async setup fn while the attribute sits on the tests.
**Draft experiment:** `cargo test -p terraphim_mcp_server --test mcp_rolegraph_validation_test -- --list` and a `rg '#\[serial\]' -c` per file must stay equal to the test count; a CI guard test could assert that.
**Cross-refs:** G, F-006.

## F-006: `terraphim_test_utils` SAFETY comment prescribes `#[serial]`, which does not satisfy the stated precondition in a mixed binary

**File:line:** ~/.cargo/registry/src/git.terraphim.cloud-72dcd9d4c11821d8/terraphim_test_utils-1.20.3/src/lib.rs:21-25 (crate doc), :37-38, :59-60 (block comments); consumers at crates/terraphim_agent/tests/tui_service_tests.rs:37-43 and the F-005 sites.
**Kind:** unsound safe abstraction (safe fn whose soundness depends on a caller-side rule that is insufficient).
**Bucket(s):** 7 (+ library-invariant).
**Severity:** Medium (root cause of F-002; external crate, so the fix lands outside this workspace).
**Static evidence:** The crate says "Callers must ensure: No other threads are concurrently reading or writing the same env vars; Tests using these functions should use #[serial]". `serial_test` only excludes annotated tests, so a `#[serial]` caller in a binary with unannotated siblings follows the rule and still violates the invariant, as tui_service_tests demonstrates. The wrapper is a safe `pub fn`, so no `unsafe` block appears at the call site and section A's `rg unsafe` sweep missed it. `crates/terraphim_agent/build.rs:19` emits a `rust_has_unsafe_env_setters` cfg with zero consumers in the workspace (dead since 58d3c2d).
**Draft experiment:** none (design finding). Remediation candidates for the hand-off: make the wrapper `unsafe fn`, or have it take a process-wide `std::sync::Mutex` that every env-reading test also takes, or mark whole binaries serial.
**Cross-refs:** C (open question 2), A (dead cfg row), F-002, F-005.

## F-007: Logical race: sibling openrouter tests observe `OPENROUTER_BASE_URL` mid-mutation; restore is skipped on panic

**File:line:** crates/terraphim_grep/src/openrouter_client.rs:51 (reader) via tests :158, :196, :205; writer :164-193, restore :184-192.
**Kind:** std-locked logical race (flaky observation), plus non-RAII restore.
**Bucket(s):** 7 (logical, not UB).
**Severity:** Low.
**Static evidence:** Tests 54/56/57 (section F numbering) call `OpenRouterClient::new`, which reads the variable through `std::env::var` (locked). They assert `model`/`api_key`/`name` only, so the flake is latent. If `assert_eq!` at :174 or :181 panics, the restore branch never runs and the process keeps `https://example.com/v1` for every later test.
**Draft experiment:** add a temporary assertion on `base_url` in test 54 and loop the binary; expect sporadic failures at default parallelism and none at `--test-threads=1`.
**Cross-refs:** F-001, A.

## F-008: Logical race: `DEVICE_STORAGE` singleton binds the sqlite path to whichever HOME/TERRAPHIM_DATA_PATH is live at first init

**File:line:** terraphim_persistence-1.20.2/src/lib.rs:55 (`static DEVICE_STORAGE: AsyncOnceCell`), :129-143; terraphim_settings-1.20.2/src/lib.rs:158 (`${TERRAPHIM_DATA_PATH:-~/.terraphim}` expansion), :161-163, :198-204 (HOME); writer crates/terraphim_agent/tests/tui_service_tests.rs:37-43.
**Kind:** check-then-act on process-global state via std-locked env reads (no UB).
**Bucket(s):** 7 (logical), cross-tag shared-service-state.
**Severity:** Medium for test reliability (silent persistence failures, real `~/.terraphim` writes from tests).
**Static evidence:** Whichever test first reaches `DeviceStorage::instance()` fixes the sqlite datadir for the process. If that happens while the serial test's guards are live, the path is under a `TempDir` deleted at :61; every later `save()` warns and fails (`search.rs:782-789` logs and continues). If it happens before or after, the nine embedded-defaults tests write to the developer's real `~/.terraphim/sqlite/terraphim.db` and the sqlx pool keeps a WAL open for the process lifetime. Neither outcome is deterministic.
**Draft experiment:** run the binary with `RUST_LOG=warn` and `--test-threads=10` twenty times; grep for "Failed to save document" and for a `sqlite` directory appearing under a vanished `/var/folders/.../tmp*` path; compare with `--test-threads=1`.
**Cross-refs:** F-002, C (notable pattern 2 on inherited HOME), G (open question 1).

## F-009: `CommandRegistry` readers acquire `aliases`/`commands` in opposite orders under a write-preferring RwLock

**File:line:** crates/terraphim_agent/src/commands/registry.rs:149-152 (`resolve_command`: aliases -> commands) vs :244-246 (`get_stats`: commands -> aliases -> categories); writers :103-129, :226-234, :623-627.
**Kind:** lock-order inversion (deadlock hazard, not UB).
**Bucket(s):** 7 (cross-tag deadlock).
**Severity:** Low (needs two concurrent writers plus both readers; sharing model unresolved, section B open question 2).
**Static evidence:** tokio-1.52.3 `src/sync/rwlock.rs:41-43` documents the lock as fair/write-preferring with a FIFO queue, so a queued writer blocks later readers. Cycle: `get_stats` holds `commands.read` waiting on `aliases`; `resolve_command` holds `aliases.read` waiting on `commands`; one writer queued on each. Writers take one lock at a time, so the cycle requires two writers in flight.
**Draft experiment:** scratch `#[tokio::test(flavor = "multi_thread")]` spawning 4 tasks (two readers, two `register_command` writers) in a loop under `tokio::time::timeout(5s)`; a timeout demonstrates the hang. Alternatively a loom model of the four lock acquisitions.
**Cross-refs:** B.

## F-010: Tokio guards held across awaits into persistence and search (starvation/serialisation, not UB)

**File:line:** crates/terraphim_agent/src/service.rs:350,449,457,463 (`TuiService.service` guard across `TerraphimService` awaits); crates/terraphim_agent/src/repl/handler.rs:1914 (`SESSION_SERVICE` guard across awaits, order SESSION_SERVICE -> TuiService.service, consistent); crates/terraphim_command_runtime/src/lib.rs:66-70 (`config` guard across `config.save().await`); crates/terraphim_cli/src/service.rs:266,296,335; crates/terraphim_mcp_server/src/lib.rs:510-581 (`autocomplete_index.read()` held across `service.search(&sq).await` at :534,:553).
**Kind:** await-holding-lock (tokio locks; no poisoning, no UB).
**Bucket(s):** 7 (cross-tag deadlock/starvation).
**Severity:** Low. The mcp case is a stall, not a deadlock: `service.search` never re-enters `autocomplete_index`, but a queued writer (:372/:788) blocks all other readers until the awaited search returns. `command_runtime::save_config` holds the config lock across persistence I/O while terraphim_service's own `update_selected_role` (terraphim_service-1.21.1 `src/lib.rs:176-215`) deliberately snapshots and drops before saving, so the workspace is inconsistent with itself.
**Static evidence:** quoted line ranges; `ast-grep` rule `await-holding-mutex.yml` matches only `std` `.lock().unwrap()` and therefore reports nothing here (see section 4).
**Draft experiment:** none for soundness. For the mcp stall: two concurrent `call_tool` requests (autocomplete with snippets, then `build_autocomplete_index`) plus a third reader, measure the third's latency.
**Cross-refs:** B, G, H.

## F-011: `KG_CACHE` std `RwLock` held across filesystem I/O on a tokio worker; poison is fail-open

**File:line:** crates/terraphim_agent/src/kg_validation.rs:28, :85-116.
**Kind:** std lock held across blocking I/O in a sync fn called from async (`main.rs:2232`); `.ok()?` on poison.
**Bucket(s):** 7 (cross-tag poisoning, blocking-in-async).
**Severity:** Low. No `.await` under the guard, so not an await-holding-lock; a panic inside `compute_kg_source_hash`/`build_kg_thesaurus_with_hash` while holding the write guard poisons the lock and every later validation silently returns `None` for the process lifetime.
**Static evidence:** fast path :87-93 holds `read()` across `compute_kg_source_hash` (directory walk); slow path :97-116 holds `write()` across hash + `find_kg_dir` + automaton build; both use `.ok()?`.
**Draft experiment:** unit test that injects a panicking KG file reader under the write guard, then asserts the next call returns `Some` (it will return `None`).
**Cross-refs:** B.

## F-012: `maybe_auto_import` check-then-act allows duplicate imports

**File:line:** crates/terraphim_sessions/src/service.rs:72-98.
**Kind:** TOCTOU between `auto_import_attempted.read()` (:78-83) and `.write()` (:96-97); `import_all` awaited in between with no guard held.
**Bucket(s):** 7 (logical check-then-act).
**Severity:** Low (duplicate work, idempotent import; no UB).
**Static evidence:** two concurrent callers both see `attempted == false` and `cache.is_empty()`, both run `import_all`, both set the flag.
**Draft experiment:** spawn two `search` calls on a fresh `SessionService` under `tokio::join!` with a counting connector; expect the import count to be 2.
**Cross-refs:** D.

## F-013: `shared_learning::store::block_on` bridge panics on a current_thread runtime

**File:line:** crates/terraphim_agent/src/shared_learning/store.rs:608-612 (`block_in_place(|| Handle::current().block_on(fut))`), used by the sync `LearningStore` impl at :622-745.
**Kind:** runtime-flavour dependency (panic, not UB; no deadlock because `block_in_place` hands the worker's tasks off).
**Bucket(s):** 7 (cross-tag runtime-nesting).
**Severity:** Low. Production caller is `terraphim_middleware::feedback_loop` (cross-crate); in-crate tests are all `multi_thread`.
**Static evidence:** section B runtime-nesting table; `Handle::current()` panics with no runtime, `block_in_place` panics on current_thread or inside a `LocalSet`.
**Draft experiment:** `#[tokio::test]` (current_thread) calling any sync `LearningStore` method; expected panic "can call blocking only when running on the multi-threaded runtime".
**Cross-refs:** B (open question 1).

## 4. ast-grep pattern run (step 4)

Script run as requested:

```
$ bash ~/.claude/skills/rust-undefined-behavior-exorcist/scripts/ast-grep-ub-patterns.sh /Users/alex/projects/terraphim/terraphim-clients data-races
No patterns found.
```
(exit 1: the pattern directory has no `data-races-*.yml`; the files present are listed below and none carries that prefix.)

`ls ~/.claude/skills/rust-undefined-behavior-exorcist/scripts/patterns/`: aliasing-cast-ref-to-mut, aliasing-deref-while-borrowed, alignment-repr-packed-field-ref, async-drop-block-on, atomic-relaxed-load-store, await-holding-mutex, const-mutation-cast, custom-Hash-without-Eq, float-from-int-bits, get-unchecked, lifetime-escape-as-ptr, manual-Allocator-impl, manual-Iterator-size-hint, MaybeUninit-write-after-assume-init, pin-new-unchecked, provenance-int-cast, raw-fd-without-OwnedFd, refcount-from-raw, repr-Rust-transmute, send-sync-manual-impl, set-len, slice-from-raw-parts, type-punning-transmute, uninit-maybeuninit-assume-init, uninit-mem-zeroed-uninit, utf8-as-bytes-index.

The four race-adjacent rules were run by hand over `crates/` (the script's first attempt with `crates src` failed because there is no top-level `src/`):

```
$ for p in await-holding-mutex atomic-relaxed-load-store send-sync-manual-impl async-drop-block-on; do
    echo "=== $p"; ast-grep scan --rule ~/.claude/skills/rust-undefined-behavior-exorcist/scripts/patterns/$p.yml crates; done
=== await-holding-mutex
=== atomic-relaxed-load-store
=== send-sync-manual-impl
=== async-drop-block-on
```

Zero matches for all four. Blind spot: `await-holding-mutex.yml` matches only `let $GUARD = $MUTEX.lock().unwrap();` followed by an `.await` in the same fn (std mutexes), so it cannot see the tokio `.lock().await` / `.read().await` guards held across awaits listed in F-010; those were found by reading. `send-sync-manual-impl` returning nothing is the positive evidence for section 5.

## 5. Safe-code concurrency: is there any real data race?

No. Evidence:

- No `unsafe impl Send`/`Sync` anywhere in `crates/` (ast-grep, section 4), no `static mut`, no raw pointers, no `UnsafeCell` wrappers, no `transmute` (phase 1 tallies for every section).
- Every piece of shared mutable state found is behind `Arc` + a tokio or std lock (`registry.rs`, `service.rs`, `store.rs`, `handler.rs`, `kg_validation.rs`, `sessions/service.rs`, `mcp_server/src/lib.rs:99`, `lsp/server.rs:18-19`, `command_runtime`), a `OnceLock`/`LazyLock`/`AsyncOnceCell` (`capture.rs:733`, `handler.rs:1909`, `redaction.rs:55`, persistence `DEVICE_STORAGE`), or a `SeqCst` atomic (`terraphim_update` tests, `mcp_server/tests/support/mod.rs:25`, `terraphim_grep/src/lib.rs:520` test double). `AtomicUsize` in `terraphim_update/src/scheduler.rs:299-303` is never read. `UpdateScheduler.is_running` (`scheduler.rs:37`) is a plain `bool` touched only through `&mut self`.
- `McpService` clones (`main.rs:295`) alias one `Arc<RwLock<Option<AutocompleteIndex>>>`; all access is through the lock.
- `RetrievedChunk.haystack: &'static str` (`terraphim_grep/src/hybrid_searcher.rs:42`) is populated from literals only.
- std `Mutex` poisoning: the only `.lock().unwrap()` on a std mutex is `#[cfg(test)]` `logging.rs:154,272` (a panic under the lock inside one test makes later `log!` calls in that test panic inside the logger). `kg_validation.rs:87,97` and `store.rs:667` recover from poison by failing open (`.ok()?`, `if let Ok`). Tokio locks do not poison; `registry.rs:226-234` `reload` clears three maps in separate scopes, so a panic mid-way leaves a partially cleared registry (state, not memory, hazard).

## 6. Non-findings ruled out

| Candidate | Where | Why it is not a race partner |
|---|---|---|
| `std::process::Command::spawn` (rg haystack, mcp child servers, CLI tests) | terraphim_middleware-1.21.0 command/ripgrep.rs:284; tokio process/mod.rs:865 -> std | std takes `env_read_lock()` before reading `environ` (sys/process/unix/unix.rs:93, :763-764); `Command::env` mutates only the child's captured map |
| rayon `RAYON_NUM_THREADS`, `ignore` `XDG_CONFIG_HOME`/`HOME`, tempfile `TMPDIR`, dirs-sys `HOME`, hyper-util proxy vars, console `TERM`/`NO_COLOR`, tokio `TOKIO_WORKER_THREADS`, settings `${VAR}` expansion | fff-search / ignore / tempfile / dirs-sys 0.4.1:34 & 0.5.0:34 / hyper-util matcher.rs:230,325,411 / terraphim_settings lib.rs:158 | all `std::env::var`/`var_os`, behind `ENV_LOCK` |
| `SCDynamicStoreCopyProxies` | hyper-util 0.1.20 matcher.rs:575-590 via reqwest `Client::build` (grep tests 54-57) | native, but reads the macOS dynamic store, not `environ` |
| `getpwuid_r` fallback | dirs-sys :52; libgit2 sysdir.c `get_passwd_home` | reads passwd, not the environment |
| aws-lc-sys / ring | linked via rustls in grep and update binaries | grep: reqwest selects the ring provider; ring source has no `getenv`; aws-lc `getenv` lives only in x509 by_dir/by_file and test code, not on `Client::build` |
| sqlite `SQLITE_FORCE_PROXY_LOCKING` getenv | sqlite3.c:44853 | guarded by `isAutoProxy = (flags & SQLITE_OPEN_AUTOPROXY)` (:44633); neither rusqlite nor sqlx sets that flag |
| sqlite `azTempDirs` raw `getenv` pointers reused later | sqlite3.c:44352-44353, :44360-44397 | dangling only if `TMPDIR`/`SQLITE_TMPDIR` is mutated; no workspace test mutates them |
| iana-time-zone / CoreFoundation time zone | linked via chrono in agent, grep, update trees | no `chrono::Local` call in any workspace crate or in self_update/dialoguer; `Utc::now()`/`jiff::Timestamp::now()` only |
| libgit2 `spawn_git_status` and `Repository::open` config reads | fff-search file_picker.rs:801,1708 | not reached from tests: the tempdir fixtures are outside any repository on this host (phase 1 F verified) |
| LMDB (`heed`/`lmdb-master-sys`) | fff-search frecency | `SharedFrecency::default()` holds no tracker on the `collect_files` path |
| `DeviceStorage::init_memory_only` | mcp_server e2e/rolegraph tests | memory operator only; no sqlite open, no sqlx worker thread |
| Section C integration binaries other than tui_service_tests | 46 binaries | env writes are child-scoped (`Command::env`), or in-process reads only; each binary is its own process |
| Bench `Runtime::new()` (multi-thread) | terraphim_grep/benches/hybrid_search.rs:81,112 | `harness = false`, not part of `cargo test`; separate process |
| `static WARNED: Once` | terraphim_grep/src/hybrid_searcher.rs:414 | compiled out under default `code-search` feature |
| `Arc<tokio::RwLock<RoleGraph>>` in grep | hybrid_searcher.rs:219,239,318 | read-locked only; no writer in the crate |
| `terraphim_update` `spawn_blocking` threads | lib.rs:263-1283, scheduler.rs:250 | run ureq/rustls/ring/self_update code with no `getenv` found; only the DNS resolver remains open (F-003) |
| `extract_functionality_validation.rs:96-109` `SERVER_PORT` check-then-set | terraphim_agent tests | all four tests `#[serial]` |
| terraphim_lsp `pending` guard across `tokio::spawn` | server.rs:46-73 | no `.await` under the guard; spawned task never touches `pending` (entries are never pruned, a leak, not a race) |
| terraphim_negative_contribution, terraphim_hooks | section H | no concurrency primitives at all |

## 7. Verdict summary

| Binary | Writer | Concurrent tests | Native reader | Verdict |
|---|---|---|---|---|
| terraphim_grep lib | openrouter_client.rs:170-191 | 56 (no serial) | yes: libgit2 sysdir `getenv` x4, `Once`, 8 tests | LIKELY-UB (F-001) |
| terraphim_agent tui_service_tests | terraphim_test_utils via EnvVarGuard, 6 mutation points | 9 (writer alone is serial) | yes: sqlite `unixTempFileInit` `getenv` x2 via rusqlite on the test thread, 6 tests | LIKELY-UB (F-002) |
| terraphim_update lib | state.rs:131-137 | 120 (10 serial) | none found; DNS resolver on `test_check_for_updates_auto` unresolved | SUSPICIOUS (F-003) |
| terraphim_mcp_server test_mcp_stdio | test_mcp_stdio.rs:15-20 | 0 | none | CONTRACTUAL-BUT-DEFENSIBLE (F-004) |
| terraphim_mcp_server e2e / rolegraph_validation | terraphim_test_utils::set_env_var x8 | 0 (all serial) | none (memory-only persistence) | CONTRACTUAL-BUT-DEFENSIBLE (F-005) |
