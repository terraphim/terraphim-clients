#!/usr/bin/env bash
# Hermetic tests for the terraphim-clients managed-package producer
# (terraphim-agent + terraphim-grep, DEB + RPM, per MUSL target).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
RENDER="$ROOT/.github/scripts/nfpm/render-client-nfpm.sh"
BUILD="$ROOT/.github/scripts/nfpm/build-client-packages.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/terraphim-client-nfpm-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
export SOURCE_DATE_EPOCH=1700000000

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

# REQUIRE_TOOLS=1 turns every prerequisite SKIP in this suite into a hard
# failure so wiring it into CI without provisioning dpkg-deb/nFPM cannot pass
# vacuously. Call sites still `return 0` (or `exit 0` at top level) on the
# SKIP path themselves; this only escalates when REQUIRE_TOOLS=1.
require_tool_or_skip() {
    local reason="$1"
    if [[ "${REQUIRE_TOOLS:-0}" == "1" ]]; then
        fail "REQUIRE_TOOLS=1: $reason"
    fi
    echo "SKIP: $reason" >&2
}

assert_contains() {
    local file="$1"
    local pattern="$2"
    grep -Fq -- "$pattern" "$file" || fail "expected '$pattern' in $file"
}

assert_not_contains() {
    local file="$1"
    local pattern="$2"
    ! grep -Fq -- "$pattern" "$file" || fail "did not expect '$pattern' in $file"
}

make_fixture_binary() {
    local path="$1"
    local bin_name="$2"
    mkdir -p "$(dirname "$path")"
    printf '#!/usr/bin/env sh\nprintf "%s 9.8.7\\n" "%s"\n' "$bin_name" "$bin_name" > "$path"
    chmod 0755 "$path"
}

make_elf_header_fixture() {
    local path="$1"
    local machine="$2"
    mkdir -p "$(dirname "$path")"
    case "$machine" in
        x86_64) machine='\x3e\x00' ;;
        aarch64) machine='\xb7\x00' ;;
        *) fail "unsupported ELF fixture machine: $machine" ;;
    esac
    # A deterministic ELF64 little-endian executable header is sufficient for
    # source qualification tests; these fixtures are never executed.
    printf '%b' \
        "\x7f\x45\x4c\x46\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00${machine}\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x40\x00\x38\x00\x00\x00\x40\x00\x00\x00\x00\x00" > "$path"
    chmod 0755 "$path"
}

# A genuine, real ELF64 x86_64 binary (unlike make_elf_header_fixture, which
# produces a bare header stub that real ELF tooling rejects with "file format
# not recognized"), pre-stripped before being handed to build-client-packages.sh
# -- exactly like a real qualified release binary must be, since the
# production script only validates that its inputs are already stripped and
# never strips them itself.
make_stripable_binary() {
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
    command -v strip >/dev/null 2>&1 || return 1
    local src="$path.fixture.c"
    mkdir -p "$(dirname "$path")"
    cat > "$src" <<EOF
#include <stdio.h>
int main(void) { printf("${bin_name}\n"); return 0; }
EOF
    "$cc_bin" -O0 -o "$path" "$src" || return 1
    rm -f "$src"
    strip --strip-unneeded -- "$path" || return 1
    chmod 0755 "$path"
}

BIN_NAMES=(terraphim-agent terraphim-grep)

test_render_deb_descriptor_for_each_binary() {
    for bin_name in "${BIN_NAMES[@]}"; do
        local bin="$TMP/target/x86_64-unknown-linux-musl/release/$bin_name"
        local yaml="$TMP/$bin_name-deb.yaml"
        make_fixture_binary "$bin" "$bin_name"

        "$RENDER" --format deb --binary-name "$bin_name" --version 9.8.7 \
            --target x86_64-unknown-linux-musl --binary "$bin" --output "$yaml" >/dev/null

        assert_contains "$yaml" "name: $bin_name"
        assert_contains "$yaml" "arch: amd64"
        assert_contains "$yaml" "section: utils"
        assert_contains "$yaml" "dst: /usr/bin/$bin_name"
        assert_contains "$yaml" "dst: /usr/share/terraphim/package-manager.d/$bin_name"
        assert_contains "$yaml" "dst: /usr/share/doc/$bin_name/copyright"
        assert_contains "$yaml" "dst: /usr/share/doc/$bin_name/changelog.Debian.gz"
        assert_contains "$yaml" "dst: /usr/share/man/man1/$bin_name.1.gz"
        assert_contains "$yaml" "src: $yaml.$bin_name.receipt"
        grep -qx 'dpkg' "$yaml.$bin_name.receipt"
        [[ "$(stat -c '%a' "$yaml.$bin_name.receipt")" == "644" ]] || fail "receipt mode is not 0644 for $bin_name"
        assert_contains "$yaml" "depends: []"
        assert_not_contains "$yaml" "changelog:"
    done
}

test_render_rpm_descriptor_for_each_binary() {
    for bin_name in "${BIN_NAMES[@]}"; do
        local bin="$TMP/target/aarch64-unknown-linux-musl/release/$bin_name"
        local yaml="$TMP/$bin_name-rpm.yaml"
        make_fixture_binary "$bin" "$bin_name"

        "$RENDER" --format rpm --binary-name "$bin_name" --version 9.8.7 \
            --target aarch64-unknown-linux-musl --binary "$bin" --output "$yaml" >/dev/null

        assert_contains "$yaml" "arch: aarch64"
        assert_contains "$yaml" "src: $yaml.$bin_name.receipt"
        grep -qx 'rpm' "$yaml.$bin_name.receipt"
        assert_contains "$yaml" "dst: /usr/bin/$bin_name"
        assert_contains "$yaml" "changelog: $yaml.changelog.yaml"
        assert_contains "$yaml" "dst: /usr/share/licenses/$bin_name/"
        assert_contains "$yaml" "type: license"
        assert_contains "$yaml" "type: doc"
        grep -q 'semver: 9.8.7' "$yaml.changelog.yaml" || fail "changelog.yaml missing semver entry for $bin_name"
    done
}

test_render_uses_distinct_licenses_per_binary() {
    local agent_bin="$TMP/lic/terraphim-agent" grep_bin="$TMP/lic/terraphim-grep"
    local agent_yaml="$TMP/agent-license.yaml" grep_yaml="$TMP/grep-license.yaml"
    make_fixture_binary "$agent_bin" terraphim-agent
    make_fixture_binary "$grep_bin" terraphim-grep

    "$RENDER" --format deb --binary-name terraphim-agent --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$agent_bin" --output "$agent_yaml" >/dev/null
    "$RENDER" --format deb --binary-name terraphim-grep --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$grep_bin" --output "$grep_yaml" >/dev/null

    assert_contains "$agent_yaml" "license: Apache-2.0"
    assert_contains "$grep_yaml" "license: MIT"
    assert_contains "$agent_yaml" "dst: /usr/share/licenses/terraphim-agent/LICENSE-Apache-2.0"
    assert_contains "$grep_yaml" "dst: /usr/share/licenses/terraphim-grep/LICENSE-MIT"
}

