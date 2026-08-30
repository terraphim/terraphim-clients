# Robot mode: structured output for AI agents

Robot mode is the contract between `terraphim-agent` and any
upstream AI agent (Claude Code, your own orchestrator, CI bots).
This post walks through the JSON output, the exit codes, and the
self-describing schemas.

## Quick start

```bash
# Machine-readable search result
terraphim-agent --robot --format json search "guard priority"

# Self-discover every command
terraphim-agent --robot --format json robot schemas | jq '.[].name'

# Capabilities + exit codes
terraphim-agent --robot --format json robot capabilities
```

## Global flags

`--robot` and `--format` are **global** on the top-level `Cli`
struct. They must precede the subcommand (Refs #127):

```bash
# Correct
terraphim-agent --robot --format json search "x"

# Wrong -- clap errors with `unexpected argument '--robot' found`
terraphim-agent search "x" --robot --format json
```

## Output formats

| Format | Use case |
|--------|----------|
| `human` | Default. Coloured terminal output. |
| `json` | Pretty-printed JSON. Suitable for humans reading a log. |
| `json-compact` | One-line JSON. Suitable for piping into `jq`. |

`--robot` implies machine-readable: with `--format human` it falls
back to `json` automatically.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success (results or no-results-without-`--fail-on-empty`) |
| 1 | Generic error (also `Block` from `guard`) |
| 2 | Invalid invocation (clap parse error) |
| 3 | Reserved |
| 4 | `ERROR_NOT_FOUND` — only with `--fail-on-empty` |
| 5 | `ERROR_AUTH` — auth required or failed |
| 6 | `ERROR_NETWORK` — transport-level failure |
| 7 | `ERROR_TIMEOUT` — exceeded configured timeout |

`--fail-on-empty` makes empty results return exit code 4 instead of
0, so pipelines can distinguish "found nothing" from "ran".

## Self-describing schemas

`robot schemas` returns one `CommandDoc` per subcommand, including
the `repl_only` flag introduced in #131:

```bash
terraphim-agent --robot --format json robot schemas | \
    jq -r '.[] | "\(.name)\t\(.repl_only)"'
# search  false
# config  false
# vm      true     # REPL-only — no top-level CLI parity
# chat    true     # REPL-only, behind --features repl-chat
```

Filter REPL-only entries out when checking top-level CLI parity:

```bash
terraphim-agent --robot --format json robot schemas | \
    jq -r '.[] | select(.repl_only == false) | .name'
```

## Examples

`robot examples <command>` returns worked examples for a single
subcommand, with expected output captured.

## References

* Source: `crates/terraphim_agent/src/robot/` (`mod.rs`, `docs.rs`,
  `exit_codes.rs`, `schema.rs`)
* Reference: `docs/agent-reference.md` (`robot`)
* Test: `crates/terraphim_agent/tests/robot_schemas.rs`
