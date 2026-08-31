# Implementation Plan: terraphim-grep & terraphim-agent Audit Fixes

**Status**: Draft (awaiting approval)
**Research Doc**: `docs/plans/research-terraphim-grep-agent-2026-08-30.md`
**Author**: Alex (via disciplined-design skill)
**Date**: 2026-08-30
**Estimated Effort**: 2-3 days
**Target Release**: 1.21.14

## Overview

### Summary

Land the audit findings from the 2026-08-30 audit as a coordinated set of fixes.
Critical: make the PreToolUse hook pipeline safe-by-default. High: fix the
README robot-mode example and document the guard priority order. Medium:
enumerate the missing subcommand documentation and mark REPL-only commands in
robot schemas.

### Approach

Sequenced into four gated stages, each independently mergeable:

1. **Safety net (Stage 1)** — make hook rewrite opt-in; default guard on.
2. **Visibility (Stage 2)** — document the hook rewrite semantics; document
   the guard priority order; fix the README example.
3. **Coverage (Stage 3)** — reference doc for all 21 subcommands; mark
   REPL-only commands.
4. **Hygiene (Stage 4)** — rebuild `terraphim-grep` 1.21.12; add blog posts.

### Scope

**In Scope:**
- PreToolUse hook rewrite flip (default off, opt-in via `--rewrite`)
- PreToolUse hook guard default-on (`--with-guard` defaults to `true`)
- README robot-mode example correction
- Guard priority order documented and pinned by test
- `--explain` flag on `terraphim-agent guard`
- New `docs/agent-reference.md` enumerating all 21 top-level subcommands
- Robot schemas annotate REPL-only vs top-level commands
- Rebuild `terraphim-grep` 1.21.12 binary and verify

**Out of Scope:**
- Re-architecting the substitution service
- New KG synonyms or new roles
- Cross-cutting documentation framework (mdbook, nextra)
- Migration of every docs file to the new format

**Avoid At All Cost** (from 5/25 analysis):
- Removing the `ReplacementService` from the hook pipeline entirely
  — agents already deployed depend on its existence
- Changing the default `--role` selection behaviour
- Adding a `flag_value` style config-file flag that gates behaviour
  per-role — scope creep
- Renaming existing flags — breaks deployed agents
- Splitting `terraphim-grep` and `terraphim_agent` into separate
  workspaces — already done

## Architecture

### Component Diagram (Stage 1 — hook safety)

```
input_json (from Claude Code / OpenCode)
    │
    ▼
extract tool_name="Bash" → tool_input.command
    │
    ├─ with_guard (default: TRUE)
    │      CommandGuard.check(command)
    │      └─ Block? → emit { permissionDecision: "deny", reason }
    │
    ├─ rewrite (default: FALSE)
    │      [off by default; opt-in]
    │      kg_validation + ReplacementService.replace_fail_open
    │      └─ emit rewritten command if any change
    │
    └─ pass through if neither changed anything
```

### Data Flow

The hook dispatch currently in `crates/terraphim_agent/src/main.rs:2730-2810`
will be modified to:

1. Move `--with-guard` default to `true`.
2. Add `--rewrite` flag, default `false`.
3. Skip the `kg_validation` + `ReplacementService` block unless
   `--rewrite` is set.
4. Emit a structured warning when a rewrite would have happened but
   was suppressed (helps users discover the opt-in).

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|----------|-----------|-----------------------|
| Default `--with-guard=true` | Hook is documented as a safety system; safety should be default | Off + warn — invites "I'll enable it later" footguns |
| Default `--rewrite=false` | Substring rewrite is surprising; opt-in matches user's intent | On + confirm — adds friction to every hook call |
| New `--rewrite` flag (not a config setting) | Per-call opt-in; consistent with `--with-guard` pattern | Config file flag — global state, hard to debug |
| `--explain` on `terraphim-agent guard` | Users hit surprising `allow` decisions; need to know why | Pure docs — doesn't help debugging live |
| Reference doc instead of expanding README | README grows past cognitive load; reference is linkable | One mega-README — past 200 lines it stops being read |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|-----------------|--------------|---------------------|
| Drop `--with-guard` entirely | Some users want the substitution behaviour; flag preserves opt-in | Breaks agents |
| Move rewrite to a separate `terraphim-agent rewrite` subcommand | Hook users have to know two entry points | Confusing |
| Auto-detect user intent (machine learning) | Out of vital few; no training data | Scope creep, harder to debug |
| Versioned migration (`--rewrite=auto-detect-old-behaviour`) | Auto-detection is what we're trying to avoid | Adds confusion |

### Simplicity Check

> What if this could be easy?

