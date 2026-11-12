# Research, Design, Verification and Validation: the allowlist cannot exempt a destructive command (#510)

**Issue**: terraphim-clients#510
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect, measured, local change)

## Research

### The defect

The guard's allowlist stage exempts a command before the destructive stage
runs. Since #485 (the #462 fix) an entry exempts a command when either

- the whole command **is** that literal entry (whitespace-insensitive), or
- the command parses as that entry's **safe operation** and nothing else.

The first branch short-circuits before the second and performs no safety
evaluation at all. The shipped allowlist carries literal entries:

```json
"rm -rf /tmp/": { "nterm": "safe_tmp_cleanup", ... }
"rm -fr /tmp/": { "nterm": "safe_tmp_cleanup", ... }
"rm -rf /var/tmp/": { "nterm": "safe_tmp_cleanup", ... }
"rm -fr /var/tmp/": { "nterm": "safe_tmp_cleanup", ... }
```

So `rm -rf /tmp/` matched its own allowlist entry literally and was **allowed**
- while `rm -rf /tmp` was blocked. The two delete the same thing.

### Measured on this host, before the fix

```
$ terraphim-agent guard --json <<< 'rm -rf /tmp/'
allow
$ terraphim-agent guard --json <<< 'rm -rf /tmp'
block
```

### The concept was already right

The concept check (`is_tmp_cleanup`) refuses the temporary **root**:

```rust
let root = TEMP_ROOTS.iter().find(|root| path.starts_with(root)).copied()?;
// `rm -rf /tmp` itself is never "cleanup of a temporary directory".
if path == Path::new(root) {
    return Some(false);
}
```

`Path::new("/tmp/") == Path::new("/tmp")` is **true** - Rust's `Path` equality
compares components, and a trailing slash is not a component. So the concept
already refused `rm -rf /tmp/`; the literal comparison simply never let it
speak.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The two spellings delete the same thing | They must get the same verdict |
| C2 | The concept defines the safe family | The concept must be consulted, not bypassed |
| C3 | A full-command custom entry must keep working | The literal comparison stays, with a narrower job |
| C4 | The allowlist must not become a rubber stamp | A stale entry may not exempt what the destructive stage blocks |

## Design

The destructive matches are now computed **once, before** the allowlist stage,
so the allowlist stage knows what the destructive stage thinks about the
command:

```rust
let destructive_matches = find_matches(command, &self.destructive_thesaurus, false);
let destructive_blocks = matches!(destructive_matches, Ok(ref matches) if !matches.is_empty());
```

and the literal comparison may only exempt a command the destructive stage does
not block:

```rust
.find(|found| {
    let literal = normalize_whitespace(&found.term) == normalized_command;
    let concept = allowlist_concept_permits(&found.normalized_term.value.to_string(), command);
    (literal && !destructive_blocks) || concept
})
```

The **concept** branch is unchanged: `rm -rf /tmp/foo` is still allowed (the
concept permits a path strictly inside the root), and `rm -rf /tmp/` is now
decided by the concept as well - which refuses it.

One supporting change: `is_git_unstage` accepted only `git restore --staged
<paths...>` (four words). The shipped allowlist entry is the *path-less*
spelling `git restore --staged`, and the destructive dictionary contains
`git restore ` - so with the literal comparison narrowed, the path-less form
would have fallen through to the destructive stage. The concept now accepts
the path-less form too (it unstages nothing), which keeps the shipped entry
meaningful.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1222 passed, 0 failed |
| `cargo test -p terraphim_agent --lib guard_patterns` | 63 passed |
| `cargo test -p terraphim_agent --test guard_priority` | 8 passed |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

Measured verdicts after the fix:

| Command | Before | After |
|---|---|---|
| `rm -rf /tmp/` | allow | **block** |
| `rm -fr /tmp/` | allow | **block** |
| `rm -rf /var/tmp/` | allow | **block** |
| `rm -rf /tmp` | block | block |
| `rm -rf /tmp/foo` | allow | allow |
| `rm -fr /tmp/a/b/c` | allow | allow |
| `git checkout -b topic` | allow | allow |
| `git restore --staged` | allow | allow |
| `git restore --staged file.txt` | allow | allow |
| `git restore --worktree file.txt` | block | block |
| `git clean -n` | allow | allow |
| `git clean --dry-run` | allow | allow |
| `git push --force-with-lease origin main` | allow | allow |
| `git push --force origin main` | block | block |

New tests:

- `guard_patterns.rs`: the temporary root with a trailing slash is blocked
  (`rm -rf /tmp/`, `rm -fr /tmp/`, `rm -rf /var/tmp/`, `rm -fr /var/tmp/`,
  `rm -rf /tmp//`), and a literal allowlist entry cannot exempt a destructive
  command while the force-with-lease concept keeps working.
- `guard_priority.rs` (through the real binary): the temporary root is blocked
  with or without a trailing slash; the trace shows the allowlist stage
  *continuing* instead of short-circuiting; a cleanup inside the root stays
  allowed.

## Validation

| Acceptance criterion (issue #510) | Result |
|---|---|
| "`rm -rf /tmp/` ALLOWED, `rm -rf /tmp` blocked" | PASS - both are blocked now |
| "the guard's own documented root rejection not running" | PASS - the concept check runs for every shipped entry |
| "the two spellings are semantically identical" | PASS - they get the same verdict |

## Limits

- A **bespoke** allowlist entry for a destructive command (an entry whose
  concept the guard does not know, naming a destructive command verbatim) is no
  longer honoured: the destructive stage wins. That is the point of the fix -
  the allowlist names safe operation families, not destructive commands - but a
  user who relied on such an entry has to change the command instead.
- The destructive **dictionary** still matches literal strings, so the coverage
  gaps of #511 (long-form `rm --recursive --force`, `find -delete`, the force
  refspec) are untouched by this change.
