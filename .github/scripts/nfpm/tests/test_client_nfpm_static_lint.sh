#!/usr/bin/env bash
# Behavioral regression for the fail-closed static-MUSL lint policy in
# build-client-packages.sh.
#
# Adapted from the reviewed terraphim_server test_server_nfpm_static_lint.sh,
# generalized to the two hyphenated client binaries (terraphim-agent,
# terraphim-grep) sharing one enforce_lint_policy implementation keyed by
# $BIN_NAME.
#
# 1. Production probe: an actually fully-static ELF for terraphim-agent is
#    packaged through the production build-client-packages.sh pipeline (real
#    lintian/rpmlint via host tools or the pinned Docker images) and the
#    DEB+RPM production path must pass while emitting exactly the justified
#    static diagnostic for terraphim-agent at usr/bin/terraphim-agent, with
#    the full raw lint output preserved as evidence.
# 2. Mutation negatives: stub lintian/rpmlint injected through PATH (the
#    same production lint_deb/lint_rpm code path resolves host tools via
#    PATH) prove that extra or wrong error lines, wrong packages/paths,
#    malformed variants, duplicates, inconsistent exit statuses and
#    tool/install/transport failures all fail closed.
# 3. Cross-binary specificity: the justified diagnostic is scoped to
#    $BIN_NAME, so a diagnostic naming the *other* real client binary must
#    not be allowlisted while linting this binary's package.
# 4. Dynamic fixture gates stay meaningful: a clean (zero error line) tool
#    result still passes, which is what the dynamically linked fixtures of
#    the native gate rely on.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
BUILD="$ROOT/.github/scripts/nfpm/build-client-packages.sh"
RENDER="$ROOT/.github/scripts/nfpm/render-client-nfpm.sh"
NFPM_BIN="${NFPM_BIN:-nfpm}"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/terraphim-client-nfpm-static-lint.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
export SOURCE_DATE_EPOCH=1700000000

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

# REQUIRE_TOOLS=1 turns every prerequisite SKIP in this suite into a hard
# failure so wiring it into CI without provisioning nFPM/dpkg-deb/a C
# compiler/lintian-or-Docker/rpm-tooling-or-Docker cannot pass vacuously.
require_tool_or_skip() {
    local reason="$1"
    if [[ "${REQUIRE_TOOLS:-0}" == "1" ]]; then
        fail "REQUIRE_TOOLS=1: $reason"
    fi
    echo "SKIP: $reason" >&2
}

command -v "$NFPM_BIN" >/dev/null 2>&1 || { require_tool_or_skip "nFPM not available ($NFPM_BIN)"; exit 0; }
command -v dpkg-deb >/dev/null 2>&1 || { require_tool_or_skip "dpkg-deb not installed"; exit 0; }

docker_available() {
    command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1
}

# An actually fully-static ELF for the qualified MUSL payload, in the
# production --version contract, for the given client binary name.
make_static_binary() {
    local path="$1"
    local bin_name="$2"
    local cc_bin=""
    local candidate
    for candidate in "${CC:-}" cc gcc clang; do
        if [[ -n "$candidate" ]] && command -v "$candidate" >/dev/null 2>&1; then
            cc_bin="$candidate"
            break
        fi
    done
    [[ -n "$cc_bin" ]] || return 1
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
    "$cc_bin" -static -O2 -s -DVERSION="\"9.8.7\"" -o "$path" "$src" || return 1
    rm -f "$src"
    chmod 0755 "$path"
    file "$path" | grep -q 'statically linked' || return 1
}

if ! make_static_binary "$TMP/qualified/terraphim-agent" terraphim-agent; then
    require_tool_or_skip "no static-linking C compiler available (CC/cc/gcc/clang)"
    exit 0
fi
make_static_binary "$TMP/qualified/terraphim-grep" terraphim-grep ||
    fail "failed to build the terraphim-grep static fixture"

