#!/usr/bin/env bash
#
# Refuse to publish a crate unless its source is reproducible.
#
# Four of the last four terraphim_* publishes could not be traced back to a
# commit: two recorded `"dirty": true` in .cargo_vcs_info.json (uncommitted
# changes at publish time, so the published bytes match no commit anywhere),
# and three were built from SHAs unreachable from main -- one orphaned when
# main was force-reset, one absent from every repo on disk.
#
# `cargo publish` writes whatever it is given. This gate is the check that
# should have run first. It fails closed.
#
# Usage:
#   scripts/publish-gate.sh <crate-name> [remote-ref]
#
# Checks, in order:
#   1. clean worktree   -- otherwise .cargo_vcs_info.json records dirty: true
#   2. HEAD is tagged   -- gives the published artefact a durable name
#   3. HEAD is reachable from the remote ref (default origin/main)
#                       -- survives a later force-reset of the branch
#
# Exit codes: 0 pass, 1 a check failed, 2 usage/environment error.
#
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "Usage: $0 <crate-name> [remote-ref]" >&2
    exit 2
fi

CRATE="$1"
REMOTE_REF="${2:-origin/main}"

fail() { echo "publish-gate: FAIL: $*" >&2; exit 1; }
pass() { echo "publish-gate: ok: $*"; }

git rev-parse --git-dir >/dev/null 2>&1 || { echo "publish-gate: not a git repository" >&2; exit 2; }

# The crate must exist in this workspace, otherwise the gate is checking
# provenance for something it is not about to publish.
cargo metadata --no-deps --format-version 1 2>/dev/null \
    | grep -q "\"name\":\"${CRATE}\"" \
    || { echo "publish-gate: '${CRATE}' is not a member of this workspace" >&2; exit 2; }

# 1. Clean worktree. Both the index and the working tree must be clean, and no
#    untracked files may exist -- cargo packages untracked files too, so they
#    would end up in the artefact without appearing in any commit.
git update-index -q --refresh || true
git diff-index --quiet HEAD -- || fail "worktree has uncommitted changes; cargo would record dirty: true"
[ -z "$(git ls-files --others --exclude-standard)" ] \
    || fail "untracked files present; cargo packages them but no commit contains them"
pass "worktree is clean"

# 2. HEAD carries a tag.
TAG="$(git describe --exact-match --tags HEAD 2>/dev/null || true)"
[ -n "$TAG" ] || fail "HEAD $(git rev-parse --short HEAD) is not tagged; tag the release commit first"
pass "HEAD is tagged $TAG"

# 3. HEAD is reachable from the remote ref. An unreachable publish commit is
#    exactly what happened to terraphim_agent 1.21.2.
git rev-parse --verify --quiet "$REMOTE_REF" >/dev/null \
    || { echo "publish-gate: ref '${REMOTE_REF}' not found; fetch first" >&2; exit 2; }
git merge-base --is-ancestor HEAD "$REMOTE_REF" \
    || fail "HEAD is not an ancestor of ${REMOTE_REF}; push it first or it will be orphaned by a reset"
pass "HEAD is reachable from ${REMOTE_REF}"

echo "publish-gate: ${CRATE} @ ${TAG} ($(git rev-parse --short HEAD)) is safe to publish"
