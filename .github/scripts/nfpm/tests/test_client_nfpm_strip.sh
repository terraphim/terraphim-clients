#!/usr/bin/env bash
# Canonical-input contract for build-client-packages.sh: qualified Linux MUSL
# binaries are immutable release artifacts. This producer must never mutate
# or re-derive them (in particular: no in-place stripping), must reject an
# input that is not already stripped with a clear diagnostic before nFPM
# runs, and -- for an accepted (already-stripped) input -- the packaged DEB
# and RPM payload SHA-256 must exactly equal the caller-supplied canonical
# input's SHA-256, never a transformed copy.
#
# Real evidence (see /tmp/client-real-package-build.log and the remediation
# record): an earlier revision of this script stripped its private working
# copy in place and hashed the POST-strip bytes, so the packaged payload no
# longer equaled the canonical staged release bytes it was handed. That
# approach was rejected; stripping-once-when-staging is the separate #248
# producer's responsibility, and this script only verifies the precondition.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
BUILD="$ROOT/.github/scripts/nfpm/build-client-packages.sh"
RENDER="$ROOT/.github/scripts/nfpm/render-client-nfpm.sh"
NFPM_BIN="${NFPM_BIN:-nfpm}"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/terraphim-client-nfpm-strip.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
export SOURCE_DATE_EPOCH=1700000000

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

require_tool_or_skip() {
    local reason="$1"
    if [[ "${REQUIRE_TOOLS:-0}" == "1" ]]; then
        fail "REQUIRE_TOOLS=1: $reason"
    fi
    echo "SKIP: $reason" >&2
}

command -v "$NFPM_BIN" >/dev/null 2>&1 || { require_tool_or_skip "nFPM not available ($NFPM_BIN)"; exit 0; }
command -v dpkg-deb >/dev/null 2>&1 || { require_tool_or_skip "dpkg-deb not installed"; exit 0; }
command -v strip >/dev/null 2>&1 || { require_tool_or_skip "strip not installed"; exit 0; }
command -v readelf >/dev/null 2>&1 || { require_tool_or_skip "readelf not installed"; exit 0; }

find_cc() {
    local candidate
    for candidate in "${CC:-}" cc gcc clang; do
        if [[ -n "$candidate" ]] && command -v "$candidate" >/dev/null 2>&1; then
            printf '%s' "$candidate"
            return 0
        fi
    done
    return 1
}

# A "real" ELF carrying genuine unstripped DWARF debug info (-g, no -s).
make_unstripped_binary_with_debug_info() {
    local path="$1"
    local bin_name="$2"
    local cc_bin
    cc_bin="$(find_cc)" || return 1
    local src="$path.fixture.c"
    mkdir -p "$(dirname "$path")"
    cat > "$src" <<EOF
#include <stdio.h>
#include <string.h>
int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--version") == 0) {
        printf("${bin_name} %s\n", VERSION);
        return 0;
    }
    fprintf(stderr, "usage: ${bin_name} --version\n");
    return 64;
}
EOF
    "$cc_bin" -O0 -g -DVERSION="\"9.8.7\"" -o "$path" "$src" || return 1
    rm -f "$src"
    chmod 0755 "$path"
    readelf -S -W "$path" 2>/dev/null | grep -q '\.debug_info' || return 1
}

# A "minimal" unstripped ELF: no debug sections at all (no -g), but a plain
# link still leaves a .symtab behind. This proves the rejection is keyed on
# any leftover symbol-table/debug section, not just DWARF debug info.
make_minimal_unstripped_binary() {
    local path="$1"
    local bin_name="$2"
    local cc_bin
    cc_bin="$(find_cc)" || return 1
    local src="$path.fixture.c"
    mkdir -p "$(dirname "$path")"
    cat > "$src" <<EOF
#include <stdio.h>
int main(void) { printf("${bin_name}\n"); return 0; }
EOF
    "$cc_bin" -O0 -DVERSION="\"9.8.7\"" -o "$path" "$src" || return 1
    rm -f "$src"
    chmod 0755 "$path"
    readelf -S -W "$path" 2>/dev/null | grep -q '\.symtab' || return 1
    if readelf -S -W "$path" 2>/dev/null | grep -q '\.debug_info'; then
        return 1
    fi
    return 0
}

