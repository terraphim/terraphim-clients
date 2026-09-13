# Module: terraphim_sessions (section D)

Scope: `crates/terraphim_sessions/src` (18 files, 10,820 lines including inline `#[cfg(test)]` modules) plus `crates/terraphim_sessions/benches/search_nfr.rs`. No `tests/` directory exists. Crate version 1.21.2. Mapper: p1-D, 2026-09-11 23:15 BST.

Sweeps run with `rg` over `src` and `benches`: `unsafe|SAFETY`; `Arc<Mutex|Arc<RwLock|std::sync::|tokio::sync::|Atomic|OnceLock|LazyLock|thread_local|thread::spawn|tokio::spawn|spawn_blocking|rayon|par_iter|Mutex<|RwLock<|mpsc|Semaphore|Notify|watch::|oneshot`; `block_on|Handle::current|Runtime::new|Builder::new`; `impl.*Drop`; `.lock()|.read()|.write()` with and without `unwrap|expect`; `catch_unwind|resume_unwind|panic::`; `env::|set_var|remove_var|home_dir|config_dir|data_dir|current_dir|temp_dir`; `impl (Hash|PartialEq|Eq|Ord|PartialOrd|Borrow|Iterator|ExactSizeIterator|DoubleEndedIterator|Deref|AsRef|Send|Sync) for`; `#[derive(` mixing `Hash`/`Eq`/`Ord`; `partial_cmp|sort_by|unwrap()|expect(|unreachable|panic!|unwrap_unchecked|get_unchecked|from_raw|transmute|mem::forget|ManuallyDrop|MaybeUninit|ptr::|static mut|no_mangle|extern "C"`.

Expansion: `cargo +nightly expand -p terraphim_sessions --lib` -> `/private/tmp/claude-501/ubx-expand/terraphim_sessions.rs`, 9,891 lines, exit 0. cfg set: default features (`terraphim-session-analyzer`, `aider-connector`, `search-index`), no `#[cfg(test)]` code, no `enrichment`, no `cline-connector`/`opencode-connector`/`codex-connector`/`cursor-connector`. Those non-default files (`connector/cline.rs`, `connector/opencode.rs`, `connector/codex.rs`, `connector/cursor.rs`, `enrichment/*`) were audited by source read only. A supplementary `cargo +nightly expand -p terraphim_sessions --lib --tests --features enrichment` was queued to `/private/tmp/claude-501/ubx-expand/terraphim_sessions_tests_enrichment.rs`; see the addendum at the end of this file for its status.

## Purpose

Session-history import and search layer for AI coding assistants. `connector/` holds one `SessionConnector` implementation per source (native Claude Code JSONL at `connector/native.rs`, Aider markdown at `connector/aider.rs`, plus feature-gated Cline, OpenCode, Codex and Cursor SQLite readers) behind a `ConnectorRegistry` (`connector/mod.rs:152`). `cla/` wraps the sibling `terraphim-session-analyzer` crate's synchronous connectors. `service.rs` exposes `SessionService`, an in-memory cache with lazy auto-import. `redaction.rs` strips secrets with a process-wide compiled regex set. `search.rs` ranks sessions with BM25 from `terraphim_types` (feature `search-index`). `enrichment/` (feature `enrichment`) matches thesaurus concepts with `terraphim_automata` and optionally consults a shared `RoleGraph`.

## Unsafe-surface tally

| Kind | Count | Sites |
|---|---|---|
| SOURCE_DIRECT `unsafe` blocks or `unsafe fn`/`unsafe impl` in src | 0 | `rg -n 'unsafe' src benches` returns nothing. |
| SOURCE_DIRECT `unsafe` in tests | 0 | No `tests/` directory; inline `#[cfg(test)]` modules contain no `unsafe`. |
| `SAFETY` comments | 0 | |
| MACRO_GENERATED `unsafe` in `--lib` expansion | 7 | 6 x `::alloc::boxed::box_assume_init_into_vec_unsafe` from the std `vec![...]` macro on this nightly; 1 x `unsafe impl ::core::clone::TrivialClone for FileOperation` from `#[derive(Clone)]` on a `Copy` enum. Attribution below. |
| `impl Drop` | 0 | |
| `catch_unwind` / `resume_unwind` | 0 | |
| raw pointers, FFI, `transmute`, `from_raw`, `MaybeUninit`, `static mut`, `#[no_mangle]`, `extern "C"` | 0 | |
| `block_on` / `Handle::current` / `Runtime::new` | 0 | Async entry is only via `#[tokio::test]` in tests and `#[async_trait]` methods. |
| `process::exit` | 0 | `std::process::id()` is read in test support only (`search_tests_support.rs:202,221`). |

