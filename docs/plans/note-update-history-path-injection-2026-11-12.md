# Research, Design, Verification and Validation: the update-history path takes an explicit root (#438)

**Issue**: terraphim-clients#438 (the last environment-mutation site in the merged tree)
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect, one file, explicit acceptance)

## Research

### The remaining site

The unsafe-audit train removed every environment-mutation site from
`terraphim_grep` (#492), the MCP stdio test (#493) and the `terraphim_update`
`managed_mode` suite (#503, moved into its own one-test binary). One real site
was left:

```
crates/terraphim_update/src/state.rs
    unsafe {
        std::env::set_var("HOME", ...);
        std::env::set_var("USERPROFILE", ...);
        std::env::set_var("XDG_CONFIG_HOME", ...);
        std::env::set_var("APPDATA", ...);
        std::env::set_var("LOCALAPPDATA", ...);
    }
```

The helper `setup_temp_config_dir` set five environment variables so that
`dirs::config_dir()` - and therefore `get_history_path()` - resolved inside a
temporary directory.

### Why it is unsafe here

`std::env::set_var` is `unsafe` because it is not thread safe: it mutates the
process environment while other threads may be reading it. The
`terraphim_update` crate has **141 tests** and only a handful of them are
`#[serial]`, so the writer raced with unrelated readers in the same binary -
exactly the mechanism #374 documented for `managed_mode.rs`.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The tests need the history file in a temporary directory | The path must be injectable |
| C2 | The public API must keep working unchanged | The public functions delegate to the new `_in` variants |
| C3 | `dirs::config_dir()` is what production uses | Production keeps using it; only the tests stop |

## Design

`crates/terraphim_update/src/state.rs` gains explicit-path variants, and the
existing functions delegate to them:

| Public | Explicit-path variant |
|---|---|
| `get_history_path()` | `get_history_path_in(config_root: &Path)` |
| `save_update_history(&history)` | `save_update_history_in(&history, &path)` |
| `load_update_history()` | `load_update_history_in(&path)` |
| `delete_update_history()` | `delete_update_history_in(&path)` |
| `history_exists()` | `history_exists_in(&path)` |

The test helper now only creates a temporary directory; the tests pass the path
explicitly. **No environment variable is written any more** - the site is gone
rather than serialised.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_update --all-targets --no-fail-fast` | 230 passed, 3 failed - the three pre-existing downloader failures (#502) |
| `cargo test -p terraphim_update --lib state` | 11 passed |
| `cargo clippy -p terraphim_update --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

The inventory after this change - environment mutation under `crates/`:

```
$ grep -rn "env::set_var\|env::remove_var" --include=*.rs crates/ \
    | grep -v terraphim_agent/tests/ci_guards.rs \
    | grep -v terraphim_test_utils
crates/terraphim_update/tests/managed_mode_home_env.rs:48:  std::env::set_var("HOME", ...)
crates/terraphim_update/tests/managed_mode_home_env.rs:66:  ... set_var("HOME", home)
crates/terraphim_update/tests/managed_mode_home_env.rs:67:  ... remove_var("HOME")
```

The only remaining sites are the three lines in the **one-test binary** created
by #503, which the `ci_guards` arm
`home_mutating_test_file_holds_a_single_test` keeps at exactly one test.

## Validation

| Acceptance criterion (issue #438, "eliminate test-only env set_var unsafe") | Result |
|---|---|
| No environment mutation in a shared test binary | PASS - the `state.rs` helper writes nothing; the remaining sites are in a one-test binary |
| Production behaviour unchanged | PASS - the public functions still resolve the path with `dirs::config_dir()` |
| Tests still exercise the real save/load/delete round trip | PASS - the same operations run against the temporary path |

## Limits

- The one-test binary still mutates `HOME`; that is a deliberate trade-off
  (the alternative was plumbing a home root through the update-check path), and
  it is now guarded rather than assumed.
- The unsafe-audit epic (#438) cannot be closed by this change alone: the
  remaining children (#442, #443, #444) name code that does not exist on
  `main` (it lives on the unmerged coding-agent branch, PR #432), and the
  `ci_guards` arm from #499 stops the pattern from spreading.
