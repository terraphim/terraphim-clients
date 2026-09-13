# Module: terraphim_update (section E)

Scope: `crates/terraphim_update/src` (10 files, 5,542 lines) and `crates/terraphim_update/tests` (4 files, 1,511 lines). Crate version 1.20.2. Mapper: p1-E, 2026-09-11.

Sweeps run with `rg` over both directories: `unsafe|SAFETY`, `Atomic|Ordering::`, `Arc<|Mutex|RwLock|OnceLock|LazyLock|thread::spawn|tokio::spawn|spawn_blocking`, `block_on|Runtime::new|Handle::current`, `impl Drop`, `env::var|set_var|remove_var`, `#[serial]`, `self_replace|self_update|rename(|persist(|remove_file`, `impl (Hash|PartialEq|Eq|Ord|PartialOrd|Borrow|Iterator) for`, `catch_unwind`, plus `transmute|from_raw|as_ptr|mem::|process::exit`.

Expansion: `cargo +nightly expand -p terraphim_update --lib` -> `/private/tmp/claude-501/ubx-expand/terraphim_update.rs`, 14,032 lines, exit 0.

## Purpose

Shared self-update library for Terraphim binaries. Two backends: an R2 manifest backend (`manifest.rs`, `downloader.rs`, `signature.rs`, install at `lib.rs:1015`) and a GitHub Releases backend delegated to the `self_update` 0.42 crate (`lib.rs:414`, `lib.rs:574`, `lib.rs:1283`). Also provides a tokio periodic scheduler (`scheduler.rs`), backup/rollback (`rollback.rs`, `lib.rs:1488`, `lib.rs:1530`), persisted update history under the user config dir (`state.rs`), install-path resolution (`platform.rs`), and user notification text (`notification.rs`).

## Unsafe-surface tally

| Kind | Count | Sites |
|---|---|---|
| SOURCE_DIRECT `unsafe` blocks in src | 1 | `src/state.rs:131` (`std::env::set_var` x5 in test helper `setup_temp_config_dir`). Owned by section A; not re-catalogued here. |
| SOURCE_DIRECT `unsafe` in tests/ | 0 | |
| MACRO_GENERATED `unsafe` in expansion | 2 | `unsafe impl ::core::clone::TrivialClone` for `UpdateBackend` (expansion line 3287, source `src/manifest.rs:23`) and `UpdateAction` (expansion line 4614, source `src/notification.rs:10`). Both from `#[derive(Clone)]` on `Copy` enums; compiler-generated. |
| `SAFETY` comments | 1 | `src/state.rs:130`. |
| `impl Drop` | 0 | |
| `catch_unwind` | 0 | |
| raw pointers, FFI, `transmute`, `from_raw`, `mem::` | 0 | |
| `process::exit` | 0 | `std::process::Command` is used only in tests to invoke the `zipsign` CLI (`tests/signature_test.rs:43,248,266,293,311,337,355`; `tests/r2_update.rs:255,294`). |

The expansion contains no `set_var` because `cargo expand --lib` excludes `#[cfg(test)]` modules; the two-site count therefore does not contradict section A's six-site count.

## Concurrency surface

**Zero production atomics.** No `Atomic*` type and no `Ordering::` appears in non-test code anywhere in the crate. The `is_running` flag in `UpdateScheduler` (`src/scheduler.rs:37`) is a plain `bool` mutated only through `&mut self` (`start` at `scheduler.rs:136`, `stop` at `scheduler.rs:191`).

Atomics that do exist are test-only:

| Site | Type | Operations and Ordering | What it synchronises |
|---|---|---|---|
| `src/scheduler.rs:299-303` (`test_scheduler_creation`) | `Arc<AtomicUsize>` | `fetch_add(1, SeqCst)` at line 303 | Nothing in practice: the closure is stored in the scheduler but the test never calls `start`, so the counter is never incremented or read. |
| `tests/manifest.rs:76-98` (`FlakyServer`) | two `Arc<AtomicUsize>` (`fail_count`, `attempts`) | `attempts.fetch_add(1, SeqCst)` line 83; `fail_count.load(SeqCst)` lines 86 and 98; `attempts.load(SeqCst)` lines 163 and 165 in the test body | Request counter shared between the detached server thread and the test thread; `fail_count` is written once before the thread is spawned and only loaded thereafter. |

