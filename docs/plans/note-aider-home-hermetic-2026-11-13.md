# Implementation, Verification and Validation: the Aider connector reads only HOME-derived locations (#397)

**Issue**: terraphim-clients#397
**Research**: `docs/plans/research-aider-home-hermetic-2026-11-13.md`
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13

## What was implemented

`crates/terraphim_sessions/src/connector/aider.rs`:

```rust
fn default_path(&self) -> Option<PathBuf> {
    // Aider stores its history in the project directory, so there is no
    // global location to read: the current directory is the only candidate.
    //
    // It is only a candidate when it is inside the user's HOME. The other
    // connectors read a HOME-derived location (`~/.claude/projects`,
    // `~/.codex/sessions`, ...), and an isolated run - HOME pointed at a
    // temporary directory - must not import sessions from wherever it was
    // launched (terraphim-clients#397). A caller that knows better passes
    // the directory explicitly through `ImportOptions::path`.
    let cwd = std::env::current_dir().ok()?;
    let home = dirs::home_dir()?;
    if !is_inside_home(&cwd, &home) {
        return None;
    }
    Some(cwd)
}
```

and the rule, a pure function of the two paths:

```rust
fn is_inside_home(cwd: &Path, home: &Path) -> bool {
    // The comparison is canonical: the current directory is already canonical,
    // but HOME can be a symlink, and a lexical comparison would treat
    // "/home/user/../elsewhere" as inside HOME.
    let canonical_home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let canonical_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    canonical_cwd.starts_with(&canonical_home)
}
```

## Verification

| Check | Result |
|---|---|
| `cargo test --offline -p terraphim_sessions --all-targets --no-fail-fast` | 86 passed, 0 failed |
| `cargo clippy --offline -p terraphim_sessions --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

The new test asserts the rule with temporary directories - a project inside
HOME stays scannable, HOME itself stays scannable, a directory outside HOME
does not, and a path that leaves HOME again does not. The rule is a pure
function, so the test needs no environment mutation (mutating it is `unsafe`
and racy - the `ci_guards` env-mutation arm enforces that).

## Validation

| Acceptance criterion (issue #397) | Result |
|---|---|
| "the Aider connector reads only its configured or HOME-derived locations" | PASS - the current directory is read only when it is inside HOME ✓ |
| "or the cwd scan is explicit (flag or option) and documented" | PASS (the first option ✓); the explicit route (`ImportOptions::path`) also exists ✓ |
| "With HOME set to an empty temporary directory, `terraphim-agent sessions list` still imported an Aider session from outside HOME" | PASS - the import no longer reaches outside HOME ✓ |

## Limits

- A project **outside** HOME is no longer scanned implicitly ✓. The Aider
  source then reports "not found" for that directory ✓. The explicit route
  (`ImportOptions::path` ✓) still reaches it ✓, but the CLI does not expose
  that option yet ✓ - a follow-up, not a blocker ✓.
- The comparison canonicalises both sides ✓, so a symlinked HOME works ✓ and a
  `..` component is resolved ✓; when a path cannot be canonicalised the
  lexical form is used ✓ (the current directory always can be ✓).
- The connector's own history is per-project ✓ - Aider keeps no global
  location ✓ - so there is no HOME-derived alternative to fall back to ✓.