if ! make_unstripped_binary_with_debug_info "$TMP/qualified/terraphim-agent" terraphim-agent; then
    require_tool_or_skip "no debug-capable C compiler available (CC/cc/gcc/clang)"
    exit 0
fi
make_unstripped_binary_with_debug_info "$TMP/qualified/terraphim-grep" terraphim-grep ||
    fail "failed to build the terraphim-grep unstripped-with-debug-info fixture"
make_minimal_unstripped_binary "$TMP/minimal/terraphim-agent" terraphim-agent ||
    fail "failed to build the terraphim-agent minimal-unstripped fixture"

AGENT_DEBUG_SRC="$TMP/qualified/terraphim-agent"
GREP_DEBUG_SRC="$TMP/qualified/terraphim-grep"
AGENT_MINIMAL_SRC="$TMP/minimal/terraphim-agent"

# The canonical-input fixture used by the "accepted" tests: pre-stripped
# OUTSIDE build-client-packages.sh, exactly mimicking the separate #248
# producer's stripping-once-when-staging step. This is the only kind of
# input the production script is ever handed.
make_canonical_stripped_input() {
    local debug_src="$1"
    local out="$2"
    cp -- "$debug_src" "$out"
    strip --strip-unneeded -- "$out"
    readelf -S -W "$out" 2>/dev/null | grep -Eq '\.symtab|\.debug' &&
        fail "fixture precondition violated: canonical input still carries a symtab/debug section after strip: $out"
    return 0
}

AGENT_CANONICAL_INPUT="$TMP/canonical/terraphim-agent"
GREP_CANONICAL_INPUT="$TMP/canonical/terraphim-grep"
mkdir -p "$TMP/canonical"
make_canonical_stripped_input "$AGENT_DEBUG_SRC" "$AGENT_CANONICAL_INPUT"
make_canonical_stripped_input "$GREP_DEBUG_SRC" "$GREP_CANONICAL_INPUT"

AGENT_CANONICAL_SHA="$(sha256sum "$AGENT_CANONICAL_INPUT" | awk '{print $1}')"
GREP_CANONICAL_SHA="$(sha256sum "$GREP_CANONICAL_INPUT" | awk '{print $1}')"
AGENT_DEBUG_SHA="$(sha256sum "$AGENT_DEBUG_SRC" | awk '{print $1}')"

[[ "$AGENT_CANONICAL_SHA" != "$AGENT_DEBUG_SHA" ]] ||
    fail "fixture precondition violated: stripping the terraphim-agent fixture did not change its bytes"

# ---------------------------------------------------------------------------
# Static contract: the production script must contain no code path that
# mutates or strips a caller-supplied binary. Only a read-only ELF-section
# inspection (validate_stripped_binary) may run before packaging.
# ---------------------------------------------------------------------------
test_build_script_never_mutates_or_strips_inputs() {
    grep -Fq 'strip_qualified_binary' "$BUILD" &&
        fail "build-client-packages.sh still defines/calls strip_qualified_binary: inputs must never be mutated"
    grep -Fq 'strip --strip-unneeded --' "$BUILD" &&
        fail "build-client-packages.sh still invokes 'strip --strip-unneeded --' on a binary: inputs must never be mutated"
    grep -Fq 'validate_stripped_binary "$source_binary"' "$BUILD" ||
        fail "build_one_binary does not validate that the caller's source binary is already stripped"
    grep -Fq 'validate_stripped_binary "$validated_binary"' "$BUILD" ||
        fail "build_one_binary does not validate that the private staged copy is already stripped"
    grep -Fq 'EXPECTED_SHA="$(sha256sum "$BINARY"' "$BUILD" ||
        fail "no EXPECTED_SHA computation found in $BUILD"
    return 0
}

