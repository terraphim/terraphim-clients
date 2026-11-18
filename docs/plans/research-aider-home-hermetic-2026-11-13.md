# Research Document: the Aider connector must not scan the current directory blindly (#397)

**Status**: Approved
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13
**Mode**: `remediation` (the defect and the expectation are both stated in the issue)

## Executive Summary

`terraphim-agent sessions list` imported an Aider session from **outside the
user's HOME** when `HOME` pointed at a temporary directory: the Aider connector
is the only connector that reads the **current working directory**, so an
isolated run's result depends on where it was launched. The fix makes the
connector read only a HOME-derived location - the current directory, but only
when it is inside HOME - which is what every other connector already does.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | A test run that reads the developer's real sessions is a test run that cannot be trusted ✓ |
| Leverages strengths? | Yes | The other connectors already resolve a HOME-derived path ✓ - this makes one of them consistent ✓ |
| Meets real need? | Yes | The issue's reproduction is concrete ✓ (HOME=tmp, an Aider session from outside HOME ✓) |

**Proceed**: Yes (3/3).

## Problem Statement

### Description

`AiderConnector::default_path()` returns `std::env::current_dir()`, and
`import()` falls back to it when the caller gives no path. Every other
connector resolves a HOME-derived location:

| Connector | Location |
|-----------|----------|
| claude-code (native) | `~/.claude/projects` |
| codex | `~/.codex/sessions` |
| cursor | `~/.config/Cursor/User` |
| cline | `~/.cline` |
| **aider** | **the current working directory** |

### Impact

- An isolated run (`HOME` pointed at a temporary directory) still imports
  sessions from the developer's real projects ✗.
- The result of `sessions list` depends on the directory it was launched in ✗.

### Success Criteria

- With `HOME` pointed at a temporary directory, no session outside it is
  imported ✓.
- A project **inside** HOME stays scannable ✓ (the feature survives).

## Current State Analysis

### Existing Implementation

`crates/terraphim_sessions/src/connector/aider.rs`:

```rust
fn default_path(&self) -> Option<PathBuf> {
    // Aider stores files in individual project directories
    // We use the current working directory as a starting point
    std::env::current_dir().ok()
}
```

and `import()`:

```rust
let base_path = options
    .path
    .clone()
    .or_else(|| self.default_path())
    .ok_or_else(|| anyhow::anyhow!("No path specified and default not found"))?;
```

### Why the cwd is the only candidate

Aider writes `.aider.chat.history.md` **into the project directory** ✓ - there
is no global location ✓ (the CLI's `--chat-history-file` defaults to the
project ✓). So the current directory is the only sensible candidate ✓; the
defect is not the candidate ✓ but that it is used **implicitly** ✓, outside
HOME ✓.

### Code Locations

| Component | Location | Purpose |
|-----------|----------|---------|
| The connector | `crates/terraphim_sessions/src/connector/aider.rs` | `default_path()` and `import()` |
| The explicit route | `crates/terraphim_sessions/src/connector/mod.rs::ImportOptions::path` | a caller that knows the directory passes it ✓ |
| The service | `crates/terraphim_sessions/src/service.rs::maybe_auto_import` | the implicit import that triggered the report |

## Constraints

### Technical Constraints

- Aider's histories are per-project ✓, so the connector cannot fall back to a
  global directory ✗.
- `std::env::current_dir()` is already canonical ✓, but `HOME` can be a
  symlink ✓ - so the comparison canonicalises both sides ✓.
- The test must not mutate the process environment ✓ (mutating it is `unsafe`
  ✓ and this package has suites that run concurrently ✓ - the `ci_guards`
  env-mutation arm enforces that ✓). So the rule is a **pure function** of two
  paths ✓ and the test asserts it with temporary directories ✓.

### Business Constraints

