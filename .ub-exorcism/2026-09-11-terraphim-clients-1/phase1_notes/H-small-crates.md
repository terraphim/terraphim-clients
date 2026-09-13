# Phase 1 notes, section H: small crates

Run: 2026-09-11-terraphim-clients-1. Mapper: p1-H. Scope: `crates/terraphim_lsp`, `crates/terraphim_cli`, `crates/terraphim_hooks`, `crates/terraphim_command_runtime`, `crates/terraphim_negative_contribution` (src and tests of each).

Sweep method: `rg -n -t rust` over the five crate roots for `unsafe|SAFETY`, `Arc<|Mutex|RwLock|Atomic|OnceLock|LazyLock|static |thread::spawn|tokio::spawn|spawn_blocking`, `block_on|Runtime::new|Handle::current`, `impl Drop`, `env::var|set_var|remove_var|env!(`, `catch_unwind|AssertUnwindSafe|resume_unwind|set_hook`, `impl (Hash|PartialEq|Eq|Ord|PartialOrd|Borrow|Iterator|Send|Sync) for`, and `unwrap()|expect(|panic!|unreachable!|todo!|unimplemented!`. Registry sources consulted read-only for types the crates hold: `terraphim_types-1.22.1`, `terraphim_config-1.20.2`, `terraphim_rolegraph-1.20.2`, `terraphim_automata-1.21.0`, `terraphim_service-1.21.1`, `tower-lsp-0.20.0`.

Headline: zero `unsafe` tokens and zero `SAFETY` comments in the source of all five crates (src and tests). No FFI, no raw pointers, no `impl Drop`, no manual `Send`/`Sync`, no `static` items with interior mutability, no `set_var`/`remove_var`. The audit surface in this section is entirely concurrency (tokio mutexes in two crates) and panic-safety.

Macro expansion: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives. The expand job (bfy5fj0hy) was killed by the lead to release the shared build-directory lock.

## terraphim_lsp

### Purpose
tower-lsp language server. Full-document sync; on open/change it debounces and runs the Explicit Deferral Marker scanner from `terraphim_negative_contribution` over the document, publishing diagnostics. Also exposes `kg_analysis::analyse_kg_document` (Aho-Corasick term matching for hover). Binary `terraphim-lsp` is feature-gated (`required-features = ["terraphim-lsp"]`, Cargo.toml:16-19); the library is always built.

Files: `src/server.rs` (119 lines), `src/kg_analysis.rs` (251), `src/diagnostic.rs` (92), `src/config.rs` (26), `src/lib.rs` (16), `src/bin/terraphim-lsp.rs` (11). No `tests/` directory.

### Unsafe-surface tally
- Source `unsafe` blocks/fns/impls/traits: 0.
- `SAFETY` comments: 0.
- Raw pointers, FFI, `transmute`, `MaybeUninit`: none.
- Macro-generated `unsafe` in expansion: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives.

### Concurrency surface
Shared state on `TerraphimLspServer` (`src/server.rs:14-20`):
- `client: Client` (tower-lsp, cloned into the spawned task at line 41).
- `scanner: NegativeContributionScanner` (owns a `Thesaurus`; `Clone`; cloned per schedule at line 40).
- `config: LspConfig` (plain data; only `debounce_ms` is read, line 42).
- `documents: Arc<tokio::sync::Mutex<HashMap<Url, String>>>` (line 18).
- `pending: Arc<tokio::sync::Mutex<HashMap<Url, tokio::task::AbortHandle>>>` (line 19).

Which handlers touch which state:
- `did_open` (97-104): locks `documents` (insert, guard dropped at end of statement), then `schedule_diagnostics`.
- `did_change` (106-112): locks `documents` (insert of `content_changes[0].text`, full-sync so first change is the whole document), then `schedule_diagnostics`.
- `did_close` (114-118): locks `documents` (remove), then publishes an empty diagnostics vector. Does not touch `pending`: neither aborts nor removes the URI's pending handle.
- `schedule_diagnostics` (37-74): locks `pending` at line 46, aborts any previous handle for the URI (47-49), `tokio::spawn`s the debounce task (51-71), inserts the new abort handle (73). The `pending` guard is held across the `tokio::spawn` call but across no `.await`; the spawned task never touches `pending`.
- Spawned task (51-71): sleeps `debounce_ms`, locks `documents` briefly (54-57, guard dropped by block scope), runs the synchronous `scanner.scan_file` (62) on the runtime worker, then awaits `client.publish_diagnostics` (68-70).
- `initialize`, `initialized`, `shutdown` (79-95): touch no state.

