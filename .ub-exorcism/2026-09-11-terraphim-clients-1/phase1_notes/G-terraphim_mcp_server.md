# Module: G — terraphim_mcp_server

Scope: `crates/terraphim_mcp_server/src` (lib.rs 2945 lines, main.rs 364, resource_mapper.rs 59) and `crates/terraphim_mcp_server/tests` (14 integration-test binaries plus `tests/support/mod.rs`). Toolchain observed: rustc 1.97.1, edition 2024 (workspace). All paths below are relative to `crates/terraphim_mcp_server/` unless prefixed.

## Purpose

MCP (Model Context Protocol) server exposing Terraphim search, autocomplete, knowledge-graph, file-search and RLM tools over stdio or SSE. `McpService` (src/lib.rs:95) hand-implements `rmcp::ServerHandler` (src/lib.rs:1694) with a manual `match` over tool names; no `rmcp` `#[tool]`/`#[tool_router]` proc-macros are used (sweep for `#\[tool` returned nothing). `main.rs` wires the config profile, optional project `.terraphim/` discovery, a `KgPathScorer`, and either the stdio transport or an axum SSE server.

## Unsafe-surface tally

| Item | Count | Evidence |
|---|---|---|
| `unsafe` blocks in src | 0 | `rg 'unsafe\|SAFETY' src` returns nothing |
| `unsafe` blocks in tests (source-direct) | 1 | tests/test_mcp_stdio.rs:15 (`env::set_var`), catalogued by section A; not duplicated here |
| Hidden env mutation via safe wrapper | 8 call sites | `terraphim_test_utils::set_env_var` at tests/mcp_autocomplete_e2e_test.rs:22, :28; tests/mcp_rolegraph_validation_test.rs:21, :27, :137, :284, :394, :618. The `unsafe { std::env::set_var }` lives in the registry crate `~/.cargo/registry/src/git.terraphim.cloud-*/terraphim_test_utils-1.20.3/src/lib.rs:39` (cfg `rust_has_unsafe_env_setters`, always on for edition 2024 per its build.rs). SAFETY comment at that file's line 37-38 delegates the obligation to the caller ("Tests using this should be marked with #[serial]"), through a safe fn signature |
| FFI / raw pointers / `transmute` / `static mut` | 0 | none in src or tests |
| `catch_unwind` / `panic::` | 0 | none |
| `impl Drop` | 0 | none |
| `unsafe` in `cargo +nightly expand --lib` | PENDING | see "Expansion" below |

Discrepancy for the lead: the inventory header states "6 `unsafe` blocks, all set_var/remove_var in test code". The eight `set_env_var` call sites in this section reach an `unsafe` block that is outside the workspace (registry crate) and therefore outside that count, but the safety obligation is discharged by workspace test code. Recorded here, header not edited.

## Per-test-binary thread model

libtest runs tests inside one binary on parallel threads (default `--test-threads` = CPU count); separate binaries run sequentially under `cargo test`. CI uses plain `cargo test` (`.github/workflows/ci.yml:26` uses `--lib` only, so none of these integration binaries run there; `.gitea/workflows/native-ci.yml:54` runs `--workspace --all-targets`, so all 14 run there). No `nextest.toml` exists in the repo, so the `#[serial]` process-scoped guarantee holds.

