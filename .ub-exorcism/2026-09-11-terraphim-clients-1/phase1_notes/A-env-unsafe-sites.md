# Module

Section A: env-unsafe-sites. Files in scope:

- crates/terraphim_update/src/state.rs (test helper `setup_temp_config_dir`, unsafe block :131-137)
- crates/terraphim_grep/src/openrouter_client.rs (test `test_openrouter_client_base_url_resolution`, unsafe blocks :170, :177, :185, :189)
- crates/terraphim_mcp_server/tests/test_mcp_stdio.rs (test `test_mcp_autocomplete_via_stdio`, unsafe block :15-20)
- crates/terraphim_agent/build.rs (emits `cfg(rust_has_unsafe_env_setters)`; not an unsafe site)

Mapper run 2026-09-11 23:11 BST on rustc 1.97.1 / cargo 1.97.1, host darwin arm64. Workspace edition 2024 (Cargo.toml:19), so `std::env::set_var` / `remove_var` are `unsafe fn` everywhere in the workspace.

## Purpose

state.rs: redirect `dirs::config_dir()` (state.rs:16) into a `TempDir` so the ten update-history tests write `update_history.json` somewhere disposable.

openrouter_client.rs: prove that `OpenRouterClient::new` (openrouter_client.rs:51) honours `OPENROUTER_BASE_URL` and falls back to the public URL when it is unset.

test_mcp_stdio.rs: point the spawned `terraphim_mcp_server` child (:24-32) at `settings_local_dev.toml` via inherited environment.

build.rs: historical shim so pre-1.92 toolchains could call `set_var` safely; see "Notable patterns".

## Unsafe-surface tally

| binary | unsafe blocks | env calls | SAFETY status |
|---|---|---|---|
| terraphim_update lib tests | 1 (state.rs:131) | 5 x set_var | PRESENT_WEAK (claim false) |
| terraphim_grep lib tests | 4 (openrouter_client.rs:170/177/185/189) | 2 x set_var, 2 x remove_var | PRESENT_WEAK x4 via one shared comment at :166 (claim false) |
| terraphim_mcp_server test_mcp_stdio | 1 (test_mcp_stdio.rs:15) | 1 x set_var | MISSING (de facto sound) |

Total: 6 unsafe blocks, 10 env-mutation calls, all SOURCE_DIRECT. Matches the inventory header archetype.

Workspace-wide `rg "set_var|remove_var" --type rust` returns only these three files (excluding target/ and .ub-exorcism/).

## Notable patterns

- **Shared SAFETY comment covering several blocks.** openrouter_client.rs:166 is one line for four blocks; only :170 is adjacent. Graded uniformly as PRESENT_WEAK.
- **No restore, or restore that a panic skips.** state.rs never restores HOME/USERPROFILE/XDG_CONFIG_HOME/APPDATA/LOCALAPPDATA; after `TempDir` drops (end of each state.rs test) HOME points at a deleted directory for the remainder of the process. openrouter_client.rs restores at :184-192 but the restore is skipped if the `assert_eq!` at :174 or :181 panics.
- **Platform mismatch in the state.rs helper.** `dirs::config_dir()` on macOS is `$HOME/Library/Application Support` (dirs-5.0.1/src/mac.rs:10 `config_dir` -> :7 `home_dir().join("Library/Application Support")`; HOME is read via `env::var_os` at dirs-sys-0.4.1/src/lib.rs:34; XDG_CONFIG_HOME is consulted only in the Linux implementation). So on macOS the `XDG_CONFIG_HOME` set_var is inert and the doc comment at state.rs:14 ("`~/.config/terraphim/update_history.json` on Unix") does not describe macOS. USERPROFILE/APPDATA/LOCALAPPDATA are Windows-only inputs and inert on Unix.
- **Dead cfg.** `rust_has_unsafe_env_setters` is emitted by crates/terraphim_agent/build.rs:19 for rustc >= 1.92 but consumed nowhere in the workspace. Commit 58d3c2d (2026-01-31) added the build script and, in the same change, removed the only consumers from crates/terraphim_agent/tests/tui_service_tests.rs in favour of `terraphim_test_utils::EnvVarGuard`. That external crate (registry git.terraphim.cloud, terraphim_test_utils-1.20.3/src/lib.rs:35,45) has its own build script; cfgs from terraphim_agent's build.rs do not propagate to it. With rustc 1.97.1 the cfg is always set and, under edition 2024, irrelevant.
- **Env reads sit behind std's lock everywhere in the Rust code.** Every Rust-side reader found (platform.rs:112, lib.rs:159/167/183, openrouter_client.rs:51, main.rs:390-462, tempfile TMPDIR, tokio `TOKIO_WORKER_THREADS`, console-0.15.11 TERM/NO_COLOR/LANG, dirs-sys HOME) uses `std::env::var`/`var_os`, which takes std's `ENV_LOCK` read side. The discriminating readers are therefore native ones (below).

