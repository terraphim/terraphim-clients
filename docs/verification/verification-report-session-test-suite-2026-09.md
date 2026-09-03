# Verification Report: session-test-suite-2026-09 (Wave 1)

**Date**: 2026-09-03 · **Repo**: terraphim-clients · **Slug**: session-test-suite-2026-09

## Scope

Wave-1 of the cass-parity session-search test suite (design: docs/plans/design-session-test-suite-2026-09.md; research: docs/plans/research-session-test-parity-2026-09.md; workstream terraphim/terraphim-ai#3084).

## Deliverables Merged (5/5)

| PR | Issue | What landed | Local verification |
|---|---|---|---|
| #156 | #150 | Parity harness: `search_tests_support.rs` fixture builders; `--all-features` CI lane (native-ci.yml) | 74 default / 88 enrichment / 122 all-features green; clippy clean |
| #157 | #151 | Hybrid KG-boost suite: 8 tests (`search_sessions_hybrid` had zero before) | 96 enrichment green; clippy clean |
| #158 | #152 | Import contracts: global-limit truncation, auto-import single-attempt; cline + aider hermetic import suites | 105 (partial features) / 135 all-features green |
| #159 | #153 | REPL/CLI contract: 7 integration tests (exit-4 payload, flag order, JSON shapes) | 7/7 green |
| #160 | #154 | DOCS-DRIFT probes (3) + NFR bench port (criterion, 10K corpus) | bench: 89.4ms [88.2-91.4] — G1 <100ms proven; 135 sessions tests green |

## Key Findings

1. **G1 NFR now has executable proof in-repo**: `search_sessions` over 10K sessions ≈ 89ms, within the <100ms spec claim (terraphim-ai#3014 AC closed here).
2. **Hybrid search is now covered**: the flagship KG-boost path (count x 10000 ordering) has 8 ordering/monotonicity/degrade tests.
3. **CLI contract pinned**: machine-mode empty search exits 4 with a zero payload; `--robot` root-flag order enforced (exit 2 on misplacement).
4. **Docs drift documented as defects** (per plan's doc-drift policy):
   - `CLAUDE_SESSIONS_DIR`: documented, unimplemented -> probe asserts no effect; fix = doc decision (issue #154 body).
   - `robot capabilities.supported_formats`: advertises json/jsonl/minimal/table; CLI accepts human/json/json-compact.
   - `/sessions import`: removed; CLI rejects as unrecognized, REPL explains.
5. **CI blind spot closed**: `--all-features` lane now exercises cursor/codex/extras connectors + search-index module (previously invisible).

## Gaps / Follow-ups (Wave 2+)

- Wave-2 backlog from the traceability CSV remains: REPL `/sessions` handler-level tests (concepts/related/timeline/enrich/cluster/files/by-file/index — handler.rs still has 0 direct tests), cursor SQLite hermetic corpus, opencode SQLite import test, service `search_by_concept`/`find_related` tests, native watcher nightly lane.
- GAP-deferred rows (pagination, aggregations, --explain, pack, analytics, resume, doctor/health) remain deferred by design; each has an implementation-ready spec in the research artefact Ch5/Ch6.
- NFR bench not yet wired into CI schedule (native-ci nightly job still to be added — tracked in the #154 PR notes).