| Binary | `#[test]` | `#[tokio::test]` | `#[serial]` | Model | Notes |
|---|---|---|---|---|---|
| auto_route.rs | 0 | 1 | 0 | single test, trivially serial | in-process `McpService` with hand-built `ConfigState` (line 30-64) |
| integration_test.rs | 0 | 7 | 0 | 7 parallel tests, each spawns a child server process (`TokioChildProcess`, line 3-46) | reads `std::env::current_dir()` (lines 56, 68, 80); no env writes |
| mcp_autocomplete_e2e_test.rs | 0 | 8 | 8 | fully serialised | env writes via `set_env_var` (22, 28); fixed temp file (66); child server per test |
| mcp_rolegraph_validation_test.rs | 0 | 4 | 4 | fully serialised | env writes via `set_env_var` (21, 27, 137, 284, 394, 618); fixed temp file (76); child server per test |
| test_advanced_automata_functions.rs | 0 | 4 | 0 | 4 parallel, child process each | `current_dir()` read at 28 |
| test_all_mcp_tools.rs | 1 | 0 | 0 | single test, trivially serial | hermetic root + `current_dir(&root)` (37); `thread::sleep(3s)` (line 44) |
| test_bug_report_extraction.rs | 0 | 2 | 0 | 2 parallel, child process each | |
| test_find_files.rs | 0 | 5 | 0 | 5 parallel, in-process | each calls `ConfigState::new` (line 13) in-process |
| test_kg_term_verification.rs | 0 | 1 | 0 | single | child process |
| test_mcp_fixes_validation.rs | 0 | 3 | 0 | 3 parallel, child process each | |
| test_mcp_stdio.rs | 1 | 0 | 0 | single `#[test]`, trivially single-threaded; additionally gated on `RUN_MCP_STDIO_TEST=1` (line 10) | contains the section-A `unsafe set_var` (line 15) |
| test_selected_role_usage.rs | 0 | 2 | 0 | 2 parallel, child process each | `current_dir()` reads at 28, 267 |
| test_tools_list.rs | 1 | 0 | 0 | single test; spawns one std thread (line 47) draining child stderr into `Arc<Mutex<String>>` (line 44); joined on both exit paths (lines 88, 158) | `thread::sleep(3s)` at 60 |
| test_working_advanced_functions.rs | 0 | 2 | 0 | 2 parallel, child process each | |

All `#[tokio::test]` attributes are bare (default `current_thread` flavour); none use `flavor = "multi_thread"`.

Env-mutation invariant check: the only two binaries that mutate the process environment (via `set_env_var`) have every test marked `#[serial]` (8/8 and 4/4), so within each process no other test thread runs while `set_var` executes. The tokio `current_thread` runtime still owns a blocking pool; no code in these tests reads env from a spawned blocking task before the writes at the top of the setup function. `set_env_var` is called before any child process spawn in the same test, so the child inherits the mutated env deterministically.

## Shared test-support state

`tests/support/mod.rs` (76 lines, compiled separately into 9 binaries via `mod support;`):

- `static COUNTER: AtomicU64` (line 25), `fetch_add(SeqCst)` (line 28). Process-local, one copy per binary. Only used to salt the hermetic root name together with `std::process::id()` and a nanosecond timestamp (lines 34-39), so roots are unique per test even across parallel binaries.
- No `OnceLock`, `LazyLock`, `Mutex`, ports, or fixed temp paths originate from this module.
- `mcp_server_binary()` (line 53) reads env `TERRAPHIM_MCP_SERVER_BIN` then `env!("CARGO_BIN_EXE_terraphim_mcp_server")`. Read-only.
- Hermetic roots are created but never removed (no `Drop`, no cleanup); they accumulate under `std::env::temp_dir()`.

Shared state that lives outside `support/` but is shared across tests:

- Fixed-name files in `std::env::temp_dir()`: `terraphim_engineer_thesaurus.json` (tests/mcp_rolegraph_validation_test.rs:76, written at :80) and `terraphim_engineer_autocomplete_thesaurus.json` (tests/mcp_autocomplete_e2e_test.rs:66, written at :70). Written by every test in the respective binary; safe under `#[serial]` within a process and under sequential binary execution; the two names differ so the binaries do not collide with each other.
- `tempfile::Builder::new().prefix("terraphim-logs").tempdir()?.keep()` (e2e:23-26, rolegraph:22-25) leaks a unique dir per call; not shared.
- `DeviceStorage::init_memory_only()` (e2e:30, rolegraph:28) initialises the persistence crate's process-wide `static DEVICE_STORAGE: AsyncOnceCell<DeviceStorage>` (registry `terraphim_persistence-1.20.2/src/lib.rs:55`). Once-only per process; subsequent calls return the existing instance. `thesaurus.save()` / `.load()` (rolegraph:67, :71; e2e:58, :62) go through that global.
- The seven unserialised binaries that spawn the real server binary (integration_test, test_advanced_automata_functions, test_bug_report_extraction, test_kg_term_verification, test_mcp_fixes_validation, test_selected_role_usage, test_working_advanced_functions) never set `TERRAPHIM_PROFILE_MEMORY_TYPE`. Each child server therefore resolves persistence from the default settings, whose default profile is SQLite at `${TERRAPHIM_DATA_PATH:-~/.terraphim}/sqlite/terraphim.db` (registry `terraphim_settings-1.20.2/default/settings.toml`, `[profiles.sqlite]`). Up to 7 child processes (integration_test) can share that one on-disk database concurrently. Whether `ConfigState::new` actually opens the store for the default server profile is not resolved in this section (see Open questions).
- `test_find_files.rs:13` calls `ConfigState::new` in-process five times in parallel on the default `ConfigBuilder` config.