test_render_rejects_gnu_target() {
    local bin="$TMP/target/x86_64-unknown-linux-gnu/release/terraphim-agent"
    local yaml="$TMP/gnu.yaml"
    make_fixture_binary "$bin" terraphim-agent

    if "$RENDER" --format deb --binary-name terraphim-agent --version 9.8.7 \
        --target x86_64-unknown-linux-gnu --binary "$bin" --output "$yaml" 2>"$TMP/reject.err"; then
        fail "renderer accepted a GNU target"
    fi
    assert_contains "$TMP/reject.err" "only qualified MUSL targets are accepted"
}

test_render_rejects_unsupported_binary_name() {
    local bin="$TMP/target/x86_64-unknown-linux-musl/release/terraphim-server"
    local yaml="$TMP/badname.yaml"
    make_fixture_binary "$bin" terraphim-server

    if "$RENDER" --format deb --binary-name terraphim-server --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$bin" --output "$yaml" 2>"$TMP/badname.err"; then
        fail "renderer accepted an unsupported binary name"
    fi
    assert_contains "$TMP/badname.err" "only terraphim-agent and terraphim-grep are accepted"
}

# ---------------------------------------------------------------------------
# #326 P2-1: the renderer must independently validate --version and --binary
# rather than trusting its caller (defense-in-depth: build-client-packages.sh
# already validates VERSION before invoking the renderer in production, so
# these prove the renderer's OWN gate, not just the caller's).
# ---------------------------------------------------------------------------
test_render_rejects_injected_newline_version() {
    local bin="$TMP/target/x86_64-unknown-linux-musl/release/terraphim-agent"
    local yaml="$TMP/injected-version.yaml"
    make_fixture_binary "$bin" terraphim-agent
    local malicious_version=$'9.9.9\nprovides: ["INJECTED-PKG"]'

    if "$RENDER" --format deb --binary-name terraphim-agent --version "$malicious_version" \
        --target x86_64-unknown-linux-musl --binary "$bin" --output "$yaml" 2>"$TMP/injected-version.err"; then
        fail "renderer accepted a version containing a newline (YAML injection)"
    fi
    assert_contains "$TMP/injected-version.err" "unsupported client package version"
    [[ ! -e "$yaml" ]] || fail "renderer produced a config despite rejecting the malicious version"
}

test_render_rejects_prerelease_and_build_metadata_versions() {
    local bin="$TMP/target/x86_64-unknown-linux-musl/release/terraphim-agent"
    make_fixture_binary "$bin" terraphim-agent

    local version
    for version in "1.2.3-rc.1" "1.2.3-rc-1" "1.2.3+build.1" "1.2.3-rc.1+b.2"; do
        local yaml="$TMP/reject-version-$RANDOM.yaml"
        if "$RENDER" --format deb --binary-name terraphim-agent --version "$version" \
            --target x86_64-unknown-linux-musl --binary "$bin" --output "$yaml" 2>"$TMP/reject-version.err"; then
            fail "renderer accepted unsupported version form: $version"
        fi
        assert_contains "$TMP/reject-version.err" "unsupported client package version"
        [[ ! -e "$yaml" ]] || fail "renderer produced a config for rejected version: $version"
    done
}

test_render_rejects_symlinked_binary_path() {
    local real="$TMP/symlink-src/real-terraphim-agent"
    local linked="$TMP/symlink-src/terraphim-agent"
    make_fixture_binary "$real" terraphim-agent
    ln -s "$real" "$linked"
    local yaml="$TMP/reject-symlink.yaml"

    if "$RENDER" --format deb --binary-name terraphim-agent --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$linked" --output "$yaml" 2>"$TMP/reject-symlink.err"; then
        fail "renderer accepted a symlinked binary path"
    fi
    assert_contains "$TMP/reject-symlink.err" "must be a regular non-symlink file"
    [[ ! -e "$yaml" ]] || fail "renderer produced a config for a symlinked binary path"
}

test_render_rejects_binary_path_with_embedded_newline() {
    local bin=$'/tmp/does-not-exist\n/terraphim-agent'
    local yaml="$TMP/reject-binary-newline.yaml"

    if "$RENDER" --format deb --binary-name terraphim-agent --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$bin" --output "$yaml" 2>"$TMP/reject-binary-newline.err"; then
        fail "renderer accepted a binary path containing a newline"
    fi
    assert_contains "$TMP/reject-binary-newline.err" "must not contain newline or control characters"
    [[ ! -e "$yaml" ]] || fail "renderer produced a config for an unsafe binary path"
}

test_render_rejects_output_path_with_embedded_newline() {
    local bin="$TMP/target/x86_64-unknown-linux-musl/release/terraphim-agent"
    local output="$TMP/unsafe-output"$'\nprovides: ["INJECTED-PKG"]'
    make_fixture_binary "$bin" terraphim-agent

    if "$RENDER" --format deb --binary-name terraphim-agent --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$bin" --output "$output" \
        2>"$TMP/reject-output-newline.err"; then
        fail "renderer accepted an output path containing a newline"
    fi
    assert_contains "$TMP/reject-output-newline.err" "output path (must not contain newline or control characters)"
    [[ ! -e "$output" ]] || fail "renderer produced a config for an unsafe output path"
}

test_render_quotes_version_and_binary_src_safely() {
    local bin="$TMP/target/x86_64-unknown-linux-musl/release/terraphim-agent"
    local yaml="$TMP/quoted.yaml"
    make_fixture_binary "$bin" terraphim-agent

    "$RENDER" --format deb --binary-name terraphim-agent --version 9.8.7 \
        --target x86_64-unknown-linux-musl --binary "$bin" --output "$yaml" >/dev/null

    assert_contains "$yaml" "version: '9.8.7'"
    assert_contains "$yaml" "- src: '$bin'"
}

expect_build_source_fail() {
    local message="$1"
    local target="$2"
    local agent_binary="$3"
    local grep_binary="$4"
    local label="$5"
    local log="$TMP/source-$label.log"
    if "$BUILD" --version 9.8.7 --target "$target" \
        --agent-binary "$agent_binary" --grep-binary "$grep_binary" \
        --out-dir "$TMP/source-$label-out" --nfpm /bin/true >"$log" 2>&1; then
        fail "build accepted unsafe source binary ($label)"
    fi
    assert_contains "$log" "$message"
}

test_build_rejects_unqualified_source_binaries() {
    local good_grep="$TMP/source-good/terraphim-grep"
    make_elf_header_fixture "$good_grep" x86_64

    local real="$TMP/source-real/terraphim-agent"
    local linked="$TMP/source-linked/terraphim-agent"
    local empty="$TMP/source-empty/terraphim-agent"
    local directory="$TMP/source-directory/terraphim-agent"
    local text="$TMP/source-text/terraphim-agent"
    local arm="$TMP/source-arm/terraphim-agent"

    make_elf_header_fixture "$real" x86_64
    mkdir -p "$(dirname "$linked")" "$(dirname "$empty")"
    ln -s "$real" "$linked"
    : > "$empty"
    mkdir -p "$directory"
    make_fixture_binary "$text" terraphim-agent
    make_elf_header_fixture "$arm" aarch64

    expect_build_source_fail "qualified terraphim-agent binary must be a regular non-symlink file" \
        x86_64-unknown-linux-musl "$linked" "$good_grep" symlink
    expect_build_source_fail "qualified terraphim-agent binary must not be zero-length" \
        x86_64-unknown-linux-musl "$empty" "$good_grep" empty
    expect_build_source_fail "qualified terraphim-agent binary must be a regular non-symlink file" \
        x86_64-unknown-linux-musl "$directory" "$good_grep" directory
    expect_build_source_fail "qualified terraphim-agent binary is not a valid ELF file" \
        x86_64-unknown-linux-musl "$text" "$good_grep" non-elf
    expect_build_source_fail "qualified terraphim-agent binary ELF architecture mismatch" \
        aarch64-unknown-linux-musl "$real" "$good_grep" x86-as-arm
    expect_build_source_fail "qualified terraphim-agent binary ELF architecture mismatch" \
        x86_64-unknown-linux-musl "$arm" "$good_grep" arm-as-x86
}

