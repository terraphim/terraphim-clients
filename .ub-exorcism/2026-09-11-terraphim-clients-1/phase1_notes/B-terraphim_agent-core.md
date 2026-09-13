# Module

Section B: `terraphim_agent` core — `crates/terraphim_agent/src` (44,878 lines across lib + bin; lib exposes `client`, `logging`, `memory_retrieve`, `onboarding`, `service`, `shared_learning`, `tui_backend`, `robot`, `forgiving`, `mcp_tool_index`, `guard_patterns`, `learnings`, `repl`, `commands`; the `terraphim-agent` bin additionally compiles `main.rs`, `cli_helpers`, `cli_schema`, `learn_command`, `listener`, `memory_command`, `robot_dispatch`, `server_command`, `shell_dispatch`, `kg_validation`, `judge`).

Default features: `repl-interactive`, `llm`, `repl-sessions`, `shared-learning`. `server` is NOT default (`crates/terraphim_agent/Cargo.toml` `[features]`).

## Purpose

CLI/TUI/REPL agent binary plus supporting library. No FFI, no raw pointers, no `unsafe` in source. The soundness-relevant surface is entirely: tokio runtime construction and nesting, sync-over-async bridges, async and std locks, one global std `RwLock` cache, two `OnceLock` statics, child-process lifetimes, and env reads that pair with `set_var` in other sections' tests.

## Unsafe-surface tally

| Kind | Count | Evidence |
|---|---|---|
| `unsafe` blocks / fns / impls in source | 0 | `rg -n 'unsafe' crates/terraphim_agent/src` matches only prose: `commands/modes/local.rs:89,294` (comment/assert message "unsafe" = not whitelisted), `shared_learning/wiki_sync.rs:804,809` (test name), `memory_command.rs:1032` (string match on content). |
| `SAFETY:` comments | 0 | none |
| `unsafe impl Send/Sync` | 0 | none |
| MACRO_GENERATED unsafe, lib expansion (`cargo +nightly expand -p terraphim_agent --lib`, 38,910 lines, `/private/tmp/claude-501/ubx-expand/terraphim_agent.rs`) | 100 lines | 89 × `::alloc::boxed::box_assume_init_into_vec_unsafe` from `vec![]` (nightly std macro); 10 × `unsafe impl ::core::clone::TrivialClone for …` from `#[derive(Clone, Copy)]` on `SetupMode`, `QuickStartChoice` (onboarding wizard), `ExitCode` (`exit_codes`), `OutputFormat` (`output`), `GuardDecision` (`guard_patterns`), `CorrectionTypeFilter` (`export_kg`), `ExecutionTier` (`guard`), `LearnHookType`, `AgentFormat` (`hook`), `AgentType` (`install`); 1 × `unsafe { ::core::intrinsics::unreachable() }` from `#[derive(PartialEq)]` on `SyncResult` (shared_learning/wiki_sync). All std-derive/std-macro; low priority. |
| MACRO_GENERATED unsafe, bin expansion (`--bin terraphim-agent`, 35,697 lines) | 15 lines | 8 × `vec![]`, 6 × `TrivialClone` derives; see "Bin expansion" below. |

## Concurrency surface

