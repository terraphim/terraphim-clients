#!/usr/bin/env bash
#
# Fail the build when a terraphim_* crate appears at more than one version or
# source in the dependency graph.
#
# Two copies of the same crate mean two distinct copies of its types, and the
# compiler reports that as:
#
#     error[E0308]: mismatched types
#         expected `terraphim_config::ConfigState`, found `ConfigState`
#
# which reads like a bug in the calling code and is not. It has cost hours twice
# (#112, #118). This check names the real problem before clippy gets a chance to
# misdescribe it, so it runs first.
#
# Only terraphim_* crates are checked. Duplicate third-party crates are normal in
# a large graph and nothing here can fix them; failing on those would make this
# noisy and it would get switched off.
#
# Usage: scripts/ci/check-no-duplicate-terraphim.sh
#
# Exit: 0 clean, 1 duplicates found, 2 cargo could not produce a tree.
#
set -uo pipefail

# CHECK_TREE_FIXTURE lets the tests feed recorded `cargo tree -d` output in place
# of a live resolve. Real duplicates only arise transitively, and cargo refuses
# outright to resolve a *direct* dependency that would conflict, so a duplicate
# cannot be contrived on demand in a scratch workspace.
if [ -n "${CHECK_TREE_FIXTURE:-}" ]; then
    tree_out=$(cat "$CHECK_TREE_FIXTURE")
    status=$?
else
    tree_out=$(cargo tree --workspace --all-features --duplicates 2>/dev/null)
    status=$?
fi
if [ "$status" -ne 0 ]; then
    echo "check-no-duplicate-terraphim: 'cargo tree --duplicates' failed (exit ${status})" >&2
    echo "  this is an environment problem, not a duplicate; not treating it as a pass" >&2
    exit 2
fi

# `cargo tree -d` prints each duplicated package at column 0, one line per copy.
dupes=$(printf '%s\n' "$tree_out" | grep -E '^terraphim[_a-z-]* v' | sort -u)

if [ -z "$dupes" ]; then
    echo "check-no-duplicate-terraphim: ok, no terraphim crate is duplicated"
    exit 0
fi

echo "check-no-duplicate-terraphim: FAIL -- terraphim crates present at more than one version:" >&2
printf '%s\n' "$dupes" | sed 's/^/  /' >&2
cat >&2 <<'HINT'

Every terraphim_* dependency must resolve to a single version from the Gitea
registry. A crates.io copy usually creeps in when:

  - a dependency names a version the [patch.crates-io] entry does not satisfy
    (an exact `=x.y.z` pin does not satisfy a `^x.y.w` requirement, and cargo
    falls back to crates.io silently rather than erroring), or
  - a manifest omits `registry = "terraphim"`, so it resolves against crates.io.

Run `cargo tree -i <crate>@<version>` to see which package pulls the stray copy.
HINT
exit 1