## Thread-model analysis

Harness facts common to all three binaries: no `test-threads`, `RUST_TEST_THREADS`, `.config/nextest.toml`, or `harness = false` on a test target anywhere in the repo (`rg` over the tree; the two `harness = false` hits are `[[bench]]` targets in terraphim_grep and terraphim_sessions). CI runs `cargo test --workspace --lib --no-fail-fast` (.github/workflows/ci.yml:26, Linux x86_64) and `cargo test --workspace --all-targets --no-fail-fast` (.gitea/workflows/native-ci.yml:54). libtest therefore uses its default worker count (available parallelism) and runs tests on spawned threads with the main thread parked.

### terraphim_update lib test binary (state.rs:131)

Tests in the binary: 130 (`rg -c '#[test]|#[tokio::test]'` over src/: platform 12, signature 14, scheduler 10, state 10, downloader 11, manifest 11, lib 19, config 10, notification 15, rollback 18). `#[serial]`: exactly the 10 in state.rs (:144, :163, :176, :199, :227, :248, :266, :277, :289, :311). All `#[tokio::test]` uses are default (current_thread) flavour; none pass `flavor`.

serial_test 3.5.0 semantics (serial_test-3.5.0/src/lib.rs:28-31): `#[serial]` tests are mutually exclusive with each other and with `#[parallel]` tests; "Tests with neither attribute may run at any time and no guarantees are made about their timing". So the 120 non-serial tests run concurrently with `setup_temp_config_dir`. The SAFETY claim "runs in isolation before spawning other threads" (state.rs:130) is false as a description of the process: worker threads already exist and are executing other tests.

Concurrent readers, all under std's lock (data-race free, logically racy):

- platform.rs:112 `env::var("HOME")` inside `get_config_dir`, called by `test_get_config_dir` (platform.rs:287) and `test_get_config_dir_creates_parent` (:402). Both would observe the temp HOME if interleaved; their asserts (`contains(".config/terraphim")`, absolute path) still hold.
- lib.rs:159 `TERRAPHIM_UPDATE_BACKEND`, :167 `TERRAPHIM_UPDATE_BASE_URL`, :183 `GITHUB_TOKEN` inside `UpdaterConfig::new`, called by lib.rs:1569, :1586, :1592, :1598, :1609. Not among the variables state.rs sets.
- tempfile `TempDir::new` / `NamedTempFile::new` (TMPDIR via `std::env::temp_dir`) in many tests.
- dirs-sys-0.4.1/src/lib.rs:34 `env::var_os("HOME")` with `getpwuid_r` fallback at :52 (getpwuid_r does not read the environment).

Extra OS threads beyond libtest workers: tokio `spawn_blocking` at lib.rs:263/307/414/574/692/1283 and scheduler.rs:250 (reached by `test_check_for_updates_auto` lib.rs:1713 and the scheduler tokio tests :348-502). Those threads run self_update/ureq/rustls/ring code.

Native readers searched for and not found in this binary's dependency tree (`cargo tree -p terraphim_update -e normal,dev`): no openssl/native-tls, no aws-lc-sys (rustls uses ring v0.17.14; roots are webpki-roots), no libgit2-sys, no libsqlite3-sys, no iana-time-zone use (chrono is present but only `Utc::now()` at rollback.rs:39 and `DateTime<Utc>` in signature.rs:10; no `Local`), no `std::process::Command` in src/. The -sys crates present are core-foundation-sys, dirs-sys, getrandom, libbz2-rs-sys, ring, zstd-sys; none read the environment on the paths tests exercise. jiff `Timestamp::now()` (config.rs, platform.rs, state.rs) is a clock read only.