# ---------------------------------------------------------------------------
# 1. Production probe: full pipeline with real lintian/rpmlint (host tools
#    or Docker). The exact justified diagnostics must be accepted for both
#    binaries and the evidence preserved in the output.
# ---------------------------------------------------------------------------
test_production_static_elf_probe_passes_exact_justified_diagnostics() {
    if ! command -v lintian >/dev/null 2>&1 && ! docker_available; then
        require_tool_or_skip "production static probe needs host lintian or Docker (rpmlint likewise)"
        return 0
    fi

    local out="$TMP/prod-out"
    local log="$TMP/prod-run.log"
    if "$BUILD" \
        --version 9.8.7 \
        --target x86_64-unknown-linux-musl \
        --agent-binary "$TMP/qualified/terraphim-agent" \
        --grep-binary "$TMP/qualified/terraphim-grep" \
        --out-dir "$out" \
        --nfpm "$NFPM_BIN" >"$log" 2>&1; then
        :
    else
        fail "production build-client-packages.sh rejected fully-static MUSL payloads: $(tail -30 "$log")"
    fi

    local bin
    for bin in terraphim-agent terraphim-grep; do
        grep -Fq "E: ${bin}: statically-linked-binary [usr/bin/${bin}]" "$log" ||
            fail "lintian evidence missing the justified static diagnostic for $bin: $(cat "$log")"
        grep -Fq "${bin}.x86_64: E: statically-linked-binary /usr/bin/${bin}" "$log" ||
            fail "rpmlint evidence missing the justified static diagnostic for $bin: $(cat "$log")"
    done
    grep -Fc 'lint policy satisfied: lintian accepted' "$log" | grep -Fxq 2 ||
        fail "missing both lintian policy verdicts: $(cat "$log")"
    grep -Fc 'lint policy satisfied: rpmlint accepted' "$log" | grep -Fxq 2 ||
        fail "missing both rpmlint policy verdicts: $(cat "$log")"
    grep -Fq 'package payload ok terraphim-agent' "$log" ||
        fail "production probe did not complete terraphim-agent payload qualification"
    grep -Fq 'package payload ok terraphim-grep' "$log" ||
        fail "production probe did not complete terraphim-grep payload qualification"
    [[ -f "$out/terraphim-agent_9.8.7-1_amd64.deb" ]] || fail "probe terraphim-agent DEB missing"
    [[ -f "$out/terraphim-agent-9.8.7-1.x86_64.rpm" ]] || fail "probe terraphim-agent RPM missing"
    [[ -f "$out/terraphim-grep_9.8.7-1_amd64.deb" ]] || fail "probe terraphim-grep DEB missing"
    [[ -f "$out/terraphim-grep-9.8.7-1.x86_64.rpm" ]] || fail "probe terraphim-grep RPM missing"
    [[ -f "$out/terraphim-clients-9.8.7-x86_64-unknown-linux-musl.package-sha256sums.txt" ]] ||
        fail "probe package checksum manifest missing"
}

# ---------------------------------------------------------------------------
# 2/3/4. Stub-driven mutation negatives and clean-dynamic acceptance through
# the production lint functions (PATH-injected stub tools are resolved by
# the same `command -v` the production host path uses). Exercised against
# BIN_NAME=terraphim-agent unless a scenario specifically tests cross-binary
# scoping.
# ---------------------------------------------------------------------------
STUB_BIN="$TMP/stub-bin"
mkdir -p "$STUB_BIN"

write_stub() {
    # write_stub <tool> <exit-code> <output-file>
    local tool="$1" rc="$2" out="$3"
    # shellcheck disable=SC2016
    printf '#!/usr/bin/env bash\ncat %q\nexit %s\n' "$out" "$rc" > "$STUB_BIN/$tool"
    chmod 0755 "$STUB_BIN/$tool"
}

