#!/usr/bin/env bash
# REQUIRE_INSTALL policy regression tests for test_client_nfpm_native.sh.
#
# Asserts the required lifecycle semantics:
#   * REQUIRE_INSTALL=1 with a non-native target FAILS; there is no
#     successful QUALIFIED skip.
#   * REQUIRE_INSTALL=0 with a non-native target explicitly QUALIFIES
#     (byte/metadata/lint checks remain the gate's responsibility).
#   * REQUIRE_INSTALL=0 with a native target skips the lifecycle without
#     attempting an install.
#
# The gate is sourced with TERRAPHIM_CLIENT_NFPM_NATIVE_SOURCED=1 so the
# policy functions can be driven directly on nFPM fixture packages.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
GATE="$ROOT/.github/scripts/nfpm/tests/test_client_nfpm_native.sh"
RENDER="$ROOT/.github/scripts/nfpm/render-client-nfpm.sh"
NFPM_BIN="${NFPM_BIN:-nfpm}"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/terraphim-client-nfpm-policy.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
export SOURCE_DATE_EPOCH=1700000000

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

# REQUIRE_TOOLS=1 turns the nFPM prerequisite SKIP below into a hard failure
# so wiring this suite into CI without provisioning nFPM cannot pass
# vacuously. The host-CPU-architecture SKIP further down (no non-native MUSL
# target pair) is not tool-gated: it reflects the runner's own machine
# architecture, which REQUIRE_TOOLS cannot provision.
require_tool_or_skip() {
    local reason="$1"
    if [[ "${REQUIRE_TOOLS:-0}" == "1" ]]; then
        fail "REQUIRE_TOOLS=1: $reason"
    fi
    echo "SKIP: $reason" >&2
}

command -v "$NFPM_BIN" >/dev/null 2>&1 || { require_tool_or_skip "nFPM not available ($NFPM_BIN)"; exit 0; }

MACHINE="$(uname -m)"
case "$MACHINE" in
    x86_64) NON_NATIVE_TARGET=aarch64-unknown-linux-musl ;;
    aarch64) NON_NATIVE_TARGET=x86_64-unknown-linux-musl ;;
    *)
        echo "SKIP: no non-native MUSL target pair for $MACHINE" >&2
        exit 0
        ;;
esac
NATIVE_TARGET="${MACHINE}-unknown-linux-musl"

make_fixture_binary() {
    local path="$1"
    local bin_name="$2"
    mkdir -p "$(dirname "$path")"
    printf '#!/usr/bin/env sh\nprintf "%s %%s\\n" "%s"\n' "$bin_name" "$bin_name" > "$path"
    chmod 0755 "$path"
}

build_nfpm_package() {
    local format="$1"
    local bin_name="$2"
    local version="$3"
    local target="$4"
    local binary="$5"
    local out_dir="$6"

    local cfg="$TMP/$bin_name-$format-$version-$target.yaml"
    "$RENDER" \
        --format "$format" \
        --binary-name "$bin_name" \
        --version "$version" \
        --target "$target" \
        --binary "$binary" \
        --output "$cfg" >/dev/null
    "$NFPM_BIN" pkg --packager "$format" --config "$cfg" --target "$out_dir" >/dev/null
}

# Drive install_upgrade_remove_deb/rpm from the sourced gate in a subshell.
# $1 = REQUIRE_INSTALL value, $2 = TARGET, $3/$4 = deb pair, $5/$6 = rpm pair,
# $7 = bin_name.
run_lifecycle() {
    (
        export REQUIRE_INSTALL="$1"
        export TARGET="$2"
        export VERSION_OLD=0.0.1
        export VERSION_NEW=0.0.2
        TERRAPHIM_CLIENT_NFPM_NATIVE_SOURCED=1 source "$GATE"
        install_upgrade_remove_deb "$3" "$4" "$7"
        install_upgrade_remove_rpm "$5" "$6" "$7"
    ) >"$TMP/lifecycle.stdout" 2>"$TMP/lifecycle.stderr"
}

# Build one old/new fixture package pair per format for the given target,
# for terraphim-agent only (the policy gate is bin-name agnostic).
build_packages() {
    local target="$1" suffix="$2"
    local old_bin="$TMP/old-$target/terraphim-agent"
    local new_bin="$TMP/new-$target/terraphim-agent"
    make_fixture_binary "$old_bin" terraphim-agent
    make_fixture_binary "$new_bin" terraphim-agent
    build_nfpm_package deb terraphim-agent 0.0.1 "$target" "$old_bin" "$TMP/out-old-$target-deb"
    build_nfpm_package deb terraphim-agent 0.0.2 "$target" "$new_bin" "$TMP/out-new-$target-deb"
    build_nfpm_package rpm terraphim-agent 0.0.1 "$target" "$old_bin" "$TMP/out-old-$target-rpm"
    build_nfpm_package rpm terraphim-agent 0.0.2 "$target" "$new_bin" "$TMP/out-new-$target-rpm"
    OLD_DEB="$TMP/out-old-$target-deb/terraphim-agent_0.0.1-1_${suffix}.deb"
    NEW_DEB="$TMP/out-new-$target-deb/terraphim-agent_0.0.2-1_${suffix}.deb"
    OLD_RPM="$TMP/out-old-$target-rpm/terraphim-agent-0.0.1-1.${suffix}.rpm"
    NEW_RPM="$TMP/out-new-$target-rpm/terraphim-agent-0.0.2-1.${suffix}.rpm"
}

