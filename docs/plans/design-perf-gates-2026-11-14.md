# Design Document: Benchmark discipline for terraphim-agent (fx-style perf gates)

**Issue**: #437
**Status**: Draft
**Date**: 2026-11-14
**Research**: `docs/plans/research-perf-gates-2026-11-14.md`

## Summary

One measurement mechanism, one manifest, one CI lane.

- A library module `crates/terraphim_agent/src/perf_gate.rs` measures the
  agent's own CLI by spawning the built binary under a hermetic environment,
  and evaluates the measurements against budgets.
- A committed manifest `docs/benchmarks/agent-cli-budgets.json` holds the
  scenarios and their budgets, per build profile.
- A test driver `crates/terraphim_agent/tests/perf_gate.rs` runs the suite.
  It always asserts the structural invariants (the commands work, the
  manifest is coherent, the measurements are sane); it asserts the **budgets**
  when `TERRAPHIM_PERF_GATE=1`, which is what the CI lane sets.
- A checked-in script `scripts/ci/perf-gate.sh` is the CI entry point, called
  from `native-ci.yml` with one line.

## Motivation

The repository has Criterion benches for library functions and no gate for the
product. fx's discipline - a fixed ceiling per command, checked in CI, with
the number published - is the model the issue names. The mechanism is
deliberately a timed-commands suite rather than a Criterion bench: the
product is a process, and `cargo bench` would force a release build of the
package into every lane.

## Architecture

```text
docs/benchmarks/agent-cli-budgets.json     the scenarios and their budgets
                |
                v
crates/terraphim_agent/src/perf_gate.rs    manifest types, measurement,
                |                          evaluation, markdown rendering
                v
crates/terraphim_agent/tests/perf_gate.rs  the driver: measure, evaluate,
                |                          print the table, assert
                v
scripts/ci/perf-gate.sh                    the CI entry point (one line in
                                           the workflow calls this)
```

### The manifest

```json
{
  "version": 1,
  "binary_size_budgets": { "dev": 60000000, "release": 36000000 },
  "scenarios": [
    {
      "name": "startup",
      "args": ["--version"],
      "repetitions": 5,
      "budgets": {
        "dev":    { "median_ms": 50.0, "p95_ms": 100.0 },
        "release": { "median_ms": 30.0, "p95_ms": 60.0 }
      }
    },
    {
      "name": "code_doctor",
      "args": ["code", "doctor"],
      "requires_subcommand": "code",
      "absent_note": "the coding agent is not merged (PR #432)",
      "repetitions": 5,
      "budgets": {
        "dev": { "median_ms": 1400.0, "p95_ms": 2200.0 },
        "release": { "median_ms": 1400.0, "p95_ms": 2200.0 }
      }
    }
  ]
}
```

Fields:

| Field | Meaning |
|---|---|
| `name` | The scenario's identity in the report. |
| `args` | The argument vector passed to the agent binary. |
| `stdin` | Optional text piped to the child's stdin (the REPL scenario sends an EOF). |
| `repetitions` | How many timed runs; the median and the p95 are computed over them. |
| `requires_subcommand` | The subcommand that must exist for the scenario to be measurable. When the subcommand is absent from the build the scenario is **skipped with the reason**, not failed. |
| `absent_note` | The reason printed with a skip. |
| `budgets` | Per profile: `median_ms` and `p95_ms` ceilings. A profile with no entry is not enforced for that profile. |
| `binary_size_budgets` | Per profile: a ceiling on the measured binary's size in bytes. |

### Availability detection

A scenario that names `requires_subcommand` is probed with
`<binary> <subcommand> --help`. Clap answers an unknown subcommand with
status 2 and "unrecognized subcommand '<name>'"; the probe therefore
distinguishes "the command is not in this build" from "the command is broken".
A scenario without `requires_subcommand` must exist: if it fails, the suite
fails - a removed command is a regression, not a skip.

Skips are counted and named in the summary, so a report cannot be mistaken for
a full pass.

### Budget semantics

- The **median** is the gate's primary number: the typical cost.
- The **p95** (nearest rank over the repetitions) is the second ceiling: it
  catches a tail that the median hides.
- Both ceilings are absolute, per profile, with the headroom documented in
  the manifest. A budget is never derived from the measurement of the run
  that checks it - a gate that moves with the thing it gates is not a gate.