# Run a production lint function in a sourced subshell with the stub tools
# on PATH; captures the exit status without tripping this test's set -e.
run_lint() {
    local fn="$1" pkg="$2" bin_name="${3:-terraphim-agent}"
    (
        export PATH="$STUB_BIN:$PATH"
        export WORK_DIR="$TMP/work"
        export RPM_ARCH=x86_64
        export DEB_ARCH=amd64
        export BIN_NAME="$bin_name"
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        "$fn" "$pkg"
    ) >"$TMP/lint.stdout" 2>"$TMP/lint.stderr"
}

expect_lint_pass() {
    local fn="$1" pkg="$2" scenario="$3"
    if ! run_lint "$fn" "$pkg"; then
        fail "lint scenario '$scenario' should pass: $(cat "$TMP/lint.stderr")"
    fi
    grep -Fq 'lint policy satisfied' "$TMP/lint.stdout" ||
        fail "lint scenario '$scenario' passed without a policy verdict: $(cat "$TMP/lint.stdout" "$TMP/lint.stderr")"
}

expect_lint_fail() {
    local fn="$1" pkg="$2" scenario="$3" diagnostic="$4"
    if run_lint "$fn" "$pkg"; then
        fail "lint scenario '$scenario' must fail closed, but it passed: $(cat "$TMP/lint.stdout")"
    fi
    grep -Fq -- "$diagnostic" "$TMP/lint.stdout" "$TMP/lint.stderr" ||
        fail "lint scenario '$scenario' missing diagnostic '$diagnostic': $(cat "$TMP/lint.stdout" "$TMP/lint.stderr")"
}

LINTIAN_JUSTIFIED='E: terraphim-agent: statically-linked-binary [usr/bin/terraphim-agent]'
RPMLINT_JUSTIFIED='terraphim-agent.x86_64: E: statically-linked-binary /usr/bin/terraphim-agent'
# Verified against the real qualified x86_64 MUSL terraphim-agent binary
# (lintian 2.116.3; see /tmp/client-real-package-build.log and the
# remediation record): it bundles a statically-linked copy of libyaml that
# lintian's embedded-code heuristic detects, independent of the
# statically-linked-binary diagnostic. terraphim-grep's real binary does not
# emit this diagnostic, so it must stay scoped to terraphim-agent only.
LINTIAN_EMBEDDED_LIBYAML_JUSTIFIED='E: terraphim-agent: embedded-library libyaml [usr/bin/terraphim-agent]'

make_lintian_output() { printf '%s\n' "$@" > "$TMP/lintian-out"; }
make_rpmlint_output() { printf '%s\n' "$@" > "$TMP/rpmlint-out"; }

build_fixture_packages() {
    local bin_name="$1"
    local bin="$TMP/qualified/$bin_name"
    "$RENDER" --format deb --binary-name "$bin_name" --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$bin" --output "$TMP/$bin_name-deb.yaml" >/dev/null
    "$RENDER" --format rpm --binary-name "$bin_name" --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$bin" --output "$TMP/$bin_name-rpm.yaml" >/dev/null
    "$NFPM_BIN" pkg --packager deb --config "$TMP/$bin_name-deb.yaml" --target "$TMP" >/dev/null
    "$NFPM_BIN" pkg --packager rpm --config "$TMP/$bin_name-rpm.yaml" --target "$TMP" >/dev/null
}

build_fixture_packages terraphim-agent
build_fixture_packages terraphim-grep
mkdir -p "$TMP/work"

DEB_PKG="$TMP/terraphim-agent_9.8.7-1_amd64.deb"
RPM_PKG="$TMP/terraphim-agent-9.8.7-1.x86_64.rpm"
GREP_DEB_PKG="$TMP/terraphim-grep_9.8.7-1_amd64.deb"
GREP_RPM_PKG="$TMP/terraphim-grep-9.8.7-1.x86_64.rpm"

[[ -f "$DEB_PKG" ]] || fail "fixture terraphim-agent DEB was not built"
[[ -f "$RPM_PKG" ]] || fail "fixture terraphim-agent RPM was not built"
[[ -f "$GREP_DEB_PKG" ]] || fail "fixture terraphim-grep DEB was not built"
[[ -f "$GREP_RPM_PKG" ]] || fail "fixture terraphim-grep RPM was not built"

