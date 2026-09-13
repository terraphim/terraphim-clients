# Module: F -- terraphim_grep

Run: 2026-09-11-terraphim-clients-1. Mapper: p1-F. Written 2026-09-11 23:20 BST.
Scope: `crates/terraphim_grep/src` and `crates/terraphim_grep/tests` (plus `benches/hybrid_search.rs` for completeness).
Source is unmodified. No remediation is proposed here.

## Purpose

`terraphim_grep` is a hybrid code grep: `HybridSearcher` (src/hybrid_searcher.rs) runs a
knowledge-graph concept lookup (`terraphim_rolegraph` + `terraphim_automata`) and an
`fff-search` file-content grep, boosts chunks that match KG concepts, then
`SufficiencyJudge` (src/sufficiency_judge.rs) decides whether to return search-only or
call an LLM (`LlmClient` from `terraphim_service`) for synthesis. `OpenRouterClient`
(src/openrouter_client.rs, feature `llm`) is a `reqwest` client with a 120 s timeout.
`src/main.rs` is the CLI binary (`terraphim-grep`).

Features (crates/terraphim_grep/Cargo.toml:62-72): `default = ["llm", "code-search"]`;
`llm` pulls `terraphim_service` + `reqwest` (`rustls-tls`); `code-search` pulls
`fff-search 0.8.4`; `openrouter` is opt-in and only used by the ignored e2e test.

## Unsafe-surface tally

| Category | Count | Where |
|---|---|---|
| `unsafe` keyword in source (src + tests + benches) | 4 blocks, 1 `// SAFETY` comment | src/openrouter_client.rs:166 (comment), :170, :177, :185, :189 -- all `std::env::set_var`/`remove_var` inside `test_openrouter_client_base_url_resolution`. Catalogued by section A; not duplicated here. |
| `unsafe` outside that test | 0 | rg `unsafe|SAFETY` over src, tests, benches returns only the lines above. |
| `unsafe` in `cargo +nightly expand -p terraphim_grep --lib` (no cfg(test)) | 2 | Both `unsafe impl ::core::clone::TrivialClone for ...` emitted by `#[derive(Clone, Copy)]`: `Haystack` (expansion line 231; source src/hybrid_searcher.rs:28) and `SufficiencyMetrics` (expansion line 4212; source src/sufficiency_judge.rs:34). Compiler-generated marker impls, no bodies. Expansion file: /private/tmp/claude-501/ubx-expand/terraphim_grep.rs (5338 lines, 240437 bytes). |
| `unsafe` in `--lib --tests` expansion | SKIPPED | --tests expansion SKIPPED by lead: #[tokio::test] wraps bodies in safe code; no MACRO_GENERATED env rows expected; lib expansion shows only derive marker impls. (The `cargo +nightly expand -p terraphim_grep --lib --tests` invocation exited 0 but produced an empty file, so no count was obtained before the job was closed.) |
| FFI / raw pointers / `static mut` / `transmute` / `from_utf8_unchecked` / `get_unchecked` / `catch_unwind` / `ManuallyDrop` / `mem::forget` | 0 | rg over src, tests, benches: no hits. |
| `impl Drop` | 0 | none. |
| `#[serial]` / `serial_test` | 0 | Not a dependency; no test is serialised. |

Archetype for this section: pure safe Rust; the only hazard is *reachability* of native
`getenv` and Rust-side `std::env::var` from tests that run in the same process as the
section-A `set_var`/`remove_var` test.

## Lib test binary thread model

All `#[cfg(test)]` modules in `src/` (except `src/main.rs`, which is the *bin* test
binary and a separate process) are compiled into one lib test binary and run
concurrently by the default libtest harness. The section-A test
`test_openrouter_client_base_url_resolution` (src/openrouter_client.rs:164-193) executes
`set_var`/`remove_var` in that same process.

Two distinct columns matter:

- **std env read**: `std::env::var*` from Rust. These are serialised against `set_var`
  by std's internal environment lock, so the outcome is a *logical* flake (a test can
  observe the other test's value), not a memory-safety race.
- **native getenv**: C `getenv()` reached through a `-sys` crate. This is the case the
  `unsafe` on `set_var` is about: libc `getenv` does not take the Rust lock, and a
  concurrent `setenv`/`unsetenv` can free or rewrite the buffer it returns.

Runtime: every `#[tokio::test]` here is the default current-thread flavour (no
`flavor = "multi_thread"` anywhere; rg confirms). Thread spawning therefore comes only
from `fff-search` inside `HybridSearcher::search_code`.

Reachability chain for "native getenv" and "threads" (feature `code-search`, on by default):

