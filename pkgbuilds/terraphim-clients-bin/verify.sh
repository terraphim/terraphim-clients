#!/usr/bin/env bash
# Full Arch verification for the terraphim-clients-bin split package.
#
# Intended to run inside an Arch Linux container (see README.md):
#   docker run --rm -v "$PWD/..":/mnt -w /mnt archlinux:base-devel \
#     bash -lc 'pacman -Sy --noconfirm namcap >/dev/null && ./pkgbuilds/terraphim-clients-bin/verify.sh'
#
# When run as root it builds as an unprivileged \`builder\` user (makepkg
# refuses to run as root) and installs/removes with pacman.
set -euo pipefail
cd "$(dirname "$0")"

PKGVER="$(sed -n 's/^pkgver=//p' PKGBUILD)"
echo "== pkgver: $PKGVER =="

if [ "$(id -u)" -eq 0 ]; then
  id builder >/dev/null 2>&1 || useradd -m builder
  chown -R builder:builder .
  run_makepkg() { runuser -u builder -- makepkg "$@"; }
else
  run_makepkg() { makepkg "$@"; }
fi

echo "== .SRCINFO drift =="
run_makepkg --printsrcinfo > /tmp/.SRCINFO.generated
if ! diff -u .SRCINFO /tmp/.SRCINFO.generated; then
  echo "ERROR: .SRCINFO is stale; regenerate with 'makepkg --printsrcinfo > .SRCINFO'" >&2
  exit 1
fi

echo "== build =="
rm -f ./*.pkg.tar.zst
run_makepkg --force --noconfirm

agent="$(ls terraphim-agent-bin-"$PKGVER"-1-*.pkg.tar.zst)"
grep_pkg="$(ls terraphim-grep-bin-"$PKGVER"-1-*.pkg.tar.zst)"
echo "artifacts: $agent $grep_pkg"

echo "== namcap =="
namcap "$agent" "$grep_pkg"

echo "== split ownership =="
list_agent="$(bsdtar -tf "$agent" | sort)"
list_grep="$(bsdtar -tf "$grep_pkg" | sort)"
printf '%s\n' "$list_agent"
printf '%s\n' "$list_grep"
for f in usr/bin/terraphim-agent usr/share/licenses/terraphim-agent-bin/LICENSE-Apache-2.0 usr/share/terraphim/package-manager.d/terraphim-agent; do
  grep -qx "$f" <<<"$list_agent" || { echo "agent missing $f" >&2; exit 1; }
done
for f in usr/bin/terraphim-grep usr/share/licenses/terraphim-grep-bin/LICENSE-MIT usr/share/terraphim/package-manager.d/terraphim-grep; do
  grep -qx "$f" <<<"$list_grep" || { echo "grep missing $f" >&2; exit 1; }
done
! grep -q 'terraphim-grep' <<<"$list_agent"
! grep -q 'terraphim-agent' <<<"$list_grep"

echo "== install / Qkk / smoke / receipt / remove =="
as_root() { if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi; }
as_root pacman -U --noconfirm "$agent" "$grep_pkg"
as_root pacman -Qkk terraphim-agent-bin terraphim-grep-bin
terraphim-agent --version | grep -F "$PKGVER"
terraphim-grep --version | grep -F "$PKGVER"
test "$(tr -d '\n' < /usr/share/terraphim/package-manager.d/terraphim-agent)" = pacman
test "$(tr -d '\n' < /usr/share/terraphim/package-manager.d/terraphim-grep)" = pacman
as_root pacman -R --noconfirm terraphim-agent-bin terraphim-grep-bin
test ! -e /usr/bin/terraphim-agent
test ! -e /usr/bin/terraphim-grep
test ! -e /usr/share/terraphim/package-manager.d/terraphim-agent

echo "ALL CHECKS PASSED"
