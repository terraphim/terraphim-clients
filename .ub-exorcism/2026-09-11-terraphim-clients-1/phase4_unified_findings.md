# Phase 4 unified findings

Run: 2026-09-11-terraphim-clients-1 (Exhaustive; archetype pure-safe-with-env-unsafe; tracking issue terraphim/terraphim-clients#252). Written by the Phase 4 synthesizer, 2026-09-12 10:05 BST. Source unmodified.

Inputs: `phase0_run.json`, `phase0_partition.json`, `phase1_unsafe_surface_inventory.md`, `phase1_notes/A..J.md`, `phase2_summary.md`, the five live bucket files (`phase2_findings_{data-races,send-sync,panic-safety,library-trait,async-drop}.md`), the 20 NO_SURFACE bucket files, `phase3_dynamic_findings.md`, `phase3_raw/miri_*.log`, and the perf-track root cause for rank drift (scratchpad `perf/score_drift_rca.md`, terraphim/terraphim-ai#3376). The experiment registry is `UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md` (EXP ids below).

## How to read the table

- **Numbering.** Every per-bucket file restarted its F-numbers at F-001 (send-sync used F-8NN; Phase 3 used F-1NN). This table renumbers globally as UF-001 onwards and keeps a `source ids` column mapping back. Dedup rule: one writer or one code site is one finding, whatever the number of phases that reported it. The merged pairs are listed under "Dedup decisions".
- **Severity** uses the taxonomy MUST-BE-UB / LIKELY-UB / SUSPICIOUS / CONTRACTUAL-BUT-DEFENSIBLE, re-scored on the union of static and dynamic evidence, followed by the dynamic verdict where an experiment ran. No finding reached MUST-BE-UB. Rows are ordered by severity, then by how far the evidence goes.
- **Status** is one of four tokens. CONFIRMED_UB: a tool observed the unsynchronised conflicting accesses (or other UB) in the described binary or in a faithful reduction. NO_EVIDENCE: an experiment able to see the predicted signal ran and stayed clean. OPEN: an experiment is allocated in the registry and Phase 5 must run it. DEFERRED: no experiment allocated this round, because the item is safe by construction with a stated static rationale, is a dependency defect waiting on the lockfile bump, or is a non-UB defect routed to another tracker (see "Non-UB defects routed elsewhere").
- **Platform note** matters for the env findings: Apple Libc serialises `getenv`/`setenv` behind `environ_lock_np()`, glibc does not. Both CI runners (`ubuntu-latest` for `.github/workflows/ci.yml`, bigbox for `.gitea/workflows/native-ci.yml`) are glibc.

## Master table

| UF | Title | file:line | Bucket(s) | Severity (static; dynamic verdict) | Platform note | Status | Source ids | Cross-refs |
|---|---|---|---|---|---|---|---|---|
| UF-001 | terraphim_grep lib tests mutate `OPENROUTER_BASE_URL` while libgit2 `getenv` runs on sibling test threads | crates/terraphim_grep/src/openrouter_client.rs:170-191 (writer); crates/terraphim_grep/src/hybrid_searcher.rs:362-364 -> fff-search -> git2::init -> libgit2 sysdir.c:347,357,402,405 (reader) | 7 data races (+ffi-transitive, safety-comment-accuracy) | LIKELY-UB; CONFIRMED_UB (EXP-001b, 1 unassisted overlap in 100 runs at widen=0, 1007 widened, all the predicted libgit2 chain) | Superseded by lead addendum 2 and plan section 1 (edit delegated to the lead in plan section 13 and applied by Phase 10): the two nextest `test` lanes are one process per test and are NOT exposed; the two `cargo llvm-cov --lib` coverage lanes and BUILD.md's `cargo test` run libtest in-process on glibc and ARE. glibc: unsynchronised read of `__environ` during `setenv`/`unsetenv`. macOS: libc-locked, contract violation only | CONFIRMED_UB | data-races F-001; phase3 F-102; EXP-001 (NEEDS_REFINEMENT, TSan blind), EXP-001b | #280; UF-012 (same writer, macOS reader); UF-024 (logical race on the same variable) |
| UF-002 | `EnvVarGuard` in tui_service_tests mutates HOME/XDG_CONFIG_HOME/TERRAPHIM_DATA_PATH while sqlite `unixTempFileInit` may `getenv` on a sibling thread | crates/terraphim_agent/tests/tui_service_tests.rs:37,40,43 and Drop at :61 -> terraphim_test_utils-1.20.3/src/lib.rs:39,61,106-113 (writer); crates/terraphim_agent/src/service.rs:431,462-464 -> terraphim_service thesaurus.rs:393 -> terraphim_persistence settings.rs:110 -> rusqlite -> bundled sqlite3.c:44351-44354 (reader) | 7 data races (+hidden-unsafe-in-safe-wrapper) | LIKELY-UB; CONFIRMED_UB for the mechanism (EXP-002c, 102 unassisted overlaps in 150 runs), NO_EVIDENCE in the shipped binary (EXP-002b, 0 overlaps in 300 runs: the serial writer is the first sqlite opener, so the once-per-process reader lands on the writer's own thread) | Superseded by lead addendum 2 (applied by Phase 10): native-ci runs this binary but under nextest, one process per test, so that lane is NOT exposed; the coverage lanes exclude it (`--lib`); BUILD.md's `cargo test` is the exposed path. glibc | CONFIRMED_UB | data-races F-002; phase3 F-103; EXP-002 (NEEDS_REFINEMENT), EXP-002b, EXP-002c | #252; UF-003 (root cause), UF-023 (DEVICE_STORAGE path binding), UF-014 (same wrapper) |
| UF-003 | `terraphim_test_utils` SAFETY rule ("use #[serial]") cannot satisfy its own precondition in a mixed binary; safe `pub fn` over `unsafe set_var` | ~/.cargo/registry/src/git.terraphim.cloud-72dcd9d4c11821d8/terraphim_test_utils-1.20.3/src/lib.rs:21-25,37-38,59-60; consumers tui_service_tests.rs:37-43 and the UF-014 sites | 7 data races (+library-invariant) | SUSPICIOUS (unsound safe abstraction; its UB manifestation is UF-002) | all | OPEN | data-races F-006; EXP-014 | UF-002, UF-014; dead cfg `rust_has_unsafe_env_setters` in crates/terraphim_agent/build.rs:19 |
| UF-004 (severity raised by lead 2026-09-12: LIKELY-UB; glibc resolver reads RES_OPTIONS et al. via internal getenv, the reader std::env::set_var documents, and EXP-010 showed the writer/getaddrinfo window in 6/100 widened iterations; unobservable by the shim, hence DEFERRED experiment) | terraphim_update `setup_temp_config_dir` sets five env vars under a false SAFETY comment; 120 non-serial siblings; resolver reader not ruled out; HOME left dangling | crates/terraphim_update/src/state.rs:130-137; live-network reader candidate lib.rs:1713 (`getaddrinfo`) | 7 data races (+safety-comment-accuracy, test-hygiene) | SUSPICIOUS (contract violated; no native reader found statically; not probed) | glibc; ci.yml runs this binary (`--lib`) | OPEN | data-races F-003; EXP-010 | UF-001 (same shape); note the LD_PRELOAD probe is likely blind to glibc-internal resolver reads, see EXP-010 falsifiability |
| UF-005 | crossbeam-epoch 0.9.18 Stacked Borrows report in `Local::element_of` on rayon worker threads (Miri, session-analyzer) | crossbeam-epoch-0.9.18/src/internal.rs:548-549 via rayon `par_iter` at crates/terraphim-session-analyzer/src/parser.rs:177,242,284 and analyzer.rs:72 | 1 aliasing (dependency-inherited) | SUSPICIOUS (dependency; Tree Borrows accepts the same tests; container-of pattern) | Miri only | NO_EVIDENCE (EXP-101, post-bump: crossbeam-epoch-only Stacked Borrows report, Tree Borrows clean) | phase3 miri_default_terraphim_session_analyzer_probe1/probe_parser logs; EXP-003f (OPEN, two axes PENDING), EXP-004 | RUSTSEC-2026-0204 bump (0.9.21 carries identical `element_of` code, so the bump will not clear it); #264 |
| UF-006 | Rollback truncates the running executable in place (`fs::copy` over `current_exe()`) | crates/terraphim_update/src/lib.rs:1536; crates/terraphim_update/src/rollback.rs:310; callers crates/terraphim_cli/src/main.rs:939-944, crates/terraphim_agent/src/repl/handler.rs:1849-1852 | 11 panic safety (file lifecycle) | SUSPICIOUS (not memory UB; durability defect in shipped commands) | Linux: `ETXTBSY`, rollback can never succeed. macOS: interrupted copy leaves a truncated binary | DEFERRED | panic-safety F-001 | routed (non-UB); phase1 E open question answered "yes" |
| UF-007 | Nested `Runtime::new` + `block_on` inside async `run_server_command` panics deterministically (`--server sessions ...`) | crates/terraphim_agent/src/server_command.rs:751-752 (driven by main.rs:552-558) | 17 runtime nesting (+11) | SUSPICIOUS (deterministic panic; no double panic, inner pool threads leaked; no UB) | feature `server`, no CI lane | DEFERRED | async-drop F-001 | routed (non-UB); offline twin main.rs:1593 is correct |
| UF-008 | `shared_learning::store::block_on` bridge (`block_in_place` + `Handle::current().block_on`) panics on current_thread runtimes and with no runtime; contract undocumented on a public sync trait impl | crates/terraphim_agent/src/shared_learning/store.rs:608-612, impl :616-753 | 17 runtime nesting (+7 cross-tag) | SUSPICIOUS (panic, not UB; zero production callers in this workspace) | all | OPEN | data-races F-013; async-drop F-002; EXP-013 | phase1 B open question 1 (answered for this workspace); registry `terraphim_types` trait |
| UF-009 | Enricher byte-window context extraction slices mid-character | crates/terraphim_sessions/src/enrichment/enricher.rs:177-188 (reached from :103-108; production caller crates/terraphim_agent/src/repl/handler.rs:2518) | 12 library trait (str boundary) (+11) | SUSPICIOUS (safe-Rust panic on ordinary session text) | all | OPEN | library-trait F-001; EXP-012 | correct idiom at terraphim_sessions/src/search.rs:80-85 |
| UF-010 | `Session::summary` cuts the first user message at byte 100 | crates/terraphim_sessions/src/model.rs:326-336 (via search.rs:36, every `search_sessions` call) | 12 (+11) | SUSPICIOUS (panics `sessions search` for the whole corpus on one non-ASCII prompt) | all | OPEN | library-trait F-002; EXP-012 | UF-009 |
| UF-011 | Fixed-byte display truncations on input-controlled strings (titles, message content, shell commands, procedure steps) | crates/terraphim_agent/src/repl/handler.rs:994,1995,2051,2098,2172,2226,2321,2585; learnings/procedure.rs:427; main.rs:2774; crates/terraphim_sessions/src/model.rs:332; `&session.id[..8]` at handler.rs:2241,2329 (bounds, defensible today) | 12 (+11) | SUSPICIOUS, low impact (one REPL command or CLI invocation panics) | all | OPEN | panic-safety F-003; library-trait F-007; EXP-012 | prior fix of the same class at cli_helpers.rs:52; `cli_helpers::truncate_snippet` exists |
| UF-012 | macOS-only native `getenv` reader (SystemConfiguration / libsystem_trace via hyper-util `Matcher::from_system`) overlaps the grep writer on a sibling test thread | reader crates/terraphim_grep/src/openrouter_client.rs `OpenRouterClient::new` -> reqwest `ClientBuilder::build` -> hyper_util `mac::with_system` -> `SCDynamicStoreCreateWithOptions` -> `_os_trace_init_slow` -> `getenv`; writer as UF-001 | 7 data races | CONTRACTUAL-BUT-DEFENSIBLE (contract violation; Apple Libc locks both sides so not memory-unsafe); dynamic: 11 strict overlaps in 30 macOS runs (EXP-001 probe) | macOS only; no equivalent path on Linux | DEFERRED (reader observed, libc-locked; no further experiment) | phase3 F-101 | UF-001; corrects Phase 2's `env-read-std` classification of the hyper-util proxy path on macOS |
| UF-013 | `test_mcp_stdio` sets env vars with no SAFETY comment; sound only because it is a one-test binary | crates/terraphim_mcp_server/tests/test_mcp_stdio.rs:15-20 | 7 | CONTRACTUAL-BUT-DEFENSIBLE (safe by construction; gated by `RUN_MCP_STDIO_TEST`) | all | DEFERRED | data-races F-004 | adding a second test recreates UF-001's shape |
| UF-014 | `terraphim_test_utils::set_env_var` sites in the two serialised mcp_server binaries; invariant enforced one level away from the mutation | crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs:22,28; tests/mcp_rolegraph_validation_test.rs:21,27,137,284,394,618 | 7 (+hidden-unsafe-in-safe-wrapper) | CONTRACTUAL-BUT-DEFENSIBLE (all tests `#[serial]`; memory-only persistence; child servers are separate processes) | all | DEFERRED | data-races F-005; phase1 G open question 3 | UF-003; a CI guard test comparing `#[serial]` count to test count is the cheap fix |
| UF-015 | Seven unserialised mcp_server integration binaries spawn the real server without `TERRAPHIM_PROFILE_MEMORY_TYPE`; may share `~/.terraphim/sqlite/terraphim.db` across up to seven child processes | crates/terraphim_mcp_server/tests/{integration_test,test_advanced_automata_functions,test_bug_report_extraction,test_kg_term_verification,test_mcp_fixes_validation,test_selected_role_usage,test_working_advanced_functions}.rs | 7 (cross-process shared state) (+11 file lifecycle) | CONTRACTUAL-BUT-DEFENSIBLE (sqlite file locks; failure modes are stalls and writes into the developer's real config) | all; native-ci `--all-targets` | OPEN | phase1 G open question 1; EXP-015 | UF-023; phase1 C item 2 (inherited-HOME spawns and documented WAL stalls) |
| UF-016 | event-listener 5.4.1 unsound `Send`/`Sync` for `StackSlot` (RUSTSEC-2026-0221) reaches five crates through sqlx via opendal; unsound path (`Event::with_tag` + `listener!`) unused by sqlx-core | Cargo.lock:1752-1755; sqlx-core-0.8.6/src/pool/inner.rs:33,62,119 | 8 Send/Sync (dependency) | CONTRACTUAL-BUT-DEFENSIBLE (sqlx is live at runtime; the tag path is unreachable) | all | CLOSED (the lockfile bumps of 2026-09-12 landed at ae06cd2 and 46bbc43; HEAD carries event-listener 5.4.2, anyhow 1.0.104, lru 0.18.4, memmap2 0.9.11, libgit2-sys 0.18.8+1.9.7; cargo deny check passes on all four checks) | send-sync F-801; phase1 J | #254 (lockfile bump class); bump to 5.4.2 |
| UF-017 | Other UB-class advisories in the resolved graph: anyhow 1.0.102 (RUSTSEC-2026-0190, `downcast_mut`, 0 workspace uses), lru 0.18.0 (0253, UAF only if a value's Drop panics; workspace has zero `impl Drop`), memmap2 0.9.10 (0186), git2 0.20.4 (0183/0184, `Remote::list`/`BlameHunk` not called) | Cargo.lock | 8/13/20 (dependency) | CONTRACTUAL-BUT-DEFENSIBLE (no reachable call path found for any) | all | CLOSED (same lockfile bumps; memmap2 at 0.9.11. The advisory-specific functions advise_range, unchecked_advise_range, flush_range and flush_async_range are still not called; the live-mapping exposure is tracked separately as UF-039) | phase1 J advisory table | #257 (git2 via fff-search), #254, #256 and #258 (DoS-class, not UB); crossbeam-epoch is UF-005 |
| UF-018 | Global `KG_CACHE` std `RwLock` held across filesystem I/O on a tokio worker; poison is fail-open (`.ok()?`) | crates/terraphim_agent/src/kg_validation.rs:28, :83-116 (sole caller main.rs:2232) | 7 (blocking-in-async, poison) (+11) | CONTRACTUAL-BUT-DEFENSIBLE (no torn value possible: single assignment after full construction; no await under the guard) | all | DEFERRED | data-races F-011; panic-safety F-004 | one-shot hook process, so "rest of process" is one invocation |
| UF-019 | Tokio lock guards held across awaits into persistence, search and an LLM HTTP call (stall surface; consistent lock order `service` -> `config`, no inversion) | crates/terraphim_agent/src/service.rs:350,449,457,463,555-590,682-683; repl/handler.rs:1914; crates/terraphim_command_runtime/src/lib.rs:66-70; crates/terraphim_cli/src/service.rs:266,296,335; crates/terraphim_mcp_server/src/lib.rs:510-581 (read guard across `service.search` in a loop; write-preferring RwLock) | 7 / 17 (await-holding-lock) | CONTRACTUAL-BUT-DEFENSIBLE (head-of-line blocking in the MCP server under concurrent tool calls; no deadlock; tokio locks do not poison) | all | DEFERRED | data-races F-010; async-drop F-005 | phase1 H open question 4 answered |
| UF-020 | `CommandRegistry` readers acquire `aliases`/`commands` in opposite orders under a write-preferring tokio RwLock | crates/terraphim_agent/src/commands/registry.rs:149-152 vs :244-246; writers :103-129, :226-234, :623-627 | 7 (deadlock hazard) | CONTRACTUAL-BUT-DEFENSIBLE (cycle needs two queued writers plus both readers concurrently; sharing model is single-task today) | all | DEFERRED | data-races F-009 | phase1 B open question 2 |
| UF-021 | `CommandRegistry::reload`/`register_command` apply multi-lock updates in separate scopes; a task panic leaves aliases and commands mutually inconsistent | crates/terraphim_agent/src/commands/registry.rs:95-134, :223-238 | 11 | CONTRACTUAL-BUT-DEFENSIBLE (every reader tolerates the torn shapes with `Option`) | all | DEFERRED | panic-safety F-005 | UF-020 |
| UF-022 | `maybe_auto_import` check-then-act allows duplicate imports | crates/terraphim_sessions/src/service.rs:72-98 | 7 (TOCTOU) | CONTRACTUAL-BUT-DEFENSIBLE (idempotent duplicate work) | all | DEFERRED | data-races F-012 | phase1 D |
| UF-023 | `DEVICE_STORAGE` singleton binds the sqlite data dir to whichever HOME/TERRAPHIM_DATA_PATH is live at first init; tests may write into the real `~/.terraphim` or into a deleted TempDir | terraphim_persistence-1.20.2/src/lib.rs:55,129-143; terraphim_settings-1.20.2/src/lib.rs:158,161-163,198-204; writer tui_service_tests.rs:37-43 | 7 (logical check-then-act; shared-service-state) | CONTRACTUAL-BUT-DEFENSIBLE for UB (Medium for test reliability) | all | DEFERRED | data-races F-008 | UF-002, UF-015; phase1 C notable pattern 2, G open question 1 |
| UF-024 | Sibling openrouter tests observe `OPENROUTER_BASE_URL` mid-mutation (std-locked logical race); restore skipped if the writer's assertion panics | crates/terraphim_grep/src/openrouter_client.rs:51 via tests :158,:196,:205; writer :164-193, restore :184-192 | 7 (logical) | CONTRACTUAL-BUT-DEFENSIBLE (latent flake; tests do not assert `base_url`) | all | DEFERRED | data-races F-007 | UF-001; fixed by the same remediation |
| UF-025 | KG-boost comparator is total only while no `RetrievedChunk.relevance_score` is NaN (`partial_cmp(..).unwrap_or(Equal)`); NaN concept scores are masked by the `f64::min` cap | crates/terraphim_grep/src/hybrid_searcher.rs:210-214, :428-432, mask at :110,:123,:126 | 12 (comparator; Deserialize-produced state) | CONTRACTUAL-BUT-DEFENSIBLE (all in-tree producers finite; serde_json rejects NaN; needs a direct construction or non-JSON format) | all | OPEN | library-trait F-003; EXP-011 | UF-026, UF-027 (same shape) |
| UF-026 | `store_with_dedup` best-match uses `partial_cmp(..).unwrap()` with no NaN filter; NaN derivable from an all-empty corpus (`0.0/0.0` in BM25 length norm) | crates/terraphim_agent/src/shared_learning/store.rs:242 (scorer :88-124, normaliser :126-132) | 12 (+11) | CONTRACTUAL-BUT-DEFENSIBLE (test-only callers in-tree; precondition is two textually empty learnings with distinct ids) | all | OPEN | library-trait F-004; EXP-011 | safe sibling at store.rs:497-500 |
| UF-027 | Markdown frontmatter parse accepts `importance_total: NaN` and feeds the `list_all_entries` sort | crates/terraphim_agent/src/learnings/capture.rs:433,436,462-473; comparator :1445-1463 | 12(e) Deserialize-produced invalid state | CONTRACTUAL-BUT-DEFENSIBLE (in-tree writer cannot produce NaN; hand-edited file only) | all | OPEN | library-trait F-005; EXP-011 | UF-025 |
| UF-028 | Paragraph finder mixes char indices with byte offsets (`chars().nth(byte_pos)`, `pos + 20` unaligned) | crates/terraphim_agent/src/commands/markdown_parser.rs:421-423, :433-449, :452-470 | 12 (+11) | CONTRACTUAL-BUT-DEFENSIBLE (feature `repl-custom`, test-only callers; also O(n^2)) | all | OPEN | library-trait F-006; EXP-012 | UF-011 |
| UF-029 | Rolegraph document rank depends on AHashMap insertion order: the first document to create an edge is counted one less than every later document on that edge (`or_insert(1)` then `+= 1`) | terraphim_rolegraph-1.20.2/src/lib.rs:1226-1239 (`init_or_update_edge`), terraphim_types-1.22.1/src/lib.rs:769-775 (`Edge::new`); insertion order from terraphim_service-1.21.1/src/search.rs:758,770 and terraphim_middleware-1.21.0/src/indexer/mod.rs:190 | 12 / 25 cross-tag (correctness, not UB) | CONTRACTUAL-BUT-DEFENSIBLE for UB (correctness bug: score and tie order vary per process; sum conserved) | all | DEFERRED | library-trait F-008; perf-track RCA `perf/score_drift_rca.md` | terraphim/terraphim-ai#3376; routed (non-UB). The RCA supersedes F-008's candidate mechanism (TF-IDF corpus change): TF-IDF bonus was constant across runs |
| UF-030 | Windows promotion window: `remove_file` then `rename` leaves no binary if killed in between; staging dir orphaned | crates/terraphim_update/src/lib.rs:1083-1096 (`#[cfg(not(unix))]`) | 11 (file lifecycle) | CONTRACTUAL-BUT-DEFENSIBLE (not compiled on the audit hosts; fails cleanly on a running exe) | Windows only | DEFERRED | panic-safety F-002 | UF-006; `self_replace` is already a dependency |
| UF-031 | `notify` FSEvents `extern "C"` callback hosts a workspace closure (`mpsc::Sender::send`, no panic path); the dependency's own `expect`s abort on a non-UTF-8 path | crates/terraphim_sessions/src/connector/native.rs:163-165 -> notify-8.2.0/src/fsevent.rs:506-577 | 11 (FFI callback) / 21 | CONTRACTUAL-BUT-DEFENSIBLE (panic across `extern "C"` aborts on rustc 1.97.1, defined) | macOS only (inotify on Linux has no C callback) | DEFERRED | panic-safety F-006 | phase2_findings_ffi-callback.md (NO_SURFACE stands: the `extern "C"` frame is the dependency's) |
| UF-032 | Update scheduler builds and tears down a fresh multi-thread `Runtime` per tick inside `spawn_blocking`; shutdown of the outer runtime stalls for the duration of an in-flight check | crates/terraphim_update/src/lib.rs:1407-1410 via scheduler.rs:250-254 | 17 | CONTRACTUAL-BUT-DEFENSIBLE (legal on both flavours; no caller in workspace tests) | all | DEFERRED | async-drop F-003 | phase1 E open question 3 |
| UF-033 | Two long-lived multi-thread runtimes coexist on the main thread (start-up update check plus REPL/command/TUI runtime); TUI loop blocks the UI thread per backend call | crates/terraphim_agent/src/main.rs:449,494,552,2992 | 17 | CONTRACTUAL-BUT-DEFENSIBLE (all `Runtime::new` on a `NotEntered` thread; resource cost only) | all | DEFERRED | async-drop F-004 | responsiveness note for the TUI, outside UB scope |
| UF-034 | Detached tasks with dropped `JoinHandle`s: LSP debounce task (`pending` never pruned, post-close diagnostics) and sessions watcher `spawn_blocking` loop (hangs runtime drop if the receiver outlives the runtime; no in-workspace caller does) | crates/terraphim_lsp/src/server.rs:46-73; crates/terraphim_sessions/src/connector/native.rs:161-249 | 17 (+11 silent panic) | CONTRACTUAL-BUT-DEFENSIBLE | all | DEFERRED | async-drop F-006 | phase1 H open questions 2-3 |
| UF-035 | `std::process::exit` from inside a running runtime (45 sites) skips destructors and kills pool threads mid-operation; every site is on an error path after its own output completed | crates/terraphim_agent/src/{main.rs,server_command.rs,learn_command.rs,robot_dispatch.rs}; crates/terraphim_cli/src/main.rs:437,452,472 | 17 (+11) | CONTRACTUAL-BUT-DEFENSIBLE (stdout flushed by `rt::cleanup`; no `TempDir` live at any site) | all | DEFERRED | async-drop F-007; panic-safety non-finding row | none |
| UF-036 | self_update `set_ssl_vars!()` is a second unannotated env writer in the terraphim_update lib test binary | self_update-0.42.0/src/lib.rs:692 (Linux) reached from crates/terraphim_update/src/lib.rs tests `test_check_for_updates_auto` (non-`#[serial]`) | 7 data races (+ hidden-unsafe-in-safe-wrapper) | SUSPICIOUS | Linux only (`set_ssl_vars!` is cfg(target_os = "linux")); same binary as UF-004 | OPEN | phase5 EXP-010 log (gaiprobe self-test) | UF-004; EXP-010 |
| UF-037 | self_update `set_ssl_vars!()` runs on the PRODUCTION path: `get_latest_release` (self_update github.rs:518, Linux only) is called from the pre-dispatch update check inside `spawn_blocking` while tokio worker threads of the same runtime (and, per UF-033, a second runtime) are live and may call glibc `getenv` (resolver RES_OPTIONS/LOCALDOMAIN via getaddrinfo, SSL_CERT_FILE/SSL_CERT_DIR probes) | crates/terraphim_agent/src/main.rs:448-456 -> crates/terraphim_update/src/manifest.rs -> self_update-0.42.0/src/backends/github.rs:518 | 7 data races (hidden unsafe in dependency, safe wrapper) | LIKELY-UB (Linux shipped binary; outside the test-only archetype of every other row) | Linux | NO_EVIDENCE (EXP-105 on bigbox: 400/400 GitHub-backend iterations fired both setenvs with 0 overlaps; default R2 backend never enters self_update) LIKELY-UB by contract only | escalated by the Phase 8 architect 2026-09-12; no dynamic evidence yet | UF-004; UF-033; UF-036; EXP-102; round 2 must probe the shipped binary with the envprobe LD_PRELOAD shim on bigbox |
| UF-038 | terraphim-session-analyzer builds its own Aho-Corasick automaton (patterns/matcher.rs:97-120) without rejecting empty pattern strings; an empty pattern in the user tools.toml yields zero-width matches at non-char boundaries and matcher.rs:143 panics | crates/terraphim-session-analyzer/src/patterns/matcher.rs:97-120,143 (path corrected by Phase 10: there is no `src/matcher.rs`; `src/patterns/matcher.rs` is the file, as phase8 D2.3 already cites) | 15 panic safety (safe-Rust panic, not UB) | NOT-UB (robustness) | all | NO_EVIDENCE for UB (EXP-103: panic confirmed at matcher.rs:143 with an empty pattern on non-ASCII text; robustness fix) | EXP-103 (mechanism confirmed in phase6_raw/structprobe/acempty.log) | Phase 6 round 1 idea I-08; closes phase1_notes/I.md open question 1; remediation: reject empty keys at load time as terraphim_automata::find_matches does |
| UF-039 (severity raised to CONFIRMED-UB by EXP-104, 2026-09-12; see Lead addendum 4) | fff-search 0.8.4 mmaps every 16 KiB-10 MiB file during every workspace grep (types.rs:625-646) regardless of enable_mmap_cache; its SAFETY argument relies on the background watcher to invalidate mappings, but all four workspace call sites pass watch: false; grep validates the mapping once (grep.rs:1685) then builds lines with from_utf8_unchecked (grep.rs:1710), so a concurrent in-place writer yields a String with invalid UTF-8 through the safe API | crates/terraphim_grep/src/hybrid_searcher.rs:354-359; crates/terraphim_mcp_server/src/lib.rs:1274,1372,1485; fff-search-0.8.4 types.rs:625-646, grep.rs:1685-1710 | 1 aliasing / 3 validity (dependency-inherited, hidden unsafe behind a safe API) | LIKELY-UB; CONFIRMED_UB (EXP-104; lead addendum 4). Mechanism note added by Phase 10: the `from_utf8_unchecked` at grep.rs:1710 named in the title cell is Fuzzy-mode only and is NOT the workspace mechanism; the PlainText path yields invalid `String`s through `from_utf8_lossy(..).into_owned()` over the live mapping, and the root UB is the `Mmap::map` aliasing contract | all (page-size gate differs: 16 KiB aarch64, 4 KiB Linux) | CONFIRMED_UB (EXP-104) | EXP-104 | Phase 6 round 1 idea I-2x; UF-017 rationale refined below; watch: false is a workspace config choice, which would make this a new shape (dependency unsafe defeated by consumer config) |
| UF-040 (CONFIRMED-UB by EXP-201, 2026-09-12) | Hostile non-UTF-8 filenames on Linux make fff-search's lossy path (`to_string_lossy`) inflate past PATH_BUF_SIZE=4096 so `read_to_buf` truncates mid-U+FFFD and returns an invalid `str` via `from_utf8_unchecked` (fff-search simd_path.rs:186); base path is attacker-chosen via `--paths` and the MCP path argument | crates/terraphim_grep/src/hybrid_searcher.rs (search_code -> fff-search collect_files); crates/terraphim_mcp_server/src/lib.rs grep_files; fff-search-0.8.4 simd_path.rs:186 | 3 validity (dependency-inherited, hidden unsafe) | LIKELY-UB; CONFIRMED_UB (EXP-201; lead addendum 5) | Linux | CONFIRMED_UB (EXP-201: 9 invalid-UTF-8 path Strings through the safe fff-search API across hostile geometries, control 0, plus 1 dir-name panic; run in an OrbStack Linux container because bigbox was full) | EXP-201 | Phase 6 round 2; same hostile tree as UF-039 |
| UF-041 | Hostile YAML frontmatter reaches unsafe-libyaml 0.2.11 (c2rust translation) through serde_yaml 0.9 in the command markdown and shared-learning parsers; never run under Miri | crates (command md parser, shared-learning parser; see EXP-202 for file:line) | 3 validity / 1 aliasing (dependency-inherited) | SUSPICIOUS | all | NO_EVIDENCE (EXP-202: Stacked Borrows axis clean over the hostile corpus, 43 inputs, exit 0, no diagnostic; Tree Borrows on bigbox covered 34 of 43 inputs incl. the 52 KB folded case with no diagnostic before hitting the time limit. Migration off the unmaintained serde_yaml 0.9 tracked as #317 on hygiene grounds, not on evidence of UB) | EXP-202 | Phase 6 round 2; serde_yaml 0.9 is deprecated, migration is a Phase 8 note regardless |
| UF-042 | Attacker-influenced manifest version string is parsed by semver 1.0.28 (inline SSO unsafe in identifier.rs) BEFORE the zipsign signature gate (parse at terraphim_update lib.rs:264/312, verify at :359/783) | crates/terraphim_update/src/lib.rs:264,312,359,783 | 3 validity (dependency-inherited) | SUSPICIOUS | all | NO_EVIDENCE (EXP-203: 308 crafted versions incl. 64 KB strings and the SSO boundary sizes, 0 panics, no Miri diagnostic, admissible exit 0 on the Stacked Borrows axis; Tree Borrows axis aborted four times, twice on the 16 GB audit host and twice more on bigbox at 110/48.2/54.4/43.7 GB, and is outstanding; per addendum 13 the memory curve is independent of the corpus cap, so the harness must stop accumulating values or run one process per input) | EXP-203 | Phase 6 round 2; defence in depth: verify before parse |
| UF-043 | `parse_correction_pattern` applies byte offsets computed on a lowercased/trimmed copy to the source string: panics on non-ASCII prompts and silently corrupts ASCII-whitespace input (confirmed in scratch) | see EXP-204 for file:line | 15 panic safety (safe-Rust panic, not UB) | NOT-UB (robustness) | all | NO_EVIDENCE for UB (EXP-204: 3 panics and 7 wrong tuples of 20 inputs, corroborated on the installed release binary via learn hook) | EXP-204 | Phase 6 round 2; cannot feed an unsafe boundary |
| UF-044 | fff-search case-insensitive memmem AVX2 kernels (`verify_avx2`, `search_packed_pair_avx2`) reached by every PlainText grep on x86_64 hosts have no upstream test above 26-byte needles and have never run under a sanitiser | fff-search-0.8.4 case_insensitive_memmem.rs:107-170,355,376-470; grep.rs:1281 | 2 out-of-bounds (dependency-inherited SIMD unsafe) | SUSPICIOUS | x86_64 only | NO_EVIDENCE (EXP-301 resolved on bigbox: 51,215 cases, `total_divergences=0`, zero ASan reports, aarch64 NEON fingerprint byte-identical; status corrected by Phase 10, the DEFERRED token predated the run) | EXP-301 | Phase 6 round 3 idea I-301; the Phase 11 campaign is no longer required for this row |

Counts (recounted by the Phase 10 fresh-eyes reviewer, 2026-09-12; the figures below were written when the table held 35 rows and were never updated as lead addenda 1 and 3 to 6 appended UF-036 to UF-044): 44 unified findings. The original 35 came from 42 sources (38 per-bucket F-blocks: data-races 13, send-sync 1, panic-safety 6, library-trait 8, async-drop 7, phase3 3; plus the Miri crossbeam-epoch report, the perf RCA, phase1 G open question 1 and the phase1 J advisory table) with 7 merges; UF-036 to UF-044 were added later from Phase 5 rounds 2 and 3 and Phase 6 rounds 1 to 3. Severity split over all 44 rows (taking UF-004 as raised to LIKELY-UB by lead addendum 1, and UF-039/UF-040 as raised to CONFIRMED-UB by addenda 4 and 5): LIKELY-UB 6, SUSPICIOUS 12, CONTRACTUAL-BUT-DEFENSIBLE 24, NOT-UB 2. Status split: CONFIRMED_UB 4 (UF-001, UF-002, UF-039, UF-040), NO_EVIDENCE 5 (UF-005, UF-037, UF-038, UF-043, UF-044), OPEN 14, DEFERRED 21.

### Dedup decisions

| Merged into | Source ids | Reason |
|---|---|---|
| UF-001 | data-races F-001 + phase3 F-102 | same writer site and the reader chain Phase 2 predicted |
| UF-002 | data-races F-002 + phase3 F-103 | same writer; F-103 is the reduction of F-002's mechanism |
| UF-008 | data-races F-013 + async-drop F-002 | same bridge at store.rs:608-612 |
| UF-011 | panic-safety F-003 + library-trait F-007 | same handler.rs/procedure.rs/main.rs/model.rs sites |
| UF-018 | data-races F-011 + panic-safety F-004 | same `KG_CACHE` guard |
| UF-019 | data-races F-010 + async-drop F-005 | same guard-across-await sites; async-drop adds `chat` at service.rs:555-590 |
| UF-029 | library-trait F-008 + perf RCA | same symptom; the RCA identifies the mechanism |

Kept separate on purpose: UF-012 (phase3 F-101) shares UF-001's writer but has a different reader and a different platform verdict; UF-020 and UF-021 are the same registry with different failure kinds (deadlock vs torn state); UF-003 and UF-014 are the wrapper and one of its consumers.

## Top 5 by severity

**Phase 10 correction (2026-09-12).** This list was written before UF-039 and UF-040 existed and was never revised. On the evidence as it now stands the order is UF-039, UF-040, UF-001, UF-002, UF-004: UF-039 and UF-040 are the only CONFIRMED_UB rows on a **production** path (terraphim-grep `search_code`, the three MCP tools), whereas UF-001 and UF-002 are confirmed in test binaries. The original list is kept below unchanged for the record.

1. UF-001 (CONFIRMED_UB on the Linux CI platform, #280).
2. UF-002 (mechanism CONFIRMED_UB; shipped binary safe only by test ordering).
3. UF-003 (the unsound wrapper behind UF-002; fix lands in the `terraphim_test_utils` registry crate).
4. UF-004 (same shape as UF-001 in terraphim_update, never probed; EXP-010).
5. UF-005 (dependency Stacked Borrows report; not cleared by the planned bump; EXP-004).

## Miri matrix (EXP-003a to EXP-003f, EXP-004)

| Crate | default (Stacked Borrows) | tree-borrows | strict-provenance | symbolic-alignment | Tests run / filtered | Notes |
|---|---|---|---|---|---|---|
| terraphim_negative_contribution | clean, 40 | clean, 40 | clean, 40 | clean, 40 | 40 / 0 | EXP-003a NO_EVIDENCE |
| terraphim_hooks | clean, 39 | clean, 39 | clean, 39 | clean, 39 | 39 / 4 (`discovery`, `latency`) | EXP-003b NO_EVIDENCE |
| terraphim_command_runtime | clean, 13 checks | clean, 13 checks | clean, 13 checks | clean, 13 checks | 13 / 13 | **Superseded 2026-09-13 (addendum 13):** the "0 tests / not run" state that made EXP-003c NEEDS_REFINEMENT is resolved. A purpose-built harness (experiments/EXP-003c) drives four of the five public helpers over nine query shapes plus a clone and lock round trip, with `save_config` excluded because it writes to the real persistence backends. All four axes clean, captured exit 0. EXP-003c NO_EVIDENCE |
| terraphim_sessions | clean, 50 | clean, 50 | clean, 50 | clean, 50 | 50 / 36 (native, cursor, service, aider tempdir: `kqueue`/rusqlite FFI) | EXP-003d NO_EVIDENCE |
| terraphim_grep | clean, 34 | clean, 34 | clean, 34 | clean, 34 | 34 / 23 (openrouter, hybrid_searcher, six I/O tests) | EXP-003e NO_EVIDENCE; the filtered tests are exactly the UF-001 surface |
| terraphim-session-analyzer | crossbeam-epoch SB report on rayon tests (UF-005); 121 clean with 6 rayon tests skipped; 80-test subset clean | 128 clean, then Miri leak-check error (rayon pool threads outlive main) plus 2 int-to-ptr warnings in crossbeam-epoch | clean, 80 (completed; the "running" note was stale) | clean, 73 of 128 (proptest modules also skipped; see lead addendum 1) | varies by axis: 73 / 80 / 128 | EXP-003f NO_EVIDENCE (registry block; the OPEN token here predated the lead's flip) with the subset caveat below; EXP-004 DEFERRED; #264 not in the subset. **Superseded 2026-09-13 by Phase 11 campaign C (addendum 13):** every axis now covers the same **123 of 128** tests (default 80+43, strict-provenance 80+43, symbolic-alignment 73+50, tree-borrows 128). The 5 uncovered on the three Stacked-Borrows-based axes are the same 5 every time; all drive `analyze`, whose `par_iter` starts rayon's global pool, after which crossbeam-epoch 0.9.21 either violates Stacked Borrows at internal.rs:567 or performs an integer-to-pointer cast at atomic.rs:204 that strict-provenance refuses. Every erroring frame is a dependency frame. Tree Borrows covers all 128 because it does not flag the pattern |

No Miri report anywhere names a workspace frame.

## Closed buckets (NO_SURFACE, evidence in the named file)

- `phase2_findings_aliasing.md`: bucket 1, `rg 'UnsafeCell|\*const |\*mut |as_ptr\(\)|as_mut_ptr'` over crates/ = 0 hits.
- `phase2_findings_provenance.md`: bucket 2, int-to-ptr casts, `from_raw_parts`, `expose_provenance` = 0.
- `phase2_findings_alignment.md`: bucket 3, `repr(packed`, `read_unaligned`, `align_to` = 0.
- `phase2_findings_validity.md`: bucket 4, `transmute`, `from_u32_unchecked`, `new_unchecked`, `assume(` = 0.
- `phase2_findings_uninit.md`: bucket 5, `MaybeUninit`, `mem::zeroed`, `set_len(` = 0.
- `phase2_findings_type-punning.md`: bucket 6, `transmute`, bytemuck, zerocopy = 0.
- `phase2_findings_pin.md`: bucket 9, `Pin::new_unchecked`, `get_unchecked_mut`, `PhantomPinned` = 0.
- `phase2_findings_ffi.md`: bucket 10, `extern "C"`, `#[no_mangle]`, `#[link(`, bindgen, cxx = 0.
- `phase2_findings_refcount.md`: bucket 13, `Arc::from_raw`/`into_raw`, strong-count intrinsics = 0.
- `phase2_findings_const-mut-write.md`: bucket 14, `as *mut`, `cast_mut()` = 0.
- `phase2_findings_lifetimes-escape.md`: bucket 15, `transmute::<&`, `Box::leak`, `mem::forget` = 0.
- `phase2_findings_volatile.md`: bucket 16, `read_volatile`/`write_volatile`, `core::intrinsics` = 0.
- `phase2_findings_inline-asm.md`: bucket 18, `asm!`, `global_asm!`, naked = 0.
- `phase2_findings_target-feature.md`: bucket 19, `target_feature`, feature detection, `core::arch` = 0.
- `phase2_findings_box-allocator.md`: bucket 20, 10 hits all `ratatui::layout::Layout::default()`, zero allocator-API sites.
- `phase2_findings_ffi-callback.md`: bucket 21, `extern "C" fn` = 0 (the only candidate, UF-031, lives in the `notify` dependency).
- `phase2_findings_repr-packed.md`: bucket 22, `repr(packed` = 0.
- `phase2_findings_invalid-ref-cast.md`: bucket 23, `&T as *const T as *mut`, `cast_mut()` = 0.
- `phase2_findings_coherence.md`: bucket 24, specialization, `#[rustc_` = 0.
- `phase2_findings_hash-eq.md`: bucket 25, manual `Hash`/`PartialEq`/`Eq`/`Ord`/`PartialOrd`/`Borrow` impls = 0 (corroborated by the library-trait sweep).

`phase2_findings_rustonomicon.md` (24 anti-pattern rules) reported 0 hits and is corroboration, not a bucket.

## Safe-by-construction items (no experiment allocated)

UF-012, UF-013, UF-014, UF-018, UF-020, UF-021, UF-022, UF-030, UF-031, UF-032, UF-033, UF-034, UF-035 are DEFERRED with the static rationale recorded in their rows and in the source bucket files. Future rounds should not re-investigate them unless the cited lines change. UF-016 and UF-017 are DEFERRED on the lockfile bump; UF-005 on EXP-004 after the bump.

## Non-UB defects routed elsewhere

These were found by the audit but are functional, correctness or hygiene defects, not undefined behaviour. They belong in the Phase 8 remediation plan or in their own Gitea issues, not in the UB registry.

- **Rollback truncates the running executable in place** (UF-006; terraphim_update lib.rs:1536, rollback.rs:310; callers terraphim_cli main.rs:944 and repl/handler.rs:1852). Fails with `ETXTBSY` on Linux, leaves a truncated binary if interrupted on macOS. Fix shape: copy to a sibling temp file, fsync, rename, as `promote_staged_binaries` already does. No issue yet; file one under the #252 umbrella.
- **Nested Runtime panic in `--server sessions`** (UF-007; server_command.rs:751-752). Deterministic `Cannot start a runtime from within a runtime`; no robot JSON envelope on the error path. Fix: `.await` directly as main.rs:1593 does. Needs a `server` feature CI lane to be visible. No issue yet.
- **String byte-slicing panics** (UF-009, UF-010, UF-011, UF-028; sites listed in the table). Safe-Rust panics on non-ASCII input; two are on production paths (`sessions search`, REPL enrichment). Fix: `cli_helpers::truncate_snippet` or `floor_char_boundary` at every site. EXP-012 provides the regression inputs. No issue yet.
- **Rank non-determinism** (UF-029). terraphim/terraphim-ai#3376: `init_or_update_edge` over-counts non-creator documents by one per shared edge; insertion order comes from AHashMap iteration. Fix A (`or_insert(0)`) plus a secondary sort key on document id. Lives in the registry crates, not this workspace.
- **learn_no_service_tests CLI drift**. terraphim/terraphim-clients#278: the test passes a `--original` flag the CLI no longer accepts. Test hygiene, unrelated to memory safety.
- **Leaked `terraphim_server` child processes from integration tests**. Workspace evidence: `extract_functionality_validation.rs:45` keeps the server `Child` in a `static Mutex<Option<Child>>` and never kills it; `kg_ranking_integration_test.rs:207-213` drops the `Child` on purpose so the server outlives the binary; `shell_dispatch.rs` spawns without `kill_on_drop` (phase1_notes/C.md item 4, B.md). The team lead reports leaked server processes observed on the CI host (bigbox), consistent with those sites. Cross-process contention on the default sqlite store (UF-015, UF-023) is the same family.
- **Codex `test_parse_response_item` fails under workspace feature unification** (terraphim/terraphim-clients#264). Not part of the Miri subset; recorded so the session-analyzer matrix is read with that gap in mind.
- **Dependency advisories that are DoS-class, not UB**: rmcp 0.9.1 DNS rebinding (#256), quick-xml 0.37/0.38 (#258), h2 0.4.15 (#254). Licence: html2md GPL-3.0+ via terraphim_middleware (terraphim/terraphim-ai#3374).

## Per-finding anchors for the convergence tracker

`convergence-tracker.sh` and `validate-phase.sh` snapshot findings by grepping `^## F-[0-9]+` in this file, so each UF row is repeated here as a one-line heading with the same number (F-001 is UF-001). Details stay in the master table above.

## F-001: UF-001 terraphim_grep lib tests mutate `OPENROUTER_BASE_URL` while libgit2 `getenv` runs on sibling test threads

## F-002: UF-002 `EnvVarGuard` in tui_service_tests mutates HOME/XDG_CONFIG_HOME/TERRAPHIM_DATA_PATH while sqlite `unixTempFileInit` may `getenv` on a sibling thread

## F-003: UF-003 `terraphim_test_utils` SAFETY rule ("use #[serial]") cannot satisfy its own precondition in a mixed binary; safe `pub fn` over `unsafe set_var`

## F-004: UF-004 terraphim_update `setup_temp_config_dir` sets five env vars under a false SAFETY comment; 120 non-serial siblings; resolver reader not ruled out; HOME left dangling

## F-005: UF-005 crossbeam-epoch 0.9.18 Stacked Borrows report in `Local::element_of` on rayon worker threads (Miri, session-analyzer)

## F-006: UF-006 Rollback truncates the running executable in place (`fs::copy` over `current_exe()`)

## F-007: UF-007 Nested `Runtime::new` + `block_on` inside async `run_server_command` panics deterministically (`--server sessions ...`)

## F-008: UF-008 `shared_learning::store::block_on` bridge (`block_in_place` + `Handle::current().block_on`) panics on current_thread runtimes and with no runtime; contract undocumented on a public sync trait impl

## F-009: UF-009 Enricher byte-window context extraction slices mid-character

## F-010: UF-010 `Session::summary` cuts the first user message at byte 100

## F-011: UF-011 Fixed-byte display truncations on input-controlled strings (titles, message content, shell commands, procedure steps)

## F-012: UF-012 macOS-only native `getenv` reader (SystemConfiguration / libsystem_trace via hyper-util `Matcher::from_system`) overlaps the grep writer on a sibling test thread

## F-013: UF-013 `test_mcp_stdio` sets env vars with no SAFETY comment; sound only because it is a one-test binary

## F-014: UF-014 `terraphim_test_utils::set_env_var` sites in the two serialised mcp_server binaries; invariant enforced one level away from the mutation

## F-015: UF-015 Seven unserialised mcp_server integration binaries spawn the real server without `TERRAPHIM_PROFILE_MEMORY_TYPE`; may share `~/.terraphim/sqlite/terraphim.db` across up to seven child processes

## F-016: UF-016 event-listener 5.4.1 unsound `Send`/`Sync` for `StackSlot` (RUSTSEC-2026-0221) reaches five crates through sqlx via opendal; unsound path (`Event::with_tag` + `listener!`) unused by sqlx-core

## F-017: UF-017 Other UB-class advisories in the resolved graph: anyhow 1.0.102 (RUSTSEC-2026-0190, `downcast_mut`, 0 workspace uses), lru 0.18.0 (0253, UAF only if a value's Drop panics; workspace has zero `impl Drop`), memmap2 0.9.10 (0186), git2 0.20.4 (0183/0184, `Remote::list`/`BlameHunk` not called)

## F-018: UF-018 Global `KG_CACHE` std `RwLock` held across filesystem I/O on a tokio worker; poison is fail-open (`.ok()?`)

## F-019: UF-019 Tokio lock guards held across awaits into persistence, search and an LLM HTTP call (stall surface; consistent lock order `service` -> `config`, no inversion)

## F-020: UF-020 `CommandRegistry` readers acquire `aliases`/`commands` in opposite orders under a write-preferring tokio RwLock

## F-021: UF-021 `CommandRegistry::reload`/`register_command` apply multi-lock updates in separate scopes; a task panic leaves aliases and commands mutually inconsistent

## F-022: UF-022 `maybe_auto_import` check-then-act allows duplicate imports

## F-023: UF-023 `DEVICE_STORAGE` singleton binds the sqlite data dir to whichever HOME/TERRAPHIM_DATA_PATH is live at first init; tests may write into the real `~/.terraphim` or into a deleted TempDir

## F-024: UF-024 Sibling openrouter tests observe `OPENROUTER_BASE_URL` mid-mutation (std-locked logical race); restore skipped if the writer's assertion panics

## F-025: UF-025 KG-boost comparator is total only while no `RetrievedChunk.relevance_score` is NaN (`partial_cmp(..).unwrap_or(Equal)`); NaN concept scores are masked by the `f64::min` cap

## F-026: UF-026 `store_with_dedup` best-match uses `partial_cmp(..).unwrap()` with no NaN filter; NaN derivable from an all-empty corpus (`0.0/0.0` in BM25 length norm)

## F-027: UF-027 Markdown frontmatter parse accepts `importance_total: NaN` and feeds the `list_all_entries` sort

## F-028: UF-028 Paragraph finder mixes char indices with byte offsets (`chars().nth(byte_pos)`, `pos + 20` unaligned)

## F-029: UF-029 Rolegraph document rank depends on AHashMap insertion order: the first document to create an edge is counted one less than every later document on that edge (`or_insert(1)` then `+= 1`)

## F-030: UF-030 Windows promotion window: `remove_file` then `rename` leaves no binary if killed in between; staging dir orphaned

## F-031: UF-031 `notify` FSEvents `extern "C"` callback hosts a workspace closure (`mpsc::Sender::send`, no panic path); the dependency's own `expect`s abort on a non-UTF-8 path

## F-032: UF-032 Update scheduler builds and tears down a fresh multi-thread `Runtime` per tick inside `spawn_blocking`; shutdown of the outer runtime stalls for the duration of an in-flight check

## F-033: UF-033 Two long-lived multi-thread runtimes coexist on the main thread (start-up update check plus REPL/command/TUI runtime); TUI loop blocks the UI thread per backend call

## F-034: UF-034 Detached tasks with dropped `JoinHandle`s: LSP debounce task (`pending` never pruned, post-close diagnostics) and sessions watcher `spawn_blocking` loop (hangs runtime drop if the receiver outlives the runtime; no in-workspace caller does)

## F-035: UF-035 `std::process::exit` from inside a running runtime (45 sites) skips destructors and kills pool threads mid-operation; every site is on an error path after its own output completed
## F-036: UF-036 self_update set_ssl_vars!() second unannotated env writer in the terraphim_update lib test binary
## F-037: UF-037 self_update set_ssl_vars!() on the production update-check path (Linux)
## F-038: UF-038 session-analyzer empty Aho-Corasick pattern char-boundary panic
## F-039: UF-039 fff-search live mmap with watcher disabled and from_utf8_unchecked line construction
## F-040: UF-040 fff-search lossy filename inflation past PATH_BUF_SIZE with from_utf8_unchecked
## F-041: UF-041 hostile YAML into unsafe-libyaml via serde_yaml 0.9
## F-042: UF-042 semver parse of manifest version before the zipsign gate
## F-043: UF-043 parse_correction_pattern byte-offset mismatch panic
## F-044: UF-044 fff-search AVX2 memmem kernels untested under sanitisers

## Experiment registry summary

| EXP | Finding | Verdict |
|---|---|---|
| EXP-001 | UF-001 | NEEDS_REFINEMENT (TSan blind to the pair on both platforms) |
| EXP-001b | UF-001 | CONFIRMED_UB |
| EXP-002 | UF-002 | NEEDS_REFINEMENT |
| EXP-002b | UF-002 | NO_EVIDENCE (ordering caveat) |
| EXP-002c | UF-002 | CONFIRMED_UB (mechanism) |
| EXP-003a to EXP-003e | Miri matrix | NO_EVIDENCE x4, NEEDS_REFINEMENT (command_runtime, 0 tests) |
| EXP-003f | Miri matrix, session-analyzer | NO_EVIDENCE, with the differing-test-set caveat in the Miri matrix row above |
| EXP-004 | UF-005 | DEFERRED (re-run SB axis after the crossbeam-epoch bump) |
| EXP-010 | UF-004 | DEFERRED in the registry; NEEDS_REFINEMENT in its own Phase 5 note (see the Phase 10 note under this table) |
| EXP-011 | UF-025, UF-026, UF-027 | NO_EVIDENCE (panic confirmed) |
| EXP-012 | UF-009, UF-010, UF-011, UF-028 | NO_EVIDENCE (panic confirmed) |
| EXP-013 | UF-008 | NO_EVIDENCE (panic confirmed) |
| EXP-014 | UF-003 | NO_EVIDENCE (hypothesis confirmed) |
| EXP-015 | UF-015 | NO_EVIDENCE (test hygiene) |
| EXP-101 | UF-005 | NO_EVIDENCE |
| EXP-102 | UF-036 | DEFERRED |
| EXP-103 | UF-038 | NO_EVIDENCE (panic confirmed) |
| EXP-104 | UF-039 | CONFIRMED_UB |
| EXP-105 | UF-037 | NO_EVIDENCE (instrument blind to the predicted glibc-internal reader; see the Phase 10 caveat below) |
| EXP-201 | UF-040 | CONFIRMED_UB |
| EXP-202 | UF-041 | OPEN (a log exists at phase5_experiment_results/EXP-202.log; no result is recorded in the block) |
| EXP-203 | UF-042 | OPEN (a log exists; `MIRI_EXIT=` is empty in it, so the run is not admissible as recorded) |
| EXP-204 | UF-043 | NO_EVIDENCE (panic confirmed) |
| EXP-205, EXP-206, EXP-207 | hostile-input probes | DEFERRED (blocked on bigbox disk and an ASan corpus, not safe-by-construction) |
| EXP-301 | UF-044 | NO_EVIDENCE |
| EXP-302 | UF-039 blast radius | CONFIRMED_UB (bigbox pass 2, 2026-09-12) |
| EXP-801 to EXP-806 | post-remediation proofs | DEFERRED (gated on Phase 13) |

**Phase 10 additions to this table.** The rows from EXP-101 down were appended by the fresh-eyes reviewer on 2026-09-12; the table previously stopped at EXP-015 and still showed the pre-Phase-5 tokens for EXP-010 to EXP-015. Verdicts are copied from the registry blocks, not re-derived. Two tokens in the table above disagree with the registry and are recorded here rather than silently changed: EXP-001 and EXP-002 read NEEDS_REFINEMENT here and DEFERRED in the registry (both are superseded by EXP-001b/EXP-002b/002c, so nothing turns on it), and EXP-010 reads OPEN here, NEEDS_REFINEMENT in its own Phase 5 note and DEFERRED in its verdict line.

## Lint output

`python3 ~/.claude/skills/rust-undefined-behavior-exorcist/scripts/lint-experiment-designs.py UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md`, run 2026-09-12 after the final edit:

```
[OK] UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md — all blocks well-formed
exit status: 0
```


## Lead addendum (2026-09-12, after Phase 5 round 1)

- UF-036 added (self_update SSL env writer); UF-004 raised to LIKELY-UB on the documented-reader-plus-demonstrated-window argument; the experiment stays DEFERRED pending helgrind on a host that has it.
- Miri matrix caveat for session-analyzer: default and strict-provenance ran the 80-test subset (rayon and cursor modules skipped); tree-borrows ran 128 (rayon tests included, clean); symbolic-alignment was first attempted on the 80-test subset and aborted after 13 clean tests because the proptest modules are impractically slow under that axis (log kept as miri_symbolic_alignment_terraphim_session_analyzer_full_attempt_aborted.log), then re-run with the proptest modules also skipped: 73 passed, 0 failed (55 filtered out), clean. The cells were therefore not the same test set per axis when this note was written. **Superseded 2026-09-13 by campaign C**, which brought every axis to the same 123 of 128 tests by running each delta test in its own Miri process (a batch run aborts on the first failure, so one uncoverable test hid everything behind it). See addendum 13.

## Lead addendum 2 (2026-09-12, after Phase 8)

- Scoping for UF-001 and UF-002: both CI test lanes run cargo-nextest (one process per test, ci.yml:57 and native-ci.yml:73), so the in-process cross-test race cannot occur in those lanes. Both coverage lanes run plain `cargo llvm-cov --workspace --lib` in-process on glibc with `--ignore-run-fail` (ci.yml:79, native-ci.yml:94), so the grep and update lib test binaries ARE exposed there and a crash would not turn the lane red; BUILD.md's canonical `cargo test` is exposed too. The earlier "both CI runners" wording is superseded by this scoping. The regression instrument must run the libtest binary directly, never through nextest.
- UF-037 added: the self_update SSL env writer is reached on the production update-check path on Linux, which is the first row outside the test-only archetype. It is OPEN and is the priority probe for convergence round 2. It also coincides with the top entry of the #253 opportunity matrix (gating the pre-dispatch update check), so remediation and the perf lever may be one change.

## Lead addendum 3 (2026-09-12, after Phase 6 round 1)

- UF-038 and UF-039 added from EXP-103 and EXP-104.
- UF-017 (memmap2 advisory) rationale is refined: it is not "no reachable call path". The mapping is live in every grep via fff-search; the advisory-specific functions (advise_range, unchecked_advise_range, flush_range, flush_async_range) are the ones not called, and the lockfile is now at memmap2 0.9.11. The row's verdict stands; the exposure is recorded under UF-039.

## Lead addendum 4 (2026-09-12, after Phase 5 round 2)

- UF-039 is CONFIRMED_UB and is the first confirmed finding on a production path (terraphim-grep `search_code` and the MCP `grep_files` tool). Correction to the mechanism as filed: the `from_utf8_unchecked` at fff-search grep.rs:1710 is only reached by `GrepMode::Fuzzy`, which no workspace call site uses. The workspace PlainText path still returns Strings holding invalid UTF-8 because `from_utf8_lossy(..).into_owned()` validates and then copies in two separate reads over the live mapping (440 invalid of 186 M lines with a sibling in-place writer; 2,039 of 93 M under ASan). The truncate arm kills the process with SIGBUS in both modes (ASan symbolises it in `memchr::One::find_raw` under `SliceByLine<PlainTextMatcher, PlainTextSink>::run` from `perform_grep`). The root UB is the `memmap2::Mmap::map` aliasing contract (the mapped file must not be modified concurrently) being relied on by a SAFETY argument that assumes the watcher invalidates mappings, while every workspace call site passes `watch: false`. A fix that only swaps the unchecked conversion does not close the finding; the remediation must stop mapping files that can change under the reader (read into an owned buffer, or upstream fff-search must honour its own `enable_mmap_cache` / watcher precondition). Phase 8 plan needs a delta for this row.
- UF-037 narrows: the shipped binary dispatches on `UpdateBackend` (default R2, ureq, never enters self_update). `set_ssl_vars!()` fires only with `TERRAPHIM_UPDATE_BACKEND=github` or the explicit update commands. EXP-105 fired the writer in 400 of 400 GitHub-backend iterations with zero overlaps; verdict NO_EVIDENCE, row stays LIKELY-UB by contract. The envprobe shim sees only PLT `getenv`, not glibc-internal reads inside `getaddrinfo` (carried from EXP-010).
- UF-038: panic confirmed, NOT-UB, robustness fix (reject empty keys at load time).

## Lead addendum 5 (2026-09-12, after Phase 6 round 2)

- UF-040 to UF-043 added from EXP-201 to EXP-204. EXP-205/206/207 (hostile git repo into libgit2, crafted SQLite file into the Cursor connectors, FFF_FRECENCY_PATH into heed/LMDB) are DEFERRED on ASan-on-bigbox corpora and stay tracked in the registry without UF rows until they produce evidence.
- Correction to phase1_notes/J.md: the libgit2-sys bump is lockfile-only. git2 0.20.4 accepts ^0.18.3 and crates.io has libgit2-sys 0.18.8+1.9.7, so `cargo update -p libgit2-sys --precise 0.18.8+1.9.7` moves the C library to libgit2 1.9.7 with no fff-search change. **This claim was wrong and is retracted (see Lead addendum 10):** that bump does not clear RUSTSEC-2026-0183/0184, which are advisories against the git2 *crate*, patched only in git2 >= 0.21.0. #257 is the blocker after all. The SQLite bump needs rusqlite 0.34+ (0.32 pins libsqlite3-sys 0.30 = SQLite 3.46.0); no SQL-text CVE is reachable because every query is a literal.

## Lead addendum 6 (2026-09-12, after Phase 6 round 3)

- The round 3 idea-wizard agent hit the account session limit after writing phase6_idea_wizard_round_3.md but before appending its two blocks and the rollup. The lead wrote EXP-301 and EXP-302 from the round report (both DEFERRED: x86_64 host and bigbox disk) and UF-044. EXP-302 measures blast radius of the already-confirmed UF-039 in the long-lived SSE server; it does not hold convergence open. Round 3 refinements adopted: EXP-104 exposure on CI is bigbox only (I-327); EXP-207 surface is open-only (I-309); the Phase 8 zero-file budget closes every mapping path (I-304); ASan admissibility applies to every bundled-C experiment (I-303).


## Phase 10 fresh-eyes addendum (2026-09-12, SOLO review)

Written by the Phase 10 fresh-eyes reviewer. Flat errors in the body above were corrected in place and are listed in `phase10_fresh_eyes_log.md`; the items here change how a verdict should be read and are recorded rather than applied, because they belong to the agents who wrote them.

- **EXP-105 / UF-037: NO_EVIDENCE was recorded on an instrument this file's own definition excludes.** "NO_EVIDENCE" is defined at the top of this document as "an experiment **able to see the predicted signal** ran and stayed clean". EXP-105's own note says the LD_PRELOAD shim sees only PLT `getenv` and is blind to the glibc-internal resolver reads that are the predicted reader, exactly the property for which EXP-001 was given NEEDS_REFINEMENT. The two experiments are in the same epistemic position with different tokens. The row's conclusion (the default R2 backend never enters self_update, so there is no writer on the default path) is unaffected and rests on the `setenv=0` count, which the shim *can* see; only the "0 overlaps" arm is instrument-limited. Recommended token for the overlap arm: NEEDS_REFINEMENT, with the `setenv=0` default-path result kept as NO_EVIDENCE.
- **Four DEFERRED rows are OPEN experiments blocked on infrastructure, not DEFERRED by this file's definition.** DEFERRED here means safe-by-construction, awaiting a lockfile bump, or routed elsewhere. EXP-010-a (helgrind absent on bigbox), EXP-205, EXP-206 and EXP-207 (ASan corpus plus bigbox disk) and EXP-302 (bigbox disk) meet none of those; they are unrun experiments with a resource gate. This matters because the phase's exit criteria treat DEFERRED as closed and OPEN as outstanding. EXP-801 to EXP-806 are a fifth category again (not runnable until Phase 13 lands the fix) and are honestly labelled, but the taxonomy has no token for them.
- **EXP-003c carried NO_EVIDENCE over a vacuous run.** Zero tests executed, so the instrument could not see the signal by construction; the block's own Falsifiability line says "neither supports nor refutes". Corrected in the registry to NEEDS_REFINEMENT, which is what the Miri matrix row in this file always said.
- **The Miri matrix is four partial results.** Symbolic-alignment covered 73 tests, default and strict-provenance 80, tree-borrows 128, and `connectors::cursor` is skipped on every axis. No axis covered the crate. Do not cite EXP-003e against UF-001 either: its 23 filtered tests are, by its own note, exactly the UF-001 surface.

## Lead addendum 7 (2026-09-12, after the bigbox pass)

- UF-044 closes NO_EVIDENCE: EXP-301 exercised both public entry points into the two named AVX2 kernels over 51,215 cases under ASan on x86_64 with no divergence and no report, and the aarch64 NEON run produced a byte-identical result fingerprint. A benign startup ASan `odr-violation` in libgit2's bundled llhttp (transitively via fff-search's git2 dependency, off the memmem path) was suppressed with `detect_odr_violation=0` and is not a memory-safety report.
- EXP-205, EXP-206, EXP-207 and EXP-302 remain DEFERRED and are now **blocked on an operator decision, not on a gate**. Their harnesses, corpus generators and (for EXP-302) a built server binary are prepared under `experiments/` and on bigbox under `/tmp/ubx-r7/`, but the executor session's safety classifier refused to continue once the work turned to generating deliberately corrupted SQLite and LMDB files and a hostile git packfile with an inflated delta result-size varint. The executor correctly stopped rather than working around the refusal, and this lead did not take the blocked work over, which would be the same circumvention by another route. Finishing them needs the user's explicit direction about how and where that hostile-corpus generation should happen.
- EXP-003c was re-opened from NO_EVIDENCE to NEEDS_REFINEMENT during the Phase 10 review: `terraphim_command_runtime` has zero `#[test]` functions, so its Miri pass was vacuous and cannot stand as evidence.

## Lead addendum 8 (2026-09-12, adopting the Phase 10 fresh-eyes findings)

The reviewer's three re-derivations and its corrections are adopted as written. Four points are recorded here because they change how later readers must use this file:

1. **Two gaps no earlier artefact stated.** The UF-039 zero-file cache budget does close every `memmap2::Mmap::map` path in `fff-search` as this workspace uses it (one map site, guarded before it; `warmup_mmaps` is dead upstream because its only caller is commented out). But the process still maps memory elsewhere: LMDB through the frecency tracker and libgit2 through the repository discovery that the same grep performs. Neither is covered by UF-039's remediation, and neither has been probed. They are the natural targets if this audit is ever resumed.
2. **`follow_symlinks` is never set explicitly** at any of the four `FilePicker::new` sites, so the UF-040 guard's soundness depends on an upstream default of `false`. The `ci_guards` test must assert the value rather than inherit it; this is added to Gitea issue #297's acceptance criteria.
3. **NO_EVIDENCE on a blind instrument is not absence of the finding.** UF-037 is the clearest case: EXP-105 closed its overlap arm NO_EVIDENCE while the probe shim, by its own note, cannot see the glibc-internal resolver reads that are the predicted reader. The verdict token stands because the conclusion rests on the `setenv` count arm the shim *can* see, but the final report must state the instrument's blindness next to the verdict, exactly as EXP-001 was given NEEDS_REFINEMENT for the same reason. UF-037 remains the only LIKELY-UB row on a shipped production path.
4. **"DEFERRED" is overloaded in this file.** Five rows marked DEFERRED are open hypotheses blocked on infrastructure (a host, a corpus, an operator decision), not judgements that the question was set aside. The final report must separate "investigated and set aside" from "not yet investigated because the instrument was unavailable", and the latter belongs in the runbook as outstanding work.

## Lead addendum 9 (2026-09-12, stale-row sweep found by the Phase 12 author)

- UF-016 and UF-017 were DEFERRED on a condition that the two lockfile bumps of 2026-09-12 (ae06cd2 and 46bbc43) already met; both are now closed against HEAD.
- EXP-004 was waiting for exactly the post-bump Stacked Borrows re-run that EXP-101 performed; its verdict is corrected in the registry from DEFERRED to NO_EVIDENCE, superseded by EXP-101.
- UF-005's only cross-reference pointed at #264, which is the unrelated codex test failure. The row has no remediation issue because there is nothing in workspace code to remediate; that is now stated in the row.
- Counting convention, recorded once so future readers stop reconciling the wrong things: there are 44 unified findings and 38 experiment blocks. The convergence JSONs count experiment verdicts, not findings, so their totals will never match the findings table.
- The archived env-probe driver ignores the environment overrides the remediation plan assumes and hard-codes its shim path. A maintainer following the plan verbatim would have loaded no shim and read the silent result as clean. The corrected invocation is in UB_RUNBOOK.md; the plan's text should be amended when it is next touched.

## Lead addendum 10 (2026-09-12, retraction: the libgit2 advisories are NOT cleared)

A claim this lead accepted from the Phase 6 round 2 report, repeated in commit 46bbc43's message, in deny.toml and in a comment on Gitea #254, is false and is retracted here.

- **What was claimed:** `cargo update -p libgit2-sys --precise 0.18.8+1.9.7` clears RUSTSEC-2026-0183 and RUSTSEC-2026-0184, so #257 was not the blocker.
- **What is true:** both advisories are against the **git2 crate**, not the libgit2 C library. RUSTSEC-2026-0183 is a null pointer passed to `slice::from_raw_parts` in `Remote::list()` when a remote advertises no references; RUSTSEC-2026-0184 is null `Signature` pointers from a `BlameHunk` created via `Blame::blame_buffer()`. Both are patched in **git2 >= 0.21.0**. The lockfile still carries git2 0.20.4, pulled solely by fff-search 0.8.4, and `cargo audit` still reports both at HEAD.
- **Why `cargo deny check` passes anyway:** both are `informational = "unsound"`, and this deny.toml treats unsound-class advisories as informational rather than denials. A passing `cargo deny` was mistaken for the advisories being gone. That is the same error class the Phase 10 review named: a clean result from an instrument that was not looking for the thing.
- **The libgit2-sys bump was still harmless and is kept** (libgit2 1.9.4 to 1.9.7 is a patch-level C-library move), but its stated justification was wrong.
- **The real fix is an fff-search upgrade.** fff-search 0.10.6 requires `git2 ^0.21.0`. The same upgrade is what the Phase 8 plan already wanted checked for the UF-039 and UF-040 upstream fixes, so one dependency move may close #257, UF-039 and UF-040 together and replace both workspace workarounds. This must be evaluated before Phase 13 lands commits 0 and 0b, because it could make them unnecessary.
- **Gitea #257 stays open**, with its original diagnosis (bump fff-search to a release on a patched git2) restored as correct.

## Lead addendum 11 (2026-09-12): the fff-search upgrade that fixes #257 would defeat the UF-039 remediation

Prompted by addendum 10, the lead read the fff-search 0.10.6 sources (downloaded from crates.io into the session scratchpad; not added to the workspace). The upgrade is not the free win it looked like.

**UF-040 is not fixed upstream.** `ChunkedString::read_to_buf` in 0.10.6 (`src/simd_path.rs:169-186`) is functionally identical to 0.8.4: it copies `byte_len.min(buf.len())` bytes and returns `core::str::from_utf8_unchecked(&buf[..total])`. `PATH_BUF_SIZE` is still 4096 (now spelled `libc::PATH_MAX` on Linux), and the lossy path storage (`to_string_lossy().into_owned()`, `src/file_picker.rs:205,411`) is unchanged. The hostile-tree guard of commit 0b is still required after an upgrade.

**UF-039 gets worse, and the planned remediation stops working.** fff-search 0.8.4 has exactly one `memmap2::Mmap::map` site, inside `get_cached_content`, behind the `max_files` budget check; that is what makes the zero-file `ContentCacheBudget` of commit 0 a complete fix. Version 0.10.6 has **two**:

| Site | Location in 0.10.6 | Gated by |
|---|---|---|
| Cached mapping | `src/types.rs:719` in `get_cached_content` | `MMAP_THRESHOLD`, `max_file_size`, **and** `max_files`/`max_bytes` |
| Fresh mapping (new) | `src/types.rs:762` in `get_content_for_search` | `FRESH_MMAP_THRESHOLD` and `max_file_size` **only** |

The new fresh-mapping path is reached after `get_cached_content` returns `None`, which is exactly what a zero-file budget makes it do. So on 0.10.6 the commit 0 fix would send every file at or above `FRESH_MMAP_THRESHOLD` (1 MiB on macOS, 256 KiB on Linux) straight into an unbudgeted mapping, and the remediation would appear to be in place while doing nothing for large files. The SAFETY comment at `src/types.rs:715-718` still rests on the same unsatisfiable precondition ("the watcher mitigates by invalidating on modification") for a picker built by `FilePicker::new`, which installs no watcher.

**Consequences.**

1. Commits 0 and 0b stand; neither is replaced by an upgrade.
2. The fff-search version must be pinned, and the `ci_guards` test must assert the pinned version, because a routine `cargo update` past 0.8.4 would silently reopen UF-039 at a larger file size. This is a new acceptance criterion for Gitea #294.
3. Any future upgrade must gate both map sites. The only budget field that reaches the fresh path is `max_file_size`, and setting it low enough to suppress mapping also refuses those files to grep, so an upgrade needs the upstream fix, not a configuration workaround.
4. The upstream issue text in `phase9_gitea_log.md` must be extended to cover the second map site and to state that `enable_mmap_cache` gates neither.
5. #257 (git2 advisories, needing fff-search >= 0.10.6) is therefore in tension with UF-039: the version that fixes the advisories is the version where the workspace's mitigation fails. Resolving that tension needs the upstream fix or a vendored patch, and it is a decision for the user.

## Phase 5 bigbox pass 2 addendum (2026-09-12): UF-039 blast radius CONFIRMED in the shipped server (EXP-302)

- **EXP-302 promoted DEFERRED -> CONFIRMED_UB.** The shipped release `terraphim_mcp_server --sse` (HEAD 46bbc43, bigbox x86_64) died with SIGBUS (bash `Bus error`, harness exit code 135 = 128+7, server.log truncated mid-scan) when a sibling process truncated the mapped tree while 8 SSE clients ran `terraphim_grep` concurrently. This lifts UF-039 from a confirmed mechanism in a harness (EXP-104) to a confirmed blast radius in the production binary: one client's truncation of a mapped file kills the long-lived server for every connected client. Hundreds of thousands of grep responses succeeded before the crash, so the mapping path was genuinely exercised.
- The writer arm (same-length in-place rewrite) left the server alive with no invalid UTF-8 over the wire, because the MCP `terraphim_grep` tool uses the PlainText/`from_utf8_lossy` sink, not the Fuzzy `from_utf8_unchecked` path. This matches lead addendum 4: the invalid-`String` symptom belongs to the Fuzzy mode, which no shipped tool selects; the process-killing symptom (SIGBUS on truncate) is mode-independent and is the one that reaches the production server.
- Correction to the pre-run scaffold recorded for reproducibility: the MCP tool is `terraphim_grep` (not `grep_files`), the rmcp 0.9 client needs `transport-sse-client-reqwest`, the corpus must place the needle at the file tail so the grep reads the full mapping, and each `call_tool` needs a client-side timeout so a stalled SSE response cannot mask the result. A run using the wrong tool name returns `-32601` on every call and is inadmissible (the grep path never executes).
- Remediation unchanged: the Phase 8 UF-039 zero-file cache budget must make EXP-302 pass; it is the pre-fix half of the pre/post pair with EXP-805. No new finding beyond the confirmed blast radius; this does not reopen convergence.

## Lead addendum 12 (2026-09-12): the Miri exit-capture defect, and the log-based admissibility rule that replaces it

The Phase 12 author found a defect in the lead's own experiment drivers. The tmux drivers capture the pipeline status as `${PIPESTATUS[0]}`, but tmux inherits the user's login shell, which is zsh, and **zsh has no `PIPESTATUS`**: the array is `pipestatus` and it is one-indexed. Verified on this host:

```
zsh -c 'false | true; echo "PIPESTATUS0=[${PIPESTATUS[0]}] pipestatus1=[${pipestatus[1]}]"'
PIPESTATUS0=[] pipestatus1=[1]
```

So every axis driven by one of those drivers records an empty `MIRI_EXIT=`, which under the audit's stated admissibility rule would make the run unscoreable. This affects EXP-203's completed axis and the EXP-202 axis in flight. It is the same shape as the env-probe driver defect recorded in addendum 9: an instrument whose own reporting is broken, where the silence reads as success.

**Resolution, after one reversal and a reversal of the reversal. The honest account of both is part of the record.**

First position: demote the exit status to corroboration for all runs and put a log-based rule in its place. Second position, after the Phase 12 author kept the strict rule: withdraw that, on the grounds that relaxing a stated standard so an otherwise unscoreable run becomes scoreable is motivated reasoning, and that the lead had already made two convenient readings today.

The second position was an over-correction, and the reasoning in it was poor. "I was wrong twice today, so this is probably wrong too" is pattern-matching, not analysis. Judged on its merits the log-based rule is sound: the exit status was only ever a proxy for "did this run fail", and Miri's own diagnostics in the log are a more direct record of the thing the experiment is actually asking about. The substitution was validated against a known-failing run before being adopted, not asserted. The only thing the exit status catches that the log does not is Miri's leak check exiting non-zero after a successful rayon run, which is a known benign case.

**Settled position, and `FINAL_UB_REPORT.md` plus `UB_RUNBOOK.md` are authoritative for it.** The strict rule stands: an empty exit capture makes a run inadmissible regardless of what the log says, and the log markers are corroboration. The runbook records in one line that a log-based substitute was proposed for the two runs whose captures were lost and was rejected, because weakening a stated rule so that an unscoreable run becomes scoreable is precisely the failure that document exists to prevent.

Where a re-run is cheap, the right answer is to re-run rather than argue:

- **EXP-203 is being re-driven in full** under `bash -c` at HEAD e90b193 (log `phase5_experiment_results/EXP-203_bashdriver.log`, started 18:40 BST), both axes, about twenty minutes for the default axis. That gives it correctly captured exit statuses and moots the question for this experiment.
- **EXP-202's verdict rests on its admissible default axis** (`-Zmiri-disable-isolation`, Stacked Borrows, the stricter aliasing model): 43 inputs, `MIRI_EXIT=0` captured by another agent's bash driver, no diagnostic. Its `-Zmiri-tree-borrows` axis was launched by the defective driver, so it is **corroboration only, must not be cited as the basis of the verdict**, and a bash-driven re-run of that axis is outstanding. It is not load-bearing, which is why it is allowed to finish at four hours and more rather than being restarted immediately.

**Driver fix for any re-run: use `bash -c`, not the inherited shell.** The EXP-203 tree-borrows axis, which the lead aborted for host memory pressure, must be restarted with a corrected driver.

## Lead addendum 13 (2026-09-13, close-out of the bigbox Miri pass)

- **UF-041 closes NO_EVIDENCE** (EXP-202). The largest attacker-reachable `unsafe` parser in the tree, `unsafe-libyaml` reached through `serde_yaml` 0.9, showed no undefined behaviour under the stricter Stacked Borrows model over the full hostile corpus, nor under Tree Borrows over the 34 of 43 inputs that completed. Migration remains worth doing on hygiene grounds (#317), not on evidence.
- **UF-042 and EXP-003c close NO_EVIDENCE**; EXP-003c now has a real harness (four of the five public helpers over nine query shapes plus a clone and lock round trip, `save_config` deliberately excluded because it writes to the real persistence backends) and is clean on all four axes with captured exit 0. The registry's last NEEDS_REFINEMENT is cleared.
- **Campaign C closes the Miri matrix caveat properly.** Every axis now covers the same 123 of 128 tests in `terraphim-session-analyzer`. The 5 uncovered on the three Stacked-Borrows-based axes are the same 5 every time, all driving `analyze`, whose `par_iter` starts rayon's global pool, after which crossbeam-epoch 0.9.21 either violates Stacked Borrows at internal.rs:567 or performs an integer-to-pointer cast at atomic.rs:204 that strict-provenance refuses. Every erroring frame is a dependency frame; workspace code appears only in Miri's "called indirectly" note. Dependency limitation on the EXP-101 precedent. Tree Borrows covers all 128 precisely because it does not flag the pattern. The final report must replace "the cells are not comparable" with this.
- **A remedy recorded earlier in `phase11_soak_designs.md` is wrong and is corrected there.** EXP-203's Tree Borrows axis aborted four times (110 GB uncapped, then 48.2, 54.4 and 43.7 GB under input caps of 8192, 256 and 32 bytes). All four share one memory curve, roughly 13 GB at ten minutes and past 40 GB by 25 to 28 minutes, **independent of the cap**. Memory tracks interpreted steps and Miri's accumulating Tree Borrows state, not input length, so "re-run with the corpus capped below 16 KB" cannot work. The correct remedy is to drop each `Version` immediately instead of accumulating them, or to run one Miri process per input.
- **Host-safety incident, the second of the day on bigbox.** The uncapped EXP-203 run went from 33 GB to 110 GB between two 45-second polls and took bigbox to 4 GB available, with the Gitea PostgreSQL container on that host. The executor killed it and the host recovered immediately. No service was observed down, but the margin was thin, and a 20-second poll still overshot to 48 and 54 GB. Any future Tree Borrows run on that host needs an automatic kill polling at 10 seconds or tighter. This joins the disk-full incident of 2026-09-12 as evidence that heavyweight audit work and the team's git server should not share a host.
