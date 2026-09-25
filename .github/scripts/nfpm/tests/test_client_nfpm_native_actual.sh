#!/usr/bin/env bash
# Post-build native lifecycle gate for the ACTUAL terraphim-clients DEB/RPM
# packages produced by build-client-packages.sh for a real release version --
# not the synthetic fixtures used by test_client_nfpm_native.sh.
#
# test_client_nfpm_native.sh proves the packaging *mechanism* (byte
# correlation, receipt path/content, managed-mode contract) against a C
# fixture that deliberately replicates terraphim_update's behavior. This gate
# closes the remaining coverage gap: it installs the real DEB/RPM built from
# the real qualified terraphim-agent/terraphim-grep binaries and drives the
# REAL Rust `check-update`/`update` commands (crates/terraphim_update) inside
# them, for both binaries and both package formats.
#
# The workflow runs this on both native hosted runner legs (x86_64 and
# aarch64 MUSL), after build-client-packages.sh has independently verified
# the packages (verify_deb/verify_rpm: payload SHA, receipt, arch, forbidden
# deps, lint).
#
# REQUIRE_INSTALL semantics mirror test_client_nfpm_native.sh exactly:
#   REQUIRE_INSTALL=0 -> QUALIFIED (non-native target) or SKIP (native target)
#                        without attempting an install; there is no successful
#                        QUALIFIED skip for a REQUIRE_INSTALL=1 non-native
#                        target -- that fails closed instead.
#   REQUIRE_INSTALL=1 -> the actual-package lifecycle MUST run.
#
# "Upgrade" is proven at the package-manager level: a second package is built
# from the SAME actual qualified binaries (never rebuilt) under a strictly
# higher synthetic package version, purely to exercise the dpkg/rpm Version
# transition. The installed executable's own `--version` output is asserted
# identical before and after (same real bytes), and the package Version field
# is asserted to have advanced.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
GATE="$ROOT/.github/scripts/nfpm/tests/test_client_nfpm_native.sh"
BUILD="$ROOT/.github/scripts/nfpm/build-client-packages.sh"

TARGET="${TARGET:-x86_64-unknown-linux-musl}"
VERSION="${VERSION:?VERSION is required: the real release version the actual packages were built for}"
PACKAGE_DIR="${PACKAGE_DIR:?PACKAGE_DIR is required: directory holding the actual built DEB/RPM packages}"
AGENT_BINARY="${AGENT_BINARY:?AGENT_BINARY is required: the qualified terraphim-agent binary the actual packages were built from}"
GREP_BINARY="${GREP_BINARY:?GREP_BINARY is required: the qualified terraphim-grep binary the actual packages were built from}"
NFPM_BIN="${NFPM_BIN:-nfpm}"
REQUIRE_INSTALL="${REQUIRE_INSTALL:-1}"

TMP="$(mktemp -d "${TMPDIR:-/tmp}/terraphim-client-nfpm-native-actual.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

export TARGET REQUIRE_INSTALL NFPM_BIN
export VERSION_OLD="$VERSION"
export VERSION_NEW="$VERSION"
TERRAPHIM_CLIENT_NFPM_NATIVE_SOURCED=1 source "$GATE"

if [[ "$REQUIRE_INSTALL" == "0" ]]; then
    if ! is_native_target; then
        echo "QUALIFIED: $TARGET actual-package install lifecycle skipped for non-native target (REQUIRE_INSTALL=0)"
    else
        echo "SKIP: actual-package install/upgrade/remove gate disabled by REQUIRE_INSTALL=0 for native target $TARGET"
    fi
    exit 0
fi

if ! is_native_target; then
    echo "BLOCKED: REQUIRE_INSTALL=1 requires the actual-package install/upgrade/remove gate, but target $TARGET is non-native on $(uname -m); cross-target qualification must be requested explicitly with REQUIRE_INSTALL=0" >&2
    exit 1
fi

command -v "$NFPM_BIN" >/dev/null 2>&1 || {
    echo "BLOCKED: nFPM is required to build the upgrade-transition package: $NFPM_BIN" >&2
    exit 127
}