`HybridSearcher::search` (src/hybrid_searcher.rs:252) -> `search_code` (:341) ->
`fff_search::FilePicker::new` (:349; fff file_picker.rs:696, no I/O beyond `exists()`) ->
`picker.collect_files()` (:360; fff file_picker.rs:796) ->
`FileSync::discover_git_workdir` (fff :800, :1694) -> `git2::Repository::discover`
(git2 repo.rs:248) -> `git2::init()` (git2 lib.rs:748-756, `Once`) -> `raw::init()` ->
`git_libgit2_init` -> init list in libgit2/src/libgit2/libgit2.c:36-58 includes
`git_sysdir_global_init` (line 46) -> sysdir.c:453-465 runs every `guess` function
eagerly -> `git_sysdir_guess_home_dirs` (sysdir.c:324) calls `getenv("APP_SANDBOX_CONTAINER_ID")`
(:347) and `git__getenv(out, "HOME")` (:357); `git_sysdir_guess_xdg_dirs` (:375) calls
`git__getenv(&env, "XDG_CONFIG_HOME")` (:402) and `"HOME"` (:405); `git__getenv` is
`getenv(name)` on Unix (libgit2/src/util/util.c:786-788). Because `git2::init` is a
`Once`, only the *first* lib test to reach `collect_files` pays these native reads; later
tests are no-ops on that path, so the hazard is ordering-dependent and probabilistic.
`Repository::discover` then walks up from the tempdir; on this host the tempdir is not
inside a repository (verified: `git -C $(mktemp -d) rev-parse --show-toplevel` reports
"not a git repository"), so `spawn_git_status` (fff :1708, `std::thread::spawn`) is not
reached from tests (it is reached in production when the search path is inside a repo).

Threads on the same path: `collect_files` -> `walk_filesystem` (fff :1721) touches
`BACKGROUND_THREAD_POOL` (fff :73, `LazyLock<rayon::ThreadPool>`, built on first use;
rayon reads `RAYON_NUM_THREADS` via `std::env`, Rust-side) at :1736, and
`ignore::WalkBuilder::build_parallel()` at :1754 spawns walker threads. The `ignore`
crate reads `$XDG_CONFIG_HOME` via `std::env::var_os` (ignore gitignore.rs:604) and
`$HOME` via the deprecated `std::env::home_dir` (gitignore.rs:663-668), both Rust-side.
`picker.grep` (fff file_picker.rs:1156) uses rayon `par_iter` (fff grep.rs:1250, :1641).
File contents are read through `memmap2::Mmap::map` (fff types.rs:640, `unsafe` in the
dependency, not in this crate). LMDB (`heed`/`lmdb-master-sys`) is opened only from
`dbs::frecency`/`dbs::query_tracker` via `new_with_shared_state`; `collect_files` uses
`SharedFrecency::default()` (fff :803, shared.rs:262-268 -> `None` tracker), so LMDB is
linked but **not reached** from these tests.

Reachability chain for the three non-`unsafe` openrouter tests: `OpenRouterClient::new`
(src/openrouter_client.rs:35-56) reads `OPENROUTER_BASE_URL` with `std::env::var` (:51,
Rust-side) and calls `reqwest::Client::builder()...build()` (:39-45). `build()` pushes
`ProxyMatcher::system()` because `auto_sys_proxy` defaults to true (reqwest 0.12.28
async_impl/client.rs:418-421) -> hyper-util `Matcher::from_system()` (hyper-util 0.1.20
client/proxy/matcher.rs:109) -> `from_env` reads `REQUEST_METHOD`, `HTTP_PROXY`,
`HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` via `std::env::var`/`var_os` (:230, :325, :411-412,
Rust-side) and, since `hyper-util` feature `client-proxy-system` is enabled in this graph
(cargo tree -e features), on macOS also `SCDynamicStoreCopyProxies` through
`system-configuration` (matcher.rs:575-590, native, reads the system store, not the
process environment). TLS: reqwest features resolved to `rustls-tls-webpki-roots`;
root store is `webpki-roots` (no `SSL_CERT_FILE`/`SSL_CERT_DIR` read); rustls has both
`ring` and `aws-lc-rs` provider features unified, so `aws-lc-sys` is linked. `getenv`
in aws-lc appears only in x509 by_file.c/by_dir.c (cert-directory lookup, not on the
`Client::build` path) and test/console code; **no evidence** that aws-lc reads env during
client construction. Verdict for these three tests: std env read = yes; native getenv =
none found.

Table of every test in the lib test binary (default features `llm` + `code-search`):