make_deb_payload_fixture() {
    local root="$1"
    local deb="$2"
    local bin_name="$3"
    local payload_type="$4"
    mkdir -p "$root/DEBIAN" "$root/usr/bin" \
        "$root/usr/share/terraphim/package-manager.d"
    case "$payload_type" in
        symlink)
            printf 'validated payload\n' > "$root/usr/bin/real-$bin_name"
            ln -s "real-$bin_name" "$root/usr/bin/$bin_name"
            ;;
        empty)
            : > "$root/usr/bin/$bin_name"
            ;;
        directory)
            mkdir "$root/usr/bin/$bin_name"
            ;;
        *) fail "unsupported DEB payload fixture type: $payload_type" ;;
    esac
    printf 'dpkg\n' > "$root/usr/share/terraphim/package-manager.d/$bin_name"
    cat > "$root/DEBIAN/control" <<EOF
Package: $bin_name
Version: 9.8.7
Section: utility
Priority: optional
Architecture: amd64
Maintainer: Terraphim Contributors <team@terraphim.ai>
Description: Terraphim client payload validation fixture
EOF
    dpkg-deb --build --root-owner-group "$root" "$deb" >/dev/null
}

expect_deb_payload_fail() {
    local payload_type="$1"
    local expected_sha="$2"
    local root="$TMP/deb-$payload_type-root"
    local deb="$TMP/deb-$payload_type.deb"
    local log="$TMP/deb-$payload_type.log"
    make_deb_payload_fixture "$root" "$deb" terraphim-agent "$payload_type"

    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/deb-$payload_type-work"
        BIN_NAME=terraphim-agent
        DEB_ARCH=amd64
        EXPECTED_SHA="$expected_sha"
        mkdir -p "$WORK_DIR"
        lint_deb() { :; }
        verify_deb "$deb"
    ) >"$log" 2>&1; then
        fail "verify_deb accepted $payload_type payload"
    fi
    assert_contains "$log" "extracted DEB payload must be a non-empty regular non-symlink file"
}

expect_rpm_payload_fail() {
    local payload_type="$1"
    local expected_sha="$2"
    local rpm="$TMP/rpm-$payload_type.rpm"
    local log="$TMP/rpm-$payload_type.log"
    printf 'fixture rpm\n' > "$rpm"

    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/rpm-$payload_type-work"
        BIN_NAME=terraphim-agent
        RPM_ARCH=x86_64
        EXPECTED_SHA="$expected_sha"
        mkdir -p "$WORK_DIR"
        docker_rpm_tool() {
            local extract="$2"
            local metadata="$4"
            mkdir -p "$extract/usr/bin" \
                "$extract/usr/share/terraphim/package-manager.d"
            case "$payload_type" in
                symlink)
                    printf 'validated payload\n' > "$extract/usr/bin/real-terraphim-agent"
                    ln -s real-terraphim-agent "$extract/usr/bin/terraphim-agent"
                    ;;
                empty)
                    : > "$extract/usr/bin/terraphim-agent"
                    ;;
                directory)
                    mkdir "$extract/usr/bin/terraphim-agent"
                    ;;
            esac
            printf 'rpm\n' > "$extract/usr/share/terraphim/package-manager.d/terraphim-agent"
            printf 'arch=x86_64\nrequires<<EOF\nEOF\nfile_digest=8\n' > "$metadata"
        }
        lint_rpm() { :; }
        command() {
            if [[ "$1" == "-v" &&
                ( "$2" == "rpm2cpio" || "$2" == "rpm" || "$2" == "cpio" ) ]]; then
                return 1
            fi
            builtin command "$@"
        }
        verify_rpm "$rpm"
    ) >"$log" 2>&1; then
        fail "verify_rpm accepted $payload_type payload"
    fi
    assert_contains "$log" "extracted RPM payload must be a non-empty regular non-symlink file"
}

test_extracted_payload_type_and_size_are_rejected() {
    command -v dpkg-deb >/dev/null 2>&1 || { require_tool_or_skip "dpkg-deb not installed"; return 0; }
    local content_sha empty_sha
    content_sha="$(printf 'validated payload\n' | sha256sum | awk '{print $1}')"
    empty_sha="$(sha256sum /dev/null | awk '{print $1}')"

    expect_deb_payload_fail symlink "$content_sha"
    expect_deb_payload_fail empty "$empty_sha"
    expect_deb_payload_fail directory "$empty_sha"
    expect_rpm_payload_fail symlink "$content_sha"
    expect_rpm_payload_fail empty "$empty_sha"
    expect_rpm_payload_fail directory "$empty_sha"
}

# ---------------------------------------------------------------------------
# Mutation coverage for the payload-SHA and receipt enforcement blocks in
# verify_deb/verify_rpm (build-client-packages.sh:501-504, :506, :552-555,
# :557). Unlike test_extracted_payload_type_and_size_are_rejected (which
# proves symlink/empty/directory payloads are rejected before a SHA is even
# computed), these tests exercise a *regular, non-empty* extracted payload
# whose bytes simply do not match EXPECTED_SHA, and a regular payload whose
# receipt file is missing or carries the wrong package-manager value -- the
# branches a mutation that replaces either enforcement block with `:` would
# silently defeat. Real dpkg-deb --build/--extract is used for DEB; the RPM
# side stubs docker_rpm_tool exactly like expect_rpm_payload_fail so the
# extracted content is deterministic regardless of host rpm tooling. No host
# install is performed.
# ---------------------------------------------------------------------------
make_deb_content_payload_fixture() {
    local root="$1"
    local deb="$2"
    local bin_name="$3"
    local payload_content="$4"
    local receipt_mode="$5" # a literal receipt value ("dpkg", "apt", ...) or "missing"

    mkdir -p "$root/DEBIAN" "$root/usr/bin"
    printf '%s' "$payload_content" > "$root/usr/bin/$bin_name"
    if [[ "$receipt_mode" != "missing" ]]; then
        mkdir -p "$root/usr/share/terraphim/package-manager.d"
        printf '%s\n' "$receipt_mode" > "$root/usr/share/terraphim/package-manager.d/$bin_name"
    fi
    cat > "$root/DEBIAN/control" <<EOF
Package: $bin_name
Version: 9.8.7
Section: utility
Priority: optional
Architecture: amd64
Maintainer: Terraphim Contributors <team@terraphim.ai>
Description: Terraphim client payload validation fixture
EOF
    dpkg-deb --build --root-owner-group "$root" "$deb" >/dev/null
}

