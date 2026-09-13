# Module

Section C: `crates/terraphim_agent/tests/*.rs` (47 auto-discovered integration test binaries, shared helpers in `tests/support/`) and the inline `#[cfg(test)]` modules under `crates/terraphim_agent/src` (52 files, 658 inline tests: 650 compile into the lib test harness and the 8 in `src/main.rs` into the bin test harness).

Toolchain facts: no `harness = false`, no `[[test]]` overrides, no `.cargo/config.toml` test flags (only registry config), no `--test-threads`/`RUST_TEST_THREADS` in `.github/workflows/ci.yml`. CI runs `cargo test --workspace --lib` plus only the `packaged_install_graph_regression` integration binary; every other integration binary in this section runs only on developer machines. `tests/command_system_integration_tests.rs.backup` is dead (not `.rs`).

Date of sweep: 2026-09-11.

## Purpose

Establish whether any test in this crate mutates process-global state (environment, cwd, signal handlers, global loggers) in a way that races with other tests in the same binary, and catalogue shared state between tests and between binaries.

## Unsafe-surface tally

| Category | Count | Where |
|---|---|---|
| `unsafe` blocks in section source | 0 | rg over tests/ and src/ |
| `SAFETY` comments in section source | 0 | none needed |
| Direct `std::env::set_var` / `remove_var` | 0 | none |
| `std::env::set_current_dir` | 0 | none; all `.current_dir(` are `Command::current_dir` |
| Signal handlers / `panic::set_hook` / `catch_unwind` | 0 | none |
| Global logger installs | 1 | `src/logging.rs:117` under `Once`, idempotent |
| Indirect env mutation via dependency `unsafe` | 3 call sites + 3 implicit `Drop` restores, all in one test | `tests/tui_service_tests.rs:37,40,43` -> `terraphim_test_utils-1.20.3/src/lib.rs:39,61` |
| `static` items in tests | 7 | 5 `OnceLock`/`OnceCell` binaries and server URLs, 1 `Mutex<Option<Child>>`, 1 `AtomicU64` |
| `#[serial]` attributes | 108 in 14 binaries | see table |
| `#[tokio::test(flavor = "multi_thread")]` | 1 in tests (`persistence_tests.rs:354`), 10 in `src/shared_learning/store.rs` (section B) | |
| In-process `thread::spawn` / `tokio::spawn` | 3 sites | `extract_functionality_validation.rs:17`, `persistence_tests.rs:386`, `error_handling_test.rs:257` |

## Per-test-binary thread model

Legend for `#[serial]`: all = every test serial; none; mixed = some. Env writes column lists in-process writes first, then `Command::env` keys marked (child only), which are safe.