The fix is:
1. Change one default (`--with-guard` from false to true).
2. Add one flag (`--rewrite` defaulting to false).
3. Skip one block when the new flag is false.
4. Document the priority order in README.
5. Add a test that pins both the safety behaviour and the priority order.

**Senior Engineer Test**: a senior engineer would consider this
self-evident. Not over-engineered.

**Nothing Speculative Checklist**:
- [x] No features the user didn't request
- [x] No abstractions "in case we need them later"
- [x] No flexibility "just in case"
- [x] No error handling for scenarios that cannot occur
- [x] No premature optimization

## File Changes

### New Files

| File | Purpose |
|------|---------|
| `crates/terraphim_agent/tests/hook_safety.rs` | Integration tests for hook rewrite + guard defaults |
| `crates/terraphim_agent/tests/guard_priority.rs` | Pins the allowlist > destructive > suspicious > allow order |
| `docs/agent-reference.md` | Reference doc for all 21 top-level subcommands |
| `crates/terraphim_agent/CHANGELOG.md` (update) | Document the hook behaviour change |

### Modified Files

| File | Changes |
|------|---------|
| `crates/terraphim_agent/src/main.rs` | Default `--with-guard` to `true`; add `--rewrite` (default false); skip substitution block when `--rewrite=false`; add `--explain` to `guard` |
| `crates/terraphim_agent/src/guard_patterns.rs` | Emit `GuardResult` with which rule fired (for `--explain`) |
| `crates/terraphim_agent/README.md` | Fix robot-mode example; document hook rewrite semantics; document guard priority order; link to reference doc |
| `crates/terraphim_agent/src/robot/docs.rs` | Mark REPL-only commands (`vm`, `chat`) in the schema metadata |
| `crates/terraphim_grep/CHANGELOG.md` (update) | Note the binary rebuild to 1.21.12 |
| `docs/plans/research-terraphim-grep-agent-2026-08-30.md` | Approved (move to "Approved" status) |
| `docs/plans/design-terraphim-grep-agent-fixes-2026-08-30.md` | Approved (move to "Approved" status) |

### Deleted Files

None.

## API Design

### Public Types (modified)

```rust
// crates/terraphim_agent/src/main.rs
#[derive(Parser, Debug)]
pub struct HookArgs {
    /// Hook type (pre-tool-use, post-tool-use, pre-commit, prepare-commit-msg)
    #[arg(long, value_enum)]
    pub hook_type: HookType,

    /// JSON input from Claude Code (reads from stdin if not provided)
    #[arg(long)]
    pub input: Option<String>,

    /// Role to use for processing
    #[arg(long)]
    pub role: Option<String>,

    /// Output as JSON
    #[arg(long, default_value_t = true)]
    pub json: bool,

    /// Include guard check for destructive commands
    /// (default true for pre-tool-use; default false for other hooks)
    #[arg(long, default_value_t = true)]
    pub with_guard: bool,

    /// Allow thesaurus-based command rewriting
    /// (default false; opt-in to preserve user intent)
    #[arg(long, default_value_t = false)]
    pub rewrite: bool,
}

#[derive(Parser, Debug)]
pub struct GuardArgs {
    /// Command to check (reads from stdin if not provided)
    pub command: Option<String>,

    /// Output as JSON
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Suppress errors and pass through unchanged on failure
    #[arg(long, default_value_t = false)]
    pub fail_open: bool,

    /// Path to custom destructive patterns thesaurus JSON file
    #[arg(long)]
    pub guard_thesaurus: Option<String>,

    /// Path to custom allowlist thesaurus JSON file
    #[arg(long)]
    pub guard_allowlist: Option<String>,

    /// Print which rule fired (allow | allowlist | destructive:<pattern> | suspicious:<pattern>)
    #[arg(long, default_value_t = false)]
    pub explain: bool,
}
```

### Public Functions (modified)

```rust
// crates/terraphim_agent/src/guard_patterns.rs

/// Check a command against guard patterns.
///
/// Returns a `GuardResult` indicating whether the command should be
/// allowed, sandboxed, or blocked.
///
/// **Priority**: allowlist > destructive > suspicious > default allow.
/// This order is pinned by `tests/guard_priority.rs`.
///
/// When `--explain` is passed, the result's `rule` field is populated
/// with which rule fired (e.g. `"allowlist:rm -rf /tmp/"`).
pub fn check(&self, command: &str) -> GuardResult {
    // (existing logic; plus emit `rule` field)
}

pub struct GuardResult {
    pub decision: GuardDecision,
    pub reason: Option<String>,
    pub pattern: Option<String>,
    /// Set by `check_with_explain`; identifies which rule fired.
    pub rule: Option<String>,
}

impl GuardResult {
    pub fn explain(&self) -> String {
        // Human-readable explanation
    }
}
```