Production synchronisation primitives:

- `src/scheduler.rs:33-34`: `Arc<UpdateConfig>` and `Arc<dyn Fn() -> Result<UpdateCheckResult> + Send + Sync>` shared into the spawned task.
- `src/scheduler.rs:36`: `tokio::sync::mpsc::UnboundedSender<UpdateNotification>`; receiver returned from `create_notification_channel` (`scheduler.rs:97`). Unbounded: no backpressure.
- `src/scheduler.rs:160`: `tokio::spawn` of the interval loop; `JoinHandle` kept at line 35. `stop` (`scheduler.rs:198-200`) calls `handle.abort()` then awaits the handle under a 5 second `tokio::time::timeout`.
- `src/scheduler.rs:250`: `tokio::task::spawn_blocking` runs the user check closure. `abort()` on the outer task does not cancel an in-flight `spawn_blocking`; the blocking closure runs to completion on the blocking pool.
- `src/lib.rs:263,307,414,574,692,1283`: every public async entry point wraps its blocking work (ureq HTTP, `self_update`, file I/O) in `spawn_blocking`.
- `src/lib.rs:1434`: `tokio::spawn` of the notification-forwarding loop in `start_update_scheduler`; the `UpdateScheduler` value itself is dropped at the end of that function (line 1457, after `Ok(handle)` at 1456) while its spawned interval task keeps running, so the returned `JoinHandle` is for the forwarding loop only and the interval task can no longer be stopped via `stop()`.
- `src/downloader.rs:182` and `src/manifest.rs:184`: `std::thread::sleep` for retry backoff inside blocking code (reached only via `spawn_blocking` from the async API, or directly from the sync API).
- No `Mutex`, `RwLock`, `OnceLock`, `LazyLock` or `static mut` in production code. `Arc<Mutex<i32>>` appears only in `tests/integration_test.rs:411`.
- Test-side threads: `tests/r2_update.rs:26,30` (detached HTTP server, one thread per connection), `tests/manifest.rs:31,80` (one-shot and flaky servers), `tests/integration_test.rs:418` (five threads racing `fs::copy` onto one path under a `TempDir`).

## Runtime-nesting sites

Single site: `src/lib.rs:1407-1410` inside the `check_fn` closure built by `start_update_scheduler` (`lib.rs:1388`):

```rust
tokio::task::block_in_place(|| {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async { check_for_updates_auto(&bin_name, &current_version).await })
})
```

Can this run inside an existing tokio runtime? The closure is never exposed; its only invocation path is `UpdateScheduler::perform_check` -> `tokio::task::spawn_blocking` (`src/scheduler.rs:250`). Facts from tokio 1.52.3 (`Cargo.lock:7135`):

- Blocking-pool threads enter via `rt.enter()` (`tokio-1.52.3/src/runtime/blocking/pool.rs:473`), which sets the current `Handle` but leaves the thread's `EnterRuntime` state as `NotEntered`.
- `block_in_place` (`tokio-1.52.3/src/runtime/scheduler/multi_thread/worker.rs:359`) matches on `(current_enter_context(), has_worker_context)`. On a blocking-pool thread this is `(NotEntered, false)`, the "outside the runtime, blocking is fine" branch: the closure runs directly with no core hand-off.
- `Runtime::new()` then `block_on` calls `enter_runtime` (`tokio-1.52.3/src/runtime/context/runtime.rs:55-73`), which panics only when the thread is already `Entered`. It is not, so the nested multi-thread runtime starts and drives `check_for_updates_auto`, which itself calls `spawn_blocking` on the nested runtime (`lib.rs:1283`).
- Consequence: legal today, both under the multi-thread flavour and under the `#[tokio::test]` default current_thread flavour, because the outer runtime's flavour is invisible from a blocking-pool thread.
- If the closure were ever invoked directly on a current_thread runtime worker (an async task rather than via `spawn_blocking`), `block_in_place` would return the `"can call blocking only when running on the multi-threaded runtime"` error and panic. No such call site exists.
- A fresh multi-thread `Runtime` is constructed and dropped on every scheduler tick. Dropping a runtime from a blocking-pool thread is permitted (the drop-in-async-context panic applies only to `Entered` threads).
- Lifetime: `stop()` aborts the interval task, but the nested runtime lives inside a `spawn_blocking` closure that abort cannot cancel. `check_for_updates_auto` via `self_update` has no explicit timeout; the R2 manifest path (not used by this closure, which is GitHub-only at `lib.rs:1288-1289`) would be up to 3 attempts x 15 s plus backoff (`src/manifest.rs:156-184`).
- Test coverage: no test in src or tests calls `start_update_scheduler`. The seven `#[tokio::test]` scheduler tests (`scheduler.rs:347-511`) use plain closures and the default current_thread flavour.
- `check_for_updates_auto` hard-codes `repo_name("terraphim-ai")` at `lib.rs:1289` whereas `UpdaterConfig::new` uses `terraphim-clients` at `lib.rs:178`. Recorded as a fact only.