# REQUIRE_INSTALL=1 + non-native target: hard failure, and crucially no
# successful QUALIFIED skip anywhere in the output.
test_require_install_non_native_fails_without_qualified_skip() {
    local target="$NON_NATIVE_TARGET" suffix
    case "$target" in
        aarch64-unknown-linux-musl) suffix=arm64 ;;
        *) suffix=amd64 ;;
    esac
    build_packages "$target" "$suffix"

    if run_lifecycle 1 "$target" "$OLD_DEB" "$NEW_DEB" "$OLD_RPM" "$NEW_RPM" terraphim-agent; then
        fail "REQUIRE_INSTALL=1 passed for non-native target $target on $MACHINE"
    fi
    if grep -q 'QUALIFIED' "$TMP/lifecycle.stdout" "$TMP/lifecycle.stderr"; then
        fail "REQUIRE_INSTALL=1 produced a successful QUALIFIED skip for non-native target"
    fi
    grep -Fq "REQUIRE_INSTALL=1 requires the DEB install/upgrade/remove gate" "$TMP/lifecycle.stderr" ||
        fail "missing DEB REQUIRE_INSTALL policy diagnostics: $(cat "$TMP/lifecycle.stderr")"
}

# REQUIRE_INSTALL=0 + non-native target: explicit cross-target qualification.
test_no_require_install_non_native_qualifies() {
    local target="$NON_NATIVE_TARGET" suffix
    case "$target" in
        aarch64-unknown-linux-musl) suffix=arm64 ;;
        *) suffix=amd64 ;;
    esac

    if ! run_lifecycle 0 "$target" "$OLD_DEB" "$NEW_DEB" "$OLD_RPM" "$NEW_RPM" terraphim-agent; then
        fail "REQUIRE_INSTALL=0 failed for non-native target $target: $(cat "$TMP/lifecycle.stderr")"
    fi
    grep -Fq "QUALIFIED: $target DEB byte/metadata/lint checks passed" "$TMP/lifecycle.stdout" ||
        fail "missing explicit DEB QUALIFIED message"
    grep -Fq "QUALIFIED: $target RPM byte/metadata/lint checks passed" "$TMP/lifecycle.stdout" ||
        fail "missing explicit RPM QUALIFIED message"
    grep -Fq "REQUIRE_INSTALL=0" "$TMP/lifecycle.stdout" ||
        fail "QUALIFIED skip must state REQUIRE_INSTALL=0 explicitly"
}

# REQUIRE_INSTALL=0 + native target: skip without attempting an install
# (verified by the absence of any dpkg/docker/rpm execution diagnostics).
test_no_require_install_native_skips_without_install() {
    local target="$NATIVE_TARGET" suffix
    case "$target" in
        x86_64-unknown-linux-musl) suffix=amd64 ;;
        *) suffix=arm64 ;;
    esac
    build_packages "$target" "$suffix"

    if ! run_lifecycle 0 "$target" "$OLD_DEB" "$NEW_DEB" "$OLD_RPM" "$NEW_RPM" terraphim-agent; then
        fail "REQUIRE_INSTALL=0 failed for native target $target: $(cat "$TMP/lifecycle.stderr")"
    fi
    grep -Fq "SKIP: DEB install/upgrade/remove gate disabled by REQUIRE_INSTALL=0 for native target $target" "$TMP/lifecycle.stdout" ||
        fail "missing native SKIP message for DEB"
    grep -Fq "SKIP: RPM install/upgrade/remove gate disabled by REQUIRE_INSTALL=0 for native target $target" "$TMP/lifecycle.stdout" ||
        fail "missing native SKIP message for RPM"
}

# The workflow must place each target on its native hosted runner and require
# both the synthetic and actual-package lifecycle gates. Cross-target skip
# semantics remain tested above as a fail-closed library policy, but the
# release workflow must never use that qualified skip for either matrix leg.
test_workflow_each_target_uses_native_runner_with_required_install() {
    local workflow="$ROOT/.github/workflows/release-binaries.yml"
    local block
    block="$(sed -n '/^  build-client-packages:/,/^  create-universal-macos:/p' "$workflow")"
    [[ "$block" == *$'- target: x86_64-unknown-linux-musl\n            runner: ubuntu-22.04\n            runner_arch: X64'* ]] ||
        fail "workflow package matrix missing native x86_64 runner pairing"
    [[ "$block" == *$'- target: aarch64-unknown-linux-musl\n            runner: ubuntu-22.04-arm\n            runner_arch: ARM64'* ]] ||
        fail "workflow package matrix missing native aarch64 runner pairing"
    grep -Fq 'runs-on: ${{ matrix.runner }}' <<<"$block" ||
        fail "workflow package job does not consume the per-target native runner"
    [[ "$(grep -Fc 'REQUIRE_INSTALL: "1"' <<<"$block")" -eq 2 ]] ||
        fail "workflow must require both synthetic and actual package lifecycle gates"
    [[ "$(grep -Fc 'test "${{ runner.arch }}" = "$EXPECTED_RUNNER_ARCH"' <<<"$block")" -eq 2 ]] ||
        fail "workflow must fail closed when either hosted runner architecture is wrong"
    ! grep -Fq "&& '1' || '0'" <<<"$block" ||
        fail "workflow must not turn either native lifecycle into a successful no-op"
}

test_require_install_non_native_fails_without_qualified_skip
test_no_require_install_non_native_qualifies
test_no_require_install_native_skips_without_install
test_workflow_each_target_uses_native_runner_with_required_install

echo "client nFPM REQUIRE_INSTALL policy tests passed"