Dispatch model: tower-lsp 0.20 runs handlers with `DEFAULT_MAX_CONCURRENCY = 4` (`tower-lsp-0.20.0/src/transport.rs:22`, adjustable via `concurrency_level`, transport.rs:96). Consequently two `did_change` notifications for the same URI can be in flight concurrently. Because the spawned task re-reads `documents` at fire time (54-57) rather than capturing the text, the diagnostics published correspond to whichever text was last written into `documents`, independent of which notification's task survived the abort race. Whether tower-lsp preserves per-URI ordering of notifications is an open question below.

Task lifecycle facts:
- `pending` entries are inserted on every schedule but never removed on task completion; the map holds one stale `AbortHandle` per URI ever opened (bounded by distinct URIs, overwritten per URI).
- The `JoinHandle` returned by `tokio::spawn` is dropped at line 73 after `abort_handle()` is taken; the task is detached. Nothing observes its completion or its panic.
- A debounce task scheduled before `did_close` is not aborted by `did_close`, so it can fire after the document was removed; it then takes the `None` branch (line 65) and publishes an empty diagnostics vector for the closed URI.
- No `thread::spawn`, `spawn_blocking`, `block_on`, `Runtime::new`, `Handle::current`, atomics, `OnceLock`, `LazyLock`, or `static` with interior mutability.
- `#[tokio::main]` only in the feature-gated binary (`src/bin/terraphim-lsp.rs:257`).

Send/Sync: `LanguageServer` requires `Send + Sync + 'static`; satisfied structurally (Client, Thesaurus = String + AHashMap + Option<String>, Arc<tokio::Mutex<..>>). No manual impls.

### Panic-safety surface
- `catch_unwind` at `src/kg_analysis.rs:243-245`, inside `#[cfg(test)] mod tests`, test `analyse_never_panics_on_unicode`. Facts for bucket 11:
  1. Closure body: a single call `analyse_kg_document("Rust merhaba مرحبا 你好 Rust", &thesaurus)`.
  2. Captures: `&Thesaurus` (shared reference to a local built by `make_thesaurus`, line 241) and a string literal. Nothing else.
  3. No `AssertUnwindSafe`; `UnwindSafe` is therefore compiler-checked, and it holds: `Thesaurus` is `name: String`, `data: AHashMap<NormalizedTermValue, NormalizedTerm>`, `source_hash: Option<String>` (`terraphim_types-1.22.1/src/lib.rs:837-846`), no `UnsafeCell`, no `RefCell`, no mutex.
  4. No lock guard is held inside or across the closure; the function under test is pure (it takes `&str`, `&Thesaurus` and returns an owned `KgAnalysis`).
  5. No shared `&mut` state exists that could be observed torn after an unwind; all mutation inside `analyse_kg_document` is on function-local `Vec`s and `HashSet`s that are dropped by the unwind.
  6. The `Result<KgAnalysis, Box<dyn Any + Send>>` is consumed by `result.is_ok()` (247); the payload is dropped, never `resume_unwind`ed. No `set_hook`/`take_hook`.
- `NegativeContributionScanner::new()` calls `.expect("Failed to load embedded edm_tier1.json")` (`terraphim_negative_contribution/src/scanner.rs:19-20`). `TerraphimLspServer::with_config` invokes it at `src/server.rs:30`, which the binary reaches via `LspService::new(TerraphimLspServer::new)` at `src/bin/terraphim-lsp.rs:262`. A malformed embedded JSON therefore panics at server construction (startup), not inside a request handler.
- `scanner.scan_file` runs inside the detached spawned task (server.rs:62). A panic there is caught by the tokio task boundary and is silent (no `JoinHandle` awaited). It does not poison anything: the only tokio mutex guard in scope (`docs`) is dropped at line 57 before the scan, and tokio mutexes do not poison.
- Byte-slicing in `kg_analysis.rs`: line 70 uses `text.get(start..end)` with a fallback, so an off-boundary position cannot panic. Line 112 slices `text[byte_offset..]`; `byte_offset` is advanced by each split-word's byte length plus the UTF-8 length of the single delimiter char (110-117), and `split` on a `char` predicate yields boundary-aligned pieces, so the index is always a char boundary. Line 116 `.unwrap_or(1)` is unreachable in practice because `byte_offset < text.len()` guarantees a next char.
- `diagnostic.rs:124-128`: `finding.line - 1` guarded by a zero check; `u32::MAX` used as end column (131).
- `unwrap`/`expect` in non-test lib code: none in this crate. Test asserts and `unreachable!`: none beyond ordinary `assert!`.