No other `Runtime::new`, `Handle::current`, or `block_on` sites exist in the crate.

## Self-replace / file lifecycle

Three distinct binary-replacement mechanisms are reachable:

**1. R2 path: `TerraphimUpdater::install_verified_archive` (`src/lib.rs:1015-1055`) and `promote_staged_binaries` (`lib.rs:1059-1096`).**

- Install directory is `std::env::current_exe()?.parent()` (`lib.rs:1018-1021`).
- Staging directory: `tempfile::tempdir_in(install_dir).or_else(|_| tempfile::tempdir())` (`lib.rs:1030`). The fallback places the staging directory under `TMPDIR`, which may be a different filesystem from `install_dir`.
- Unix promotion (`lib.rs:1059-1077`): `chmod 0755` then `std::fs::rename(&staged, &dst)`. There is no copy fallback on this branch, so a cross-filesystem staging directory would make `rename` return `EXDEV`. The comment at `lib.rs:1026-1029` states the same-filesystem invariant; the fallback at line 1030 does not enforce it.
- With same-filesystem staging, `rename(2)` over the running executable is atomic; the old inode stays mapped by the running process. There is no window in which the running executable is unlinked or truncated on Unix.
- Windows promotion (`lib.rs:1082-1096`): `remove_file(&dst)` then `rename`, with a copy-then-delete fallback. Between the remove and the rename there is a window where no binary exists at `dst`. Not compiled on this host (macOS).
- Both `bin_name` and `bin_name.replace('_', "-")` are promoted if present in staging (`lib.rs:1062,1085`).
- Archive extraction: `extract_tarball` (`lib.rs:1148-1173`) extracts only the entry whose `file_name()` equals `bin_name` into `target_dir.join(bin_name)`, so tar member paths cannot escape staging. `extract_zip` (`lib.rs:1124-1145`) uses `file.mangled_name()` joined onto `target_dir`; `mangled_name` strips `..` components but is documented by the `zip` crate as less strict than `enclosed_name`. Only reached on Windows (`lib.rs:1041`).
- The staging `TempDir` is dropped at the end of `install_verified_archive`, removing any leftover files.

**2. GitHub path: `self_update` 0.42 (`lib.rs:574-611`, `lib.rs:1283`).**

- `builder.bin_install_path(platform::get_binary_path(&bin_name)?)` is set at `lib.rs:431`, `lib.rs:590`, and `lib.rs:1297`. In `self_update-0.42.0/src/update.rs`, `bin_install_path` is read at line 163 and used only for a `println!` at line 256. The actual replacement at `update.rs:308` is `self_replace::self_replace(new_exe)`, which targets `env::current_exe()` regardless of `bin_install_path`.
- `self-replace` 1.5.0 `src/unix.rs:13-47`: resolves one level of symlink via `read_link` (not `canonicalize`), copies the new binary into a `tempfile` created in the executable's parent directory, copies the old permissions, `keep()`s the tempfile, then `fs::rename` over the executable. On rename failure the temp file is removed. No unlink or truncate of the running executable.
- `self_replace::self_delete` (`unix.rs:7-11`, `remove_file` of the canonicalised exe) exists in the dependency but is not called anywhere in `terraphim_update` or in `self_update-0.42.0/src/update.rs`.
- `Move::execute` in `self_update-0.42.0/src/lib.rs:581-606` (rename with optional temp swap-back) is a public helper; not called from `terraphim_update`.