| # | Test (file:line) | Runtime | std env read | native getenv | Threads spawned | reqwest/TLS | chrono/time local | -sys reached | Notes |
|---|---|---|---|---|---|---|---|---|---|
| 1 | `grep_result_serialises_sufficiency_explanation` src/lib.rs:490 | sync | no | no | no | no | no | none | serde_json only |
| 2 | `needs_synthesis_without_answer_skips_llm` src/lib.rs:607 | tokio current_thread | yes (fff `ignore`: XDG_CONFIG_HOME/HOME; rayon RAYON_NUM_THREADS; tempfile TMPDIR) | **yes** (libgit2 sysdir init, first-caller only) | rayon pool + `ignore` parallel walk | no (CountingLocalLlm is in-process) | no | libgit2-sys, libz-sys (linked via git2) | cfg `llm`+`code-search`; `AtomicUsize` counter in test double src/lib.rs:520 |
| 3 | `needs_synthesis_with_answer_invokes_llm` src/lib.rs:640 | tokio current_thread | yes (as 2) | **yes** (as 2) | as 2 | no | no | as 2 | cfg `llm`+`code-search` |
| 4 | `force_rlm_without_answer_invokes_llm` src/lib.rs:673 | tokio current_thread | yes (as 2) | **yes** (as 2) | as 2 | no | no | as 2 | cfg `llm`+`code-search` |
| 5 | `rlm_requested_only_for_explicit_flags` src/lib.rs:703 | sync | no | no | no | no | no | none | pure |
| 6 | `test_grep_options_default` src/lib.rs:726 | sync | no | no | no | no | no | none | pure |
| 7 | `insufficient_path_propagates_chunk_count` src/lib.rs:741 | tokio current_thread | yes (as 2) | **yes** (as 2) | as 2 | no | no | as 2 | cfg `code-search` |
| 8 | `search_without_llm_degrades_to_search_only` src/lib.rs:792 | tokio current_thread | yes (as 2) | **yes** (as 2) | as 2 | no | no | as 2 | cfg `code-search` |
| 9 | `search_without_thesaurus_uses_fff_mode` src/lib.rs:840 | tokio current_thread | yes (as 2) | **yes** (as 2) | as 2 | no | no | as 2 | cfg `code-search` |
| 10 | `test_answer_prompt_includes_json_instructions` src/lib.rs:882 | sync | no | no | no | no | no | none | pure string ops |
| 11 | `test_list_prompt_excludes_json_instructions` src/lib.rs:918 | sync | no | no | no | no | no | none | pure |
| 12 | `test_hybrid_results_empty` src/hybrid_searcher.rs:441 | tokio current_thread | no | no | no | no | no | none | runtime built but nothing awaited |
| 13 | `test_hybrid_results_to_chunks` src/hybrid_searcher.rs:452 | tokio current_thread | no | no | no | no | no | none | as 12 |
| 14 | `default_build_greps_populated_directory` src/hybrid_searcher.rs:484 | tokio current_thread | yes (as 2) | **yes** (as 2) | as 2 | no | no | as 2 | cfg `code-search` |
| 15 | `multiple_search_paths_merge_results` src/hybrid_searcher.rs:526 | tokio current_thread | yes (as 2) | **yes** (as 2) | as 2, two `collect_files` calls | no | no | as 2 | cfg `code-search` |
| 16 | `thesaurus_query_concepts_uses_automata_not_substring_expansion` src/hybrid_searcher.rs:594 | sync | no | no | no | no | no | none | terraphim_automata only |
| 17 | `thesaurus_query_concepts_expands_shared_normalised_term` src/hybrid_searcher.rs:604 | sync | no | no | no | no | no | none | as 16 |
| 18 | `kg_boost_promotes_matching_chunks_to_top` src/hybrid_searcher.rs:628 | sync | no | no | no | no | no | none | as 16 |
| 19 | `kg_boost_no_concepts_is_a_noop` src/hybrid_searcher.rs:649 | sync | no | no | no | no | no | none | pure |
| 20 | `kg_boost_stacks_when_multiple_concepts_match` src/hybrid_searcher.rs:659 | sync | no | no | no | no | no | none | as 16 |
| 21 | `kg_boost_does_not_match_concept_embedded_in_larger_word` src/hybrid_searcher.rs:675 | sync | no | no | no | no | no | none | as 16 |
| 22 | `kg_boost_matches_concept_at_identifier_boundary` src/hybrid_searcher.rs:685 | sync | no | no | no | no | no | none | as 16 |
| 23 | `kg_boost_keeps_author_only_chunk_below_real_auth_chunk` src/hybrid_searcher.rs:695 | sync | no | no | no | no | no | none | as 16 |
| 24 | `test_grep_options_default` src/hybrid_searcher.rs:710 | sync | no | no | no | no | no | none | duplicate of 6 |
| 25 | `test_persist_concepts_writes_markdown_files` src/kg_curation.rs:203 | sync | yes (tempfile TMPDIR via `std::env::temp_dir`) | no | no | no | no | none | fs writes in tempdir; uses a test-local copy of `persist_concepts`, not the real one |
| 26 | `test_persist_concepts_skips_existing_files` src/kg_curation.rs:239 | sync | yes (tempfile) | no | no | no | no | none | as 25 |
| 27 | `test_persist_concepts_empty_is_noop` src/kg_curation.rs:258 | sync | yes (tempfile) | no | no | no | no | none | as 25 |
| 28 | `test_extract_json_strips_markdown_fences` src/signatures.rs:167 | sync | no | no | no | no | no | none | pure |
| 29 | `test_extract_json_finds_bare_json_in_prose` src/signatures.rs:184 | sync | no | no | no | no | no | none | pure |
| 30 | `test_extract_json_strips_generic_markdown_fence` src/signatures.rs:198 | sync | no | no | no | no | no | none | pure |
| 31 | `test_extract_json_finds_array_in_prose` src/signatures.rs:215 | sync | no | no | no | no | no | none | pure |
| 32 | `test_extract_json_handles_unterminated_fence` src/signatures.rs:228 | sync | no | no | no | no | no | none | pure |
| 33 | `test_extract_json_prefers_json_fence_over_other_language` src/signatures.rs:241 | sync | no | no | no | no | no | none | pure |
| 34 | `test_search_result_signature_parse` src/signatures.rs:258 | sync | no | no | no | no | no | none | serde_json |
| 35 | `test_answer_signature_parse` src/signatures.rs:277 | sync | no | no | no | no | no | none | serde_json |
| 36 | `test_answer_signature_parse_markdown_wrapped` src/signatures.rs:296 | sync | no | no | no | no | no | none | serde_json |
| 37 | `test_concept_extraction_signature_parse` src/signatures.rs:321 | sync | no | no | no | no | no | none | serde_json |
| 38 | `test_search_result_signature_invalid` src/signatures.rs:340 | sync | no | no | no | no | no | none | serde_json |
| 39 | `test_signature_instructions` src/signatures.rs:349 | sync | no | no | no | no | no | none | pure |
| 40 | `test_empty_results_insufficient` src/sufficiency_judge.rs:166 | sync | no | no | no | no | no | none | pure |
| 41 | `test_low_results_insufficient` src/sufficiency_judge.rs:179 | sync | no | no | no | no | no | none | pure |
| 42 | `test_high_coverage_sufficient` src/sufficiency_judge.rs:192 | sync | no | no | no | no | no | none | pure |
| 43 | `test_coverage_calculation` src/sufficiency_judge.rs:213 | sync | no | no | no | no | no | none | pure |
| 44 | `test_diversity_calculation` src/sufficiency_judge.rs:225 | sync | no | no | no | no | no | none | pure |
| 45 | `test_kg_confidence_calculation` src/sufficiency_judge.rs:241 | sync | no | no | no | no | no | none | pure |
| 46 | `test_judge_with_metrics_reports_measurements` src/sufficiency_judge.rs:265 | sync | no | no | no | no | no | none | pure |
| 47 | `test_judge_with_metrics_empty_results` src/sufficiency_judge.rs:291 | sync | no | no | no | no | no | none | pure |
| 48 | `test_judge_delegates_to_judge_with_metrics` src/sufficiency_judge.rs:309 | sync | no | no | no | no | no | none | pure |
| 49 | `test_rlm_context_new` src/rlm_context.rs:116 | sync | no | no | no | no | no | none | pure |
| 50 | `test_rlm_context_with_chunks` src/rlm_context.rs:124 | sync | no | no | no | no | no | none | pure |
| 51 | `test_rlm_context_with_concepts` src/rlm_context.rs:136 | sync | no | no | no | no | no | none | pure |
| 52 | `test_build_prompt` src/rlm_context.rs:149 | sync | no | no | no | no | no | none | pure |
| 53 | `test_truncate` src/rlm_context.rs:171 | sync | no | no | no | no | no | none | pure |
| 54 | `test_openrouter_client_new_validates_inputs` src/openrouter_client.rs:157 | sync | **yes** (`OPENROUTER_BASE_URL` :51; proxy env via hyper-util) | none found | no | yes: `reqwest::Client::build` x1 (rustls config built; `SCDynamicStoreCopyProxies` native on macOS) | no | aws-lc-sys/ring linked; system-configuration-sys called | cfg `llm`; runs concurrently with the section-A `set_var` test and can observe its intermediate value |
| 55 | `test_openrouter_client_base_url_resolution` src/openrouter_client.rs:164 | sync | yes (:167) | none found | no | yes: `Client::build` x2 | no | as 54 | **section A** (the 4 unsafe env blocks); listed for completeness only |
| 56 | `test_openrouter_client_timeout_is_120_seconds` src/openrouter_client.rs:195 | sync | **yes** (as 54) | none found | no | yes: `Client::build` x1 | no | as 54 | cfg `llm` |
| 57 | `test_into_llm_client_returns_trait_object` src/openrouter_client.rs:204 | sync | **yes** (as 54) | none found | no | yes: `Client::build` x1 | no | as 54 | cfg `llm` |