- The `startup` scenario is the machine-speed canary: it is the cheapest
  command, so its ceiling is tight. When it breaches, the diagnostic says the
  host is probably degraded, because everything else measured on that host is
  then suspect.

### Enforcement

`TERRAPHIM_PERF_GATE=1` turns the budget assertions on. Without it the suite
still measures and prints, and still asserts that every command succeeded and
that the measurements are coherent; only the ceilings are not enforced. This
keeps the ordinary test lane fast and free of timing assertions, while the
dedicated CI lane is the gate.

### The CI lane

```yaml
      - name: Performance gate (agent CLI budgets)
        run: bash ./scripts/ci/perf-gate.sh
```

One line, one program (`bash`), one checked-in script: the shape the native
runner's command policy accepts. The lane carries **no** registry-token alias
- it builds one crate, and the registry crates are already in the runner's
`CARGO_HOME` from the earlier workspace lanes. Adding an alias would break
`native_ci_aliases_gitea_token_for_the_registry_resolving_lanes`, which
requires the alias to precede exactly the two workspace-building lanes.

## File changes

| File | Change |
|---|---|
| `crates/terraphim_agent/src/perf_gate.rs` | New: the harness (manifest types, measurement, evaluation, rendering). |
| `crates/terraphim_agent/src/lib.rs` | New: `pub mod perf_gate;`. |
| `crates/terraphim_agent/tests/perf_gate.rs` | New: the driver test and the harness's unit tests' host. |
| `docs/benchmarks/agent-cli-budgets.json` | New: the scenarios and the budgets. |
| `scripts/ci/perf-gate.sh` | New: the CI entry point. |
| `.gitea/workflows/native-ci.yml` | New step: the lane. |
| `docs/plans/research-perf-gates-2026-11-14.md` | New: the research. |
| `docs/plans/design-perf-gates-2026-11-14.md` | New: this document. |
| `docs/plans/note-perf-gates-2026-11-14.md` | New: the implementation note and the measured numbers. |

## Scenarios

| Scenario | Command | Profile budgets | Note |
|---|---|---|---|
| `startup` | `--version` | dev / release | The canary. |
| `help` | `--help` | dev / release | Argument parsing and the help text. |
| `roles_list` | `roles list` | dev / release | The command listing on `main`: it loads the settings and the role graph. |
| `config_show` | `config show` | dev / release | Settings load only. |
| `repl_startup` | `repl` (stdin: EOF) | dev / release | The REPL starts and exits cleanly. |
| `search_turn` | `search hello` | dev / release | One scripted knowledge-graph turn: the closest analogue on `main` to the AgentLoop turn. |
| `code_doctor` | `code doctor` | release | Skipped until the coding agent lands (PR #432). |
| `code_commands` | `code commands` | release | Skipped until the coding agent lands (PR #432). |

The two `code` scenarios carry no `dev` budget: they are measured against the
release profile, which is the profile the coding agent ships in.

## Testing strategy

| Level | What it covers |
|---|---|
| Unit (in `src/perf_gate.rs`) | The percentile arithmetic (nearest rank), the manifest parsing and validation, the budget evaluation, the profile detection, the markdown rendering. |
| Integration (`tests/perf_gate.rs`) | The end-to-end suite: the real binary, the real hermetic environment, the real manifest. Asserts that every available command exits successfully and that the measurements are coherent. Asserts the budgets when the gate is enabled. |

No mocks: the harness spawns the real binary; the unit tests exercise real
data structures, not doubles.

## Risks

| Risk | Mitigation |
|---|---|
| A loaded CI host breaches a ceiling | Generous headroom, the median as the primary number, the canary diagnostic. |
| The dev profile is coarse | Per-profile budgets; the release numbers are published on the pull request. |
| The `code` scenarios stay skipped | The skip is printed with the reason and counted in the summary. |

## Verification plan

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --all-targets --no-fail-fast`
4. `TERRAPHIM_PERF_GATE=1 cargo test -p terraphim_agent --test perf_gate -- --nocapture`
   - the gate in enforce mode, with the table.
5. The same against the release binary, for the published numbers.
6. `cargo test -p terraphim_agent --test ci_guards` - the workflow guards.
