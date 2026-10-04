# ADR 003: PreToolUse Hook Substitution is Opt-In

Status: Accepted

Date: 2026-08-30

## Context

The `terraphim-agent hook --hook-type pre-tool-use` pipeline runs two
KG-driven transformations on every intercepted `Bash` tool call
before returning the (possibly modified) JSON envelope to Claude
Code:

1. **Guard check** (`CommandGuard::check`) — blocks or allows based
   on the priority order in ADR-002.
2. **Thesaurus substitution** (`ReplacementService::replace_fail_open`)
   — replaces matched substrings with a KG-known alternative
   (e.g. `npm install` → `bun add`, `grep ... | xargs` →
   `terraphim-grep ... | xargs`).

Prior to this ADR the substitution was **always-on**. A stray KG
synonym or typo could mutate a destructive command in two ways:

* **Silent rewrite**: `rm -rf /tmp/foo` could become
  `rm -Readiness Feedback /tmp/foo` because `/tmp/` (or some other
  substring) appeared in the thesaurus. The user got no warning.
* **Undetected destruction**: a typo'd destructive synonym like
  `rm -Readiness Feedback` could blow away files because the guard
  was bypassed (it ran on the post-substitution command, but the
  thesaurus also substituted parts of the path the guard relied
  on to recognise `/tmp/`).

The original bug report is captured in
`terraphim-clients#126` and reproduced verbatim in the design doc at
`docs/plans/design-terraphim-grep-agent-fixes-2026-08-30.md`.

## Decision

The PreToolUse hook pipeline now runs in two distinct modes:

1. **Default mode (no `--rewrite`)**:
   * The substitution service is still invoked so we can **probe**
     for matches.
   * If the probe finds replacements, the hook emits a `warnings`
     array entry on the returned JSON instead of mutating the
     command. Example:
     ```json
     {
       "tool_input": {"command": "rm -rf /tmp/foo"},
       "tool_name": "Bash",
       "warnings": [
         "command contained 1 KG-replaceable substring(s); pass --rewrite to enable substitution. Original: `rm -rf /tmp/foo`"
       ]
     }
     ```
   * The agent runtime (Claude Code) is expected to surface the
     `warnings` to the user, who can then decide to either
     re-run the tool with `--rewrite` or amend the command.
   * This is the safe default: a stray KG match cannot mutate a
     destructive command.

2. **Opt-in mode (`--rewrite`)**:
   * The substitution runs as before: matched substrings are
     replaced in the returned `tool_input.command`.
   * The agent runtime can use this when it has user consent (e.g.
     in a known-safe context, or when the user explicitly types
     `--rewrite`).

The guard check runs **unconditionally** in the default mode for
pre-tool-use (it short-circuits to `deny` for destructive commands)
and **never** short-circuits the substitution probe. The substitution
probe is informational only — it never mutates `tool_input.command`
unless `--rewrite` is set.

The `--no-with-guard` escape hatch (clap does not auto-derive
`--no-with-guard` for a bool field named `with_guard`, so the flag is
explicit) is the only way to bypass the guard. Substitution
substitution cannot be disabled because the probe is
informational-only.

## Consequences

- **Positive**: a stray KG match can never silently mutate a
  destructive command. The user is warned instead.
- **Positive**: the agent runtime can opt into substitution when it
  has user consent (one opt-in for the whole session, not per-call).
- **Positive**: the warnings array gives Claude Code something to
  surface in its reply, so the user has visibility into what would
  have been rewritten.
- **Negative**: tools that legitimately relied on silent substitution
  (e.g. `npm install` → `bun add` in a workflow that has user
  consent baked into the conversation) now require an explicit
  `--rewrite` flag on the hook invocation. This is a one-line
  configuration change in `~/.claude/settings.json`.
- **Negative / residual risk**: the warnings array is only as
  useful as the agent runtime's surfacing. If a future Claude Code
  version silently drops unknown fields, the warning is lost. The
  probe still runs, so the substitution never mutates the command
  regardless.

## Alternatives considered

- **Always-off substitution**: rejected — removes the KG-driven
  ergonomic benefit entirely. The thesaurus curation work would
  become inert at the hook boundary.
- **Always-on substitution, more warning**: rejected — does not
  fix the core problem (silent mutation of destructive commands).
- **Per-call opt-in via a sentinel in the command itself
  (e.g. `terraphim:rewrite npm install`)**: rejected — couples
  substitution to command syntax, which is brittle and would
  require the agent to insert the sentinel on every call.
- **Default-on substitution, explicit `--no-rewrite` opt-out**:
  rejected — keeps the failure mode intact. The default must be
  the safe behaviour; opt-in is the only way to flip the default
  safely.

## References

- Source: `crates/terraphim_agent/src/main.rs` (PreToolUse arm,
  `--rewrite` flag, `warnings` array)
- Tests: `crates/terraphim_agent/tests/hook_safety.rs` (Refs #126)
- Related ADRs:
  * ADR-002 — guard priority order
  * ADR-001 — release-signing key rotation
- Design doc: `docs/plans/design-terraphim-grep-agent-fixes-2026-08-30.md`
