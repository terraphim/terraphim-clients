# Research, Design, Verification and Validation: the session analyzer's warning flood (#402)

**Issue**: terraphim-clients#402
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect with a measured reproduction; the cause is in a dependency)

## Research

### The reproduction

The issue reports ~9 000 `missing field \`uuid\`` warnings per
`terraphim-agent sessions search`, all from
`terraphim_session_analyzer::parser`, on JSONL event types the parser does
not recognise.

Measured on this host, before the fix:

```
$ terraphim-agent sessions search "checklist" 2>/tmp/err.log
$ grep -c WARN /tmp/err.log
385
$ grep WARN /tmp/err.log | head -1
WARN terraphim_session_analyzer::parser] Failed to parse line 9:
  {"type":"atis-latch","atis":"","sessionId":"bc40..."} - Error: missing field `uuid` at line 1 column 82
```

(The count is host-dependent: it is one warning per unrecognised line of every
Claude Code session on the machine. The issue's host had ~9 000.)

### Where the warning comes from

`terraphim-session-analyzer` 1.23.0, `src/parser.rs`:

```rust
const SKIP_ENTRY_TYPES: &[&str] = &[
    "last-prompt", "mode", "permission-mode", "ai-title",
    "file-history-snapshot", "queue-operation", "agent-name",
    "pr-link", "tool_reference", "text", "attachment", "system", ...
];

// ... for every line that is not skipped:
Err(e) => {
    warn!("Failed to parse line {}: {} - Error: {}", line_num + 1, line, e);
}
```

The analyzer already skips the metadata event types it knows - the comment
says that removes "~2003 of ~2430 WARN lines" - but Claude Code adds event
types faster than the analyzer learns them, and the ones it does not know
(`atis-latch`, `api-request-shape`, `api-request-blob`, `cost-state`, ...)
fall into the warning path.

### What this repository can and cannot fix

- The **parser** is an external crate (`terraphim-session-analyzer` 1.23.0,
  pinned in `crates/terraphim_sessions/Cargo.toml`). The registry index
  carries no newer version, so there is nothing to bump to.
- The **warning text** reaches the agent's stderr because the agent installs
  its own logger (`crates/terraphim_agent/src/logging.rs`), and the tracing
  events of the dependency arrive through the `log` facade.
- The agent already uses that logger to drop a benign `ERROR` line from
  `terraphim_service` for exactly this reason: the fix lives in a crate this
  repository cannot edit, so the agent filters the record instead.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The noise is the problem | The flood must stop; stderr must stay readable |
| C2 | A real parse failure is information | The filter must be narrow |
| C3 | The real fix belongs upstream | The agent-side filter is a mitigation, and says so |

## Design

`crates/terraphim_agent/src/logging.rs` gains a second predicate next to the
existing thesaurus one:

```rust
fn is_unrecognised_session_event(level: Level, target: &str, message: &str) -> bool {
    if level != Level::Warn || !target.starts_with("terraphim_session_analyzer") {
        return false;
    }
    if !message.contains("Failed to parse line") {
        return false;
    }
    message.contains("missing field")
}
```

The match is narrow on purpose:

- only a `WARN` from the session analyzer,
- only the "failed to parse line" record,
- and only when the error is a **missing field** - a valid JSON object that
  simply is not a conversational entry. A line that is genuinely malformed
  (`expected value at line 1 column 2`) keeps its warning.

The `FilteredLogger` drops those records and passes everything else through
untouched, exactly as it does for the benign thesaurus `ERROR`.

## Verification

| Check | Result |
|---|---|
| `terraphim-agent sessions search "checklist"` - WARN lines before | 385 |
| `terraphim-agent sessions search "checklist"` - WARN lines after | **0** |
| the same search's results | unchanged ("Found 1 matching sessions") |
| stderr lines after | 75 (the normal service INFO lines) |
| `cargo test -p terraphim_agent --lib logging` | 14 passed |

New tests: the predicate matches the real warning format (pinned to the
message the analyzer writes), keeps a genuinely malformed line, keeps warnings
from other targets and other levels; and the filtered logger drops the flood
while the genuine warning survives.

## Validation

| Acceptance criterion (issue #402) | Result |
|---|---|
| "Parser skips unrecognised top-level event types silently (or debug-level)" | PASS for the agent's output - the unrecognised envelopes no longer produce a warning |
| "the noise is the problem: one WARN per unrecognised line makes stderr unusable and hides real warnings" | PASS - 0 warnings instead of 385 on this host; the search still finds its results |
| "or maps the newer envelopes" | NOT DONE - the mapping belongs to the analyzer crate |

## Limits

- This is a **mitigation**, not the fix: the analyzer still parses every
  unrecognised line and still warns; the agent drops the record. The real fix
  is to add the newer event types to the analyzer's `SKIP_ENTRY_TYPES` (or to
  make it skip any envelope without a `uuid`), which lives in the
  `terraphim-session-analyzer` crate. No newer version of that crate is
  published, so there is nothing to bump to here.
- A line that is valid JSON but has no `uuid` for a *different* reason would
  also be dropped. The error text cannot tell the two apart; the upstream fix
  can.
- The filter lives in the agent's logger, so the warning still appears in a
  binary that does not install it (the MCP server, for instance).