# Canonical stub results mirroring the real tool observations pinned from
# the production probe (lintian 2.116 exits 2 on errors, rpmlint 2.8 exits
# 64; both print the exact justified line plus nonfatal warnings).
test_canonical_justified_static_passes() {
    make_lintian_output \
        'N: running with root privileges is not recommended!' \
        "$LINTIAN_JUSTIFIED" \
        'W: terraphim-agent: initial-upload-closes-no-bugs [usr/share/doc/terraphim-agent/changelog.Debian.gz:1]'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_pass lint_deb "$DEB_PKG" "lintian exact justified static diagnostic"
    grep -Fq 'with 1 justified static-MUSL diagnostic(s)' "$TMP/lint.stdout" ||
        fail "lintian verdict must count exactly one justified diagnostic"

    make_rpmlint_output \
        '============================ rpmlint session starts ============================' \
        "$RPMLINT_JUSTIFIED" \
        'terraphim-agent.x86_64: W: position-independent-executable-suggested /usr/bin/terraphim-agent' \
        ' 1 packages and 0 specfiles checked; 1 errors, 1 warnings, 0 filtered, 1 badness'
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_pass lint_rpm "$RPM_PKG" "rpmlint exact justified static diagnostic"
    grep -Fq 'with 1 justified static-MUSL diagnostic(s)' "$TMP/lint.stdout" ||
        fail "rpmlint verdict must count exactly one justified diagnostic"
}

# Dynamic fixtures (the native gate) rely on a genuinely clean result: zero
# error lines and a clean exit must keep passing.
test_clean_dynamic_result_still_passes() {
    make_lintian_output \
        'W: terraphim-agent: initial-upload-closes-no-bugs [usr/share/doc/terraphim-agent/changelog.Debian.gz:1]'
    write_stub lintian 0 "$TMP/lintian-out"
    expect_lint_pass lint_deb "$DEB_PKG" "lintian clean dynamic result"
    grep -Fq 'with 0 justified static-MUSL diagnostic(s)' "$TMP/lint.stdout" ||
        fail "clean lintian verdict must count zero justified diagnostics"

    make_rpmlint_output \
        '============================ rpmlint session starts ============================' \
        'terraphim-agent.x86_64: W: position-independent-executable-suggested /usr/bin/terraphim-agent' \
        ' 1 packages and 0 specfiles checked; 0 errors, 1 warnings, 0 filtered, 0 badness'
    write_stub rpmlint 0 "$TMP/rpmlint-out"
    expect_lint_pass lint_rpm "$RPM_PKG" "rpmlint clean dynamic result"
}

test_injected_extra_error_fails() {
    make_lintian_output "$LINTIAN_JUSTIFIED" \
        'E: terraphim-agent: another-real-error [usr/bin/terraphim-agent]'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian extra injected error" \
        'unjustified error'

    make_rpmlint_output "$RPMLINT_JUSTIFIED" \
        'terraphim-agent.x86_64: E: no-documentation'
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint extra injected error" \
        'unjustified error'
}

test_wrong_path_variant_fails() {
    make_lintian_output \
        'E: terraphim-agent: statically-linked-binary [usr/sbin/terraphim-agent]'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian wrong path" 'unjustified error'

    make_rpmlint_output \
        'terraphim-agent.x86_64: E: statically-linked-binary /usr/sbin/terraphim-agent'
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint wrong path" 'unjustified error'
}

test_wrong_package_variant_fails() {
    make_lintian_output \
        'E: terraphim-other: statically-linked-binary [usr/bin/terraphim-agent]'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian wrong package" 'unjustified error'

    make_rpmlint_output \
        'terraphim-other.x86_64: E: statically-linked-binary /usr/bin/terraphim-agent'
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint wrong package" 'unjustified error'

    # Wrong arch in the rpmlint N-V-R.A prefix is a wrong package identity.
    make_rpmlint_output \
        'terraphim-agent.aarch64: E: statically-linked-binary /usr/bin/terraphim-agent'
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint wrong arch identity" 'unjustified error'
}