test_verify_deb_rejects_payload_sha_mismatch() {
    command -v dpkg-deb >/dev/null 2>&1 || { require_tool_or_skip "dpkg-deb not installed"; return 0; }
    local root="$TMP/deb-sha-mismatch-root" deb="$TMP/deb-sha-mismatch.deb" log="$TMP/deb-sha-mismatch.log"
    make_deb_content_payload_fixture "$root" "$deb" terraphim-agent $'actual payload bytes\n' dpkg
    local wrong_sha
    wrong_sha="$(printf 'a completely different payload\n' | sha256sum | awk '{print $1}')"

    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/deb-sha-mismatch-work"
        BIN_NAME=terraphim-agent
        DEB_ARCH=amd64
        EXPECTED_SHA="$wrong_sha"
        mkdir -p "$WORK_DIR"
        lint_deb() { :; }
        verify_deb "$deb"
    ) >"$log" 2>&1; then
        fail "verify_deb accepted a regular DEB payload whose SHA-256 did not match EXPECTED_SHA"
    fi
    assert_contains "$log" "DEB payload SHA mismatch"
}

test_verify_deb_rejects_missing_receipt() {
    command -v dpkg-deb >/dev/null 2>&1 || { require_tool_or_skip "dpkg-deb not installed"; return 0; }
    local root="$TMP/deb-receipt-missing-root" deb="$TMP/deb-receipt-missing.deb" log="$TMP/deb-receipt-missing.log"
    local payload=$'payload for missing DEB receipt\n'
    make_deb_content_payload_fixture "$root" "$deb" terraphim-agent "$payload" missing
    local correct_sha
    correct_sha="$(printf '%s' "$payload" | sha256sum | awk '{print $1}')"

    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/deb-receipt-missing-work"
        BIN_NAME=terraphim-agent
        DEB_ARCH=amd64
        EXPECTED_SHA="$correct_sha"
        mkdir -p "$WORK_DIR"
        lint_deb() { :; }
        verify_deb "$deb"
    ) >"$log" 2>&1; then
        fail "verify_deb accepted a DEB with no package-manager receipt file (payload SHA matched)"
    fi
    assert_contains "$log" "DEB package-manager receipt missing or does not read exactly 'dpkg'"
}

test_verify_deb_rejects_wrong_receipt() {
    command -v dpkg-deb >/dev/null 2>&1 || { require_tool_or_skip "dpkg-deb not installed"; return 0; }
    local root="$TMP/deb-receipt-wrong-root" deb="$TMP/deb-receipt-wrong.deb" log="$TMP/deb-receipt-wrong.log"
    local payload=$'payload for wrong DEB receipt\n'
    make_deb_content_payload_fixture "$root" "$deb" terraphim-agent "$payload" apt
    local correct_sha
    correct_sha="$(printf '%s' "$payload" | sha256sum | awk '{print $1}')"

    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/deb-receipt-wrong-work"
        BIN_NAME=terraphim-agent
        DEB_ARCH=amd64
        EXPECTED_SHA="$correct_sha"
        mkdir -p "$WORK_DIR"
        lint_deb() { :; }
        verify_deb "$deb"
    ) >"$log" 2>&1; then
        fail "verify_deb accepted a DEB whose receipt said 'apt' instead of 'dpkg' (payload SHA matched)"
    fi
    assert_contains "$log" "DEB package-manager receipt missing or does not read exactly 'dpkg'"
}

rpm_docker_env_hides_host_rpm_tools() {
    command() {
        if [[ "$1" == "-v" && ( "$2" == "rpm2cpio" || "$2" == "rpm" || "$2" == "cpio" ) ]]; then
            return 1
        fi
        builtin command "$@"
    }
}

test_verify_rpm_rejects_payload_sha_mismatch() {
    local rpm="$TMP/rpm-sha-mismatch.rpm" log="$TMP/rpm-sha-mismatch.log"
    printf 'fixture rpm\n' > "$rpm"
    local wrong_sha
    wrong_sha="$(printf 'a completely different payload\n' | sha256sum | awk '{print $1}')"

    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/rpm-sha-mismatch-work"
        BIN_NAME=terraphim-agent
        RPM_ARCH=x86_64
        EXPECTED_SHA="$wrong_sha"
        mkdir -p "$WORK_DIR"
        docker_rpm_tool() {
            local extract="$2"
            local metadata="$4"
            mkdir -p "$extract/usr/bin" "$extract/usr/share/terraphim/package-manager.d"
            printf 'actual rpm payload bytes\n' > "$extract/usr/bin/terraphim-agent"
            printf 'rpm\n' > "$extract/usr/share/terraphim/package-manager.d/terraphim-agent"
            printf 'arch=x86_64\nrequires<<EOF\nEOF\nfile_digest=8\n' > "$metadata"
        }
        lint_rpm() { :; }
        rpm_docker_env_hides_host_rpm_tools
        verify_rpm "$rpm"
    ) >"$log" 2>&1; then
        fail "verify_rpm accepted a regular RPM payload whose SHA-256 did not match EXPECTED_SHA"
    fi
    assert_contains "$log" "RPM payload SHA mismatch"
}

test_verify_rpm_rejects_missing_receipt() {
    local rpm="$TMP/rpm-receipt-missing.rpm" log="$TMP/rpm-receipt-missing.log"
    printf 'fixture rpm\n' > "$rpm"
    local payload=$'payload for missing RPM receipt\n'
    local correct_sha
    correct_sha="$(printf '%s' "$payload" | sha256sum | awk '{print $1}')"

    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/rpm-receipt-missing-work"
        BIN_NAME=terraphim-agent
        RPM_ARCH=x86_64
        EXPECTED_SHA="$correct_sha"
        mkdir -p "$WORK_DIR"
        docker_rpm_tool() {
            local extract="$2"
            local metadata="$4"
            mkdir -p "$extract/usr/bin"
            printf '%s' "$payload" > "$extract/usr/bin/terraphim-agent"
            printf 'arch=x86_64\nrequires<<EOF\nEOF\nfile_digest=8\n' > "$metadata"
        }
        lint_rpm() { :; }
        rpm_docker_env_hides_host_rpm_tools
        verify_rpm "$rpm"
    ) >"$log" 2>&1; then
        fail "verify_rpm accepted an RPM with no package-manager receipt file (payload SHA matched)"
    fi
    assert_contains "$log" "RPM package-manager receipt missing or does not read exactly 'rpm'"
}

test_verify_rpm_rejects_wrong_receipt() {
    local rpm="$TMP/rpm-receipt-wrong.rpm" log="$TMP/rpm-receipt-wrong.log"
    printf 'fixture rpm\n' > "$rpm"
    local payload=$'payload for wrong RPM receipt\n'
    local correct_sha
    correct_sha="$(printf '%s' "$payload" | sha256sum | awk '{print $1}')"

    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/rpm-receipt-wrong-work"
        BIN_NAME=terraphim-agent
        RPM_ARCH=x86_64
        EXPECTED_SHA="$correct_sha"
        mkdir -p "$WORK_DIR"
        docker_rpm_tool() {
            local extract="$2"
            local metadata="$4"
            mkdir -p "$extract/usr/bin" "$extract/usr/share/terraphim/package-manager.d"
            printf '%s' "$payload" > "$extract/usr/bin/terraphim-agent"
            printf 'dnf\n' > "$extract/usr/share/terraphim/package-manager.d/terraphim-agent"
            printf 'arch=x86_64\nrequires<<EOF\nEOF\nfile_digest=8\n' > "$metadata"
        }
        lint_rpm() { :; }
        rpm_docker_env_hides_host_rpm_tools
        verify_rpm "$rpm"
    ) >"$log" 2>&1; then
        fail "verify_rpm accepted an RPM whose receipt said 'dnf' instead of 'rpm' (payload SHA matched)"
    fi
    assert_contains "$log" "RPM package-manager receipt missing or does not read exactly 'rpm'"
}

