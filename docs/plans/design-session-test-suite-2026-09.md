# Implementation Plan: cass-Parity Session-Search Test Suite (Wave 1)

**Status**: Draft → Review
**Canonical Path**: `docs/plans/design-session-test-suite-2026-09.md`
**Change Slug**: `session-test-suite-2026-09`
**Research**: `docs/plans/research-session-test-parity-2026-09.md` (merged via #148)
**Author**: Kokoro (agent) for Alex
**Date**: 2026-09-03
**Estimated Effort**: 3–4 working days (5 PRs, sequenced)

---

## Overview

### Summary

Implement the first implementation wave of the cass-parity test plan for `terraphim_sessions` + `terraphim_agent` (both in this repo). Five sequenced PRs: (1) parity harness + search-index feature lane, (2) hybrid KG-boost search suite (P0), (3) per-connector import/hermetic-suite tests, (4) REPL/CLI sessions contract tests incl. exit-4 + flag-order, (5) DOCS-DRIFT resolution + NFR bench wiring. Closes the two CI blind spots that make parity claims unverifiable and converts the plan's traceability rows into executable tests.

### Approach

Test-first, per PR, using the existing hermetic CLI test scaffolding (`tests/support/cli_test_env.rs`, `CARGO_BIN_EXE_terraphim-agent`). All new tests live in-crate (`#[cfg(test)]`) or in `crates/terraphim_agent/tests/`, feature-gated exactly as the code under test is gated. Each PR is independently mergeable and maps 1:1 to Gitea issues created in the split (see `docs/plans/issue-split-session-test-suite-2026-09.md`).

### Scope

**In Scope (vital few):**
1. `search_with_thesaurus` / `search_sessions_hybrid` KG-boost ordering suite (plan Ch5 TC-SEARCH-01…26, P0 rows first)
2. Service-level import contracts: `import_all` skip-failures, global limit, auto-import single-attempt, `since/until/limit` (TC-IMPORT-*)
3. Per-connector hermetic import tests for aider discovery, cline, opencode legacy + SQLite, cursor (TC-SOURCES/IMPORT coverage rows)
4. REPL/CLI sessions contract: exit-4-on-empty payload, `--robot/--format` root-flag order, JSON shapes, membership connector set (TC-ROBOT-CLI-01…08 subset)
5. DOCS-DRIFT probes: `CLAUDE_SESSIONS_DIR` no-op probe + doc decision, `supported_formats` advertisement probe (TC-DOCS-DRIFT-01/02/05)
6. CI: add `--all-features` lane + wire `search_nfr` bench (extends #3014 work into this repo)

**Out of Scope (this wave):**
- Any MISSING capability implementation (GAP rows stay deferred: cursor pagination, aggregations, `--explain`, ANN, pack, analytics, resume, doctor/health)
- cass itself, skill-doc rewrites beyond the three drift decisions above
- TUI/scripting surfaces, self-upgrade, completions (N-A)

**Avoid At All Cost (5/25 rule):**
- No Tantivy/persistent-index revival (spec line 360 deprecation is settled)
- No embeddings/vector search (KG thesaurus is the design alternative)
- No exact-set connector/capability asserts (membership only — dev-dep feature unification)
- No bare exit-code-only assertions (payload+behavior pairing is mandatory)
- No test that reads real `~/.claude`, `~/.cursor`, or any user session store

### Reality Adjustments vs the Research Artefact

The research artefact was written against the polyrepo snapshot; five facts changed or sharpened on `terraphim-clients@main` (verified 2026-09-03, commit `5d62274`):

| # | Research assumption | Verified reality | Consequence |
|---|-----|-----|-----|
| 1 | Agent builds `terraphim_sessions` from registry 1.20.4; local crate 1.21.3 unreferenced (decision R1) | In this repo both crates are workspace-local; `terraphim_agent` uses `path = "../terraphim_sessions"` (1.21.2) | R1's registry-vs-local question **dissolves**. Canary lane unnecessary. Tests target the same tree CI builds. |
| 2 | Cursor connector missing/incomplete; "no cursor-connector feature" | `connector/cursor.rs` exists with `import()` + 15 tests incl. `import_with_limit`, v1/v2 parse, CJK/emoji title truncation | Cursor parity rows upgrade: parse coverage EXISTS; what remains is REPL/CLI exposure + hermetic-env coverage. |
| 3 | Aider discovery unbounded | `MAX_DETECT_DEPTH=6`, `follow_links(false)`, hit cap 64 (fix #123, `33e0f13`) | Detection-bounding is tested in-repo; our wave adds only the CWD-scoped import test. |
| 4 | CI runs `--lib` only (agent integration tests never run) | `.gitea/workflows/native-ci.yml` already runs `--workspace --all-targets` + server-bin env (#91 family, merged); `.github/workflows/ci.yml` still `--lib` | Remaining CI gap is narrower: add `--all-features` lane (search-index, cursor, codex, extras) to native-ci.yml; align gh ci.yml. |
| 5 | `search_nfr` bench missing | Exists in `terraphim-ai@8fb947863` (refs #3014) but was **not carried into this repo's** `terraphim_sessions/benches/`; no criterion dev-dep | PR-5 ports the bench + wires a scheduled CI job. |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|---|---|---|
| Test the registry 1.20.4 build (R1 as written) | Path dep makes local = production in this repo | Testing a version nobody ships |
| Port all 158 TCs in one wave | 5/25 rule; P0/P1 rows carry the parity signal | Review paralysis, merge debt |
| Golden-file snapshots of full robot JSON | Schema drift churn; membership+shape asserts suffice | Brittle CI |
| Windows-path fixtures | CI is Linux; macOS covered by HOME-only rule + dev runs | Maintenance burden |

### Simplicity Check

**What if this could be easy?** It is: every P0 test is a pure-Rust unit/integration test against existing public APIs (`search_with_thesaurus`, `SessionService`, connector `import()`), using the existing hermetic-env support. No new production code is required by this wave except one CLI fix (respect `--fail-on-empty`/exit-4 contract *optionally* — see Open Items) and CI yaml edits. **Senior-engineer test:** passes — no new abstractions, one shared fixture module, no speculation.

**Nothing speculative:** no features not in the plan, no "just in case" traits, no error handling for impossible states, no premature optimization.

---

## Architecture

### Component / Data Flow

```
[fixtures/mod.rs]  synthetic .jsonl/.md/.vscdb corpora under tempdir HOME
        │
        ▼
[terraphim_sessions]                [terraphim_agent]
  service.rs (auto-import)            main.rs CLI (--robot/--format ROOT flags)
  search.rs (BM25 + hybrid boost)     repl/handler.rs (14 /sessions subcommands)
  connector/*.rs (6 connectors)       robot/exit_codes.rs (0–7)
        │                                   │
        └───────── tests ───────────────────┘
   in-crate #[cfg(test)] (Lane A)   crates/terraphim_agent/tests/ (Lane B, CARGO_BIN_EXE)
```

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|---|---|---|
| One shared `fixtures` module per crate, not a fixtures crate | Two consumers (sessions, agent tests); a shared crate adds workspace-wide coupling for 2 users | Workspace-level `test-utils` crate (over-engineering for now) |
| Boost-direction asserts, never score-equality asserts | Fusion math is implementation-specific; ordering is the contract (plan Ch5 rule) | Numeric golden scores (brittle) |
| Hermetic HOME via existing `create_hermetic_root()` + `set_hermetic_env()` | Pattern proven by #143/#144; zero new infra | Reimplementing isolation per-test |
| Fixture corpora generated in Rust (deterministic builders), not committed blobs | Reviewable, parameterizable, no binary blobs in git; matches `search_nfr` corpus style | Committed JSONL fixtures (drift, size) |
| `--all-features` CI lane added to native-ci.yml (not per-feature matrix) | One lane covers search-index/cursor/codex/extras; matrix cost unjustified for one crate | Full feature matrix |
| Bench assertion as `#[test]` with relaxed budget (p50 < 100ms on 10K) + criterion bench | Closes #3014 AC in-repo; nightly job avoids runner contention | CI-per-PR bench (flaky, slow) |

## Expected Lifecycle Artefacts

| Artefact | Path | Required? |
|---|---|---|
| Research | `docs/plans/research-session-test-parity-2026-09.md` | Done (#148) |
| Design | `docs/plans/design-session-test-suite-2026-09.md` (this doc) | Yes |
| Issue split | `docs/plans/issue-split-session-test-suite-2026-09.md` | Yes (this wave) |
| Verification | `docs/verification/verification-report-session-test-suite-2026-09.md` | Yes (per PR-5 close) |
| Traceability | `docs/verification/traceability-matrix-session-test-suite-2026-09.md` | Yes (updated CSV → in-repo copy) |
| Validation | n/a (test-only wave; no user-visible behavior change except optional CLI fix) | No |

## File Changes

### New Files
| File | Purpose |
|------|---------|
| `crates/terraphim_sessions/src/search_tests_support.rs` (cfg(test)) or `src/fixtures/mod.rs` | Deterministic `Session`/`Thesaurus` builders shared by search suites |
| `crates/terraphim_agent/tests/sessions_cli_contract.rs` | Lane B: CLI/robot sessions contract (exit-4, flag order, JSON shape, membership) |
| `crates/terraphim_agent/tests/sessions_docs_drift.rs` | DOCS-DRIFT probes (CLAUDE_SESSIONS_DIR, supported_formats) |
| `crates/terraphim_sessions/benches/search_nfr.rs` | Ported 10K-session NFR bench (+ regression `#[test]`) |
| `docs/plans/issue-split-session-test-suite-2026-09.md` | Issue list → Gitea numbers cross-ref |
| `docs/verification/traceability-matrix-session-test-suite-2026-09.md` | Updated matrix (in-repo CSV + checked rows) |

### Modified Files
| File | Changes |
|------|---------|
| `crates/terraphim_sessions/src/search.rs` | Add `#[cfg(test)]` hybrid-boost suite (TC-SEARCH-01…26 subset; P0 first) |
| `crates/terraphim_sessions/src/service.rs` | Add import-contract tests (import_all skip/limit, auto-import single-attempt, since/until) |
| `crates/terraphim_sessions/src/connector/{aider,cline,opencode,cursor}.rs` | Hermetic import tests (tempdir corpora via `options.path`) |
| `crates/terraphim_agent/tests/support/cli_test_env.rs` | Extend hermetic env with per-connector fixture dirs (`$HOME/.claude/projects`, `.codex/sessions`, `.config/Cursor/User`, `Library/Application Support/Cursor/User` platform-mirrored) |
| `.gitea/workflows/native-ci.yml` | Add `cargo test -p terraphim_sessions --all-features` lane; add nightly bench job |
| `.github/workflows/ci.yml` | Align: add `--all-features` sessions lane (keep gh lane thin) |
| `crates/terraphim_sessions/Cargo.toml` | Add `criterion` dev-dep + `[[bench]]` |
| `docs/skills`…/session-search/SKILL.md (this repo's copy if present) | Per DOCS-DRIFT decisions from PR-5 |

### Deleted Files
None.

## API Design

No new public APIs. Tests consume existing surfaces:

```rust
// search.rs (enrichment feature)
pub fn search_sessions(sessions: &[Session], query: &str) -> Vec<Scored<Session>>;
pub fn search_sessions_hybrid(sessions: &[Session], query: &str, thesaurus: Option<Thesaurus>) -> Vec<Scored<Session>>;

// service.rs
pub async fn search_with_thesaurus(&self, query: &str, thesaurus: Option<Thesaurus>) -> Vec<Session>;
pub async fn import_all(&self, options: &ImportOptions) -> Result<Vec<Session>>;
pub async fn import_from(&self, connector_id: &str, options: &ImportOptions) -> Result<Vec<Session>>;

// enrichment/enricher.rs
pub fn find_related_sessions<'a>(session_id: &str, concepts_map: &'a HashMap<String, SessionConcepts>, min_shared: usize) -> Vec<(&'a str, usize, Vec<String>)>;
```

Fixture builder signatures (test-only):

```rust
pub fn make_enriched_session(id: &str, title: &str, msgs: usize, concepts: &[(&str, u64)]) -> Session;
pub fn make_thesaurus(terms: &[(&str, u64)]) -> Thesaurus;  // NormalizedTermValue→NormalizedTerm
pub fn write_claude_jsonl(dir: &Path, name: &str, lines: &[serde_json::Value]) -> PathBuf;
pub fn write_aider_history(dir: &Path, turns: &[(&str, &str)]) -> PathBuf;
```

### Error Types
No new errors. Tests assert on existing `anyhow`/connector errors.

## Test Strategy

### Unit / in-crate (Lane A — `cargo nextest run -p terraphim_sessions`)
| Suite | Feature gate | Covers |
|---|---|---|
| `search::tests::hybrid_*` (12 tests) | `enrichment` (+`search-index` where scorer used) | TC-SEARCH-01..12: boost ordering, monotonicity, None-degrade, empty-query/corpus, MAX_SEARCH_RESULTS=50, MIN_SCORE_FRACTION cutoff, 50k body cap, multibyte truncation, dedup, deterministic order |
| `service::tests::import_*` (8 tests) | default + `aider-connector` | TC-IMPORT: import_all skip-failure continues, global limit truncates, auto-import single-attempt (attempt counter), since/until/limit honored, clear/clone reset semantics |
| `connector::{aider,cline,opencode,cursor}::tests` (10 tests) | per-connector features | hermetic tempdir import: aider history md, cline taskHistory JSON, opencode legacy jsonl + sqlite (via `options.path`), cursor v1/v2 + limit |

### Integration (Lane B — `cargo test -p terraphim_agent --test sessions_cli_contract`)
| Test | Asserts |
|---|---|
| `sessions_sources_membership` | `--robot sessions sources` JSON: `session_search:true` capability; compiled connector set ⊇ {claude-code-native}; each entry has status+estimate; no panic on empty HOME |
| `sessions_search_exit4_payload` | machine mode + empty corpus → exit 4 AND JSON payload `{"total":0,"shown":0,...}` printed (payload+exit pairing rule) |
| `sessions_search_flag_order` | `--robot` before subcommand parses; after subcommand rejected with exit 2 + usage text |
| `sessions_search_json_shape` | fields `query,total,shown,sessions[].{id,title,message_count,preview}`; preview ≤100 chars; human mode unchanged |
| `sessions_stats_json` | `total_sessions,total_messages,total_user_messages,total_assistant_messages,by_source` present; `by_agent/top_workspaces/date_range/raw_mirror` absent |
| `sessions_export_roundtrip` | `/sessions export --format json -o <tmp>` → serde round-trip to `Vec<Session>`; unknown format rejected |
| `docs_drift_claude_sessions_dir` | `CLAUDE_SESSIONS_DIR=<tmp>` → `sessions sources` output **unchanged** (documented-but-unimplemented) |
| `docs_drift_output_formats` | robot `capabilities` advertised formats ⊇ actually-accepted enum {human,json,json-compact}; advertise-only formats listed as drift finding |

### Property/regression
- Reuse existing suites: native #814/#815 watcher regressions (in-crate), cursor v1/v2, cluster family — referenced, not duplicated.
- `#[test] search_nfr_10k_p50_under_100ms` (release-profile, `#[ignore]` on debug builds) + criterion bench in `benches/`.

### CI
- native-ci.yml: `cargo test -p terraphim_sessions --all-features --no-fail-fast` (new lane after enrichment lane)
- native-ci.yml nightly (schedule): `cargo bench -p terraphim_sessions --features enrichment,search-index`
- gh ci.yml: add same `--all-features` test line for parity

## Implementation Steps

### PR-1 — Harness + search-index feature lane (issue #1)
**Files:** `search_tests_support.rs` (new), `native-ci.yml`, `.github/workflows/ci.yml`
**Tests:** support builders unit-tested (thesaurus builder round-trip, session builder defaults)
**Estimated:** 0.5 day

### PR-2 — Hybrid KG-boost suite, P0 (issue #2)
**Files:** `search.rs` tests, uses PR-1 support
**Tests:** TC-SEARCH-01/02/03/06 (boost ordering, monotone, None-degrade, empty-query) + P1s (05/07/08/09/10/11/12)
**Deps:** PR-1. **Estimated:** 1 day

### PR-3 — Import contracts + connector hermetic suites (issue #3)
**Files:** `service.rs` tests, connector test additions, `cli_test_env.rs` extension
**Tests:** TC-IMPORT subset (8) + connector import tests (10)
**Deps:** PR-1. **Estimated:** 1 day

### PR-4 — REPL/CLI contract tests (issue #4)
**Files:** `tests/sessions_cli_contract.rs` (new)
**Tests:** 8 integration tests above
**Deps:** PR-3 (fixture env). **Estimated:** 1 day

### PR-5 — DOCS-DRIFT + NFR bench wiring (issue #5)
**Files:** `tests/sessions_docs_drift.rs`, `benches/search_nfr.rs` port, Cargo.toml, CI nightly job, doc decisions
**Deps:** PR-4. **Estimated:** 0.5–1 day

### Rollback Plan
Each PR is test-only (or CI-yaml) — revert the merge commit; no data migrations, no flags.

## Open Items

| Item | Status | Owner |
|---|---|---|
| Exit-4: sessions search exits 4 on empty *unconditionally* in machine mode; global `--fail-on-empty` not honored here | Decide: honor flag (tiny prod change) or pin current behavior in tests | Alex |
| DOCS-DRIFT outcomes (fix docs vs implement env var) | Decided in PR-5 based on probe results | Alex |
| `search.rs` tests are NOT feature-gated today (compile under default features) — keep BM25 tests ungated, gate only `enrichment` hybrid tests | Confirmed intentional (search-index feature adds the score module only) | settled |

## Approval

- [ ] Alex approves design + issue split (this PR)
- [ ] Gate: `cargo clippy --workspace --all-targets -- -D warnings` clean on each PR
- [ ] Gate: no test reads real user session stores (reviewer checklist)
