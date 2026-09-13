# Phase 2 findings — bucket 17: async-drop / runtime nesting

Sweeper: p2 static-bucket-sweeper (async-drop). Run: 2026-09-11-terraphim-clients-1, written 2026-09-11 23:47 BST. Source not modified.

**Verdict: NO MEMORY UB.** One deterministic panic on a shipped (feature-gated) command path (F-001), one undocumented runtime-flavour contract on a public sync trait impl with no in-workspace production caller (F-002), and a set of contractual-but-defensible runtime, lock and shutdown patterns (F-003 to F-007). Failure modes are panic, stall or truncated I/O; none is undefined behaviour.

tokio in Cargo.lock: 1.52.3 (Cargo.lock:7135-7136). Workspace tokio features: `full` (Cargo.toml:89), so `rt-multi-thread` is available to every crate. Registry sources cited below live under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.3/` (written `<tokio>`) and `~/.cargo/registry/src/git.terraphim.cloud-72dcd9d4c11821d8/terraphim_middleware-1.21.0/` (written `<middleware>`).

## Evidence

Signature sweep over `crates/` (rg only, per the rules):

```
rg -n 'block_on|Runtime::new|Builder::new_(current_thread|multi_thread)|Handle::current|block_in_place|spawn_blocking|#\[tokio::main|process::exit|impl Drop|mem::drop|drop\(rt|drop\(runtime' --type rust crates
```

Hit count: 109 lines. Breakdown after de-duplication and removal of false positives:

| pattern | hits | notes |
|---|---|---|
| `Runtime::new` (tokio) | 7 | main.rs:449, 494, 552, 2992; server_command.rs:751; update lib.rs:1408; benches/hybrid_search.rs:81, 112 (criterion, no runtime entered) |
| `Runtime::new` (false positive) | 10 | listener.rs `ListenerRuntime::new` is a domain struct, not tokio |
| `.block_on(` (method call) | 19 | main.rs x16 (112, 450, 498, 509, 545, 558, 570, 3004, 3009, 3011, 3117, 3157, 3165, 3174, 3189, 3220; `rg -c '\.block_on\(' main.rs` = 16), server_command.rs:752, update lib.rs:1409, store.rs:611 |
| `block_on(` (bare call to the local helper fn) | 8 | store.rs 622, 633, 646, 701, 709, 719, 731, 745, all inside the sync `LearningStore` impl (F-002) |
| `block_in_place` | 2 | store.rs:609, update lib.rs:1407 |
| `Handle::current` | 1 | store.rs:610 |
| `spawn_blocking` | 12 | update lib.rs (263, 307, 414, 574, 692, 1283), scheduler.rs:250, sessions cursor.rs:129, native.rs:161, cla/connector.rs:62, 122 |
| `#[tokio::main]` | 4 | terraphim_grep main.rs:477, terraphim_mcp_server main.rs:149, terraphim_lsp bin:4, terraphim_cli main.rs:351; all bare (multi_thread flavour) |
| `process::exit` | 50 | terraphim_agent main.rs x20, server_command.rs x14, learn_command.rs x5, robot_dispatch.rs:44, terraphim_cli main.rs x3, see F-007 |
| `impl Drop` | 0 | confirmed workspace-wide |
| `mem::drop` / `drop(rt` / `drop(runtime` | 0 | no Runtime is dropped by hand anywhere |

Supplementary sweeps:

```
rg -n 'LocalSet|spawn_local|Builder::new_current_thread|tokio::main\(|shutdown_background|shutdown_timeout|try_current|futures::executor' --type rust crates
```
Hits: 0 (four lines in listener.rs match `_current_` inside an unrelated identifier). No `LocalSet`, no explicit current_thread builder, no explicit runtime shutdown API, no non-tokio executor.

```
rg -n '#\[tokio::test\(' --type rust crates
```
Hits: 12. All are `flavor = "multi_thread"`: store.rs 1180, 1199, 1206, 1229, 1253, 1284, 1304, 1330, 1381, 1443; persistence_tests.rs:354; sessions native.rs:754. Every other `#[tokio::test]` in the workspace (roughly 420 across 53 files) is the bare form, which tokio's macro expands to the **current_thread** flavour.

Feature reachability (`crates/terraphim_agent/Cargo.toml` `[features]`): `default = ["repl-interactive", "llm", "repl-sessions", "shared-learning"]`; `server` is not default and is pulled in only by `repl-full`. No CI lane builds terraphim_agent with `server` (`rg -n features .github/workflows/*.yml .gitea/workflows/*.yml` hits only terraphim_sessions `enrichment`/`--all-features` and terraphim_grep lanes).

## Findings

### F-001 nested `Runtime::block_on` inside an async fn already driven by `Runtime::block_on`

- **File:line:** crates/terraphim_agent/src/server_command.rs:751-752 (inside `pub(crate) async fn run_server_command`, server_command.rs:23), `use tokio::runtime::Runtime` at server_command.rs:9.
- **Kind:** runtime nesting (`Runtime::new` + `Runtime::block_on` from an entered runtime).
- **Bucket(s):** 17 async-drop / runtime nesting; cross-ref 11 panic-safety.
- **Severity:** SUSPICIOUS. Deterministic panic on every execution of the path; not memory UB.
- **Static evidence:**
  - main.rs:552-558: `let rt = Runtime::new()?; ... rt.block_on(server_command::run_server_command(command, ...))` when `cli.server` is set (feature `server`).
  - `Runtime::block_on` on a multi_thread runtime dispatches to `MultiThread::block_on` (`<tokio>/src/runtime/runtime.rs:350-374`), which calls `context::enter_runtime(handle, true, ...)` (`<tokio>/src/runtime/scheduler/multi_thread/mod.rs:87-94`); the `true` is `allow_block_in_place`, so the calling thread becomes `EnterRuntime::Entered { allow_block_in_place: true }` (`<tokio>/src/runtime/context/runtime.rs:55-73`).
  - server_command.rs:751 `Runtime::new()?` succeeds (constructing a runtime does not enter it). server_command.rs:752 `rt.block_on(...)` calls `enter_runtime` again on the same thread; the context is already `Entered`, so `maybe_guard` is `None` and tokio panics with `"Cannot start a runtime from within a runtime. This happens because a function (like block_on) attempted to block the current thread while the thread is being used to drive asynchronous tasks."` (`<tokio>/src/runtime/context/runtime.rs:69`).
  - Unwinding drops the inner `rt` while the thread is still `Entered`. `Runtime` fields drop in order `scheduler`, `handle`, `blocking_pool` (`<tokio>/src/runtime/runtime.rs:97-106`); `BlockingPool::drop` calls `shutdown(None)` (`<tokio>/src/runtime/blocking/pool.rs:282-286`) which reaches `Receiver::wait` (`<tokio>/src/runtime/blocking/shutdown.rs:37-60`). `try_enter_blocking_region` returns `None` because the thread is entered (`<tokio>/src/runtime/context/blocking.rs:17-31`), but the `std::thread::panicking()` guard at shutdown.rs:47-49 returns `false` instead of panicking. So there is **no double panic and no abort**; the inner pool's threads are leaked and the original panic propagates through main.rs:558 and out of `main` (exit status 101).
  - The `Command::Sessions` arm is not intercepted before dispatch: main.rs:459-575 handles `Interactive`, `Repl`, `Listen`, `Robot` specially and passes every other command (including `Sessions`) to `run_server_command` when `--server` is given. The offline twin at main.rs:1593 (`handle_sessions_command(sub, &output).await`) awaits directly and is correct.
  - Consequence for the robot contract: `emit_robot_error_and_exit` (main.rs:562-565) is never reached because the panic skips the `Err` branch; no JSON error envelope is emitted in `--robot` mode.
  - Reachability: `--features server` (or `repl-full`) AND default `repl-sessions`, invoked as `terraphim-agent --server sessions <sources|list|...>`. `crates/terraphim_agent/tests/server_mode_tests.rs` has no `sessions` test (`rg -n sessions` over the file: 0 hits). No CI lane builds this feature combination.
- **Failure mode:** panic on first use; process exits 101 without robot JSON. No deadlock, no UB.
- **Draft experiment:** process-level, because the site lives in the bin crate. `cargo run -p terraphim_agent --features server -- --server sessions sources`; assert exit status 101 and stderr containing `Cannot start a runtime from within a runtime`. A second run with `--robot --server sessions sources` asserts that stdout carries no JSON envelope. Fix shape for the lead (not applied): the enclosing fn is already `async`, so replace `let rt = Runtime::new()?; rt.block_on(async { ... })` with a direct `.await` block, exactly as main.rs:1593 does.
- **Cross-refs:** Phase 1 B note row `server_command.rs:751-752` and open question B-3; p2-panic-safety (panic on shipped path).

### F-002 `block_in_place` + `Handle::current` sync-over-async bridge with an undocumented runtime-flavour contract

- **File:line:** crates/terraphim_agent/src/shared_learning/store.rs:608-612 (`fn block_on`, feature `shared-learning`, default on). Callers, all inside `impl terraphim_types::shared_learning::LearningStore for SharedLearningStore` (store.rs:615-753): `insert` 622, `get` 633, `query_relevant` 646, `record_applied` 701, `record_effective` 709, `list_by_trust` 719, `archive_stale` 731 and 745.
- **Kind:** runtime nesting (sync trait method blocks on the ambient runtime).
- **Bucket(s):** 17; cross-ref 11 panic-safety, 12 send-sync (trait is `Send + Sync`, `terraphim_types-1.22.1/src/shared_learning.rs:129`).
- **Severity:** SUSPICIOUS. Chosen over CONTRACTUAL-BUT-DEFENSIBLE because the flavour requirement is not documented on the impl or the trait, the trait is public and sync, and two of the three possible calling contexts panic. It would be CONTRACTUAL-BUT-DEFENSIBLE if the impl carried a doc comment stating "multi_thread runtime only".
- **Static evidence:** behaviour by calling context, from `<tokio>/src/runtime/scheduler/multi_thread/worker.rs:359-445` (`block_in_place`) and `<tokio>/src/runtime/handle.rs` (`Handle::current`):
  1. **multi_thread worker thread** (`(Entered, cx.is_some())`, worker.rs:399-403): legal. The worker hands its core to a fresh thread, exits the runtime context, runs the closure, then `Handle::current()` reads the still-set current handle and `Handle::block_on` re-enters as a fresh `enter_runtime` (context now `NotEntered`), so the nested `block_on` succeeds. Cost: one extra OS thread per concurrent sync call (thread growth, not starvation).
  2. **`Runtime::block_on` on the main thread of a multi_thread runtime** (`Entered { allow_block_in_place: true }` because `MultiThread::block_on` passes `true` to `enter_runtime`, multi_thread/mod.rs:91; `Handle::block_on` does the same at handle.rs:368; no worker context, worker.rs:404-418): legal, same mechanism.
  3. **current_thread runtime or `LocalSet`** (`Entered { allow_block_in_place: false }`, worker.rs:419-425): `block_in_place` panics with `"can call blocking only when running on the multi-threaded runtime"`. Every bare `#[tokio::test]` in the workspace is this flavour.
  4. **no runtime at all** (`(NotEntered, false)`, worker.rs:432-436): `block_in_place` runs the closure directly, then `Handle::current()` panics with `"there is no reactor running, must be called from the context of a Tokio 1.x runtime"` (`<tokio>/src/util/error.rs:5-6`, surfaced via handle.rs:726).
  5. **blocking-pool thread of a multi_thread runtime** (`(NotEntered, false)` but `rt.enter()` has set the current handle, `<tokio>/src/runtime/blocking/pool.rs:473`): legal; `Handle::block_on` enters the runtime from the pool thread.
  - Deadlock analysis: none inherent. In case 1 the worker's other tasks migrate with the core, so the future passed to `block_on` (a `tokio::sync::RwLock` acquire on `self.index` or a markdown-backend I/O future) can make progress. `archive_stale` (731-745) takes the write guard, drops it at 741, then re-enters `block_on` per stale item; no guard is held across the nested `block_on`.
  - **Tests exercising the sync API and their flavour** (awk over store.rs: every `dyn LearningStore` line 1183, 1202, 1209, 1232, 1256, 1287, 1307, 1365, 1433, 1446 sits under a `#[tokio::test(flavor = "multi_thread")]` attribute at 1180, 1199, 1206, 1229, 1253, 1284, 1304, 1330, 1381, 1443). Ten tests, all multi_thread. The bare `#[tokio::test]` functions in store.rs (781-948 region) call only the inherent async methods and never the sync trait. `crates/terraphim_agent/tests/shared_learning_cli_tests.rs` uses `SharedLearningStore::open` and async methods only. The tests therefore pass exactly because they pick the one flavour that works, and they encode the contract without stating it.
  - **Production callers (correction to the Phase 1 B note premise):** the B note assumed `terraphim_middleware::feedback_loop` drives the sync `LearningStore`. In the locked version (terraphim_middleware 1.21.0, Cargo.lock:6683-6684) `feedback_loop.rs` defines an **async** trait `GraphTouchStore` (feedback_loop.rs:16) and its two entry points take `&dyn GraphTouchStore` (feedback_loop.rs:50, 85); `SharedLearningStore` implements that trait at store.rs:585 with `Box::pin(async move { ... })`, no `block_on` involved. `rg -n 'dyn LearningStore|LearningStore for' --type rust crates` finds the sync trait used only in store.rs tests. main.rs:2433-2437 and 2670-2674 open `SharedLearningStore` and call its async methods. **The sync impl has zero production callers in this workspace.** Because `terraphim_types::shared_learning::LearningStore` is public, out-of-workspace consumers (the registry copy `terraphim_agent-1.21.3` and any terraphim-ai server code) remain an open question for the lead, not an assumption either way.
- **Failure mode:** panic (cases 3 and 4); thread growth (case 1). No deadlock, no UB.
- **Draft experiment:** two new tests in the `trait_tests` module of store.rs (not written):
  ```rust
  #[tokio::test] // bare: current_thread flavour
  #[should_panic(expected = "can call blocking only when running on the multi-threaded runtime")]
  async fn sync_trait_panics_on_current_thread() {
      let store = create_trait_test_store().await;
      let dyn_store: &dyn LearningStore = &store;
      let _ = dyn_store.get("missing");
  }

  #[test]
  #[should_panic(expected = "there is no reactor running")]
  fn sync_trait_panics_without_runtime() {
      let rt = tokio::runtime::Runtime::new().unwrap();
      let store = rt.block_on(create_trait_test_store());
      drop(rt); // no runtime entered, current handle unset on this thread
      let dyn_store: &dyn LearningStore = &store;
      let _ = dyn_store.get("missing");
  }
  ```
  Expected: both panic with the quoted substrings. A third, positive test can assert that ten concurrent sync calls from `tokio::spawn`ed tasks on a `worker_threads = 1` multi_thread runtime complete (exercising the core hand-off).
- **Cross-refs:** Phase 1 B note rows `shared_learning/store.rs:136` and `:608-612`, open question B-1 (now answered for this workspace: no production caller); p2-send-sync.

### F-003 `block_in_place` + fresh `Runtime::new` + `block_on` per scheduler tick

- **File:line:** crates/terraphim_update/src/lib.rs:1407-1410 inside the `check_fn` closure of `pub async fn start_update_scheduler` (lib.rs:1388). Sole invocation path: `UpdateScheduler::perform_check` -> `tokio::task::spawn_blocking(move || check_fn())` (scheduler.rs:250-254).
- **Kind:** runtime nesting (nested runtime on a blocking-pool thread) plus detached task lifetime.
- **Bucket(s):** 17.
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE.
- **Static evidence:**
  - On a blocking-pool thread the enter context is `NotEntered` and there is no worker context (worker.rs:432-436), so `block_in_place` runs the closure directly; `Runtime::new()` then `block_on` performs a fresh `enter_runtime` which succeeds (context/runtime.rs:55-66). This holds whether the outer runtime is multi_thread or current_thread, because `spawn_blocking` always moves the closure to the blocking pool. `Runtime::drop` on that thread is permitted (`try_enter_blocking_region` returns `Some` when not entered).
  - There is no other caller: `rg -n start_update_scheduler --type rust crates` hits only the definition and its doc example (lib.rs:1375-1388). No test calls it. `check_for_updates_auto` (lib.rs:1277) is an ordinary async fn that uses `spawn_blocking` internally (lib.rs:1283); calling it from a bare current_thread `#[tokio::test]` is fine and lib.rs:1713 `test_check_for_updates_auto` already does exactly that (live GitHub I/O, tolerates `Failed`). Direct callers terraphim_cli main.rs:846 and repl/handler.rs:1793 await it normally.
  - Shutdown facts (from Phase 1 E note, re-verified): `stop()` (scheduler.rs:191-201) aborts the interval task and waits up to 5 s, but abort cannot cancel an in-flight `spawn_blocking` closure; the nested runtime and its GitHub HTTP call run to completion. The `UpdateScheduler` value is dropped at the end of `start_update_scheduler` (lib.rs:1457) while its interval task keeps running, so the interval task can only be stopped by dropping the outer runtime. Dropping the outer runtime then waits indefinitely for the blocking closure (`BlockingPool::drop` -> `shutdown(None)`, pool.rs:282-286), so a `#[tokio::test]` or `#[tokio::main]` return would stall until the HTTP call finishes or times out.
  - A new multi_thread `Runtime` (worker threads = CPU count, plus blocking pool) is constructed and torn down on every tick. `start_update_scheduler` hard-codes `UpdateConfig::default()` (lib.rs:1397), whose `Default` impl sets `auto_update_enabled: true` and a 24 h interval (config.rs:42-43), and `UpdateScheduler::start` performs an immediate first check before the first tick (scheduler.rs:160-163). So the nested runtime runs once at start-up and then daily; it is not inert by default.
- **Failure mode:** none today (legal on both flavours). Latent: stalled runtime shutdown for the duration of one update check; per-tick runtime construction cost. If the closure were ever called from an async task instead of `spawn_blocking`, case 3 of F-002 applies (panic on current_thread) or `"Cannot start a runtime from within a runtime"` on a multi_thread worker.
- **Draft experiment:** `#[tokio::test]` (current_thread) that calls `start_update_scheduler("terraphim", "0.0.1", Box::new(|_| {}))` and returns immediately; the default config is enabled (config.rs:42) so the immediate first check (scheduler.rs:163) is already inside `spawn_blocking` when the test body ends. Measure test wall time. Expected: the test body completes but the runtime teardown blocks until the first GitHub check completes (seconds, or the reqwest timeout offline), demonstrating the shutdown stall. Also a static assertion that the closure is only reachable via `spawn_blocking` (already true). Fix shape (not applied): capture `Handle::current()` when `start_update_scheduler` is entered and use `handle.block_on` inside the closure, or make the closure async and drop the `spawn_blocking` hop.
- **Cross-refs:** Phase 1 E note "Runtime-nesting sites" and open questions E-3; inventory row `crates/terraphim_update/src/lib.rs:1407-1410`.

### F-004 multiple long-lived runtimes created on the sync main thread (terraphim-agent)

- **File:line:** crates/terraphim_agent/src/main.rs:449 (`rt` for the start-up update check, lives until `main` returns), 494 (`Repl` arm), 552 (generic command arm), 2992 (`run_tui`, called from `run_tui_offline_mode`/`run_tui_server_mode` at 579-586, which are called from the `Interactive` arm at 472-489); `ensure_tui_server_reachable` main.rs:105-113 borrows the TUI runtime.
- **Kind:** runtime construction / coexistence.
- **Bucket(s):** 17.
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE.
- **Static evidence:** all four `Runtime::new()` calls are in sync functions on the main thread and none sits inside a `block_on` closure or an async fn (`fn main` 442, `fn run_tui` 2917). The first runtime's `block_on` at 450-456 returns before any second runtime is created, so the thread is `NotEntered` at 494, 552 and 2992. Two runtimes (three thread pools with the blocking pools) coexist for the process lifetime; each holds CPU-count idle worker threads. In `run_tui` the runtime is created once at 2992. Three `rt.block_on` calls are pre-loop set-up (3004 backend construction, 3009 initial config, 3011 initial rolegraph terms); the crossterm loop starts at 3016 and the six calls inside it (3117, 3157, 3165, 3174, 3189, 3220) reuse the same `rt`: **no per-iteration runtime creation**. Runtimes are dropped when `main` returns, on a `NotEntered` thread, so no drop-in-async panic; `BlockingPool::drop` waits for any outstanding `spawn_blocking` work (none expected, the update check is awaited to completion at 450-456).
- **Failure mode:** none. Resource cost only. Each `rt.block_on` inside the TUI draw/poll loop blocks the UI thread for the duration of the backend call (search, autocomplete, LLM summary at 3189/3220); that is a responsiveness fact outside this bucket, recorded because the user's global rule says the UI must stay responsive.
- **Draft experiment:** none needed for soundness. If the lead wants the thread count measured: run `terraphim-agent` in TUI mode and count threads (`ps -M <pid>` on macOS); expect roughly 2 x CPU count worker threads plus blocking-pool threads.
- **Cross-refs:** Phase 1 B note rows `main.rs:449-456`, `494-509`, `552-570`, `2992-3230`, `107-113`.

### F-005 tokio lock guards held across `.await` (stall surface, consistent lock order, no inversion)

- **File:line:**
  - crates/terraphim_mcp_server/src/lib.rs:510-581: `autocomplete_index.read().await` guard live across `self.terraphim_service().await` (531-534) and `service.search(&sq).await` (553) inside a loop, until scope end at 581.
  - crates/terraphim_cli/src/service.rs:266-267 (`service` mutex across `TerraphimService::update_selected_role`, which takes `config_state.config` internally), 296-297 (`service` across `search`), 335-336 (`service` across `ensure_thesaurus_loaded`).
  - crates/terraphim_command_runtime/src/lib.rs:67-68 (`config` mutex across `config.save().await`, persistence I/O); the same pattern at crates/terraphim_agent/src/service.rs:682-683.
  - New in this sweep: crates/terraphim_agent/src/service.rs:555-590 (`chat`): `config_state.config` mutex guard held across the LLM HTTP call `llm_client.chat_completion(...).await` at 588-590.
  - crates/terraphim_agent/src/repl/handler.rs:1914: `OnceLock<Arc<tokio::sync::Mutex<SessionService>>>` guard held for the whole sessions subcommand.
- **Kind:** guard across await (tokio `Mutex` / `RwLock`).
- **Bucket(s):** 17; cross-ref 10 data-races (none: the guards are tokio async locks, `Send` across await by design).
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE.
- **Static evidence:**
  - Lock order observed across terraphim_cli service.rs, terraphim_agent service.rs and terraphim_command_runtime: `service` -> `config` (inside `TerraphimService::update_selected_role`, registry `terraphim_service-1.21.1/src/lib.rs:185` per Phase 1 H note) and `service` -> `config`/`rolegraph` (inside `search`/`ensure_thesaurus_loaded`). Every helper that takes `config` directly (command_runtime lib.rs:18, 24, 30, 42, 67; agent service.rs:555, 682, 696, 923, 938) takes no other lock while holding it; `chat` at 555 holds `config` across an HTTP await but never touches `service` or `rolegraph`. The rolegraph-only sites (agent service.rs:479, 518, 720; cli service.rs:311) take no other lock. **No path takes `config` and then `service`, so there is no lock-order inversion and no deadlock.**
  - MCP server: `terraphim_service()` (lib.rs:159-171) locks `config_state.config`, clones, and drops the guard at 165 before awaiting `ConfigState::new` at 167-168; so while the `autocomplete_index` read guard is held the only nested lock is a brief `config` mutex. Writers to `autocomplete_index` are lib.rs:372 (`build_autocomplete_index`) and 788 (`deserialize_autocomplete_index`); neither holds any other lock at the time and neither is called from inside the 510-581 read scope, so there is no re-entrant write and no self-deadlock. `tokio::sync::RwLock` is write-preferring, so a writer queued behind the 510 reader blocks the other readers at 418, 590, 640, 691, 737 until every `service.search` in the loop returns (up to `limit` sequential searches). This is a stall/fairness surface under rmcp's concurrent tool dispatch, not a deadlock.
  - Contention reality: terraphim_cli is a single task (`#[tokio::main]` running one command); the agent TUI drives everything serially via `rt.block_on` on one thread; the REPL is a single task. Only the MCP server handles requests concurrently, and its config mutex is held only briefly.
  - tokio locks do not poison; a panic inside a guarded region releases the lock on unwind, so no permanent wedge follows a panic.
- **Failure mode:** stall (head-of-line blocking) proportional to the awaited I/O; no deadlock; no UB.
- **Draft experiment:** for the MCP site, a multi_thread `#[tokio::test]` that (a) builds the index, (b) starts `autocomplete_with_snippets` with `limit = 50` against a slow haystack, (c) concurrently calls `build_autocomplete_index` and then `autocomplete_terms`, and measures that (c)'s reader is delayed until (b) finishes; expected: delay equals the duration of (b), confirming write-preferring head-of-line blocking without deadlock. For the CLI and agent sites no experiment is needed; the static order argument closes them.
- **Cross-refs:** Phase 1 G note lines 68-70, 74; H note lines 101-106, 197, open question H-4 (answered: the shared helpers are only multi-task in the MCP server, which does not use them); p2-data-races.

### F-006 detached tasks whose `JoinHandle` is dropped while they hold shared `Arc` state

- **File:line:**
  - crates/terraphim_lsp/src/server.rs:46-73 (`schedule_diagnostics`): `pending` mutex guard held across `tokio::spawn` (51) but across no `.await`; the spawned task captures `Arc<Mutex<HashMap<Url, String>>>` (`documents`), a `Client` clone and a `NegativeContributionScanner` clone; only the `AbortHandle` is stored (73), the `JoinHandle` is dropped.
  - crates/terraphim_sessions/src/connector/native.rs:161-249 (`watch_at`): `spawn_blocking` closure owning `Arc<PathBuf>`, the notify watcher and the `mpsc::Sender<Session>`; its handle is dropped at 249; initialisation errors are relayed through a oneshot (159, 168-176, 179, 255).
- **Kind:** detached task lifetime past shutdown.
- **Bucket(s):** 17; cross-ref 11 panic-safety (silent panic in a detached task).
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE.
- **Static evidence:**
  - LSP: the detached task performs a `sleep`, a brief `documents.lock().await`, a pure `scan_file` and `client.publish_diagnostics(...).await`. Nothing in it blocks a thread. On runtime shutdown (`#[tokio::main]` return in the bin at src/bin/terraphim-lsp.rs:4) pending tasks are dropped, releasing their `Arc`s; there is no `Drop` impl anywhere that could run async work during that teardown (impl Drop count = 0). A panic in `scan_file` is swallowed at the task boundary because no `JoinHandle` is awaited; it does not poison `documents` (tokio mutex). `pending` is never pruned and `did_close` does not abort the debounce task (Phase 1 H open question 2): unbounded map growth per URI over a long editor session and a possible post-close `publish_diagnostics`.
  - Sessions: the blocking loop exits when `tx.is_closed()` (192-194) or the watcher channel disconnects (216), polling every 50 ms (`recv_timeout`, 196). `Runtime::drop` first shuts the scheduler (dropping tasks, and with them any `Receiver` a task owns) and only then drops `blocking_pool`, whose `shutdown(None)` joins the blocking thread (runtime.rs:97-106, pool.rs:282-286). So when the receiver is owned by a task or by a local in `#[tokio::main]`/`#[tokio::test]`, the loop observes `is_closed()` within 50 ms and shutdown completes. The hang case is a receiver that outlives the runtime (held on a stack frame that drops the `Runtime` first); `ConnectorRegistry::watch_all` (connector/mod.rs:276-298) returns the receivers to the caller and **has no callers in the workspace** (`rg -n 'watch_all\(\)|\.watch\(\)\.await' --type rust crates`: 0 outside mod.rs), so the only in-workspace driver is the multi_thread test at native.rs:754-761, where `rx` is a local dropped before the test runtime. Parsing runs synchronously on the blocking thread (`parse_session_file_sync`, 451-453) precisely to avoid the `tokio::spawn`-inside-`spawn_blocking` stall on current_thread test runtimes that the comment at 451 records.
- **Failure mode:** LSP: silent panic, unbounded `pending` growth, stale post-close diagnostics. Sessions: none in-workspace; latent shutdown hang for an external caller that drops the runtime before the receiver. No UB.
- **Draft experiment:** sync `#[test]`: `let rt = Runtime::new()?; let rx = rt.block_on(NativeClaudeConnector.watch_at(tmp))?; drop(rt);` while `rx` is still alive. Expected: `drop(rt)` blocks forever (the loop never sees `is_closed()`); wrap in a thread with a 5 s join timeout to turn the hang into an assertion. A control that drops `rx` before `rt` must return within roughly 100 ms.
- **Cross-refs:** Phase 1 H note lines 36-60 and open questions H-2, H-3; D note (terraphim_sessions).

### F-007 `std::process::exit` from inside a running runtime

- **File:line:** inside async fns driven by `Runtime::block_on` on the main thread: crates/terraphim_agent/src/main.rs:740, 752, 770, 786 (`handle_guard_command`), 858 (`handle_setup_command`), 1333 (`handle_search_command`), 1760, 1817 (`handle_sessions_command`), 1897, 1903 (`handle_config_command`), 2351, 2375 (`run_cache_command`); crates/terraphim_agent/src/server_command.rs:246, 497, 513, 549, 561, 572, 579, 637, 649, 727, 865, 927, 969, 977 (`run_server_command`); crates/terraphim_agent/src/learn_command.rs:332, 341, 353, 397, 500 (`run_learn_command`); crates/terraphim_cli/src/main.rs:437, 452, 472 (inside `#[tokio::main] async fn main`). Sync-context sites (no runtime entered): main.rs:464, 469, 521, 527 and robot_dispatch.rs:44 (`emit_robot_error_and_exit`, called from `main` after `block_on` has returned, 562-573).
- **Kind:** process exit skipping runtime shutdown.
- **Bucket(s):** 17.
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE.
- **Static evidence:** `std::process::exit` runs `crate::rt::cleanup()` before `sys::exit::exit(code)` (rust std `library/std/src/process.rs:2537-2539`), which flushes the buffered stdout, so JSON already written with `println!` survives. No destructors run: worker and blocking-pool threads are killed mid-operation, so any `spawn_blocking` persistence write in flight (for example `config.save()` on another task) is truncated at whatever point the OS kills the thread. Since the CLIs are single-task and the exit calls sit on the error path after their own output is complete, there is no in-flight concurrent work in practice. The pattern is the same in terraphim_cli's async `main`.
- **Failure mode:** truncated background I/O in the unlikely concurrent case; otherwise none. No UB.
- **Draft experiment:** none for soundness. If the lead wants the robot contract exercised, a process-level test that triggers main.rs:1333 (search with an unknown role in `--robot` mode) and asserts a complete JSON envelope on stdout before exit code `ErrorNotFound` confirms the flush behaviour.
- **Cross-refs:** p2-panic-safety; Phase 1 B note (child-process lifetimes).

## Non-findings

| site | pattern | why it is not a finding |
|---|---|---|
| workspace-wide | `impl Drop` | 0 hits (`rg -c 'impl Drop\|impl<.*> Drop' --type rust crates`). No drop path can block on async work. |
| workspace-wide | `mem::drop` / `drop(rt` / `drop(runtime` of a tokio `Runtime` | 0 hits. The only Runtime dropped inside an async context is the unwind path of F-001, which tokio guards with `thread::panicking()`. |
| workspace-wide | `LocalSet` / `spawn_local` / `Builder::new_current_thread` / `shutdown_background` / `shutdown_timeout` / `Handle::try_current` / `futures::executor::block_on` | 0 hits. The third `block_in_place` panic branch (LocalSet) is unreachable in-workspace. |
| crates/terraphim_agent/src/listener.rs:488-1748 | `ListenerRuntime::new` | domain struct, not `tokio::runtime::Runtime`; `run_forever().await` at 1748 is driven by main.rs:545 `rt.block_on`. |
| crates/terraphim_grep/benches/hybrid_search.rs:81, 112 | `Runtime::new` | criterion bench body, sync thread, no outer runtime. |
| crates/terraphim_grep/src/main.rs:477, terraphim_mcp_server/src/main.rs:149, terraphim_lsp/src/bin/terraphim-lsp.rs:4, terraphim_cli/src/main.rs:351 | `#[tokio::main]` | bare attribute (multi_thread); no nested `block_on` in those crates (`rg 'block_on\|Runtime::new\|Handle::current' crates/terraphim_mcp_server crates/terraphim_grep/src crates/terraphim_lsp crates/terraphim_cli`: 0 hits in src). |
| crates/terraphim_update/src/lib.rs:263, 307, 414, 574, 692, 1283; scheduler.rs:250; sessions cursor.rs:129, cla/connector.rs:62, 122 | `spawn_blocking` | awaited `JoinHandle`s on the ambient runtime; legal under both flavours. |
| crates/terraphim_update/src/scheduler.rs tests 347-511 | bare `#[tokio::test]` (current_thread) | they pass plain closures to the scheduler and never touch the nested-runtime closure of F-003. |
| crates/terraphim_update/src/lib.rs:1713 `test_check_for_updates_auto` | current_thread `#[tokio::test]` calling `check_for_updates_auto` | the function uses `spawn_blocking` only; no nested runtime. |
| crates/terraphim_agent/src/shared_learning/store.rs:267-298, 429-447 | `tokio::sync::RwLock` write guards | explicitly dropped before `persist(...).await` (283, 298, 447); no guard across await. |
| crates/terraphim_agent/src/shared_learning/store.rs:667 | `std::sync::RwLock` read inside sync `query_relevant` | taken after the tokio read guard is dropped at 659; no std lock inside an async guard, no await under it. |
| crates/terraphim_lsp/src/server.rs:46 | tokio mutex guard across `tokio::spawn` | `spawn` is not an await point; guard released at 74. |
| terraphim_middleware 1.21.0 `feedback_loop` | assumed sync-trait consumer | takes async `&dyn GraphTouchStore`; does not call the F-002 bridge. |

## Open questions for the lead

1. Does any consumer outside this workspace (terraphim-ai server, the registry `terraphim_agent-1.21.3`) call the sync `terraphim_types::shared_learning::LearningStore` methods from a current_thread runtime or from no runtime? F-002 is latent-only until that is answered.
2. Should the `server` feature gain a CI lane? F-001 is invisible to every current build.
