# Implementation Plan: terraphim-grep Update Support

**Status**: Approved for implementation
**Research Doc**: `.docs/research-terraphim-grep-update.md`
**Author**: OpenCode
**Date**: 2026-07-04
**Estimated Effort**: 1-2 hours

## Overview

### Summary

Add explicit update support to `terraphim-grep` by reusing `terraphim_update`. The CLI will gain `check-update` and `update` subcommands while preserving the existing `terraphim-grep <QUERY>` search form.

### Approach

Use the same command pattern as `terraphim-agent`, but configure the updater for `terraphim/terraphim-clients` because that is where `terraphim-grep` releases are published.

### Scope

**In Scope:**
- `terraphim-grep check-update`.
- `terraphim-grep update`.
- Shared `UpdaterConfig::with_repo` helper.
- CLI tests for update command visibility and legacy query mode.

**Out of Scope:**
- Automatic background update checks.
- Binary asset build/signing pipeline.
- Custom install path selection.
- Updating other binaries.

**Avoid At All Cost**:
- Reimplementing update logic outside `terraphim_update`.
- Breaking `terraphim-grep <QUERY>`.
- Adding network calls to normal search.

## Architecture

### Component Diagram

```text
terraphim-grep CLI
  |-- legacy search path -> TerraphimGrep::search
  |-- check-update -----> UpdaterConfig -> TerraphimUpdater::check_update
  `-- update -----------> UpdaterConfig -> TerraphimUpdater::check_and_update
```

### Data Flow

```text
check-update -> config(bin=terraphim-grep, repo=terraphim/terraphim-clients, version=CARGO_PKG_VERSION) -> GitHub Releases -> status output
```

```text
search query -> existing role/thesaurus resolution -> existing grep search path
```

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|----------|-----------|----------------------|
| Add subcommands, keep optional query | Matches `terraphim-agent` and preserves old usage. | Flags-only update API. |
| Add `UpdaterConfig::with_repo` | Avoids mutating public fields directly in each binary and keeps configuration fluent. | Hardcode clients repo inside updater crate. |
| Do not auto-check on search startup | Avoids latency and network dependency on grep. | Background update check on every run. |

### Eliminated Options

| Option Rejected | Why Rejected | Risk of Including |
|-----------------|--------------|-------------------|
| Autoupdate on startup | Not requested; makes grep non-deterministic. | Slow or failed searches due to network. |
| New `terraphim-grep self` command tree | More structure than needed for two commands. | CLI complexity. |
| Release asset/signing work | Separate release pipeline concern. | Larger, riskier change. |

### Simplicity Check

The simplest design is a direct wrapper around `terraphim_update`, plus one repo override method. No speculative abstraction is needed.

**Nothing Speculative Checklist:**
- [x] No features the user did not request.
- [x] No extra update providers.
- [x] No install-path configuration yet.
- [x] No auto network calls during search.

## File Changes

### New Files

None.

### Modified Files

| File | Changes |
|------|---------|
| `crates/terraphim_update/src/lib.rs` | Add `UpdaterConfig::with_repo(owner, repo)`. |
| `crates/terraphim_grep/Cargo.toml` | Add `terraphim_update` dependency. |
| `crates/terraphim_grep/src/main.rs` | Add subcommand enum, updater helper, and early command handling. |
| `crates/terraphim_grep/tests/no_thesaurus_cli.rs` | Add CLI help/check-update visibility or legacy search guard if suitable. |

## API Design

### Shared Updater API

```rust
impl UpdaterConfig {
    pub fn with_repo(mut self, owner: impl Into<String>, name: impl Into<String>) -> Self;
}
```

### Grep CLI Types

```rust
#[derive(Subcommand, Debug)]
enum Command {
    CheckUpdate,
    Update,
}

#[derive(Parser, Debug)]
struct Args {
    query: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
    // existing search options unchanged
}
```

### Grep Helper

```rust
fn grep_updater() -> TerraphimUpdater;

async fn handle_update_command(command: Command) -> Result<()>;
```

## Test Strategy

### Unit Tests

| Test | Location | Purpose |
|------|----------|---------|
| `updater_config_accepts_repo_override` | `terraphim_update/src/lib.rs` tests | Verify helper changes owner/repo. |

### Integration Tests

| Test | Location | Purpose |
|------|----------|---------|
| `cli_runs_without_thesaurus` | Existing grep CLI test | Ensure legacy search still works. |
| `cli_help_lists_update_commands` | `crates/terraphim_grep/tests/no_thesaurus_cli.rs` | Ensure commands are exposed. |

### Manual Smoke Tests

```bash
terraphim-grep --help
terraphim-grep check-update
terraphim-grep "score_kg_boost" --haystack code --paths crates/terraphim_grep/src -C 1
```

## Implementation Steps

### Step 1: Shared Updater Config

**Files:** `crates/terraphim_update/src/lib.rs`
**Description:** Add fluent repo override.
**Tests:** Unit test asserting owner/repo fields change.

### Step 2: Grep Dependency

**Files:** `crates/terraphim_grep/Cargo.toml`
**Description:** Add workspace-local `terraphim_update` dependency with version metadata.
**Tests:** `cargo check -p terraphim_grep`.

### Step 3: Grep CLI Commands

**Files:** `crates/terraphim_grep/src/main.rs`
**Description:** Add subcommands and early handling before query-required search flow.
**Tests:** Help and existing search tests.

### Step 4: Verification

**Files:** tests only if needed.
**Description:** Run focused test suite and manual smoke commands.

## Rollback Plan

1. Revert the grep dependency and CLI command wiring.
2. Revert `UpdaterConfig::with_repo` if no other consumer uses it.
3. Existing search behaviour returns to the prior flat parser.

## Dependencies

### New Dependencies

| Crate | Version | Justification |
|-------|---------|---------------|
| `terraphim_update` | Local path, version metadata | Required shared update implementation. |

## Performance Considerations

Normal search should not call update code, so search performance remains unchanged. `check-update` and `update` are explicitly network-bound commands.

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Release binary assets for actual self-update install | Deferred | Release engineering |
| Configurable install path | Deferred | Future design |

## Approval

- [x] Technical review complete.
- [x] Test strategy defined.
- [x] Human request received.
