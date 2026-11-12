# Research, Design, Verification and Validation: functional evidence for the pi-terraphim-learn package (#467)

**Issue**: terraphim-clients#467
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect, explicit acceptance criteria, local change)

## Research

### The gap

`packages/pi-terraphim-learn` is a **JavaScript** plugin: an ESM module whose
default export `activate(pi)` registers an `onToolResult` handler that
normalises a bash failure into the Claude learn envelope and pipes it to
`terraphim-agent learn hook --format claude --learn-hook-type post-tool-use`.

The workspace's cargo tests cannot reach it (it is not a Rust crate), and no
package-level CI job existed - which is exactly what the 2026-11-03 audit
recorded.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The native runner's command policy allows only `cargo` and repository scripts | The test must be invoked through a `bash ./scripts/...` wrapper |
| C2 | The runner may not have `node` | The wrapper skips with a notice instead of failing the lane |
| C3 | The plugin spawns `terraphim-agent` | The test needs a fake agent on `PATH` to observe the envelope |
| C4 | The plugin is fail-open by design | The test must also prove it never throws |

## Design

1. `packages/pi-terraphim-learn/index.test.mjs` - a `node:test` suite that
   imports the real plugin, registers it on a fake `pi` object, and drives the
   `onToolResult` handler:

   - a bash failure is piped to the agent as the Claude learn envelope
     (`tool_name: "Bash"`, the command, `exit_code`, stdout/stderr),
   - a successful command is not captured,
   - a non-bash tool is not captured,
   - a missing agent binary resolves without throwing (fail-open).

   The fake agent is a shell script on `PATH` that appends its stdin to a file,
   so the assertions are on what the agent actually receives.

2. `scripts/ci/pi-terraphim-learn-tests.sh` - the CI entry point. It runs
   `node --test packages/pi-terraphim-learn/index.test.mjs`, and prints a
   `::notice::` skip when node is absent.

3. CI wiring: the GitHub lane (`ubuntu-latest`, node present) runs the wrapper
   as a step; the native lane runs the same wrapper and skips when the runner
   image has no node.

4. `packages/pi-terraphim-learn/package.json` gains a `test` script, so the
   package can be tested the way any npm package would be.

## Verification

| Check | Result |
|---|---|
| `node --test packages/pi-terraphim-learn/` | 4 passed, 0 failed |
| `bash ./scripts/ci/pi-terraphim-learn-tests.sh` | exit 0, 4 passed |
| `cargo test -p terraphim_agent --test ci_guards` | 25 passed (the workflow edits do not break the workflow guards) |
| YAML parse of both workflows | ok |

## Validation

| Acceptance criterion (issue #467) | Result |
|---|---|
| "Add a smoke test that loads the package into pi and runs one learn-capture round trip" | PASS - the test registers the plugin on a fake `pi`, fires a tool result, and asserts the envelope the agent receives |
| "Wire it into native-ci" | PASS - a `bash ./scripts/ci/pi-terraphim-learn-tests.sh` step; it runs where node exists and skips with a notice where it does not |
| "A CI job installs the package and passes the round-trip test" | PASS on the GitHub lane (node is present on `ubuntu-latest`); the native lane runs the same wrapper and reports the skip explicitly |

## Limits

- The native runner image may not carry node, so the native lane can only
  report the skip. The GitHub lane is the one that actually executes the test.
  Moving the node runtime onto the runner image would close that gap.
- The test exercises the plugin's own logic and the envelope it produces; it
  does not run the real `terraphim-agent learn hook` behind it (the agent side
  is covered by the agent's own suites).