# Cross-binary specificity: this is the generalization-specific scenario the
# single-binary reference could not exercise. The justified diagnostic must
# be scoped to $BIN_NAME, so a diagnostic that is genuinely valid for
# terraphim-grep must not be allowlisted while linting terraphim-agent's
# package (and vice versa).
test_cross_binary_diagnostic_is_not_justified() {
    make_lintian_output \
        'E: terraphim-grep: statically-linked-binary [usr/bin/terraphim-grep]'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian cross-binary diagnostic (agent linted against grep's line)" \
        'unjustified error'

    make_rpmlint_output \
        'terraphim-grep.x86_64: E: statically-linked-binary /usr/bin/terraphim-grep'
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint cross-binary diagnostic (agent linted against grep's line)" \
        'unjustified error'

    # And the reverse direction: terraphim-agent's line must not be
    # allowlisted while linting terraphim-grep's package.
    make_lintian_output \
        'E: terraphim-agent: statically-linked-binary [usr/bin/terraphim-agent]'
    write_stub lintian 2 "$TMP/lintian-out"
    if run_lint lint_deb "$GREP_DEB_PKG" terraphim-grep; then
        fail "lint scenario 'lintian cross-binary diagnostic (grep linted against agent's line)' must fail closed, but it passed: $(cat "$TMP/lint.stdout")"
    fi
    grep -Fq 'unjustified error' "$TMP/lint.stdout" "$TMP/lint.stderr" ||
        fail "missing unjustified-error diagnostic: $(cat "$TMP/lint.stdout" "$TMP/lint.stderr")"
}

# ---------------------------------------------------------------------------
# embedded-library libyaml: narrowly scoped to terraphim-agent, bound to the
# exact package/binary/path, never a wildcard for any other embedded
# library. Only lintian is exercised: rpmlint's real observation for the
# qualified binaries never included an embedded-library diagnostic (only
# lintian's embedded-code heuristic fired), so no rpmlint allowance exists
# to test.
# ---------------------------------------------------------------------------
test_embedded_library_libyaml_justified_for_terraphim_agent() {
    make_lintian_output "$LINTIAN_JUSTIFIED" "$LINTIAN_EMBEDDED_LIBYAML_JUSTIFIED"
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_pass lint_deb "$DEB_PKG" "lintian statically-linked-binary + embedded-library libyaml"
    grep -Fq 'with 2 justified static-MUSL diagnostic(s)' "$TMP/lint.stdout" ||
        fail "lintian verdict must count exactly two justified diagnostics"
}

test_embedded_library_different_library_fails() {
    make_lintian_output "$LINTIAN_JUSTIFIED" \
        'E: terraphim-agent: embedded-library libz [usr/bin/terraphim-agent]'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian embedded-library different from libyaml" \
        'unjustified error'
}

test_embedded_library_libyaml_not_justified_for_terraphim_grep() {
    # Correctly named for terraphim-grep at its own usr/bin path (not a
    # cross-binary mislabel): still must fail closed, because the
    # embedded-library libyaml allowance is scoped to terraphim-agent only.
    make_lintian_output 'E: terraphim-grep: embedded-library libyaml [usr/bin/terraphim-grep]'
    write_stub lintian 2 "$TMP/lintian-out"
    if run_lint lint_deb "$GREP_DEB_PKG" terraphim-grep; then
        fail "lint scenario 'lintian embedded-library libyaml is not justified for terraphim-grep' must fail closed, but it passed: $(cat "$TMP/lint.stdout")"
    fi
    grep -Fq 'unjustified error' "$TMP/lint.stdout" "$TMP/lint.stderr" ||
        fail "missing unjustified-error diagnostic: $(cat "$TMP/lint.stdout" "$TMP/lint.stderr")"
}

test_embedded_library_libyaml_wrong_path_fails() {
    make_lintian_output "$LINTIAN_JUSTIFIED" \
        'E: terraphim-agent: embedded-library libyaml [usr/sbin/terraphim-agent]'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian embedded-library libyaml wrong path" \
        'unjustified error'
}

test_embedded_library_libyaml_duplicate_fails() {
    make_lintian_output "$LINTIAN_JUSTIFIED" \
        "$LINTIAN_EMBEDDED_LIBYAML_JUSTIFIED" "$LINTIAN_EMBEDDED_LIBYAML_JUSTIFIED"
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian embedded-library libyaml duplicate" \
        'justified static-MUSL diagnostic more than once'
}

test_malformed_variant_fails() {
    # Parentheses instead of brackets (a plausible lintian formatting
    # change) must not be allowlisted by accident.
    make_lintian_output \
        'E: terraphim-agent: statically-linked-binary (usr/bin/terraphim-agent)'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian malformed brackets" 'unjustified error'

    # Extra tag argument/spacing variants are different diagnostics.
    make_lintian_output \
        'E: terraphim-agent: statically-linked-binary [usr/bin/terraphim-agent] extra'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian trailing junk" 'unjustified error'

    make_rpmlint_output \
        'terraphim-agent.x86_64: E: statically-linked-binary  /usr/bin/terraphim-agent'
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint malformed spacing" 'unjustified error'
}

test_duplicate_justified_line_fails() {
    make_lintian_output "$LINTIAN_JUSTIFIED" "$LINTIAN_JUSTIFIED"
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian duplicate justified" \
        'justified static-MUSL diagnostic more than once'

    make_rpmlint_output "$RPMLINT_JUSTIFIED" "$RPMLINT_JUSTIFIED"
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint duplicate justified" \
        'justified static-MUSL diagnostic more than once'
}

test_tool_failure_exit_fails() {
    # lintian usage/internal failures exit 25; 0 and 2 are the only
    # legitimate policy exits.
    make_lintian_output 'lintian: internal error'
    write_stub lintian 25 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian tool failure exit" \
        'tool/install/transport failure'

    # rpmlint exits 2 on usage/config errors; 0/64/65 are the only
    # legitimate policy exits.
    make_rpmlint_output 'rpmlint: error: no such file'
    write_stub rpmlint 2 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint tool failure exit" \
        'tool/install/transport failure'
}

test_inconsistent_status_fails() {
    # errors-exit without any parseable error line (lintian also exits 2
    # for an unreadable package, so this is the transport-failure guard).
    make_lintian_output \
        'W: terraphim-agent: initial-upload-closes-no-bugs [usr/share/doc/terraphim-agent/changelog.Debian.gz:1]'
    write_stub lintian 2 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian errors-exit without error line" \
        'no error line could be parsed'

    make_rpmlint_output \
        '============================ rpmlint session starts ============================' \
        ' 1 packages and 0 specfiles checked; 0 errors, 0 warnings, 0 filtered, 0 badness'
    write_stub rpmlint 64 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint errors-exit without error line" \
        'no error line could be parsed'

    # clean-exit with an error line present is equally inconsistent.
    make_lintian_output "$LINTIAN_JUSTIFIED"
    write_stub lintian 0 "$TMP/lintian-out"
    expect_lint_fail lint_deb "$DEB_PKG" "lintian clean-exit with error line" \
        'contains error lines'

    make_rpmlint_output "$RPMLINT_JUSTIFIED"
    write_stub rpmlint 0 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint clean-exit with error line" \
        'contains error lines'
}

test_empty_rpmlint_output_fails() {
    : > "$TMP/rpmlint-out"
    write_stub rpmlint 0 "$TMP/rpmlint-out"
    expect_lint_fail lint_rpm "$RPM_PKG" "rpmlint empty output" \
        'empty lint output'
}

# Full production script with an injected hostile lintian on PATH must fail
# closed (verify_deb lint runs before any RPM verification, and before the
# second binary is processed).
test_production_run_with_injected_extra_error_fails() {
    make_lintian_output "$LINTIAN_JUSTIFIED" \
        'E: terraphim-agent: injected-hostile-error [usr/bin/terraphim-agent]'
    write_stub lintian 2 "$TMP/lintian-out"

    local out="$TMP/hostile-out"
    if (
        export PATH="$STUB_BIN:$PATH"
        "$BUILD" \
            --version 9.8.7 \
            --target x86_64-unknown-linux-musl \
            --agent-binary "$TMP/qualified/terraphim-agent" \
            --grep-binary "$TMP/qualified/terraphim-grep" \
            --out-dir "$out" \
            --nfpm "$NFPM_BIN"
    ) >"$TMP/hostile.log" 2>&1; then
        fail "production pipeline accepted an injected extra lint error"
    fi
    grep -Fq 'unjustified error' "$TMP/hostile.log" ||
        fail "hostile run missing unjustified-error diagnostic: $(cat "$TMP/hostile.log")"
    [[ ! -f "$out/terraphim-clients-9.8.7-x86_64-unknown-linux-musl.package-sha256sums.txt" ]] ||
        fail "hostile run must not publish package checksum manifests"
}

# Full production script with a hostile rpmlint on PATH must fail closed
# (requires host rpm tooling or Docker for the RPM payload verification
# that precedes the RPM lint stage).
test_production_run_with_injected_wrong_path_rpm_error_fails() {
    if ! command -v rpm2cpio >/dev/null 2>&1 || ! command -v rpm >/dev/null 2>&1 || ! command -v cpio >/dev/null 2>&1; then
        if ! docker_available; then
            require_tool_or_skip "hostile RPM production run needs host rpm tooling or Docker"
            return 0
        fi
    fi

    make_lintian_output "$LINTIAN_JUSTIFIED"
    write_stub lintian 2 "$TMP/lintian-out"
    make_rpmlint_output \
        'terraphim-agent.x86_64: E: statically-linked-binary /usr/sbin/terraphim-agent'
    write_stub rpmlint 64 "$TMP/rpmlint-out"

    local out="$TMP/hostile-rpm-out"
    if (
        export PATH="$STUB_BIN:$PATH"
        "$BUILD" \
            --version 9.8.7 \
            --target x86_64-unknown-linux-musl \
            --agent-binary "$TMP/qualified/terraphim-agent" \
            --grep-binary "$TMP/qualified/terraphim-grep" \
            --out-dir "$out" \
            --nfpm "$NFPM_BIN"
    ) >"$TMP/hostile-rpm.log" 2>&1; then
        fail "production pipeline accepted an injected wrong-path RPM lint error"
    fi
    grep -Fq 'unjustified error' "$TMP/hostile-rpm.log" ||
        fail "hostile RPM run missing unjustified-error diagnostic: $(cat "$TMP/hostile-rpm.log")"
}

test_production_static_elf_probe_passes_exact_justified_diagnostics
test_canonical_justified_static_passes
test_clean_dynamic_result_still_passes
test_injected_extra_error_fails
test_wrong_path_variant_fails
test_wrong_package_variant_fails
test_cross_binary_diagnostic_is_not_justified
test_embedded_library_libyaml_justified_for_terraphim_agent
test_embedded_library_different_library_fails
test_embedded_library_libyaml_not_justified_for_terraphim_grep
test_embedded_library_libyaml_wrong_path_fails
test_embedded_library_libyaml_duplicate_fails
test_malformed_variant_fails
test_duplicate_justified_line_fails
test_tool_failure_exit_fails
test_inconsistent_status_fails
test_empty_rpmlint_output_fails
test_production_run_with_injected_extra_error_fails
test_production_run_with_injected_wrong_path_rpm_error_fails

echo "client nFPM static-MUSL lint policy tests passed"