### Env access
- None at runtime in the library or binary. No `env::var`, `set_var`, `remove_var`, `env!`, `dirs::` or `current_exe`.
- Indirect: none (the crate does not read config from disk; `LspConfig` is constructed by the caller or `Default`).

### Library-trait invariant surface
- No manual `impl Hash/PartialEq/Eq/Ord/PartialOrd/Borrow/Iterator/Send/Sync`.
- Derived `PartialEq` on `TermMatch` (kg_analysis.rs:11) and `PartialEq + Default` on `KgAnalysis` (24); neither is used as a map key.
- `LspConfig` derives `Serialize, Deserialize` with `serde(default = ...)` fields (config.rs:214-219); a manual `Default` impl (222-229) that agrees with the serde defaults.

### Notable patterns
- Debounce via abort-and-respawn keyed by URL, with the abort map never pruned.
- `Url` (from `lsp_types`) is used as a `HashMap` key; it is a third-party type with derived `Hash`/`Eq`.
- `uri_for_spawn.path()` (server.rs:61) passes the raw URL path to the exclusion logic in `terraphim_negative_contribution`, so percent-encoded paths are matched as-is.
- The crate relies on `NegativeContributionScanner: Clone` and clones the whole `Thesaurus` on every `schedule_diagnostics` (line 40), i.e. once per keystroke burst.

## terraphim_cli

### Purpose
Binary crate `terraphim-cli` (bin only, `src/main.rs`, no lib target). Clap CLI over `CliService` (`src/service.rs`), which wraps `terraphim_service::TerraphimService` and a `ConfigState`. Commands: search, config, roles, graph, replace, find, extract, coverage, evaluate, thesaurus, check-update, update, rollback, kg list, usage (feature `usage`, default on), completions.

Files: `src/main.rs` (987 lines), `src/service.rs` (549). Tests: `tests/cli_command_tests.rs` (878), `tests/integration_tests.rs` (899), `tests/service_tests.rs` (693), `tests/fixtures/`.

### Unsafe-surface tally
- Source `unsafe`: 0 (src and tests). `SAFETY`: 0.
- Raw pointers, FFI, transmute: none.
- Macro-generated `unsafe` in expansion of `--bin terraphim-cli`: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives.

### Concurrency surface
`CliService` (`src/service.rs:17-21`), `#[derive(Clone)]`:
- `config_state: ConfigState`, which is `{ config: Arc<tokio::sync::Mutex<Config>>, roles: AHashMap<RoleName, RoleGraphSync> }` (`terraphim_config-1.20.2/src/lib.rs:1109-1114`); `RoleGraphSync { inner: Arc<tokio::sync::Mutex<RoleGraph>> }` (`terraphim_rolegraph-1.20.2/src/lib.rs:1340-1342`).
- `service: Arc<tokio::sync::Mutex<TerraphimService>>` (line 20, constructed at 234).

Two routes reach the same `Arc<Mutex<Config>>`:
1. Directly through `config_state.config` from `CliService::list_roles` (service.rs:250) and from every `terraphim_command_runtime` helper (see that crate).
2. Indirectly through `TerraphimService`, which is built with `config_state.clone()` (service.rs:230) and locks `self.config_state.config` internally, e.g. `update_selected_role` at `terraphim_service-1.21.1/src/lib.rs:185`.