- The feature (picking up the current project's Aider history) must survive ✓
  for the ordinary case ✓.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| An isolated run reads nothing outside HOME | 0 sessions | every session under the cwd |
| A project inside HOME is still scanned | yes | yes |

## Vital Few (Essential Constraints)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| The implicit import reads only HOME-derived locations | that is what "hermetic" means for a HOME-scoped tool | the other connectors ✓ |
| The rule must be testable without the environment | mutating the environment is `unsafe` and racy | the `ci_guards` env-mutation arm ✓ |
| The current project must stay scannable | otherwise the feature is gone | the issue's first option ✓ |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| A CLI flag for the current project | the issue accepts either option ✓; the explicit route (`ImportOptions::path`) already exists ✓ |
| The other connectors | they already resolve HOME-derived locations ✓ |
| The shared `/tmp` persistence paths | a different defect (#513's family ✓) |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `ImportOptions::path` | the explicit route for a directory outside HOME | low ✓ - it exists ✓ |
| `dirs::home_dir()` | the HOME side of the comparison | low ✓ - the other connectors use it ✓ |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `dirs` | 5.0 | low | `std::env::home_dir` (deprecated ✓) |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| A project outside HOME stops being scanned | medium | the user loses the automatic pickup | the connector says so in the log ✓; the explicit route remains ✓ |
| A symlinked HOME breaks the comparison | low | the project is not scanned | both sides are canonicalised ✓ |
| A HOME that does not exist | low | nothing is scanned | the comparison falls back to the lexical form ✓ |

### Open Questions

1. Should the CLI grow a `--path` for the sessions commands? - The explicit
   route exists in the library ✓ (`ImportOptions::path` ✓); the CLI does not
   expose it ✓. A follow-up ✓, not a blocker ✓.
2. Does the agent's own session service pass a path? - It does not ✓ (the
   auto-import uses the default options ✓), so the fix is in the connector ✓.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| `dirs::home_dir()` honours `$HOME` on Linux | it reads the environment ✓ | the comparison would use the real home ✓ and the isolation would fail | Yes - the `dirs` crate reads `$HOME` ✓ |
| Aider keeps no global history | the CLI's default history file is the project's ✓ | a HOME-derived location would exist and be preferable | Yes - the connector's own comment says the files are per project ✓ |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| (a) The connector reads only a HOME-derived location | the cwd is one, but only inside HOME | **Chosen** - it satisfies the issue's first option ✓ and keeps the feature ✓ |
| (b) The cwd scan becomes an explicit flag | the cleanest separation, but a CLI change | Rejected for now - the issue accepts (a) ✓ and (b) is a follow-up ✓ |
| (c) The connector reads nothing by default | the Aider source disappears ✗ | Rejected - it removes a feature the issue did not ask to remove ✓ |

## Research Findings

### Key Insights

1. The Aider connector is the **only** connector that reads the cwd ✓ - the
   fix makes it consistent with the other five ✓.
2. The rule can be a pure function of two paths ✓, so the test needs no
   environment mutation ✓.
3. The explicit route already exists ✓ (`ImportOptions::path` ✓), so nothing
   has to be built for the "outside HOME" case ✓.

### Relevant Prior Art

- `crates/terraphim_sessions/src/connector/cline.rs::default_path` and
  `native.rs::default_path`: the HOME-derived pattern ✓ this change follows ✓.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| none | the rule is two lines ✓ | - |

## Recommendations

### Proceed

Proceed with the HOME-derived rule ✓, a pure helper ✓ and its test ✓.

### Scope Recommendations

- The connector's `default_path()` returns `None` outside HOME ✓.
- The connector logs why it is not scanning ✓ (so the behaviour is visible ✓).
- The explicit route stays ✓.

### Risk Mitigation Recommendations

- Canonicalise both sides ✓ (a symlinked HOME ✓ and a `..` component ✓).
- Keep the rule a pure function ✓ so it is testable without the environment ✓.

## Next Steps

1. Phase 2 (design): the rule, the helper's shape, the test.
2. Phase 3 (implementation): the connector, the helper, the test.
3. Phase 4 (verification): the sessions suite, clippy, fmt.
4. Phase 5 (validation): the issue's acceptance.

## Appendix

### Reference Materials

- The issue ✓.
- The connector's own comment: "Aider stores files in individual project
  directories" ✓.