| test file | number of tests | any #[serial] | env writes | env reads (in-process) | shared global state |
|---|---|---|---|---|---|
| ci_guards.rs | 2 | none | none | none | TempDir per test |
| cli_auto_route.rs | 2 | all (2; third rg hit is a doc comment) | `env_remove` RUST_LOG, JMAP_ACCESS_TOKEN (child only) | none | none |
| command_system_integration_tests.rs | 10 | none | none | `current_dir()` :375 | TempDir x4 |
| comprehensive_cli_tests.rs | 10 | all | none | none | spawns agent binary with inherited HOME |
| cross_mode_consistency_test.rs | 3 | all | TERRAPHIM_SERVER_HOSTNAME, TERRAPHIM_SETTINGS_PATH, RUST_LOG (child only) | `TERRAPHIM_SERVER_BIN`, `current_dir()` | portpicker port; `temp_dir()/terraphim_test_<port>`, `terraphim_settings_<port>`, `terraphim_test_config_<port>.json`; temp_dir sweep :371; `$CARGO_TARGET_TMPDIR/kg/test_ranking_kg.md` |
| enhanced_search_tests.rs | 22 | none | none | none | none |
| error_handling_test.rs | 10 | all | none | none | external localhost:8000 (skips if down); `tokio::spawn` x10 |
| execution_mode_tests.rs | 16 | none | none | none | none |
| exit_codes_integration_test.rs | 6 | none | none | none | none |
| exit_codes.rs | 11 | none | none | none | TempDir; fixed URL 127.0.0.1:19999 (expected unreachable) |
| extract_feature_tests.rs | 4 | none | none | none | none |
| extract_functionality_validation.rs | 4 | all | TERRAPHIM_SERVER_PORT, TERRAPHIM_SERVER_HOSTNAME (child only) | `CI`, `GITHUB_ACTIONS`, `USER`, `HOME`, `TERRAPHIM_*_BIN` | `static SERVER_PORT: OnceLock`, `static SERVER_CHILD: Mutex<Option<Child>>` (never killed); `TcpListener::bind(127.0.0.1:0)` for port; detached waiter thread |
| file_operations_basic_tests.rs | 7 | none | none | none | none |
| file_operations_command_parsing.rs | 5 | none | none | none | none |
| guard_priority.rs | 5 | none | none | none | TempDir |
| hook_safety.rs | 5 | none | hermetic root (HOME, XDG_CONFIG_HOME, TERRAPHIM_SETTINGS_PATH, TERRAPHIM_DEFAULT_DATA_PATH) (child only) | `current_dir()` via helper | unique hermetic root |
| hook_system_tests.rs | 20 | none | none | none | TempDir x6; `git` subprocesses with `Command::current_dir` |
| integration_test.rs | 14 | all | TERRAPHIM_SERVER=localhost:8000 (child only) | none | external localhost:8000 |
| integration_tests.rs | 5 | all | TERRAPHIM_SERVER_HOSTNAME, TERRAPHIM_CLIENT_TIMEOUT, hermetic root (child only) | `TERRAPHIM_AGENT_BIN`, `TERRAPHIM_SERVER_BIN`, `TERRAPHIM_CLIENT_TIMEOUT`, `current_dir()` | `static AGENT_BINARY/SERVER_BINARY: OnceLock`; portpicker; deletes fixed `temp_dir()/{terraphim_sqlite,dashmaptest,terraphim_rocksdb,opendal}` |
| kg_ranking_integration_test.rs | 3 | all | TERRAPHIM_SERVER_HOSTNAME, TERRAPHIM_SETTINGS_PATH, RUST_LOG, XDG_DATA_HOME (child only) | `TERRAPHIM_SERVER_BIN`, `current_dir()` | `static SHARED_SERVER_URL: OnceCell` (Child dropped, process leaked by design); portpicker; deletes fixed `temp_dir()/{terraphim_sqlite,dashmaptest,opendal,terraphim_test_kg}`; `$CARGO_TARGET_TMPDIR/kg/test_ranking_kg.md` |
| learn_no_service_tests.rs | 4 | none | none | none | TempDir x4 |
| offline_mode_tests.rs | 24 | all | hermetic root (child only) | none | unique hermetic root; fixed URL localhost:9999 (expected unreachable) |
| onboarding_integration.rs | 11 | none | none | none | TempDir |
| packaged_install_graph_regression.rs | 1 | none | CARGO_TARGET_DIR, hermetic root (child only) | `EXE_SUFFIX` | TempDir x7; nested cargo builds |
| persistence_tests.rs | 9 | mixed (8 of 9; the multi_thread test is intentionally not serial) | HOME, XDG_CONFIG_HOME, hermetic root (child only) | none | hermetic root; `tokio::spawn` x5 on `Arc<ConfigState>` |
| phase1_robot_mode_tests.rs | 18 | none | none | none | none |
| procedure_cli_tests.rs | 15 | none | HOME, XDG_DATA_HOME, XDG_CACHE_HOME (14 sites) (child only) | none | TempDir per test |
| repl_integration_tests.rs | 30 | none | none | none | `ApiClient` to localhost:8000 constructed, not necessarily called |
| replace_feature_tests.rs | 14 | none | none | `CI`, `GITHUB_ACTIONS`, `USER`, `HOME`, `CARGO_MANIFEST_DIR` | reads `docs/src/kg` fixtures read-only |
| robot_schemas.rs | 7 | none | none | none | TempDir |
| robot_search_output_regression_tests.rs | 5 | all | none | `TERRAPHIM_AGENT_BIN` | `static AGENT_BINARY: OnceLock`; insta snapshots |
| rolegraph_suggestions_tests.rs | 18 | none | none | none | none |
| selected_role_tests.rs | 8 | all | none | none | spawns agent binary with inherited HOME (in-memory storage per comment :393) |
| server_mode_tests.rs | 11 | all | TERRAPHIM_SERVER_HOSTNAME, RUST_LOG (child only) | `CI`, `GITHUB_ACTIONS`, `USER`, `HOME`, `TERRAPHIM_SERVER_BIN` | portpicker; server child killed per test |
| sessions_cli_contract.rs | 7 | none | hermetic root (child only) | none | unique hermetic root |
| sessions_docs_drift.rs | 3 | none | hermetic root + caller-supplied pairs (child only) | none | unique hermetic root |
| sessions_repl_parser_tests.rs | 13 | none | none | none | none |
| shared_learning_cli_tests.rs | 9 | none | none | none | TempDir x3 |
| tui_service_tests.rs | 10 | mixed (1 of 10) | HOME, XDG_CONFIG_HOME, TERRAPHIM_DATA_PATH via `EnvVarGuard::set` (:37,:40,:43) plus 3 restores on drop | all 9 other tests read HOME through `DeviceSettings::new()` -> `ProjectDirs::from` | in-process only; LIVE RACE, see Notable patterns |
| unit_test.rs | 17 | none | none | none | `ApiClient` to localhost:8000 |
| update_functionality_tests.rs | 9 | none | none | `CI`, `GITHUB_ACTIONS`, `USER`, `HOME` | spawns agent binary with inherited HOME |
| user_prompt_submit_tests.rs | 4 | none | hermetic root (child only) | `TERRAPHIM_AGENT_BIN` | unique hermetic root |
| vm_api_tests.rs | 13 | none | none | none | none |
| vm_functionality_tests.rs | 9 | none | none | none | none |
| vm_management_tests.rs | 27 | none | none | none | none |
| web_operations_basic_tests.rs | 16 | none | none | none | none |
| web_operations_tests.rs | 33 | none | none | none | none |
| lib + bin test harnesses (inline `#[cfg(test)]` in src) | 658 (650 lib, 8 bin) | none | none | `GITEA_REPO`, `GITEA_TOKEN` (`src/shared_learning/wiki_sync.rs:458,467`) | `TempDir` throughout; `src/lib.rs:164` fixed-prefix tmp file with `subsec_nanos` suffix; `src/logging.rs` `Once` guarded logger |

