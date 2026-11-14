# terraphim-agent CLI performance gate

The gate measures the agent's own commands - the binary a user runs - and
fails when one of them crosses the ceiling committed next to it. It is the
fx discipline applied to `terraphim-agent`: a benchmarked command has a
budget, the budget is in the repository, and the numbers travel with the pull
request so a regression is visible in the review (terraphim-clients#437).

Nothing here changes what the agent does. This work makes a change of
behaviour visible before a user finds it.

## What is measured

| Scenario | Command | What it covers |
|---|---|---|
| `startup` | `terraphim-agent --version` | Process start-up and argument parsing. It is the machine-speed canary: the cheapest command, so its ceiling is the tightest, and when it breaks every other number measured on that host is suspect. |
| `help` | `terraphim-agent --help` | The same, with the help text rendered. |
| `roles_list` | `terraphim-agent roles list` | The command listing on `main`: it loads the settings and builds the role graph. |
| `config_show` | `terraphim-agent config show` | The settings load on its own. |
| `repl_startup` | `terraphim-agent repl` (end-of-file on stdin) | The REPL starts, initialises and exits cleanly. |
| `search_turn` | `terraphim-agent search hello` | One scripted knowledge-graph turn: the closest analogue on `main` to the AgentLoop turn the coding agent will add. |
| `code_doctor` | `terraphim-agent code doctor` | The coding agent's environment check. Skipped until the coding agent is merged (PR #432). |
| `code_commands` | `terraphim-agent code commands` | The coding agent's markdown-command listing. Skipped until the coding agent is merged (PR #432). |

Every command runs under the hermetic environment the test suite already
uses: a HOME-scoped `settings.toml` and persistence profiles that point at a
temporary directory, never at the shared `/tmp/terraphim_sqlite` the CI lanes
contend over (#398).

## The budgets

The budgets live in `docs/benchmarks/agent-cli-budgets.json`, one entry per
scenario and build profile. The document and the manifest are checked against
each other in both directions by `tests/perf_gate.rs`, so this table cannot
describe a gate that the tree does not enforce.

| Scenario | Profile | Median budget (ms) | p95 budget (ms) |
|---|---|---|---|
| `startup` | `dev` | 50.0 | 100.0 |
| `startup` | `release` | 30.0 | 60.0 |
| `help` | `dev` | 50.0 | 100.0 |
| `help` | `release` | 30.0 | 60.0 |
| `roles_list` | `dev` | 1200.0 | 1800.0 |
| `roles_list` | `release` | 1000.0 | 1500.0 |
| `config_show` | `dev` | 1200.0 | 1800.0 |
| `config_show` | `release` | 1000.0 | 1500.0 |
| `repl_startup` | `dev` | 1800.0 | 2800.0 |
| `repl_startup` | `release` | 1400.0 | 2200.0 |
| `search_turn` | `dev` | 1800.0 | 2800.0 |
| `search_turn` | `release` | 1400.0 | 2200.0 |
| `code_doctor` | `release` | 1400.0 | 2200.0 |
| `code_doctor` | `dev` | 1400.0 | 2200.0 |
| `code_commands` | `release` | 1100.0 | 1700.0 |
| `code_commands` | `dev` | 1100.0 | 1700.0 |

The binary size is gated too:

| Profile | Budget (bytes) | Measured (bytes) |
|---|---|---|
| `dev` | 60000000 | 43164400 |
| `release` | 36000000 | 29942240 |

The two profiles carry separate budgets because the timings are not
interchangeable: the dev profile is what the CI lane builds and measures, the
release profile is what a user runs. A scenario with no budget for the profile
being measured is measured and reported, but not gated.

### Why the ceilings are so far above the measurements

Every ceiling is roughly eight times the measurement it was set from (the
canary, `startup`, gets more, because it is the cheapest command and its
ceiling only has to catch a pathological host). The measurement host is a
developer laptop; the CI host is a shared machine that also runs the forge,
and its speed has not been measured - the runner is currently disabled. A
gate that fails because the machine was busy teaches everybody to ignore the
gate, so the ceilings are set to catch a regression (an order of magnitude, a
new network call in the start-up path, a store that is suddenly shared)
rather than to catch a ten per cent change.

The fine-grained signal is the number, not the ceiling: the pull request
carries the measurement of every scenario, so a ten per cent regression is
visible in the review. Tightening the ceilings is a follow-up for whoever
first measures the CI host's speed - one number in the manifest per
scenario, with the measurement that justifies it in this document.

## The reference measurements

Measured on the host named below, with the harness as it stands, five
repetitions per scenario, the median and the nearest-rank p95 in
milliseconds.

| Scenario | Command | dev median | dev p95 | release median | release p95 |
|---|---|---|---|---|---|
| `startup` | `--version` | 2.2 | 3.1 | 1.6 | 1.7 |
| `help` | `--help` | 2.1 | 2.2 | 1.5 | 1.7 |
| `roles_list` | `roles list` | 102.0 | 105.8 | 119.8 | 136.8 |
| `config_show` | `config show` | 102.9 | 121.8 | 120.8 | 129.9 |
| `repl_startup` | `repl` | 120.4 | 127.5 | 110.5 | 125.1 |
| `search_turn` | `search hello` | 144.1 | 178.6 | 136.3 | 156.3 |

The dev and the release numbers are close: the cost of these commands is
dominated by loading the settings and building the role graph, not by the
arithmetic the optimiser would have removed.

The run-to-run spread on the reference host is around twenty per cent for the
commands that load the settings, and the first repetition of a scenario pays
for a cold store, which is why the p95 sits above the median. That spread is
the other reason the ceilings are several times the measurement: a gate that
fails on a ten per cent change is a gate that gets switched off.

### The two coding-agent scenarios

The `code` scenarios can only be measured with a binary that carries the
coding agent, so they were measured against the release and the dev build of
`origin/task/422-coding-agent` (PR #432), with the same harness and the same
hermetic environment:

| Scenario | Profile | Median (ms) | p95 (ms) | Budget (median / p95) |
|---|---|---|---|---|
| `code_doctor` | release | 145.7 | 148.2 | 1400.0 / 2200.0 |
| `code_commands` | release | 133.8 | 152.9 | 1100.0 / 1700.0 |
| `code_doctor` | dev | 141.9 | 192.3 | 1400.0 / 2200.0 |
| `code_commands` | dev | 136.1 | 150.2 | 1100.0 / 1700.0 |

The ceilings those rows carry are the ones in the manifest, and they were set
from these measurements with the same headroom as the other commands.

One number in that measurement is worth its own line: the coding agent's
release binary is **37,063,968 bytes**, over the 36,000,000 byte release
ceiling this document states for `main`'s binary. That is the gate doing its
job - the coding agent adds a model client and the grep crate - and it is a
decision for the pull request that merges it: the ceiling is raised
deliberately, with the measurement that justifies it, or the binary gets
smaller.

The reference host is the development laptop (16 cores, 62 GB). The CI host is
not named here because the runner's identity has moved more than once; the
measurement that matters is the one the CI lane prints, and it is published on
the pull request with every change to these budgets.

## Running it

The gate is a test, so it runs with the rest of the suite:

```bash
# measure and report, without asserting on the ceilings
cargo test -p terraphim_agent --test perf_gate -- --nocapture

# the gate: assert the ceilings, print the table
TERRAPHIM_PERF_GATE=1 cargo test -p terraphim_agent --test perf_gate -- --nocapture

# what the CI lane runs
bash ./scripts/ci/perf-gate.sh
```

The binary the gate measures is the one cargo built for the test. Two
environment variables move it elsewhere:

| Variable | Effect |
|---|---|
| `TERRAPHIM_PERF_GATE` | Set it to assert the ceilings. Without it the suite measures and reports, and asserts only that every command works. |
| `TERRAPHIM_PERF_BINARY` | Measure another build, for instance the release binary: `TERRAPHIM_PERF_BINARY=target/release/terraphim-agent TERRAPHIM_PERF_PROFILE=release cargo test -p terraphim_agent --test perf_gate -- --nocapture`. |
| `TERRAPHIM_PERF_PROFILE` | The profile the budgets are taken from when the binary's path does not say which one it is. |

The ordinary test lane measures without asserting on the ceilings on purpose:
wall-clock numbers on a developer's machine, which is running a compiler
while the tests run, would make the gate a coin toss. The CI lane runs the
same script with the gate on.

## How the CI lane works

`.gitea/workflows/native-ci.yml` has one step for the gate:

```yaml
      - name: Agent CLI performance gate
        run: bash ./scripts/ci/perf-gate.sh
```

One line, one program, one checked-in script: the shape the native runner's
command policy accepts, which classifies the literal first token of a step.
The lane carries no registry-token alias - it builds one crate, and the
registry crates are already in the runner's `CARGO_HOME` from the lanes above
it - because the guard that keeps the alias scoped to the two
workspace-building lanes refuses an alias anywhere else.

## How a missing command is reported

The two `code` scenarios name a subcommand that `main` does not have yet. The
harness probes `<binary> code --help` first: clap answers an unknown
subcommand with status 2 and "unrecognized subcommand", which is how the
harness tells "the command is not in this build" from "the command is there
and broken". A scenario that names no subcommand must run: if it fails, the
gate fails, because a removed command is a regression and not a skip.

A skip is printed with its reason and counted in the summary:

```text
Summary: 6 measured, 2 skipped, 0 breached.
```

so a report can never be mistaken for a full pass. When the coding agent
lands, the two scenarios start measuring without any change to the harness:
the budgets are already in the manifest, for both profiles, and they were set
from a measurement of the coding agent's own binary (the table above).

## Where the numbers live

| Artefact | Path |
|---|---|
| The budgets | `docs/benchmarks/agent-cli-budgets.json` |
| The harness | `crates/terraphim_agent/src/perf_gate.rs` |
| The driver | `crates/terraphim_agent/tests/perf_gate.rs` |
| The CI entry point | `scripts/ci/perf-gate.sh` |
| The research | `docs/plans/research-perf-gates-2026-11-14.md` |
| The design | `docs/plans/design-perf-gates-2026-11-14.md` |