# ---------------------------------------------------------------------------
# Behavioral: an unstripped input (real DWARF debug info, or a minimal
# unstripped link with only a leftover .symtab) must be rejected before nFPM
# ever runs, with a clear diagnostic, and the caller's bytes must be
# untouched.
# ---------------------------------------------------------------------------
run_build_one_binary() {
    local bin_name="$1"
    local source_binary="$2"
    local work="$3"
    local pkgdir="$4"
    (
        export WORK_DIR="$work"
        export PACKAGE_DIR="$pkgdir"
        export RENDER
        export NFPM_BIN
        export TARGET=x86_64-unknown-linux-musl
        export DEB_ARCH=amd64
        export RPM_ARCH=x86_64
        export VERSION=9.8.7
        export PKG_VERSION=9.8.7
        export EXPECTED_BASENAMES=()
        # Defined AFTER sourcing so these stubs override the script's real
        # lint_deb/lint_rpm (sourcing would otherwise redefine them back):
        # this isolates the strip/byte-binding contract from lint-policy
        # outcomes, which are covered independently by
        # test_client_nfpm_static_lint.sh.
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        lint_deb() { :; }
        lint_rpm() { :; }
        build_one_binary "$bin_name" "$source_binary"
        printf '%s\n' "$EXPECTED_SHA"
    )
}

test_unstripped_real_debug_info_input_rejected() {
    local work="$TMP/reject-debug-work" pkgdir="$TMP/reject-debug-pkg"
    mkdir -p "$work" "$pkgdir"
    local log="$TMP/reject-debug.log"
    if run_build_one_binary terraphim-agent "$AGENT_DEBUG_SRC" "$work" "$pkgdir" >"$log" 2>&1; then
        fail "build_one_binary accepted an unstripped input carrying real DWARF debug info: $(cat "$log")"
    fi
    grep -Fq 'is not stripped' "$log" ||
        fail "rejection did not report a clear stripped-binary diagnostic: $(cat "$log")"
    grep -Fq '.debug_info' "$log" ||
        fail "rejection did not name the leftover debug section: $(cat "$log")"
    find "$pkgdir" -mindepth 1 -print -quit | grep -q . &&
        fail "rejected input still produced package output"
    local after_sha
    after_sha="$(sha256sum "$AGENT_DEBUG_SRC" | awk '{print $1}')"
    [[ "$after_sha" == "$AGENT_DEBUG_SHA" ]] ||
        fail "rejected input's bytes were mutated by build_one_binary"
}

test_minimal_unstripped_input_rejected() {
    local work="$TMP/reject-minimal-work" pkgdir="$TMP/reject-minimal-pkg"
    mkdir -p "$work" "$pkgdir"
    local before_sha log
    before_sha="$(sha256sum "$AGENT_MINIMAL_SRC" | awk '{print $1}')"
    log="$TMP/reject-minimal.log"
    if run_build_one_binary terraphim-agent "$AGENT_MINIMAL_SRC" "$work" "$pkgdir" >"$log" 2>&1; then
        fail "build_one_binary accepted a minimal unstripped input (leftover .symtab only): $(cat "$log")"
    fi
    grep -Fq 'is not stripped' "$log" ||
        fail "rejection did not report a clear stripped-binary diagnostic: $(cat "$log")"
    grep -Fq '.symtab' "$log" ||
        fail "rejection did not name the leftover .symtab section: $(cat "$log")"
    local after_sha
    after_sha="$(sha256sum "$AGENT_MINIMAL_SRC" | awk '{print $1}')"
    [[ "$after_sha" == "$before_sha" ]] ||
        fail "rejected minimal input's bytes were mutated by build_one_binary"
}