Attribution of the 7 MACRO_GENERATED sites (expansion line -> source):

| Expansion line | Source site | Originating macro |
|---|---|---|
| 1097 | `src/connector/native.rs:333` (`blocks: vec![ContentBlock::Text {..}]`) | std `vec!` |
| 1135 | `src/connector/native.rs:362` (`blocks: vec![ContentBlock::Text { text }]`) | std `vec!` |
| 3662 | `src/connector/aider.rs:153` (`vec!["aider".to_string(), "chat".to_string()]`) | std `vec!` |
| 3906 | `src/connector/aider.rs:322` (`let mut dirs_to_visit = vec![base_path.to_path_buf()]`) | std `vec!` |
| 6495 | `src/model.rs:193` (`Message::text`, `blocks: vec![ContentBlock::Text { text: content }]`) | std `vec!` |
| 7857 | `src/model.rs:755` (`#[derive(Debug, Clone, Copy, ...)] pub enum FileOperation`) | `#[derive(Clone)]` on a `Copy` type emits `unsafe impl TrivialClone` |
| 9679 | `src/cla/connector.rs` (`from_normalized_message`, `blocks: vec![ContentBlock::Text {..}]`) | std `vec!` |

`vec![]` with zero elements (`native.rs:321,505`) expands to `Vec::new()` and contributes no `unsafe`. `#[async_trait]` expands to `Box::pin` with no `unsafe`. `serde` derives in this expansion contain no `unsafe`.

## Concurrency surface

All lock types in the crate are `tokio::sync`; there is no `std::sync::Mutex` or `std::sync::RwLock` anywhere. There are no atomics, no `thread_local!`, no `LazyLock`, no `std::thread::spawn`, no `rayon`.