Lock acquisition per method in `service.rs`:
- `list_roles` (249-252): config only.
- `update_selected_role` (265-269): `service` guard taken at 266 and held across `.await` into `TerraphimService::update_selected_role`, which takes `config` at service crate line 185. Observed ordering: service then config.
- `search_with_options` (277-298): `service` guard (mutable, 296) held across `.await` of `service.search` (297).
- `list_kg_entries` (304-331): `RoleGraphSync` guard (310) held for the duration of the synchronous iteration; no other lock.
- `get_thesaurus` (334-337): `service` guard held across `.await` of `ensure_thesaurus_loaded`.
- `get_top_concepts` (342-381): `RoleGraphSync` guard (351) only.
- `find_matches` (384-394), `extract_with_grounding` (397-425), `replace_matches` (523-535): call `get_thesaurus` (service guard released before the automata call).
- `extract_with_schema` (428), `calculate_coverage` (486): no locks (synchronous, build a local `Thesaurus`).
No method in this crate takes `config` and then `service`, so the only ordering present is service before config.

Runtime: `#[tokio::main]` (main.rs:351). `main` constructs exactly one `CliService` (368), calls one handler, prints, exits. No `tokio::spawn`, `spawn_blocking`, `thread::spawn`, `block_on`, `Handle::current`, atomics, `OnceLock`/`LazyLock`, or mutable `static` anywhere in src. The `Arc<Mutex<..>>` therefore has a single owner and no contention in the binary; it exists to satisfy `TerraphimService`'s API shape and the `Clone` derive.

Tests: `tests/service_tests.rs` uses `#[tokio::test]` (lines 38, 51, 68, 81, 103, 120, 137, 157) and constructs `CliService` in-process. `tests/cli_command_tests.rs` and `tests/integration_tests.rs` spawn the real binary through `assert_cmd::Command::cargo_bin("terraphim-cli")` (cli_command_tests.rs:13, integration_tests.rs:15, 20) and mark the service-touching tests `#[serial]` (serial_test 3.3; e.g. cli_command_tests.rs:162 onwards, integration_tests.rs:46 onwards). The serialisation exists because each binary invocation opens the same on-disk persistence (SQLite via `terraphim_persistence`) and the `roles select` tests mutate persisted state (integration_tests.rs role_switching_tests). That is process-level shared state, not in-process concurrency.

### Panic-safety surface
- `std::process::exit(1)` at main.rs:437 (no command), 452 (coverage below threshold, after printing) and 472 (error path). Each exits without unwinding, so `Drop` for `service` (and any persistence connections it owns) does not run; the tokio runtime created by `#[tokio::main]` is not shut down gracefully.
- `unreachable!()` at main.rs:434 for `Commands::Completions`, which is handled earlier at 356-365.
- main.rs:448: `.unwrap()` on `serde_json::to_string(&output)` inside the `format_as_text` fallback (`unwrap_or_else`). The value is a `serde_json::Value`, which serialises infallibly in practice.
- service.rs:210: `.unwrap_or_else(|_| config.clone())` swallows a failed merge.
- service.rs:534: `String::from_utf8(result).unwrap_or_else(|_| text.to_string())` swallows invalid UTF-8 from `replace_matches` by returning the input.
- No `catch_unwind`, `set_hook`, `resume_unwind`.
- Tests: `expect` on `TempDir::new()` (integration_tests.rs:727, 756) and on `Command::output` (cli_command_tests.rs:167 and similar); ordinary test-side panics.

### Env access
Compile time:
- `env!("CARGO_PKG_VERSION")` at main.rs:843 and 889 (check-update and update paths).
- `env!("CARGO_MANIFEST_DIR")` at tests/cli_command_tests.rs:22 and tests/service_tests.rs:460.
Runtime:
- `std::env::current_exe()` at main.rs:941 (`handle_rollback`), then `terraphim_update::rollback(&backup_path, &current_exe)` at 944 replaces the running executable on disk.
- `std::env::var("CARGO_MANIFEST_DIR")` at tests/service_tests.rs:11 with a `"."` fallback.
- Delegated reads: `terraphim_service::logging::detect_logging_config()` (service.rs:34-36) and `DeviceSettings::load_from_env_and_file(None)` (service.rs:58) read the process environment and `settings.toml` inside registry crates; `Config::load_from_json_file` (43, 127) reads a path from `--config` or from settings.
- No `set_var`/`remove_var` in src or tests. The CLI tests pass no `.env()` overrides to the spawned binary; the child inherits the test process environment and the user's real persistence directory.

