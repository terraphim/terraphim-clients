# Implementation, Verification and Validation: benchmark discipline for terraphim-agent (#437)

**Issue**: terraphim-clients#437
**Research**: `docs/plans/research-perf-gates-2026-11-14.md`
**Design**: `docs/plans/design-perf-gates-2026-11-14.md`
**Author**: opencode-cli (agent session)
**Date**: 2026-11-14

## What was implemented

A timed-commands performance gate over the agent's own binary, with the
budgets committed next to the code and the numbers published on the pull
request.

### The harness

`crates/terraphim_agent/src/perf_gate.rs` is the measuring half:

- `Manifest` is the committed manifest: the scenarios, their budgets per
  build profile, and the binary size ceiling. `Manifest::parse` refuses a
  manifest that would make the gate meaningless - an unknown version, two
  scenarios with one name, a scenario with no arguments or no repetitions, a
  non-positive budget, a p95 ceiling tighter than the median ceiling.
- `run_scenario` spawns the real binary, times the repetitions, and returns
  the median and the nearest-rank p95. A scenario that names
  `requires_subcommand` is probed with `<binary> <subcommand> --help` first:
  clap answers an unknown subcommand with status 2 and "unrecognized
  subcommand", which is how the harness tells "the command is not in this
  build" from "the command is there and broken". Such a scenario is skipped
  **with its reason**; a scenario that names no subcommand must run, and a
  failing command is an error rather than a slow measurement.
- `evaluate` compares the measurements with the manifest's budgets for the
  profile being measured, and `render_markdown` prints the table the pull
  request carries, with the summary line naming how many scenarios were
  measured, skipped and breached.

The gate has no opinion about *when* a budget was broken: the driver decides,
so the ordinary test lane can measure and report without asserting on
wall-clock numbers that a busy machine would make meaningless.

### The driver