# ---------------------------------------------------------------------------
# Ubuntu 24.04 / nFPM 2.47 absolute-member regression coverage.
#
# /usr/bin/rpm2cpio on Ubuntu 24.04 emits RPM payload members with absolute
# names (/usr/bin/<bin>, ...). `cpio -idmv` without --no-absolute-filenames
# then either fails outright (unwritable /usr) or writes toward the host's
# real /usr instead of the private extraction directory -- and the old
# pipeline discarded stderr, so CI was opaque either way. verify_rpm's host
# branch must pass `--no-absolute-filenames`, keep the extraction private,
# and surface the rpm2cpio/cpio diagnostics when extraction fails. These
# tests drive the REAL host cpio through verify_rpm with function stubs for
# rpm2cpio (emitting a real cpio archive whose member names are absolute)
# and rpm (canned metadata answers), so removing the safe option or the
# failure diagnostics fails the suite.
# ---------------------------------------------------------------------------

# Append one newc (SVR4 ASCII) cpio entry to stdout. $1 = member name
# (stored verbatim; absolute here, mirroring Ubuntu 24.04 rpm2cpio output
# for nFPM 2.47 RPMs), $2 = "dir"|"file", $3 = ino, $4 = payload file for
# "file" entries.
newc_entry() {
    local name="$1" kind="$2" ino="$3" payload_file="${4:-}"
    local mode nlink=1 size=0 pad
    if [[ "$kind" == "dir" ]]; then
        mode=$((8#40755))
        nlink=2
    else
        mode=$((8#100755))
        size="$(stat -c %s "$payload_file")"
    fi
    printf '070701'
    printf '%08x' "$ino" "$mode" 0 0 "$nlink" 1700000000 "$size" 0 0 0 0 "$((${#name} + 1))" 0
    printf '%s\0' "$name"
    pad=$(( (4 - (110 + ${#name} + 1) % 4) % 4 ))
    if [[ "$pad" -gt 0 ]]; then head -c "$pad" /dev/zero; fi
    if [[ "$kind" == "file" ]]; then
        cat "$payload_file"
        pad=$(( (4 - size % 4) % 4 ))
        if [[ "$pad" -gt 0 ]]; then head -c "$pad" /dev/zero; fi
    fi
}

# Build a real newc cpio archive at $1 whose member names are absolute
# (/usr/...), exactly what /usr/bin/rpm2cpio on Ubuntu 24.04 hands cpio for
# an nFPM 2.47 RPM. $2 = payload file for /usr/bin/terraphim-agent, $3 =
# receipt file for /usr/share/terraphim/package-manager.d/terraphim-agent.
make_absolute_member_cpio_archive() {
    local archive="$1" payload_file="$2" receipt_file="$3"
    command -v cpio >/dev/null 2>&1 || fail "cpio is required for the absolute-member fixture"
    {
        newc_entry /usr dir 1
        newc_entry /usr/bin dir 2
        newc_entry /usr/bin/terraphim-agent file 3 "$payload_file"
        newc_entry /usr/share dir 4
        newc_entry /usr/share/terraphim dir 5
        newc_entry /usr/share/terraphim/package-manager.d dir 6
        newc_entry /usr/share/terraphim/package-manager.d/terraphim-agent file 7 "$receipt_file"
        # TRAILER!!! entry: all-zero metadata, namesize 11.
        printf '070701'
        printf '%08x' 0 0 0 0 1 0 0 0 0 0 0 11 0
        printf 'TRAILER!!!\0'
        head -c $(( (4 - (110 + 11) % 4) % 4 )) /dev/zero
    } > "$archive"
    [[ -s "$archive" ]] || fail "absolute-member cpio fixture archive is empty"
    # The fixture is only meaningful if the payload member name is absolute.
    cpio -it --quiet < "$archive" 2>/dev/null | grep -qx '/usr/bin/terraphim-agent' ||
        fail "absolute-member cpio fixture archive lacks /usr/bin/terraphim-agent"
}

# Canned `rpm` metadata answers for the host branch of verify_rpm: the arch
# query, the requires query, and the file-digest-algorithm query.
stub_rpm_metadata_x86_64() {
    rpm() {
        case " $* " in
            *' %{FILEDIGESTALGO} '*) printf '8' ;;
            *' %{ARCH} '*) printf 'x86_64' ;;
            *' -qpR '*) : ;;
            *) return 64 ;;
        esac
    }
}

test_verify_rpm_host_branch_strips_absolute_member_names() {
    command -v cpio >/dev/null 2>&1 || { require_tool_or_skip "cpio not installed"; return 0; }
    local payload=$'absolute-member RPM payload bytes\n'
    local payload_file="$TMP/rpm-abs-member-payload"
    local receipt_file="$TMP/rpm-abs-member-receipt"
    printf '%s' "$payload" > "$payload_file"
    printf 'rpm\n' > "$receipt_file"
    local archive="$TMP/absolute-members.cpio"
    make_absolute_member_cpio_archive "$archive" "$payload_file" "$receipt_file"
    local expected_sha
    expected_sha="$(printf '%s' "$payload" | sha256sum | awk '{print $1}')"
    printf 'fake rpm container\n' > "$TMP/fake-absolute-member.rpm"

    local work="$TMP/rpm-abs-member-work"
    local log="$TMP/rpm-abs-member.log"
    if ! (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$work"
        BIN_NAME=terraphim-agent
        RPM_ARCH=x86_64
        EXPECTED_SHA="$expected_sha"
        mkdir -p "$WORK_DIR"
        rpm2cpio() { cat "$archive"; }
        stub_rpm_metadata_x86_64
        lint_rpm() { :; }
        verify_rpm "$TMP/fake-absolute-member.rpm"
    ) >"$log" 2>&1; then
        fail "verify_rpm host branch rejected an RPM payload with absolute member names: $(cat "$log")"
    fi

    # Extraction must have stayed private to WORK_DIR; the archive member
    # names were absolute, so any path-unsafe extraction would have written
    # back over the fixture tree (or toward /usr) and left WORK_DIR empty.
    [[ "$(sha256sum "$work/rpm-extract-terraphim-agent/usr/bin/terraphim-agent" | awk '{print $1}')" == "$expected_sha" ]] ||
        fail "verify_rpm host branch did not extract the absolute-member payload privately: $(cat "$log")"
}

# rpm 4.17's rpm2cpio exits 1 on valid nFPM 2.47 RPMs while writing a
# complete payload and printing nothing to stderr (`rpm -K` on the same
# package reports "digests OK"). Hosted CI run 36153935369 failed the arch
# suite on exactly this: all five members plus "26 blocks" in the
# extraction log, and the pipeline status alone rejected the package.
# verify_rpm must judge extraction by its output, not by that status.
test_verify_rpm_accepts_complete_payload_despite_rpm2cpio_exit_status() {
    command -v cpio >/dev/null 2>&1 || { require_tool_or_skip "cpio not installed"; return 0; }
    local payload=$'complete payload despite nonzero rpm2cpio\n'
    local payload_file="$TMP/rpm-nonzero-payload"
    local receipt_file="$TMP/rpm-nonzero-receipt"
    printf '%s' "$payload" > "$payload_file"
    printf 'rpm\n' > "$receipt_file"
    local archive="$TMP/nonzero-status.cpio"
    make_absolute_member_cpio_archive "$archive" "$payload_file" "$receipt_file"
    local expected_sha
    expected_sha="$(printf '%s' "$payload" | sha256sum | awk '{print $1}')"
    printf 'fake rpm container\n' > "$TMP/fake-nonzero.rpm"

    local work="$TMP/rpm-nonzero-work"
    local log="$TMP/rpm-nonzero.log"
    if ! (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$work"
        BIN_NAME=terraphim-agent
        RPM_ARCH=x86_64
        EXPECTED_SHA="$expected_sha"
        mkdir -p "$WORK_DIR"
        # Complete archive on stdout, but the tool itself exits nonzero --
        # the exact rpm 4.17 behaviour observed on the hosted runner.
        rpm2cpio() { cat "$archive"; return 1; }
        stub_rpm_metadata_x86_64
        lint_rpm() { :; }
        verify_rpm "$TMP/fake-nonzero.rpm"
    ) >"$log" 2>&1; then
        fail "verify_rpm rejected a complete payload because rpm2cpio exited nonzero: $(cat "$log")"
    fi
    [[ "$(sha256sum "$work/rpm-extract-terraphim-agent/usr/bin/terraphim-agent" | awk '{print $1}')" == "$expected_sha" ]] ||
        fail "verify_rpm did not bind the extracted payload SHA: $(cat "$log")"
}

test_verify_rpm_host_branch_surfaces_extraction_diagnostics() {
    command -v cpio >/dev/null 2>&1 || { require_tool_or_skip "cpio not installed"; return 0; }
    local log="$TMP/rpm-extract-failure.log"
    printf 'fake rpm container\n' > "$TMP/fake-corrupt.rpm"
    if (
        TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 source "$BUILD"
        WORK_DIR="$TMP/rpm-extract-failure-work"
        BIN_NAME=terraphim-agent
        RPM_ARCH=x86_64
        EXPECTED_SHA="$(sha256sum /dev/null | awk '{print $1}')"
        mkdir -p "$WORK_DIR"
        # rpm2cpio emits a corrupt archive, so the extraction produces no
    # payload; the cpio diagnostic must reach the operator.
    rpm2cpio() { printf 'not a cpio archive\n'; }
        stub_rpm_metadata_x86_64
        lint_rpm() { :; }
        verify_rpm "$TMP/fake-corrupt.rpm"
    ) >"$log" 2>&1; then
        fail "verify_rpm host branch accepted a corrupt RPM payload archive"
    fi
    # The failure is reported by what extraction produced, not by the
    # pipeline status: rpm 4.17's rpm2cpio exits 1 even on valid nFPM
    # payloads, so the producer only fails closed when no binary appears.
    assert_contains "$log" "RPM payload extraction produced no terraphim-agent"
    # The underlying cpio diagnostic must be surfaced, not discarded.
    assert_contains "$log" "cpio"
}

test_rpm_extraction_uses_no_absolute_filenames_everywhere() {
    local gate="$ROOT/.github/scripts/nfpm/tests/test_client_nfpm_native.sh"
    local strip="$ROOT/.github/scripts/nfpm/tests/test_client_nfpm_strip.sh"
    # Every rpm2cpio | cpio extraction in the production script and the
    # native/strip test gates must pass --no-absolute-filenames (host and
    # Docker branches alike); a bare `cpio -i...` consumer of an RPM payload
    # is a path-safety regression.
    assert_contains "$BUILD" 'cpio --no-absolute-filenames -idmv'
    assert_contains "$gate" 'cpio --no-absolute-filenames -idmv'
    assert_not_contains "$BUILD" 'cpio -idmv'
    assert_not_contains "$gate" 'cpio -idmv'
    assert_not_contains "$BUILD" 'cpio -i --to-stdout'
    assert_not_contains "$gate" 'cpio -i --to-stdout'
    assert_not_contains "$strip" 'cpio -i --to-stdout'
}

test_build_reports_missing_nfpm() {
    local agent="$TMP/target/x86_64-unknown-linux-musl/release/terraphim-agent"
    local grep_bin="$TMP/target/x86_64-unknown-linux-musl/release/terraphim-grep"
    make_elf_header_fixture "$agent" x86_64
    make_elf_header_fixture "$grep_bin" x86_64

    if "$BUILD" --version 9.8.7 --target x86_64-unknown-linux-musl \
        --agent-binary "$agent" --grep-binary "$grep_bin" \
        --out-dir "$TMP/out" --nfpm "$TMP/missing-nfpm" 2>"$TMP/missing.err"; then
        fail "build succeeded without nFPM"
    fi
    assert_contains "$TMP/missing.err" "nFPM is required"
}

test_deb_payload_fixture_matches_input_binary() {
    command -v dpkg-deb >/dev/null 2>&1 || { require_tool_or_skip "dpkg-deb not installed"; return 0; }

    for bin_name in "${BIN_NAMES[@]}"; do
        local bin="$TMP/payload/$bin_name"
        local pkgroot="$TMP/deb-root-$bin_name"
        local deb="$TMP/${bin_name}_9.8.7_amd64.deb"
        local extract="$TMP/deb-extract-$bin_name"
        make_fixture_binary "$bin" "$bin_name"

        mkdir -p "$pkgroot/DEBIAN" "$pkgroot/usr/bin" "$pkgroot/usr/share/terraphim/package-manager.d"
        cp "$bin" "$pkgroot/usr/bin/$bin_name"
        chmod 0755 "$pkgroot/usr/bin/$bin_name"
        printf 'dpkg\n' > "$pkgroot/usr/share/terraphim/package-manager.d/$bin_name"
        cat > "$pkgroot/DEBIAN/control" <<EOF
Package: $bin_name
Version: 9.8.7
Section: utility
Priority: optional
Architecture: amd64
Maintainer: Terraphim Contributors <team@terraphim.ai>
Description: Terraphim client test package
EOF

        dpkg-deb --build --root-owner-group "$pkgroot" "$deb" >/dev/null
        dpkg-deb --extract "$deb" "$extract"

        local expected actual
        expected="$(sha256sum "$bin" | awk '{print $1}')"
        actual="$(sha256sum "$extract/usr/bin/$bin_name" | awk '{print $1}')"
        [[ "$actual" == "$expected" ]] || fail "DEB payload SHA mismatch for $bin_name"
        grep -qx 'dpkg' "$extract/usr/share/terraphim/package-manager.d/$bin_name"
    done
}

test_build_script_expects_nfpm_deb_filename() {
    assert_contains "$BUILD" 'deb_base="${bin_name}_${PKG_VERSION}-1_${DEB_ARCH}.deb"'
}

# Single explicit package/version contract (#326 P1-4): only a canonical
# stable MAJOR.MINOR.PATCH version is ever handed to nFPM, so the '-' -> '~'
# normalization nFPM applies to prerelease identifiers never triggers in
# production; PKG_VERSION is always exactly VERSION. Prerelease and
# build-metadata forms must be rejected before nFPM (or any binary
# validation) ever runs.
test_build_accepts_canonical_stable_version_verbatim() {
    assert_contains "$BUILD" 'local PKG_VERSION="$VERSION"'
}

test_build_rejects_unsupported_version_forms() {
    local agent="$TMP/badversion/terraphim-agent"
    local grep_bin="$TMP/badversion/terraphim-grep"
    make_elf_header_fixture "$agent" x86_64
    make_elf_header_fixture "$grep_bin" x86_64

    local version
    for version in \
        "1.2" "v1.2.3" "1.2.3." "1.2.3-" "1.2.3-rc..1" "1.2.3.4" \
        "01.2.3" "1.2.3-rc.1" "1.2.3-rc-1" "1.2.3+build.1" "1.2.3-rc.1+b.2" \
        "1.2.3+build.5"; do
        local log="$TMP/badversion-$RANDOM.log"
        if "$BUILD" --version "$version" --target x86_64-unknown-linux-musl \
            --agent-binary "$agent" --grep-binary "$grep_bin" \
            --out-dir "$TMP/badversion-out-$RANDOM" --nfpm /bin/true >"$log" 2>&1; then
            fail "build accepted unsupported version form: $version"
        fi
        assert_contains "$log" "unsupported client package version"
    done
}

test_build_accepts_canonical_stable_version_end_to_end() {
    command -v dpkg-deb >/dev/null 2>&1 || { require_tool_or_skip "dpkg-deb not installed"; return 0; }
    command -v "${NFPM_BIN:-nfpm}" >/dev/null 2>&1 || { require_tool_or_skip "nFPM not available (${NFPM_BIN:-nfpm})"; return 0; }

    local agent="$TMP/stable-version/terraphim-agent"
    local grep_bin="$TMP/stable-version/terraphim-grep"
    local out="$TMP/stable-version-out"
    if ! make_stripable_binary "$agent" terraphim-agent; then
        require_tool_or_skip "no C compiler available (CC/cc/gcc/clang) to build a strip-able fixture"
        return 0
    fi
    make_stripable_binary "$grep_bin" terraphim-grep ||
        fail "failed to build the terraphim-grep strip-able fixture"

    if ! "$BUILD" --version 1.2.3 --target x86_64-unknown-linux-musl \
        --agent-binary "$agent" --grep-binary "$grep_bin" --out-dir "$out" \
        --nfpm "${NFPM_BIN:-nfpm}" >"$TMP/stable-version.log" 2>&1; then
        # Real qualified-binary fixtures here are synthetic ELF stubs, not
        # genuine executables, so lintian/rpmlint may still reject them on
        # unrelated grounds (e.g. a missing PT_GNU_STACK section). The only
        # thing this test asserts is that the expected filename was found
        # (no "missing DEB/RPM output" failure) before any such lint verdict.
        assert_not_contains "$TMP/stable-version.log" "missing DEB output"
        assert_not_contains "$TMP/stable-version.log" "missing RPM output"
        assert_contains "$TMP/stable-version.log" "terraphim-agent_1.2.3-1_amd64.deb"
        assert_contains "$TMP/stable-version.log" "terraphim-agent-1.2.3-1.x86_64.rpm"
        return 0
    fi
    [[ -f "$out/terraphim-agent_1.2.3-1_amd64.deb" ]] ||
        fail "expected DEB filename missing: $out/terraphim-agent_1.2.3-1_amd64.deb"
    [[ -f "$out/terraphim-agent-1.2.3-1.x86_64.rpm" ]] ||
        fail "expected RPM filename missing: $out/terraphim-agent-1.2.3-1.x86_64.rpm"
    [[ -f "$out/terraphim-grep_1.2.3-1_amd64.deb" ]] ||
        fail "expected DEB filename missing: $out/terraphim-grep_1.2.3-1_amd64.deb"
    [[ -f "$out/terraphim-grep-1.2.3-1.x86_64.rpm" ]] ||
        fail "expected RPM filename missing: $out/terraphim-grep-1.2.3-1.x86_64.rpm"
}

test_build_script_fails_closed_without_source_date_epoch_fallback() {
    assert_contains "$BUILD" 'SOURCE_DATE_EPOCH is required outside a git worktree'
    assert_not_contains "$BUILD" 'SOURCE_DATE_EPOCH=0'
}

test_build_script_packages_a_validated_private_copy() {
    assert_contains "$BUILD" 'cp -P --reflink=never -- "$source_binary" "$validated_binary"'
    assert_contains "$BUILD" 'cmp -s -- "$source_binary" "$validated_binary"'
    assert_contains "$BUILD" 'EXPECTED_SHA="$(sha256sum "$BINARY"'
}

# Canonical release inputs are immutable: the producer must only verify that
# they are already stripped, never strip or otherwise mutate them itself.
# See test_client_nfpm_strip.sh for the full behavioral contract.
test_build_script_never_strips_or_mutates_qualified_inputs() {
    assert_not_contains "$BUILD" 'strip_qualified_binary'
    assert_not_contains "$BUILD" 'strip --strip-unneeded --'
    assert_contains "$BUILD" 'validate_stripped_binary "$source_binary"'
    assert_contains "$BUILD" 'validate_stripped_binary "$validated_binary"'
}

test_build_script_fails_closed_on_package_architecture() {
    assert_contains "$BUILD" 'dpkg-deb --field "$pkg" Architecture'
    assert_contains "$BUILD" "DEB arch mismatch expected=\$DEB_ARCH actual=\$pkg_arch"
    assert_contains "$BUILD" "sed -n 's/^arch=//p' \"\$metadata\""
    assert_contains "$BUILD" "rpm -qp --qf '%{ARCH}' \"\$pkg\""
    assert_contains "$BUILD" "RPM arch mismatch expected=\$RPM_ARCH actual=\$pkg_arch"
}

test_build_script_builds_both_binaries_and_all_formats() {
    assert_contains "$BUILD" 'build_one_binary terraphim-agent "$AGENT_BINARY"'
    assert_contains "$BUILD" 'build_one_binary terraphim-grep "$GREP_BINARY"'
    assert_contains "$BUILD" 'render_and_build deb'
    assert_contains "$BUILD" 'render_and_build rpm'
}

test_build_script_produces_a_deterministic_checksum_manifest() {
    assert_contains "$BUILD" 'sha256sum "${EXPECTED_BASENAMES[@]:0:4}" > "$sums_base"'
    assert_contains "$BUILD" 'terraphim-clients-${VERSION}-${TARGET}.package-sha256sums.txt'
}

test_build_script_has_docker_closed_fallbacks_and_lint() {
    assert_contains "$BUILD" "docker_rpm_tool"
    assert_contains "$BUILD" "require_docker_or_fail"
    assert_contains "$BUILD" "lintian --fail-on error"
    assert_not_contains "$BUILD" "--fail-on error,warning"
    assert_contains "$BUILD" "rpmlint"
    assert_contains "$BUILD" "RPM payload and metadata verification"
}

test_build_script_has_fail_closed_static_musl_lint_policy() {
    # The only allowlisted lint error is the exact justified static-MUSL
    # diagnostic for the current $BIN_NAME at usr/bin/$BIN_NAME; the
    # fail-closed parser is shared by the host and Docker lint paths.
    assert_contains "$BUILD" 'justified_literals=("E: ${BIN_NAME}: statically-linked-binary [usr/bin/${BIN_NAME}]")'
    assert_contains "$BUILD" 'justified_literals+=("E: terraphim-agent: embedded-library libyaml [usr/bin/terraphim-agent]")'
    assert_contains "$BUILD" 'justified_re="^${BIN_NAME}\\.${RPM_ARCH}: E: statically-linked-binary /usr/bin/${BIN_NAME}\$"'
    assert_contains "$BUILD" "enforce_lint_policy lintian"
    assert_contains "$BUILD" "enforce_lint_policy rpmlint"
    assert_contains "$BUILD" "unjustified error"
    assert_contains "$BUILD" "tool/install/transport failure"
    assert_contains "$BUILD" "no error line could be parsed"
    assert_contains "$BUILD" "contains error lines"
    assert_contains "$BUILD" "justified static-MUSL diagnostic more than once"
    assert_contains "$BUILD" "empty lint output"
    assert_contains "$BUILD" "--tag-display-limit 0"
    # Broad tag suppression is a forbidden policy escape hatch.
    assert_not_contains "$BUILD" "--suppress-tags"
    assert_not_contains "$BUILD" "--suppress-tags-from-file"
}

test_failed_validation_leaves_no_partial_outputs() {
    local agent="$TMP/partial/terraphim-agent"
    local grep_bin="$TMP/partial/terraphim-grep"
    local out="$TMP/partial-out"
    local fake_nfpm="$TMP/fake-nfpm"
    if ! make_stripable_binary "$agent" terraphim-agent; then
        require_tool_or_skip "no C compiler available (CC/cc/gcc/clang) to build a strip-able fixture"
        return 0
    fi
    make_stripable_binary "$grep_bin" terraphim-grep ||
        fail "failed to build the terraphim-grep strip-able fixture"

    cat > "$fake_nfpm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
format=""
target=""
config=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --packager)
            format="$2"
            shift 2
            ;;
        --target)
            target="$2"
            shift 2
            ;;
        --config)
            config="$2"
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done
name="$(sed -n 's/^name: //p' "$config" | head -n1)"
case "$format" in
    deb) printf 'invalid deb\n' > "$target/${name}_9.8.7-1_amd64.deb" ;;
    rpm) printf 'invalid rpm\n' > "$target/${name}-9.8.7-1.x86_64.rpm" ;;
    *) exit 2 ;;