# ---------------------------------------------------------------------------
# Behavioral: a pre-stripped canonical input is accepted unchanged, and the
# packaged DEB+RPM payload SHA-256 binds EXACTLY to the canonical input's own
# SHA-256 -- never a transformed copy.
# ---------------------------------------------------------------------------
test_prestripped_canonical_input_accepted_and_payload_binds_to_input_sha() {
    local work="$TMP/accept-work" pkgdir="$TMP/accept-pkg"
    mkdir -p "$work" "$pkgdir"
    local log="$TMP/accept.log"
    local reported_sha
    reported_sha="$(run_build_one_binary terraphim-agent "$AGENT_CANONICAL_INPUT" "$work" "$pkgdir" 2>"$TMP/accept.err" | tail -n1)" ||
        fail "build_one_binary rejected a pre-stripped canonical input: $(cat "$TMP/accept.err")"

    # 1. Caller input bytes never change.
    local input_sha_after
    input_sha_after="$(sha256sum "$AGENT_CANONICAL_INPUT" | awk '{print $1}')"
    [[ "$input_sha_after" == "$AGENT_CANONICAL_SHA" ]] ||
        fail "canonical input bytes were mutated by build_one_binary"

    # 2. The reported/hashed SHA is exactly the canonical input's own SHA
    #    (not a transformed copy: there is nothing left to transform).
    [[ "$reported_sha" == "$AGENT_CANONICAL_SHA" ]] ||
        fail "EXPECTED_SHA ($reported_sha) does not equal the canonical input SHA ($AGENT_CANONICAL_SHA)"

    # 3. DEB payload SHA-256 equals the canonical input SHA-256 exactly.
    local deb="$pkgdir/terraphim-agent_9.8.7-1_amd64.deb"
    [[ -f "$deb" ]] || fail "expected DEB not found: $deb"
    local deb_extract="$TMP/deb-extract"
    mkdir -p "$deb_extract"
    dpkg-deb --extract "$deb" "$deb_extract"
    local deb_payload_sha
    deb_payload_sha="$(sha256sum "$deb_extract/usr/bin/terraphim-agent" | awk '{print $1}')"
    [[ "$deb_payload_sha" == "$AGENT_CANONICAL_SHA" ]] ||
        fail "installed DEB payload SHA ($deb_payload_sha) does not exactly equal the canonical input SHA ($AGENT_CANONICAL_SHA)"

    # 4. RPM payload SHA-256 equals the canonical input SHA-256 exactly
    #    (skipped only when neither host rpm tooling nor Docker is present).
    if command -v rpm2cpio >/dev/null 2>&1 && command -v cpio >/dev/null 2>&1; then
        local rpm="$pkgdir/terraphim-agent-9.8.7-1.x86_64.rpm"
        [[ -f "$rpm" ]] || fail "expected RPM not found: $rpm"
        local rpm_payload="$TMP/rpm-payload"
        local pattern_file="$TMP/rpm-pattern"
        printf '*usr/bin/terraphim-agent\n' > "$pattern_file"
        ( set +o pipefail
          rpm2cpio "$rpm" | cpio -i --to-stdout --pattern-file="$pattern_file" \
              >"$rpm_payload" 2>"$TMP/rpm-cpio.err" )
        [[ -s "$rpm_payload" ]] || fail "RPM payload extraction produced no bytes: $(cat "$TMP/rpm-cpio.err")"
        local rpm_payload_sha
        rpm_payload_sha="$(sha256sum "$rpm_payload" | awk '{print $1}')"
        [[ "$rpm_payload_sha" == "$AGENT_CANONICAL_SHA" ]] ||
            fail "installed RPM payload SHA ($rpm_payload_sha) does not exactly equal the canonical input SHA ($AGENT_CANONICAL_SHA)"
    else
        require_tool_or_skip "rpm2cpio/cpio not installed; RPM payload SHA-binding not independently checked"
    fi
}

# ---------------------------------------------------------------------------
# End-to-end production run (real $BUILD entrypoint, both binaries) with
# lintian/rpmlint stubbed clean via PATH so this proves the byte-binding
# contract independent of lint-policy specifics (covered separately by
# test_client_nfpm_static_lint.sh). Proves the printed inventory line and
# the checksum manifest both correspond to the canonical input SHAs, not a
# transformed copy.
# ---------------------------------------------------------------------------
STUB_BIN="$TMP/stub-bin"
mkdir -p "$STUB_BIN"
printf '#!/usr/bin/env bash\nprintf "N: clean\\n"\nexit 0\n' > "$STUB_BIN/lintian"
printf '#!/usr/bin/env bash\nprintf "============================ rpmlint session starts ============================\\n 1 packages and 0 specfiles checked; 0 errors, 0 warnings, 0 filtered, 0 badness\\n"\nexit 0\n' > "$STUB_BIN/rpmlint"
chmod 0755 "$STUB_BIN/lintian" "$STUB_BIN/rpmlint"