| file:line | primitive | what it guards | accessed from which tasks/threads |
|---|---|---|---|
| `shared_learning/store.rs:136` | `tokio::sync::RwLock<HashMap<String, SharedLearning>>` (`SharedLearningStore.index`) | in-memory learning index | async methods on the owning task (`insert`, `get`, `merge_learning` 267, `record_graph_touch` 293, `record_application` 429, `find_similar` 458, `load_all` 176, feature-gated `GraphTouchStore::record_graph_touch` 592). Every write-guard is explicitly `drop(index)` before `self.persist(...).await` (lines 283, 298, 447), so no write guard is held across the markdown-backend await. Also reached synchronously via `block_on` from the `LearningStore` trait impl (646 read, 731 write) on whatever thread the middleware feedback loop calls from. |
| `shared_learning/store.rs:140,575,579,667` | `std::sync::RwLock<terraphim_rolegraph::RoleGraph>` (`role_graph`, feature `shared-learning`) | role graph used to rank `query_relevant` candidates | set once via `&mut self` (`set_role_graph`), read under `graph_lock.read()` at 667 inside sync `query_relevant`. Read uses `if let Ok(graph)`: poison is fail-open (graph ranking silently skipped). `query_graph` runs while the read guard is held; the tokio `index` read guard was already dropped at 659, so no std-lock-inside-async-guard overlap. |
| `commands/registry.rs:25,27,29` | `Arc<tokio::sync::RwLock<HashMap>>` × 3 (`commands`, `aliases`, `categories`) | command registry maps | REPL/executor tasks. Writers (`register_command` 103-129, `reload` 226-234, `build_autocomplete_index` 623-627) each take one lock at a time in their own `{}` scope. Readers: `resolve_command` 149-152 holds `aliases.read` then takes `commands.read` (order aliases→commands); `get_stats` 244-246 takes `commands.read`, `aliases.read`, `categories.read` simultaneously (order commands→aliases→categories). Reader-only orderings differ. Tokio `RwLock` is FIFO-fair (a queued writer blocks later readers), so a cycle needs four parties: `get_stats` holding `commands.read` waiting on `aliases`, `resolve_command` holding `aliases.read` waiting on `commands`, one writer queued on `commands` and a second writer queued on `aliases`. Writers (`register_command`, `reload`) take the three locks one at a time, so two concurrent writers are required. Catalogued for phase 2 to decide whether the registry is ever shared that way. |
| `commands/registry.rs:35,37` | `Arc<tokio::sync::RwLock<Option<AutocompleteIndex>>>`, `Arc<tokio::sync::RwLock<Option<Thesaurus>>>` | lazily built autocomplete automaton | `build_autocomplete_index` builds the index outside the lock (618) then writes under two sequential short guards (623, 627). `discover_commands` 642-649: read guard, `drop`, await the build, re-acquire read. Two concurrent first callers can both build (benign duplicate work, last writer wins). |
| `service.rs:16` | `Arc<tokio::sync::Mutex<TerraphimService>>` (`TuiService.service`) | the `terraphim_service::TerraphimService` state machine | held across `.await` on network/search in `update_selected_role` 350, `search` 449, `search_with_query` 457, `get_thesaurus` 463 (guard lives to end of fn; the awaited calls are the service's own async methods). No site takes `config_state.config` or `config_state.roles` while `service` is held. |
| `service.rs:479,518,720` | `RoleGraphSync` (`tokio::sync::Mutex<RoleGraph>` from `terraphim_config::ConfigState.roles`) | per-role rolegraph | read-only iteration/query under guard; no nested lock. |
| `service.rs:555,682,696,923,938` | `terraphim_config::ConfigState.config` (`tokio::sync::Mutex<Config>`) | live config | `save_config` 682 holds guard across `config.save().await` (persistence I/O). `add_role` 923 / `set_role` 938 take the guard in a scope, drop, then call `save_config` (re-acquires). `reload_from_json` 696 same pattern. |
| `repl/handler.rs:1909-1914` | `static OnceLock<Arc<tokio::sync::Mutex<SessionService>>>` (`SESSION_SERVICE`) | process-wide sessions service | REPL handler task. Guard `svc` at 1914 is held for the whole `match subcommand` and across awaits at 1961, 1963, 2023, 2070. At 2019 (`Search` arm) it also awaits `tui_service.get_thesaurus(...)` which takes `TuiService.service`: lock order SESSION_SERVICE → TuiService.service. No site takes them in the reverse order. |
| `learnings/capture.rs:733` | `static OnceLock<Option<Thesaurus>>` (`KG_THESAURUS`) | cached KG thesaurus for entity annotation | `get_or_init` on the capture path; `None` is cached permanently if the KG dir was absent at first call. |
| `kg_validation.rs:28` | `static std::sync::RwLock<Option<CachedThesaurus>>` (`KG_CACHE`) | KG thesaurus for `PreToolUse` command validation, hash-invalidated | called from `main.rs:2232` inside `async fn handle_hook_command` (2157), i.e. on a tokio worker thread. Fast path 87: read guard held across `compute_kg_source_hash` (filesystem walk). Slow path 97-116: write guard held across `compute_kg_source_hash` + `find_kg_dir` + `build_kg_thesaurus_with_hash` (filesystem I/O + automaton build), blocking the worker thread. Both paths use `.ok()?` so a poisoned lock yields `None` → empty result forever (fail-open, silent). |
| `logging.rs:131-154,272` | `std::sync::Mutex<Vec<String>>` (`CapturingLogger.records`), `#[cfg(test)]` only | test log capture | `.lock().unwrap()` inside `Log::log` 154 and in the test body 272: a panic under the lock in one test poisons it for any later `log!` in the same test, which then panics inside the logger. Test-only. |
| `commands/modes/local.rs:152-158`, `shared_learning/wiki_sync.rs:156-157` | `tokio::process::Command` child with `kill_on_drop(true)` | sandboxed local command / `gitea-robot` subprocess | awaited via `tokio::time::timeout(_, child.wait_with_output())`; child is killed if the future is dropped. |
| `shell_dispatch.rs:212-223, 284-295` | `tokio::process::Command` child WITHOUT `kill_on_drop` | agent-binary / agent-CLI dispatch from the listener | awaited via `tokio::time::timeout(_, child.wait_with_output())`; on timeout the future is dropped and the child continues running detached (tokio default: no kill on drop). |
| `listener.rs:1267-1275` | plain owned state (`HashSet`, `BTreeSet`, `String`), no locks | Gitea event polling loop | single task (`run_forever(mut self)` 1351). |
| thread spawns | none | `rg 'std::thread|tokio::spawn|spawn_blocking|JoinHandle|mpsc|Notify|Semaphore|oneshot|broadcast'` returns nothing in src. The four `.spawn()` hits are the process spawns above. | |

## Runtime-nesting sites

| file:line | construct | can it run inside an existing runtime? | verdict |
|---|---|---|---|
| `main.rs:449-456` | `Runtime::new()` (`rt`) in sync `main`, `block_on` update check | not inside a runtime | safe. Note `rt` is never dropped before the arms below create a second runtime; two multi-thread runtimes coexist for the process lifetime in the `Repl`, generic-command and TUI arms. Also used at 545 for `Listen`. |
| `main.rs:494-509` | second `Runtime::new()` + `block_on(repl::run_repl_*)` | sync context | safe (two runtimes alive). |
| `main.rs:552-570` | second `Runtime::new()` + `block_on(run_server_command / run_offline_command)` | sync context | safe (two runtimes alive). |
| `main.rs:2992-3230` | `Runtime::new()` in `run_tui` + ~12 `rt.block_on` calls inside the crossterm draw/poll loop (3004, 3009, 3011, 3117, 3157, 3165, 3174, 3189, 3220) | sync TUI thread | safe; each `block_on` blocks the UI thread for the duration of the backend call (responsiveness, not soundness). |
| `main.rs:107-113` | `runtime.block_on(api.health())` (`ensure_tui_server_reachable`, feature `server`) | called from sync `run_tui` with the TUI runtime | safe. |
| `server_command.rs:751-752` | `Runtime::new()?` then `rt.block_on(async { … })` inside `pub(crate) async fn run_server_command` (23), which is itself driven by `rt.block_on` at `main.rs:558` | NO. `Runtime::block_on` called from within an async context panics ("Cannot start a runtime from within a runtime"); additionally, dropping the inner `Runtime` inside an async context panics. | reachable only with `--features server` (non-default) AND `repl-sessions` (default) via `terraphim-agent --server sessions <sub>`. The offline twin at `main.rs:1593` awaits `handle_sessions_command` directly and is fine. |
| `shared_learning/store.rs:608-612` | `fn block_on` = `tokio::task::block_in_place(|| Handle::current().block_on(fut))`, feature `shared-learning` | only on a multi-thread runtime worker (or a thread that entered a multi-thread runtime). Panics: (a) `Handle::current()` when called with no runtime entered; (b) `block_in_place` on a `current_thread` runtime ("can call blocking only when running on the multi-threaded runtime"); (c) `block_in_place` inside a `LocalSet`. Deadlock: none inherent, because `block_in_place` moves the worker's other tasks off the thread. | used by every method of the sync `terraphim_types::shared_learning::LearningStore` impl (622, 633, 646, 701, 709, 719, 731, 745). In-crate tests that exercise it are all `#[tokio::test(flavor = "multi_thread")]` (1199 onwards); the plain `#[tokio::test]` tests at 781-948 never touch the sync trait. The real caller is `terraphim_middleware::feedback_loop` (cross-crate, section not B) whose runtime flavour cannot be seen from here. |
| `listener.rs:488…1251` | `ListenerRuntime::new` | not a tokio runtime; plain struct (1267) | n/a |
| `server_command.rs:9` | `use tokio::runtime::Runtime` | only used at 751 | see above |

No `#[tokio::main]`, no `Handle::try_current`, no `futures::executor::block_on`, no `Builder::new_current_thread` in src.

## Panic/poison surface

- `impl Drop`: none in src.
- `catch_unwind`, `mem::forget`, `ManuallyDrop`, `set_hook`, `resume_unwind`: none.
- `.lock().unwrap()` on std locks: `logging.rs:154,272` (`#[cfg(test)]` only). No `into_inner()`/`PoisonError` recovery anywhere.
- Std-lock poison fail-open: `kg_validation.rs:87,97` (`.ok()?` → validation permanently returns empty after any panic under the lock) and `store.rs:667` (`if let Ok(graph)` → graph ranking skipped).
- Tokio locks (`registry.rs`, `service.rs`, `store.rs:136`, `handler.rs:1914`) do not poison; a panic while holding one releases it and leaves partially-updated state (e.g. `reload` 226-234 clears the three maps in separate scopes, so a panic in `load_all_commands` leaves an empty registry).
- `unreachable!` at `server_command.rs:743` (`Command::Repl` arm, feature `repl`): relies on `main.rs:494` matching `Repl` before dispatch, which it does.
- `OnceLock` statics (`capture.rs:733`, `handler.rs:1909`): `get_or_init` closures are infallible constructors (`SessionService::new()`, thesaurus builder returning `Option`), so no panic-while-initialising path was found.
- Env: reads only. `std::env::var` at `onboarding/prompts.rs:301,420`, `main.rs:82`, `learnings/mod.rs:100,141`, `learnings/capture.rs:190,191`, `logging.rs:97`, `memory_command.rs:858`, `client.rs:15`, `listener.rs:1298`, `shared_learning/wiki_sync.rs:63,64,81,83,84,85,458,467`, `shared_learning/markdown_store.rs:47`. No `set_var`/`remove_var` in this crate; these reads are the race partners for the six `set_var` test blocks the inventory header attributes to another section when tests run in the same process.

## Notable patterns

- Sync-over-async bridge (`store.rs:608`) is the only `block_in_place` in the crate and the only place `Handle::current()` is used; correctness depends entirely on the caller's runtime flavour.
- Consistent "clone under write guard, drop guard, then persist" discipline in `SharedLearningStore` (283, 298, 447, 592-599), so the markdown backend is never awaited under the index lock.
- `TuiService.service` guard (a single tokio `Mutex` around the whole `TerraphimService`) is held across every backend await; all REPL search/config traffic serialises on it.
- Global mutable caches are std `RwLock`/`OnceLock` (`KG_CACHE`, `KG_THESAURUS`) touched from async contexts with filesystem I/O under the guard (`kg_validation.rs:87-116`).
- Child processes: three of four spawn sites set `kill_on_drop(true)`; `shell_dispatch.rs` (two sites) does not, so a timed-out agent dispatch leaks the child.
- Library-trait invariant surface: no manual `Hash`/`PartialEq`/`Eq`/`Ord`/`PartialOrd`/`Borrow`/`Iterator`/`size_hint`/`ExactSizeIterator`/`DoubleEndedIterator` impls; derives only. `judge/family.rs:60` returns `impl Iterator<Item=&'static str>` over a fixed slice.
- No `Rc`, `RefCell`, `Cell`, `thread_local!`, `LazyLock`, `once_cell`, `lazy_static`, atomics, channels, or `Semaphore` in src.

## Open questions

1. What runtime flavour does `terraphim_middleware::feedback_loop` (the production caller of the sync `LearningStore` trait) run under? If any consumer drives it from a `current_thread` runtime or a `LocalSet`, `store.rs:609` panics.
2. Is the `CommandRegistry` (`registry.rs`) ever shared between concurrently running tasks (e.g. REPL autocomplete plus a `reload`)? If yes, the `resolve_command` (aliases→commands) vs `get_stats` (commands→aliases) reader ordering with a queued writer needs a phase 2 interleaving check; if it is always single-task, the ordering is moot.
3. Does any CI or release configuration build with `--features server` and exercise `--server sessions …`? That is the only path to the nested `block_on` at `server_command.rs:751`.
4. Is the detached child left by `shell_dispatch.rs` on timeout intended (fire-and-forget agent), or an oversight relative to the `kill_on_drop` discipline used elsewhere?
5. `KG_CACHE` write guard across KG directory hashing and automaton build on a tokio worker: is `handle_hook_command` ever run on a shared multi-task runtime where blocking a worker matters, or only from the one-shot `hook` CLI arm?

## Bin expansion

`cargo +nightly expand -p terraphim_agent --bin terraphim-agent` (run against a private `CARGO_TARGET_DIR` to avoid the shared build lock; 35,697 lines, `/private/tmp/claude-501/ubx-expand/terraphim_agent_bin_B.rs`): 15 unsafe lines, all MACRO_GENERATED std output: 8 × `vec![]` (`box_assume_init_into_vec_unsafe`), 6 × `unsafe impl ::core::clone::TrivialClone` from `#[derive(Clone, Copy)]` on `CommandOutputMode`, `PollDecision`, `ModelFamily`, `CliKind`, `TuiAction`, `CommandOutputConfig`. No `unsafe fn`, no `unsafe extern`, no other `unsafe {}` blocks. The lead had directed this section be closed as SKIPPED on the basis of the lib result; the expand completed first and confirms the same picture.