esac
EOF
    chmod 0755 "$fake_nfpm"

    if "$BUILD" --version 9.8.7 --target x86_64-unknown-linux-musl \
        --agent-binary "$agent" --grep-binary "$grep_bin" --out-dir "$out" --nfpm "$fake_nfpm" \
        >"$TMP/partial.log" 2>&1; then
        fail "build accepted invalid package fixtures"
    fi
    assert_contains "$TMP/partial.log" "dpkg-deb"
    [[ ! -e "$out/terraphim-agent_9.8.7-1_amd64.deb" ]] ||
        fail "failed validation left a partial DEB in OUT_DIR"
    [[ ! -e "$out" ]] || {
        find "$out" -mindepth 1 -maxdepth 1 -print -quit | grep -q . &&
            fail "failed validation left partial outputs in OUT_DIR"
        true
    }
    if find "$TMP" -mindepth 1 -maxdepth 1 -name '.terraphim-client-nfpm.*' -print -quit | grep -q .; then
        fail "failed validation left its private staging directory behind"
    fi

    local occupied="$TMP/occupied-out"
    mkdir -p "$occupied"
    printf 'keep me\n' > "$occupied/unrelated-user-data"
    if "$BUILD" --version 9.8.7 --target x86_64-unknown-linux-musl \
        --agent-binary "$agent" --grep-binary "$grep_bin" --out-dir "$occupied" --nfpm "$fake_nfpm" \
        >"$TMP/occupied.log" 2>&1; then
        fail "build accepted a non-empty output directory"
    fi
    assert_contains "$TMP/occupied.log" "refusing to delete pre-existing data"
    grep -qx 'keep me' "$occupied/unrelated-user-data" ||
        fail "non-empty output rejection altered unrelated user data"

    local victim="$TMP/output-symlink-victim" unsafe="$TMP/unsafe-out"
    mkdir -p "$victim"
    printf 'keep me too\n' > "$victim/unrelated-user-data"
    ln -s "$victim" "$unsafe"
    if "$BUILD" --version 9.8.7 --target x86_64-unknown-linux-musl \
        --agent-binary "$agent" --grep-binary "$grep_bin" --out-dir "$unsafe" --nfpm "$fake_nfpm" \
        >"$TMP/unsafe.log" 2>&1; then
        fail "build accepted a symlink output directory"
    fi
    assert_contains "$TMP/unsafe.log" "unsafe output directory"
    grep -qx 'keep me too' "$victim/unrelated-user-data" ||
        fail "symlink output rejection altered its target"
}