### Library-trait invariant surface
- No manual trait impls in the sweep. Output structs in main.rs derive `Serialize` only. `CoverageResult` (service.rs:539-549) derives `Debug, Clone, Serialize`.
- `entry_lookup` (service.rs:433-437) keys on `term.to_lowercase()` while the lookup at 443 uses `m.normalized_term.value.to_string()` unmodified; a case mismatch falls through to `"unknown"` (445). This is a data-consistency fact, not a trait invariant.

### Notable patterns
- Config resolution ladder in `CliService::new` (service.rs:32-103) with three fallbacks to embedded defaults; `load_with_role_config` (106-171) writes to persistence on first run (156).
- `handle_rollback` (939-951) self-replaces the binary.
- `#[allow(deprecated)]` on `cargo_bin` in both binary test files.
- Feature `usage` compiled in by default; `handle_usage` (962-987) registers five providers, all constructed synchronously.

## terraphim_hooks

### Purpose
Library shared by Claude Code hooks and git hooks: `ReplacementService` (thesaurus-driven text replacement, `src/replacement.rs`), `ValidationService` (first-match allow/warn/block/modify, `src/validation.rs`), result types (`src/validation_types.rs`), and `discover_binary` for locating `terraphim-agent` (`src/discovery.rs`).

Files: `src/replacement.rs` (439 lines), `src/validation.rs` (245), `src/validation_types.rs` (222), `src/discovery.rs` (100), `src/lib.rs` (18). No `tests/` directory; dev-deps `tokio` and `tempfile` are declared but unused by the in-file tests (no `#[tokio::test]`, no `TempDir` in this crate).

### Unsafe-surface tally
- Source `unsafe`: 0. `SAFETY`: 0. No raw pointers or FFI.
- Macro-generated `unsafe` in expansion: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives.

### Concurrency surface
- None. No `Arc`, mutex, atomics, statics, spawns or runtimes. `ReplacementService` and `ValidationService` are plain `Clone` structs holding a `Thesaurus` by value (replacement.rs:82-86, validation.rs:16-19). Every method takes `&self` and is synchronous; each `find_matches`/`replace_matches` call rebuilds the Aho-Corasick automaton from the thesaurus (`terraphim_automata-1.21.0/src/matcher.rs:56-59`).
- `discovery.rs:57-63` spawns a child process (`which <binary>`) synchronously via `std::process::Command::output()`.

### Panic-safety surface
- Non-test lib code contains no `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!` or `unimplemented!`. Errors are `thiserror` enums (`ReplacementError`, replacement.rs:12-18; `ValidationError`, validation_types.rs:23-31) with fail-open constructors (`HookResult::fail_open` 70-78, `ValidationResult::fail_open` 89-96).
- `validation.rs:59` indexes `matches[0]` after an `is_empty()` check at 53.
- `validation.rs:76` uses `str::replace` with the matched `term`; the Aho-Corasick match is ASCII case-insensitive (matcher.rs:58) while `str::replace` is case-sensitive, so a match on differently-cased input can produce a `Modify` outcome whose text equals the input. Data fact, no panic.
- `validate_fail_open` (86-88) is an alias of `validate`; the fail-open behaviour lives in `validate` itself (38-48).
- Test code: `unwrap()` on service results and serde round-trips (replacement.rs:168-410, validation_types.rs:203-219); `panic!("Expected ... outcome")` in match arms at validation.rs:155, 171, 188 and validation_types.rs:149, 166, 181. `discovery.rs:82-86` test runs `discover_binary()` against the real host (PATH, `./target/release`, `~/.cargo/bin`).
- No `catch_unwind`.

### Env access
- `dirs::home_dir()` at discovery.rs:47 (reads `HOME` on unix).
- Implicit `PATH` read via `Command::new("which")` at discovery.rs:58, and a relative-path probe `./target/release/terraphim-agent` at 41 that depends on the process working directory.
- `#[cfg(unix)]` permission-bit check at 65-71 uses `PermissionsExt::mode() & 0o111`; the non-unix fallback (73-76) treats existence as executable.
- No `env::var`, `env!`, `set_var`, `remove_var`.

