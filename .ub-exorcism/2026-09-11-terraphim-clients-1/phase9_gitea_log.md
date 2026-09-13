# Phase 9 Gitea log

Run: 2026-09-11-terraphim-clients-1. Tracker: Gitea (git.terraphim.cloud), repository terraphim/terraphim-clients unless stated. Written by the Phase 9 issue author, 2026-09-12 14:37 to 15:05 BST. Sources: phase8_remediation_plan.md (sections 3 to 8, 12, 13, Delta 2, Delta 3), phase4_unified_findings.md (rows UF-001, UF-002, UF-004, UF-036 to UF-040, UF-043 and lead addenda 2 to 6), UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md (EXP-001b, 002b, 002c, 010, 014, 102, 103, 104, 201, 204, 801 to 806). No project source was edited; no commit was made; nothing was filed on GitHub or any external tracker.

Duplicate check before filing: all 48 open issues in terraphim-clients and all 391 open issues in terraphim-ai were paged through the API (limit 50 until an empty page; `gtr list-issues` defaults to 20 and hides #252). No existing issue matched any title below. #280 already covers UF-001 and was commented on, not duplicated. terraphim-ai #2687 (LLM_PROXY_URL shared across concurrent tests) and #3022 (secret via unsafe set_var) are neighbours of the EnvVarGuard finding and are cross-referenced from the upstream issue.

Labels: terraphim-clients has only `distribution`, `enhancement`, `infra/r2` and `status/in-progress`. `bug`, `security`, `ci`, `test` and `docs` do not exist and were not created (not authorised). `enhancement` was applied to #310 (lint lever) and #313 (CI hardening) only; the UB and robustness issues carry no label. terraphim-ai has `type/bug`, applied to #3378.

Dependency direction: `gtr add-dep --issue X --blocks Y` records "X is blocked by Y" (gtr help: `-issue` is the one being blocked, `-blocks` the blocker). The first edge (#280 blocked by #288) was verified in `gtr graph` as `{"from": 280, "to": 288, "type": "depends_on"}` before the rest were added; all edges below were re-read from the graph afterwards. Parenthood is recorded as "Parent: #252" in every body (there is no parent field); the pre-existing edge "#280 blocked by #252" was left as found and not replicated.

Gitea health: the API answered normally throughout (the 2026-09-12 11:45 BST disk-full outage had cleared). One scripting error (zsh not splitting a flag variable) failed a whole batch cleanly before anything was created; the batch was rerun under bash. No issue was created twice.

## Issues created (terraphim-clients)

| Number | Title | Type | Blocked by | Blocks |
|---|---|---|---|---|
| #280 (existing) | UB (confirmed on Linux): terraphim_grep lib tests mutate the environment while libgit2 reads it on sibling test threads | implementation (UF-001) | #252 (pre-existing edge), #288, #289 | #310 |
| #288 | test(grep): EXP-801 LD_PRELOAD probe loop as the #280 regression instrument (fails on 1120180, passes after the injection commit) | test | | #280 |
| #289 | docs(grep): remove the false SAFETY comment at openrouter_client.rs:166; document with_base_url; BUILD.md note that cargo test runs libtest in-process | docs | | #280 |
| #290 | UB (mechanism confirmed): tui_service_tests EnvVarGuard mutates HOME/XDG_CONFIG_HOME/TERRAPHIM_DATA_PATH on a multi-threaded binary; inject the settings dir instead (UF-002, EXP-002c) | implementation (UF-002) | #291, #292 | #310 |
| #291 | test(agent): EXP-802 probe loop on tui_service_tests as the UF-002 regression instrument (main process setenv=0 after TuiService::new_with_settings_dir) | test | | #290 |
| #292 | docs(agent): TuiService::new_with_settings_dir; remove the EnvVarGuard usage note; BUILD.md and deny.toml rationale (UF-002) | docs | | #290 |
| #293 | CONFIRMED UB on a production path: fff-search maps files during terraphim-grep search_code and MCP grep_files with the watcher disabled; a concurrent writer yields invalid-UTF-8 Strings and truncation SIGBUSes the process (UF-039, EXP-104) | implementation (UF-039, commit 0, own PR) | #294, #295 | |
| #294 | test: EXP-805 rerun of the EXP-104 harness with the post-fix no-mmap option block; ci_guards no_fff_picker_maps_files (UF-039) | test | | #293 |
| #295 | docs: helper comment and BUILD.md/ci-gap-analysis note that fff-search content mapping is disabled workspace-wide and why; drop the pin when fff-search ships the upstream fix (UF-039) | docs | | #293 |
| #296 | CONFIRMED UB on a production path: hostile non-UTF-8 entry names under a user-supplied search path inflate fff-search's lossy path storage past 4096 bytes; read_to_buf returns an invalid str (UF-040, EXP-201); also dir_name panic and silent drop of non-UTF-8 files | implementation (UF-040, commit 0b, same PR as #293) | #297, #298 | |
| #297 | test: EXP-806 rerun of the EXP-201 geometries through HybridSearcher::search and the three MCP tools; Linux-only integration tests; ci_guards arm (UF-040) | test | | #296 |
| #298 | docs: path_guard rationale (non-UTF-8 and over-long entries refused, TOCTOU residual, keep or remove when fff-search ships the upstream fix); BUILD.md/ci-gap-analysis note; error text in the terraphim-grep and MCP tool docs (UF-040) | docs | | #296 |
| #299 | LIKELY-UB: terraphim_update state.rs tests set HOME et al. on a 130-test binary under a false SAFETY comment; inject the history path (UF-004, EXP-010) | implementation (UF-004) | #300, #301 | #310 |
| #300 | test(update): EXP-803 probe loop on the terraphim_update lib binary; setenv count 52 -> 2 -> 0 (UF-004 fix; UF-036 count arm) | test (shared by UF-004 and UF-036) | | #299, #302 |
| #301 | docs(update): remove the SAFETY comment at state.rs:130; document the history_path_in and _at variants (UF-004) | docs | | #299 |
| #302 | self_update set_ssl_vars!() writes SSL_CERT_FILE/SSL_CERT_DIR from the GitHub backend on Linux: ignore the live-network test; upstream the macro gating; production path assessed (UF-036/UF-037, EXP-102/EXP-105) | implementation (UF-036) | #300, #303 | |
| #303 | docs(update): #[ignore] rationale for the live-network GitHub check; self_update pin and upstream note; the lint is blind to dependency writers (UF-036) | docs | | #302 |
| #304 | session-analyzer: empty pattern in tools.toml panics find_matches on non-ASCII text; reject empty patterns at load time (UF-038, EXP-103, NOT-UB) | implementation (UF-038, robustness) | #305, #306 | |
| #305 | test(session-analyzer): EXP-103 harness as loader and matcher unit tests (empty and whitespace-only patterns rejected; get-sliced matches never panic) (UF-038) | test | | #304 |
| #306 | docs(session-analyzer): tools.toml documentation states that empty patterns are rejected (UF-038) | docs | | #304 |
| #307 | learnings hook: parse_correction_pattern applies byte offsets from a lowercased/trimmed copy to the original string; panics on non-ASCII prompts and stores wrong corrections for ASCII (UF-043, EXP-204, NOT-UB) | implementation (UF-043, robustness) | #308, #309 | |
| #308 | test(agent): EXP-204 corpus as parse_correction_pattern unit tests (3 panics and 7 wrong tuples before, 0 after) (UF-043) | test | | #307 |
| #309 | docs(agent): parse_correction_pattern rustdoc: offsets on the sliced string, ASCII-only case folding, never panics (UF-043) | docs | | #307 |
| #310 | chore(workspace): forbid unsafe_code in every crate and target; ci_guards manifest test; deny terraphim_test_utils (headline lever for UF-001/002/004/013, EXP-804) | implementation (lever; label `enhancement`) | #280, #290, #299, #311, #312 | |
| #311 | test(workspace): EXP-804 fault-injection patch proves the unsafe_code lint, the ci_guards manifest test and the deny.toml ban each fire (lever) | test | | #310 |
| #312 | docs: unsafe_code policy in BUILD.md and ci-gap-analysis; nextest.toml comment; coverage-lane exposure and the isolation dissent recorded (lever) | docs | | #310 |
| #313 | ci: switch both coverage lanes from in-process cargo llvm-cov to cargo llvm-cov nextest; preset SSL_CERT_FILE/SSL_CERT_DIR in CI (Lead addendum 2, EXP-102 mitigation; #254 track) | CI hardening (label `enhancement`; #254 referenced in prose, no edge) | | |

## Issue created (terraphim-ai)

| Number | Title | Type | Notes |
|---|---|---|---|
| terraphim/terraphim-ai#3378 | terraphim_test_utils: set_env_var/remove_env_var/EnvVarGuard are safe wrappers over unsafe set_var whose # Safety rule (#[serial]) cannot be satisfied in a mixed binary (EXP-014); make them unsafe fn with a real contract, or delete the crate | upstream root cause (UF-003), label `type/bug` | References terraphim/terraphim-clients#252; cross-linked from #290 and #310. Not a blocker of the local fix (the workspace drops and bans the crate). |

## Deviations from the brief, stated

- UF-036 has no separate test issue. The plan (section 6) names EXP-803's count arm as its test dependency, and EXP-803 is #300 (the UF-004 test issue), so #300 was made to block #302 as well rather than filing a duplicate probe. #302 has its own docs issue (#303).
- UF-043 has no section in phase8_remediation_plan.md. #307 was written from EXP-204 (hook.rs:227-275, :235, :238-240, :268-270); its body says so and proposes the fix shape (trim the original first, search a byte-length-preserving `to_ascii_lowercase` fold, slice the same string, replace indexing with `get`). It needs the remediation architect's (p8-remediation) sign-off before implementation.
- The UF-039 and UF-040 implementation issues carry no tracker edge between them; the plan's sequencing (commit 0 then 0b, same production-path PR) is stated in both bodies.
- `enhancement` is the only label applied; the requested `bug`/`security`/`ci`/`test`/`docs` labels do not exist in terraphim-clients.

## Comment added to #280

Posted 2026-09-12 (author: the session token's user). Content: the plan's UF-001 winner (A, constructor injection `with_base_url` plus pure `resolve_base_url`, plus the workspace lint; rubric 19/20) with the concrete code sketch and the three env-free assertions; the runners-up with scores (B `#[serial]` x11, 11/20; C one-test binary, 15/20, recorded as the fallback with a `ci_guards` count test); the Lead addendum 2 scope correction (nextest test lanes not exposed; both coverage lanes and `cargo test` exposed; the instrument must run the libtest binary directly); the wiring (#280 blocked by #288 test and #289 docs; #280 blocks #310; sibling issue numbers); the commit convention (commit 1, `fix(grep): inject the OpenRouter base URL; no env mutation in lib tests (Refs #280)`, branch `task/252-env-unsafe-remediation`, PR `Fix #280: remove env mutation from tests; forbid unsafe_code workspace-wide`); and the blockers (bigbox disk, #281 orphans, rogue runner). Full text in the scratchpad file `comment-280.md` used for posting; it is reproduced on the issue itself.

## Upstream issue texts for the user to file (not filed by the agent)

### fff-search, for UF-039 (plan D2.1)

Repository: the crate declares no `repository`; the sole crates.io owner is `dmtrKovalenko` (github.com/dmtrKovalenko, the fff.nvim project). Before filing, locate the crate's `Cargo.toml` in that repository's workspace and check whether 0.10.6 or the 0.10.7 nightlies already changed `get_cached_content`, which would turn this into an upgrade task.

Title: `get_cached_content` maps files regardless of `enable_mmap_cache` and without a watcher when constructed via `FilePicker::new`; the SAFETY comment's precondition is unsatisfiable for that constructor.

Body: In fff-search 0.8.4, `get_cached_content` (types.rs:608-646) calls `memmap2::Mmap::map` (:640) for every 16 KiB to 10 MiB file during every grep, whatever `enable_mmap_cache` says. The SAFETY argument (types.rs:598-607, :637-639) relies on the background watcher invalidating mappings when files change, but `FilePicker::new` (file_picker.rs:696-730) never installs a watcher (`background_watcher: None` at :717); `watch` is only honoured by `new_with_shared_state`. With a concurrent in-place writer of the same length, `PlainTextSink::matched` (grep.rs:750) returns `line_content` Strings containing invalid UTF-8, because `from_utf8_lossy(..).into_owned()` validates and copies in two separate reads over the live mapping (observed 440 invalid of 186 M lines; 2,039 of 93 M under ASan); with a concurrent truncation the process dies with SIGBUS inside `memchr::One::find_raw` under `SliceByLine<PlainTextMatcher, PlainTextSink>::run`. `GrepMode::Fuzzy` additionally reaches `from_utf8_unchecked` at grep.rs:1710 after a single whole-file check. Proposed fix: have `get_cached_content` return `None` unless a watcher is installed (`has_watcher()` gating), or validate per line and read into the caller's buffer when unwatched; and make `enable_mmap_cache: false` actually disable mapping. Consumers today can only refuse mapping through an explicit `ContentCacheBudget { max_files: 0, .. }`; note that `ContentCacheBudget::zero()` and `max_file_size: 0` also disable the slow path and silence grep entirely, and `from_overrides` cannot express `max_files: 0`.

### fff-search, for UF-040 (plan D3.1)

Same repository check as above; also check whether 0.10.6 or the 0.10.7 nightlies already changed `read_to_buf` or the lossy path storage.

Title: `ChunkedString::read_to_buf` truncates at `PATH_BUF_SIZE` and returns `from_utf8_unchecked` of the prefix; with lossy path storage a non-UTF-8 entry name inflates 3x and the cut lands inside `U+FFFD`, so `FileItem::absolute_path` and `DirItem::dir_name` return invalid `str`/`String`/`PathBuf` through the safe API.

Body: fff-search 0.8.4 stores every relative path as a `String` from `to_string_lossy().into_owned()` (file_picker.rs:232, :352, :406), so on Linux every invalid byte in an entry name becomes a three-byte `U+FFFD`. `ChunkedString::read_to_buf` (simd_path.rs:170-186) copies `byte_len.min(buf.len())` bytes and returns `from_utf8_unchecked(&buf[..total])`; its only comment (:168) documents the truncation, not the UTF-8 hazard. When the inflated relative path exceeds `PATH_BUF_SIZE` (4096) the cut lands inside a `U+FFFD` and `FileItem::absolute_path` (types.rs:271-275, reached from `get_content_for_search` :678, `get_cached_content` :635 and `collect_files` file_picker.rs:831) returns an invalid `str`/`PathBuf`. Reproducer: six nested directories each named 250 x `0xFF` on Linux (ext4); observed 9 invalid-UTF-8 path strings across the 6x249, 6x250, 6x251, 7x200 and 14x100 geometries with 2x250 and 3x255 clean. `write_dir_name` (types.rs:153-164) additionally panics with `start byte index 10724 is out of bounds for string of length 4096` for 15 x 255. Non-UTF-8 entries are also unsearchable, because the lossily stored name no longer opens the file (0 matches with `total_files_searched=1`). Suggested fix: truncate on a char boundary (`floor_char_boundary`) or return an error when `byte_len > buf.len()`; build long paths through a heap `String` in `absolute_path`; bound the `dir_name` offset by the written length; longer term, store `OsString`/bytes instead of a lossy `String` so non-UTF-8 entries stay openable. `FilePickerOptions` (file_picker.rs:434-448) offers no path-length cap, strict-UTF-8 mode or non-lossy storage switch, so consumers can only pre-walk and refuse hostile trees.

### self_update (github.com/jaemk/self_update), for UF-036 (plan section 6)

Title: `set_ssl_vars!` calls `std::env::set_var` from library code; unsound with concurrent `getenv` on glibc.

Body: `src/macros.rs:12-24` defines `set_ssl_vars!` as `if var_os("SSL_CERT_FILE").is_none() { set_var("SSL_CERT_FILE", ..) }` (and likewise `SSL_CERT_DIR`), `#[cfg(target_os = "linux")]`, and it is invoked from `src/backends/github.rs:157, 518, 548, 587` and `src/lib.rs:692`, that is inside `get_latest_release` and the update paths that callers run from worker threads (in our case tokio `spawn_blocking` while other runtime threads are live). `std::env::set_var` is `unsafe fn` in edition 2024 precisely because glibc `setenv` may reallocate `environ` while an unlocked native `getenv` (the resolver's `RES_OPTIONS`/`LOCALDOMAIN` reads inside `getaddrinfo`, OpenSSL's own `SSL_CERT_FILE` probe, sqlite, libgit2) walks it on another thread; that pair is a C11 data race. A library should not mutate the process environment on behalf of its caller. Suggested fix: gate the macro behind the `openssl` feature (rustls-based consumers never need it) or drop it and document that callers who need `SSL_CERT_FILE`/`SSL_CERT_DIR` should set them before spawning threads; if it must stay, make the behaviour opt-in via the builder and document the thread-safety contract. Observed in self_update 0.42.0 from a 130-test binary at default parallelism (two `setenv` calls per run from one test).

## What could not be done

- No `bug`, `security`, `ci`, `test` or `docs` labels exist in terraphim-clients; none were created, so most issues are unlabelled.
- No assignees were set (the brief did not name an agent identity to assign; claiming is left to Phase 13).
- The three upstream issues above were not filed (external trackers are out of scope for the agent).
- EXP-801 to EXP-806 cannot run until bigbox has disk space again (or an alternative glibc host is chosen) and #281's orphans are killed; this is recorded in each test issue.


## Phase 10 fresh-eyes verification (2026-09-12, SOLO)

`gtr graph --owner terraphim --repo terraphim-clients` was re-read independently. Result: **21 edges** created by this phase, all present and all pointing the way this log describes, plus the pre-existing `#280 depends_on #252`. The prose above says "all 22 edges"; the table itself lists 21 (two each for #280, #290, #293, #296, #299, #302, #304 and #307, and five for #310), so the figure of 22 appears to include the pre-existing #252 edge. Corrected to avoid a later reader hunting for a missing edge.

Direction verified rather than assumed: the graph reports `{"from": 280, "to": 288, "type": "depends_on"}`, i.e. `--issue X --blocks Y` records "X is blocked by Y", which is the reading this log and CLAUDE.md use. No inverted edge was found anywhere in the set.

Issue bodies were spot-checked against the plan they cite for the three retractions that would have misled Phase 13, and none is present: no body offers `ContentCacheBudget::zero()` or `max_file_size: 0` as the UF-039 mitigation, none cites `grep.rs:1710` as the workspace mechanism, and #302 does not offer `#[serial]` as the UF-036 fix. Acceptance criteria were checked for testability; the one that looked internal, EXP-805's `cached_count = 0` in #294, is in fact observable from outside fff-search through the public `FilePicker::cache_budget()` accessor. Comments recording the findings were posted on #293, #294, #296, #302 and #310.
