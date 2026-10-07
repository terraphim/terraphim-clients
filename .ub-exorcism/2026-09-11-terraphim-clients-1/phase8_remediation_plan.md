# Phase 8 remediation plan

Run: 2026-09-11-terraphim-clients-1 (Exhaustive; archetype pure-safe-with-env-unsafe; tracking issue terraphim/terraphim-clients#252). Written by the Phase 8 remediation-architect, 2026-09-12 10:55 BST. Plan only: no source file was edited, no commit was made. Phase 13 executes this plan; later convergence rounds append to the "Delta" section at the end rather than rewriting the body.

Inputs: `phase4_unified_findings.md` (with the lead addendum), `UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md` (EXP-001b, EXP-002b, EXP-002c, EXP-010, EXP-014), `phase3_dynamic_findings.md` and `phase3_raw/envprobe*/`, `phase1_unsafe_surface_inventory.md`, `phase5_experiment_results/summary.md`, the skill's REMEDIATION-PATTERNS.md (rubric, Shapes 1-20) and REMEDIATION-PRINCIPLES.md (Principles 1-12), and the current tree at commit ae06cd2 (`git status` clean apart from `.agent/` and this workspace; the diff since 1120180, the commit the audit probed, touches only Cargo.lock, deny.toml, the two workflows, `.config/nextest.toml`, `rust-toolchain.toml` and `docs/ci-gap-analysis.md`, so every `crates/` line cited by earlier phases is unchanged). Every file:line below was re-read from that tree on 2026-09-12; line numbers are current, not copied from earlier phases.

## 0. Decisions at a glance

| Finding | Status | Winner | Deciding axis | Runners-up (score) | Proves original | Proves remediation |
|---|---|---|---|---|---|---|
| UF-001 grep `OPENROUTER_BASE_URL` writer vs libgit2 `getenv` | CONFIRMED_UB | A: constructor injection (`with_base_url` + pure `resolve_base_url`) plus workspace `unsafe_code = "forbid"` | Maintainability (lint makes regression a compile error) | B `#[serial]` on writer and all readers (11), C one-test binary (15) | EXP-001b | EXP-801, EXP-804 |
| UF-002 `EnvVarGuard` in tui_service_tests vs sqlite `getenv` | CONFIRMED_UB (mechanism) | A: settings-dir injection (`TuiService::new_with_settings_dir`) plus `DeviceStorage::init_memory_only()`, drop `EnvVarGuard`, plus the same lint | Correctness margin (removes the writer regardless of reader ordering or the upstream fix) | B `#[serial]` on all 10 tests (12), C one-test binary (15) | EXP-002c (EXP-002b for the shipped ordering) | EXP-802, EXP-804 |
| UF-004 terraphim_update `setup_temp_config_dir` (5 x `set_var`) | LIKELY-UB (lead) | A: history-path injection (`history_path_in`, `*_at` variants), delete the helper and its SAFETY comment | Correctness margin | B `#[serial]` on all 130 tests (7), C own binary (15) | EXP-010 (window shown, reader unobservable) | EXP-803, EXP-804 |
| UF-036 self_update `set_ssl_vars!()` in the same binary | SUSPICIOUS | A: `#[ignore]` the live-network test locally; upstream issue on self_update; production-path hypothesis escalated to the lead | Blast radius (dependency writer; only the upstream owns the fix) | B CI env exports (14), C `[patch.crates-io]` fork (14, corrected by Phase 10) | EXP-010 note | EXP-803 (count arm) |
| Headline lever | n/a | `[workspace.lints.rust] unsafe_code = "forbid"` + `[lints] workspace = true` in all 11 members + `ci_guards` manifest test + `deny.toml` ban on `terraphim_test_utils` | Coverage of `tests/*.rs` (attribute form cannot reach them) | `#![forbid(unsafe_code)]` per crate root; isolation-only | n/a | EXP-804 |

Zero `unsafe` remains in all 11 crates after the plan (the six inventoried blocks are the only source-direct `unsafe` in the workspace, `phase1_unsafe_surface_inventory.md` section A; the mcp_server lib expansion re-checked today has 0 `unsafe` tokens in 10,636 lines, closing the one expansion Phase 1 skipped).

## 1. Scope-of-exposure correction (read before the per-finding sections)

The audit assumed both CI lanes run the libtest binaries at default parallelism. They do not, uniformly:

| Lane | Command (current tree) | Process model | Exposed to UF-001/UF-002/UF-004? |
|---|---|---|---|
| GitHub `test` job | `.github/workflows/ci.yml:57` `cargo nextest run --workspace --lib --profile ci` | nextest: one process per test | No (a test cannot race a sibling that lives in another process) |
| native-ci `test` job | `.gitea/workflows/native-ci.yml:73` `cargo nextest run --workspace --lib --bins --tests --examples --profile ci` | nextest | No |
| GitHub `coverage` job | `.github/workflows/ci.yml:79` `cargo llvm-cov --workspace --lib --lcov ... --ignore-run-fail` | plain libtest binaries, in-process, default parallelism, ubuntu-latest (glibc) | Yes: terraphim_grep lib (UF-001) and terraphim_update lib (UF-004, UF-036). Not UF-002 (a `--test` binary, excluded by `--lib`) |
| native-ci `coverage` job | `.gitea/workflows/native-ci.yml:94` same command on bigbox (glibc 2.39) | plain libtest | Yes, same two binaries |
| Developer default | `BUILD.md:12-15` canonical `cargo test --workspace --no-fail-fast` | plain libtest | Yes, all three binaries |
| Audit probes | libtest binaries executed directly (EXP-001b, EXP-002b, EXP-010) | plain libtest | This is what was measured |

Consequences for this plan. The CONFIRMED_UB verdicts stand: the binaries are what BUILD.md tells every developer to run and what both coverage lanes run on glibc. The correction only narrows the "both CI test lanes" wording in `phase4_unified_findings.md` row UF-001/UF-002 to "both coverage lanes and every `cargo test` invocation". It also fixes the regression instrument: any fails-before/passes-after test must execute the libtest binary directly under the LD_PRELOAD shim, never through nextest, which would pass trivially. Finally, `.config/nextest.toml:27-32` `serial-external-state` (max-threads = 1) serialises whole binaries against each other; it says nothing about threads inside a binary, so it is not a remediation candidate for in-process races and is listed under candidate C only as a co-location note.

## 2. Shape identification

None of Shapes 1-20 in REMEDIATION-PATTERNS.md covers this. Recorded here as a new shape for the playbook (to be appended to REMEDIATION-PATTERNS.md by the skill maintainer, not by Phase 13):

**Shape 21: process-environment mutation inside a multi-threaded test binary.** A `#[test]` calls `std::env::set_var`/`remove_var` (edition 2024: `unsafe fn`) while libtest runs sibling tests on other threads, some of which reach a native `getenv` (libgit2, sqlite, glibc resolver) that std's `ENV_LOCK` cannot cover. On glibc the pair is a C11 data race on `__environ`; on Apple Libc it is a contract violation only.

| Candidate | Pros | Cons |
|---|---|---|
| A. Inject the value the code under test would have read (constructor parameter, path parameter, `Command::env` on a child) and forbid `unsafe_code` workspace-wide | Removes the writer; regression becomes a compile error; no scheduling argument needed | Needs a small API addition per site |
| B. `#[serial]` on the writer and on every reader test | Small diff | EXP-014: `serial_test` excludes only other `#[serial]` tests; readers added later are unannotated; the `unsafe` and the false SAFETY story stay |
| C. Move the writer into its own one-test binary (`tests/<name>.rs`) or `harness = false` | Sound by construction while the binary has one test (UF-013 today) | The one-test invariant is unguarded; private fields become inaccessible from `tests/*.rs`; `unsafe` stays |
| D. Runner-level `--test-threads=1` or a nextest group | One config line | Only under the runner that honours it; `cargo test` and llvm-cov ignore it; hides, does not remove |

**Default pick:** A. Principle 1 ("fix at the chokepoint, don't delete it") is honoured by keeping the production env read (`OpenRouterClient::new` still consults `OPENROUTER_BASE_URL`; `get_history_path()` still uses `dirs::config_dir()`) and moving only the tests onto the injected path.

## 3. UF-001: terraphim_grep lib tests mutate `OPENROUTER_BASE_URL` while libgit2 `getenv` runs on sibling threads

### 3.1 Evidence and current sites

- Writer: `crates/terraphim_grep/src/openrouter_client.rs:165-193` (`test_openrouter_client_base_url_resolution`): `remove_var` at :170-172, `set_var` at :177-179, restore `set_var` at :185-187 and `remove_var` at :189-191, all under the single-line SAFETY comment at :166 ("test code, single-threaded, no concurrent env access"), shown false by EXP-001b (F-102) and F-101.
- Production reader that the test exercises: `openrouter_client.rs:51-52` `std::env::var("OPENROUTER_BASE_URL").unwrap_or_else(|| "https://openrouter.ai/api/v1")` inside `OpenRouterClient::new` (:34-55). Field `base_url: String` at :29 is private; `chat_completion` uses it at :88.
- Native readers on sibling threads: `hybrid_searcher.rs:342` `search_code` -> fff-search -> `git2::init` (libgit2 sysdir `getenv`), reached by lib tests at `lib.rs:607,640,673,741,792,840` and `hybrid_searcher.rs:484,526` (Phase 1 F). Std-locked readers (logical race UF-024): tests at :159-161, :197, :206.
- Verdict: CONFIRMED_UB, EXP-001b (bigbox, glibc 2.39): 1 unassisted `call` overlap in 100 iterations at widen 0, 1007 overlaps widened, all the predicted libgit2 chain. Gitea: #280 (open, no labels, no assignee).

### 3.2 Candidates

**A. Constructor injection plus lint.** Split the env read out of the constructor:

```rust
// openrouter_client.rs (sketch; Phase 13 writes the real code)
const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";
/// Pure: the value `OPENROUTER_BASE_URL` would select, given its observed content.
fn resolve_base_url(override_: Option<String>) -> String {
    override_.filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}
impl OpenRouterClient {
    pub fn new(api_key: &str, model: &str) -> anyhow::Result<Self> {
        Self::with_base_url(api_key, model, resolve_base_url(std::env::var("OPENROUTER_BASE_URL").ok()))
    }
    pub fn with_base_url(api_key: &str, model: &str, base_url: impl Into<String>) -> anyhow::Result<Self> { /* the body of today's :35-54 with base_url: base_url.into() */ }
}
```

The test becomes three env-free assertions: `resolve_base_url(None)` is the default, `resolve_base_url(Some("https://example.com/v1"))` passes through, and `with_base_url("key","model","https://example.com/v1")?.base_url` equals the argument. Whether an empty variable should fall back to the default (today it does not: `unwrap_or_else` only fires on `Err`) is a behaviour question; the plan keeps today's semantics (`Option<String>` maps `Some` verbatim) and notes the `filter` line above as an optional tightening for the reviewer to decide. UF-024 (siblings observing a mid-mutation value; restore skipped on panic) disappears with the writer. The lint (section 7) makes any reintroduction of `unsafe { set_var }` a compile error in every target of the crate.

**B. `#[serial]` on the writer and on every test that can reach a native `getenv`.** Eleven attributes (the writer, the eight fff-search tests, the two `OpenRouterClient::new` siblings for the macOS SystemConfiguration reader UF-012) and a new `serial_test` dev-dependency for terraphim_grep (it has none today, `crates/terraphim_grep/Cargo.toml:55-59`). EXP-014 proved partial annotation is worthless; full annotation works only until the next test that calls `HybridSearcher::search` is written without the attribute. The `unsafe` blocks and the SAFETY comment stay and would need a rewritten multi-line contract (Principle 7).

**C. Isolate the writer in a one-test integration binary.** Move the test to `crates/terraphim_grep/tests/openrouter_base_url_env.rs`; it needs a public accessor for `base_url` (private field, :29) so the API changes anyway; keep the `unsafe` blocks with an honest SAFETY comment ("sole test in this binary; the only other thread is libtest's parked main thread"). Sound today (the UF-013 argument), unguarded tomorrow: a second `#[test]` in that file silently recreates this finding. D (`--test-threads=1`) is a variant of C that only the nextest lanes would honour and is scored inside C's row.

### 3.3 Rubric (0-4 per axis; higher is better)

| Candidate | Correctness margin | Perf delta | Diff blast radius | Reviewability | Maintainability | Total |
|---|---|---|---|---|---|---|
| A injection + forbid | 4 (writer gone; compile-time gate) | 4 (test-only; one extra fn call in `new`) | 3 (one file, about 40 lines changed, plus manifest lines shared with the headline lever) | 4 (pure function, obvious assertions) | 4 (lint-enforced; new readers cannot regress it) | 19 |
| B `#[serial]` x11 | 2 (hides by scheduling; `unsafe` stays; depends on `Once` timing of libgit2) | 2 (11 tests serialised, including the 8 slowest I/O tests) | 3 (11 attributes, 1 dev-dep) | 3 | 1 (next unannotated reader reintroduces it) | 11 |
| C one-test binary | 3 (sound while one test) | 4 | 3 (new file, accessor, delete from lib tests) | 3 | 2 (invariant unguarded; needs a guard test) | 15 |

### 3.4 Winner

A, decided by maintainability: A is the only candidate under which reintroducing the bug fails `cargo check` rather than a probabilistic probe. Runners-up recorded above (Principle 4). C is the fallback if Phase 13 finds that the public `with_base_url` constructor is unwanted on the crate API: in that case the accessor plus one-test binary must land together with a `ci_guards` test counting `#[test]` occurrences in that file.

### 3.5 Cross-references

- Original UB proved by EXP-001b (CONFIRMED_UB; F-102). EXP-001 (TSan) stays NEEDS_REFINEMENT: the instrument is blind, and remains so after the fix.
- Remediation proved by EXP-801 (post-fix libtest binary under the shim: `setenv=0 unsetenv=0`, zero overlaps, 100 iterations at widen 0 and 2000 us, pre-fix binary as the control that reproduces EXP-001b) and EXP-804 (fault-injection patch fails compilation under the lint).

### 3.6 Gitea

- Main: #280 (exists). Claim: `gtr edit-issue --owner terraphim --repo terraphim-clients --index 280 --add-labels "status/in-progress" --add-assignees "<agent>"`.
- Test dependency (new): "test(grep): EXP-801 LD_PRELOAD probe loop as the #280 regression instrument (fails on 1120180, passes after the injection commit)". Carries the probe sources (section 11) in the runbook and records the bigbox run log as `phase5_experiment_results/EXP-801.log`.
- Docs dependency (new): "docs(grep): remove the false SAFETY comment at openrouter_client.rs:166; document `with_base_url`; BUILD.md note that `cargo test` runs libtest in-process" (section 13).
- Edges, using the CLAUDE.md reading verbatim ("issue X is blocked by issue Y (Y blocks X): `gtr add-dep --issue X --blocks Y`"): `gtr add-dep --owner terraphim --repo terraphim-clients --issue 280 --blocks <test-issue>` and `--issue 280 --blocks <docs-issue>`. Verify the direction with `gtr graph` after the first edge; an inverted edge is the likeliest execution error.
- #280 also blocks the headline lever issue (section 7): the lint cannot land while this `unsafe` exists.

### 3.7 Files, line ranges, commits

| File | Lines (current) | Change |
|---|---|---|
| crates/terraphim_grep/src/openrouter_client.rs | :29 (field), :34-55 (`new`), :51-52 (env read) | add `resolve_base_url`, `with_base_url`; `new` delegates |
| same | :164-193 | rewrite the test env-free; delete the SAFETY line at :166 and the four `unsafe` blocks |
| same | :157-162, :195-208 | unchanged (they no longer observe a mutating variable) |

Commit 1 of the sequence (section 10): `fix(grep): inject the OpenRouter base URL; no env mutation in lib tests (Refs #280)`.

## 4. UF-002: `EnvVarGuard` in tui_service_tests mutates HOME, XDG_CONFIG_HOME, TERRAPHIM_DATA_PATH while sqlite `getenv` may run on a sibling thread

### 4.1 Evidence and current sites

- Writer: `crates/terraphim_agent/tests/tui_service_tests.rs:9` (`use terraphim_test_utils::EnvVarGuard`), :33-61 (`test_tui_service_new_uses_host_settings_path`, the only `#[serial]` test of 10), guards at :37, :40, :43; three `Drop`s at :61 -> registry `terraphim_test_utils-1.20.3/src/lib.rs:39,61,106-113` (safe `pub fn` over `unsafe { set_var }`, UF-003).
- Code under test: `crates/terraphim_agent/src/service.rs:31` `TuiService::new(config_path, no_project_config)` -> `load_config` :49 -> `DeviceSettings::load_from_env_and_file(None)` at :78, which resolves `default_config_path()` from `ProjectDirs` (HOME) in registry `terraphim_settings-1.20.2/src/lib.rs:197-214` and bootstraps `settings.toml` (`init_config_file` :272-284). The test asserts `DeviceSettings::default_config_path().join("settings.toml")` exists (:49-55).
- Reader: `service.rs:431,462-464` -> terraphim_service -> terraphim_persistence -> rusqlite -> bundled sqlite `unixTempFileInit` (`getenv("SQLITE_TMPDIR")`, `getenv("TMPDIR")`), once per process on the first `sqlite3_initialize`.
- Verdicts: EXP-002c CONFIRMED_UB for the mechanism (102 unassisted overlaps in 150 runs); EXP-002b NO_EVIDENCE in the shipped binary only because the serial test is the first sqlite opener so the once-per-process reader executes on the writer's own thread; EXP-014 showed `#[serial]` cannot exclude the nine unannotated siblings. Root cause of the false safety story is upstream (UF-003, terraphim-ai repository).
- Binary exposure: nextest lanes isolate it; `cargo test` (BUILD.md) does not; the coverage lanes do not run it (`--lib`).

### 4.2 Candidates

**A. Settings-dir injection plus memory-only storage, drop `EnvVarGuard`, plus lint.** Add to `service.rs`:

```rust
// service.rs (sketch)
pub async fn new(config_path: Option<String>, no_project_config: bool) -> Result<Self> {
    Self::new_with_settings_dir(config_path, no_project_config, None).await
}
/// `settings_dir` overrides `DeviceSettings::default_config_path()`; tests pass a TempDir so the
/// process environment is never mutated.
pub async fn new_with_settings_dir(config_path: Option<String>, no_project_config: bool,
                                   settings_dir: Option<PathBuf>) -> Result<Self> {
    let config = Self::load_config_in(config_path, no_project_config, settings_dir).await?;
    Self::from_config(config).await
}
```

`load_config` (:49) keeps its signature and delegates to `load_config_in`, which passes `settings_dir` to `DeviceSettings::load_from_env_and_file(settings_dir)` at :78 (the registry function already takes `Option<PathBuf>`, `terraphim_settings-1.20.2/src/lib.rs:221`). `load_config_with_role_config` (:98-103) already receives `&device_settings`, so nothing else threads through. The test becomes:

```rust
#[tokio::test]
async fn test_tui_service_new_uses_host_settings_path() -> Result<()> {
    let _ = terraphim_persistence::DeviceStorage::init_memory_only().await; // keeps sqlite out of ~/.terraphim
    let temp_home = tempfile::tempdir()?;
    let settings_dir = temp_home.path().join(".config").join("terraphim");
    let service = TuiService::new_with_settings_dir(None, false, Some(settings_dir.clone())).await?;
    assert!(settings_dir.join("settings.toml").exists(), "bootstraps host settings at {settings_dir:?}");
    assert!(!service.get_config().await.roles.is_empty());
    Ok(())
}
```

`init_memory_only` is `get_or_try_init` on the process-wide `DEVICE_STORAGE` (registry `terraphim_persistence-1.20.2/src/lib.rs:55,86-97`; the `memory` feature is in that crate's default set, `Cargo.toml:46-50`, and terraphim_agent already depends on it, `crates/terraphim_agent/Cargo.toml:78`). If a sibling initialises sqlite first, this test silently gets the real store; that is UF-023 hygiene, already true today for the nine siblings, and it does not affect UB removal, which depends only on the writer being gone. Phase 13 may add the same one-liner to the other nine tests (each is a separate process under nextest, so each must call it) or leave UF-023 as recorded. `#[serial]` and the `serial_test` import (:6) are dropped from this file; the dev-dependency stays for the 108 other uses. `terraphim_test_utils` leaves `crates/terraphim_agent/Cargo.toml:112` in the same commit as the UF-014 sweep (section 9).

**B. `#[serial]` on all 10 tests.** Nine attribute lines. Works only while every future test in the file is also annotated, and only because `serial_test` is process-local to this binary; the `unsafe` stays in the dependency behind a wrapper whose own rule was shown insufficient (EXP-014). It does nothing about sqlite's `char*` retention hazard noted under F-103.

**C. Move the writer test into its own one-test binary** (`tests/tui_service_host_settings.rs`), keep `EnvVarGuard`, add the binary to the nextest `serial-external-state` override list (`.config/nextest.toml:29-31`) for the external-state reasons that already apply to `tui_service_tests`. Sound by construction today; unguarded; the dependency on an unsound wrapper remains.

### 4.3 Rubric

| Candidate | Correctness margin | Perf delta | Diff blast radius | Reviewability | Maintainability | Total |
|---|---|---|---|---|---|---|
| A injection + memory storage + forbid | 4 (writer gone; independent of reader ordering and of the upstream fix) | 4 | 2 (service.rs: one new constructor, one param threaded through `load_config`; test file about 25 lines; two manifests) | 4 | 4 | 18 |
| B `#[serial]` x10 | 2 | 2 (ten tests serialised) | 4 | 3 | 1 | 12 |
| C one-test binary | 3 | 4 | 3 | 3 | 2 | 15 |

### 4.4 Winner

A, decided by correctness margin: it is the only candidate whose soundness does not rest on which test opens sqlite first (EXP-002b's caveat) or on the upstream `terraphim_test_utils` fix landing. Runners-up recorded above. C is the fallback if adding a public constructor to `TuiService` is refused; it must then ship with a `ci_guards` test that the new file contains exactly one `#[tokio::test]`.

### 4.5 Cross-references

- Original proved by EXP-002c (mechanism) with EXP-002b explaining the shipped binary's ordering luck and EXP-014 explaining why `#[serial]` is not a fix.
- Remediation proved by EXP-802 (post-fix `tui_service_tests` binary executed directly under the shim on bigbox: main-process `setenv=0 unsetenv=0`; the child processes it spawns are ignored as in EXP-002b; the test still asserts `settings.toml` under the injected dir) and EXP-804.

### 4.6 Gitea

- Main (new, child of #252): "UB (mechanism confirmed): tui_service_tests EnvVarGuard mutates HOME/XDG_CONFIG_HOME/TERRAPHIM_DATA_PATH on a multi-threaded binary; inject the settings dir instead" with the EXP-002b/002c/014 evidence in the body.
- Test dependency (new): "test(agent): EXP-802 probe loop on tui_service_tests as the regression instrument" (blocks the main issue).
- Docs dependency (new): "docs(agent): TuiService::new_with_settings_dir; remove the EnvVarGuard usage note; BUILD.md" (blocks the main issue).
- Upstream (new, repository terraphim/terraphim-ai): "terraphim_test_utils: `set_env_var`/`remove_env_var`/`EnvVarGuard` are safe wrappers over `unsafe set_var` whose `# Safety` rule (`#[serial]`) cannot be satisfied in a mixed binary (EXP-014); make them `unsafe fn` with a multi-line contract, or delete the crate". Cross-repo reference `terraphim/terraphim-ai#<n>` in the main issue. The local fix is independent of it (the workspace stops depending on the crate; section 9).
- Edges: `gtr add-dep --owner terraphim --repo terraphim-clients --issue <main> --blocks <test-issue>`, `--issue <main> --blocks <docs-issue>`; `--issue <headline-lever> --blocks <main>`.

### 4.7 Files, line ranges, commits

| File | Lines (current) | Change |
|---|---|---|
| crates/terraphim_agent/src/service.rs | :31-34 (`new`), :49-52 (`load_config` signature), :78 (`load_from_env_and_file(None)`) | add `new_with_settings_dir`, `load_config_in`; thread `settings_dir` |
| crates/terraphim_agent/tests/tui_service_tests.rs | :6 (`use serial_test::serial`), :9 (`use terraphim_test_utils::EnvVarGuard`), :33-61 | rewrite the test as above; drop both imports |
| crates/terraphim_agent/Cargo.toml | :112 (`terraphim_test_utils` dev-dep) | remove, together with section 9's mcp_server sweep |

Commit 2: `fix(agent): TuiService::new_with_settings_dir; tui_service_tests stop mutating the environment (Refs #<main>)`.

## 5. UF-004: terraphim_update `setup_temp_config_dir` sets five variables under a false SAFETY comment (lighter section)

Sites: `crates/terraphim_update/src/state.rs:126-140` (helper), SAFETY line :130, `unsafe` block :131-137 (`HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`, `APPDATA`, `LOCALAPPDATA`), never restored; ten `#[serial]` callers at :144, :163, :176, :199, :227, :248, :266, :277, :289, :311 (module ends :327). Production functions read `dirs::config_dir()` only through `get_history_path()` (:15-22); `save_update_history` :42-53, `load_update_history` :71-88, `delete_update_history` :94-105, `history_exists` :111-116 all call it. The binary has 130 tests (120 unannotated); it runs in-process under both coverage lanes and under `cargo test`. Severity raised to LIKELY-UB by the lead: the glibc resolver reader (reached by `test_check_for_updates_auto`, `lib.rs:1712-1713`, via `getaddrinfo`) is documented by std, EXP-010 showed the writer/`getaddrinfo` window in 6 of 100 widened iterations, and the shim cannot see resolver reads (EXP-010 DEFERRED pending helgrind, EXP-010-a).

Candidates and scores:

| Candidate | Design | Correctness | Perf | Blast | Review | Maintain | Total |
|---|---|---|---|---|---|---|---|
| A path injection | `pub fn history_path_in(config_dir: &Path) -> PathBuf` (pure: `config_dir.join("terraphim").join("update_history.json")`), `save_update_history_to(&UpdateHistory, &Path)`, `load_update_history_from(&Path)`, `delete_update_history_at(&Path)`, `history_exists_at(&Path)`; the four no-arg functions delegate through `get_history_path()` unchanged. Tests build a `TempDir` and call the `_at`/`_to`/`_from` variants; delete `setup_temp_config_dir` and the SAFETY comment; drop the ten `#[serial]`; `test_get_history_path` (:311) asserts the pure suffix via `history_path_in` and no longer touches the real config dir | 4 | 4 | 3 (one file, about 60 lines) | 4 | 4 | 19 |
| B `#[serial]` on all 130 tests | annotate every test in the lib binary | 1 (does not cover the self_update writer; hides) | 1 | 1 | 3 | 1 | 7 |
| C own binary | move `state` tests to `tests/update_history_state.rs` with `harness = false` or one test each; keep `unsafe`; HOME still dangles for the process | 3 | 4 | 3 | 3 | 2 | 15 |

Winner A (correctness margin). Runners-up recorded. Also fixed by A: the dangling `HOME` after `TempDir` drop and the macOS `dirs::config_dir()` divergence noted in Phase 1 (both were artefacts of mutating the real environment).

Cross-references: original evidenced by EXP-010 (window shown; reader unobservable; helgrind follow-up EXP-010-a stays in the runbook, unchanged by this plan); remediation proved by EXP-803 (post-fix `terraphim_update` lib binary under the shim: the 50 `setenv` calls per iteration from the ten state tests drop to 0; any remaining `setenv` is the self_update writer of section 6 and is counted separately) and EXP-804.

Gitea: main (new, child of #252): "LIKELY-UB: terraphim_update state.rs tests set HOME et al. on a 130-test binary under a false SAFETY comment; inject the history path"; test dependency: "test(update): EXP-803 probe loop"; docs dependency: "docs(update): remove the SAFETY comment at state.rs:130; document the `_at` variants". Edges as in 3.6.

Files: `crates/terraphim_update/src/state.rs` :15-22 (add `history_path_in`, keep `get_history_path`), :42-53, :71-88, :94-105, :111-116 (add path-taking variants; delegate), :119-140 (test module header: drop `use serial_test::serial`, delete the helper), :143-326 (ten tests rewritten onto a `TempDir`). Commit 3: `fix(update): inject the update-history path in state tests; no env mutation (Refs #<main>)`.

## 6. UF-036: self_update `set_ssl_vars!()` is a second env writer in the terraphim_update lib test binary (lighter section, with an escalation)

Phase 6 round 1 filed EXP-102 (DEFERRED: reachability chain confirmed statically, one non-serial caller; dynamic arm waits on the EXP-010-a instrument) after this section was drafted; its facts agree with the ones below and its remediation note (`#[serial]` on the test, or an env-flag gate as the test already performs live HTTPS) is answered here: `#[serial]` alone is not a fix (EXP-014), and `#[ignore]` is preferred over an env-flag gate because an ignored test is visible in every run's count whereas a flag-gated one passes silently. Facts re-checked today: `self_update-0.42.0/src/macros.rs:12-24` defines `set_ssl_vars!` as `if var_os("SSL_CERT_FILE").is_none() { set_var(...) }` (and `SSL_CERT_DIR`), `#[cfg(target_os = "linux")]` only, in an edition-2021 crate (so no `unsafe` block is needed there). It is invoked at `src/backends/github.rs:157, 518, 548, 587` and `src/lib.rs:692`. `github.rs:518` is inside `get_latest_release`, which `check_for_updates_auto` reaches (`crates/terraphim_update/src/lib.rs:1287-1300`) from `spawn_blocking` (:1283). The non-serial `test_check_for_updates_auto` (:1712-1713) performs live HTTPS to api.github.com and is the only caller in the lib test binary. EXP-010 counted the two extra `setenv` calls per iteration.

Escalation for the lead (new hypothesis, not resolved here): the same writer executes in production binaries on Linux whenever the GitHub backend is used (the fallback path of `check_for_updates` at :419-440 and the auto path at :1287), inside `spawn_blocking` while other runtimes and threads are live (UF-033: two long-lived runtimes on the main thread; the TUI loop; any sqlite or libgit2 reader). That is outside the "test code only" archetype of this run. Whether rustls-native-certs consults `SSL_CERT_FILE`/`SSL_CERT_DIR` (which would make the variables live, not dead, under the workspace's rustls-only policy, `deny.toml:79-81`) is for the upstream issue to settle. Recommended for the Delta section: allocate an EXP in the next convergence round (probe on the shipped `terraphim-agent` binary with `TERRAPHIM_UPDATE_BACKEND` forced to GitHub) and decide between an upstream PR (gate the macro behind an `openssl` feature, or drop it) and a `[patch.crates-io]` fork.

Local candidates:

| Candidate | Correctness | Perf | Blast | Review | Maintain | Total |
|---|---|---|---|---|---|---|
| A `#[ignore = "live network; UF-036"]` on `test_check_for_updates_auto` (lib.rs:1712); the test is already described as flaky and network-bound | 3 (removes the writer from the default lib run and both coverage lanes; production untouched) | 4 | 4 (one attribute) | 4 | 3 | 18 |
| B export `SSL_CERT_FILE`/`SSL_CERT_DIR` in both workflows so the macro's `is_none()` guard skips the write | 2 (hides; not applied to `cargo test` on a developer host) | 4 | 3 (two workflow files) | 3 | 2 | 14 |
| C `[patch.crates-io]` fork of self_update without the macro, or upstream PR | 4 (upstream) / 4 (fork) | 4 | 2 (fork maintenance) | 3 | 3 (upstream) / 1 (fork) | 14 (fork; corrected by Phase 10 from 13 — 4+4+2+3+1=14. The winner does not change: A scores 18 and wins on blast radius) |

Winner A now, plus the upstream issue (C, upstream form) as a tracked dependency of the headline lever's "dependency writers" note. Note that the obvious in-tree mitigation, pre-setting the two variables on the main thread before any runtime exists, is itself `set_var` and is blocked by the headline lint; that is by design (Principle 1 applied to the lint: the lint is the chokepoint, do not carve exceptions).

Gitea: main (new, child of #252): "self_update set_ssl_vars!() writes SSL_CERT_FILE/SSL_CERT_DIR from the GitHub backend on Linux: ignore the live-network test; upstream the macro gating; assess the production path"; upstream issue on github.com/jaemk/self_update (title: "set_ssl_vars! calls std::env::set_var from library code; unsound with concurrent getenv on glibc"). EXP-803's count arm is the test dependency (expects `setenv=0` in the default run once A lands). Files: `crates/terraphim_update/src/lib.rs:1711-1713`. Commit 9: `test(update): ignore the live-network GitHub check; it is the only caller of self_update's env writer (Refs #<main>)`.

## 7. Headline lever: workspace-wide `forbid(unsafe_code)`

Design decision, attribute versus manifest: `#![forbid(unsafe_code)]` in `lib.rs`/`main.rs` (15 files) does not cover `tests/*.rs`, `benches/`, `examples/` or `build.rs`, each of which is its own crate root; `crates/terraphim_mcp_server/tests/test_mcp_stdio.rs` is exactly such a target. Therefore:

- Root `Cargo.toml`: add after the `[workspace.package]` table (:16-24)

  ```toml
  [workspace.lints.rust]
  unsafe_code = "forbid"
  ```

- Each of the 11 member manifests (`[package]` at line 1 in every one; `[dependencies]` at agent :40, cli :19, command_runtime :13, grep :15, hooks :14, lsp :23, mcp_server :17, negative_contribution :14, sessions :52, update :14, session-analyzer :34): insert `[lints]\nworkspace = true` immediately before `[dependencies]`. No crate has a `[lints]` table today. Toolchain 1.97.1 (`rust-toolchain.toml`) supports the table (stable since 1.74). `forbid` cannot be relaxed by an in-source `#[allow]`, which is the point.
- Macro-generated `unsafe` does not trip the lint: built-in derives and std macros (`vec!`) are exempt via their spans, which is why the 89 `vec!` and 18 `TrivialClone` rows in Phase 1 are not a problem; proc-macro output is not exempt, so the mcp_server lib expansion was checked today (0 `unsafe` in 10,636 lines) and Phase 13's pre-flight is `cargo clippy --workspace --all-targets -- -D warnings` after the lint lands, which fails loudly if any proc macro (rmcp, tokio, serial_test, clap, serde, async-trait, proptest, insta) emits `unsafe` in a target Phase 1 did not expand.
- `ci_guards` (crates/terraphim_agent/tests/ci_guards.rs, already run by native-ci at `.gitea/workflows/native-ci.yml:84`): add `every_member_inherits_workspace_lints`, parsing `Cargo.toml` `[workspace] members` and asserting each member manifest contains `[lints]` with `workspace = true` (the `toml` dev-dependency exists, `crates/terraphim_agent/Cargo.toml:115`). A new crate cannot silently opt out. Note that ci.yml (:64) does not run `ci_guards`; adding `--test ci_guards` there is part of the docs/CI commit.
- Dependency writers are a separate class the lint cannot see: safe wrappers in `terraphim_test_utils` (removable, section 9) and `self_update` (not removable, section 6). Add `terraphim_test_utils` to `deny.toml` `[bans].deny` (:79-81) with reason "safe wrapper over std::env::set_var; workspace forbids env mutation in tests, inject values instead" so the existing `cargo deny check` gate (`docs/ci-gap-analysis.md:21`) enforces absence.
- Sequencing: this commit must come after commits 1-6 (the six `unsafe` blocks and the two wrapper consumers), otherwise it does not compile. It is its own Gitea issue (child of #252): "chore(workspace): forbid unsafe_code in every crate and target; ci_guards manifest test; deny terraphim_test_utils", blocked by #280 and the UF-002/UF-004 main issues.

## 8. Triangulation: forbid(unsafe_code) versus isolation

**For the lint (chosen).** The audit found exactly one class of `unsafe` in 11 crates, all of it avoidable by injecting a value. Once removed, the lint turns the whole class into a compile error across every target, including the integration-test roots where isolation arguments live. Isolation (candidate C everywhere) keeps four `unsafe` sites alive under "sole test in this binary" contracts that nothing checks; UF-013 is already that shape and survives only because `RUN_MCP_STDIO_TEST` is never set in CI. The lint also documents policy in one place a reviewer can point at, and `deny.toml` extends it to the removable wrapper.

**For isolation (dissent, preserved).** The lint is blind to the dependency writers, which are the writers that now matter most: `self_update` executes `set_var` on production paths on Linux (section 6), and any future dependency can do the same. A team that reads `forbid(unsafe_code)` as "we have no env races" is wrong, and a runner-level isolation policy (one process per test, which nextest already gives both `test` lanes) protects against dependency writers too, at no source cost. Isolation is also the only lever for the coverage lanes without touching source: `cargo llvm-cov nextest` would close the in-process exposure of section 1 for every writer, in-tree or not.

**Resolution.** Both, in their own lanes. The lint plus injection is the remediation of the four findings (it removes the writers and proves it at compile time); switching the two coverage lanes to `cargo llvm-cov nextest --workspace --lib --profile ci` is recorded as a CI hardening item for #254's owner (not a remediation of any finding, so no rubric), because it is the only thing that also contains dependency writers. The dissent's strongest point, that the lint does not make the binaries env-race-free, is written into the docs commit (section 13) so nobody reads the lint as more than it is. The isolation-only path lost on maintainability: it never removes the false SAFETY comments, and every one-test invariant it relies on needs its own guard.

## 9. Ancillary shape-sweep (Principle 2: same shape, same change set)

| Site | Finding | Change | Commit |
|---|---|---|---|
| crates/terraphim_mcp_server/tests/test_mcp_stdio.rs:14-20 (`unsafe { env::set_var("TERRAPHIM_SETTINGS_PATH", ...) }`), child spawned at :25-32 | UF-013 (DEFERRED, sound today as a one-test binary) | the variable is for the child: `command.env("TERRAPHIM_SETTINGS_PATH", ...)` on the `Command` at :25; delete the block and the comment at :14. Flag, do not fix: the value `../terraphim_settings/default/settings_local_dev.toml` points at a crate that is not in this workspace, so the gated test cannot pass if ever enabled | 4 |
| crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs:11 (import), :22, :28 (`set_env_var`), child at :124-135; tests/mcp_rolegraph_validation_test.rs:7, :21, :27, :137, :284, :394, :618 (`set_env_var`), children at :159-164, :294-299, :412-417, :636-641 | UF-014 (DEFERRED, defensible) | in-process consumer is already `DeviceStorage::init_memory_only()` (e2e :30, rolegraph :28); the variables are for the spawned servers: pass `TERRAPHIM_PROFILE_MEMORY_TYPE=memory` and `TERRAPHIM_LOG_DIR` via `cmd.env(..)` at each spawn; drop the import and the dev-dep (`crates/terraphim_mcp_server/Cargo.toml:66`); the eight `#[serial]` stay for the fixed-name temp files (:66, :76) | 5 |
| crates/terraphim_agent/build.rs (whole file) and Cargo.toml :121-122 (`[build-dependencies] rustc_version`) | Phase 1 A non-site: dead cfg `rust_has_unsafe_env_setters`, zero consumers | delete both | 6 |
| UF-024 (sibling openrouter tests observe a mid-mutation value) | logical race | closed by commit 1 without further change | 1 |
| UF-003 (upstream wrapper) | SUSPICIOUS | terraphim-ai issue (section 4.6); local decoupling by commits 2 and 5 | n/a |

## 10. Commit sequence (one lever per commit; branch `task/252-env-unsafe-remediation` off `main`; each commit builds and passes `cargo clippy --workspace --all-targets -- -D warnings` and the affected binary's tests)

1. `fix(grep): inject the OpenRouter base URL; no env mutation in lib tests (Refs #280)` — openrouter_client.rs only.
2. `fix(agent): TuiService::new_with_settings_dir; tui_service_tests stop mutating the environment (Refs #<UF-002 main>)` — service.rs, tui_service_tests.rs.
3. `fix(update): inject the update-history path in state tests; no env mutation (Refs #<UF-004 main>)` — state.rs.
4. `test(mcp_server): pass TERRAPHIM_SETTINGS_PATH to the child with Command::env (Refs #252)` — test_mcp_stdio.rs.
5. `test(mcp_server): pass profile and log dir to spawned servers with Command::env; drop terraphim_test_utils (Refs #<UF-002 main>)` — two test files, two manifests (mcp_server :66, agent :112), Cargo.lock.
6. `chore(agent): remove the dead rust_has_unsafe_env_setters build script (Refs #252)` — build.rs, Cargo.toml :121-122, Cargo.lock.
7. `chore(workspace): forbid unsafe_code in every crate and target; ci_guards manifest test; deny terraphim_test_utils (Refs #<lever>)` — root Cargo.toml, 11 member manifests, ci_guards.rs, deny.toml.
8. `docs: SAFETY comments removed; BUILD.md and ci-gap-analysis record the unsafe_code policy and the libtest exposure (Refs #<docs issues>)` — BUILD.md, docs/ci-gap-analysis.md, .github/workflows/ci.yml (:64 add `--test ci_guards` only if the GitHub lane can resolve the terraphim registry, since `no_duplicate_terraphim_crates` shells out to `cargo tree --all-features`; native-ci already runs the binary at native-ci.yml:84).
9. `test(update): ignore the live-network GitHub check; it is the only caller of self_update's env writer (Refs #<UF-036 main>)` — lib.rs :1711.
10. `test(runbook): EXP-801..804 probe sources and fault-injection patch under .ub-exorcism/.../runbook (Refs #252)` — audit workspace only, no `crates/` change.

Pull request: `gtr create-pull --owner terraphim --repo terraphim-clients --title "Fix #280: remove env mutation from tests; forbid unsafe_code workspace-wide" --base main --head task/252-env-unsafe-remediation`. Forward-only release afterwards (Principle 5, 11): bump `workspace.package.version` (`Cargo.toml:17`, 1.21.14) in its own commit when the release train next runs; not part of this PR.

## 11. Regression instrument and runbook contents (Principle 8: prove detection, not just presence)

Instrument: the LD_PRELOAD shim `phase3_raw/envprobe/linux/envprobe_linux.c` (87 lines) run against the libtest binary directly on bigbox (glibc, the `terraphim-native` runner host). Never via nextest (section 1). The runbook (`UB_RUNBOOK.md`, Phase 12) carries, verbatim from `phase3_raw/envprobe/linux/` and `experiments/EXP-010/`:

- `envprobe_linux.c`, `symbolise2.py` (unchanged);
- `run_probe.sh` with lines 4-5 parameterised (`SO=${ENVPROBE_SO:-...}`, `L=${ENVPROBE_LOGDIR:-...}`) instead of the hard-coded `/tmp/ubx-probe-scratch-src/libenvprobe.so` and `/tmp/ubx-probe-logs`;
- `scratch_main.rs` + `Cargo.toml` (EXP-002c control: proves the shim still sees the sqlite reader on the same host, so a clean EXP-802 is "no writer", not "blind instrument");
- `gaiprobe.c`, `resolver_selftest.c`, `run.sh`, `run_gai.sh` (EXP-010-a, unchanged, pending helgrind);
- `exp804_reintroduce_set_var.patch`: a stored patch that re-adds one `unsafe { std::env::set_var("UBX_FAULT", "1") }` to a `#[test]` in openrouter_client.rs, applied only inside EXP-804 and reverted with `git apply -R`; no cfg-gated `unsafe` is left in source.

Pass criteria, per binary (the CI test lane is red on #264 and #278, so lane colour is not the signal):

| Binary | Before (control) | After |
|---|---|---|
| terraphim_grep lib (pre-fix control: ae06cd2, whose crates/ equal 1120180) | EXP-001b: `setenv=1 unsetenv=2` per run, >= 1 `call` overlap per 100 at widen 0, hundreds widened | `setenv=0 unsetenv=0`, 0 overlaps, 100 iterations at widen 0 and 2000 us, 57 tests ok |
| terraphim_agent tui_service_tests | EXP-002b: main process `setenv=4 unsetenv=2` | main process `setenv=0 unsetenv=0`, 10 tests ok, `settings.toml` under the injected dir |
| terraphim_update lib | EXP-010: `setenv=52` per iteration | `setenv=0` (with commit 9) or `setenv=2` (the self_update pair, if commit 9 is deferred) |
| workspace | n/a | EXP-804: fault patch -> `cargo check --workspace --all-targets` fails with `error: usage of an \`unsafe\` block` (lint `unsafe_code`); without the patch, `rg -n '\bunsafe\b' crates/ -t rust` matches only prose lines. **Phase 10 correction (2026-09-12, re-run at 46bbc43):** the expected set is `wiki_sync.rs:804`, `memory_command.rs:1032`, `modes/local.rs:89`, `modes/local.rs:294` — four lines, not six. `wiki_sync.rs:809` and `command_system_integration_tests.rs:100` do **not** match: the first is `sync_learning_rejects_unsafe_page_name`, where `unsafe` sits between underscores and so fails the `\b` anchors, and the second has no `unsafe` token at all. A fifth line, `crates/terraphim_agent/build.rs:12`, matches today and disappears with commit 6, which deletes that file; if commit 6 is reordered or dropped, add it back to the expected set. As written the criterion would fail on a correct tree |

## 12. Blockers and operator actions

| Blocker | Effect on this plan | Operator action (not for an agent) |
|---|---|---|
| #281: 62 orphaned `terraphim_server` processes on bigbox | Scheduling noise inflates or hides widen=0 hit rates in EXP-801/802/803 controls; the pre-fix control may need more than 100 iterations to reproduce EXP-001b's single unassisted overlap | Kill the orphans (`pgrep -af terraphim_server` then a deliberate `pkill -f /tmp/terraphim_server_install/bin/terraphim_server`) before the probe session; record `uptime` load in each EXP log header; land #281's `kill_on_drop` fix separately |
| CI test lane red on #264 (session-analyzer codex parser under feature unification) and #278 (`--original` flag) | "fails before, passes after" cannot be read from lane colour; section 11 defines it per binary | Fix or `#[ignore]` the two tests on their own issues; until then Phase 13 reports per-binary results |
| Rogue runner `terraphim-native-c610efaf` | Any probe or CI result from that runner is inadmissible for this plan | Deregister the runner in Gitea; pin the probe session and the PR's CI to `terraphim-native` (bigbox) by name |
| helgrind and ltrace absent on bigbox | EXP-010-a (resolver reader) stays DEFERRED; UF-004 is remediated regardless (the writer is removed), so this blocks only the *evidence* for the LIKELY-UB rating, not the fix | Install valgrind on bigbox, or run EXP-010-a on any glibc host that has it |
| Coverage lanes run plain `cargo llvm-cov` (section 1) | In-process exposure persists for dependency writers after this plan | #254 owner: switch both lanes to `cargo llvm-cov nextest --workspace --lib --profile ci` (records: `.github/workflows/ci.yml:79`, `.gitea/workflows/native-ci.yml:94`) |
| Upstream fixes (terraphim_test_utils in terraphim-ai; self_update macro) | None on the local remediation | File the two issues; link from the main issues |

## 13. Docs dependencies

- Remove: the SAFETY line at `crates/terraphim_grep/src/openrouter_client.rs:166`; the SAFETY comment at `crates/terraphim_update/src/state.rs:130`; the descriptive comment at `crates/terraphim_mcp_server/tests/test_mcp_stdio.rs:14` (its block goes with it). No `# Safety` doc section exists in this workspace; the ones that no longer apply are in the registry crate (`terraphim_test_utils-1.20.3/src/lib.rs:21-25, 30-35, 52-57`) and belong to the upstream issue.
- `BUILD.md` (no CONTRIBUTING.md exists): keep the canonical `cargo test --workspace --no-fail-fast` line (:14; the plan said :15, which is the closing fence — corrected by Phase 10) but add two sentences: the workspace forbids `unsafe_code` in every crate and target (inject values in tests; never mutate the process environment; child processes get `Command::env`), and `cargo test` runs libtest binaries in-process at default parallelism while the CI test lanes use nextest (one process per test), so a race that CI cannot show can still show locally. Also note that the line is now stale against the nextest lanes.
- `docs/ci-gap-analysis.md`: add a row to the gates table (:14-25) "Implementation | `unsafe_code = forbid` (workspace lint, ci_guards manifest test, deny.toml ban on terraphim_test_utils) | none | this PR"; amend the Miri row (:22) since zero `unsafe` remains (the `ub-gates` job's crate list is a policy choice, not a necessity); add the coverage-lane note from section 1.
- `.config/nextest.toml` comment block (:20-32): one sentence that the serial group serialises binaries, not threads, and that in-process env races are prevented by the lint, not by this group.
- Phase 4 table rows UF-001/UF-002 "Platform note": wording per section 1 (the lead edits the Phase 4 file; recorded here only).

## 14. Delta

(Later convergence rounds append dated entries here: new findings, changed verdicts, and any change to the winners above.)

- 2026-09-12, Phase 6 round 1 (filed while this plan was being written; no winner changes): EXP-101 (UF-005, post-bump Stacked Borrows re-run; dependency-only, no remediation in this plan), EXP-102 (UF-036 reachability; folded into section 6), EXP-103 (session-analyzer empty-pattern slice panic; safe-Rust panic, same class as UF-009 to UF-011, no UF id yet; the fix shape is the same `floor_char_boundary`/reject-empty-pattern change and belongs with the routed string-slicing issue), EXP-104 (fff-search mmaps files with the watcher disabled and builds `&str` with `from_utf8_unchecked` after a single whole-file check; no UF id yet). If EXP-104 confirms, it is a new shape for this playbook (dependency `unsafe` whose SAFETY precondition is defeated by workspace configuration, `watch: false` at hybrid_searcher.rs:354-359 and mcp_server lib.rs:1274-1279, :1372-1377, :1485-1490) and gets its own section with candidates (enable the watcher; copy instead of mmap via an fff-search option; upstream fix) in a later delta. It is outside the env-writer class and unaffected by the `unsafe_code` lint.
- Still pending for a later delta: the UF-036 production-path hypothesis of section 6; EXP-010-a once helgrind is available.


### Delta 2 (2026-09-12, after Phase 5 round 2; Lead addendum 4)

Three updates. Nothing above changes except where this section says so.

#### D2.1 UF-039 (CONFIRMED_UB, EXP-104): fff-search maps files during every workspace grep with the watcher disabled

**What was confirmed (corrected mechanism).** The `from_utf8_unchecked` at fff-search grep.rs:1710 is reached only by `GrepMode::Fuzzy`, which no workspace call site uses. The workspace `PlainText` path still returns `String`s holding invalid UTF-8 because `PlainTextSink::matched` (grep.rs:750) builds `line_content` with `String::from_utf8_lossy(display_bytes).into_owned()`, which validates and then copies in two separate reads over the live mapping (440 invalid of 186 M lines with a same-length sibling writer; 2,039 of 93 M under ASan). In-place truncation during a grep kills the process with SIGBUS (ASan: `memchr::One::find_raw` under `SliceByLine<PlainTextMatcher, PlainTextSink>::run` from `perform_grep`). Root cause: `memmap2::Mmap::map` (types.rs:640) requires that the mapped file is not modified concurrently; fff-search's SAFETY argument (types.rs:598-607, :637-639) delegates that to the background watcher's invalidation, and all four workspace call sites pass `watch: false`. This is the first confirmed finding on a production path: `terraphim-grep` `search_code` and the MCP `grep_files`/`multi_grep` tools. No conversion change can fix the SIGBUS arm; only not mapping, or mapping under the discipline the crate documents, can.

**Facts re-read from fff-search 0.8.4 today that shape the candidates.**
- `FilePicker::new` (file_picker.rs:696-730) never installs a watcher (`background_watcher: None` at :717); `watch` is only consulted by `new_with_shared_state` (:733-780) and `spawn_background_watcher` (:844-858). Passing `watch: true` to `FilePicker::new` therefore changes nothing.
- `get_cached_content` (types.rs:608-646) returns `None` before mapping when `count >= budget.max_files` (:627-631). `get_content_for_search` (:655-684) then falls through to the slow path (`read_exact` into the caller's owned `Vec<u8>`, :676-683) provided `size <= budget.max_file_size` (:669-671).
- `ContentCacheBudget` fields are `pub` (types.rs:814-820). `ContentCacheBudget::zero()` (:832-840) and any budget with `max_file_size: 0` also disable the slow path at :670, so they silence grep entirely; `from_overrides` (:883-899) treats 0 as "library default" and cannot express `max_files: 0`. The EXP-104 note's "`max_file_size: 0`" mitigation is therefore wrong and must not be used. The usable form is a direct struct literal with `max_files: 0`, `max_bytes: 0`, `max_file_size: 10 * 1024 * 1024` (the workspace's existing `GrepSearchOptions.max_file_size`).
- With `cache_budget: Some(..)`, `has_explicit_cache_budget` is true (:713-720) and `collect_files` (:817-821) does not replace the budget with `new_for_repo`; with `None` (today) it does, and mapping is live in every grep (EXP-104 vmmap evidence).
- On Windows `get_cached_content` is a stub returning `None` (types.rs:589-596), so the slow path is already the only path there; candidate A makes Unix behave like Windows.
- Crate metadata (`Cargo.toml` as published) declares no `repository`; the author's project is fff.nvim by Dmitriy Kovalenko, which Phase 13 confirms before filing upstream.

**Candidates.**

| Candidate | Design | Correctness margin | Perf delta | Diff blast radius | Reviewability | Maintainability | Total |
|---|---|---|---|---|---|---|---|
| A. Stop mapping in the workspace | At all four call sites replace `cache_budget: None` with an explicit budget whose `max_files` is 0 (mapping refused at types.rs:627-631; slow path reads each file into the owned scratch buffer). One small helper per crate, `fn no_mmap_budget() -> ContentCacheBudget`, with a comment naming UF-039 and the invariant it protects; a `ci_guards` test asserting no `cache_budget: None` remains under `crates/` | 4 (no aliasing of externally mutable memory; SIGBUS impossible; a torn `read_exact` yields mixed but owned bytes, and `from_utf8_lossy` over owned bytes yields a valid `String`) | 3 (one `read` syscall per file per grep instead of `mmap` plus page faults; the picker is rebuilt per call so the "persistent" cache never survived a call anyway; worst case one 10 MiB copy per large file) | 3 (four sites, about 12 lines each, two crates; one guard test) | 4 | 3 (a fifth call site written with `None` is caught by the guard test, not by the compiler) | 17 |
| B. Satisfy the SAFETY precondition: run the watcher | Adopt `new_with_shared_state` (`SharedFilePicker`, `SharedFrecency`, background scan thread, notify watcher) at every site; keep mapping | 1 (invalidation is event-driven: the window between a write and the notify event is exactly the EXP-104 window; the crate's own comment says "mitigates", not "prevents"; SIGBUS on truncate remains reachable) | 2 (background threads and a second notify copy per grep call; round-1 notes I-07 and I-28 show it activates `PostScanUnsafeSnapshot` (`unsafe impl Send/Sync`) and a second FSEvents/inotify callback host in the same process as the workspace's own notify watcher) | 1 (constructor, shared handles, lifecycle at four sites) | 1 | 1 | 6 |
| C. Upstream fix in fff-search | `get_cached_content` returns `None` unless a watcher is installed (`has_watcher()`), or validates per line and reads into the buffer when unwatched; pin the workspace to the released version | 4 once released | 4 | 2 (external; unknown timeline; a `[patch.crates-io]` git pin meanwhile) | 3 | 3 (the invariant lives where the `unsafe` lives) | 16 |
| D. Swap the conversion only (per-line `from_utf8` or validate-and-copy in one read) | rejected: cannot address SIGBUS, and the mapped `&[u8]` still aliases memory another process mutates, which is the `Mmap::map` contract violation itself | 1 | 4 | 2 | 3 | 1 | 11 |

**Winner: A**, decided by correctness margin **and availability**: it is the only change *available today* that removes the aliasing (the root UB) and with it both consequences, including SIGBUS. **Phase 10 correction (2026-09-12):** "correctness margin" alone does not discriminate here, because A and C both score 4 on that axis; the axis that actually separates them is availability, which the rubric expresses as blast radius (A 3, C 2, the difference being that C is external with an unknown timeline). The winner is unchanged and the reasoning below already says so in prose; only the one-line deciding-axis label was wrong. C is the durable companion (filed upstream, tracked as a dependency of the docs issue, and the pin dropped when released); B is rejected on every axis; D is recorded so nobody "fixes" this by touching the sink. Residual after A: a concurrent writer can still produce a torn but valid line (owned bytes from two versions of the file); that is a correctness artefact of grepping a changing tree, not UB, and is documented in the helper comment.

**Proving experiment.** EXP-104 proved the original. EXP-805 (appended to the registry, layout of EXP-801, verdict DEFERRED, gated on Phase 13) proves the remediation: the EXP-104 harness rerun with the post-fix option block copied from hybrid_searcher.rs must show `cached_count = 0` after every grep, no mapped temp files in the process map, 0 invalid `line_content` in 200 PlainText greps against the same-length writer, and the truncate arm surviving (an `Err` or a short result, never SIGBUS); the pre-fix option block on the same host is the control that reproduces EXP-104; the `ci_guards` arm proves the guard fires when a `cache_budget: None` is reintroduced.

**Touch list (current tree, ae06cd2).**

| File | Lines | Change |
|---|---|---|
| crates/terraphim_grep/src/hybrid_searcher.rs | :350-353 (`use fff_search::{...}`), :354-360 (`FilePicker::new` options: `watch: false, cache_budget: None`) | import `ContentCacheBudget`; `cache_budget: Some(no_mmap_budget())`; add the helper next to `search_code` with the UF-039 comment |
| crates/terraphim_mcp_server/src/lib.rs | :1274-1280, :1372-1378, :1485-1491 (three `FilePicker::new` blocks with `cache_budget: None`) | same replacement; one private helper in the crate |
| crates/terraphim_agent/tests/ci_guards.rs | append | `no_fff_picker_maps_files`: `rg`-equivalent scan (std `fs` walk) of `crates/**/*.rs` asserting no `cache_budget: None` and, per `FilePicker::new(` occurrence, a `no_mmap_budget()` within the same options block |
| crates/terraphim_mcp_server/tests/test_find_files.rs | :146-164 (greps the whole checkout) | unchanged; it becomes the in-CI exercise of the slow path on both runners |

**Commit slot.** Commit 0, ahead of the ten commits in section 10, on its own branch `task/<UF-039 issue>-fff-mmap` and its own pull request, because it is the only production-path fix and must not wait for the test-only remediation train: `fix(grep,mcp_server): refuse fff-search mmap content cache at every FilePicker call site (Refs #<UF-039 main>)`. The `ci_guards` addition rides in the same commit (one lever: "no unwatched mapping in this workspace"). Release: forward-only, next workspace version bump (Principle 5, 11).

**Gitea.**
- Main (new, child of #252, label `bug`): "CONFIRMED UB on a production path: fff-search maps files during terraphim-grep search_code and MCP grep_files with the watcher disabled; a concurrent writer yields invalid-UTF-8 Strings and truncation SIGBUSes the process (UF-039, EXP-104)". Body: the EXP-104 numbers, the four sites, the corrected mechanism, and why `zero()`/`max_file_size: 0` must not be used.
- Test dependency (new): "test: EXP-805 rerun of the EXP-104 harness with the post-fix option block; ci_guards no_fff_picker_maps_files" (blocks main).
- Docs dependency (new): "docs: helper comment and BUILD.md/ci-gap-analysis note that fff-search content mapping is disabled workspace-wide and why; drop the pin when fff-search ships the upstream fix" (blocks main).
- Upstream (new, on the fff-search repository once confirmed): "`get_cached_content` maps files regardless of `enable_mmap_cache` and without a watcher when constructed via `FilePicker::new`; the SAFETY comment's precondition is unsatisfiable for that constructor; propose `has_watcher()` gating or per-line validation plus buffered reads when unwatched". Cross-linked from the main issue; not a blocker of it.
- Edges (CLAUDE.md reading): `gtr add-dep --owner terraphim --repo terraphim-clients --issue <main> --blocks <test-issue>`, `--issue <main> --blocks <docs-issue>`.

**Playbook note.** This is the new shape flagged in the round-1 delta entry: dependency `unsafe` whose SAFETY precondition is defeated by consumer configuration. Default pick for the shape: make the consumer configuration satisfy the precondition if the crate offers a switch that does so soundly; otherwise disable the unsafe path from the consumer side and file upstream. Here the switch (`watch`) does not reach the constructor in use, so the second branch applies.

#### D2.2 UF-036 and UF-037 narrowed (EXP-105 NO_EVIDENCE)

EXP-105 on bigbox: the shipped pre-dispatch update check dispatches on `UpdateBackend` (default `R2`, ureq, `spawn_blocking`) and never enters self_update; `set_ssl_vars!()` fires only with `TERRAPHIM_UPDATE_BACKEND=github` or the explicit install/update commands. With the GitHub backend forced, 400 of 400 iterations fired both `setenv`s with zero overlaps (the PLT-visible readers are sequential; the shim remains blind to glibc-internal resolver reads, as in EXP-010). UF-037 stays LIKELY-UB by contract, scoped to that configuration.

Effect on section 6: the escalation is answered; no production-path remediation is required for the default path. The local winner (A, `#[ignore]` on `test_check_for_updates_auto`) and the upstream self_update issue stand unchanged; EXP-803's count arm stands. The Delta 1 bullet "UF-036 production-path hypothesis" is closed by EXP-105. The gating of the pre-dispatch update check tracked under #253 is a performance lever (top of that matrix), not a UB fix: on the default path there is no writer to remove, and on the GitHub path the writer lives in self_update and moves only with the upstream fix. Do not cite #253 as remediation of UF-037.

#### D2.3 UF-038 (NOT-UB, panic confirmed by EXP-103): robustness fix

Sites: `crates/terraphim-session-analyzer/src/patterns/loader.rs:79-91` (`load_patterns_from_str` rejects only an empty `patterns` Vec, :79-81), `:136-155` (`merge_patterns`, user patterns override built-ins by name), `crates/terraphim-session-analyzer/src/patterns/matcher.rs:97-120` (`build_automaton` passes every string to `AhoCorasickBuilder`), `:143` (`&text[mat.start()..mat.end()]`). Fix, mirroring `terraphim_automata::find_matches` (which drops empty and sub-minimum keys, terraphim_automata-1.21.1/src/matcher.rs:61-78): in `load_patterns_from_str` reject any pattern string that is empty or whitespace-only with an error naming the tool (`Tool '{name}' has an empty pattern`), apply the same check in `merge_patterns` so user files reaching it through `load_user_patterns` cannot bypass it, and as a second line change `matcher.rs:143` to `text.get(mat.start()..mat.end())` with a `continue` on `None`, so a future non-boundary match can never panic. Test dependency: the EXP-103 harness as two unit tests in the crate (loader: `["cargo build", ""]` and `["cargo build", "  "]` return `Err` naming the tool; matcher: a built automaton over `["", "caf"]` constructed directly, then `find_matches("caf\u{e9} \u{1F600} cargo build")` returns only char-boundary matches). No mocks. Docs dependency: one line in the analyzer's `tools.toml` documentation stating that empty patterns are rejected. Commit slot: commit 12, `fix(session-analyzer): reject empty tool patterns at load time; slice matches with get (Refs #<UF-038 issue>)`, independent of every other commit. Gitea: main (new, child of #252, label `bug`, NOT-UB): "session-analyzer: empty pattern in tools.toml panics find_matches on non-ASCII text (UF-038, EXP-103)"; test dependency as above; edges as in 3.6.


### Delta 3 (2026-09-12, after Phase 5 round 3; Lead addenda 5 and 6)

#### D3.1 UF-040 (CONFIRMED_UB, EXP-201): hostile non-UTF-8 entry names inflate fff-search's lossy path storage past the 4096-byte path buffer; `read_to_buf` truncates mid-`U+FFFD` and returns an invalid `str`

**What was confirmed (with EXP-201's corrections).** fff-search 0.8.4 stores every relative path as a `String` from `to_string_lossy().into_owned()` (file_picker.rs:232, :352, :406): on Linux every invalid byte in an entry name becomes a three-byte `U+FFFD`. `ChunkedString::read_to_buf` (simd_path.rs:170-186) copies `byte_len.min(buf.len())` bytes and returns `from_utf8_unchecked(&buf[..total])`; its only comment (:168) documents the truncation, not the UTF-8 hazard. The truncating site on the grep path is `FileItem::absolute_path` (types.rs:271-275: `read_to_buf` into a local `[u8; PATH_BUF_SIZE]`, then `base_path.join(rel)`), called from `get_content_for_search` (:678) and `get_cached_content` (:635), and also during `collect_files` for the git status lookup (file_picker.rs:831). EXP-201 (OrbStack Linux container; bigbox was full) observed invalid-UTF-8 `PathBuf`/`str` from `absolute_path` in five of seven hostile geometries (9 invalid strings, controls clean) and `DirItem::dir_name` panicking with `start byte index 10724 is out of bounds for string of length 4096` at types.rs:163 in the 15x255 geometry (`dir_name_panics=1`). The base path is attacker-chosen through `terraphim-grep --paths` (main.rs:79-81, :524-534) and the MCP `find_files`, `grep_files` and `multi_grep_files` `path` argument (mcp_server lib.rs:1264-1267, :1349-1352, :1455-1458). Linux only: APFS refuses non-UTF-8 names.

**Why commit 0 does not cover it (confirming and scoping round 3's I-304).** The single `Mmap::map` site in fff-search 0.8.4 is types.rs:640, inside `get_cached_content` (:608-646), whose only callers are `get_content_for_search` (:665) and `warmup_mmaps` (file_picker.rs:1925, reached only from the shared-state scan path, never from `FilePicker::new`). With the UF-039 budget (`max_files: 0`) `get_cached_content` returns at :627-631 before both `absolute_path` (:635) and `Mmap::map` (:640). I-304 is therefore correct as stated: the budget closes every mapping path. It does not touch inflation: `get_content_for_search` falls through to the slow path and calls `absolute_path` at :678 on every file, and `collect_files` calls it at file_picker.rs:831 before any grep. UF-040 is live after commit 0 exactly as before it. The two findings share the hostile tree and nothing else.

**Robustness note (folded from EXP-201).** Beyond the invalid `str`: `write_dir_name` (types.rs:153-164) indexes the truncated string with an offset computed on the untruncated one and panics when the inflated relative path exceeds 4096 bytes; and every non-UTF-8 entry becomes unsearchable, because the lossily stored name no longer names the on-disk entry (`File::open` fails, the file is silently dropped: 0 matches with `total_files_searched=1` in every geometry, including the controls). Both go into the upstream issue; candidate B also prevents both in the workspace.

**Candidates.** Candidate C was checked against the sources, not assumed: `FilePickerOptions` (file_picker.rs:434-448) has seven fields (`base_path`, `enable_mmap_cache`, `enable_content_indexing`, `mode`, `cache_budget`, `watch`, `follow_symlinks`); there is no path-length cap, no strict-UTF-8 mode and no non-lossy storage switch; the only `to_str()` guards (file_picker.rs:1585, :1608, :1621) are on the frecency and git-status side, not between the `ignore` walker and the arena (EXP-201's falsifiability check found every hostile file collected). `PATH_BUF_SIZE` is a crate constant used at ten sites and is not configurable.

| Candidate | Design | Correctness margin | Perf delta | Diff blast radius | Reviewability | Maintainability | Total |
|---|---|---|---|---|---|---|---|
| A. Upstream fix in fff-search, workspace pin until released | `read_to_buf` truncates on a char boundary (`floor_char_boundary`) or returns `Err` when `byte_len > buf.len()`; `absolute_path` builds through a heap `String` for long paths; `write_dir_name` bounds `offset` by the written length; longer term, store `OsString`/bytes instead of a lossy `String` so non-UTF-8 entries stay openable. Workspace pins via `[patch.crates-io] fff-search = { git, rev }` until a release carries it | 4 (removes the `unsafe` precondition failure at its site) once released; 0 until then | 4 | 2 (external; crates.io is already at 0.10.6 and 0.10.7-nightly, so the fix may land on a line the workspace has not adopted; a 0.8.4 to 0.10.x upgrade is its own task with API drift) | 3 | 4 (the invariant lives where the `unsafe` lives) | 17 on release, not available today |
| B. Workspace pre-walk rejecting hostile trees before the base path reaches fff-search | A `reject_hostile_tree(base: &Path) -> Result<(), String>` helper (std `read_dir` recursion, names only, no `stat`, symlinks not followed, matching `follow_symlinks: false`) that fails with a named entry when any entry name is not valid UTF-8 or when any relative path exceeds `PATH_BUF_SIZE - 1` bytes (the second rule covers deep trees of valid multi-byte names, which truncate mid-character by the same mechanism; EXP-201 did not test that geometry, so it is a conservative extension, not a proved case). Called at the top of `HybridSearcher::search_code` (covers `--paths` and every library caller) and at the top of the three MCP tools before `FilePicker::new`. Error surfaces as the existing `Err(String)` / `ErrorData::invalid_params` | 3 (removes the enabling condition for a cloned hostile checkout; residual TOCTOU: an entry created between the pre-walk and `collect_files` still reaches the inflation, which requires a concurrent writer in the searched tree, the UF-039 exposure class, and is bounded by the duration of one grep) | 2 (a second directory walk per grep; `readdir` only, no metadata, so cheap relative to fff-search's own `ignore` walk with git status, but it does not honour `.gitignore`, so `target/` and `node_modules/` are walked; acceptable for a CLI and a per-call MCP picker, to be measured against the #253 baselines) | 3 (one 30-line helper duplicated in two crates, terraphim_mcp_server does not depend on terraphim_grep; four call sites, one line each; two Linux-only tests) | 4 | 3 (a fifth `FilePicker::new` without the guard is caught by extending the D2.1 `ci_guards` test, not by the compiler) | 15 |
| C. fff-search option that disables lossy storage or caps path length | none exists in 0.8.4 (verified above) | 0 | n/a | n/a | n/a | n/a | not available |
| D. Reject only at the CLI/MCP argument boundary (validate the base path string, not the tree) | rejected: the base path is always valid UTF-8 in EXP-201; the hostile names are inside the tree | 0 | 4 | 4 | 4 | 1 | 13 |

**Winner: B**, decided by correctness margin among the options the workspace controls today: it is the only available change that removes the enabling condition on every attacker-reachable path (CLI `--paths`, three MCP tools) and also prevents the `dir_name` panic and the silent drop of non-UTF-8 files (they are refused with a named error instead). A is the durable fix and is tracked as a dependency of the docs issue: when a released fff-search carries it, rerun EXP-806 with the guard bypassed and only then consider removing the guard (the guard also closes the robustness defects, so keeping it is reasonable). The TOCTOU residual is stated in the helper comment and in the docs commit; it does not change the verdict because the residual needs an active concurrent writer, whereas the confirmed exposure is a static hostile checkout.

**Proving experiment.** EXP-201 proved the original. EXP-806 (appended to the registry, EXP-801 layout, verdict DEFERRED gated on Phase 13) proves the remediation: the EXP-201 hostile geometries driven through the workspace entry points (`HybridSearcher::search` over `with_search_paths(vec![hostile])`, and `McpService::grep_files`/`find_files`/`multi_grep_files` with the hostile `path`) must return an error naming the offending entry before any `FilePicker` is built, with the control geometries (2x250, 3x255) searching normally; the same trees through raw fff-search 0.8.4 must still reproduce EXP-201 (proves the trees are still hostile); the `ci_guards` arm must fail when a `FilePicker::new` site loses its guard call.

**Touch list (current tree, 46bbc43; crates/ unchanged since 1120180).**

| File | Lines | Change |
|---|---|---|
| crates/terraphim_grep/src/path_guard.rs (new) and src/lib.rs :23-30 (`pub mod path_guard;`) | new | `pub fn reject_hostile_tree(base: &Path) -> Result<(), String>`; `PATH_BUF_SIZE` mirrored as a local constant 4096 with a comment naming fff-search simd_path.rs and UF-040 |
| crates/terraphim_grep/src/hybrid_searcher.rs | :342-354 (`search_code` head, before `FilePicker::new`) | `crate::path_guard::reject_hostile_tree(&search_path)?;` (map to the existing `Err(String)`) |
| crates/terraphim_grep/src/main.rs | :524-534 | unchanged; `--paths` reaches `search_code` |
| crates/terraphim_mcp_server/src/lib.rs | :1264-1273 (`find_files`), :1349-1373 (`grep_files`), :1455-1486 (`multi_grep_files`): before each `FilePicker::new` | private `reject_hostile_tree` copy (or a tiny shared module if the two crates gain a common dependency later); errors as `ErrorData::invalid_params` |
| crates/terraphim_grep/tests/path_guard_hostile_tree.rs (new, `#![cfg(target_os = "linux")]`) | new | 6x250 `0xFF` tree returns `Err` naming the entry; 2x250 tree searches; no mocks |
| crates/terraphim_mcp_server/tests/test_find_files.rs | append, `#[cfg(target_os = "linux")]` | `grep_files` over the hostile tree returns `is_error` with the entry named |
| crates/terraphim_agent/tests/ci_guards.rs | extend the D2.1 test | per `FilePicker::new(` occurrence under `crates/`, a `reject_hostile_tree(` call within the preceding 40 lines |

**Commit slot.** Commit 0b, in the same production-path pull request as commit 0 (branch `task/<UF-039 issue>-fff-mmap`, renamed to `task/<n>-fff-hardening` if both issues share a branch): `fix(grep,mcp_server): refuse trees with non-UTF-8 or over-long entry names before fff-search indexes them (Refs #<UF-040 main>)`. Same crates, same four call sites, same reviewer, same release; separate commits so either can be reverted alone. It must not wait for the test-only train of section 10.

**Gitea.**
- Main (new, child of #252, label `bug`): "CONFIRMED UB on a production path: hostile non-UTF-8 entry names under a user-supplied search path inflate fff-search's lossy path storage past 4096 bytes; read_to_buf returns an invalid str (UF-040, EXP-201); also dir_name panic and silent drop of non-UTF-8 files". Body: the EXP-201 numbers and geometries, the four entry points, why commit 0 does not cover it, and the TOCTOU residual.
- Test dependency (new): "test: EXP-806 rerun of the EXP-201 geometries through HybridSearcher::search and the three MCP tools; Linux-only integration tests; ci_guards arm" (blocks main).
- Docs dependency (new): "docs: path_guard rationale (non-UTF-8 and over-long entries refused, TOCTOU residual, remove or keep when fff-search ships the upstream fix); BUILD.md/ci-gap-analysis note; error text in the terraphim-grep and MCP tool docs" (blocks main).
- Upstream: the crate declares no repository; crates.io lists the sole owner as `dmtrKovalenko` (github.com/dmtrKovalenko). Phase 13 files against the repository that publishes `fff-search` from that account (the fff.nvim project, to be confirmed by locating the crate's `Cargo.toml` in that repository's workspace before filing) and first checks whether 0.10.6 or the 0.10.7 nightlies already changed `read_to_buf` or the lossy storage, which would turn the upstream item into an upgrade task. Proposed title and body: "`ChunkedString::read_to_buf` truncates at `PATH_BUF_SIZE` and returns `from_utf8_unchecked` of the prefix; with lossy path storage (`to_string_lossy`, file_picker.rs:232/352/406) a non-UTF-8 entry name inflates 3x and the cut lands inside `U+FFFD`, so `FileItem::absolute_path` and `DirItem::dir_name` return invalid `str`/`String`/`PathBuf` through the safe API (0.8.4; reproducer: six nested directories named 250 x 0xFF on Linux). `write_dir_name` (types.rs:153-164) additionally panics with an out-of-bounds index for 15 x 255. Non-UTF-8 entries are also unsearchable because the stored name no longer opens the file. Suggested fix: truncate on a char boundary or return an error when `byte_len > buf.len()`, bound the `dir_name` offset by the written length, and consider storing paths as bytes." Cross-linked from the main issue; not a blocker of it.
- Edges (CLAUDE.md reading): `gtr add-dep --owner terraphim --repo terraphim-clients --issue <main> --blocks <test-issue>`, `--issue <main> --blocks <docs-issue>`.

**Playbook note.** Same shape as D2.1 (dependency `unsafe` whose precondition the consumer's input can defeat), second branch again: the crate offers no switch, so the consumer refuses the input class and files upstream.

## 15. Report

- Winners: UF-001 A (constructor injection plus lint; runners-up B 11, C 15); UF-002 A (settings-dir injection plus memory-only storage plus lint; B 12, C 15); UF-004 A (history-path injection; B 7, C 15); UF-036 A locally, 18 (`#[ignore]` the live-network test) plus upstream issue, production-path hypothesis escalated (runner-up B 14, C fork 14 after the Phase 10 arithmetic correction); headline lever `[workspace.lints.rust] unsafe_code = "forbid"` with `[lints] workspace = true` in 11 manifests, a `ci_guards` manifest test and a `deny.toml` ban.
- EXP ids appended to the registry: EXP-801 (UF-001 post-fix probe), EXP-802 (UF-002 post-fix probe), EXP-803 (UF-004 and UF-036 post-fix count), EXP-804 (workspace lint fault injection), all OPEN; EXP-805 (UF-039 post-fix rerun of the EXP-104 harness) and EXP-806 (UF-040 post-fix rerun of the EXP-201 geometries through the workspace entry points), both DEFERRED gated on Phase 13.
- Delta 3 winner: UF-040 B (workspace pre-walk `reject_hostile_tree` at `search_code` and the three MCP tools, refusing non-UTF-8 or over-long entry names; runner-up A upstream fix 17 on release, tracked; C does not exist in 0.8.4; D 13). Commit 0b in the production-path PR. The UF-039 budget closes every mapping path (I-304 confirmed) but not the `absolute_path` inflation.
- Delta 2 winners: UF-039 A (explicit `ContentCacheBudget` with `max_files: 0` at all four `FilePicker::new` sites, no mapping; runners-up C upstream 16, D conversion-only 11, B watcher 6); UF-037 no production remediation on the default path (EXP-105); UF-038 reject empty patterns at load time plus `get` slicing.
- Source files the plan touches: crates/terraphim_grep/src/openrouter_client.rs; crates/terraphim_grep/src/hybrid_searcher.rs (Delta 2, Delta 3); crates/terraphim_grep/src/path_guard.rs and src/lib.rs (Delta 3, new module); crates/terraphim_grep/tests/path_guard_hostile_tree.rs (Delta 3, new); crates/terraphim_mcp_server/tests/test_find_files.rs (Delta 3); crates/terraphim_mcp_server/src/lib.rs (Delta 2, Delta 3); crates/terraphim-session-analyzer/src/patterns/loader.rs and src/patterns/matcher.rs (Delta 2); crates/terraphim_agent/src/service.rs; crates/terraphim_agent/tests/tui_service_tests.rs; crates/terraphim_agent/tests/ci_guards.rs; crates/terraphim_agent/build.rs (deleted); crates/terraphim_agent/Cargo.toml; crates/terraphim_update/src/state.rs; crates/terraphim_update/src/lib.rs; crates/terraphim_mcp_server/tests/test_mcp_stdio.rs; crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs; crates/terraphim_mcp_server/tests/mcp_rolegraph_validation_test.rs; crates/terraphim_mcp_server/Cargo.toml; Cargo.toml (root) and the other nine member Cargo.toml files (cli, command_runtime, grep, hooks, lsp, negative_contribution, sessions, update, session-analyzer); Cargo.lock; deny.toml; BUILD.md; docs/ci-gap-analysis.md; .config/nextest.toml (comment only); .github/workflows/ci.yml (`--test ci_guards`).

## Blocker update (lead, 2026-09-12 11:45 BST): bigbox root disk full

At about 10:38 BST the bigbox root filesystem (/dev/md2, 3.5 TB) reached 100 percent with 0 bytes free. The Gitea PostgreSQL container (gitea-stack-db-1) went into a restart loop with "could not write lock file postmaster.pid: No space left on device", so git.terraphim.cloud answered 500 on git push and 401 on the API (the token was valid; the health endpoint showed database:ping fail). The audit's own build cache /tmp/ubx-probe-target (7.4 GB, rebuildable) was deleted by the lead; probe sources and logs under /tmp/ubx-probe, /tmp/ubx-probe-logs and /tmp/ubx-probe-scratch were kept (all copied into this workspace already). Round 3 executor was told to halt all bigbox work; EXP-201 is blocked on bigbox disk space unless run elsewhere. Largest consumers for the operator to review (read-only survey): /home/alex/gitea-stack 415 GB, /home/alex/quarantine-20260819 226 GB, /home/alex/projects 71 GB, /home/alex/scratch 35 GB, /tmp 169 GB before clean-up. This joins #281 (orphaned terraphim_server processes) and the rogue runner as operator actions.

## Delta 4 (2026-09-12, Phase 10 fresh-eyes review, SOLO)

Independent re-derivation of the three load-bearing claims the reviewer was asked not to trust, plus the gaps found. Nothing in the plan's winners changes. Flat corrections were applied in place (rubric arithmetic in section 6, the deciding-axis label in D2.1, the prose-line list in section 11) and are listed in `phase10_fresh_eyes_log.md`.

**Precondition re-checked first.** `git diff --stat 1120180..46bbc43 -- crates/` is empty, so every `crates/` file:line in this plan is valid at HEAD. All eleven members carry `edition.workspace = true` against `edition = "2024"` at root `Cargo.toml:19`, and `rust-toolchain.toml` pins 1.97.1, so `set_var` is an `unsafe fn` in every member and the headline lint really is a compile-time gate everywhere. `terraphim_grep`'s `default = ["llm", "code-search"]` puts both the UF-001 writer and the fff-search path in the default build, so section 1's exposure table stands.

**(1) UF-039: the zero-file budget does close every `Mmap::map` path — confirmed, and by a wider margin than claimed.** `memmap2::Mmap::map` appears exactly once in fff-search 0.8.4, at `types.rs:640` inside `get_cached_content`. That function's guards run in the order: already-cached; `size < MMAP_THRESHOLD || size > budget.max_file_size`; then `count >= max_files || bytes + size > max_bytes`. With `max_files: 0` the third guard fires on the first file (`0 >= 0`) and with `max_bytes: 0` it fires again, both before `absolute_path` at :635 and `Mmap::map` at :640. Its only callers are `get_content_for_search` (:665) and `warmup_mmaps` (`file_picker.rs:1925`) — and `warmup_mmaps` is **dead in 0.8.4**: its sole call site, `scan.rs:317`, is commented out under the upstream comment "Skipped as potentially unsafe - figure this out later". The budget-retention argument also holds: `FilePicker::new` sets `has_explicit_cache_budget` from `options.cache_budget.is_some()` (`file_picker.rs:713-720`) and `collect_files` calls `self.cache_budget.reset()` rather than replacing the budget when it is set (`:817-821`). The warning against `ContentCacheBudget::zero()` and `max_file_size: 0` is correct: with `max_file_size: 0` the slow path's `size > max_file_size` test (`types.rs:669-671`) rejects every non-empty file, so grep returns nothing. `from_overrides` genuinely cannot express `max_files: 0` (`:883-899` treats 0 as "library default"). **Residual worth stating in the docs commit:** the budget removes fff-search's mapping, not all mapping in the process. `FrecencyTracker::open` (`crates/terraphim_mcp_server/src/lib.rs:110`) still opens an LMDB environment that memory-maps its data file, which is EXP-207's surface, and libgit2 maps pack files during the git-status lookup. Nobody should read commit 0 as "this process no longer mmaps".

**(2) UF-040: the guard sits ahead of every `FilePicker::new` the workspace builds — confirmed, with two qualifications.** There are exactly four constructor sites at HEAD (`hybrid_searcher.rs:354`; `mcp_server/src/lib.rs:1274`, `:1372`, `:1485`), exactly two dependent crates, and no `new_with_shared_state` anywhere, so the plan's touch list is complete. `terraphim_mcp_server` does not depend on `terraphim_grep`, so the private-copy decision is forced, as the plan says. The three MCP function heads are `find_files` at :1264, `grep_files` at :1349 and `multi_grep_files` at :1455, matching the cited ranges. Qualification (a): all four sites use `..FilePickerOptions::default()` and never set `follow_symlinks` explicitly; the default is `false` (`file_picker.rs:460`), so the helper's "symlinks not followed, matching `follow_symlinks: false`" is right today but rests on an upstream default, not on a workspace assertion — the `ci_guards` arm should assert the absence of `follow_symlinks: true` as well as the presence of the guard call. Qualification (b): the guard covers the grep path, but `collect_files` also calls `absolute_path` at `file_picker.rs:831` for the git-status lookup, and the two other workspace entry points into fff-search — `FrecencyTracker::open` (`lib.rs:110`) and `QueryParser::new(AiGrepConfig)` (`:1506`) — are outside the guard. Neither builds a picker or touches the path arena, so neither is a gap for UF-040, but the touch list should say so rather than leave it inferred.

**(3) UF-001 and UF-002 alone do NOT remove every env writer from the test binaries — the plan as a whole does.** Six `unsafe` blocks at HEAD, re-counted: `openrouter_client.rs:170`, `:177`, `:185`, `:189` (four), `terraphim_update/src/state.rs:131`, `mcp_server/tests/test_mcp_stdio.rs:15`. That matches `phase1_unsafe_surface_inventory.md` section A. Commits 1 and 2 close the four openrouter blocks and the `EnvVarGuard` usage, leaving **two of six** `unsafe` blocks plus eight `terraphim_test_utils::set_env_var` call sites (`mcp_autocomplete_e2e_test.rs:22`, `:28`; `mcp_rolegraph_validation_test.rs:21`, `:27`, `:137`, `:284`, `:394`, `:618`) and the `self_update` dependency writer. Those are closed by commits 3, 4, 5 and 9 respectively, all already in section 10, so the plan reaches every call site — but the sequencing note in section 7 is the load-bearing part: commit 7 cannot land before commits 1 to 6 or the workspace will not compile. No gap, but the claim must be read as "the plan", not "the two injections".

**Remaining items for the plan's author (not applied by the reviewer).**
- Section 11's control row says "pre-fix control: ae06cd2"; HEAD is 46bbc43 and `crates/` is identical, so the control is still valid, but Phase 13 should pin the control by tree hash rather than by commit as the branch advances.
- The upstream owner for fff-search is given as `dmtrKovalenko` in Delta 3 and the fff.nvim project is attributed to "Dmitriy Kovalenko" in Delta 2, while `crates/terraphim_grep/RELEASE_RESEARCH.md:93` records the workspace's own historical git dependency as `github.com/AlexMikhalev/fff.nvim`. Phase 13 should resolve which repository publishes the crate before filing, as Delta 3 already instructs.

## Delta 5 (2026-09-12, lead: the fff-search version tension, and the recommendation)

Written by the lead because the remediation architect reached its model limit. Evidence is in Lead addenda 10 and 11 of `phase4_unified_findings.md`; the 0.10.6 sources were read read-only from a scratchpad extraction and were not added to the workspace.

### The tension

Gitea #257 asks for fff-search to move to a release on a patched git2, because RUSTSEC-2026-0183 and RUSTSEC-2026-0184 are against the git2 crate and are fixed only in git2 >= 0.21.0. The first fff-search release requiring `git2 ^0.21.0` is 0.10.6. But 0.10.6 is also the version in which this plan's UF-039 remediation stops working: it adds a second, unbudgeted `Mmap::map` at `src/types.rs:762` in `get_content_for_search`, gated only by `FRESH_MMAP_THRESHOLD` and `max_file_size`, reached exactly when a zero-file budget makes `get_cached_content` return `None`. The advisory fix and the UB mitigation are therefore in direct conflict.

### The fact that decides it: the vulnerable APIs are unreachable

Both advisories are API-specific, not blanket:

- RUSTSEC-2026-0183 requires a call to `Remote::list()`.
- RUSTSEC-2026-0184 requires a `BlameHunk` obtained from `Blame::blame_buffer()`.

The complete set of git2 items fff-search 0.8.4 references is `git2::Repository`, `git2::Error` and the `git2::Status` flags (`CURRENT`, `INDEX_DELETED`, `INDEX_MODIFIED`, `INDEX_NEW`, `INDEX_RENAMED`, `INDEX_TYPECHANGE`, `WT_MODIFIED`, `WT_NEW`). There is no occurrence of `Remote`, `Blame` or `blame_buffer` anywhere in its sources. No workspace crate depends on git2 directly (no `git2` entry in any member `Cargo.toml`, no `git2::` path in `crates/`), so fff-search is the only consumer. **Neither advisory's vulnerable code path is reachable from this workspace at any input.**

That inverts the risk comparison. Staying on 0.8.4 carries two unsound-class advisories whose APIs are never called. Upgrading to 0.10.6 clears those unreachable advisories and, in exchange, reintroduces a *confirmed, reachable* UB on the production grep path for every file at or above 256 KiB on Linux.

### Candidates and rubric

Axes as elsewhere in this plan, 0-4 each: correctness margin, perf delta, diff blast radius, reviewability, maintainability.

| Candidate | Correctness | Perf | Blast radius | Reviewability | Maintainability | Total |
|---|---|---|---|---|---|---|
| **A. Stay on 0.8.4, pin it, assert the pin, document the reachability argument** | 4 | 4 | 4 | 4 | 3 | **19** |
| B. Upgrade to 0.10.6 and vendor a patch gating the fresh-mapping site | 3 | 4 | 1 | 1 | 1 | 10 |
| C. Upgrade to 0.10.6 and wait on an upstream fix | 0 | 4 | 3 | 3 | 2 | 12 |
| D. Drop fff-search and use another walker or ripgrep's crates | 4 | 1 | 0 | 1 | 2 | 8 |

**Winner: A, stay on 0.8.4 and pin it.** The deciding axis is correctness margin, and unusually it is not close: A is the only candidate that leaves no reachable UB. C is disqualified outright, since it knowingly ships the reachable finding while waiting on a third party. B scores badly on everything except perf because a vendored patch to a dependency's `unsafe` is the least reviewable change in this plan and has to be re-applied on every upgrade. D is a rewrite of the search stack to remove two unreachable advisories.

**Runner-up if the user overrides:** B, and then the vendored patch must gate `get_content_for_search`'s fresh path on the same budget as `get_cached_content`, and EXP-805 must be re-run with files above `FRESH_MMAP_THRESHOLD` in the corpus, which the current harness does not have.

### What this changes in the plan

1. **Commits 0 and 0b stand unchanged.** No upgrade replaces either. The zero-file `ContentCacheBudget` is complete on 0.8.4, and `read_to_buf` is unchanged in 0.10.6, so the hostile-tree guard is needed at any version.
2. **New requirement: pin fff-search to `=0.8.4`** in the two dependent crates and assert the pin in `ci_guards`. Rationale to carry in the comment: a routine `cargo update` past 0.8.4 silently reopens UF-039 for large files while leaving the mitigation code visibly in place. This is the single most dangerous silent regression this audit found, because the code review after such an update would show the fix still present. Added to Gitea #294's acceptance criteria.
3. **New requirement: assert `follow_symlinks` explicitly** at all four `FilePicker::new` sites rather than relying on the upstream default (Delta 4 qualification (a)).
4. **deny.toml gains a reachability note** for RUSTSEC-2026-0183/0184 recording the enumerated git2 surface above, so a future reader does not re-derive it. This belongs in the docs commit, not a new ignore entry: both advisories are unsound-class and this config already treats them as informational.
5. **#257 stays open, with its meaning changed.** It is no longer "bump fff-search to clear the advisories" but "revisit when fff-search ships a release that both requires git2 >= 0.21 and gates the fresh-mapping path". It should be re-titled and made to depend on the upstream issue.
6. **The upstream fff-search issue text in `phase9_gitea_log.md` must be extended** to cover the second map site in 0.10.6 and to state that `enable_mmap_cache` gates neither site, so the report is against current upstream rather than only the pinned version. Not filed; the text is for the user.

### Caveat on candidate A

The reachability argument is a property of fff-search 0.8.4's source as read today, not a guarantee. If the pin is ever raised, or if fff-search adds a remote or blame feature, the argument lapses. The `ci_guards` pin assertion is what keeps that from happening silently, which is why requirement 2 is not optional cosmetics.

## Delta 6 (2026-10-07, lead: re-baseline on `main@c7ae820` after four weeks)

**Status**: Draft, awaiting user sign-off (Phase 2 approval gate).
**Research doc**: `RESEARCH.md` (this scratchpad), Phase 1 of the re-baseline.
**Date verified**: `date '+%Y-%m-%d %H:%M %Z'` -> 2026-10-07 10:03 BST.
**Why this is a delta, not a new plan**: no `crates/` change since `1120180` touched any
mechanism the audit proved, so every candidate set, rubric and winner in sections 3-7 and
Deltas 2, 3 and 5 stands unchanged. What changed is the *ground*: the exposure table, the
file:line map, the writer inventory and four supporting files. Re-deriving the winners
would discard the audit's evidence for no gain.
**Traceability note**: this repository tracks work in Gitea, not Linear. The design-skill
`Linear:` marker is read as `Gitea:` throughout; `General:` and `Out-of-scope:` keep their
meaning.

### D6.1 Corrected scope-of-exposure table (supersedes section 1 entirely)

Section 1's table was written against tree `ae06cd2` on `task/254-ci-pipeline`, the branch
that *introduces* nextest. `git merge-base --is-ancestor 1120180 main` is false: the audit
baseline is not on `main`, and `main` has never run the test suite under nextest. The
exposure is the inverse of what section 1 recorded.

| Lane | Command on `main@c7ae820` | Process model | Exposed? |
|---|---|---|---|
| GitHub `build` | `.github/workflows/ci.yml:45` `cargo test --workspace --lib --no-fail-fast` | plain libtest, in-process, ubuntu-latest (glibc) | **Yes**: terraphim_grep lib (UF-001 x2 after D6.2), terraphim_update lib (UF-004, UF-036) |
| GitHub updater | `ci.yml:50` `cargo test -p terraphim_update --test manifest --test r2_update --test managed_mode --test policy` | plain libtest | **Yes**: `managed_mode` (D6.2 writer 2) |
| native-ci `test` | `.gitea/workflows/native-ci.yml:130` `cargo test --workspace --all-targets --no-fail-fast` | plain libtest, bigbox glibc 2.39 | **Yes**: all of the above, plus UF-002 (`--all-targets` reaches `--test` binaries) |
| native-ci `coverage` | `native-ci.yml:199` `cargo llvm-cov nextest --workspace --all-targets` | **nextest** | No (#313 landed, 2026-09; the one piece of remediation that did) |
| GitHub coverage | absent | n/a | n/a |
| Developer default | `BUILD.md:13` `cargo test --workspace --no-fail-fast` | plain libtest | Yes |

Consequences:
1. **The CONFIRMED_UB verdicts strengthen.** Both exposed lanes are glibc, where the
   `setenv`/`getenv` pair is a C11 data race, not merely an Apple Libc contract violation.
   The wording "both coverage lanes and every `cargo test` invocation" in section 1 and in
   `phase4_unified_findings.md` rows UF-001/UF-002 becomes "both CI test lanes, every
   `cargo test` invocation, and not the coverage lane".
2. **The regression-instrument rule is unchanged and now matters more**: EXP-801/802/803
   must run the libtest binary directly under the LD_PRELOAD shim. It cannot become a CI
   gate, because native-ci's runner allowlist rejects any program that is not `cargo` or a
   repo script (`native-ci.yml:164-167`). It stays a manual bigbox step, recorded in
   `UB_RUNBOOK.md`.
3. `.config/nextest.toml` does not exist on `main`, so section 4.2 candidate C's
   `serial-external-state` co-location note is void here. It does not change C's score;
   C is a runner-up in both places.

### D6.2 Two new writers of the confirmed shape, both committed after the audit baseline

The `unsafe` inventory grew from 6 blocks to 14 (`rg -n 'unsafe \{|unsafe impl|unsafe fn' crates/`).
Neither new cluster is an ancestor of `1120180`. Both carry a SAFETY comment of the kind
the audit disproved. Both are in scope for Delta 6 as new commits.

**Writer A: `crates/terraphim_grep/src/lib.rs:905-953`** — `rlm_max_tokens_default_and_override`,
added by `e672f13` on 2026-10-04 (#349, "raise RLM max_tokens to 8000"). Six `unsafe`
blocks on `TERRAPHIM_GREP_MAX_TOKENS` under the comment *"test code, single-threaded, no
concurrent env access"* — the same sentence EXP-001b falsified for this crate. It lives in
the `terraphim_grep` **lib** binary with roughly eleven sibling tests that call `.search(...)`
-> `HybridSearcher::search_code` -> fff-search -> `git2::init` -> libgit2 `getenv`
(`lib.rs:651,678,711,744,788,819,868,1013,1059,1111`). This is UF-001's mechanism, same
binary, on a lane D6.1 verifies exposed. **Classification: UF-001 second instance**, not a
new finding; it inherits EXP-001b as its proof of mechanism (an EXP-801 arm on
`TERRAPHIM_GREP_MAX_TOKENS` would confirm it directly and is the honest next instrument).

Remedy: the same winner as section 3, candidate A. The reader under test is
`TerraphimGrep::rlm_max_tokens()`. Split the env read from the parse:

```rust
// crates/terraphim_grep/src/lib.rs (sketch; implementation is Phase 3)
const DEFAULT_RLM_MAX_TOKENS: u32 = 8000;
/// Pure: the value `TERRAPHIM_GREP_MAX_TOKENS` would select, given its observed content.
/// Invalid, zero and absent all fall back to the default.
fn parse_rlm_max_tokens(raw: Option<&str>) -> u32 {
    raw.and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_RLM_MAX_TOKENS)
}
impl TerraphimGrep {
    fn rlm_max_tokens() -> u32 {
        parse_rlm_max_tokens(std::env::var("TERRAPHIM_GREP_MAX_TOKENS").ok().as_deref())
    }
}
```

The test becomes five env-free assertions over `parse_rlm_max_tokens` (`None` -> 8000;
`Some("4000")` -> 4000; `Some("not-a-number")` -> 8000; `Some("0")` -> 8000; and one
boundary of the implementer's choosing), and all six `unsafe` blocks plus the false SAFETY
comment go. Behaviour is preserved exactly as the current test asserts it: the `filter`
reproduces today's zero-falls-back rule, which the existing assertions pin.

**Writer B: `crates/terraphim_update/tests/managed_mode.rs:320-347`** — writes `HOME`
under `#[serial(update_home_env)]`. The file holds 9 tests; exactly one carries that
attribute. This is EXP-014's proven defeat condition: `serial_test` excludes only other
`#[serial]` tests sharing the key, so the eight unannotated siblings run concurrently with
the writer. The SAFETY comment ("serialized via `#[serial]` within this test binary; no
other thread in this process mutates HOME concurrently") is true of *writers* and silent
about *readers*, which is the precise error EXP-014 exists to expose. The binary is run by
name on both exposed lanes (`ci.yml:50`, `native-ci.yml:130`).

Remedy: the same winner, candidate A, applied to the seam the test already names. The test
comment states that `check_for_updates_auto_with_policy` "is the injectable seam this test
exercises directly"; the fake `HOME` exists only so the zero-write assertion does not
depend on the real home directory. So inject the home root rather than setting it:
`check_for_updates_auto_with_policy_in(bin, version, policy, home: &Path)`, with the
existing function delegating with the resolved home. The test then passes
`temp_home.path()` and mutates nothing. If that seam cannot take the parameter without
disturbing the updater's policy resolution, the fallback is section 4.2 candidate C (its
own one-test binary plus a `ci_guards` arm asserting the one-test invariant) — **not**
candidate B, which is what the file does today and what EXP-014 refuted.

**Classification**: UF-004 second instance (same crate, same variable class, same
mechanism), inheriting EXP-010's window evidence.

**Design note on recurrence.** The shape spread twice in four weeks, both times with a
plausible-sounding SAFETY comment, in commits whose reviewers were looking at an updater
fix and a token-limit fix. That is evidence for the plan's own headline conclusion: review
does not catch this, and only the compile-time lever does. Until the lever lands, the
cheapest interim stop is a `ci_guards` arm scanning `crates/**/*.rs` for
`env::set_var`/`env::remove_var` against an explicit allow-list of the known sites, so a
*new* site fails a test even while the known ones are still being fixed. This is added to
the commit sequence as commit 6b and is the one genuinely new design element in Delta 6.

### D6.3 Corrected file:line map (supersedes the touch lists' line numbers)

Only two plan-touched files drifted (`git diff --stat 1120180..main -- <plan files>`):
`openrouter_client.rs` (+87 lines) and `ci_guards.rs` (+557, now 656 lines, 5 tests).

| Finding / issue | Plan said | `main@c7ae820` says |
|---|---|---|
| UF-039 / #293 `cache_budget: None` | hybrid_searcher.rs:354-360; mcp lib.rs:1274-1280, :1372-1378, :1485-1491 | `hybrid_searcher.rs:358`; `mcp_server/src/lib.rs:1278`, `:1376`, `:1489` (constructors at `:354`, `:1274`, `:1372`, `:1485` — unchanged) |
| UF-040 / #296 guard sites | same four constructors | unchanged; `reject_hostile_tree` absent |
| UF-001 / #280 writer | openrouter_client.rs:166 (SAFETY), :170, :177, :185, :189 | `openrouter_client.rs:191` (SAFETY), `:195`, `:202`, `:210`, `:214` |
| UF-001 second instance | n/a | `terraphim_grep/src/lib.rs:908` (SAFETY), `:911`, `:920`, `:925`, `:934`, `:945`, `:949` |
| UF-004 / #299 writer | state.rs:131 | `state.rs:131` (unchanged) |
| UF-004 second instance | n/a | `terraphim_update/tests/managed_mode.rs:326`, `:343` |
| UF-013 writer | test_mcp_stdio.rs:15 | unchanged |
| Guard-test host | "append to ci_guards.rs" | `crates/terraphim_agent/tests/ci_guards.rs`, 656 lines, append after `publish_gate_tests_pass` |
| `terraphim_test_utils` dev-dep | mcp :66, agent :112 | mcp_server/Cargo.toml:66 (unchanged), **agent/Cargo.toml:114** (was :112) |

### D6.4 Four supporting files are branch-only; two are prerequisites, two are not

| File | Plan dependency | Disposition |
|---|---|---|
| `deny.toml` | Delta 5 item 4 (advisory-reachability note); commit 7 (ban `terraphim_test_utils`) | **Absent on `main`, and no `cargo deny` step in either workflow.** Porting it is a prerequisite of commit 7's ban clause only. Decision: drop the ban clause from commit 7 and move the whole `cargo deny` gate — config plus workflow step plus the reachability note — to #254/#257 as separate work. Rationale: a UB remediation train should not carry a new supply-chain gate, and the gate adds nothing to the UB fix. |
| `rust-toolchain.toml` | Delta 4 precondition ("pins 1.97.1, so `set_var` is `unsafe fn` everywhere") | **Absent, and not a blocker.** `set_var` is `unsafe fn` because the root manifest sets `edition = "2024"` (`Cargo.toml:19`) and all eleven members inherit it; that is an edition property, not a toolchain property. The headline lever remains a true compile-time gate. The missing pin is a separate CI-reproducibility gap, noted for #254. **This corrects Delta 4's stated reasoning without changing its conclusion.** |
| `.config/nextest.toml` | section 1; 4.2 candidate C | Absent; void on `main` (D6.1 item 3). No action. |
| `.ub-exorcism/` workspace | every artefact, `UB_RUNBOOK.md`, the LD_PRELOAD probe sources | **Prerequisite.** The regression instruments for #288/#291/#300 are only reachable from `task/254-ci-pipeline`. Execution step 0 below. |

### D6.5 Execution order (unchanged from section 10 and Delta 2/3, with the new commits slotted)

| # | Branch / PR | Commit | Issue | Why here |
|---|---|---|---|---|
| 0 | `task/252-audit-record` off `main` | cherry-pick `c8b3599`, `bc3261b`, `9b455e9` (`.ub-exorcism/` only; the path does not exist on `main`, so no conflict) plus Delta 6 appended to `phase8_remediation_plan.md` | #252 | Makes the plan, runbook and probe sources reachable from `main`. Prerequisite for every regression test. |
| 1 | `task/293-fff-hardening` off `main`, **own PR** | commit 0: `fix(grep,mcp_server): refuse fff-search mmap content cache at every FilePicker call site` | #293 | Only production-path fix; must not wait for the test train. Delta 2 winner A, unchanged. |
| 2 | same branch/PR | commit 0b: `fix(grep,mcp_server): refuse trees with non-UTF-8 or over-long entry names before fff-search indexes them` | #296 | Delta 3 winner B, unchanged. Separate commit so either reverts alone. |
| 3 | same branch/PR | `chore(deps): pin fff-search to =0.8.4 and assert the pin in ci_guards` | #293, #257 | Delta 5 requirement 2. Caret `^0.8.4` cannot reach 0.10.6 today, so this is prudence, not urgency — but it is the only thing that keeps the reachability argument from lapsing silently. |
| 4 | `task/252-env-unsafe-remediation` off `main`, **second PR** | commits 1-5 of section 10, unchanged, with D6.3's line numbers | #280, #290, #299, #252 | Test-only; one lever per commit. |
| 5 | same | **commit 5b (new)**: `fix(grep): inject the RLM max-tokens parse; no env mutation in lib tests` | new issue (D6.2 writer A) | Must precede the lever. |
| 6 | same | **commit 5c (new)**: `fix(update): inject the home root in the managed-mode zero-write test` | new issue (D6.2 writer B) | Must precede the lever. |
| 7 | same | commit 6: remove the dead `rust_has_unsafe_env_setters` build script | #252 | unchanged |
| 8 | same | **commit 6b (new)**: `test(workspace): ci_guards arm rejecting new env::set_var sites against an allow-list` | new issue (D6.2 design note) | The interim stop that would have caught both new writers. Lands before the lever so it is useful during the train, not after. |
| 9 | same | commit 7: `chore(workspace): forbid unsafe_code in every crate and target` — **ban clause on `terraphim_test_utils` dropped per D6.4** | #310 | Will not compile until commits 1-6c close all 14 blocks. Last by construction. |
| 10 | same | commits 8, 9: docs; `#[ignore]` the live-network GitHub check | #289, #292, #295, #298, #301, #303, #312, #302 | unchanged |

### D6.6 Test strategy (what proves each change; no mocks, per project policy)

| Change | Proves original | Proves remediation | Runs where |
|---|---|---|---|
| commit 0 (#293) | EXP-104 (done) | EXP-805: EXP-104 harness with the post-fix option block; `cached_count = 0`, no mapped temp files, 0 invalid `line_content` in 200 greps, truncate arm returns `Err`/short rather than SIGBUS | bigbox, manual (#294) |
| commit 0b (#296) | EXP-201 (done) | EXP-806: the seven hostile geometries through `HybridSearcher::search` and the three MCP tools return an error naming the entry; controls search normally; raw fff-search still reproduces EXP-201 | Linux container + `#[cfg(target_os = "linux")]` integration tests (#297) |
| commit 3 (pin) | n/a | `ci_guards` arm asserting `=0.8.4` in both manifests | both CI lanes |
| commits 1-5, 5b, 5c | EXP-001b, EXP-002c, EXP-010, EXP-014 (done) | EXP-801/802/803: post-fix libtest binary under the LD_PRELOAD shim shows `setenv=0 unsetenv=0`, zero overlaps; pre-fix binary is the control | bigbox, manual only — the runner allowlist bars the shim from CI (D6.1 item 2) |
| commit 6b | n/a | the arm fails when a new `env::set_var` is added outside the allow-list; it passes on `main` today only if the allow-list is seeded with all 14 known sites | both CI lanes |
| commit 7 (#310) | n/a | EXP-804: the stored fault-injection patch re-adding one `unsafe { set_var }` fails `cargo check` | both CI lanes |

### D6.7 Eliminated options

| Option rejected | Why rejected | Risk of including | Trace |
|---|---|---|---|
| Re-derive candidates and rubrics for UF-001/002/004/039/040 | No mechanism changed; the audit's evidence is intact | Discards proofs; invites a different, unproven winner | `Gitea: #252` (audit is converged and closed to new rounds) |
| Rebase or merge `task/254-ci-pipeline` to recover the workspace | 19 conflicting files of unrelated CI work | Couples two urgent UB fixes to a stalled CI port | `Gitea: #254` |
| Treat the new writers as new findings needing new experiments | Same mechanism, same binary class, same proven readers | Re-opens a converged audit for no new information | `Gitea: #252` |
| `#[serial]` on the new writers (candidate B shape) | EXP-014 refuted it, and writer B *is* this option already failing | Would ship the refuted remedy as the fix | `Gitea: #290` (EXP-014 recorded there) |
| Carry the `cargo deny` gate inside the UB train | `deny.toml` is absent on `main`; a supply-chain gate is a separate lever | Commit 7 grows a second purpose and a second failure mode | `Gitea: #257` |
| Port `rust-toolchain.toml` as part of this work | The lever does not depend on it (edition, not toolchain) | Scope creep into CI reproducibility | `Gitea: #254` |

**Avoid at all cost** (5/25):
- Upgrading fff-search to 0.10.6 to clear RUSTSEC-2026-0183/0184 — `Gitea: #257`: Delta 5 proved both advisories' APIs unreachable and 0.10.6 reintroduces a *reachable* UB on the grep path. This is the trade the audit most wants not made.
- Using `ContentCacheBudget::zero()` or `max_file_size: 0` as the no-mmap budget — `Gitea: #293`: both also close the slow path, silencing grep entirely. Only `max_files: 0`, `max_bytes: 0`, `max_file_size: 10 * 1024 * 1024` is correct.
- Landing the `forbid(unsafe_code)` lever before the writer commits — `Gitea: #310`: the workspace will not compile, and the failure will look like the lever being wrong.
- Making the LD_PRELOAD probe a CI step — `Gitea: #288`: the runner allowlist rejects non-cargo programs; the lane would fail for a reason unrelated to the finding.
- Any `#[cfg(test)]`-gated `unsafe` left in source as a "temporary" measure — `General:` it defeats the only compile-time gate the plan has.
- Fixing the UTF-8 conversion in fff-search's sink instead of the mapping — `Gitea: #293`: Delta 2 candidate D, recorded precisely so nobody "fixes" it there; it cannot address the SIGBUS arm.

### D6.8 Simplicity check

What if this could be easy? It nearly is: commits 0 and 0b are roughly 12 lines at each of
four call sites plus one 30-line helper per crate, and they close both urgent findings
without touching any test. Everything else in the train is test-only and mechanical. The
only non-obvious judgement in Delta 6 is commit 6b, and it is a 40-line scan in a test file
that already does four such scans. Nothing speculative: no new abstraction, no feature
flag, no trait, and the two new commits reuse the injection pattern commits 1-3 already
establish.

### D6.9 Open items for the user

| Item | Needs |
|---|---|
| Step 0 cherry-pick of the audit workspace onto `main` | Go-ahead |
| Two new Gitea issues for the D6.2 writers (plus a third for commit 6b) | Go-ahead; I can file them |
| Moving `cargo deny` / `deny.toml` / `rust-toolchain.toml` to #254/#257 | Confirmation that is the right home |
| Whether commits 1-10 should be one PR or the two the plan specifies | Default is the plan's two |

### D6.10 Amendments made after review (2026-10-07, same day)

1. **D6.2 writer B provenance corrected.** `47c5d11` is dated 2026-09-11 23:40 BST (the day the audit started) and is **not** on `main`. The site reached `main` via `e814910` ("ci: semantic GitHub v1.21.16 release port (#26)") on 2026-09-25, through the lineage divergence tracked by #342. Accurate claim: two sites exist on `main` that the audit never saw -- one authored after convergence (#373, 2026-10-04), one authored concurrently with the audit and imported later by a release port (#374). The plan consequence is unchanged.
2. **D6.2 writer B reader corrected.** Not sqlite: `cargo tree -p terraphim_update` shows neither `rusqlite` nor `libsqlite3-sys`. The reader is the glibc resolver via `self_update`/`reqwest`, the EXP-010-a chain.
3. **D6.4 amended.** Moving the whole `cargo deny` gate to #254/#257 would have dropped the `terraphim_test_utils` ban, which was the only stop on re-entry through the *safe* wrapper (`EnvVarGuard`/`set_env_var`, the UF-003/UF-014 shape). `forbid(unsafe_code)` is structurally blind to a safe fn wrapping `unsafe`. The ban's function moves to commit 6b / #375, which now also matches the wrapper forms (seed list 14 direct + 11 wrapper uses) and asserts no member manifest lists `terraphim_test_utils`. The `cargo deny` gate itself still goes to #254/#257.
4. Issues filed: #373 (high), #374 (high), #375 (medium); all three added as blockers of #310 (direction verified: #310 `dependencies` = [280, 290, 299, 311, 312, 373, 374, 375], `blocks` = []).
5. Delta 6 and this amendment are recorded on #252 as comments 95871 and 95913.
