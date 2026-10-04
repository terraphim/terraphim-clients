# Implementation Plan: #118 — get `main` green and make the failure class loud

**Status**: Draft
**Research**: `.docs/research/2026-08-29-ci-red-duplicate-config.md`
**Date**: 2026-08-29
**Estimated effort**: ~1 hour

## Summary

Two changes. Commit a verified `Cargo.lock` so CI builds the graph a human checked, and add a guard
that fails CI with a named error when duplicate `terraphim_*` crates appear.

## Approach

Research showed a cold resolve is correct and duplicate-free, and that the local lock is
byte-identical to it. The committed source is fine; only CI's environment differs, and the lock being
gitignored is what permits that. Committing it removes the degree of freedom. The guard exists
because the symptom (`expected ConfigState, found ConfigState`) reads as a code bug and has now cost
hours twice.

### Scope

**In:** un-ignore and commit `Cargo.lock`; `scripts/ci/check-no-duplicate-terraphim.sh`; one CI step.

**Out:** unyanking 1.20.4 on Gitea; changing the `=1.20.2` pins; the 0s runner-scheduling fault;
`--all-features` in CI (would hit #113).

**Avoid at all cost:**
- Widening the pins to `^1.20.4` — Gitea's 1.20.4 is **yanked**, so it would resolve to crates.io and
  reintroduce the duplicate this fixes
- Unyanking 1.20.4 without knowing why it was yanked
- `cargo update` in CI — that reintroduces the drift the lock removes
- Vendoring, or a second lock for CI

## Key design decisions

| Decision | Rationale | Rejected |
|---|---|---|
| Commit `Cargo.lock` | Repo ships binaries (`[[bin]] terraphim-agent`), Cargo's guidance for that case; `terraphim-ai` already does it; the ignore came from scaffolding `81ec742`, not a decision | `cargo update` in CI (keeps drift); vendoring (heavy) |
| Guard on `cargo tree -d`, not the lock file | `cargo tree -d` is the same command that diagnosed #112 and #118, and covers duplicates however they arise | Parsing `Cargo.lock` (reimplements cargo) |
| Guard scoped to `terraphim_*` | Third-party duplicates are normal and unfixable here; a global check would be noise and get disabled | Failing on any duplicate |
| Guard as its own CI step | Fails with a named message before clippy's confusing type error | Folding into an existing step |

### Simplicity check

The whole fix is: stop ignoring a file, and run one command in CI. The guard is a dozen lines of
shell around `cargo tree -d`. No new dependencies, no new abstractions, nothing speculative.

## File changes

| File | Change |
|---|---|
| `.gitignore` | remove the `Cargo.lock` line |
| `Cargo.lock` | **new** — the verified resolution (identical to a cold resolve, 0 duplicates) |
| `scripts/ci/check-no-duplicate-terraphim.sh` | **new** — the guard |
| `.gitea/workflows/native-ci.yml` | one step, before clippy |

### Guard contract

```bash
# scripts/ci/check-no-duplicate-terraphim.sh
# Exit 0  no terraphim_* crate appears at more than one version/source
# Exit 1  duplicates found; prints each crate and its versions
# Exit 2  cargo tree failed (environment problem, not a duplicate)
```

Placed **before** clippy: a duplicate makes clippy's output actively misleading, so the build should
stop with the real reason first.

## Test strategy

| Check | How |
|---|---|
| Guard passes on the current tree | run it — must exit 0 |
| Guard detects a real duplicate | temporary worktree with a manifest edit that reintroduces a crates.io copy; guard must exit 1 and name `terraphim_config` |
| Guard fails loudly, not silently, when cargo errors | run in a non-cargo directory; must exit 2, not 0 |
| Lock is the verified one | `git diff` after `cargo check` must be empty — the committed lock is already the resolved one |
| CI is actually green | push and read run status; this is the only check that tests the real hypothesis |

The last row matters most: every local check already passes, so only CI can confirm the fix.

## Steps

1. **Guard script + its checks** — write, run all three cases above.
2. **Un-ignore and commit the lock** — verify `cargo check` leaves it unmodified first.
3. **Wire the CI step**, push, read the run.

## Rollback

Re-add `Cargo.lock` to `.gitignore` and `git rm --cached` it; drop the CI step. Nothing else depends
on either.

## Open items

| Item | Status |
|---|---|
| Stale runner lock vs Linux-specific resolution | Unresolved; the fix covers both. If CI stays red, it is Linux-specific and the next step is a Linux cold resolve |
| 0s scheduling failures on runs 210-218 | Separate infrastructure fault, not tracked here |