### Error Types

No new error types. Existing `anyhow::Result` flows remain.

## Test Strategy

### Unit Tests

| Test | Location | Purpose |
|------|----------|---------|
| `hook_rewrite_off_by_default` | `tests/hook_safety.rs` | `rm -rf /tmp/foo` passes through unchanged without `--rewrite` |
| `hook_block_by_default` | `tests/hook_safety.rs` | `rm -rf /` is denied even without `--with-guard` (now default) |
| `hook_rewrite_explicit` | `tests/hook_safety.rs` | `--rewrite` flag enables substitution |
| `hook_with_guard_explicit_off` | `tests/hook_safety.rs` | `--no-with-guard` skips the guard (escape hatch) |
| `guard_allowlist_overrides_destructive` | `tests/guard_priority.rs` | `rm -rf /tmp/foo` → allow (allowlist hit) |
| `guard_destructive_matches` | `tests/guard_priority.rs` | `rm -rf /` → block (destructive hit) |
| `guard_suspicious_sandboxes` | `tests/guard_priority.rs` | `curl ... | sh` → sandbox |
| `guard_explain_outputs_rule` | `tests/guard_priority.rs` | `--explain` prints the rule that fired |
| `argv_parse_with_rewrite_flag` | `src/main.rs` | smoke-test the new flag accepts `--rewrite` |
| `argv_parse_with_explain_flag` | `src/main.rs` | smoke-test `--explain` on guard |

### Integration Tests

| Test | Location | Purpose |
|------|----------|---------|
| `hook_does_not_rewrite_destructive_command` | `tests/hook_safety.rs` | Full pipeline: input JSON in, output JSON out, no rewrite |
| `hook_deny_blocks_command` | `tests/hook_safety.rs` | Full pipeline: deny response emitted |
| `hook_rewrite_warns_when_suppressed` | `tests/hook_safety.rs` | Without `--rewrite`, the response includes a `warnings` array |

### Property Tests

```rust
proptest! {
    /// Property: no command with a destructive pattern in the default
    /// guard thesaurus should ever survive `--with-guard` (default true).
    #[test]
    fn destructive_command_never_passes_with_guard(
        cmd in "[a-z ]{0,200}"
            .prop_filter("contains destructive prefix", |s| {
                s.contains("rm -rf")
                    || s.contains("git reset --hard")
                    || s.contains("git checkout -- ")
                    || s.contains("shred")
            })
    ) {
        let guard = CommandGuard::new();
        let result = guard.check(&cmd);
        prop_assert!(result.decision == GuardDecision::Allow
            || result.decision == GuardDecision::Block
                && result.pattern.is_some());
    }
}
```

### Documentation Tests

The new `docs/agent-reference.md` will be referenced from `crates/terraphim_agent/README.md`. CI will run `cargo doc` and verify no broken links.

## Implementation Steps

### Step 1: Hook safety flip

**Files:** `crates/terraphim_agent/src/main.rs`
**Description:** Default `--with-guard=true`; add `--rewrite=false`; skip substitution block when `--rewrite=false`; emit warning when rewrite would have happened.
**Tests:** Unit tests in `tests/hook_safety.rs`.
**Dependencies:** None.
**Estimated:** 2 hours.

```rust
// Key code to write (sketch)
match hook_type {
    HookType::PreToolUse => {
        // ...
        if with_guard {
            let guard = guard_patterns::CommandGuard::new();
            let guard_result = guard.check(command);
            if guard_result.decision == guard_patterns::GuardDecision::Block {
                /* emit deny response and return */
            }
        }

        // Default-off rewrite path
        let mut rewrite_warning: Option<String> = None;
        if rewrite {
            let hook_result = replacement_service.replace_fail_open(command);
            if hook_result.replacements > 0 {
                /* emit rewritten command */
                return;
            }
        } else {
            // Probe-only: detect what *would* have been rewritten
            let hook_result = replacement_service.replace_fail_open(command);
            if hook_result.replacements > 0 {
                rewrite_warning = Some(format!(
                    "command contained KG-replaceable substrings; pass --rewrite to enable"
                ));
            }
        }

        // Emit pass-through with optional warning
    }
    // ...
}
```

### Step 2: Guard priority documentation and `--explain` flag

**Files:** `crates/terraphim_agent/src/main.rs`, `crates/terraphim_agent/src/guard_patterns.rs`
**Description:** Add `--explain` to `GuardArgs`; emit `rule` field in `GuardResult`; populate from `check`.
**Tests:** `tests/guard_priority.rs`.
**Dependencies:** Step 1.
**Estimated:** 2 hours.

