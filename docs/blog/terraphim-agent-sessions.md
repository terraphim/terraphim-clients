# Sessions search across AI coding assistants

The Terraphim agent can import the session history of Claude Code,
Cursor, and Aider into a single search index, so a query like
"guard priority" returns the relevant conversation from whichever
tool you happened to use that day. This post walks through the
import flow.

## Quick start

```bash
# One-shot import from all sources (Claude Code, Cursor, Aider)
terraphim-agent sessions import

# Search across the imported corpus
terraphim-agent sessions search "guard priority"
```

The importer auto-detects the standard install paths
(`~/.claude/projects/`, `~/.cursor/`, `~/.aider.chat.history.md`).
It walks the JSONL files, extracts user/assistant turns, and
indexes them under the active role's knowledge graph.

## Why this matters

Each AI assistant has its own session log format and storage path.
Without a unifying index, you have to remember which tool produced
the snippet you want to recover. Sessions search collapses that
into one search box.

## What it returns

`sessions search` returns role-ranked JSON chunks, each one a
window of conversation around the matched turn:

```json
{
  "chunks": [
    {
      "rank": 1,
      "title": "PreToolUse hook rewrite semantics",
      "score": 0.87,
      "preview": "we need --rewrite to be opt-in because...",
      "source": "claude-code",
      "session_id": "ses_40ae",
      "turn": 14
    }
  ],
  "concepts_matched": ["guard", "pretool", "rewrite"]
}
```

## Bounded import

The import walker is bounded: depth limit, symlink guard, and a hit
cap per directory (Refs #123). You can override the bounds via
flags:

```bash
terraphim-agent sessions import --depth 8 --max-files 5000
```

## References

* README: `crates/terraphim_agent/README.md` (Sessions)
* Reference: `docs/agent-reference.md` (`sessions`)
* Source: `crates/terraphim_agent/src/listener.rs` (importer),
  `crates/terraphim_agent/src/sessions/` (search index)