test_production_run_binds_receipts_and_inventory_to_canonical_input_sha() {
    local out="$TMP/prod-out"
    local log="$TMP/prod.log"
    (
        export PATH="$STUB_BIN:$PATH"
        "$BUILD" \
            --version 9.8.7 \
            --target x86_64-unknown-linux-musl \
            --agent-binary "$AGENT_CANONICAL_INPUT" \
            --grep-binary "$GREP_CANONICAL_INPUT" \
            --out-dir "$out" \
            --nfpm "$NFPM_BIN"
    ) >"$log" 2>&1 || fail "production run rejected pre-stripped canonical inputs: $(cat "$log")"

    grep -Fq "package payload ok terraphim-agent x86_64-unknown-linux-musl $AGENT_CANONICAL_SHA" "$log" ||
        fail "printed inventory line does not report the canonical terraphim-agent input SHA: $(cat "$log")"
    grep -Fq "package payload ok terraphim-grep x86_64-unknown-linux-musl $GREP_CANONICAL_SHA" "$log" ||
        fail "printed inventory line does not report the canonical terraphim-grep input SHA: $(cat "$log")"

    local sums="$out/terraphim-clients-9.8.7-x86_64-unknown-linux-musl.package-sha256sums.txt"
    [[ -f "$sums" ]] || fail "checksum manifest missing: $sums"

    # Re-derive the payload SHA from each staged package independently of
    # the script's own EXPECTED_SHA bookkeeping, and confirm it is exactly
    # the canonical input SHA for both binaries and both formats.
    local deb_extract="$TMP/prod-deb-extract"
    mkdir -p "$deb_extract/agent" "$deb_extract/grep"
    dpkg-deb --extract "$out/terraphim-agent_9.8.7-1_amd64.deb" "$deb_extract/agent"
    dpkg-deb --extract "$out/terraphim-grep_9.8.7-1_amd64.deb" "$deb_extract/grep"
    [[ "$(sha256sum "$deb_extract/agent/usr/bin/terraphim-agent" | awk '{print $1}')" == "$AGENT_CANONICAL_SHA" ]] ||
        fail "production DEB payload SHA for terraphim-agent does not equal the canonical input SHA"
    [[ "$(sha256sum "$deb_extract/grep/usr/bin/terraphim-grep" | awk '{print $1}')" == "$GREP_CANONICAL_SHA" ]] ||
        fail "production DEB payload SHA for terraphim-grep does not equal the canonical input SHA"

    if command -v rpm2cpio >/dev/null 2>&1 && command -v cpio >/dev/null 2>&1; then
        local pf="$TMP/prod-rpm-pattern"
        for bin_name in terraphim-agent terraphim-grep; do
            printf '*usr/bin/%s\n' "$bin_name" > "$pf"
            local payload="$TMP/prod-rpm-payload-$bin_name"
            ( set +o pipefail
              rpm2cpio "$out/${bin_name}-9.8.7-1.x86_64.rpm" | cpio -i --to-stdout --pattern-file="$pf" \
                  >"$payload" 2>"$TMP/prod-rpm-cpio-$bin_name.err" )
            [[ -s "$payload" ]] || fail "production RPM payload extraction produced no bytes for $bin_name"
        done
        [[ "$(sha256sum "$TMP/prod-rpm-payload-terraphim-agent" | awk '{print $1}')" == "$AGENT_CANONICAL_SHA" ]] ||
            fail "production RPM payload SHA for terraphim-agent does not equal the canonical input SHA"
        [[ "$(sha256sum "$TMP/prod-rpm-payload-terraphim-grep" | awk '{print $1}')" == "$GREP_CANONICAL_SHA" ]] ||
            fail "production RPM payload SHA for terraphim-grep does not equal the canonical input SHA"
    else
        require_tool_or_skip "rpm2cpio/cpio not installed; production RPM payload SHA-binding not independently checked"
    fi
}

test_build_script_never_mutates_or_strips_inputs
test_unstripped_real_debug_info_input_rejected
test_minimal_unstripped_input_rejected
test_prestripped_canonical_input_accepted_and_payload_binds_to_input_sha
test_production_run_binds_receipts_and_inventory_to_canonical_input_sha

echo "client nFPM canonical-input (no in-place stripping) contract tests passed"
