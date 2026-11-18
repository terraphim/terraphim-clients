# Research, Design, Verification and Validation: the cleanup-script guard must not depend on a hardcoded shell (#498)

**Issue**: terraphim-clients#498
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect, local change, no high-risk surface)

## Research

### Problem

`ci_guards::cleanup_script_only_signals_verified_checkout_processes` failed in its own harness, before it checked anything about the cleanup script:

```
copying test executable failed: exit status: 1
cp: cannot stat '/usr/bin/dash': No such file or directory
```

The test hardcodes `/usr/bin/dash` as the stand-in for `terraphim_server`. `dash` is not installed everywhere - on this machine neither `/usr/bin/dash` nor `/bin/dash` exists (`/bin/sh` is bash) - so the guard could not run at all, and the failure read like a defect in the script it was supposed to guard.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The stand-in must accept `-c <script>`, stay alive and handle TERM | Any POSIX shell qualifies |
| C2 | The guard must not fail for a machine without `dash` | Resolve a shell from candidates; skip with a notice if none exists |
| C3 | The copy must not leave the destination busy (ETXTBSY) | Keep the copy in a separate process (an in-process `std::fs::copy` reproduced ETXTBSY) |
| C4 | The `/proc/<pid>/exe` comparison must survive `/bin` being a symlink | Compare canonicalised paths, not raw strings |

### Options considered

| Option | Implication | Chosen |
|--------|-------------|---------|
| Install `dash` | Not our machine to fix; the guard must run anywhere | No |
| Hardcode `/bin/sh` | Same class of bug on a distro without it | No |
| Resolve a shell from candidates, skip when none exists | Works everywhere, and the guard's real subject (the cleanup script) still gets covered | **Yes** |

## Design

1. `stand_in_shell()` returns the first existing candidate among
   `/usr/bin/dash`, `/bin/dash`, `/usr/bin/sh`, `/bin/sh`; the test skips with a
   notice when none exists (the repository's convention, see #466).
2. The copy stays in a separate process (`cp`); the error message names the
   stand-in instead of "test executable".
3. The `/proc/<pid>/exe` assertion compares canonicalised paths, so
   `/bin/sleep` -> `/usr/bin/sleep` (or the reverse) does not matter.

No new fields, no new dependencies, no production code touched.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --test ci_guards` | 24 passed, 0 failed |
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | **1198 passed, 0 failed** (the whole package is green locally for the first time in this session; it started at 23 failures) |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

An in-process `std::fs::copy` was tried first and produced `ETXTBSY` when the
copied stand-in was started; the subprocess copy is kept for that reason, and
the reason is now documented in the code.

## Validation

| Acceptance criterion (issue #498) | Result |
|---|---|
| "cargo test -p terraphim_agent --test ci_guards passes on a machine without a special setup" | PASS - the guard suite is green here |
| "the failure message names the cause instead of 'exit status: 1'" | PASS - the message names the stand-in executable and the copy |
| "the guard still fails when the cleanup script stops signalling the wrong processes" | UNCHANGED - the assertions on the script's behaviour are untouched; only the stand-in selection and the path comparison changed |

## Limits

- The test still needs `cp` on `PATH`. Where it is missing the guard fails with
  a message that names the cause; replacing `cp` with a fork-free copy would
  reintroduce ETXTBSY, so a future change should copy in a child process
  written in Rust rather than shelling out.
- If a machine has no POSIX shell at all, the guard skips rather than fails.