**3. Rollback paths: truncate-in-place.**

- `lib.rs:1536` (`rollback`): `fs::copy(backup_path, target_path)`.
- `src/rollback.rs:310` (`BackupManager::rollback_to_version`): `fs::copy(&backup.path, target_path)`.
- `std::fs::copy` opens the destination with truncation. These are the only two sites in the crate that overwrite an existing file at the binary's path in place instead of renaming over it. Whether any caller passes the currently running executable as `target_path` is outside this crate (see Open questions).
- Backup creation is `fs::copy` to a new sibling path (`lib.rs:1500`, `rollback.rs:170`); `rollback.rs` also computes a SHA-256 of the copy (`rollback.rs:171,182-188`) and re-checks it before rollback (`rollback.rs:301`).
- Deletions: `rollback.rs:230` (rotation), `rollback.rs:347` (`delete_backup`), `rollback.rs:366` (`cleanup`), `state.rs:98` (`delete_update_history`). All target backup or history files, never the executable.

**Other file-lifecycle sites:**

- `src/platform.rs:155-164` `check_write_permissions`: writes a fixed-name file `.write_test_12345` into the probed directory and removes it. Probed directories include `current_exe().parent()` (`platform.rs:48-57`) and `/usr/local/bin` (`platform.rs:70`). Two processes probing the same directory concurrently race on the shared file name.
- `src/platform.rs:83` `get_binary_path`: `fs::create_dir_all("$HOME/.local/bin")` as a side effect of a path lookup when the earlier candidates are not writable.
- `src/state.rs:18` `get_history_path`: `fs::create_dir_all` of `dirs::config_dir()/terraphim` on every call, including from `history_exists` (`state.rs:112`).
- `src/downloader.rs:226`: `File::create(output_path)` then streamed write; the caller in `update_r2` names the download after the asset inside a `TempDir` (`lib.rs:337-345`, see `signature.rs:162-166` for why the file name matters as the zipsign context).
- `tests/r2_update.rs:107-113,139-140`: the test computes `current_exe().parent()` (the cargo `target/debug/deps` directory) and calls `remove_file` on `terraphim-test` there before asserting that the rejected unsigned archive is not installed.

## Lib test binary thread model

The lib test binary (`cargo test -p terraphim_update --lib`) contains 130 tests across ten `#[cfg(test)]` modules. The default harness runs them on a thread pool sized to the CPU count. Only the 10 tests in `state.rs` carry `#[serial]` (`state.rs:144-311`); `serial_test` serialises annotated tests among themselves only, so the remaining 120 tests run concurrently with the `set_var` block at `state.rs:131`. The relevant race is any `getenv` concurrent with `setenv`, so the reader set below counts explicit `env::var` reads, `dirs::` lookups (which read `XDG_CONFIG_HOME`/`HOME`), and `tempfile`/`std::env::temp_dir()` (which read `TMPDIR`).