`crates/terraphim_agent/tests/perf_gate.rs` runs the suite against the binary
cargo built for the test, under the hermetic environment the test suite
already uses (`tests/support/cli_test_env.rs`): a HOME-scoped `settings.toml`
and persistence profiles that point at a temporary directory, never at the
shared `/tmp/terraphim_sqlite` the CI lanes contend over (#398).

It asserts two different things:

- **always**: every command this build carries runs successfully, the
  measurements are coherent, and a skip carries the reason the manifest
  states. That is a real check of the agent's start-up path, and it holds in
  the ordinary lane.
- **under the gate** (`TERRAPHIM_PERF_GATE=1`, which the CI lane sets): the
  measurements meet the ceilings in `docs/benchmarks/agent-cli-budgets.json`,
  and the binary is within its size ceiling.

The second test keeps the document honest: `docs/agent-perf-gate.md` has to
quote exactly the budgets the manifest enforces, in both directions, so the
document can never describe a gate that is not the one in the tree. The
pattern is the one `tests/memory_benchmark_doc.rs` established for the memory
benchmark's floor.

### The budgets

`docs/benchmarks/agent-cli-budgets.json` holds one entry per scenario and
build profile, plus the binary size ceiling per profile. The ceilings sit
several times above the measurement they were set from: the reference host is
a developer laptop, the CI host is a shared machine, and a gate that fails
because the machine was busy teaches everybody to ignore the gate. What the
ceilings do catch is a regression - a configuration load that starts reaching
the network, a store that is suddenly shared, an order-of-magnitude
regression.

### The CI lane

`.gitea/workflows/native-ci.yml` has one step:

```yaml
      - name: Agent CLI performance gate
        run: bash ./scripts/ci/perf-gate.sh
```

One line, one program, one checked-in script: the shape the native runner's
command policy accepts, which classifies the literal first token of a step.
The lane carries no registry-token alias: it builds one crate, and the
registry crates are already in the runner's `CARGO_HOME` from the lanes above
it, and the guard that keeps the alias scoped to the two workspace-building
lanes (`ci_guards.rs::native_ci_aliases_gitea_token_for_the_registry_resolving_lanes`)
refuses an alias anywhere else.

## Verification

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --all-targets --no-fail-fast` | 2055 passed / 0 failed (2043 before this change, plus 12 new tests) |
| `cargo test -p terraphim_agent --test perf_gate -- --nocapture` | the table, 6 measured, 2 skipped, 0 breached |
| `TERRAPHIM_PERF_GATE=1 cargo test -p terraphim_agent --test perf_gate` (dev profile) | passes; a deliberately tightened budget was verified to fail the lane with the measurement in the message |
| The same against `target/release/terraphim-agent` (`TERRAPHIM_PERF_BINARY`, `TERRAPHIM_PERF_PROFILE=release`) | passes; the release numbers are the ones published on the pull request |
| The same against the coding agent's own binary (`origin/task/422-coding-agent`, dev and release) | passes; the two `code` scenarios measure 141.9 ms and 136.1 ms (dev), 145.7 ms and 133.8 ms (release), and the coding agent's release binary is **37,063,968 bytes - over the 36,000,000 byte release ceiling** |
| `cargo test -p terraphim_agent --test ci_guards` | passes; the workflow guards are not disturbed by the new step |
| the lane on the forge (run 46285, job 85367) | **failed on the first run, and the failure was the gate's** - see below |

The first run of the lane on the forge failed, and it is worth recording what
it caught. The lane measured the CI host's timings - `startup` 3.0 ms,
`roles_list` 146.9 ms, `search_turn` 134.2 ms - and every latency budget
passed. What failed was the **binary size**: the CI runner's dev binary is
**499,729,064 bytes**, against the 60,000,000 byte ceiling the change had set
from the reference host's 43,164,400 byte binary.

The same source, built by a different toolchain and a different cargo
configuration, produces a binary eleven times the size. That is debug
information, not code, so an absolute byte ceiling on the dev binary is a
statement about the runner rather than about the product. The dev profile
therefore carries **no** size ceiling - the number is still published in every
report - and the ceiling applies to the release binary, whose size is the
code.

The same run also settled the question the ceilings had been guessed at: the
CI host runs these commands within a few per cent of the reference host, so
the ceilings were tightened from roughly eight times the measurement to
roughly three times it, with the CI host's own numbers in the manifest's
justification.

The gate was also verified in the direction that matters: with the `startup`
budget tightened to 0.1 ms the lane fails with

```text
the agent CLI broke its performance budget in the dev profile:
  `startup`: median 2.1 ms is over the 0.1 ms budget; p95 2.2 ms is over the 0.2 ms budget
```

which is the difference between a gate and a report.

## Validation

| Acceptance criterion (issue #437) | Result |
|---|---|
| "A benchmark lane (cargo bench or a timed-commands suite)" | PASS - a timed-commands suite over the real binary (`crates/terraphim_agent/tests/perf_gate.rs`), run by the native CI lane ✓ |
| "covering: binary startup" | PASS - the `startup` scenario measures `--version` ✓ |
| "code doctor" | PASS with a stated limit - the scenario measures the coding agent's doctor (141.9 ms dev, 145.7 ms release, measured against PR #432's own binary); on `main` it is reported as skipped with the reason, never as a pass ✓ |
| "code commands listing" | PASS with the same limit - the `code_commands` scenario measures 136.1 ms dev and 133.8 ms release ✓ |
| "repl startup" | PASS - the `repl_startup` scenario starts the REPL, feeds it an end-of-file and measures the exit ✓ |
| "one scripted AgentLoop turn" | PASS with a stated limit - on `main` the analogue is the `search_turn` scenario, one scripted knowledge-graph turn; the coding agent's AgentLoop turn replaces it when PR #432 lands ✓ |
| "CI gate with explicit budgets" | PASS - the budgets live in the manifest, the lane asserts them ✓ |
| "doctor and commands listing under a fixed ms bound" | PASS - both scenarios carry fixed millisecond ceilings per profile ✓ |
| "binary size budget" | PASS - a per-profile ceiling on the measured binary's size ✓ |
| "Numbers published on the PR so regressions are visible" | PASS - the table is printed by the gate and carried on the pull request ✓ |

## Limits

- The two `code` scenarios **wait for PR #432**. On `main` they are skipped
  with the reason "the coding agent is not merged (PR #432)", and the
  summary line counts them, so a report can never be read as a full pass.
  Their ceilings are no longer provisional: they were measured against the
  coding agent's own binary (dev and release) and are in the manifest for
  both profiles, so the CI lane gates them the day the coding agent merges.
- **The coding agent's release binary is over the release size ceiling**:
  37,063,968 bytes against 36,000,000. That is the gate working - the coding
  agent adds a model client and the grep crate - and it is a decision for the
  pull request that merges it: raise the ceiling deliberately, with the
  measurement that justifies it, or make the binary smaller.
- The CI lane measures the **dev** profile, because that is the binary the
  lane builds; a release build of the package would add minutes to every
  lane. The release profile's ceilings are enforced by the same harness
  whenever a release binary is measured, and the release numbers are
  published on the pull request.
- The ceilings are roughly three times the measurements they were set from,
  on two hosts that run the commands within a few per cent of each other.
  That is deliberate - see the research - but it means the gate catches a
  regression, not a ten per cent change. The fine-grained signal is the
  number on the pull request.
- The lane was observed on the forge once, and it failed there for the
  reason above; the ceiling that was wrong is the one the fix removed. The
  same script was run locally in both profiles and in both directions
  (passing and failing).
- `ubs` reports the harness's two `Command::new(<path>)` calls as a critical
  "command executable from an untrusted-looking value". The executable is the
  binary under test, or a path the operator chose in their own workspace; the
  same rule fires on the suites that already spawn the agent
  (`tests/learn_no_service_tests.rs`, `tests/extract_functionality_validation.rs`).
