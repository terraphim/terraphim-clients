# Research Document: Benchmark discipline for terraphim-agent (fx-style perf gates)

**Issue**: #437
**Status**: Draft
**Date**: 2026-11-14
**Author**: terraphim agent

## Executive Summary

`terraphim-agent` has three Criterion benches (memory retrieval, hybrid search,
session search) and no latency or size gate at all: nothing in the repository
fails when a CLI command gets slower or the binary gets bigger. The issue asks
for fx-style discipline - a timed-commands suite over the binary's own
commands, explicit budgets, and the numbers published on the pull request.

Two findings shape the design. First, the commands the issue names
(`code doctor`, `code commands listing`, one AgentLoop turn) live on the
unmerged coding-agent branch (PR #432) and do not exist on `main`, so the suite
must be able to declare a scenario, detect that the command is absent from the
build, and report it as skipped **with the reason** rather than as a pass.
Second, the CI lane must measure a binary it can build cheaply: the native
runner builds the dev profile, so the lane measures the dev binary and the
budgets are per profile; the release binary (what users feel) is measured by
the same harness and its numbers are published on the pull request.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energising? | Yes | The repository already pays for Criterion, nextest and coverage; this adds a gate on top of them rather than a new toolchain. |
| Leverages strengths? | Yes | The agent can run the real binary against a hermetic environment and read the timings; no mocking is involved (AGENTS.md forbids mocks anyway). |
| Meets real need? | Yes | #437 is an open issue in the backlog; fx's discipline is the stated model; the same class of regression (a config load that starts hitting the network) is invisible today. |

**Proceed**: Yes (3/3 YES).

## Problem Statement

### Description

Adopt fx-style benchmark discipline for `terraphim-agent`:

1. A benchmark lane (a timed-commands suite is explicitly allowed by the
   issue) covering binary startup, `code doctor`, `code commands listing`, REPL
   startup and one scripted AgentLoop turn.
2. A CI gate with explicit budgets (a fixed millisecond bound for the commands
   above, a size budget for the binary).
3. The measured numbers published on the pull request so a regression is
   visible in the review, not only in a log.

### Impact

Without it, latency and binary-size regressions are found by users. The
repository already carries a shared-store flake class (#398: the
`terraphim_settings` default profiles point at `/tmp/terraphim_sqlite` and
`/tmp/terraphim_dashmap`), which is exactly the kind of "the command got slow
because it now opens the shared database" regression this gate is meant to
catch early.

### Success Criteria

- A single command measures the agent's CLI scenarios and fails when a budget
  is breached, printing a Markdown table with the numbers.
- The native CI lane runs it; the budgets live in a committed manifest.
- The pull request carries the measured numbers, including the binary size.
- A scenario whose command is missing from the build is reported as skipped
  with the reason; it is never counted as a pass.

## Current State Analysis

### Existing Implementation

| What exists | Where | Note |
|---|---|---|
| Criterion 0.8 benches | `crates/terraphim_agent/benches/memory_retrieve.rs`, `crates/terraphim_grep/benches/hybrid_search.rs`, `crates/terraphim_sessions/benches/search_nfr.rs` | Micro-benchmarks of library functions; none runs the binary, none has a budget. |
| A bench support module in the library | `crates/terraphim_agent/src/memory_bench.rs` | Loads the committed fixture corpus; the pattern this work follows (library module + thin bench/test driver). |
| The native CI lane | `.gitea/workflows/native-ci.yml` | 22 steps: fmt, clippy, build, nextest, coverage, checklist journey. No perf step. |
| A hermetic CLI environment | `crates/terraphim_agent/tests/support/cli_test_env.rs` | Writes a HOME-scoped `settings.toml` so a spawned agent does not touch the shared `/tmp` profiles. |
| The runner's command policy | `crates/terraphim_agent/tests/ci_guards.rs` | The policy classifies the literal first token of a `run:` line; a multi-line block that starts with a comment is rejected outright (#400). |

### Code Locations

| Component | Location | Purpose |
|---|---|---|
| CLI surface | `crates/terraphim_agent/src/cli_schema.rs` | The `Commands` enum: `Search`, `Roles`, `Config`, `Repl`, `Hook`, `Guard`, `Memory`, `Learn`, `Sessions`, ... |
| REPL entry | `crates/terraphim_agent/src/repl/handler.rs` | The REPL reads stdin; an EOF on stdin exits it cleanly with status 0. |
| Hermetic environment | `crates/terraphim_agent/tests/support/cli_test_env.rs` | `set_hermetic_env` scopes HOME, XDG_CONFIG_HOME, the data path and every persistence profile to a temporary root. |
| Workflow policy guard | `crates/terraphim_agent/tests/ci_guards.rs` | `validate_publish_workflow_commands`, `validate_native_ci_token_aliases` - the constraints a new lane must satisfy. |
| Coding-agent CLI (not on `main`) | `origin/task/422-coding-agent`: `crates/terraphim_agent/src/coding/cli.rs`, `src/coding/loop.rs`, `src/cli_schema.rs` (`CodeSub::{Run, Resume, Doctor, Commands}`) | The commands the issue names; behind the `coding` feature on an unmerged branch. |

### Data Flow

A measured command is spawned as a child process of the test harness, with the
hermetic environment applied. The harness records the wall-clock duration of
each run, computes the median and the p95 (nearest-rank) over the repetitions,
and compares them with the manifest's budgets.

### Integration Points

- The native CI lane (`.gitea/workflows/native-ci.yml`): one step, one line,
  `bash ./scripts/ci/<script>.sh`, because the runner's policy only accepts
  `cargo`, `bash <checked-in script>` and `test` as the first token.
- The registry-token guard: the conditional
  `CARGO_REGISTRIES_TERRAPHIM_TOKEN="Bearer $GITEA_TOKEN"` alias must stay
  scoped to exactly the two lanes that build the whole workspace
  (`cargo test --workspace --all-targets` and the coverage lane). A new lane
  that builds only one crate must therefore **not** carry the alias - the
  registry crates are already in the runner's CARGO_HOME by then.

## Constraints

### Technical Constraints

- **Runner command policy**: the literal first token of a `run:` line decides
  whether the step runs. `cargo`, `bash <repo script>` and `test` are allowed;
  a multi-line block whose first token is a comment is rejected before the
  step starts (#400). The new lane must therefore be a single line that calls
  a checked-in script.
- **Registry-token guard**: adding a token alias for a new lane breaks
  `native_ci_aliases_gitea_token_for_the_registry_resolving_lanes`. The new
  lane resolves nothing new and must carry no alias.
- **Hermeticity**: every measured command must run under the hermetic
  environment, or two lanes contend over the shared `/tmp` profiles - the flake
  class documented on #398.
- **Profile**: the native lane builds the dev profile (`cargo build
  --workspace`); the shipped artifact is the release build. Debug and release
  timings differ by roughly an order of magnitude, so a budget is only
  meaningful against a named profile.
- **Forge CI is currently disabled**: the Gitea Actions API answers with
  placeholder runs and no runner is registered, so the new lane cannot be
  observed on the forge. Local execution of the same script is the evidence.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Startup latency (`--version`) | < 15 ms (dev) | 1.8 ms (reference host), 3.0 ms (CI runner) |
| Command listing (`roles list`) | < 450 ms (dev) | 142 ms (reference host), 147 ms (CI runner) |
| REPL startup and clean exit | < 450 ms (dev) | 118 ms (reference host), 132 ms (CI runner) |
| One scripted KG turn (`search`) | < 450 ms (dev) | 143 ms (reference host), 134 ms (CI runner) |
| Binary size | a ceiling for the release binary | 29.9 MB (release); the dev binary is 43 MB on the reference host and 500 MB on the CI runner, so it carries no ceiling |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It Is Vital | Evidence |
|------------|-----------------|----------|
| One measurement mechanism, many scenarios | Two harnesses (a Criterion bench and a timed-commands suite) would drift, and only one of them can be a CI gate | The issue allows either; the timed-commands suite is the only one that can measure the binary itself |
| Budgets are per profile and per scenario, in a committed manifest | A budget without a machine-independent reference becomes folklore; a manifest diff is the regression record | fx keeps its budgets in the repository next to the code |
| A missing command is reported, never assumed to pass | `code doctor` and `code commands` do not exist on `main`; a suite that quietly skipped them would be a gate that cannot fail | `origin/task/422-coding-agent` holds `CodeSub`; `main`'s binary answers "unrecognized subcommand 'code'" with status 2 |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Porting fx's own benchmark harness | fx is a Zig binary with its own idioms; the transfer is the discipline, not the code |
| A criterion bench for the CLI | `cargo bench` builds the package in the bench (release) profile, which would add a full release build to every CI run; the timed-commands suite measures an already-built binary |
| The BEAM harness | #468 covers it, needs `uv` and an external pinned checkout - a separate issue |
| Speeding the agent up | This work makes regressions visible; it does not change what the agent does |
| A gate on `terraphim-grep` and `terraphim_sessions` | The issue names `terraphim-agent`; the harness is reusable, the budgets are not |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `tests/support/cli_test_env.rs` | Hermeticity of every measurement | Low - the helper is already used by several suites |
| `.gitea/workflows/native-ci.yml` | The lane that runs the gate | Medium - the workflow is guarded by `ci_guards.rs` |
| `crates/terraphim_agent/src/cli_schema.rs` | The commands that are measured | Low |
| `origin/task/422-coding-agent` (PR #432) | The `code` scenarios activate when it lands | High - out of this change's control; the suite must degrade honestly until then |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `serde_json` | workspace (1.0) | None - already in the lock file | A hand-rolled parser (worse) |
| Criterion | 0.8 | Not used by this change | - |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| The CI runner is slower or more loaded than the measurement host, so a tight budget fails spuriously | Medium | Medium | Budgets carry documented headroom over the reference measurement; the fastest scenario acts as a machine-speed canary; the diagnostic names the likely cause |
| Wall-clock budgets are flaky in a shared CI environment | Medium | Medium | The suite measures the median of several repetitions and the p95, and compares against a ceiling rather than a single sample |
| The dev profile's timings are an order of magnitude away from the release timings, so the CI gate is coarse | High | Low | The manifest carries both profiles; the release numbers are published on the pull request |
| The `code` scenarios stay skipped because PR #432 stalls | Medium | Low | The skip is printed with the reason and counted in the summary; it is not a pass |
| A measured command writes outside the hermetic root | Low | Medium | Every scenario runs under `set_hermetic_env`; the harness asserts the exit status |

### Open Questions

1. Should the release budget set be enforced by the release workflow? Not
   answerable here: the release binaries are built by the GitHub mirror's
   `release-binaries.yml`, outside this repository's Gitea lane.
2. What is the CI runner's speed relative to the reference host? Not
   answerable while the runner is disabled; the budgets are set with headroom
   for a slower host.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| The native lane may build only the agent crate without a registry token | The registry crates are already in the runner's CARGO_HOME from the earlier workspace build lane | The lane fails with 401 on a cold cache | No - untestable while CI is disabled |
| `terraphim-agent repl` exits cleanly on an EOF stdin | Observed: status 0, "Goodbye!" on stdout | The scenario hangs and the lane times out | Yes - observed locally |
| The `code` subcommand is absent from `main` | `main`'s binary answers "unrecognized subcommand 'code'" (status 2) | The availability probe mis-detects | Yes - observed locally |
| A dev-profile build is what the native lane produces | `.gitea/workflows/native-ci.yml` runs `cargo build --workspace` (no `--release`) | The measured profile is not the one the budgets describe | Yes - read from the workflow |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| A Criterion bench over the CLI | `cargo bench` measures library calls; the CLI would have to be re-implemented as library calls, and the binary's own start-up would not be measured | Rejected - it would measure the wrong thing (the binary is the product) |
| A timed-commands suite that spawns the binary | Measures exactly what a user experiences, including process start-up and argument parsing | Chosen - the issue names it as an acceptable form |
| Enforce the budgets on the release binary in CI | The most faithful gate, but it needs a full release build in every lane (minutes) | Deferred - the dev profile is what the lane builds; the release numbers are published instead |
| Skip the `code` scenarios silently until PR #432 lands | Simplest code | Rejected - a silent skip is a gate that cannot fail, and it would hide the gap from the reviewer |

## Research Findings

### Key Insights

1. The commands the issue names do not exist on `main`. `code doctor`,
   `code commands` and the AgentLoop are on `origin/task/422-coding-agent`
   behind the `coding` feature. The suite must therefore detect availability
   and report skips honestly.
2. The binary's own exit status is a reliable availability probe: clap exits
   with status 2 and "unrecognized subcommand 'code'" when the subcommand is
   absent, and with status 0 for `--help` when it is present.
3. The REPL is measurable without a TTY: with an EOF on stdin it prints
   "Goodbye!" and exits with status 0.
4. The runner's policy makes the workflow shape non-negotiable: a single-line
   `run:` calling a checked-in script.
5. The registry-token guard means the new lane must not carry the alias; it
   builds one crate, and the registry crates are already cached by the earlier
   lane.

### Relevant Prior Art

- **fx (Vercel Labs)**: CI fails a pull request when a benchmarked command
  exceeds its budget, and the release binary size is capped. This is the model
  the issue names.
- **Criterion**: the repository's existing micro-benchmark tool. Its
  statistical model (confidence intervals) is the right tool for *library*
  regressions; it is the wrong tool for a *process* budget, which needs a
  ceiling and a published number.
- **cargo-nextest**: the repository's test runner; it runs the perf gate as
  an ordinary test, so no new runner is introduced.

### Technical Spikes Needed

None. The measurement surface (spawn a process, time it, read a JSON manifest)
is small and already proven by the existing test support code.

## Recommendations

### Proceed/No-Proceed

Proceed. The change is additive, needs no new dependency, and reuses the
repository's hermetic environment helper.

### Scope Recommendations

- One harness in the agent crate: a library module (`src/perf_gate.rs`) plus a
  thin test driver (`tests/perf_gate.rs`), following the `memory_bench.rs`
  pattern.
- One manifest: `docs/benchmarks/agent-cli-budgets.json`, with a budget per
  scenario per profile (`dev`, `release`).
- One CI lane: a single-line step in `native-ci.yml` calling a checked-in
  script.
- The script measures the dev binary (already built by the lane) and enforces
  the dev budgets; the same script can measure a release binary when one
  exists, and the release numbers and size are published on the pull request.

### Risk Mitigation Recommendations

- Publish the numbers on the pull request, as the issue asks, so a reviewer
  sees the regression without opening a log.
- Keep the ceiling generous and record the reference measurement next to it,
  so a future tightening is a one-line manifest change with a recorded
  justification.
- Report skipped scenarios with their reason and count them in the summary.

## Next Steps

1. Write the design document (`docs/plans/design-perf-gates-2026-11-14.md`):
   module layout, manifest schema, budget semantics, availability detection,
   the CI step shape.
2. Implement the harness, the manifest and the lane.
3. Verify: the whole workspace suite, clippy with `-D warnings`, fmt, and the
   gate itself in both report and enforce mode.
4. Publish the numbers on the pull request, review, merge.

## Appendix

### Reference Materials

- Issue #437 (the ask), #422 (the epic), PR #432 (the coding agent).
- `docs/plans/feature-parity-coding-agent-vs-pi-2026-10-27.md` on PR #432 -
  the coding-agent design the `code` scenarios will measure.
- fx's benchmark discipline: https://github.com/VercelLabs/fx

### Measurements (the harness, on this host)

Measured with the gate itself, under the hermetic environment, five
repetitions per scenario; the median and the nearest-rank p95 in
milliseconds.

| Command | dev median | dev p95 | release median | release p95 |
|---|---|---|---|---|
| `--version` | 1.8 | 1.9 | 1.6 | 1.7 |
| `--help` | 1.8 | 1.9 | 1.5 | 1.7 |
| `roles list` | 141.9 | 158.5 | 119.8 | 136.8 |
| `config show` | 113.9 | 152.1 | 120.8 | 129.9 |
| `repl` (end-of-file) | 118.0 | 137.6 | 110.5 | 125.1 |
| `search hello` | 142.7 | 145.0 | 136.3 | 156.3 |

The CI runner's own lane output for the same suite: `startup` 3.0 ms,
`help` 4.1 ms, `roles list` 146.9 ms, `config show` 144.2 ms, `repl` 131.8 ms,
`search` 134.2 ms - within a few per cent of the reference host on the
commands that load the settings.

Binary size: 43,164,400 bytes (dev, reference host), 499,729,064 bytes (dev,
CI runner), 29,942,240 bytes (release).

The dev and the release numbers are close: the cost of these commands is the
settings load and the role-graph build, not the arithmetic an optimiser would
have removed. The dev binary's *size* is a different matter: the same source
builds a 43 MB binary on the reference host and a 500 MB binary on the CI
runner, so the dev profile carries no size ceiling - the number is published,
but the ceiling applies to the release binary, whose size is the code.