| Module | Tests | `#[serial]` | Env-reading tests | Reads |
|---|---|---|---|---|
| `src/config.rs` | 10 | 0 | 0 | none |
| `src/downloader.rs` | 11 | 0 | 6 | `tempfile::tempdir()` at 398, 419, 453, 488, 509, 534 (`TMPDIR`) |
| `src/lib.rs` | 19 | 0 | 12 | `UpdaterConfig::new` reads `TERRAPHIM_UPDATE_BACKEND`, `TERRAPHIM_UPDATE_BASE_URL`, `GITHUB_TOKEN` (`lib.rs:159,167,183`) in `test_updater_config` 1569 (`#[tokio::test]`), 1586, 1592, 1598, 1609, 1851; `NamedTempFile::new` (`TMPDIR`) in 1618, 1650, 1671, 1681; `test_check_for_updates_auto` 1713 (`#[tokio::test]`) reads `GITHUB_TOKEN` at 1293 and performs live GitHub HTTP plus `platform::get_binary_path` (reads `HOME` at `platform.rs:74` if earlier candidates are unwritable) |
| `src/manifest.rs` | 11 | 0 | 0 | none (uses `std::env::consts` only, compile-time) |
| `src/notification.rs` | 15 | 0 | 0 | none |
| `src/platform.rs` | 12 | 0 | 8 | `get_binary_path` (probes `current_exe().parent()` with a file write, may read `HOME` at 74 and `create_dir_all` `$HOME/.local/bin` at 83) in 245, 262, 281, 411; `get_config_dir` reads `HOME` at 112 in 287, 402; `std::env::temp_dir()` at 303; `tempfile::TempDir::new` at 311 |
| `src/rollback.rs` | 18 | 0 | 18 | `TempDir::new()` in every test (395-722) |
| `src/scheduler.rs` | 10 | 0 | 0 | none; 7 are `#[tokio::test]` (347, 372, 391, 429, 462, 480, 501) on the current_thread flavour, 3 are sync |
| `src/signature.rs` | 14 | 0 | 7 | `NamedTempFile::new` at 416, 434, 445, 559, 577; `tempfile::tempdir` at 472; 592 has no tempfile but is counted via the shared helper |
| `src/state.rs` | 10 | 10 | 10 | writer: `set_var` of `HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`, `APPDATA`, `LOCALAPPDATA` (131-136); readers: `dirs::config_dir()` at `state.rs:16` and `TempDir::new()` at 127 |

Additional cross-test interactions in the lib binary:

- `platform.rs` tests 245, 262, 281, 411 and `lib.rs:1713` all call `get_binary_path`, which writes and removes `.write_test_12345` in `target/debug/deps` (`platform.rs:155-164`); concurrent calls race on that one name (a losing `fs::write` yields a false "not writable" and falls through to `/usr/local/bin` and then `$HOME`).
- `platform.rs:287,402` (`get_config_dir`) and `platform.rs:74` read `HOME` while `state.rs` tests may have pointed `HOME` at a `TempDir`; a read during the serial window yields the temp path, and after the `TempDir` is dropped the path no longer exists. `state.rs` never restores the original values.
- `lib.rs:1713` `test_check_for_updates_auto` performs live network I/O to GitHub and tolerates `Failed`.
- `downloader.rs:415` connects to `localhost:9999` expecting refusal; `downloader.rs:397,452` use invalid URLs.

Integration test binaries (`crates/terraphim_update/tests/`), each a separate process that never shares an address space with the `state.rs` writer:

| Binary | Tests | `#[serial]` | Notes |
|---|---|---|---|
| `integration_test.rs` | 15 | 0 | All under `TempDir`; `test_concurrent_update_attempts` (400) spawns 5 threads racing `fs::copy` onto one file with an `Arc<Mutex<i32>>` success counter (411); `test_update_history_persistence` (222) and `test_history_schema_evolution` (480) use `temp_dir.path().join(...)`, not `state::get_history_path()`; `test_permission_failure_scenarios` (110) chmods a readonly dir. No env reads beyond `TMPDIR`. |
| `manifest.rs` | 4 | 0 | Real `TcpListener` servers on `127.0.0.1:0`; `test_fetch_manifest_retry_on_500_then_succeed` (154) sleeps through `manifest.rs:184` backoff. |
| `r2_update.rs` | 6 | 0 | 4 `#[tokio::test]` (117, 155, 176, 198) drive `update_r2` against a local server; 117 removes and checks `terraphim-test` in `current_exe().parent()`; 246 shells out to `zipsign` and fails if it is absent. `UpdaterConfig::new` at 231-240 reads the three env vars. |
| `signature_test.rs` | 18 (15 unconditional, 3 behind `integration-signing` feature) | 0 | Tempfiles only; the feature-gated module (240-389) shells out to `zipsign`. |

## Library-trait invariant surface

