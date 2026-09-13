# Phase 1: Unsafe-Surface Inventory — run 2026-09-11-terraphim-clients-1

Archetype: pure-safe-with-env-unsafe (6 `unsafe` blocks, all `std::env::set_var`/`remove_var` in test code; no FFI; no raw pointers).

Columns: file:line | site kind | bucket(s) | SAFETY status (PRESENT_STRONG / PRESENT_WEAK / MISSING) | expansion (SOURCE_DIRECT / MACRO_GENERATED) | notes


## Section B: terraphim_agent-core (`crates/terraphim_agent/src`)

crates/terraphim_agent/src (whole crate) | none | — | n/a | SOURCE_DIRECT | no unsafe sites (0 blocks, 0 `unsafe impl`, 0 `SAFETY:`; the three `rg unsafe` hits at commands/modes/local.rs:89,294, shared_learning/wiki_sync.rs:804,809, memory_command.rs:1032 are prose/strings). See phase1_notes/B-terraphim_agent-core.md for the concurrency surface: 1 nested `block_on` (server_command.rs:751, feature `server`), 1 `block_in_place`+`Handle::current` bridge (shared_learning/store.rs:608), 1 global std `RwLock` cache with I/O under guard (kg_validation.rs:28), 2 `OnceLock` statics, tokio `Mutex`/`RwLock` families in service.rs, commands/registry.rs, shared_learning/store.rs, repl/handler.rs.
lib expansion (`cargo +nightly expand -p terraphim_agent --lib`, 38,910 lines) | macro: `vec![]` (`::alloc::boxed::box_assume_init_into_vec_unsafe`) | std | n/a | MACRO_GENERATED | 89 sites; nightly std `vec!` lowering. Low priority.
lib expansion | macro: `#[derive(Clone, Copy)]` (`unsafe impl ::core::clone::TrivialClone`) | std | n/a | MACRO_GENERATED | 10 sites: onboarding::wizard::{SetupMode, QuickStartChoice}, exit_codes::ExitCode, output::OutputFormat, guard_patterns::GuardDecision, export_kg::CorrectionTypeFilter, guard::ExecutionTier, hook::{LearnHookType, AgentFormat}, install::AgentType. Low priority.
lib expansion | macro: `#[derive(PartialEq)]` (`unsafe { ::core::intrinsics::unreachable() }`) | std | n/a | MACRO_GENERATED | 1 site: shared_learning::wiki_sync::SyncResult. Low priority.
bin expansion (`cargo +nightly expand -p terraphim_agent --bin terraphim-agent`, 35,697 lines, private target dir) | macro: `vec![]` (8 sites) and `#[derive(Clone, Copy)]` `TrivialClone` (6 sites: CommandOutputMode, PollDecision, ModelFamily, CliKind, TuiAction, CommandOutputConfig) | std | n/a | MACRO_GENERATED | 15 unsafe lines total, no other kinds. Lead had directed this row be closed as SKIPPED (lib expansion already showed macro unsafe is limited to std vec!/derive marker impls); the private-target expand finished before the row was closed, and it confirms that finding. Low priority.

## Section E — terraphim_update (crates/terraphim_update/src, crates/terraphim_update/tests)

