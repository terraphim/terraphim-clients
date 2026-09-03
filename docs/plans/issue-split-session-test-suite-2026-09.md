# Issue Split: session-test-suite-2026-09

Design: `docs/plans/design-session-test-suite-2026-09.md` · Research: `docs/plans/research-session-test-parity-2026-09.md`

| # | Gitea issue | PR | Blocked by |
|---|---|---|---|
| 1 | test(sessions): parity harness — shared fixture builders + `--all-features` CI lane | feat→PR-1 | — |
| 2 | test(sessions): hybrid KG-boost ordering suite (P0) | PR-2 | #1 |
| 3 | test(sessions): import contracts + per-connector hermetic suites | PR-3 | #1 |
| 4 | test(sessions): REPL/CLI contract tests (exit-4 payload, flag order, JSON shapes) | PR-4 | #3 |
| 5 | test(sessions): DOCS-DRIFT probes + NFR bench wiring (ports #3014 into clients) | PR-5 | #4 |

All issues carry: design ref, AC checklists from the plan's TC tables, feature gates, and the guardrail "no test reads real user session stores".