chrono/`time` local-offset: `chrono 0.4.45` and `iana-time-zone` are in the graph (via
`terraphim_types`, `opendal`, `terraphim_router`); `terraphim_types` calls only
`Utc::now()` (no local offset, no TZ read) and none of those call sites are on any
terraphim_grep test path. `time 0.3.49` arrives via `tracing-appender` (fff-search) and
`zip` (terraphim_update); neither is invoked from lib tests. `jiff` via `env_logger`
and `terraphim_update` is not invoked from lib tests.

Separate processes (not concurrent with the section-A test; catalogued so they are not
misread as in-process hazards):

| Binary | Test (file:line) | Model | Env / native | Notes |
|---|---|---|---|---|
| bin `terraphim-grep` unit tests | `explicit_thesaurus_bypasses_ambiguous_role_error` src/main.rs:693 | sync | tempfile TMPDIR | `terraphim_config::ProjectConfig` in memory |
| bin | `explicit_thesaurus_with_role_uses_given_role` src/main.rs:719 | sync | none | pure |
| bin | `no_thesaurus_single_role_project_resolves` src/main.rs:727 | sync | tempfile | pure |
| bin | `no_thesaurus_multi_role_project_errors` src/main.rs:742 | sync | tempfile | pure |
| bin | `thesaurus_candidates_include_matching_role_shortname` src/main.rs:762 | sync | none | pure |
| bin | `discover_project_thesaurus_returns_shortname_match` src/main.rs:778 | sync | tempfile | fs only |
| tests/default_feature_smoke.rs | `default_feature_build_returns_nonzero_chunks` :16 | sync; spawns CLI child via `env!("CARGO_BIN_EXE_terraphim-grep")` (:22) | child reads `RUST_LOG` (tracing env-filter), `OPENROUTER_API_KEY`, `OLLAMA_*` via std (src/main.rs:390-462) | child process; libgit2 native getenv happens in the child |
| tests/no_thesaurus_cli.rs | `cli_runs_without_thesaurus` :8; `cli_help_lists_update_commands` :62 | sync; CLI child | as above | child process |
| tests/search_only_flag.rs | `search_only_flag_is_accepted` :21; `search_only_skips_llm_client_with_openrouter_key_present` :58; `help_documents_search_only_flag` :93 | sync; CLI child with `.env("RUST_LOG", ...)` and `.env("OPENROUTER_API_KEY", "sk-test-placeholder")` (:40, :73-74) | `Command::env` mutates only the child's environment, not the parent's | child process |
| tests/kg_curation_integration.rs | 3 tests :8, :58, :79 | sync | tempfile | re-implements slug/persist logic locally; does not call the crate |
| tests/router_capability_routing.rs | 3 tests :90, :111, :146 | sync | none | `#![cfg(feature = "llm")]`; in-memory `terraphim_router::Router`, no LLM |
| tests/e2e_smoke_openrouter.rs | `e2e_grep_synthesises_answer_via_free_openrouter_model` :57 | `#[tokio::test]` `#[ignore]` | `std::env::var("OPENROUTER_API_KEY")` :60, `OPENROUTER_MODEL` :69; real network via `terraphim_service::llm::build_llm_from_role` (genai / reqwest 0.13 / rustls-platform-verifier / security-framework native) | `#![cfg(all(feature = "code-search", feature = "openrouter"))]` (:16); not compiled under default features and skipped by default |
| benches/hybrid_search.rs | 4 criterion groups | `tokio::runtime::Runtime::new()` (:81, :112) multi-thread runtime; `b.to_async(&rt)` | tempfile; same fff/libgit2 path as tests 2-9 | `harness = false`, `required-features = ["code-search"]`; not part of `cargo test` |