| file:line | primitive | what it guards | who accesses |
|---|---|---|---|
| `src/service.rs:18` | `Arc<tokio::sync::RwLock<HashMap<SessionId, Session>>>` (`SessionService::cache`) | In-memory session cache keyed by `SessionId = String` (`model.rs:13`). | Read guards at `service.rs:80,169,175,206,253,299,309,335,364,375,512`; write guards at `service.rs:140,153,315,321`. Every guard is a local dropped at end of scope or, at `service.rs:514`, explicitly `drop(cache)` before the clustering loop. No `.await` occurs while a guard is held: the only awaits in guarded functions (`self.maybe_auto_import().await`, `connector.import(options).await`, `self.registry.import_all(options).await`) complete before the guard is taken. |
| `src/service.rs:22` | `Arc<tokio::sync::RwLock<bool>>` (`SessionService::auto_import_attempted`) | Once-only flag for lazy auto-import. | Read at `service.rs:72` (guard dropped at 75), write at `service.rs:98`. Between the read and the write, `maybe_auto_import` awaits `self.import_all` (`service.rs:87`) with no guard held. Two callers racing on a fresh service can both observe `false`, both import, and both write `true`; the second import re-inserts identical keys into the cache. This is a documented check-then-act window, recorded as a fact. |
| `src/service.rs:397-405` | `impl Clone for SessionService` | Constructs a fresh `ConnectorRegistry::new()`, a new empty `cache` and a new `auto_import_attempted = false`. | A clone shares no state with its source; only the `auto_import: bool` is copied. Callers holding `Arc<SessionService>` share state, callers using `.clone()` do not. |
| `src/enrichment/enricher.rs:58,74` (feature `enrichment`) | `Option<Arc<tokio::sync::RwLock<RoleGraph>>>` (`SessionEnricher::rolegraph`) | Shared role graph owned by the caller. | Read guard at `enricher.rs:119`, held across the synchronous call `self.find_graph_connections(&mut concepts, &graph)` (`enricher.rs:120`, fn at `enricher.rs:198`, which contains no `.await`). Guard dropped at the end of the `if let` block at `enricher.rs:121`. The crate never takes a write guard; writers live outside the crate. `SessionEnricher` itself is used by `&self` only. |
| `src/redaction.rs:55` | `static COMPILED_PATTERNS: std::sync::OnceLock<Vec<(Regex, &'static str)>>` | Lazily compiled secret-redaction regex set, built once per process. | `compiled_patterns()` at `redaction.rs:58` calls `get_or_init` on every `redact_session_content` call. Read-only after init; `Regex` is `Send + Sync`. |
| `src/connector/mod.rs:146` and `src/connector/native.rs:154` | `tokio::sync::mpsc::channel(32)` (`Sender<Session>` / `Receiver<Session>`) | Watch-event stream from the file watcher to the async consumer. | `Sender` moved into the `spawn_blocking` closure at `native.rs:161`; `tx.is_closed()` polled at `native.rs:193`; `tx.blocking_send` at `native.rs:238`. `Receiver` returned to the caller at `native.rs:255` and collected by `ConnectorRegistry::watch_all` at `mod.rs:276-297`. |
| `src/connector/native.rs:159` | `tokio::sync::oneshot::channel::<Result<()>>()` | One-shot readiness/initialisation-error hand-off from the blocking thread back to `watch_at`. | Sent at `native.rs:168,173,178` (three exclusive paths, each followed by `return` or fall-through into the loop); awaited at `native.rs:253` with `ready_rx.await??`. |
| `src/connector/native.rs:162` | `std::sync::mpsc::channel()` (unbounded) | Raw `notify` events from the watcher's internal thread to the blocking loop. | `watcher_tx` captured by the `recommended_watcher` callback closure at `native.rs:163-165` (runs on the `notify` backend thread); `watcher_rx.recv_timeout(50ms)` at `native.rs:198` on the `spawn_blocking` thread. `Disconnected` at `native.rs:216` breaks the loop. |
| `src/connector/native.rs:161` | `tokio::task::spawn_blocking` (JoinHandle dropped) | Detached watcher loop. | Runs until `tx.is_closed()` (`native.rs:193`) or the notify channel disconnects. The `watcher` value (`native.rs:163`) lives on that thread's stack for the loop's lifetime and is dropped when the loop exits. Nothing awaits the JoinHandle, so a panic inside the loop is reported only via tokio's blocking-pool logging. |
| `src/connector/native.rs:155` | `Arc<PathBuf>` (`path`) | Watch root path. | Moved into the blocking closure; the `Arc` is not cloned anywhere else, so it is effectively single-owner. |
| `src/connector/cursor.rs:129` (feature `cursor-connector`) | `tokio::task::spawn_blocking`, awaited | Synchronous `rusqlite` reads of each `state.vscdb`. | `HashSet<String>` `seen_ids` is created inside the closure (`cursor.rs:131`) and threaded through `parse_database` by `&mut`; nothing is shared across threads. `Connection::open` at `cursor.rs:163` per database. Result awaited with `.await?` at `cursor.rs:150`. |
| `src/cla/connector.rs:62,122` (feature `terraphim-session-analyzer`, default) | `tokio::task::spawn_blocking`, awaited | Synchronous `terraphim-session-analyzer` connector import. | A fresh stateless connector is constructed inside each closure; `cla_options` is moved in. Result awaited with `.await??` at `cla/connector.rs:65,125`. |
| `src/connector/mod.rs:109` | `pub trait SessionConnector: Send + Sync` (`#[async_trait]`) | Object-safety bound for `Vec<Box<dyn SessionConnector>>` at `mod.rs:153`. | All connector structs are unit or plain-data structs; no interior mutability. `SessionService` is therefore auto-`Send + Sync` through `Arc<RwLock<..>>` plus `Vec<Box<dyn SessionConnector>>`. No explicit `unsafe impl Send/Sync` and no compile-time `assert_send` test exists. |

Test-side concurrency: `native.rs:754` `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]` is the only multi-thread test and is `#[ignore]`d (`native.rs:752`); it uses `tokio::pin!` and `tokio::select!` (`native.rs:791-800`). All other async tests are default `current_thread`. `native.rs:451-455` documents that `parse_session_file_sync` exists specifically to avoid `tokio::spawn` inside `spawn_blocking` on single-threaded test runtimes.

## Lock-poisoning and panic-safety surface