- No hand-written `impl Hash`, `PartialEq`, `Eq`, `Ord`, `PartialOrd`, `Borrow`, `Iterator`, `Deref`, `AsRef`, `Send`, or `Sync` anywhere in src or tests. Hand-written impls are limited to `Default` (`manifest.rs:71`, `config.rs:39`, `config.rs:139`, `downloader.rs:46`) and `Display` for `UpdateStatus` (`lib.rs:86`); no invariant-bearing trait.
- Derived `PartialEq, Eq` at `signature.rs:90`, `config.rs:23,66,111,192,205`, `notification.rs:10`, `manifest.rs:23,46`. No derived `Hash` or `Ord`; nothing is used as a `HashMap` key by identity except `String` version keys in `BackupManager` (`rollback.rs:66-92`, `HashMap<String, Backup>`).
- `BackupManager` ordering is by `Backup.timestamp` (`jiff::Timestamp`): `rotate_backups` sorts ascending (`rollback.rs:214`) and removes the oldest; `list_backups` sorts by `Reverse(timestamp)` (`rollback.rs:271`) so `rollback_to_latest` (`rollback.rs:329-331`) takes `versions[0]` as newest. Two backups created within the same timestamp tick have unspecified relative order.
- `is_newer_version_static` (`lib.rs:52`, used at 264, 313, 444) is a semver comparison with `v` prefix stripping; `UpdateStatus` variants carry `String`s only.
- `Send + Sync` bounds are placed on the check closure (`scheduler.rs:34,78,247`) and the callback (`lib.rs:1391`); satisfied structurally, no manual impls.

## Notable patterns

- Every async API in `lib.rs` funnels blocking work through `spawn_blocking` and converts `JoinError` into `UpdateStatus::Failed` rather than propagating (`lib.rs:280-283`, `620-640`, `715-733`).
- `update_r2` return contract distinguishes `Ok(Failed)` (definitive, no fallback) from `Err` (transport, fallback to GitHub) (`lib.rs:293-300`); `check_and_update_r2` (`lib.rs:1193`) relies on it.
- Signature verification is mandatory on the R2 path (`lib.rs:790-803`: `MissingSignature` is rejected) and on the GitHub verified path via `self_update`'s `signatures` feature (`lib.rs:585`).
- Env-driven configuration at construction time (`lib.rs:159,167,183`) with no caching; every `UpdaterConfig::new` call re-reads the three variables.
- The `start_update_scheduler` helper builds a nested runtime per tick rather than reusing the caller's runtime (see Runtime-nesting sites).
- `src/state.rs` test helper mutates five process-global env vars and never restores them; its `SAFETY` comment asserts isolation that the harness does not provide.
- `crates/terraphim_update/test_debug.rs` (crate root, 14 lines) is a stray `fn main` not referenced by `Cargo.toml`; not compiled.

## Open questions

1. Does any consumer crate call `terraphim_update::rollback` (`lib.rs:1530`) or `BackupManager::rollback_to_version` (`rollback.rs:296`) with `target_path` equal to the running executable? Those are the only truncate-in-place writes; on macOS `fs::copy` onto a running Mach-O succeeds and invalidates mapped text pages, on Linux it returns `ETXTBSY`. Outside section E scope.
2. Under what conditions does `tempfile::tempdir_in(install_dir)` fail at `lib.rs:1030` such that the cross-filesystem fallback is taken, and has the resulting `EXDEV` on the unix `rename` at `lib.rs:1073` ever been observed?
3. Is `start_update_scheduler` (`lib.rs:1388`) called by any binary in the workspace, and if so on which runtime flavour? There is no test for it, and the dropped `UpdateScheduler` at `lib.rs:1457` means the interval task can only be aborted by dropping the runtime.
4. Is the `repo_name("terraphim-ai")` at `lib.rs:1289` in `check_for_updates_auto` intentional, given `UpdaterConfig::new` uses `terraphim-clients` at `lib.rs:178`?
5. Which `HOME` value do `platform.rs:287` and `platform.rs:402` observe when the lib test binary interleaves them with a `state.rs` `#[serial]` test? Determining this empirically needs a Miri or `--test-threads` run in phase 2.
