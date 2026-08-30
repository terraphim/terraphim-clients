# Shared learning store

Every agent makes mistakes. The shared learning store turns those
mistakes into a markdown-backed, BM25-deduped knowledge base that
survives across sessions and across machines. This post walks
through the capture, dedup, trust, and replay flow.

## Quick start

```bash
# Capture a learning from a PostToolUse failure (auto-capture hook)
terraphim-agent learn hook

# Manual capture
terraphim-agent learn capture "use bun add instead of npm install"

# List recent learnings
terraphim-agent learn list --recent

# Correct a stale learning
terraphim-agent learn correct <id> "use bun add (not bun install) for new projects"
```

## Where the store lives

By default the store is `~/.local/share/terraphim/learnings/` with
one markdown file per learning. Use `--global` to switch to
`/usr/local/share/terraphim/learnings/` for system-wide learnings.

## Dedup

Insertions are deduped against existing entries using BM25
similarity. A new capture with a score ≥ 0.85 against an existing
learning is rejected as a duplicate. Use `learn correct <id> "..."`
to amend the existing entry instead of creating a near-duplicate.

## Trust levels

Each learning carries one of three trust levels:

| Level | Source | Mutability |
|-------|--------|------------|
| `local` | `learn capture` from the local machine | Editable via `correct` |
| `shared` | Synced from a shared wiki / Git repo | Editable via PR |
| `system` | Embedded in the binary | Read-only |

`learn list` defaults to `local` only. Pass `--include-shared` or
`--include-system` to widen the scope.

## Replay

A learning can be replayed as a procedure:

```bash
terraphim-agent learn replay <id> --dry-run
terraphim-agent learn replay <id>
```

Replay resolves the captured command (`use bun add instead of npm
install` → `bun add <args>`) and runs it with the same args as the
original capture.

## Auto-capture

The `learn hook` command is meant to be wired into Claude Code's
PostToolUse:

```json
{
  "hooks": {
    "PostToolUse": [{
      "command": "terraphim-agent learn hook",
      "timeout": 5
    }]
  }
}
```

Failed bash commands (`exit != 0`) are captured with the command,
exit code, stderr, and a prompt-derived correction if the user
typed one before the next successful command.

## References

* Source: `crates/terraphim_agent/src/learnings/`,
  `crates/terraphim_agent/src/shared_learning/`
* Reference: `docs/agent-reference.md` (`learn`)