- **No poisonable locks.** Every `RwLock` is `tokio::sync::RwLock` (`service.rs:11`, `enricher.rs:8`), which has no poisoning; a panic while a guard is held simply drops the guard. No `.lock().unwrap()` / `.read().unwrap()` / `.expect(..)` on any lock exists.
- **`OnceLock` init.** `redaction.rs:59` `get_or_init` closure uses `filter_map` and `tracing::warn!` on regex compile failure (`redaction.rs:63-66`); it has no `unwrap`/`expect` and therefore no panic path. If it did panic the cell would stay uninitialised (not poisoned) and the next caller would retry. The closure does not re-enter `compiled_patterns`, so there is no reentrancy deadlock.
- **`spawn_blocking` panic propagation.** Panics inside the awaited blocking closures surface as `JoinError` through `.await?` at `cla/connector.rs:65,125` and `cursor.rs:150`. The detached watcher closure at `native.rs:161` has no awaiter; a panic there ends the watch silently apart from tokio's own logging, and the `Receiver` sees channel closure.
- **Non-test `expect`/`unwrap` in library code.** `src/connector/cline.rs:346-347` `jiff::Timestamp::from_millisecond(item.ts).expect("Invalid timestamp in taskHistory")` (feature `cline-connector`) panics on an out-of-range millisecond value read from the user's `taskHistory` JSON. `src/connector/cursor.rs:724` `db_path.parent().unwrap()` is inside `#[cfg(test)]`. No other `unwrap`/`expect` appears outside `#[cfg(test)]` modules or `search_tests_support.rs` (which is `#[cfg(test)]` per `lib.rs:44`).
- **Float comparator.** `src/search.rs:135-139` and `src/search.rs:197-201` sort `Vec<Scored<Session>>` with `b.score().partial_cmp(&a.score()).unwrap_or(Ordering::Equal)`. Scores are `f64` from BM25 plus an integer boost (`search.rs:193`). If a `NaN` score ever appeared the comparator would violate total-order requirements; `slice::sort_by` on Rust 1.81+ may panic on such input rather than exhibit UB. No NaN source was identified in this crate; the scores originate in `terraphim_types`.
- **Other sorts.** `enricher.rs:248,291`, `concept.rs:133`, `cline.rs:344` sort on integers via `cmp`/`sort_by_key`; total orders.
- **`panic!`** appears only inside `#[cfg(test)]` (`native.rs:557,571,583`).

## Library-trait invariant surface

- **Manual `Hash`/`PartialEq`/`Eq`/`Ord`/`PartialOrd`/`Borrow`/`Iterator`/`ExactSizeIterator` impls: none.** The only manual trait impls are `Default` (`service.rs:391`, `enricher.rs:28`, `cline.rs:260`, `mod.rs:301`), `Clone` (`service.rs:397`, discussed above), `From<&str> for MessageRole` (`model.rs:33`), and `Display` (`model.rs:45`, `model.rs:761`).
- **Derives.** `model.rs:19` `MessageRole`: `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` (no `Hash`). `model.rs:755` `FileOperation`: `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize` (no `Hash`). `connector/aider.rs:22` internal `enum ParseState`: `Debug, Clone, PartialEq` without `Eq`; it is compared with `==` only and never used as a map key. No `#[derive(Hash)]` exists in the crate, so no derived-`Hash`/manual-`PartialEq` mismatch is possible.
- **Map/set key types (dedupe surface).** All keys are std types with consistent `Hash`/`Eq`: `HashMap<SessionId = String, Session>` (`service.rs:18`), `HashSet<String>` (`cursor.rs:131` `seen_ids`, `model.rs:314`, `search.rs:179`, `service.rs:456-532`), `HashMap<String, SessionConcepts>` and `HashMap<String, ConceptMatch>` (`enricher.rs:224,256`, `concept.rs:80`), `HashMap<&str, &Session>` (`search.rs:121`), `HashMap<PathBuf, Instant>` and `HashMap<PathBuf, usize>` (`native.rs:187,189`), `HashMap<usize, ..>` (`concept.rs:160`, `service.rs:571`). Session-level dedupe in `SessionService::load_sessions` (`service.rs:321-325`) and `import_*` (`service.rs:140-143,153-156`) is last-writer-wins by `String` id. Cursor dedupe (`cursor.rs:206,249`) uses `HashSet::insert` return value on `String` ids.
- **`Scored<Session>`** (`search.rs:131`) comes from `terraphim_types`; its ordering behaviour is not defined in this crate (see comparator note above).

## Env / process-global access