## Concurrency surface

Note: the module doc at src/lib.rs:4 says the two pipelines run "concurrently via
`tokio::spawn`". There is no `tokio::spawn`, `join!`, `select!`, `JoinSet`, `thread::spawn`,
`rayon`, or `crossbeam` anywhere in src, tests or benches; `HybridSearcher::search`
awaits `search_kg` and then each `search_code` sequentially (src/hybrid_searcher.rs:262-276).
Doc drift, not a hazard.

Primitives actually present:

| Primitive | Site | Role |
|---|---|---|
| `Arc<HybridSearcher>`, `Arc<SufficiencyJudge>` | src/lib.rs:74-75, :85-86, :98-99 | shared ownership of immutable searcher/judge in `TerraphimGrep` |
| `Option<Arc<KgCurationRlm>>`, `Option<Arc<dyn LlmClient>>` | src/lib.rs:77, :79 (cfg `llm`) | trait-object sharing; `LlmClient: Send + Sync` is required by `terraphim_service` |
| `Arc<tokio::sync::RwLock<terraphim_rolegraph::RoleGraph>>` | src/hybrid_searcher.rs:219 (field), :239 (construction), :313 (param), :318 `graph.read().await` | only ever read-locked; no writer exists in this crate; guard held across two synchronous calls and dropped at end of `search_kg` |
| `Arc<dyn LlmClient>` | src/kg_curation.rs:13, :19; src/openrouter_client.rs:149-150; src/main.rs:356, :426 | trait-object sharing |
| `std::sync::atomic::AtomicUsize` (SeqCst) | src/lib.rs:520-527, :531, :557 (test double `CountingLocalLlm`) | call counter in tests only |
| `static WARNED: std::sync::Once` | src/hybrid_searcher.rs:414-415 | only under `cfg(not(feature = "code-search"))`; **compiled out** of the default lib test binary |
| `parking_lot = "0.12"` | Cargo.toml:41 | declared dependency; **no use site** in src, tests or benches (rg `parking_lot` returns only Cargo.toml) |
| `tokio::runtime::Runtime::new()` | benches/hybrid_search.rs:81, :112 | multi-thread runtime for criterion `to_async`; bench only |
| `#[tokio::test]` (current_thread) | src/lib.rs:607, :640, :673, :741, :792, :840; src/hybrid_searcher.rs:441, :452, :484, :526; tests/e2e_smoke_openrouter.rs:57 | each builds and drops its own runtime |
| `#[tokio::main]` | src/main.rs:477 | bin entry (default multi-thread) |
| `std::time::Instant` | src/lib.rs:157, :304, :306 | latency timing; monotonic, no env |