## Notable patterns

1. Live data race in `tests/tui_service_tests.rs` (the only finding that meets the section's brief). `test_tui_service_new_uses_host_settings_path` (:33-61) is `#[serial]` and sets HOME, XDG_CONFIG_HOME and TERRAPHIM_DATA_PATH in-process through `terraphim_test_utils::EnvVarGuard::set`, whose body is `unsafe { std::env::set_var }` (registry crate 1.20.3, `src/lib.rs:39`; `remove_var` at :61; `Drop` restore at :106-113). `#[serial]` only excludes other `#[serial]` tests, and the other nine tests in the binary are unannotated. Each of them calls `TuiService::new_with_embedded_defaults`, which reaches `ConfigBuilder::new_with_id` -> `DeviceSettings::new()` (`terraphim_config-1.20.4/src/lib.rs:556`) -> `ProjectDirs::from` / `std::env::var("HOME")` (`terraphim_settings-1.20.4/src/lib.rs:161-163`). So `setenv` on one thread runs concurrently with `getenv` on up to nine others, which is the exact precondition the library's SAFETY comment says the caller must exclude. The library comment is PRESENT_WEAK: it recommends `#[serial]`, but following that recommendation does not satisfy the stated invariant in a mixed binary. The `unsafe` itself is outside the workspace, so section A's env-unsafe list does not contain it. This binary is not exercised in CI.

2. Every other environment mutation in the section is child-process scoped (`Command::env`, `env_remove`), which is safe. Seventeen binaries spawn `terraphim-agent` or the server with the parent's HOME inherited (comprehensive_cli_tests, selected_role_tests, update_functionality_tests, exit_codes*, hook_system_tests, replace_feature_tests, robot_*, cli_auto_route, guard_priority, learn_no_service_tests, ci_guards, and server_mode_tests, extract_functionality_validation, cross_mode_consistency_test and kg_ranking_integration_test, which set some `Command::env` keys but not HOME); nine use the hermetic root helper in `tests/support/cli_test_env.rs`. Inherited-HOME spawns contend on the real user config and default `/tmp/terraphim_sqlite` (the helper's own comment at :80-86 documents multi-minute WAL stalls), which is a cross-process contention issue rather than a memory-safety one.