## Concurrency surface in src

- `McpService` (src/lib.rs:95) derives `Clone`; every field is `Arc`/`Option<Arc>`/`SharedFrecency`, so clones alias the same state. In SSE mode one clone is handed out per connection (src/main.rs:295 `sse_server.with_service(move || service.clone())`).
- `autocomplete_index: Arc<tokio::sync::RwLock<Option<AutocompleteIndex>>>` (src/lib.rs:99). Writers: src/lib.rs:372 (`build_autocomplete_index`) and :788 (`deserialize_autocomplete_index`). Readers: :418, :510, :590, :640, :691, :737.
- Read guard held across `.await`: the guard taken at src/lib.rs:510 (`find_matches`/snippet path) remains live across awaits at :534 and :553 (`service.search(&sq).await`), until scope end at :581. tokio `RwLock` is write-preferring, so a concurrent writer (:372/:788) queues and blocks subsequent readers until the awaited search completes. No re-entrant acquisition on that path; this is a stall/fairness surface, not a deadlock.
- `deserialize_autocomplete_index` (src/lib.rs:788-795) holds the write guard while formatting the success message and calls `.as_ref().unwrap()` on the just-assigned `Some`. No await under the write guard.
- `tokio::spawn` (src/main.rs:289) runs the axum SSE server; graceful shutdown via `CancellationToken` child token (:282-287). `shutdown_signal()` (:85) awaits `ctrl_c`.
- `tokio::process::Command` (src/lib.rs:1603) spawns `terraphim_rlm`; stdin write/shutdown and `wait_with_output` are all awaited (:1616-1631). Not blocking.
- Blocking call on a runtime worker: `find_terraphim_rlm_binary()` (src/lib.rs:48-65) runs `std::process::Command::new("which")...output()` synchronously (:57-60), invoked from the async RLM tool handler at :1595. Not UB; a blocking syscall inside an async task.
- `config_state.config.lock().await` (src/main.rs:210) is a `tokio::sync::Mutex` on `ConfigState.config`; guard dropped at end of the block (:219).
- No `std::sync::Mutex`, `Atomic*`, `OnceLock`, `LazyLock`, `static`, `thread::spawn`, or `spawn_blocking` in src.

## Env / process-global access in src

| Site | Access | Kind |
|---|---|---|
| src/lib.rs:50 | `std::env::var("HOME")` | read, per call, inside async tool handler (via :1595) |
| src/lib.rs:57 | `std::process::Command::new("which")` | child process, inherits env |
| src/lib.rs:109 | `std::env::var("FFF_FRECENCY_PATH")` | read, once at `McpService::new` |
| src/main.rs:134 | `Args::parse()` | reads `std::env::args` |
| src/main.rs:145 | `EnvFilter::from_default_env()` | reads `RUST_LOG` |
| src/main.rs:148 / :153-156 | `tracing_subscriber ... .init()` | sets the process-global tracing subscriber once |
| src/main.rs:165 | `terraphim_config::project::discover(None)` | reads `std::env::current_dir()` (registry `terraphim_config-1.20.2/src/project.rs:132`) |
| src/main.rs:281 | `TcpListener::bind` | binds `--bind` address, default `127.0.0.1:8000` (:71), SSE mode only |