### Library-trait invariant surface
- No manual impls. Derived `PartialEq, Eq` on `BinaryLocation` (discovery.rs:6) and on `ValidationOutcome` (validation_types.rs:9), with serde `rename_all = "snake_case"` on the latter. `ValidationError` derives `thiserror::Error` plus `Clone, Serialize, Deserialize`.
- Re-exports at lib.rs:17-18 (`terraphim_automata::Matched`, `terraphim_types::Thesaurus`) make registry types part of this crate's public surface.

### Notable patterns
- `HookResult::success` (42-45) derives `replacements` as 0 or 1 from string inequality; `replace` (104-114) uses `with_count` with the real match count.
- The crate re-scans the input twice per `replace` (once in `find_matches` at 105, once inside `replace_matches` at 107).

## terraphim_command_runtime

### Purpose
Five async helper functions over a borrowed `ConfigState`, extracted so `terraphim_cli` and `terraphim_agent` share config and role resolution (ADR-003, Gitea #1910 per the module doc, lib.rs:1-10). Single file `src/lib.rs` (70 lines). No tests in the crate.

### Unsafe-surface tally
- Source `unsafe`: 0. `SAFETY`: 0.
- Macro-generated `unsafe` in expansion: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives. The crate has no proc-macro derives of its own; any generated `unsafe` would come from dependencies' inline code, which `cargo expand` does not include.

### Concurrency surface
Every function locks `config_state.config` (`Arc<tokio::sync::Mutex<Config>>`):
- `get_config` (17-20): guard, `clone()` of the whole `Config`, release.
- `get_selected_role` (23-26): guard, clone one field, release.
- `list_roles_with_info` (29-36): guard held during synchronous iteration.
- `find_role_by_name_or_shortname` (39-63): guard held during two synchronous scans; early `return` from inside the guard scope (49, 58) drops the guard normally.
- `save_config` (66-70): guard held across `.await` of `config.save()` (68), which performs persistence I/O in `terraphim_persistence`. This is the only helper that awaits while holding the config lock.
No other lock is taken, so no ordering beyond "config only". No spawns, statics, atomics or runtimes.

### Panic-safety surface
- No `unwrap`, `expect`, `panic!`, `unreachable!`, `catch_unwind`. Errors flow through `anyhow::Result` in `save_config`.
- Uses let-chains (`if let Some(ref shortname) = role.shortname && ...`, 55-56), i.e. edition 2024 syntax.

### Env access
- None. The crate reads no environment, files or paths directly; `config.save()` delegates to configured persistence backends.

### Library-trait invariant surface
- None; the crate defines no types.

### Notable patterns
- Case-insensitive matching via `to_lowercase()` on both sides (44, 48, 56), so Unicode case folding differences apply equally to name and query.

## terraphim_negative_contribution

### Purpose
Static detection of Explicit Deferral Markers (`todo!()`, `unimplemented!()`, `panic!("TODO")` and similar) in Rust source, driven by an embedded thesaurus (`data/edm_tier1.json`, 1461 bytes, via `include_str!` at scanner.rs:8). Provides `NegativeContributionScanner` and `is_non_production` path/content exclusion. Consumed by `terraphim_lsp`.

Files: `src/scanner.rs` (401 lines, of which 174-401 are tests), `src/exclusion.rs` (131, tests from 50), `src/lib.rs` (7). No `tests/` directory.

### Unsafe-surface tally
- Source `unsafe`: 0. `SAFETY`: 0. No raw pointers or FFI.
- Macro-generated `unsafe` in expansion: SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives.

### Concurrency surface
- None. `NegativeContributionScanner` is `Debug + Clone` holding a `Thesaurus` by value (scanner.rs:12-15). All methods take `&self` and are synchronous. No statics beyond `const &str` (scanner.rs:8-10, exclusion.rs:1-5).

### Panic-safety surface
- `scanner.rs:19-20`: `load_thesaurus_from_json(DEFAULT_EDM_TIER1_JSON).expect("Failed to load embedded edm_tier1.json")` in `new()`, also reached through `Default` (108-112). Panics at construction if the embedded JSON does not parse; the JSON is compiled in, so this is deterministic per build.
- `scanner.rs:46`: `mat.pos.unwrap_or((0, 0))`; positions are always `Some` because `find_matches` is called with `return_positions = true` (35).
- `scanner.rs:62`: `&content[start..end.min(content.len())]` byte-slices the original content at Aho-Corasick match positions. The positions come from `ac.find_iter(text)` over the unmodified input with `ascii_case_insensitive(true)` and `LeftmostLongest` (matcher.rs:56-70); no lowercasing or re-encoding of the input occurs, and every pattern is a valid UTF-8 `&str`, so `start` and `end` are char boundaries of `content`. `end` cannot exceed `content.len()` by construction; the `.min` is redundant.
- `scanner.rs:157-171` `get_line_content`: bounds-checked against `line_starts` (159) and `content.len()` (163) before slicing; `find('\n')` gives a boundary-aligned end. `byte_to_line` (150-155) uses `binary_search`.
- `exclusion.rs:29`: `rsplit('/').next().unwrap_or(&normalized)`; `rsplit` always yields at least one item.
- Test code uses `assert!` only; the strings containing `todo!()`, `unimplemented!()` and `panic!(...)` in scanner.rs:183-398 are string literals fed to the scanner, not macro invocations.
- No `catch_unwind`.

### Env access
- None at runtime. `include_str!("../data/edm_tier1.json")` at scanner.rs:8 is compile-time. `is_non_production` operates on the caller-supplied path string (exclusion.rs:7-12) and never touches the filesystem.

### Library-trait invariant surface
- No manual impls. `Default` for `NegativeContributionScanner` (scanner.rs:108-112) delegates to `new()` and can therefore panic (see above), which is unusual for a `Default` impl but not a trait-contract violation.

### Notable patterns
- Severity, description and suggestion are smuggled through the thesaurus entry's `url` field, split on `"||"` (scanner.rs:114-138).
- Content-based exclusion: any file containing the substring `#[test]` or `#[cfg(test)]` anywhere is treated as non-production in its entirety (exclusion.rs:5, 46-48), including string literals containing those markers.
- `is_excluded_path` normalises backslashes to slashes (exclusion.rs:15) and matches the infix `/tests/` anywhere in the path.

## Open questions

1. tower-lsp 0.20 concurrency: with `DEFAULT_MAX_CONCURRENCY = 4`, does the transport preserve arrival order for notifications on the same URI, or can `did_change(v1)` observe its `documents` insert after `did_change(v2)`? The server's read-at-fire-time design makes the published diagnostics track the last write to `documents`, but the last write is not guaranteed to be the editor's latest version if reordering is possible. Needs a read of `tower-lsp-0.20.0/src/transport.rs` dispatch loop (Phase 2 candidate).
2. `pending` map in `terraphim_lsp/src/server.rs:19` is never pruned and `did_close` does not abort the debounce task. Is a post-close `publish_diagnostics` with an empty vector acceptable to all clients, and is unbounded growth over the lifetime of an editor session a concern for long sessions with many files? Fact recorded; classification deferred.
3. Silent task panics: the diagnostics task's `JoinHandle` is dropped (server.rs:73). A panic in `scan_file` leaves no trace beyond tokio's default panic output. Is any observer expected? Cross-check with the workspace's panic-handling conventions in section notes for the server crates.
4. Lock hold across I/O: `terraphim_command_runtime::save_config` (lib.rs:67-68) and `CliService::search_with_options` / `get_thesaurus` (service.rs:296, 335) hold a tokio mutex guard across awaits into registry crates. In the CLI binary there is a single task so no contention exists; the same helpers are shared with `terraphim_agent` (per lib.rs:1-10), where the TUI may be multi-task. Ownership of that question belongs to the section covering `terraphim_agent`.
5. `CliService::new` is invoked once per process in the binary, but `tests/service_tests.rs` constructs it repeatedly under `#[tokio::test]` without `#[serial]` while the binary tests run `#[serial]`. Whether those in-process tests share the on-disk persistence with the spawned-binary tests when the whole crate's test targets run in parallel is a test-isolation question, not a UB question.
6. Expansion counts were not measured for these five crates; the lead closed them as skipped on the basis of the workspace-wide expansion evidence quoted at the top of this file.

## Expansion results

SKIPPED by lead: teammate lib expansions across the workspace show macro-generated unsafe limited to std vec! and derive(Clone, Copy) marker impls; these five crates use the same derives. Job bfy5fj0hy was stopped before producing output; no per-crate counts exist for section H.