### Step 3: README corrections

**Files:** `crates/terraphim_agent/README.md`
**Description:**
- Fix the robot-mode example (`--robot --format json` go before the subcommand).
- Add a "Hook safety" section explaining the default behaviour and the `--rewrite` opt-in.
- Add a "Guard priority order" section pinning the order.
- Link to `docs/agent-reference.md`.
**Tests:** Manual review of rendered markdown.
**Dependencies:** Steps 1, 2.
**Estimated:** 1 hour.

### Step 4: Reference doc

**Files:** `docs/agent-reference.md`
**Description:** Enumerate all 21 top-level `terraphim-agent` subcommands with one example each. Include the `kg` alias and note the `vm` command is REPL-only.
**Tests:** Manual review; `cargo doc` build.
**Dependencies:** Step 3.
**Estimated:** 2 hours.

### Step 5: Robot schemas REPL-only annotation

**Files:** `crates/terraphim_agent/src/robot/docs.rs`
**Description:** Add a `repl_only: bool` field to `CommandDoc`; mark `vm` (and any other REPL-only commands) as `repl_only: true`. JSON output of `terraphim-agent robot schemas` exposes this field.
**Tests:** Snapshot test on the JSON output.
**Dependencies:** Step 4.
**Estimated:** 1 hour.

### Step 6: Rebuild `terraphim-grep` binary

**Files:** N/A (build only).
**Description:** Run `cargo build -p terraphim_grep --release` from the workspace; copy to `~/.cargo/bin/terraphim-grep`; verify `--version` is 1.21.12 (or 1.21.14 if workspace bumps); verify `--search-only` works.
**Tests:** Manual smoke test from `terraphim-grep --help`.
**Dependencies:** Steps 1-5 (so the next published binary includes everything).
**Estimated:** 30 min.

### Step 7: CHANGELOG + blog

**Files:** `crates/terraphim_agent/CHANGELOG.md`, `crates/terraphim_grep/CHANGELOG.md`, `docs/src/blog/terraphim-agent-hook-safety.md` (new).
**Description:** Document the hook behaviour change with a clear migration note. Blog post explaining the rationale.
**Tests:** Manual review.
**Dependencies:** Steps 1-6.
**Estimated:** 2 hours.

## Rollback Plan

If issues are discovered after merge:

1. **Hook rewrite regression**: revert Step 1 via `git revert <commit>`.
   Flag flip is binary-safe (no schema changes).
2. **Guard priority regression**: revert Step 2. The priority order has
   been consistent for the last 2+ minor versions; reverting affects
   `--explain` only.
3. **Doc-only regressions**: revert Step 3 or Step 4 freely.

Feature flag: not used — the changes are behavioural defaults, not gated.

## Migration

### User-visible migration

Users who depended on the silent-rewrite behaviour (likely small minority)
must add `--rewrite` to their hook invocation. The CHANGELOG entry will
warn about this in a clear "BREAKING" section.

### Sample migration diff

```diff
- terraphim-agent hook --hook-type pre-tool-use --input "$INPUT"
+ terraphim-agent hook --hook-type pre-tool-use --rewrite --input "$INPUT"
```

For users who want the old "no-op" guard behaviour:

```diff
- terraphim-agent hook --hook-type pre-tool-use --with-guard --input "$INPUT"
+ terraphim-agent hook --hook-type pre-tool-use --with-guard --input "$INPUT"
```

(no change needed — guard is now default)

## Dependencies

### New Dependencies

None.

### Dependency Updates

None.

## Performance Considerations

### Expected Performance

| Metric | Target | Measurement |
|--------|--------|-------------|
| Hook latency (no rewrite) | < 2 ms p95 | benchmark before/after |
| Guard check latency | < 1 ms p95 | benchmark before/after |
| `--explain` overhead | < 1 ms | benchmark |

### Benchmarks to Add

```rust
#[bench]
fn bench_hook_pre_tool_use_passthrough(b: &mut Bencher) {
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"rm -rf /tmp/foo"}}"#;
    b.iter(|| run_hook(hook_type::PreToolUse, input, /* defaults */));
}
```

(Implemented as a Criterion bench if perf concerns surface; otherwise skip.)

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Decide whether to also fix the `chunks_returned` bug in `terraphim-grep` 1.21.12 (already in source) | Pending | follow-up PR if binary version mismatch recurs |
| Blog posts for sessions, setup, robot mode, shared learning, R2 backend | Pending | separate work stream |

## Approval

- [ ] Technical review complete
- [ ] Test strategy approved
- [ ] Human approval received