No `set_var`, `remove_var`, or `set_current_dir` anywhere in src. Env mutations in scope are confined to tests (see tally).

## Runtime-nesting sites

None. `rg 'block_on|Runtime::new|Handle::current|Builder::new_'` over src and tests returns nothing. The only runtime entry points are `#[tokio::main]` (src/main.rs:132) and `#[tokio::test]` in the test binaries. The `#[cfg(test)]` module in main.rs (:312-364) has three sync `#[test]` functions that create no runtime.

## Library-trait invariant surface

- Hand-written: `impl From<TerraphimMcpError> for ErrorData` (src/lib.rs:88). Infallible conversion; no invariant beyond `Display`.
- Hand-written: `impl ServerHandler for McpService` (src/lib.rs:1694). Async trait methods `initialize`, `list_tools`, `call_tool`, `list_resources`, `read_resource`, `get_info`.
- Derived only: `thiserror::Error` + `Debug` on `TerraphimMcpError` (:72); `Clone` on `McpService` (:94); `Clone, Default` on `TerraphimResourceMapper` (src/resource_mapper.rs:9); `Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug` on `ConfigProfile` (src/main.rs:80), consistent by derivation; `Parser, Debug` on `Args` (:55).
- No `Hash`, `Borrow`, `Iterator`, `Deref`, `Send`, or `Sync` impls, hand-written or otherwise, in src or tests.

## Notable patterns

- Manual tool dispatch by string match rather than `rmcp` macros keeps the expansion free of generated `unsafe` (pending measurement).
- `find_terraphim_rlm_binary` (src/lib.rs:48) mixes `HOME` lookup, a blocking `which` subprocess and a bare-name fallback inside an async handler.
- Tests compare across two styles: rmcp `TokioChildProcess` clients (integration_test, e2e, rolegraph) and hand-rolled JSON-RPC over std pipes with 3 s sleeps (test_all_mcp_tools:44, test_tools_list:60).
- Both env-mutating binaries put the `set_env_var` calls inside a shared async setup function rather than in each test body; the `#[serial]` attribute is on the tests, so the invariant is enforced one level removed from the mutation.
- `tests/support/mod.rs` has a leading `#![allow(dead_code)]` between two `//!` doc-comment runs (lines 1-12); harmless, noted for completeness.

## Expansion

`mkdir -p /private/tmp/claude-501/ubx-expand && cargo +nightly expand -p terraphim_mcp_server --lib > /private/tmp/claude-501/ubx-expand/terraphim_mcp_server.rs` was launched at 23:03 local and was still "Blocking waiting for file lock on build directory / artifact directory" (shared with sibling mappers) at the time of writing. Result: PENDING. Expected 0 `unsafe` from the derives listed above; the measured count will be appended to this section when the expansion completes. `--lib` covers lib.rs and resource_mapper.rs only; main.rs (`#[tokio::main]`, clap derives) is a bin target and not covered by that command.

## Open questions

1. Does `ConfigState::new` (registry `terraphim_config-1.20.2/src/lib.rs:1121`) or the child server's startup open the default SQLite store at `~/.terraphim/sqlite/terraphim.db` when `TERRAPHIM_PROFILE_MEMORY_TYPE` is unset? If yes, the seven unserialised child-spawning binaries share one on-disk database across parallel processes (up to 7 concurrent in integration_test.rs). Out of this section's source scope; flagged for the persistence/config section owner.
2. The `#[serial]` guarantee is process-scoped. If the runner ever changes to nextest (no `nextest.toml` today), the fixed-name temp files at rolegraph:76 and e2e:66 become a cross-process write race within one binary.
3. Whether the tokio blocking pool (`current_thread` runtime still has one) can read env concurrently with `set_env_var` at e2e:22/rolegraph:21. No such reader was found in the test code; not ruled out inside `DeviceStorage::init_memory_only` (registry).
4. Expansion count pending (see above).