All accesses are reads; there is no `set_var`, `remove_var`, `set_current_dir` or `static mut` in the crate.

| file:line | access | cfg |
|---|---|---|
| `src/connector/native.rs:54` | `dirs::home_dir()` -> `~/.claude/projects` | default |
| `src/connector/aider.rs:71` | `std::env::current_dir().ok()` as the default Aider search root | `aider-connector` (default) |
| `src/connector/cline.rs:232,247` | `dirs::data_dir()`, `dirs::home_dir()` | `cline-connector` |
| `src/connector/codex.rs:111` | `dirs::home_dir()` -> `~/.codex/sessions` | `codex-connector` |
| `src/connector/opencode.rs:67` | `dirs::home_dir()` -> `~/.local/state/opencode` | `opencode-connector` |
| `src/connector/cursor.rs:76,86` | `dirs::home_dir()` (macOS, Linux branches) | `cursor-connector` |
| `src/connector/cursor.rs:91` | `std::env::var("APPDATA")` | `cursor-connector` + `target_os = "windows"` |
| `src/search_tests_support.rs:202,221` | `std::env::temp_dir()` joined with `std::process::id()` | `#[cfg(test)]` only |
| `src/redaction.rs:55` | process-wide `OnceLock` static (see above) | default |
| `src/lib.rs:71` | `env!("CARGO_PKG_VERSION")` compile-time only | default |

`dirs::*` reads `HOME`/`XDG_*`/`APPDATA` through `getenv` at call time; any test in another crate of the same test binary that calls `set_var` concurrently would race with these reads. No test in this crate sets environment variables, and connector tests use `tempfile::tempdir()` paths passed explicitly (`native.rs:598-761`, `cline.rs:464,494`) rather than overriding `HOME`.

## Notable patterns

- **Blocking-thread file watcher with cooperative shutdown.** `native.rs:153-256` runs the `notify` event loop on a `spawn_blocking` thread, polls `tx.is_closed()` every 50 ms, debounces per path for 200 ms (`native.rs:145`), and dedupes by `(path, messages.len())`. Initialisation errors are propagated over a oneshot rather than through the dropped `JoinHandle`. Parsing is done synchronously on the same thread (`parse_session_file_sync`, `native.rs:454`) to avoid nesting a runtime spawn inside a blocking thread.
- **Guard hygiene.** Every `tokio::sync::RwLock` guard in `service.rs` is scoped so that no `.await` executes while it is held; `cluster_by_concepts` drops the guard explicitly (`service.rs:514`) before heavy computation.
- **Check-then-act auto-import** (`service.rs:66-102`): correctness relies on idempotent cache insertion rather than on the flag being an exclusive gate.
- **`Clone` that forgets state** (`service.rs:397`): semantically a "new service with the same auto-import setting", not a shallow clone.
- **Per-process regex compilation** (`redaction.rs:55-70`): one `OnceLock`, initialised on first redaction call; failures downgrade to a warning and the pattern is skipped rather than aborting.
- **Feature gating of audit coverage.** Four connector files and the whole `enrichment/` module do not participate in the default `--lib` expansion.

## Open questions

1. Whether `terraphim_types::Scored::score()` can return `NaN` (e.g. BM25 over an empty corpus or zero-length document), which would make the `search.rs:135,197` comparator inconsistent. Not determinable from this crate.
2. Whether any downstream caller (`terraphim_agent`, `terraphim_mcp_server`) shares a `SessionService` across tasks via `Arc` and relies on `auto_import_attempted` as an exclusive gate; the window at `service.rs:72-98` allows duplicate first imports.
3. Whether the `RoleGraph` writer that pairs with the read guard at `enricher.rs:119` ever takes the write lock while an enrichment is in flight, and for how long; that determines read-guard hold time but lives outside this crate.
4. The supplementary expansion (`--tests --features enrichment`) had not completed at write time (blocked on the shared cargo build-directory lock); its `unsafe` count is expected to add `Pin::new_unchecked` sites from `tokio::pin!`/`tokio::select!` at `native.rs:791-800` and further `vec!`/`TrivialClone` derive output, none hand-written.

## Addendum: supplementary expansion status

Supplementary expansion SKIPPED by lead: source read of the enrichment module, feature-gated connectors and cfg(test) code found no unsafe; lib expansion limits macro-generated unsafe to std vec! and derive marker impls.