Send/Sync: no manual `unsafe impl Send`/`Sync`. `RlmSignature: Send + Sync` supertraits
(src/signatures.rs:64). `RetrievedChunk.haystack: &'static str` (src/hybrid_searcher.rs:42)
is populated only from literals (`"code"`, `"docs"`).

## Library-trait invariant surface

Manual trait impls (rg `impl (Hash|PartialEq|Eq|Ord|PartialOrd|Borrow|Iterator|ExactSizeIterator|DoubleEndedIterator) for` returns nothing; `size_hint` absent):

| Impl | Site | Contract notes |
|---|---|---|
| `impl Default for GrepOptions` | src/hybrid_searcher.rs:16 | plain |
| `impl From<Document> for RetrievedChunk` | src/hybrid_searcher.rs:46 | `doc.rank.unwrap_or(0) as f64`; lossless for u64 < 2^53 |
| `impl Default for HeuristicThresholds`, `impl Default for SufficiencyJudge` | src/sufficiency_judge.rs:11, :144 | plain |
| `impl Default for KgCurationRlm` | src/kg_curation.rs:131 | cfg `not(llm)` only |
| `impl Default for HaystackArg`, `From<HaystackArg> for Haystack`, `From<Haystack> for HaystackArg` | src/main.rs:117, :123, :133 | bin only |
| `#[async_trait] impl LlmClient for OpenRouterClient` | src/openrouter_client.rs:59 | `summarize` always errors (documented at :14-17) |
| `#[async_trait] impl LlmClient for CountingLocalLlm` | src/lib.rs:542 (test) | in-process double |
| `impl RlmSignature for SearchResultSignature / AnswerSignature / ConceptExtractionSignature` | src/signatures.rs:83, :118, :146 | parse via `extract_json` + serde |
| Derived `PartialEq, Eq, Copy, Default` on `Haystack` | src/hybrid_searcher.rs:28-33 | derive-generated; the two expanded `unsafe impl TrivialClone` come from here and from `SufficiencyMetrics` |