Conclusion for this binary: the comment's stated justification is false, but no out-of-lock reader was identified, so the site's soundness rests on a property (all readers go through std's lock) that the comment does not assert. HOME is left dangling after each state.rs test for every later-running test.

### terraphim_grep lib test binary (openrouter_client.rs:170/177/185/189)

Tests in the binary: 57 (63 `#[test]`/`#[tokio::test]` attrs across src/ minus the 6 in main.rs, which belong to the bin target): signatures 12, hybrid_searcher 13, lib 11, sufficiency_judge 9, rlm_context 5, openrouter_client 4, kg_curation 3. No `#[serial]` anywhere in the crate; `serial_test` is not a dependency. All `#[tokio::test]` are current_thread.

Std-locked concurrent readers of the mutated variable: openrouter_client.rs:51 via `test_openrouter_client_new_validates_inputs` (:159-161), `test_openrouter_client_timeout_is_120_seconds` (:197), `test_into_llm_client_returns_trait_object` (:206). None asserts `base_url`, so today the logical race is benign. `reqwest::Client::builder().build()` (:38-45) also reads HTTP_PROXY/HTTPS_PROXY/NO_PROXY through std (reqwest 0.12.28, `system-proxy` feature; macOS SystemConfiguration lookup does not read the environment).

Out-of-lock reader, confirmed by source: libgit2 `getenv`. Chain:

1. `search_without_thesaurus_uses_fff_mode` (lib.rs:841), `default_build_greps_populated_directory` (hybrid_searcher.rs:485), `multiple_search_paths_merge_results` (hybrid_searcher.rs:527) call `HybridSearcher::search` -> `search_code` (hybrid_searcher.rs:342) -> `FilePicker::new` (:354) then `picker.collect_files()` (:364).
2. fff-search-0.8.4/src/file_picker.rs:796 `collect_files` -> :800 `FileSync::discover_git_workdir` -> :1695 `git2::Repository::discover`.
3. git2-0.20.4/src/lib.rs:748 `init()` runs `git_libgit2_init` once per process (`Once` at :749).
4. libgit2-sys-0.18.5+1.9.4/libgit2/src/libgit2/libgit2.c:34-46 `git_libgit2_init` runs `git_sysdir_global_init` (sysdir.c:453), whose loop at :459/:474 calls each guess fn; those call `getenv("APP_SANDBOX_CONTAINER_ID")` (sysdir.c:347), `git__getenv("HOME")` (:357, :405) and `git__getenv("XDG_CONFIG_HOME")` (:402); the Unix `git__getenv` is a plain libc `getenv` (util.c:786-788).

This runs even though the temp directories are not git repositories (init precedes discovery) and runs on whichever of the three tests reaches it first, on a libtest worker thread that can be concurrent with the set_var/remove_var blocks. Additional OS threads created on the same path: `FileSync::spawn_git_status` (file_picker.rs:801 -> :1708 `std::thread::spawn`) when a workdir is found, and the rayon `BACKGROUND_THREAD_POOL` (file_picker.rs:73, installs at :1807/:1831/:1853). `FilePicker::new` itself (:696-731) spawns nothing; `watch: false` (hybrid_searcher.rs:357) keeps `background_watcher` off. std's own `set_var` safety contract (std/src/env.rs:304-326 in the rustc 1.97.1 sysroot source) requires that no other thread reads the environment through any API while the write happens and names foreign code and libc `getenv` explicitly as readers outside its lock; libgit2's `getenv` is such a reader. The same path is exercised by the Linux x86_64 CI run at .github/workflows/ci.yml:26.

Not on the path (recorded so the next phase does not re-derive it): aws-lc-sys cpu_aarch64_apple.c:105 `getenv("OPENSSL_armcap")` / cpu_intel.c:280 `getenv("OPENSSL_ia32cap")` run from `CRYPTO_library_init` (aws-lc-rs-1.17.0/src/lib.rs:305-307), but the reqwest 0.12.28 built at openrouter_client.rs:38 uses the ring provider (feature edges `rustls-tls` -> `__rustls-ring`), and no lib test performs a TLS handshake. libsqlite3-sys (13 `getenv` calls in sqlite3.c) is pulled by terraphim_persistence but no lib test opens a database. iana-time-zone is present via chrono but no `Local` use was found in terraphim_grep src/.

Conclusion for this binary: the SAFETY claim at :166 ("single-threaded, no concurrent env access") is false. Both halves fail: 56 other tests run on parallel libtest workers, and three of them reach an unlocked libc `getenv` through libgit2 initialisation.

### terraphim_mcp_server test_mcp_stdio binary (test_mcp_stdio.rs:15)

Tests in the binary: 1 (`test_mcp_autocomplete_via_stdio`, :6). No `#[serial]` in this file (three other integration files in the crate use serial_test, but each `tests/*.rs` is its own process). Thread model: libtest spawns one worker for the single test; the main thread waits on the result channel. Within the test, the `RUN_MCP_STDIO_TEST` read at :10 (std lock) precedes the set_var at :15; `Command::spawn` at :24-32 happens after, on the same thread, so the child inherits the already-written environment (sequenced after the write). No `ctor`/`inventory`/`linkme` static constructors in the crate's tree. No other thread exists in the process to read the environment concurrently.

Conclusion for this binary: de facto sound by construction of a one-test binary; the site carries no SAFETY comment (the :14 comment is a description of intent). The gate at :10-13 means the block does not execute in either CI workflow: `rg RUN_MCP_STDIO_TEST` over .github/, .gitea/, justfile and scripts/ finds no export.

## Open questions

1. `--list` and `cargo +nightly expand --tests` for the three binaries were queued (scratchpad list_*.txt and /private/tmp/claude-501/ubx-expand/terraphim_{update,grep}_lib_tests.rs, terraphim_mcp_server_test_mcp_stdio.rs) but were blocked on the shared cargo build-directory lock at write time. Test counts above are static (`rg`) counts; `#[cfg(feature = "code-search")]` gates (default-on) could reduce the grep count if run with `--no-default-features`.
2. Does anything in the terraphim_update test binary reach libc `getenv` on a path not inspected here (for example self_update or dialoguer internals during `test_check_for_updates_auto`, lib.rs:1713, which performs a live HTTPS request)? Source inspection of ureq/rustls/ring found none; not verified dynamically (no ThreadSanitizer run in phase 1).
3. libgit2 `git_status` on the `spawn_git_status` thread (file_picker.rs:1708) may read further `GIT_*` variables via `git__getenv` after initialisation; only the init-time reads were traced.
4. aws-lc `getenv` at CPU-capability setup becomes reachable if any grep lib test ever performs a TLS handshake on the aws-lc provider (reqwest 0.13.4 with `__rustls-aws-lc-rs` is in the tree via terraphim_service). Not reachable today.
5. Whether `terraphim_test_utils::EnvVarGuard` (external crate) is intended to replace these three sites is a design question outside phase 1 scope; recorded only because the dead cfg in build.rs points at it.

## Appendix: every test in the section-A binaries (static enumeration)

### terraphim_update lib (130 tests, 10 serial)

| file:line | test | harness | attrs |
|---|---|---|---|
| crates/terraphim_update/src/config.rs:229 | test_update_config_default | libtest | - |
| crates/terraphim_update/src/config.rs:240 | test_update_config_serialize | libtest | - |
| crates/terraphim_update/src/config.rs:253 | test_update_info_serialization | libtest | - |
| crates/terraphim_update/src/config.rs:273 | test_update_history_default | libtest | - |
| crates/terraphim_update/src/config.rs:282 | test_update_history_add_entry | libtest | - |
| crates/terraphim_update/src/config.rs:294 | test_update_history_limit_entries | libtest | - |
| crates/terraphim_update/src/config.rs:313 | test_update_history_add_backup | libtest | - |
| crates/terraphim_update/src/config.rs:338 | test_update_history_latest_backup | libtest | - |
| crates/terraphim_update/src/config.rs:347 | test_update_check_entry_serialization | libtest | - |
| crates/terraphim_update/src/config.rs:369 | test_update_check_result_variants | libtest | - |
| crates/terraphim_update/src/downloader.rs:322 | test_download_config_default | libtest | - |
| crates/terraphim_update/src/downloader.rs:333 | test_download_config_builder | libtest | - |
| crates/terraphim_update/src/downloader.rs:349 | test_calculate_backoff_delay | libtest | - |
| crates/terraphim_update/src/downloader.rs:366 | test_calculate_backoff_delay_with_custom_multiplier | libtest | - |
| crates/terraphim_update/src/downloader.rs:384 | test_calculate_backoff_delay_max_limit | libtest | - |
| crates/terraphim_update/src/downloader.rs:397 | test_download_invalid_url | libtest | - |
| crates/terraphim_update/src/downloader.rs:415 | test_download_with_timeout | libtest | - |
| crates/terraphim_update/src/downloader.rs:452 | test_download_max_retries | libtest | - |
| crates/terraphim_update/src/downloader.rs:477 | test_download_creates_output_file | libtest | - |
| crates/terraphim_update/src/downloader.rs:498 | test_download_result_success | libtest | - |
| crates/terraphim_update/src/downloader.rs:523 | test_download_silent_local_file | libtest | - |
| crates/terraphim_update/src/lib.rs:1549 | test_version_comparison | libtest | - |
| crates/terraphim_update/src/lib.rs:1569 | test_updater_config | tokio current_thread | - |
| crates/terraphim_update/src/lib.rs:1586 | test_default_backend_is_r2 | libtest | - |
| crates/terraphim_update/src/lib.rs:1592 | test_with_backend_builder | libtest | - |
| crates/terraphim_update/src/lib.rs:1598 | test_with_manifest_base_url_builder | libtest | - |
| crates/terraphim_update/src/lib.rs:1609 | test_with_auth_token_builder | libtest | - |
| crates/terraphim_update/src/lib.rs:1618 | test_backup_binary | libtest | - |
| crates/terraphim_update/src/lib.rs:1642 | test_backup_binary_nonexistent | libtest | - |
| crates/terraphim_update/src/lib.rs:1650 | test_rollback | libtest | - |
| crates/terraphim_update/src/lib.rs:1671 | test_rollback_nonexistent | libtest | - |
| crates/terraphim_update/src/lib.rs:1681 | test_backup_and_rollback_roundtrip | libtest | - |
| crates/terraphim_update/src/lib.rs:1713 | test_check_for_updates_auto | tokio current_thread | - |
| crates/terraphim_update/src/lib.rs:1737 | test_is_newer_version_static | libtest | - |
| crates/terraphim_update/src/lib.rs:1756 | test_update_status_display | libtest | - |
| crates/terraphim_update/src/lib.rs:1779 | test_version_prefix_for_github_releases | libtest | - |
| crates/terraphim_update/src/lib.rs:1803 | test_get_asset_names_includes_binary_prefix | libtest | - |
| crates/terraphim_update/src/lib.rs:1819 | test_get_asset_names_for_universal_binary | libtest | - |
| crates/terraphim_update/src/lib.rs:1837 | test_macos_targets_include_universal_fallback | libtest | - |
| crates/terraphim_update/src/lib.rs:1851 | test_config_with_binary_version | libtest | - |
| crates/terraphim_update/src/manifest.rs:297 | test_manifest_url_construction | libtest | - |
| crates/terraphim_update/src/manifest.rs:306 | test_manifest_url_strips_trailing_slash | libtest | - |
| crates/terraphim_update/src/manifest.rs:315 | test_asset_url_construction | libtest | - |
| crates/terraphim_update/src/manifest.rs:324 | test_asset_url_strips_leading_slash | libtest | - |
| crates/terraphim_update/src/manifest.rs:333 | test_manifest_parse_minimal | libtest | - |
| crates/terraphim_update/src/manifest.rs:346 | test_manifest_parse_missing_version_errors | libtest | - |
| crates/terraphim_update/src/manifest.rs:353 | test_manifest_roundtrip | libtest | - |
| crates/terraphim_update/src/manifest.rs:361 | test_resolve_asset_finds_present_target | libtest | - |
| crates/terraphim_update/src/manifest.rs:379 | test_resolve_asset_no_match_errors | libtest | - |
| crates/terraphim_update/src/manifest.rs:392 | test_backend_default_is_r2 | libtest | - |
| crates/terraphim_update/src/manifest.rs:397 | test_current_target_triples_nonempty | libtest | - |
| crates/terraphim_update/src/notification.rs:215 | test_get_update_notification_basic | libtest | - |
| crates/terraphim_update/src/notification.rs:223 | test_get_update_notification_with_notes | libtest | - |
| crates/terraphim_update/src/notification.rs:233 | test_format_release_notes_empty | libtest | - |
| crates/terraphim_update/src/notification.rs:240 | test_format_release_notes_single_line | libtest | - |
| crates/terraphim_update/src/notification.rs:247 | test_format_release_notes_multiple_lines | libtest | - |
| crates/terraphim_update/src/notification.rs:256 | test_format_release_notes_with_empty_lines | libtest | - |
| crates/terraphim_update/src/notification.rs:264 | test_get_update_success_message | libtest | - |
| crates/terraphim_update/src/notification.rs:272 | test_get_update_failure_message | libtest | - |
| crates/terraphim_update/src/notification.rs:279 | test_get_up_to_date_message | libtest | - |
| crates/terraphim_update/src/notification.rs:286 | test_prompt_user_for_update_default_value | libtest | - |
| crates/terraphim_update/src/notification.rs:300 | test_update_action_variants | libtest | - |
| crates/terraphim_update/src/notification.rs:314 | test_emoji_helpers | libtest | - |
| crates/terraphim_update/src/notification.rs:328 | test_get_update_notification_formatting | libtest | - |
| crates/terraphim_update/src/notification.rs:339 | test_get_up_to_date_message_formatting | libtest | - |
| crates/terraphim_update/src/notification.rs:350 | test_update_failure_message_formatting | libtest | - |
| crates/terraphim_update/src/platform.rs:245 | test_get_binary_path_system_writable | libtest | - |
| crates/terraphim_update/src/platform.rs:262 | test_get_binary_path_prefers_running_exe_dir | libtest | - |
| crates/terraphim_update/src/platform.rs:281 | test_get_binary_path_custom_name | libtest | - |
| crates/terraphim_update/src/platform.rs:287 | test_get_config_dir | libtest | - |
| crates/terraphim_update/src/platform.rs:301 | test_check_write_permissions_writable_dir | libtest | - |
| crates/terraphim_update/src/platform.rs:309 | test_check_write_permissions_readonly_dir | libtest | - |
| crates/terraphim_update/src/platform.rs:343 | test_check_write_permissions_file | libtest | - |
| crates/terraphim_update/src/platform.rs:355 | test_check_write_permissions_nonexistent_path | libtest | - |
| crates/terraphim_update/src/platform.rs:366 | test_show_manual_update_instructions | libtest | - |
| crates/terraphim_update/src/platform.rs:385 | test_show_manual_update_instructions_with_long_notes | libtest | - |
| crates/terraphim_update/src/platform.rs:402 | test_get_config_dir_creates_parent | libtest | - |
| crates/terraphim_update/src/platform.rs:411 | test_binary_path_consistency | libtest | - |
| crates/terraphim_update/src/rollback.rs:394 | test_backup_manager_creation | libtest | - |
| crates/terraphim_update/src/rollback.rs:406 | test_backup_manager_zero_max_backups | libtest | - |
| crates/terraphim_update/src/rollback.rs:416 | test_create_backup | libtest | - |
| crates/terraphim_update/src/rollback.rs:437 | test_create_backup_nonexistent_binary | libtest | - |
| crates/terraphim_update/src/rollback.rs:449 | test_backup_rotation | libtest | - |
| crates/terraphim_update/src/rollback.rs:480 | test_backup_rotation_not_needed | libtest | - |
| crates/terraphim_update/src/rollback.rs:503 | test_get_backup | libtest | - |
| crates/terraphim_update/src/rollback.rs:521 | test_get_backup_not_found | libtest | - |
| crates/terraphim_update/src/rollback.rs:533 | test_list_backups | libtest | - |
| crates/terraphim_update/src/rollback.rs:558 | test_rollback_to_version | libtest | - |
| crates/terraphim_update/src/rollback.rs:578 | test_rollback_to_version_not_found | libtest | - |
| crates/terraphim_update/src/rollback.rs:594 | test_rollback_to_latest | libtest | - |
| crates/terraphim_update/src/rollback.rs:619 | test_rollback_to_latest_no_backups | libtest | - |
| crates/terraphim_update/src/rollback.rs:635 | test_delete_backup | libtest | - |
| crates/terraphim_update/src/rollback.rs:654 | test_cleanup | libtest | - |
| crates/terraphim_update/src/rollback.rs:685 | test_backup_integrity | libtest | - |
| crates/terraphim_update/src/rollback.rs:701 | test_backup_integrity_corrupt | libtest | - |
| crates/terraphim_update/src/rollback.rs:719 | test_backup_integrity_missing | libtest | - |
| crates/terraphim_update/src/scheduler.rs:297 | test_scheduler_creation | libtest | - |
| crates/terraphim_update/src/scheduler.rs:312 | test_should_check_with_interval | libtest | - |
| crates/terraphim_update/src/scheduler.rs:334 | test_should_check_when_disabled | libtest | - |
| crates/terraphim_update/src/scheduler.rs:348 | test_scheduler_starts_and_stops | tokio current_thread | - |
| crates/terraphim_update/src/scheduler.rs:373 | test_scheduler_with_short_interval | tokio current_thread | - |
| crates/terraphim_update/src/scheduler.rs:392 | test_scheduler_notification_update_available | tokio current_thread | - |
| crates/terraphim_update/src/scheduler.rs:430 | test_scheduler_notification_check_failed | tokio current_thread | - |
| crates/terraphim_update/src/scheduler.rs:463 | test_scheduler_disabled_does_not_start | tokio current_thread | - |
| crates/terraphim_update/src/scheduler.rs:481 | test_scheduler_already_running | tokio current_thread | - |
| crates/terraphim_update/src/scheduler.rs:502 | test_scheduler_stop_when_not_running | tokio current_thread | - |
| crates/terraphim_update/src/signature.rs:410 | test_real_key_reports_missing_signature_for_unsigned_file | libtest | - |
| crates/terraphim_update/src/signature.rs:426 | test_nonexistent_file_returns_error | libtest | - |
| crates/terraphim_update/src/signature.rs:433 | test_invalid_base64_key_returns_error | libtest | - |
| crates/terraphim_update/src/signature.rs:444 | test_wrong_length_key_returns_invalid | libtest | - |
| crates/terraphim_update/src/signature.rs:454 | test_embedded_public_keys_has_primary_and_legacy | libtest | - |
| crates/terraphim_update/src/signature.rs:466 | test_multi_key_verifies_archive_signed_with_primary | libtest | - |
| crates/terraphim_update/src/signature.rs:505 | test_is_verification_available | libtest | - |
| crates/terraphim_update/src/signature.rs:511 | test_get_signature_filename | libtest | - |
| crates/terraphim_update/src/signature.rs:518 | test_verification_result_equality | libtest | - |
| crates/terraphim_update/src/signature.rs:540 | test_verification_result_display | libtest | - |
| crates/terraphim_update/src/signature.rs:558 | test_verify_signature_detailed_with_real_key | libtest | - |
| crates/terraphim_update/src/signature.rs:568 | test_verify_signature_detailed_nonexistent | libtest | - |
| crates/terraphim_update/src/signature.rs:576 | test_verify_with_self_update | libtest | - |
| crates/terraphim_update/src/signature.rs:592 | test_verify_with_self_update_missing_binary | libtest | - |
| crates/terraphim_update/src/state.rs:145 | test_save_and_load_history | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:164 | test_load_default_when_missing | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:177 | test_history_with_check_entries | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:200 | test_history_with_pending_update | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:228 | test_history_with_backups | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:249 | test_delete_history | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:267 | test_delete_nonexistent_history | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:278 | test_invalid_json_fails_gracefully | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:290 | test_schema_migration_compatibility | libtest | #[serial] |
| crates/terraphim_update/src/state.rs:312 | test_get_history_path | libtest | #[serial] |

### terraphim_grep lib (57 tests, 0 serial; main.rs tests excluded as they belong to the bin target)

| file:line | test | harness | attrs |
|---|---|---|---|
| crates/terraphim_grep/src/hybrid_searcher.rs:442 | test_hybrid_results_empty | tokio current_thread | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:453 | test_hybrid_results_to_chunks | tokio current_thread | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:485 | default_build_greps_populated_directory | tokio current_thread | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:527 | multiple_search_paths_merge_results | tokio current_thread | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:595 | thesaurus_query_concepts_uses_automata_not_substring_expansion | libtest | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:605 | thesaurus_query_concepts_expands_shared_normalised_term | libtest | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:629 | kg_boost_promotes_matching_chunks_to_top | libtest | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:650 | kg_boost_no_concepts_is_a_noop | libtest | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:660 | kg_boost_stacks_when_multiple_concepts_match | libtest | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:676 | kg_boost_does_not_match_concept_embedded_in_larger_word | libtest | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:686 | kg_boost_matches_concept_at_identifier_boundary | libtest | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:696 | kg_boost_keeps_author_only_chunk_below_real_auth_chunk | libtest | - |
| crates/terraphim_grep/src/hybrid_searcher.rs:711 | test_grep_options_default | libtest | - |
| crates/terraphim_grep/src/kg_curation.rs:204 | test_persist_concepts_writes_markdown_files | libtest | - |
| crates/terraphim_grep/src/kg_curation.rs:240 | test_persist_concepts_skips_existing_files | libtest | - |
| crates/terraphim_grep/src/kg_curation.rs:259 | test_persist_concepts_empty_is_noop | libtest | - |
| crates/terraphim_grep/src/lib.rs:491 | grep_result_serialises_sufficiency_explanation | libtest | - |
| crates/terraphim_grep/src/lib.rs:608 | needs_synthesis_without_answer_skips_llm | tokio current_thread | - |
| crates/terraphim_grep/src/lib.rs:641 | needs_synthesis_with_answer_invokes_llm | tokio current_thread | - |
| crates/terraphim_grep/src/lib.rs:674 | force_rlm_without_answer_invokes_llm | tokio current_thread | - |
| crates/terraphim_grep/src/lib.rs:704 | rlm_requested_only_for_explicit_flags | libtest | - |
| crates/terraphim_grep/src/lib.rs:727 | test_grep_options_default | libtest | - |
| crates/terraphim_grep/src/lib.rs:742 | insufficient_path_propagates_chunk_count | tokio current_thread | - |
| crates/terraphim_grep/src/lib.rs:793 | search_without_llm_degrades_to_search_only | tokio current_thread | - |
| crates/terraphim_grep/src/lib.rs:841 | search_without_thesaurus_uses_fff_mode | tokio current_thread | - |
| crates/terraphim_grep/src/lib.rs:883 | test_answer_prompt_includes_json_instructions | libtest | - |
| crates/terraphim_grep/src/lib.rs:919 | test_list_prompt_excludes_json_instructions | libtest | - |
| crates/terraphim_grep/src/openrouter_client.rs:158 | test_openrouter_client_new_validates_inputs | libtest | - |
| crates/terraphim_grep/src/openrouter_client.rs:165 | test_openrouter_client_base_url_resolution | libtest | - |
| crates/terraphim_grep/src/openrouter_client.rs:196 | test_openrouter_client_timeout_is_120_seconds | libtest | - |
| crates/terraphim_grep/src/openrouter_client.rs:205 | test_into_llm_client_returns_trait_object | libtest | - |
| crates/terraphim_grep/src/rlm_context.rs:117 | test_rlm_context_new | libtest | - |
| crates/terraphim_grep/src/rlm_context.rs:125 | test_rlm_context_with_chunks | libtest | - |
| crates/terraphim_grep/src/rlm_context.rs:137 | test_rlm_context_with_concepts | libtest | - |
| crates/terraphim_grep/src/rlm_context.rs:150 | test_build_prompt | libtest | - |
| crates/terraphim_grep/src/rlm_context.rs:172 | test_truncate | libtest | - |
| crates/terraphim_grep/src/signatures.rs:168 | test_extract_json_strips_markdown_fences | libtest | - |
| crates/terraphim_grep/src/signatures.rs:185 | test_extract_json_finds_bare_json_in_prose | libtest | - |
| crates/terraphim_grep/src/signatures.rs:199 | test_extract_json_strips_generic_markdown_fence | libtest | - |
| crates/terraphim_grep/src/signatures.rs:216 | test_extract_json_finds_array_in_prose | libtest | - |
| crates/terraphim_grep/src/signatures.rs:229 | test_extract_json_handles_unterminated_fence | libtest | - |
| crates/terraphim_grep/src/signatures.rs:242 | test_extract_json_prefers_json_fence_over_other_language | libtest | - |
| crates/terraphim_grep/src/signatures.rs:259 | test_search_result_signature_parse | libtest | - |
| crates/terraphim_grep/src/signatures.rs:278 | test_answer_signature_parse | libtest | - |
| crates/terraphim_grep/src/signatures.rs:297 | test_answer_signature_parse_markdown_wrapped | libtest | - |
| crates/terraphim_grep/src/signatures.rs:322 | test_concept_extraction_signature_parse | libtest | - |
| crates/terraphim_grep/src/signatures.rs:341 | test_search_result_signature_invalid | libtest | - |
| crates/terraphim_grep/src/signatures.rs:350 | test_signature_instructions | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:167 | test_empty_results_insufficient | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:180 | test_low_results_insufficient | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:193 | test_high_coverage_sufficient | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:214 | test_coverage_calculation | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:226 | test_diversity_calculation | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:242 | test_kg_confidence_calculation | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:266 | test_judge_with_metrics_reports_measurements | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:292 | test_judge_with_metrics_empty_results | libtest | - |
| crates/terraphim_grep/src/sufficiency_judge.rs:310 | test_judge_delegates_to_judge_with_metrics | libtest | - |

### terraphim_mcp_server test_mcp_stdio (1 test)

| file:line | test | harness | attrs |
|---|---|---|---|
| crates/terraphim_mcp_server/tests/test_mcp_stdio.rs:6 | test_mcp_autocomplete_via_stdio | libtest | - |