test_render_deb_descriptor_for_each_binary
test_render_rpm_descriptor_for_each_binary
test_render_uses_distinct_licenses_per_binary
test_render_rejects_gnu_target
test_render_rejects_unsupported_binary_name
test_render_rejects_injected_newline_version
test_render_rejects_prerelease_and_build_metadata_versions
test_render_rejects_symlinked_binary_path
test_render_rejects_binary_path_with_embedded_newline
test_render_rejects_output_path_with_embedded_newline
test_render_quotes_version_and_binary_src_safely
test_build_rejects_unqualified_source_binaries
test_extracted_payload_type_and_size_are_rejected
test_verify_deb_rejects_payload_sha_mismatch
test_verify_deb_rejects_missing_receipt
test_verify_deb_rejects_wrong_receipt
test_verify_rpm_rejects_payload_sha_mismatch
test_verify_rpm_rejects_missing_receipt
test_verify_rpm_rejects_wrong_receipt
test_verify_rpm_host_branch_strips_absolute_member_names
test_verify_rpm_accepts_complete_payload_despite_rpm2cpio_exit_status
test_verify_rpm_host_branch_surfaces_extraction_diagnostics
test_rpm_extraction_uses_no_absolute_filenames_everywhere
test_build_reports_missing_nfpm
test_deb_payload_fixture_matches_input_binary
test_build_script_expects_nfpm_deb_filename
test_build_accepts_canonical_stable_version_verbatim
test_build_rejects_unsupported_version_forms
test_build_accepts_canonical_stable_version_end_to_end
test_build_script_fails_closed_without_source_date_epoch_fallback
test_build_script_packages_a_validated_private_copy
test_build_script_never_strips_or_mutates_qualified_inputs
test_build_script_fails_closed_on_package_architecture
test_build_script_builds_both_binaries_and_all_formats
test_build_script_produces_a_deterministic_checksum_manifest
test_build_script_has_docker_closed_fallbacks_and_lint
test_build_script_has_fail_closed_static_musl_lint_policy
test_failed_validation_leaves_no_partial_outputs

echo "client nFPM tests passed"
