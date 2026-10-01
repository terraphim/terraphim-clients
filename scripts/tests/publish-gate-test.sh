#!/usr/bin/env bash
#
# Tests for scripts/publish-gate.sh.
#
# Builds throwaway git repositories in a temp dir and asserts the gate's
# verdict for each provenance failure it exists to catch. No network, no
# registry, no mutation of the repo under test.
#
# Usage: scripts/tests/publish-gate-test.sh
#
set -uo pipefail

GATE="$(cd "$(dirname "$0")/../.." && pwd)/scripts/publish-gate.sh"
[ -x "$GATE" ] || { echo "gate not executable at $GATE" >&2; exit 2; }

pass=0; fail=0
check() { # check <name> <expected-exit> <actual-exit>
    if [ "$2" -eq "$3" ]; then printf '  ok    %s\n' "$1"; pass=$((pass+1))
    else printf '  FAIL  %s (expected exit %s, got %s)\n' "$1" "$2" "$3"; fail=$((fail+1)); fi
}

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# A minimal single-crate workspace whose crate is named `demo_crate`.
scaffold() {
    local d="$1"
    mkdir -p "$d/src"
    cat > "$d/Cargo.toml" <<'TOML'
[package]
name = "demo_crate"
version = "0.1.0"
edition = "2021"
TOML
    echo 'pub fn f() {}' > "$d/src/lib.rs"
    git -C "$d" init -q
    git -C "$d" config user.email t@example.com
    git -C "$d" config user.name Test
    git -C "$d" add -A
    git -C "$d" commit -qm "initial"
}

# origin/main present and HEAD tagged and reachable -> pass
R="$WORK/happy"; scaffold "$R"
git -C "$R" tag v0.1.0
git -C "$R" update-ref refs/remotes/origin/main HEAD
( cd "$R" && "$GATE" demo_crate >/dev/null 2>&1 ); check "clean + tagged + reachable passes" 0 $?

# uncommitted modification -> reject (this is the dirty: true case)
R="$WORK/dirty"; scaffold "$R"
git -C "$R" tag v0.1.0
git -C "$R" update-ref refs/remotes/origin/main HEAD
echo 'pub fn g() {}' >> "$R/src/lib.rs"
( cd "$R" && "$GATE" demo_crate >/dev/null 2>&1 ); check "dirty worktree is rejected" 1 $?

# untracked file -> reject (cargo packages it, no commit contains it)
R="$WORK/untracked"; scaffold "$R"
git -C "$R" tag v0.1.0
git -C "$R" update-ref refs/remotes/origin/main HEAD
echo 'stray' > "$R/src/stray.rs"
( cd "$R" && "$GATE" demo_crate >/dev/null 2>&1 ); check "untracked file is rejected" 1 $?

# no tag -> reject
R="$WORK/untagged"; scaffold "$R"
git -C "$R" update-ref refs/remotes/origin/main HEAD
( cd "$R" && "$GATE" demo_crate >/dev/null 2>&1 ); check "untagged HEAD is rejected" 1 $?

# HEAD ahead of origin/main -> reject (the orphaned-commit case)
R="$WORK/unreachable"; scaffold "$R"
git -C "$R" update-ref refs/remotes/origin/main HEAD
echo 'pub fn h() {}' >> "$R/src/lib.rs"
git -C "$R" commit -qam "later work"
git -C "$R" tag v0.1.1
( cd "$R" && "$GATE" demo_crate >/dev/null 2>&1 ); check "HEAD unreachable from origin/main is rejected" 1 $?

# crate not in this workspace -> usage error, not a pass
R="$WORK/wrongcrate"; scaffold "$R"
git -C "$R" tag v0.1.0
git -C "$R" update-ref refs/remotes/origin/main HEAD
( cd "$R" && "$GATE" not_a_member >/dev/null 2>&1 ); check "unknown crate is a usage error" 2 $?

# missing remote ref -> environment error, not a silent pass
R="$WORK/noremote"; scaffold "$R"
git -C "$R" tag v0.1.0
( cd "$R" && "$GATE" demo_crate >/dev/null 2>&1 ); check "missing origin/main is an environment error" 2 $?

printf '\npublish-gate tests: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
