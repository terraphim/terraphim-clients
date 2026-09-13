# Phase 2 findings — bucket 11: panic-safety

**Verdict: SURFACE_PRESENT, NO_MEMORY_UB** (static sweep, 2026-09-11 23:46 BST, sweeper p2-panic-safety).

Scope: lock poisoning, `catch_unwind`, `process::exit` skipping `Drop`, panic inside `Drop`, panic unwinding across an FFI boundary via dependency callbacks, and str-slicing panics. The workspace has no `unsafe` outside the six test-only `env::set_var`/`remove_var` blocks catalogued in phase 1 section A, and no FFI, raw pointers or `unsafe impl` (phase 1 archetype: pure-safe-with-env-unsafe), so a panic cannot cause memory UB directly. Every item below is therefore classified against the bucket-11 definitions (1)-(5) in the sweeper brief, and the strongest severity reached is SUSPICIOUS. No MUST-BE-UB or LIKELY-UB.

Toolchain facts that bound the analysis: `rustc 1.97.1`; `[profile.release] panic = "unwind"` is set explicitly at `Cargo.toml:105`; a panic that reaches an `extern "C"` frame aborts (`panic_cannot_unwind`) on this toolchain rather than unwinding into C.

## Evidence

### Phase 1 rows consumed

Filtered from `phase1_unsafe_surface_inventory.md` and `phase1_notes/{A,B,C,D,E,F,G,H,I,J}.md` for `11`, `panic`, `poison`, `catch_unwind`, `process::exit`, `impl Drop`, `file-lifecycle`:

| Source | Row | Disposition here |
|---|---|---|
| inventory §E:25-27, E.md:76-110 | `fs::copy` truncate-in-place at `lib.rs:1536`, `rollback.rs:310`; Windows remove+rename at `lib.rs:1088-1094` | F-001, F-002 (E's open question "does any caller pass the running exe?" is answered: yes) |
| inventory §H:99, H.md:52-55 | `catch_unwind` at `terraphim_lsp/src/kg_analysis.rs:243` (test only) | non-finding |
| inventory §H:113, H.md:117 | `terraphim_cli/src/main.rs:437,452,472` `process::exit(1)` | non-finding (section "process::exit") |
| B.md:34, 62, 73 | `kg_validation.rs:28` global std `RwLock` with I/O under guard, `.ok()?` fail-open on poison | F-004 (CONTRACTUAL) |
| B.md:26, 62 | `store.rs:140` std `RwLock<RoleGraph>`, `if let Ok` fail-open | non-finding (readers cannot poison) |
| B.md:63 | tokio locks release on panic, `registry.rs:226-234` clears three maps in separate scopes | F-005 (CONTRACTUAL) |
| B.md:35, 61 | `logging.rs:154,272` `.lock().unwrap()` in `#[cfg(test)]` | non-finding |
| C.md:24, 92 | `extract_functionality_validation.rs:45,102` `static Mutex<Option<Child>>` | non-finding |
| G.md:41 | `test_tools_list.rs:44-51,89` `Arc<Mutex<String>>` with `.expect` | non-finding |
| D.md:55, 67 | `native.rs:161-256` detached `spawn_blocking` hosting a `notify` watcher | F-006 (FFI callback, CONTRACTUAL) |
| F.md:241-253, I.md:80-89, inventory F:195-196 | str-slicing at byte offsets | F-003 (new sites) plus cross-ref |
| J.md:16 | RUSTSEC-2026-0253 `lru 0.18.0` (UAF only if a value's `Drop` panics) | non-finding |
| inventory §A:42, C.md | `impl Drop for EnvVarGuard`/`TestEnv` in registry crate `terraphim_test_utils-1.20.3` | non-finding (double-panic angle only) |

### Enumeration (rg, workspace `crates/`, run 2026-09-11)

```
rg -n 'catch_unwind|AssertUnwindSafe|resume_unwind|process::exit|std::process::abort|impl Drop|\.lock\(\)\.unwrap\(\)|\.lock\(\)\.expect|\.read\(\)\.unwrap|\.write\(\)\.unwrap|into_inner\(\)|PoisonError|panic::set_hook|take_hook|std::sync::Mutex|std::sync::RwLock' --type rust crates
```

Counts:

| Pattern | Hits | Where |
|---|---|---|
| `catch_unwind` | 1 | `terraphim_lsp/src/kg_analysis.rs:243` (`#[cfg(test)]`) |
| `AssertUnwindSafe` / `resume_unwind` / `set_hook` / `take_hook` / `process::abort` | 0 | |
| `impl Drop` | 0 | none in `crates/` |
| `PoisonError` / lock `into_inner()` | 0 | the three `into_inner()` hits are `Cursor`/`GzEncoder` byte buffers in `terraphim_update` tests and `signature.rs:490` (test module), not locks |
| `std::sync::Mutex` (non-test) | 0 | production code uses `tokio::sync` only; std `Mutex` appears in `logging.rs:131` (`#[cfg(test)]`), `test_tools_list.rs:44`, `extract_functionality_validation.rs:45`, `integration_test.rs:411` |
| `std::sync::RwLock` (non-test) | 2 | `terraphim_agent/src/kg_validation.rs:28` (static), `terraphim_agent/src/shared_learning/store.rs:140` (field, feature `shared-learning`) |
| `.lock().unwrap()` / `.lock().expect` | 6 | `logging.rs:154,272`; `test_tools_list.rs:50,51,89`; `extract_functionality_validation.rs:102`; `integration_test.rs:428,443`. All test code. |
| `std::process::exit` | 45 | `terraphim_cli/src/main.rs` x3; `terraphim_agent/src/{main.rs x17, server_command.rs x15, learn_command.rs x5, robot_dispatch.rs:44}` |

### FFI-callback enumeration

```
rg -n 'create_scalar_function|create_aggregate_function|create_collation|busy_handler|progress_handler|authorizer|update_hook|commit_hook|rollback_hook|git2::|RemoteCallbacks|credentials\(|transfer_progress|recommended_watcher|RecommendedWatcher|PollWatcher|Watcher::new' --type rust crates ~/.cargo/registry/src/*/terraphim_{service,middleware,file_search,persistence,config,automata,rolegraph}-1.2*/src
```

| Library | Reached by | Workspace closure crossing FFI? |
|---|---|---|
| `git2 0.20.4` | only via `fff-search 0.8.4` -> `terraphim_file_search 1.21.0` -> `terraphim_middleware`/`terraphim_mcp_server` | No. `fff-search/src/git.rs:56,81` calls `repo.statuses()` and `Repository::open` only; no `foreach`/`RemoteCallbacks`/`walk`. git2 additionally wraps every callback it does expose in `panic::wrap` (`git2-0.20.4/src/panic.rs:8-13`, `catch_unwind`). |
| `rusqlite 0.32.1` / `libsqlite3-sys 0.30.1` | `terraphim_sessions` (feature `cursor-connector`, `features = ["bundled"]`), `terraphim-session-analyzer` (feature `connectors`), `terraphim_persistence 1.20.2` (`settings.rs:110` `Connection::open` only) | No. Neither the `functions` nor the `hooks` feature is enabled, so `create_scalar_function`/`update_hook` do not exist in the build. Workspace uses `Connection`, `prepare`, `params!`, `query_map`; `query_map` closures are invoked from Rust after `sqlite3_step` returns, not from inside libsqlite3. |
| `notify 8.2.0` / `fsevent-sys 4.1.0` (macOS backend) | `terraphim_sessions/src/connector/native.rs:163` `recommended_watcher(move \|res\| { let _ = watcher_tx.send(res); })` | The closure runs inside `extern "C" fn callback` (`notify-8.2.0/src/fsevent.rs:506-577`, via `event_handler.handle_event(Ok(ev))` at :574). The workspace closure body is `std::sync::mpsc::Sender::send`, which returns `Err` on disconnect and has no panic path. See F-006. |
| `notify_debouncer_full` via `terraphim_file_search::watcher::KgWatcher` | not referenced anywhere in `crates/` or in `terraphim_{middleware,service}` src (only its own unit tests) | Unreachable from this workspace. The handler closure also runs on the debouncer's own std thread, not inside the FSEvents callback, and `KgPathScorer::update_thesaurus` (`kg_scorer.rs:36`) writes through a `parking_lot::RwLock` (no poisoning). |
| `aws-lc-sys 0.41.0` | `rustls` -> `reqwest` -> `opendal` -> `terraphim_config` | No workspace callbacks are registered with aws-lc. |

### ast-grep script

```
$ bash ~/.claude/skills/rust-undefined-behavior-exorcist/scripts/ast-grep-ub-patterns.sh /Users/alex/projects/terraphim/terraphim-clients panic-safety
No patterns found.
```

Exit status 0. The script ships no `panic-safety` pattern set (the `patterns/` directory has no rule tagged for this bucket), so the enumeration above is rg-only.

## Findings

## F-001 Rollback truncates the running executable in place

- **File:line:** `crates/terraphim_update/src/lib.rs:1536` (`rollback`), `crates/terraphim_update/src/rollback.rs:310` (`BackupManager::rollback_to_version`); callers `crates/terraphim_cli/src/main.rs:939-944` and `crates/terraphim_agent/src/repl/handler.rs:1849-1852`.
- **Kind:** file-lifecycle, definition (3): a panic or kill mid-operation leaves an on-disk invariant torn; no `Drop`-based cleanup exists to skip.
- **Bucket(s):** 11 panic-safety (file-lifecycle sub-bucket), cross-listed with phase 1 §E.
- **Severity:** SUSPICIOUS (process-level and on-disk hazard; explicitly not Rust memory UB).
- **Static evidence:**
  - Both production callers pass `std::env::current_exe()` as `target_path`: `terraphim_cli/src/main.rs:941-944` (`let current_exe = std::env::current_exe()?; ... terraphim_update::rollback(&backup_path, &current_exe)?;`) and `repl/handler.rs:1849-1852` (`let target_path = std::env::current_exe().unwrap_or_else(...); manager.rollback_to_version(&version, &target_path)`). This closes phase 1 §E's open question with "yes".
  - Neither `rollback` nor `rollback_to_version` compares `target_path` with the running executable, and neither stages to a sibling temp file and renames. `std::fs::copy` opens the destination for writing with truncation.
  - Contrast: the forward-update paths (`promote_staged_binaries` `lib.rs:1061-1077`, `self-replace 1.5.0` `unix.rs:13-47`) both rename over the executable and never truncate it.
  - On Linux, opening a file that is currently being executed for write returns `ETXTBSY`, so rollback can never succeed there; the error is surfaced through `?` (no torn state, just a permanently failing feature).
  - On macOS the behaviour is not asserted here from memory: the discriminating question is whether XNU rejects the open, or lets the truncation through and later kills the process when a modified page is faulted in (code-signature invalidation). Either way, an interrupted copy (SIGKILL, power loss, panic in the copy loop) leaves `target_path` zero-length or partially written while `backup_path` is intact, and the next `terraphim-cli`/`terraphim-agent` invocation is a non-executable file.
  - `rollback_to_version` verifies the backup's SHA-256 before copying (`rollback.rs:301`) but never verifies the target after copying.
- **Draft experiment:** (a) Build a small binary that sleeps 30 s; copy it to a temp path and run it; from a second process call `std::fs::copy(other_file, temp_path)` and record the `io::Error` kind (expect `ETXTBSY` on Linux) or, on macOS, whether the copy succeeds and whether the sleeping process is later killed (record signal). (b) Under `TempDir`, call `terraphim_update::rollback` with a backup of N MiB and kill the process mid-copy (e.g. `SIGKILL` from a watchdog thread after the first 4 KiB is observed); assert the target is truncated and the backup untouched. (c) Proposed fix for phase 4: copy the backup to `target.with_extension("rollback-tmp")` in the same directory, `fsync`, then `rename` over the target, mirroring `promote_staged_binaries`.
- **Cross-refs:** phase 1 §E rows `lib.rs:1536`, `rollback.rs:310`; E.md "Rollback paths: truncate-in-place"; F-002.

## F-002 Windows promotion window with no binary present and orphaned staging dir

- **File:line:** `crates/terraphim_update/src/lib.rs:1083-1096` (`promote_staged_binaries`, `#[cfg(not(unix))]`).
- **Kind:** file-lifecycle, definition (3).
- **Bucket(s):** 11 panic-safety (file-lifecycle).
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE (not compiled on this host; functional not memory hazard).
- **Static evidence:** `let _ = std::fs::remove_file(&dst); std::fs::rename(&staged, &dst).or_else(|_| { std::fs::copy(&staged, &dst)?; std::fs::remove_file(&staged) })?;`. A kill between `remove_file` and `rename` leaves no file at `dst`. The staged copy survives in the `tempfile::tempdir_in(install_dir)` directory (`lib.rs:1030`) because the `TempDir` drop at the end of `install_verified_archive` (`lib.rs:1014-1055`) is skipped on kill, so recovery is possible by hand but not automatic. `remove_file` of the currently running Windows executable fails with a sharing violation and the error is discarded (`let _`), after which `rename` onto an existing file fails and the `copy` fallback hits the same sharing violation: the update fails cleanly rather than tearing anything.
- **Draft experiment:** Windows-only; out of scope for the macOS host. Record as a phase 4 note: use `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` (via `self_replace::self_replace`, already a dependency through `self_update`) instead of remove-then-rename.
- **Cross-refs:** phase 1 §E row `lib.rs:1088-1094`; E.md "Windows promotion".

## F-003 Byte-offset string slicing with byte-length guards only

- **File:line:** non-test sites not already catalogued by phase 1 F/I:
  - `crates/terraphim_agent/src/repl/handler.rs:994` (`&paragraph[..97]`, guard `paragraph.len() > 100`), `:1995` (`&t[..40]`), `:2051` (`&t[..50]`), `:2098` (`&t[..50]`), `:2172` (`&msg.content[..80]`), `:2226` (`&t[..40]`), `:2321` (`&t[..40]`), `:2585` (`&first_msg.content[..100]`)
  - `crates/terraphim_agent/src/learnings/procedure.rs:427` (`&first[..60]`, guard `first.len() > 60`)
  - `crates/terraphim_agent/src/main.rs:2774` (`&local.command[..60]`, guard `local.command.len() > 60`)
  - `crates/terraphim_sessions/src/model.rs:332` (`&m.content[..100]`, guard `m.content.len() > 100`)
  - `crates/terraphim_agent/src/repl/handler.rs:2241`, `:2329` (`&session.id[..8]`, no guard)
- **Kind:** str-slicing panic on a non-char-boundary index; safe-Rust panic, never UB.
- **Bucket(s):** 11 panic-safety (str-slicing sub-bucket).
- **Severity:** SUSPICIOUS for the content/command sites (reachable from imported session and learning data), CONTRACTUAL-BUT-DEFENSIBLE for the id sites.
- **Static evidence:**
  - Every guard is `len() > N` on the byte length (confirmed for all eight `handler.rs` sites with `rg -n -B2 '\[\.\.(40|50)\]'` plus the context reads at `:994`, `:2172`, `:2585`); none uses `is_char_boundary`, `char_indices`, or `floor_char_boundary`. Any multi-byte code point straddling byte N panics with `byte index N is not a char boundary`.
  - Reachability evidence from the repository itself: `crates/terraphim_agent/src/cli_helpers.rs:52` records a prior hit of exactly this shape (`&s[..120]` landed inside a typographic quote, 3 bytes e2 80 9c) and was fixed there but not at the sites above.
  - Inputs: `msg.content`/`first_msg.content` come from imported session JSONL/SQLite (user prose, arbitrary Unicode); `local.command` from captured shell commands; `first` from procedure step text; `paragraph` from KG search results.
  - `&session.id[..8]`: every connector enabled by `terraphim_agent` (`Cargo.toml:99` enables `aider-connector`, `cursor-connector`, plus the default Claude native/codex/opencode paths) prefixes the id with an ASCII source tag (`cursor:`, `codex:`, `claude-code-native:`, `aider-<project>-`, `opencode:prompt-history`), so the first 8 bytes are ASCII and at least 8 long. `cline.rs:379` copies `item.id` verbatim and could be shorter, but `cline-connector` is not enabled by the agent build. Defensible today, brittle under a feature change.
  - `handler.rs:2369-2374` (`&date[..10]`, `[..4]`, `[5..7]`) are safe: `date` is `strftime("%Y-%m-%d")` output, fixed-width ASCII.
- **Draft experiment:** unit test per site with a string whose byte N-1..N+1 spans a multi-byte code point, e.g. `"a".repeat(99) + "é"` for the `[..100]` sites and `"x".repeat(59) + "\u{201c}"` for the `[..60]` sites; assert no panic after replacing the slice with `s.char_indices().nth(N).map_or(s, |(i, _)| &s[..i])` or `s.floor_char_boundary(N)`.
- **Cross-refs:** phase 1 F.md:241-253 (`hybrid_searcher.rs:150-154`, offsets from `terraphim_automata`), I.md:82-89 (`reporter.rs:419,427`, `analyzer.rs:686`, `cursor.rs:323`), inventory H:146 (`scanner.rs:62`, boundary-aligned by construction). Same class as `cli_helpers.rs:52`'s fixed bug.

## F-004 Global `KG_CACHE` std `RwLock` with filesystem I/O under the guard

- **File:line:** `crates/terraphim_agent/src/kg_validation.rs:28` (static), `:83-116` (`get_thesaurus_with_auto_rebuild`).
- **Kind:** definition (1): panic while holding a std lock poisons it; question is whether a torn invariant is observable.
- **Bucket(s):** 11 panic-safety (poison).
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE.
- **Static evidence:**
  - Read guard (`:87`) is held across `compute_kg_source_hash` (directory walk); write guard (`:97`) is held across `compute_kg_source_hash`, `find_kg_dir` and `build_kg_thesaurus_with_hash` (I/O plus automaton build).
  - No torn value is possible: the only mutation is the single assignment `*guard = Some(cached)` at `:115` after `CachedThesaurus` is fully constructed; a panic before it leaves the previous value intact.
  - Panic surface under the write guard: `build_kg_thesaurus_from_dir` (`learnings/capture.rs`) and `find_kg_dir` (`capture.rs:855-868`) contain no `unwrap`, `expect`, indexing or `panic!` (rg over the function bodies: only `.map(..).unwrap_or(false)` at the `.md` extension check). `compute_kg_source_hash` returns `Result`. The remaining panic source is allocation failure.
  - Poison handling is `.ok()?` on both paths (`:87`, `:97`): a poisoned lock makes validation return empty for the rest of the process, silently. The sole caller is `main.rs:2232` inside the one-shot `handle_hook_command`, so "rest of the process" is one hook invocation, and a panic under the guard propagates out of the runtime and ends that process anyway.
- **Draft experiment:** none needed for UB. Optional robustness test: install a `panic::set_hook` that records, poison the lock deliberately via `catch_unwind` around a closure that panics while holding `KG_CACHE.write()`, then assert `validate_command_against_kg` returns `empty()` rather than panicking.
- **Cross-refs:** B.md:34, 62, 73; p2-data-races sweeper for the blocking-under-async-worker aspect.

## F-005 tokio locks: partially applied multi-lock updates survive a task panic

- **File:line:** `crates/terraphim_agent/src/commands/registry.rs:95-134` (`register_command`), `:223-238` (`reload`).
- **Kind:** definition (2) analogue for non-poisoning locks: a panic between sequential single-lock scopes leaves `commands`/`aliases`/`categories` mutually inconsistent, and the tokio task boundary's `catch_unwind` keeps the `Arc` state alive for other tasks.
- **Bucket(s):** 11 panic-safety.
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE.
- **Static evidence:** `reload` clears the three maps in three separate write scopes (`:226-234`) and then calls `load_all_commands`; `register_command` inserts into `commands` (`:111`), then `aliases` (`:117-121`), then `categories` (`:125-131`). A panic in `load_all_commands` leaves an empty registry; a panic between the scopes leaves an alias without a command or vice versa. Readers tolerate every torn shape: `resolve_command` (`:141-156`) returns `None` when an alias points at a missing command (`commands.get(resolved_name).cloned()`), `get_command` returns `Option`, and no reader indexes or `unwrap`s. tokio `RwLock` guards release on unwind; nothing poisons.
- **Draft experiment:** none for UB. Optional: a test that panics inside a fake command loader after `commands.clear()` and asserts `resolve_command` returns `None` for every previously registered alias and `list_commands` is empty.
- **Cross-refs:** B.md:27, 63; the four-party lock-order note at B.md:27 belongs to p2-data-races.

## F-006 `notify` FSEvents callback hosts a workspace closure inside an `extern "C"` frame

- **File:line:** `crates/terraphim_sessions/src/connector/native.rs:163-165`.
- **Kind:** definition (5): a workspace closure invoked from C via `extern "C" fn callback` (`notify-8.2.0/src/fsevent.rs:506-577`).
- **Bucket(s):** 11 panic-safety (FFI callback), cross-listed with ffi-callback.
- **Severity:** CONTRACTUAL-BUT-DEFENSIBLE.
- **Static evidence:**
  - The closure is `move |res| { let _ = watcher_tx.send(res); }`. `std::sync::mpsc::Sender::send` returns `Err(SendError)` when the receiver is gone and has no panic path; the closure captures nothing else.
  - The `extern "C"` frame is `notify`'s, declared `extern "C"` not `extern "C-unwind"`; on rustc 1.97.1 a panic reaching it aborts the process (`panic_cannot_unwind`), which is defined behaviour, not UB.
  - The panic sources that do exist in that frame are `notify`'s own: `.expect("Invalid UTF8 string.")` (`fsevent.rs:541`), `panic!("Unable to decode StreamFlags")` (`:546`), and `event_handler.lock().expect("lock not to be poisoned")` (`:573`). A non-UTF-8 path under `~/.claude/projects` would therefore abort the watching process. That is a dependency defect outside the workspace's control and is reported as a note, not a workspace finding.
  - On Linux the backend is inotify with no C callback.
- **Draft experiment:** macOS only: create a file whose name is invalid UTF-8 (`OsStr::from_bytes(b"\xff.jsonl")`) inside a watched directory during `watch_at` and observe whether the test binary aborts (expected, given `fsevent.rs:539`). Purpose is to document the dependency behaviour for phase 4's decision on whether to pin or patch `notify`.
- **Cross-refs:** D.md:55, 67; phase2_findings_ffi-callback.md (verdict NO_SURFACE, based on zero `extern "C" fn` in `crates/`; this row is the only candidate, the `extern "C"` frame lives in the dependency, and the verdict stands).

## Non-findings

| Site | Pattern | Reason it is not a bucket-11 finding |
|---|---|---|
| `terraphim_lsp/src/kg_analysis.rs:243-245` | `catch_unwind` | `#[cfg(test)]` only; closure captures `&Thesaurus` (String, AHashMap, Option<String>; no `UnsafeCell`) and a literal; no `AssertUnwindSafe`, so `UnwindSafe` is compiler-checked; result consumed via `is_ok()`. (H.md:52-55) |
| `terraphim_agent/src/shared_learning/store.rs:140,575,667` | std `RwLock<RoleGraph>` | Only read guards are ever taken (`graph_lock.read()` at `:667`, inside `if let Ok`); the sole writer is `set_role_graph(&mut self)` which constructs a new lock. A std `RwLock` is poisoned only when a writer panics, so no poison path exists. |
| `terraphim_agent/src/logging.rs:154,272` | `.lock().unwrap()` on test `Mutex<Vec<String>>` | `#[cfg(test)]`. `CapturingLogger` is a local value wrapped in `FilteredLogger`; it is never installed via `log::set_boxed_logger`, so a poison caused by a failing `assert_eq!` while the guard at `:272` is held stays inside the already-failing test. |
| `terraphim_mcp_server/tests/test_tools_list.rs:44-51,89` | `Arc<Mutex<String>>` `.expect` | Guard held only across `String::push_str`/`push` (allocation); the reader loop is `map_while(Result::ok)` so it cannot panic on I/O; thread is joined before the final `expect`. |
| `terraphim_agent/tests/extract_functionality_validation.rs:45,102` | `static Mutex<Option<Child>>` `.lock().unwrap()` | Single assignment of `Some(child)` under the guard; infallible; all four tests `#[serial]`. |
| `terraphim_update/tests/integration_test.rs:411,428,443` | `Arc<Mutex<i32>>` `.lock().unwrap()` | Test counter; `*count += 1` under guard cannot panic (i32 increments bounded by 5 threads). |
| `terraphim_update/src/signature.rs:490`, `tests/signature_test.rs:33`, `tests/r2_update.rs:87,289` | `into_inner()` | `Cursor<Vec<u8>>` / `tar::Builder` byte buffers, not lock recovery. |
| `crates/**` | `impl Drop` | Zero in workspace source. The registry crate `terraphim_test_utils-1.20.3` has `Drop for EnvVarGuard` (`lib.rs:106-113`) and `Drop for TestEnv` (`:175-184`) that call `set_var`/`remove_var`; `set_var` panics only on an empty key, a key containing `=` or NUL, or a value containing NUL, and every key at the call sites (`tui_service_tests.rs:34-61`, mcp e2e tests) is an ASCII literal. No double-panic path. Env-race aspect belongs to section A / p2-data-races. |
| `terraphim_cli/src/main.rs:437,452,472`; `terraphim_agent/src/{main.rs,server_command.rs,learn_command.rs,robot_dispatch.rs}` (45 sites) | `std::process::exit` | All 45 sites traced with `rg -n -B3 'process::exit'` over the four files: each is an `eprintln!`/`print_json_output` followed by `exit` in an `Err` arm or at a decision point (`fail_on_empty`, `results.is_empty()`, "not available in server mode", guard `Block`), reached after the awaited handler has returned (`cli/main.rs:452,472` after `match result`; `agent/main.rs:770,786` and `server_command.rs:497,513` in the `Err` arm after `check_for_updates`/`update` completed; `learn_command.rs` sites before or after a completed `store.*?` call; `robot_dispatch.rs:44` is the shared error-exit helper). The R2 install (`lib.rs:382`) and `promote_staged_binaries` run inside an awaited `spawn_blocking`, so no exit can interleave with a staged rename. Persistence is SQLite (`terraphim_persistence/settings.rs:110`, journalled) plus opendal whole-file profile writes and whole-file `fs::write` of markdown (`capture.rs:1095,1169,1317`), none of which relies on `Drop` for on-disk consistency. Skipped `Drop`s are the tokio runtime and `TerraphimService`; `rg 'TempDir\|tempdir(\|NamedTempFile'` over `terraphim_agent/src` and `terraphim_cli/src` hits only `#[cfg(test)]` code in `kg_validation.rs:289-398`, so no `TempDir` is live at any exit site. |
| `terraphim_lsp/src/server.rs:51-73`; `terraphim_sessions/src/connector/native.rs:161` | detached `tokio::spawn` / `spawn_blocking` | Panics are caught at the task boundary by tokio; captured state is `Arc<tokio::Mutex<..>>` (no poisoning) with the `docs` guard dropped at `server.rs:57` before the scan, or per-thread locals in the watcher loop. Silent-panic observability is a logging concern (H.md:251), not UB. |
| `terraphim_agent/src/repl/handler.rs:2369-2374` | `&date[..10]`, `[..4]`, `[5..7]` | `date` is `strftime("%Y-%m-%d")` output: fixed-width ASCII, boundaries guaranteed. |
| `terraphim_agent/src/learnings/capture.rs:1817` | `&full_id[..8]` | `#[cfg(test)]`; id is a generated ASCII identifier. |
| `terraphim_agent/src/{shell_dispatch.rs:120, commands/modes/*.rs, repl/commands.rs, main.rs:1167}` | `parts[1..]`, `tokens[1..]`, `all_terms[1..]` | Slices of `Vec<&str>`/`Vec<String>`, not `str`; index 1 is guarded by the preceding `is_empty`/`len()` checks on the token vector (element slicing has no char-boundary hazard). |
| `terraphim_sessions/src/connector/cline.rs:346-347` | `.expect("Invalid timestamp in taskHistory")` | Panic on user data, feature `cline-connector`, not enabled by `terraphim_agent` (`Cargo.toml:99`). Not a lock, FFI, or `Drop` context; ordinary panic. Recorded by D.md:68. |
| `hybrid_searcher.rs:207-212,430-434`; `sessions/search.rs:135,197`; `session-analyzer kg/search.rs:89, main.rs:864` | `partial_cmp(..).unwrap_or(Equal)` in `sort_by` | Total-order contract, owned by the library-invariant/validity sweep; on Rust 1.81+ a violation panics rather than causing UB. |
| `git2 0.20.4` via `fff-search` | callback across FFI | No workspace or first-party closure is passed to git2; `fff-search` uses `repo.statuses()`/`Repository::open` only; git2 wraps its callbacks in `catch_unwind` (`panic.rs:8-13`) regardless. |
| `rusqlite 0.32.1` (`bundled` only) | callback across FFI | `functions`/`hooks` features disabled, so no scalar-function, collation, authorizer, busy or progress handler API is compiled; `query_map` closures execute on the Rust side. |
| `terraphim_file_search::watcher::KgWatcher` | debouncer handler closure | Not constructed anywhere in `crates/` or in `terraphim_middleware`/`terraphim_service` src; runs on the debouncer's own std thread; `update_thesaurus` uses `parking_lot::RwLock` (no poison). |
| `aws-lc-sys 0.41.0` | callback across FFI | No workspace callbacks registered. |
| `lru 0.18.0` via `ratatui-core 0.1.2` (RUSTSEC-2026-0253) | UAF if a value's `Drop` panics inside `LruCache::pop` | Cached values are ratatui text-layout structs with no custom `Drop`; the workspace has no `impl Drop`; advisory tracked in J.md:16 for a version bump to 0.18.4. |
| `terraphim_agent/src/shared_learning/store.rs:608-612` | `block_in_place` + `Handle::current().block_on` | Panics on the wrong runtime flavour (B.md:51), which is runtime-nesting, not panic-safety; cross-ref p2-data-races / B open question 1. |
| `terraphim_update/src/lib.rs:1407-1410` | `Runtime::new` + `block_on` in closure | Same class; only caller is `spawn_blocking` (E.md:66-69). |
| `terraphim_agent/src/learnings/capture.rs:733` (`KG_THESAURUS`), `repl/handler.rs:1909` (`SESSION_SERVICE`), `terraphim_sessions/src/redaction.rs:55` | `OnceLock` statics | A panic inside `get_or_init` leaves the cell uninitialised for the next caller to retry, never torn or poisoned; all three init closures are infallible (`Option`-returning thesaurus builder, `SessionService::new()`, `filter_map` regex compile with `warn!`). B.md:65, D.md:66. |
| `Cargo.toml:104-105` | `[profile.release] panic = "unwind"` | Explicit; no `panic = "abort"` anywhere, so unwinding semantics assumed above hold for release builds too. |

## Summary for phase 3

- No panic-safety route to memory UB exists: no `unsafe` beyond section A's test-only `set_var` blocks, zero `impl Drop`, zero production std `Mutex`, two std `RwLock`s with no torn-state exposure, no workspace closure with a panic path inside an FFI frame.
- The one item worth an experiment is F-001 (rollback truncating the running executable), because it is a real durability defect in shipped CLI/REPL commands and phase 1 §E had left its reachability open.
- F-003's char-boundary slices are the most likely user-visible panics; they are cheap to fix and the repo already contains the pattern for the fix at `cli_helpers.rs:52`.
