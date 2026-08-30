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
the `repl_only` flag introduced in #131. A `chat` entry exists in
two flavours: the **CLI** `chat` (gated by `--features llm`, default-on;
one-shot prompt → response, `repl_only: false`) and the **REPL** `chat`
(gated by `--features repl-chat`; interactive `/chat` command,
`repl_only: true`). Both can be present in a `repl-chat` build — the
`repl_only` flag distinguishes them. Refs terraphim-clients#134 P1.

```bash
terraphim-agent --robot --format json robot schemas | \
    jq -r '.[] | "\(.name)\t\(.repl_only)"'
# search    false
# config    false
# vm        true       # REPL-only (firecracker-gated)
# chat      false      # CLI one-shot chat, behind --features llm (default-on)
# chat      true       # REPL interactive /chat, behind --features repl-chat
# summarize true       # REPL-only, behind --features repl-chat
```

Filter REPL-only entries out when checking top-level CLI parity:

```bash
terraphim-agent --robot --format json robot schemas | \
    jq -r '.[] | select(.repl_only == false) | .name'
# search config role graph chat
```

Note: `chat` appears twice in `repl-chat` builds; one with
`repl_only: false` (CLI) and one with `repl_only: true` (REPL). The
filter above returns each entry that has `repl_only: false`, so
`chat` shows up once (the CLI one) in builds that include it.

## Examples

`robot examples <command>` returns worked examples for a single
subcommand, with expected output captured.

## References

* Source: `crates/terraphim_agent/src/robot/` (`mod.rs`, `docs.rs`,
  `exit_codes.rs`, `schema.rs`)
* Reference: `docs/agent-reference.md` (`robot`)
* Test: `crates/terraphim_agent/tests/robot_schemas.rs`