Mapper: p1-E. Expansion: `cargo +nightly expand -p terraphim_update --lib` -> /private/tmp/claude-501/ubx-expand/terraphim_update.rs (14032 lines, exit 0). `unsafe` token count in expansion: 2, both MACRO_GENERATED. Note: `cargo expand --lib` drops `#[cfg(test)]` modules, so the `set_var` block at src/state.rs:131 (catalogued by section A) does not appear in the expansion; this is expected and does not contradict section A.

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| crates/terraphim_update/src/state.rs:131 | `unsafe { std::env::set_var x5 }` (test helper) | env-mutation, data-race-with-getenv | PRESENT_WEAK (comment "runs in isolation before spawning other threads" is false: 120 non-`#[serial]` tests in the same lib test binary run concurrently, see phase1_notes/E-terraphim_update.md) | SOURCE_DIRECT | Owned by section A; cross-reference only. |
| crates/terraphim_update/src/manifest.rs:23 (`UpdateBackend`) | `unsafe impl ::core::clone::TrivialClone` | derive-generated | N/A (compiler-generated from `#[derive(Clone)]` on a `Copy` enum) | MACRO_GENERATED (expansion line 3287) | No hand-written unsafe. |
| crates/terraphim_update/src/notification.rs:10 (`UpdateAction`) | `unsafe impl ::core::clone::TrivialClone` | derive-generated | N/A (compiler-generated from `#[derive(Clone)]` on a `Copy` enum) | MACRO_GENERATED (expansion line 4614) | No hand-written unsafe. |
| crates/terraphim_update/src/lib.rs:1407-1410 | `tokio::task::block_in_place` + `Runtime::new` + `block_on` inside a closure | runtime-nesting | not unsafe; panics on current_thread if ever invoked from a runtime worker | SOURCE_DIRECT | Only call path today is `spawn_blocking` (scheduler.rs:250), where it is legal. Details in notes. |
| crates/terraphim_update/src/lib.rs:1536, crates/terraphim_update/src/rollback.rs:310 | `fs::copy(backup, target)` truncate-in-place | file-lifecycle | not unsafe; O_TRUNC overwrite of `target` | SOURCE_DIRECT | Only sites in the crate that overwrite an existing binary in place rather than `rename` over it. |
| crates/terraphim_update/src/lib.rs:1088-1094 | Windows `remove_file` then `rename` | file-lifecycle | not unsafe; window with no binary present | SOURCE_DIRECT | `#[cfg(not(unix))]` branch only. |
| crates/terraphim_update/src/platform.rs:155-164 | fixed-name probe file `.write_test_12345` write+remove | file-lifecycle, cross-process race | not unsafe | SOURCE_DIRECT | Probes `current_exe().parent()` and `/usr/local/bin`. |
| crates/terraphim_update/tests/*.rs | 4 separate test binaries; no unsafe; no `#[serial]` | n/a | n/a | n/a | Do not share a process with the state.rs `set_var` block. |

Section E totals: SOURCE_DIRECT unsafe = 0 (excluding the section-A-owned state.rs:131); MACRO_GENERATED unsafe = 2; `impl Drop` = 0; `catch_unwind` = 0; raw pointers/FFI/transmute = 0; production `Atomic*` = 0.

## Section C: terraphim_agent-tests (crates/terraphim_agent/tests + inline #[cfg(test)] in crates/terraphim_agent/src)

Sweep result: 0 `unsafe` blocks and 0 direct `std::env::set_var`/`remove_var`/`set_current_dir` calls in the section's own source. One indirect env-mutation site reaches `unsafe std::env::set_var` through the registry crate `terraphim_test_utils` 1.20.3 (not a workspace member, so absent from section A's list). Everything else below is process-global or cross-binary shared state without `unsafe`, catalogued for the thread-model prior.

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| crates/terraphim_agent/tests/tui_service_tests.rs:37 | `EnvVarGuard::set("HOME", ...)` -> terraphim_test_utils-1.20.3/src/lib.rs:39 `unsafe { std::env::set_var }` | data-race (set_var vs getenv), test-binary thread model, SAFETY-comment accuracy | PRESENT_WEAK (library comment delegates to caller: "Tests using this should be marked with #[serial]"; caller IS `#[serial]` but 9 of 10 tests in this binary are not, so the precondition "no other threads are concurrently reading" is unmet even though the stated rule was followed) | SOURCE_DIRECT (call); unsafe body in dependency | LIVE RACE. Non-serial siblings call `TuiService::new_with_embedded_defaults` -> `ConfigBuilder::new_with_id` -> `DeviceSettings::new()` (terraphim_config-1.20.4/src/lib.rs:556) -> `ProjectDirs::from` / `std::env::var("HOME")` (terraphim_settings-1.20.4/src/lib.rs:161-163). Binary not run in CI (ci.yml runs `--lib` only). Cross-section: A |
| crates/terraphim_agent/tests/tui_service_tests.rs:40 | `EnvVarGuard::set("XDG_CONFIG_HOME", ...)` -> same unsafe wrapper | as above | PRESENT_WEAK | SOURCE_DIRECT | Same race window as :37 |
| crates/terraphim_agent/tests/tui_service_tests.rs:43 | `EnvVarGuard::set("TERRAPHIM_DATA_PATH", ...)` -> same unsafe wrapper | as above | PRESENT_WEAK | SOURCE_DIRECT | Same race window as :37 |
| crates/terraphim_agent/tests/tui_service_tests.rs:34-61 (scope exit) | `impl Drop for EnvVarGuard` -> terraphim_test_utils lib.rs:106-113 -> `set_var`/`remove_var` (three guards) | data-race | PRESENT_WEAK | MACRO_GENERATED-equivalent (implicit drop) | Fourth, fifth and sixth mutation points at test end; same unmet precondition |
| crates/terraphim_agent/tests/extract_functionality_validation.rs:44-45,96-109 | `static SERVER_PORT: OnceLock<u16>`, `static SERVER_CHILD: Mutex<Option<Child>>`; check-then-set in `ensure_server_running` | shared global state, process leak | n/a (safe code) | SOURCE_DIRECT | TOCTOU between `.get()` and `.set()` is benign because all 4 tests are `#[serial]`. Child never killed: leak, not race |
| crates/terraphim_agent/tests/kg_ranking_integration_test.rs:29,204-215 | `static SHARED_SERVER_URL: tokio::sync::OnceCell<String>`; Child handle deliberately dropped | shared global state, process leak | n/a | SOURCE_DIRECT | All 3 tests `#[serial]`; server process outlives binary by design |
| crates/terraphim_agent/tests/integration_tests.rs:22-23 ; tests/robot_search_output_regression_tests.rs:25 | `static AGENT_BINARY/SERVER_BINARY: OnceLock<Result<PathBuf,String>>` | shared global state | n/a | SOURCE_DIRECT | Read-only after init; env reads inside `get_or_init` (`TERRAPHIM_AGENT_BIN`/`TERRAPHIM_SERVER_BIN`) with no in-process writer in these binaries |
| crates/terraphim_agent/tests/support/cli_test_env.rs:9,31-46 | `static COUNTER: AtomicU64` for unique hermetic roots `terraphim-agent-hermetic-tests-<pid>-<ns>-<n>` | shared global state | n/a | SOURCE_DIRECT | Correct: pid+timestamp+counter; roots never cleaned up (disk leak, no race) |
| crates/terraphim_agent/tests/integration_tests.rs:213-229 ; tests/kg_ranking_integration_test.rs:221-233 | `remove_dir_all` of FIXED names under `temp_dir()`: `terraphim_sqlite`, `dashmaptest`, `terraphim_rocksdb`, `opendal`, `terraphim_test_kg` | cross-binary shared state | n/a | SOURCE_DIRECT | Guarded by `#[serial]` only inside each binary; `serial_test` lock is process-local. Safe under `cargo test` (binaries run sequentially), NOT under nextest/parallel runners or concurrent developer processes using the default persistence dirs |
| crates/terraphim_agent/tests/cross_mode_consistency_test.rs:371-378 | sweep of `temp_dir()` deleting every `terraphim_test_*` and `terraphim_settings_*` entry | cross-binary shared state | n/a | SOURCE_DIRECT | Prefix does not collide with hermetic root prefix `terraphim-agent-hermetic-tests-`; does collide with its own per-port dirs (intended) and with any other process using those prefixes |
| crates/terraphim_agent/tests/kg_ranking_integration_test.rs:290-292 ; tests/cross_mode_consistency_test.rs:406-408 | both write `$CARGO_TARGET_TMPDIR/kg/test_ranking_kg.md` | cross-binary shared file | n/a | SOURCE_DIRECT | `CARGO_TARGET_TMPDIR` is per package, so both binaries share one path; each also deletes a different cwd-relative path `docs/src/kg/test_ranking_kg.md` (:235, :364) that is never written |
| crates/terraphim_agent/tests/integration_tests.rs:61 ; kg_ranking_integration_test.rs:34 ; cross_mode_consistency_test.rs:32 ; support/cli_test_env.rs:12 ; command_system_integration_tests.rs:375 | `std::env::current_dir()` reads (walk up to `[workspace]`) | env reads | n/a | SOURCE_DIRECT | No `set_current_dir` anywhere in crate; all `.current_dir(` hits are `Command::current_dir` (child only) |
| crates/terraphim_agent/tests/persistence_tests.rs:354-386 | `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]` + `tokio::spawn` x5 over `Arc<ConfigState>` | Send/Sync, test-binary thread model | n/a | MACRO_GENERATED | Pure in-memory `Arc<Mutex<Config>>`; only non-`#[serial]` test in that binary; no env access |
| crates/terraphim_agent/tests/error_handling_test.rs:257 | `tokio::spawn` x10 `ApiClient` clones against localhost:8000 | thread model | n/a | SOURCE_DIRECT | `#[serial]`; skipped when server absent (`is_server_running`, :13) |
| crates/terraphim_agent/tests/extract_functionality_validation.rs:17 | `std::thread::spawn` waiting on child + mpsc timeout; `kill -TERM` via `Command` | thread model | n/a | SOURCE_DIRECT | Thread outlives call on timeout (detached); no shared mutable state |
| crates/terraphim_agent/src/shared_learning/wiki_sync.rs:458,467 (inline tests) | `std::env::var("GITEA_REPO")`, `std::env::var("GITEA_TOKEN")` | env reads | n/a | SOURCE_DIRECT | Lib test binary has zero in-process env writers, so race-free |
| crates/terraphim_agent/src/lib.rs:141,164 (inline tests) | `temp_dir().join("ta-mcp-shim-identity.json")` (never written), `ta-mcp-shim-roundtrip-<subsec_nanos>.json` (written) | shared tmp file | n/a | SOURCE_DIRECT | Second name uses only `subsec_nanos()`: collision possible across two concurrent lib-test processes, not within one |
| crates/terraphim_agent/src/logging.rs:117-126 ; src/service.rs:53 | `log::set_boxed_logger` under `Once` (`INIT.call_once`), reads `LOG_LEVEL` | global logger | n/a | SOURCE_DIRECT | Idempotent; only reached via `TuiService::new`/`load_config`, not via `new_with_embedded_defaults` |

Section C totals: unsafe blocks in scope = 0 (indirect via dependency = 1 wrapper, 6 dynamic mutation events in one test); `#[serial]` attributes = 108 across 14 binaries; statics = 7; test binaries = 47 (`tests/*.rs`; `command_system_integration_tests.rs.backup` is not compiled; no `harness = false`, no `[[test]]` entries, no `.cargo/config.toml` test flags, no `RUST_TEST_THREADS`/`--test-threads` in CI).

## Section G — terraphim_mcp_server (src + tests)

Notes: phase1_notes/G-terraphim_mcp_server.md. Zero `unsafe` in src. The single source-direct `unsafe` in tests/test_mcp_stdio.rs:15 is catalogued by section A and not repeated. Rows below are the remaining surface.

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs:22 | hidden env write via `terraphim_test_utils::set_env_var` (`TERRAPHIM_PROFILE_MEMORY_TYPE`) | env-global | PRESENT_WEAK (SAFETY comment at registry terraphim_test_utils-1.20.3/src/lib.rs:37-38 delegates to caller; caller satisfies it via `#[serial]` on all 8 tests) | SOURCE_DIRECT (unsafe block in registry crate, safe fn at call site) | called from async setup fn; before child spawn |
| crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs:28 | hidden env write via `set_env_var` (`TERRAPHIM_LOG_DIR`) | env-global | PRESENT_WEAK (as above) | SOURCE_DIRECT | same setup fn |
| crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs:21 | hidden env write via `set_env_var` (`TERRAPHIM_PROFILE_MEMORY_TYPE`) | env-global | PRESENT_WEAK (caller satisfies via `#[serial]` on all 4 tests) | SOURCE_DIRECT | setup fn `create_terraphim_engineer_config` |
| crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs:27 | hidden env write via `set_env_var` (`TERRAPHIM_LOG_DIR`) | env-global | PRESENT_WEAK | SOURCE_DIRECT | same setup fn |
| crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs:137 | hidden env write via `set_env_var` (`TERRAPHIM_PROFILE_MEMORY_TYPE`) | env-global | PRESENT_WEAK | SOURCE_DIRECT | in `#[serial]` test body |
| crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs:284 | hidden env write via `set_env_var` | env-global | PRESENT_WEAK | SOURCE_DIRECT | in `#[serial]` test body |
| crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs:394 | hidden env write via `set_env_var` | env-global | PRESENT_WEAK | SOURCE_DIRECT | in `#[serial]` test body |
| crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs:618 | hidden env write via `set_env_var` | env-global | PRESENT_WEAK | SOURCE_DIRECT | in `#[serial]` test body |
| crates/terraphim_mcp_server/src/lib.rs:50 | `std::env::var("HOME")` read | env-global | n/a (safe read) | SOURCE_DIRECT | per call inside async RLM handler via lib.rs:1595; no in-process writer exists in src |
| crates/terraphim_mcp_server/src/lib.rs:57 | blocking `std::process::Command::new("which").output()` | runtime-blocking | n/a | SOURCE_DIRECT | synchronous subprocess on a tokio worker, called from async handler |
| crates/terraphim_mcp_server/src/lib.rs:109 | `std::env::var("FFF_FRECENCY_PATH")` read | env-global | n/a (safe read) | SOURCE_DIRECT | once at `McpService::new` |
| crates/terraphim_mcp_server/src/lib.rs:510 | tokio `RwLock` read guard held across `.await` at :534 and :553 (scope ends :581) | sync-across-await | n/a | SOURCE_DIRECT | writers at :372 and :788; write-preferring lock, so a queued writer stalls later readers; no re-entrancy, no deadlock |
| crates/terraphim_mcp_server/src/main.rs:295 | `McpService` cloned per SSE connection, all clones alias one `Arc<RwLock>` | shared-service-state | n/a | SOURCE_DIRECT | `#[derive(Clone)]` at lib.rs:94 |
| crates/terraphim_mcp_server/src/main.rs:281 | `TcpListener::bind` on `--bind` (default `127.0.0.1:8000`, main.rs:71) | fixed-port | n/a | SOURCE_DIRECT | SSE mode only; no test in scope exercises SSE, so no shared-port hazard among tests |
| crates/terraphim_mcp_server/tests/support/mod.rs:25 | `static COUNTER: AtomicU64`, `fetch_add(SeqCst)` at :28 | process-static | n/a | SOURCE_DIRECT | one copy per binary; salts hermetic root name with pid + nanos; no cross-test sharing |
| crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs:76 | fixed-name file `temp_dir()/terraphim_engineer_thesaurus.json` written at :80 | fs-shared-path | n/a | SOURCE_DIRECT | shared by all 4 tests in the binary; safe under `#[serial]` and plain `cargo test`; cross-process race if runner becomes nextest |
| crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs:66 | fixed-name file `temp_dir()/terraphim_engineer_autocomplete_thesaurus.json` written at :70 | fs-shared-path | n/a | SOURCE_DIRECT | shared by all 8 tests in the binary; same caveat |
| crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs:30, tests/mcp_rolegraph_validation_test.rs:28 | `DeviceStorage::init_memory_only()` initialises process-wide `static DEVICE_STORAGE: AsyncOnceCell` (registry terraphim_persistence-1.20.2/src/lib.rs:55) | process-static | n/a | SOURCE_DIRECT | once per process |
| crates/terraphim_mcp_server/tests/test_tools_list.rs:47 | `thread::spawn` draining child stderr into `Arc<Mutex<String>>` (:44) | thread-spawn | n/a | SOURCE_DIRECT | joined on both exit paths (:88, :158); single `#[test]` in binary |
| crates/terraphim_mcp_server/tests/integration_test.rs:134-559 (7 tests), test_advanced_automata_functions.rs (4), test_mcp_fixes_validation.rs (3), test_bug_report_extraction.rs (2), test_selected_role_usage.rs (2), test_working_advanced_functions.rs (2), test_kg_term_verification.rs (1) | unserialised tests each spawning the real server binary without `TERRAPHIM_PROFILE_MEMORY_TYPE` | fs-shared-path (potential) | n/a | SOURCE_DIRECT | default persistence profile is SQLite at `~/.terraphim/sqlite/terraphim.db` (registry terraphim_settings-1.20.2/default/settings.toml); whether the child opens it at startup is an open question for the config/persistence section |
| crates/terraphim_mcp_server/src (lib.rs, resource_mapper.rs) | `cargo +nightly expand --lib` unsafe count | expansion | n/a | SKIPPED by lead | lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; rmcp macros were not observed to emit unsafe in the agent/grep expansions that share them |

Per-binary thread model summary: test_mcp_stdio.rs and test_tools_list.rs and test_all_mcp_tools.rs each hold exactly one `#[test]` (trivially single-threaded); mcp_autocomplete_e2e_test.rs (8) and mcp_rolegraph_validation_test.rs (4) are fully `#[serial]`; the remaining 9 binaries run their `#[tokio::test]`s in parallel with no env writes. No `block_on`/`Runtime::new`/`Handle::current`, no `impl Drop`, no `catch_unwind`, no hand-written `Hash`/`Eq`/`Ord`/`Borrow`/`Iterator` impls in scope.

## Section H: small crates (terraphim_lsp, terraphim_cli, terraphim_hooks, terraphim_command_runtime, terraphim_negative_contribution)

Mapper p1-H. Sweep: `rg -n -t rust` over each crate root (src and tests). Source `unsafe` count across all five: 0. `SAFETY` comments: 0. No FFI, raw pointers, `impl Drop`, manual `Send`/`Sync`, or `set_var`/`remove_var`. Detailed notes: `phase1_notes/H-small-crates.md`. Expansion column: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives.

### terraphim_lsp

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| (none) | unsafe | — | — | SOURCE_DIRECT: 0 / MACRO_GENERATED: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives. | No `unsafe` in src; no tests dir. |
| src/kg_analysis.rs:243 | catch_unwind (test only) | 11 panic-safety | n/a | SOURCE_DIRECT | `#[cfg(test)]`; closure captures `&Thesaurus` + literal; no `AssertUnwindSafe`; no guard or `&mut` inside; result consumed via `is_ok()`, payload dropped. |
| src/server.rs:18-19 | `Arc<tokio::Mutex<HashMap<..>>>` x2 | concurrency | n/a | SOURCE_DIRECT | `documents` and `pending`; touched by did_open, did_change, did_close, schedule_diagnostics and the spawned task. tower-lsp dispatches with max concurrency 4. |
| src/server.rs:46-73 | guard held across `tokio::spawn` (no await) | concurrency | n/a | SOURCE_DIRECT | `pending` guard; spawned task never touches `pending`. Entries never pruned; did_close does not abort. |
| src/server.rs:51-73 | `tokio::spawn`, JoinHandle dropped | 11 panic-safety | n/a | SOURCE_DIRECT | Detached task; panic in `scan_file` (62) unobserved. |
| src/server.rs:30 | `.expect` reached at construction | 11 panic-safety | n/a | SOURCE_DIRECT | via `NegativeContributionScanner::new()` (scanner.rs:19-20); panics at startup, not in a handler. |

### terraphim_cli

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| (none) | unsafe | — | — | SOURCE_DIRECT: 0 / MACRO_GENERATED: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives. | No `unsafe` in src or tests. Bin-only crate. |
| src/service.rs:20 | `Arc<tokio::Mutex<TerraphimService>>` | concurrency | n/a | SOURCE_DIRECT | Single owner in the binary (main.rs:368); no spawns. Same `Arc<Mutex<Config>>` reachable directly (service.rs:250) and via TerraphimService (230). |
| src/service.rs:266, 296, 335 | guard held across `.await` | concurrency | n/a | SOURCE_DIRECT | service guard across TerraphimService calls; 266 then locks config inside terraphim_service (lib.rs:185): ordering service -> config. |
| src/service.rs:310, 351 | `RoleGraphSync` guard | concurrency | n/a | SOURCE_DIRECT | Synchronous iteration under guard, no await. |
| src/main.rs:437, 452, 472 | `std::process::exit(1)` | 11 panic-safety | n/a | SOURCE_DIRECT | No unwinding; `Drop` for service/persistence skipped. |
| src/main.rs:434 | `unreachable!()` | 11 panic-safety | n/a | SOURCE_DIRECT | Completions handled earlier (356-365). |
| src/main.rs:448 | `.unwrap()` in text-format fallback | 11 panic-safety | n/a | SOURCE_DIRECT | `serde_json::to_string(&Value)`. |
| src/main.rs:843, 889 | `env!("CARGO_PKG_VERSION")` | env (compile-time) | n/a | SOURCE_DIRECT | |
| src/main.rs:941 | `std::env::current_exe` | env (runtime) | n/a | SOURCE_DIRECT | `handle_rollback` then replaces the running binary on disk (944). |
| src/service.rs:34-36, 58 | delegated env/settings reads | env (runtime, indirect) | n/a | SOURCE_DIRECT | `detect_logging_config`, `DeviceSettings::load_from_env_and_file`. |
| tests/service_tests.rs:11 | `std::env::var("CARGO_MANIFEST_DIR")` | env (runtime, test) | n/a | SOURCE_DIRECT | Fallback `"."`. |
| tests/cli_command_tests.rs:13, tests/integration_tests.rs:15,20 | spawn real binary via `cargo_bin` | process-level shared state | n/a | SOURCE_DIRECT | `#[serial]` on service-touching tests; shared on-disk persistence; no `.env()` overrides. |

### terraphim_hooks

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| (none) | unsafe | — | — | SOURCE_DIRECT: 0 / MACRO_GENERATED: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives. | No `unsafe`; no concurrency primitives; no tests dir. |
| src/discovery.rs:47 | `dirs::home_dir()` | env (runtime) | n/a | SOURCE_DIRECT | `HOME` read. |
| src/discovery.rs:58 | `Command::new("which")` | env (runtime, PATH), child process | n/a | SOURCE_DIRECT | Synchronous `.output()`. |
| src/discovery.rs:41 | relative path probe `./target/release/...` | env (cwd) | n/a | SOURCE_DIRECT | Depends on process cwd. |
| src/validation.rs:155,171,188; src/validation_types.rs:149,166,181 | `panic!` in tests | 11 panic-safety (test) | n/a | SOURCE_DIRECT | Match-arm assertions. |

### terraphim_command_runtime

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| (none) | unsafe | — | — | SOURCE_DIRECT: 0 / MACRO_GENERATED: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives. | Single 70-line lib; no types defined. |
| src/lib.rs:18, 24, 30, 43 | `config_state.config.lock().await` | concurrency | n/a | SOURCE_DIRECT | Guard held over synchronous work only. |
| src/lib.rs:67-68 | guard held across `.await` of `config.save()` | concurrency | n/a | SOURCE_DIRECT | Persistence I/O under the config lock. Shared with terraphim_agent per module doc. |

### terraphim_negative_contribution

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| (none) | unsafe | — | — | SOURCE_DIRECT: 0 / MACRO_GENERATED: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives. | No `unsafe`; no concurrency; no env. |
| src/scanner.rs:19-20 | `.expect` on embedded JSON in `new()`/`Default` | 11 panic-safety | n/a | SOURCE_DIRECT | Deterministic per build (`include_str!` at line 8). |
| src/scanner.rs:62 | `&content[start..end]` at Aho-Corasick positions | 11 panic-safety (str boundary) | n/a | SOURCE_DIRECT | Positions from `ac.find_iter(text)` on unmodified input with `ascii_case_insensitive` (automata matcher.rs:56-70); char-boundary aligned. |
| src/scanner.rs:46 | `mat.pos.unwrap_or((0,0))` | 11 panic-safety | n/a | SOURCE_DIRECT | Always `Some` (return_positions = true at 35). |
| src/scanner.rs:157-171 | bounds-checked slicing | 11 panic-safety | n/a | SOURCE_DIRECT | Guards at 159, 163. |

## Section A: env-unsafe-sites (mapper run 2026-09-11 23:11 BST)

All six sites are `std::env::set_var` / `std::env::remove_var` calls inside `#[cfg(test)]` code. Convention for openrouter_client.rs: the single `// SAFETY:` line at :166 sits at the head of the test fn and is the only comment for all four blocks; every block is graded PRESENT_WEAK against that shared comment, with the adjacency noted.

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| crates/terraphim_update/src/state.rs:131-137 | `unsafe { set_var x5 }` (HOME, USERPROFILE, XDG_CONFIG_HOME, APPDATA, LOCALAPPDATA) in test helper `setup_temp_config_dir` | env-mutation / data-race (test) | PRESENT_WEAK | SOURCE_DIRECT | Comment at :130 says "runs in isolation before spawning other threads". False: libtest has already spawned worker threads for the other 120 non-`#[serial]` tests in the same lib binary (130 tests total, 10 `#[serial]`, all in state.rs:144-312). serial_test 3.5.0 only serialises `#[serial]` tests among themselves (serial_test-3.5.0/src/lib.rs:31: tests with neither attribute "may run at any time"). Concurrent std-locked readers: platform.rs:112 `env::var("HOME")` via tests platform.rs:287 and :402; lib.rs:159/167/183 via lib.rs:1569/1586/1592/1598/1609; tempfile TMPDIR reads. No out-of-lock (libc getenv) reader identified in the terraphim_update test binary (see notes). HOME etc. are never restored; TempDir drops at test end leaving HOME dangling for the rest of the process. On macOS `dirs::config_dir()` (state.rs:16) ignores XDG_CONFIG_HOME (dirs-5.0.1/src/mac.rs:7,10), so the file lands under `<tmp>/Library/Application Support/terraphim`, not the `~/.config/terraphim` documented at state.rs:14. |
| crates/terraphim_grep/src/openrouter_client.rs:170-172 | `unsafe { remove_var("OPENROUTER_BASE_URL") }` in `test_openrouter_client_base_url_resolution` | env-mutation / data-race (test) | PRESENT_WEAK | SOURCE_DIRECT | Comment at :166 "test code, single-threaded, no concurrent env access". False on both counts: the lib test binary holds 57 tests with no `#[serial]` / harness limits; libc `getenv` (a reader outside std's lock per std/src/env.rs:304-326) is reachable from three sibling tests through fff-search -> libgit2 (see A notes). Adjacent to the SAFETY line (one statement between). |
| crates/terraphim_grep/src/openrouter_client.rs:177-179 | `unsafe { set_var("OPENROUTER_BASE_URL", ...) }` | env-mutation / data-race (test) | PRESENT_WEAK (shared comment at :166, not adjacent) | SOURCE_DIRECT | Same fn as above. Std-locked concurrent readers: openrouter_client.rs:51 via sibling tests :159-161, :197, :206 (they do not assert `base_url`, so the logical race is benign today). |
| crates/terraphim_grep/src/openrouter_client.rs:185-187 | `unsafe { set_var("OPENROUTER_BASE_URL", v) }` (restore branch) | env-mutation / data-race (test) | PRESENT_WEAK (shared comment at :166, not adjacent) | SOURCE_DIRECT | Restore is skipped if an assert between :173 and :181 panics (no guard/Drop). |
| crates/terraphim_grep/src/openrouter_client.rs:189-191 | `unsafe { remove_var("OPENROUTER_BASE_URL") }` (restore branch) | env-mutation / data-race (test) | PRESENT_WEAK (shared comment at :166, not adjacent) | SOURCE_DIRECT | As above. |
| crates/terraphim_mcp_server/tests/test_mcp_stdio.rs:15-20 | `unsafe { env::set_var("TERRAPHIM_SETTINGS_PATH", ...) }` in `test_mcp_autocomplete_via_stdio` | env-mutation / data-race (test) | MISSING | SOURCE_DIRECT | Comment at :14 ("Set the environment variable for local dev settings") is descriptive, not a safety argument. De facto sound: the integration binary has exactly one test, so one libtest worker thread plus the parked main thread; `Command::spawn` at :24-32 is sequenced after on the same thread. Gated by `RUN_MCP_STDIO_TEST=1` at :10; neither CI workflow exports it (`rg` over .github/, .gitea/, justfile, scripts/), so the block does not execute in CI. `serial_test` is a dev-dependency of the crate (Cargo.toml) but unused in this file. |

Non-site entry (no `unsafe` block; recorded for completeness):

| file:line | kind | notes |
|---|---|---|
| crates/terraphim_agent/build.rs:4,19 | build script emitting `cfg(rust_has_unsafe_env_setters)` when rustc >= 1.92.0 | Zero consumers in this workspace (`rg rust_has_unsafe_env_setters` hits only build.rs). Introduced in 58d3c2d (2026-01-31) alongside `#[cfg(rust_has_unsafe_env_setters)]` wrappers in crates/terraphim_agent/tests/tui_service_tests.rs, which the same commit replaced with `terraphim_test_utils::EnvVarGuard` (external registry crate terraphim_test_utils-1.20.3, whose own src/lib.rs:35/45 consumes the cfg from its own build script, not from this one). Toolchain is rustc 1.97.1 and workspace edition is 2024 (Cargo.toml:19), so `set_var` is unconditionally `unsafe fn` here and the cfg is dead. |

Macro-expansion check (section A): teammate expansions at /private/tmp/claude-501/ubx-expand/terraphim_update.rs and terraphim_grep.rs (lib targets, no `--tests`) contain only compiler-derived `unsafe impl ::core::clone::TrivialClone` for `UpdateBackend`, `UpdateAction`, `Haystack`, `SufficiencyMetrics` (derive(Clone) output; no env access). `--tests` expansions for the three section-A binaries were queued to /private/tmp/claude-501/ubx-expand/terraphim_{update,grep}_lib_tests.rs and terraphim_mcp_server_test_mcp_stdio.rs but were still blocked on the shared cargo build-directory lock at write time; no MACRO_GENERATED env-unsafe rows are expected because `#[serial]` (serial_test_derive 3.5.0) and `#[tokio::test]` wrap the fn body in safe code.

## Section D: terraphim_sessions (crates/terraphim_sessions/src + benches; mapper p1-D, 2026-09-11 23:15 BST)

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| crates/terraphim_sessions/src (all 18 files) and benches/search_nfr.rs | no `unsafe` keyword, no `SAFETY` comment, no FFI, no raw pointers, no `impl Drop`, no `catch_unwind`, no `set_var` | none | N/A | SOURCE_DIRECT: 0 | See phase1_notes/D-terraphim_sessions.md for the concurrency, panic-safety and env surfaces. |
| crates/terraphim_sessions/src/connector/native.rs:333,362; src/connector/aider.rs:153,322; src/model.rs:193; src/cla/connector.rs (`from_normalized_message`) | `::alloc::boxed::box_assume_init_into_vec_unsafe` | macro-generated | N/A (std `vec![..]` expansion on this nightly) | MACRO_GENERATED (expansion lines 1097, 1135, 3662, 3906, 6495, 9679) | Six non-empty `vec!` literals; zero-element `vec![]` expands to `Vec::new()` with no unsafe. |
| crates/terraphim_sessions/src/model.rs:755 (`FileOperation`) | `unsafe impl ::core::clone::TrivialClone` | derive-generated | N/A (compiler-generated from `#[derive(Clone)]` on a `Copy` enum) | MACRO_GENERATED (expansion line 7857) | No hand-written unsafe. |
| crates/terraphim_sessions/src/service.rs:18,22; src/enrichment/enricher.rs:58 | `Arc<tokio::sync::RwLock<..>>` x3 | concurrency (non-poisoning) | not unsafe; no `.await` while any guard is held | SOURCE_DIRECT | `maybe_auto_import` (service.rs:72-98) has a check-then-act window between the flag read and write; `Clone` (service.rs:397) shares no state. |
| crates/terraphim_sessions/src/redaction.rs:55 | `std::sync::OnceLock` static | process-global init | not unsafe; init closure has no panic path | SOURCE_DIRECT | Compiled regex set, read-only after init. |
| crates/terraphim_sessions/src/connector/native.rs:159-256 | detached `spawn_blocking` + `oneshot` + `std::sync::mpsc` + `tokio::sync::mpsc` | thread lifecycle | not unsafe; JoinHandle dropped, loop exits on `tx.is_closed()` | SOURCE_DIRECT | `notify` callback runs on the backend thread and forwards into the std channel. |
| crates/terraphim_sessions/src/connector/cline.rs:347 | `.expect("Invalid timestamp in taskHistory")` in non-test code | panic-on-input | not unsafe | SOURCE_DIRECT (feature `cline-connector`, outside default expansion) | Panics on an out-of-range millisecond value read from user data. |
| crates/terraphim_sessions/src/search.rs:135,197 | `partial_cmp(..).unwrap_or(Equal)` on `f64` scores inside `sort_by` | comparator total-order | not unsafe; may panic on Rust 1.81+ if NaN appears | SOURCE_DIRECT | NaN source, if any, lives in `terraphim_types`. |

## Section F: terraphim_grep (crates/terraphim_grep/src, tests, benches) -- mapper p1-F, 2026-09-11 23:20 BST

Notes: phase1_notes/F-terraphim_grep.md. Expansion: /private/tmp/claude-501/ubx-expand/terraphim_grep.rs (`--lib`, no cfg(test)); `--lib --tests` expansion pending (job b8m1rnj8o, blocked on shared build lock).

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| crates/terraphim_grep/src/openrouter_client.rs:170,177,185,189 | `unsafe { std::env::set_var / remove_var }` x4 | env-mutation | see section A | SOURCE_DIRECT | Owned by section A; not re-catalogued. Only `unsafe` keyword occurrences in section F source (rg `unsafe|SAFETY` over src, tests, benches). |
| crates/terraphim_grep/src/hybrid_searcher.rs:28 (`Haystack`) | derive-generated `unsafe impl ::core::clone::TrivialClone` | derive-marker | n/a (compiler-generated, empty body) | MACRO_GENERATED (`#[derive(Clone, Copy)]`) | Expansion line 231. |
| crates/terraphim_grep/src/sufficiency_judge.rs:34 (`SufficiencyMetrics`) | derive-generated `unsafe impl ::core::clone::TrivialClone` | derive-marker | n/a | MACRO_GENERATED (`#[derive(Clone, Copy)]`) | Expansion line 4212. Total `unsafe` in `--lib` expansion: 2, both of this kind. |
| crates/terraphim_grep/src/hybrid_searcher.rs:341-402 (`search_code`) -> fff-search file_picker.rs:796,1694 -> git2 repo.rs:248 -> git2 lib.rs:748 -> libgit2 libgit2.c:46 -> sysdir.c:347,357,402,405 -> util.c:788 | no `unsafe` keyword; reachability surface: native libc `getenv` (`APP_SANDBOX_CONTAINER_ID`, `HOME`, `XDG_CONFIG_HOME`) inside `git_libgit2_init` | env-read-native, ffi-transitive, threads (rayon pool fff:73,1736; `ignore` parallel walk fff:1754) | MISSING (no annotation; runs in same process as section-A `set_var`) | SOURCE_DIRECT (dependency) | Reached by 8 lib tests: src/lib.rs:607,640,673,741,792,840; src/hybrid_searcher.rs:484,526. `git2::init` is a `Once`, so only the first of these to run performs the native reads; ordering-dependent. Full test table in notes. |
| crates/terraphim_grep/src/openrouter_client.rs:51 (`OpenRouterClient::new`) and reqwest 0.12.28 async_impl/client.rs:418-421 -> hyper-util 0.1.20 matcher.rs:230,325,411 | no `unsafe` keyword; reachability surface: Rust-side `std::env::var` (`OPENROUTER_BASE_URL`, proxy vars) concurrent with section-A mutation | env-read-std (lock-serialised; logical flake, not memory race) | n/a | SOURCE_DIRECT | Reached by 3 lib tests: src/openrouter_client.rs:157,195,204. Also builds rustls `ClientConfig` (aws-lc-sys + ring linked; no env read found) and calls `SCDynamicStoreCopyProxies` via system-configuration-sys (native, not `environ`). |
| crates/terraphim_grep/src/hybrid_searcher.rs:207-212, 430-434 | `sort_by` with `partial_cmp(..).unwrap_or(Equal)` on `pub f64` scores | library-invariant (total order; NaN reachable via public/deserialised fields) | n/a | SOURCE_DIRECT | Panic since Rust 1.81 on inconsistent order, not UB. |
| crates/terraphim_grep/src/hybrid_searcher.rs:150-154 | `text[..start]` / `text[end..]` on byte offsets from `terraphim_automata::find_matches` | library-invariant (char-boundary contract of dependency) | n/a | SOURCE_DIRECT | Panic, not UB. |
| crates/terraphim_grep/src/hybrid_searcher.rs:414 | `static WARNED: std::sync::Once` | concurrency | n/a | SOURCE_DIRECT | Only under `cfg(not(feature = "code-search"))`; compiled out of the default lib test binary. |
| crates/terraphim_grep/src/hybrid_searcher.rs:219,239,318 | `Arc<tokio::sync::RwLock<RoleGraph>>`, read-locked only | concurrency | n/a | SOURCE_DIRECT | No writer in crate. |
| crates/terraphim_grep/src/lib.rs:4 | doc claims `tokio::spawn` concurrency; none exists (`search_kg` then `search_code` awaited sequentially, hybrid_searcher.rs:262-276) | doc-drift | n/a | SOURCE_DIRECT | Informational. |
| crates/terraphim_grep/Cargo.toml:41 | `parking_lot = "0.12"` declared, never used | dead-dependency | n/a | n/a | Informational. |
| crates/terraphim_grep/src/main.rs:693-806; tests/*.rs; benches/hybrid_search.rs | bin unit tests, 6 integration test binaries (CLI spawned as child; `Command::env` mutates child only), criterion bench with multi-thread `Runtime::new()` | separate-process | n/a | n/a | Not concurrent with section-A test. tests/e2e_smoke_openrouter.rs is `#![cfg(feature = "openrouter")]` + `#[ignore]`. |

`cargo tree -p terraphim_grep -e normal --offline` `-sys` crates: aws-lc-sys 0.41.0, core-foundation-sys 0.8.7, dirs-sys 0.4.1/0.5.0, fsevent-sys 4.1.0, libbz2-rs-sys 0.2.5, libgit2-sys 0.18.5+1.9.4, libsqlite3-sys 0.30.1, libz-sys 1.1.29, lmdb-master-sys 0.2.6, security-framework-sys 2.17.0, system-configuration-sys 0.6.0, zstd-sys 2.0.16. No openssl / openssl-sys / native-tls in the graph. Reached from lib tests: libgit2-sys (+libz-sys), aws-lc-sys/ring (linked into rustls), system-configuration-sys (+core-foundation-sys); the rest are linked but unreached (details in notes).

### Section F addendum (p1-F, 2026-09-11)

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| crates/terraphim_grep (lib test binary) | `--lib --tests` expansion | n/a | n/a | SKIPPED | --tests expansion SKIPPED by lead: #[tokio::test] wraps bodies in safe code; no MACRO_GENERATED env rows expected; lib expansion shows only derive marker impls. |

### I. terraphim-session-analyzer (`crates/terraphim-session-analyzer`)

| file:line | site kind | bucket(s) | SAFETY status | expansion | notes |
|---|---|---|---|---|---|
| (none) | no `unsafe` blocks, fns, impls or traits; no FFI; no raw pointers; no `env::set_var` | n/a | n/a | SOURCE_DIRECT sweep: 0 hits for `unsafe\|SAFETY` in `src/`, `tests/`, `examples/` | `libc` declared under `cfg(unix)` (Cargo.toml:83-84) with zero call sites. `cargo +nightly expand --lib` SKIPPED by lead (build-lock contention; lib expansions elsewhere show only std `vec!` and derive marker impls). Library-trait invariant, slicing (5 PANIC-from-input byte-cut sites: reporter.rs:419,427; analyzer.rs:686; connectors/cursor.rs:323,378) and Miri notes in `phase1_notes/I-terraphim-session-analyzer.md`. |