[[ -f "$AGENT_BINARY" && ! -L "$AGENT_BINARY" ]] || { echo "missing qualified terraphim-agent binary: $AGENT_BINARY" >&2; exit 1; }
[[ -f "$GREP_BINARY" && ! -L "$GREP_BINARY" ]] || { echo "missing qualified terraphim-grep binary: $GREP_BINARY" >&2; exit 1; }

case "$TARGET" in
    x86_64-unknown-linux-musl)
        DEB_ARCH_A="amd64"
        RPM_ARCH_A="x86_64"
        ;;
    aarch64-unknown-linux-musl)
        DEB_ARCH_A="arm64"
        RPM_ARCH_A="aarch64"
        ;;
    *)
        echo "unsupported target for actual-package native gate: $TARGET" >&2
        exit 2
        ;;
esac

# nFPM's '-' -> '~' version normalization; matches
# build-client-packages.sh's CLIENT_PACKAGE_VERSION_RE handling exactly.
PKG_VERSION="${VERSION//-/\~}"

# A strictly higher synthetic package version used only to exercise the real
# dpkg/rpm upgrade transition. Derived from VERSION's numeric MAJOR.MINOR
# (any prerelease suffix is dropped) with PATCH+1, which sorts higher under
# both dpkg and rpm version comparison regardless of VERSION's own form.
IFS='.' read -r _v_major _v_minor _v_patch_rest <<<"$VERSION"
_v_patch="${_v_patch_rest%%-*}"
if ! [[ "$_v_major" =~ ^[0-9]+$ && "$_v_minor" =~ ^[0-9]+$ && "$_v_patch" =~ ^[0-9]+$ ]]; then
    echo "cannot derive an upgrade-transition version from VERSION=$VERSION" >&2
    exit 1
fi
NEXT_VERSION="${_v_major}.${_v_minor}.$((_v_patch + 1))"
NEXT_PKG_VERSION="${NEXT_VERSION//-/\~}"

export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1700000000}"

echo "building upgrade-transition packages: $VERSION -> $NEXT_VERSION (same qualified binaries, package-manager Version field only)"
"$BUILD" --version "$NEXT_VERSION" --target "$TARGET" \
    --agent-binary "$AGENT_BINARY" --grep-binary "$GREP_BINARY" \
    --out-dir "$TMP/out-upgrade" --nfpm "$NFPM_BIN" >/dev/null

BIN_NAMES=(terraphim-agent terraphim-grep)
declare -A BINARY_PATH=(
    [terraphim-agent]="$AGENT_BINARY"
    [terraphim-grep]="$GREP_BINARY"
)

for bin_name in "${BIN_NAMES[@]}"; do
    actual_deb="$PACKAGE_DIR/${bin_name}_${PKG_VERSION}-1_${DEB_ARCH_A}.deb"
    actual_rpm="$PACKAGE_DIR/${bin_name}-${PKG_VERSION}-1.${RPM_ARCH_A}.rpm"
    upgrade_deb="$TMP/out-upgrade/${bin_name}_${NEXT_PKG_VERSION}-1_${DEB_ARCH_A}.deb"
    upgrade_rpm="$TMP/out-upgrade/${bin_name}-${NEXT_PKG_VERSION}-1.${RPM_ARCH_A}.rpm"

    for f in "$actual_deb" "$actual_rpm" "$upgrade_deb" "$upgrade_rpm"; do
        [[ -f "$f" ]] || { echo "missing package for actual-lifecycle gate: $f" >&2; exit 1; }
    done

    expected_version_output="$("${BINARY_PATH[$bin_name]}" --version)"
    [[ -n "$expected_version_output" ]] || {
        echo "qualified $bin_name binary produced empty --version output" >&2
        exit 1
    }

    install_upgrade_remove_deb "$actual_deb" "$upgrade_deb" "$bin_name" \
        "$expected_version_output" "$NEXT_PKG_VERSION"
    install_upgrade_remove_rpm "$actual_rpm" "$upgrade_rpm" "$bin_name" \
        "$expected_version_output" "$NEXT_PKG_VERSION"

    echo "actual package lifecycle passed for $bin_name $TARGET (version $VERSION)"
done

echo "client nFPM actual-package native gate passed for $TARGET"