3. Cross-binary shared state guarded only by a process-local lock. `serial_test` serialises within one binary. `integration_tests.rs:213-229` and `kg_ranking_integration_test.rs:221-233` delete fixed-name directories under `temp_dir()` that are also the default persistence locations for any running agent or server. `cargo test` runs binaries sequentially, so this is safe today; under cargo-nextest, a parallel workspace runner, or a concurrently running developer REPL it is not. `cross_mode_consistency_test.rs:371-378` deletes every `terraphim_test_*` and `terraphim_settings_*` entry in `temp_dir()`; the hermetic root prefix `terraphim-agent-hermetic-tests-` does not match, so the two helpers do not collide. `kg_ranking` and `cross_mode` both write the same `$CARGO_TARGET_TMPDIR/kg/test_ranking_kg.md` and both delete an unrelated cwd-relative `docs/src/kg/test_ranking_kg.md` that nothing writes any more (left over from the #112 fix).

4. Leaked child processes, not races. `extract_functionality_validation.rs:45` keeps the server `Child` in a `static Mutex<Option<Child>>` and never kills it; `kg_ranking_integration_test.rs:207-213` drops the `Child` handle on purpose so the server outlives the test binary. `server_mode_tests.rs` and `integration_tests.rs` kill per test. The check-then-set on `SERVER_PORT` (`extract_functionality_validation.rs:96-109`) would be a TOCTOU if two tests raced, but all four are `#[serial]`.

5. The lib test binary is clean. The src sweep found no in-process env writers, no cwd changes, and one `Once`-guarded logger install (`src/logging.rs:117`). The env reads in `wiki_sync.rs` inline tests and the `directories` lookups elsewhere are therefore race-free in that binary. `src/listener.rs:1298` reads `GITEA_TOKEN` but is production code placed after the test module (`#[allow(clippy::items_after_test_module)]`), so it belongs to section B. `src/lib.rs:164` names its tmp file with only `subsec_nanos()`, which can collide across two concurrent lib-test processes but not within one.

6. Port allocation is race-free within a binary: `portpicker::pick_unused_port` (integration_tests, server_mode_tests, kg_ranking, cross_mode) or `TcpListener::bind("127.0.0.1:0")` then drop (`extract_functionality_validation.rs:88`). Both have the usual bind-then-release window, but every caller is `#[serial]`. Fixed ports appear only as "expected unreachable" targets (9999, 19999) or the external dev server on 8000.

7. The one `multi_thread` tokio test in `tests/` (`persistence_tests.rs:354`) is deliberately not `#[serial]` and touches only an `Arc<tokio::Mutex<Config>>`; its doc comment states why. No env access on that path.

## Open questions

1. Phase 2 should decide how to demonstrate the `tui_service_tests` race empirically. ThreadSanitizer on macOS with `-Zsanitizer=thread` needs a nightly std build; an alternative is a loom-free stress run with `--test-threads=10` and `RUST_BACKTRACE` while the guard flips HOME. The `getenv` return in `directories` is copied immediately, so a crash requires the `setenv` realloc to land mid-copy; the window is narrow but real on glibc and on macOS libc.

2. Does section A (or whoever owns `terraphim_test_utils`) want the registry-crate `unsafe` wrapper counted in the workspace archetype? Its SAFETY comment gives a rule (`#[serial]`) that is insufficient for mixed binaries, which is the same SAFETY-comment-accuracy prior section A carries.

3. Whether the repo intends to move to cargo-nextest. If it does, every fixed-name `temp_dir()` deletion in item 3 and the two inherited-HOME spawn families become cross-binary races on the filesystem and on the SQLite WAL.

4. `selected_role_tests.rs:393` claims subprocesses use in-memory storage, yet the binary inherits the real HOME and the helper comment in `cli_test_env.rs` says the default settings hardcode `/tmp/terraphim_sqlite`. One of those statements is stale; phase 2 should check which.

5. Section B owns `src/shared_learning/store.rs` (10 `multi_thread` tests with a `block_on`-inside-runtime prior). Nothing in section C's inline sweep adds to that.
