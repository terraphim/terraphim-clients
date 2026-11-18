# Research, Design, Verification and Validation: destructive commands are recognised by their shape (#511)

**Issue**: terraphim-clients#511
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defects with measured evidence, local change)

## Research

### The gap

The destructive dictionary is a list of **literal strings**:

```
rm -rf, rm -fr, rm -rfi, rm -fri,
git push --force, git push -f,
git clean -f, git clean -fd, ...
```

Aho-Corasick matches those substrings, so every equivalent spelling that
shares no substring with an entry was allowed. The issue measured, against the
real guard:

| Command | Verdict | Why it slipped through |
|---|---|---|
| `rm --recursive --force /` | allow | no literal `rm -rf` |
| `rm -r --force /` | allow | no literal `rm -rf` |
| `rm -R -f /` | allow | no literal `rm -rf` |
| `rm --force --recursive /` | allow | no literal `rm -rf` |
| `git push origin +main` | allow | the `+` refspec is not in the dictionary |
| `git push origin main:main --force` | allow | `--force` is present but not preceded by `git push` |
| `find . -delete` | allow | find's own deletion flags are not in the dictionary |
| `find . -name '*.rs' -delete` | allow | same |
| `find . -type f -exec rm {} ;` | allow | only `rm -rf` inside the text would match |

The first four delete a directory tree without asking; the last three delete
files without asking; the force push overwrites the remote history.

### The fourth finding (git clean flag clusters) is not a gap

The issue reports 19 further force-bearing `git clean` clusters that are not
in the dictionary, and then records the empirical finding that materially
changes the risk: git's dry-run flag always wins over `-f`, and git's
`clean.requireForce` refuses clusters where `-f` is not the final force token.
So those clusters either dry-run or fail in git itself - there is nothing for
the guard to add. That finding is not implemented here.

### The allowlist already parses its commands

`safe_tmp_cleanup` reads the flags of an `rm` invocation (`-r`, `-R`,
`--recursive`, `-f`, `--force`) and refuses the temporary **root**. The
allowlist side already decides by *parsed shape*; only the destructive side
still matched literal strings. The fix makes the two sides symmetric.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The safe cleanup must stay allowed | The shape check consults the same concept |
| C2 | A conditional force is not a force | `--force-with-lease` is not a force push |
| C3 | The check must not fire on a quoted string | The command is tokenised, quotes resolved |
| C4 | A chained command is still a command | The command is split at the control operators |

## Design

**`crates/terraphim_agent/src/guard_patterns.rs`**

1. `rm_shape(words) -> Option<RmShape>` parses an `rm` invocation into
   `recursive`, `force`, the targets, and whether an unrecognised long flag was
   present. `is_tmp_cleanup` now uses it (same behaviour), and
   `is_recursive_force_delete` uses it for the destructive side.

2. `is_unconditional_force_push(words)`: a `git push` with `--force`, `-f`, or a
   `+` refspec. `--force-with-lease` (and `--force-with-lease=<ref>`) is
   explicitly *not* a force.

3. `find_deletes(words)`: a `find` with `-delete`, or with `-exec`/`-ok`/
   `-execdir` running `rm` (with or without a path).

4. `split_command_segments(command)` splits a command at the control operators
   (`;`, `&&`, `||`, `|`, newline) outside quotes, and `tokenise_segment`
   resolves the quotes and backslash escapes of each segment. Expansions are
   kept as ordinary characters: the shape of a command does not depend on what
   a target expands to.

5. `destructive_by_shape(command)` runs the three checks over the segments and
   returns the pattern and the reason. The destructive stage blocks on it when
   the dictionary did not match.

The allowlist keeps its priority: the stage order is still
allowlist > destructive > suspicious > default, so a command the allowlist
concept permits never reaches the shape check - and where the allowlist's
entries do not match the spelling (`rm --recursive --force /tmp/foo` shares no
substring with the `rm -rf /tmp/` entry), the shape check asks the same
`safe_tmp_cleanup` concept before it blocks.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1231 passed, 0 failed |
| `cargo test -p terraphim_agent --lib guard_patterns` | 69 passed |
| `cargo test -p terraphim_agent --test guard_priority` | 10 passed |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

Measured verdicts through the real binary:

| Command | Before | After |
|---|---|---|
| `rm --recursive --force /` | allow | **block** |
| `rm -r --force /` | allow | **block** |
| `rm -R -f /` | allow | **block** |
| `rm --force --recursive /` | allow | **block** |
| `rm --recursive --force $HOME` | allow | **block** |
| `git push origin +main` | allow | **block** |
| `git push origin main:main --force` | allow | **block** |
| `find . -delete` | allow | **block** |
| `find . -name '*.rs' -delete` | allow | **block** |
| `find . -type f -exec rm {} ;` | allow | **block** |
| `find . -exec /bin/rm {} +` | allow | **block** |
| `git push --force-with-lease origin main` | allow | allow |
| `git push origin feature` | allow | allow |
| `find . -name '*.rs' -print` | allow | allow |
| `rm -rf /tmp/foo` | allow | allow |
| `rm --recursive --force /tmp/foo` | allow | allow |
| `rm -rf build` | block | block |

New tests: six unit tests over the shape checks (the recursive force deletes,
the temporary cleanup in every spelling, the unconditional force pushes, the
conditional force push, `find` that deletes, `find` that only reads) and two
integration tests through the real binary.

## Validation

| Acceptance criterion (issue #511) | Result |
|---|---|
| "normalise the command into argv before matching, and match on the parsed flags (recursive AND force)" | PASS - `rm_shape` reads the flags the way `rm` does |
| "add `git push origin +` style coverage, or normalise argv and detect a leading '+' on the refspec" | PASS |
| "add find -delete and find -exec rm coverage, or normalise argv and detect the deletion flags" | PASS |
| the git clean flag clusters | NOT IMPLEMENTED - git's own `clean.requireForce` and dry-run precedence already make those clusters harmless, as the issue's own measurement records |

## Limits

- A recursive delete **without** force (`rm -r /`) is still not blocked: the
  dictionary has never covered it, and `rm` asks for confirmation. In an agent
  context there is nobody to ask, so this is a deliberate, documented gap
  rather than an oversight.
- `xargs rm -rf` and `sh -c "rm -rf /"` are not covered: the shape checks read
  the command, not what it hands to another program.
- The shape checks are lexical. They cannot see through a wrapper script, an
  alias, or a variable that expands to a destructive path.
- The git clean clusters are left to git's own semantics, as the issue's
  measurement found.