Total-order and slicing invariants (panics, not UB, but they are the crate's contract surface):

- `sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(Equal))`
  at src/hybrid_searcher.rs:207-212 (`boost_chunks_with_kg`) and :430-434 (`fuse_and_rank`).
  `RetrievedChunk.relevance_score: f64` and `KgConcept.score: f64` are `pub` and
  `Deserialize`; a NaN produces an inconsistent comparator, which `slice::sort_by` may
  reject with a panic since Rust 1.81. NaN is reachable only via caller-supplied or
  deserialised scores; internal scores are `1.0`, `rank as f64`, or `score / max` with
  `max > 0.0` guarded at :115-118.
- `has_concept_boundaries` at src/hybrid_searcher.rs:150-154 slices `text[..start]` and
  `text[end..]` with `(usize, usize)` byte offsets returned by
  `terraphim_automata::find_matches`. Invariant relied on: those offsets are char
  boundaries of `text`. Violation is a panic.
- `extract_json` at src/signatures.rs:10-59: `trimmed[start + 7..]`, `block[..end]`,
  `trimmed[start..=i]` are all derived from `str::find` results or ASCII byte indices
  (`{`, `[`, `}`, `]`, `"`), so they land on char boundaries.
- `RlmContext::truncate` (src/rlm_context.rs:82-99) reasons in bytes (`.len()`), never
  slices strings; `Citation.excerpt` uses `.chars().take(100)` (src/lib.rs:335).

## Native/env-reading dependencies reachable from tests

`cargo tree -p terraphim_grep -e normal --offline` (saved at
/private/tmp/claude-501/ubx-expand/terraphim_grep.tree.txt, 1286 lines). `-sys` crates and
TLS/crypto/time crates in the graph, with reachability from the **lib test binary**:

| Crate | Pulled in by | Reached from lib tests? | Native `getenv`? |
|---|---|---|---|
| `libgit2-sys 0.18.5+1.9.4` (+ `libz-sys 1.1.29`) | `git2 0.20.4` <- `fff-search 0.8.4` (feature `vendored-libgit2`) | **yes**, tests 2-4, 7-9, 14-15 via `Repository::discover` | **yes**: `git_sysdir_global_init` reads `APP_SANDBOX_CONTAINER_ID`, `HOME`, `XDG_CONFIG_HOME` through `getenv` (sysdir.c:347, :357, :402, :405; util.c:788). `git2::init` is a `Once`, so first caller only. |
| `lmdb-master-sys 0.2.6` | `heed 0.22.1` <- `fff-search` | no (`SharedFrecency::default()` holds no tracker; LMDB env opened only in `new_with_shared_state` paths) | n/a |
| `aws-lc-sys 0.41.0` | `aws-lc-rs` <- `rustls 0.23.40` (feature unified alongside `ring 0.17.14`) <- `hyper-rustls` <- `reqwest 0.12.28` | linked; tests 54-57 build a `reqwest::Client` (rustls `ClientConfig` with `webpki-roots`) | none found on the client-build path (`getenv` only in x509 `by_file.c`/`by_dir.c` and test code) |
| `system-configuration-sys 0.6.0` (+ `core-foundation-sys`) | `system-configuration` <- `hyper-util 0.1.20` (feature `client-proxy-system` enabled) <- reqwest 0.12 | **yes**, tests 54-57 via `ProxyMatcher::system()` -> `SCDynamicStoreCopyProxies` (hyper-util matcher.rs:575-590) | no (reads the macOS dynamic store, not `environ`); the env-var proxy lookup beside it is `std::env` (matcher.rs:230, :325, :411) |
| `security-framework-sys 2.17.0` | `rustls-platform-verifier 0.7.0` <- `reqwest 0.13.4` <- `genai 0.6.5` <- `terraphim_service` | no (only via `build_llm_from_role` in the ignored `openrouter`-feature e2e test) | n/a |
| `libsqlite3-sys 0.30.1` | `rusqlite` <- `terraphim_persistence` <- `terraphim_config`; `sqlx-sqlite` <- `opendal` | no (no persistence call from grep tests) | n/a |
| `zstd-sys 2.0.16`, `libbz2-rs-sys 0.2.5` | `terraphim_persistence`; `zip` <- `terraphim_update` | no | n/a |
| `fsevent-sys 4.1.0` | `notify` <- `terraphim_file_search` <- `terraphim_middleware` | no | n/a |
| `dirs-sys 0.4.1 / 0.5.0` | `dirs` (transitive) | not on grep test paths | no: `std::env::var_os("HOME")`/`XDG_CONFIG_HOME` (dirs-sys lib.rs:34, :115, Rust-side) and libc `getpwuid_r` (:52), no `getenv` |
| `openssl` / `openssl-sys` / `native-tls` | **absent** from the graph | -- | -- |
| `memmap2 0.9.10` | `fff-search` | yes (file content mapping in grep) | no env |
| `chrono 0.4.45` + `iana-time-zone 0.1.65` | `terraphim_types`, `opendal`, `terraphim_router`, `terraphim_persistence` | no call site on grep test paths; `terraphim_types` uses `Utc::now()` only | not verified (its macOS backend was not read); not on any test path |
| `time 0.3.49` / `time 1.1.0` / `jiff 0.2.28` | `tracing-appender` (fff), `zip`, `env_logger`, `terraphim_update` | no | n/a |
| `tokio 1.52.3` (features `full`) | workspace | yes (current-thread runtimes) | reads `TOKIO_WORKER_THREADS` via `std::env::var` (tokio loom/std/mod.rs:90) only for multi-thread runtimes, i.e. benches and the bin |
| `tempfile 3.27.0` | dev-dependency | yes (tests 2-4, 7-9, 14-15, 25-27) | `std::env::temp_dir()`, Rust-side |

Rust-side `std::env::var*` sites in this crate's own source: src/openrouter_client.rs:51
(`OPENROUTER_BASE_URL`, reached by tests 54-57); src/openrouter_client.rs:167 (section A);
src/main.rs:269 (`current_dir`), :390, :394, :436, :439, :451, :462 (bin only);
tests/e2e_smoke_openrouter.rs:60, :69 (ignored test). `env!("CARGO_PKG_VERSION")`
(src/main.rs:157, src/openrouter_client.rs:42) and `env!("CARGO_BIN_EXE_terraphim-grep")`
(tests/*.rs) are compile-time macros, not runtime reads.

## Notable patterns

- **Same-process env write plus native reader.** The section-A `set_var`/`remove_var`
  test (src/openrouter_client.rs:164-193) shares the lib test binary with eight tests
  whose first-to-run instance executes libc `getenv` inside `git_libgit2_init`. The
  `// SAFETY: test code, single-threaded, no concurrent env access.` claim at :166 is
  contradicted by the default multi-threaded libtest harness and by the presence of those
  eight tests. Whether the window is ever hit depends on scheduling and on which test
  calls `git2::init` first.
- **Rust-side observers of the same variable.** Tests 54, 56, 57 read
  `OPENROUTER_BASE_URL` at :51 while test 55 is mutating it; the value they observe is
  unspecified but memory-safe. They only assert on `model`/`api_key`/`name`, so the flake
  is currently latent.
- **Doc drift on concurrency** (src/lib.rs:4) as described under Concurrency surface.
- **Unused `parking_lot` dependency** (Cargo.toml:41).
- **`#[derive(Copy)]`-generated `unsafe impl TrivialClone`** accounts for the entire
  non-test `unsafe` count in the expansion; there is no author-written `unsafe` outside
  section A.
- **Test doubles are real trait impls.** `CountingLocalLlm` (src/lib.rs:519-570) and the
  ignored e2e test both drive the real `LlmClient` contract; the kg_curation and
  kg_curation_integration tests instead copy `persist_concepts` locally
  (src/kg_curation.rs:159-201; tests/kg_curation_integration.rs) and never execute the
  crate's own `KgCurationRlm::persist_concepts` (src/kg_curation.rs:77-129).
- **Thread spawning inside a current-thread tokio runtime.** rayon and `ignore` walker
  threads are created from a blocking call (`collect_files`, `grep`) made directly inside
  an `async fn` body on the current-thread runtime (src/hybrid_searcher.rs:349-402); no
  `spawn_blocking`. Not UB, but it is the crate's real thread surface.

## Open questions

1. Closed by lead: --tests expansion SKIPPED by lead: #[tokio::test] wraps bodies in safe
   code; no MACRO_GENERATED env rows expected; lib expansion shows only derive marker impls.
2. Does the first `git2::init` in the lib test binary ever overlap in time with the
   `set_var` window in test 55? A phase-2 experiment (loop the lib test binary with
   `--test-threads` at default and with `RUST_TEST_THREADS=1`, or Miri on a reduced
   reproduction) is needed to say more than "reachable".
3. On this macOS host `Repository::discover` from a `$TMPDIR` fixture finds no repository,
   so `spawn_git_status` and `Repository::open` (which would run further libgit2 config
   and env lookups) are not exercised by tests. CI runners whose temp directory sits
   inside a checkout would exercise them; that environment was not checked.
4. Closed: reqwest 0.12.28 has feature `__rustls-ring` enabled in this graph and its
   client build calls `rustls::ClientConfig::builder_with_provider(ring)` explicitly
   (reqwest async_impl/client.rs:769-776), so tests 54-57 use `ring`; `aws-lc-sys` is
   linked but not selected as provider. Neither provider was found to read the process
   environment.
5. `terraphim_rolegraph` resolves to 1.20.2 (registry) despite the manifest asking for
   `1.15.0` (Cargo.toml:26); the version examined is the resolved one. `RoleGraph::new_sync`
   (rolegraph lib.rs:366-382) is pure in-memory Aho-Corasick construction with no env,
   thread or persistence access.
