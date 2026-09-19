#!/usr/bin/env bash
# Behavioral contract tests for assemble-client-release-inventory.sh.
#
# Adapted from the reviewed terraphim_server assemble-release-inventory
# contract tests, generalized to the two-binary (terraphim-agent,
# terraphim-grep) client package matrix and with the legacy cargo-deb merge
# stage dropped (clients has no legacy managed-package publication path).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
ASSEMBLE="$ROOT/.github/scripts/release/assemble-client-release-inventory.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/terraphim-client-assemble-inventory.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

expect_ok() {
    "$ASSEMBLE" "$@" >/dev/null || fail "expected success: $*"
}

expect_fail() {
    local message="$1"
    shift
    if "$ASSEMBLE" "$@" >"$TMP/stdout" 2>"$TMP/stderr"; then
        fail "expected failure ($message): $*"
    fi
    grep -Fq -- "$message" "$TMP/stderr" || fail "expected '$message' in stderr, got: $(cat "$TMP/stderr")"
}

make_stage() {
    local dir="$1"
    shift
    mkdir -p "$dir"
    local name
    for name in "$@"; do
        printf 'fixture\n' > "$dir/$name"
    done
}

# The full, correct per-target managed inventory: 2 DEBs + 2 RPMs (one pair
# per binary) + 1 checksum manifest whose hashes actually verify against the
# fixture bytes, mirroring the real producer's round-trip contract.
make_complete_target() {
    local staging="$1" target="$2" version="$3" deb_arch="$4" rpm_arch="$5"
    local dir="$staging/client-managed-packages-$target"
    local -a names=(
        "terraphim-agent_${version}-1_${deb_arch}.deb"
        "terraphim-agent-${version}-1.${rpm_arch}.rpm"
        "terraphim-grep_${version}-1_${deb_arch}.deb"
        "terraphim-grep-${version}-1.${rpm_arch}.rpm"
    )
    make_stage "$dir" "${names[@]}"
    ( cd "$dir" && sha256sum "${names[@]}" ) > "$dir/terraphim-clients-${version}-${target}.package-sha256sums.txt"
}

# A complete managed matrix (both binaries, both targets) merges all 10
# DEB/RPM/checksum outputs into the inventory.
test_complete_managed_matrix_is_merged() {
    local out="$TMP/out1" staging="$TMP/staging1"
    mkdir -p "$out"
    : > "$out/terraphim-agent-x86_64-apple-darwin.tar.gz"
    make_complete_target "$staging" x86_64-unknown-linux-musl 1.0.0 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    expect_ok --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    local merged
    for merged in \
        "terraphim-agent-x86_64-apple-darwin.tar.gz" \
        "terraphim-agent_1.0.0-1_amd64.deb" \
        "terraphim-agent_1.0.0-1_arm64.deb" \
        "terraphim-agent-1.0.0-1.x86_64.rpm" \
        "terraphim-agent-1.0.0-1.aarch64.rpm" \
        "terraphim-grep_1.0.0-1_amd64.deb" \
        "terraphim-grep_1.0.0-1_arm64.deb" \
        "terraphim-grep-1.0.0-1.x86_64.rpm" \
        "terraphim-grep-1.0.0-1.aarch64.rpm" \
        "terraphim-clients-1.0.0-x86_64-unknown-linux-musl.package-sha256sums.txt" \
        "terraphim-clients-1.0.0-aarch64-unknown-linux-musl.package-sha256sums.txt" \
        "DEB-RPM-PACKAGES-UNSIGNED-STAGE-ONLY.txt"; do
        [[ -f "$out/$merged" ]] || fail "expected merged asset $out/$merged"
    done
    [[ "$(find "$out" -maxdepth 1 -type f | wc -l)" -eq 12 ]] ||
        fail "expected exactly 12 files in the assembled inventory"
}

# A managed DEB colliding by basename with an already-merged binary asset
# must fail closed instead of silently clobbering.
test_managed_binary_basename_conflict_fails() {
    local out="$TMP/out2" staging="$TMP/staging2"
    mkdir -p "$out"
    : > "$out/terraphim-agent_1.0.0-1_amd64.deb"
    make_complete_target "$staging" x86_64-unknown-linux-musl 1.0.0 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    expect_fail "duplicate release asset basename: terraphim-agent_1.0.0-1_amd64.deb" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    local before after
    before="$(cat "$out/terraphim-agent_1.0.0-1_amd64.deb")"
    [[ -z "$before" ]] || fail "fixture setup invariant broken"
}

# A partial managed matrix (one target present, one missing) is all-or-nothing.
test_partial_managed_matrix_fails() {
    local out="$TMP/out3" staging="$TMP/staging3"
    mkdir -p "$out"
    make_complete_target "$staging" x86_64-unknown-linux-musl 1.0.0 amd64 x86_64

    expect_fail "managed package matrix incomplete; missing targets: aarch64-unknown-linux-musl (all-or-nothing)" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ ! -e "$out/terraphim-agent_1.0.0-1_amd64.deb" ]] ||
        fail "partial managed matrix must not be merged"
}

# An absent managed stage (job skipped) leaves the inventory untouched.
test_absent_managed_stage_is_tolerated() {
    local out="$TMP/out4"
    mkdir -p "$out"
    : > "$out/terraphim-agent-x86_64-apple-darwin.tar.gz"
    expect_ok --output "$out" --expected-version 1.0.0 --managed-staging "$TMP/staging-absent" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
    [[ "$(find "$out" -maxdepth 1 -type f | wc -l)" -eq 1 ]] ||
        fail "absent managed stage must not add inventory entries"
}

# A present managed artifact directory missing one binary's RPM is rejected.
test_incomplete_managed_target_dir_fails() {
    local out="$TMP/out5" staging="$TMP/staging5"
    mkdir -p "$out"
    make_stage "$staging/client-managed-packages-x86_64-unknown-linux-musl" \
        "terraphim-agent_1.0.0-1_amd64.deb" \
        "terraphim-agent-1.0.0-1.x86_64.rpm" \
        "terraphim-grep_1.0.0-1_amd64.deb" \
        "terraphim-clients-1.0.0-x86_64-unknown-linux-musl.package-sha256sums.txt"
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    expect_fail "managed artifact directory missing terraphim-grep-1.0.0-1.x86_64.rpm" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

# No legacy stage at all must assemble cleanly: the client workflow only ever
# calls this script in managed-only mode.
test_managed_only_inventory_succeeds() {
    local out="$TMP/out7" staging="$TMP/staging7"
    mkdir -p "$out"
    : > "$out/terraphim-agent-x86_64-apple-darwin.tar.gz"
    make_complete_target "$staging" x86_64-unknown-linux-musl 1.0.0 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    expect_ok --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

# A managed target directory is an exact producer/consumer boundary. An
# unrelated fifth+sixth file must reject the entire matrix before any asset
# moves.
test_unexpected_managed_inventory_fails_before_merge() {
    local out="$TMP/out8" staging="$TMP/staging8"
    mkdir -p "$out"
    printf 'binary\n' > "$out/terraphim-agent-x86_64-apple-darwin.tar.gz"
    make_stage "$staging/client-managed-packages-x86_64-unknown-linux-musl" \
        "terraphim-agent_1.0.0-1_amd64.deb" \
        "terraphim-agent-1.0.0-1.x86_64.rpm" \
        "terraphim-grep_1.0.0-1_amd64.deb" \
        "terraphim-grep-1.0.0-1.x86_64.rpm" \
        "terraphim-clients-1.0.0-x86_64-unknown-linux-musl.package-sha256sums.txt" \
        "unexpected.txt"
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    expect_fail "unexpected managed artifact" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ ! -e "$out/terraphim-agent_1.0.0-1_amd64.deb" ]] ||
        fail "unexpected managed inventory was partially merged"
}

# Every format in a target directory must describe the same version. A stale
# DEB beside current RPM/checksum outputs must fail before authoritative merge.
test_stale_version_managed_inventory_fails_before_merge() {
    local out="$TMP/out9" staging="$TMP/staging9"
    mkdir -p "$out"
    make_stage "$staging/client-managed-packages-x86_64-unknown-linux-musl" \
        "terraphim-agent_0.9.0-1_amd64.deb" \
        "terraphim-agent-1.0.0-1.x86_64.rpm" \
        "terraphim-grep_1.0.0-1_amd64.deb" \
        "terraphim-grep-1.0.0-1.x86_64.rpm" \
        "terraphim-clients-1.0.0-x86_64-unknown-linux-musl.package-sha256sums.txt"
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    expect_fail "unexpected managed artifact" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ ! -e "$out/terraphim-agent-1.0.0-1.x86_64.rpm" ]] ||
        fail "stale managed inventory was partially merged"
}

test_unsafe_managed_inputs_fail_before_merge() {
    local out="$TMP/out10" staging="$TMP/staging10" linked="$TMP/linked-rpm"
    mkdir -p "$out"
    printf 'linked package\n' > "$linked"
    make_stage "$staging/client-managed-packages-x86_64-unknown-linux-musl" \
        "terraphim-agent_1.0.0-1_amd64.deb" \
        "terraphim-grep_1.0.0-1_amd64.deb" \
        "terraphim-grep-1.0.0-1.x86_64.rpm" \
        "terraphim-clients-1.0.0-x86_64-unknown-linux-musl.package-sha256sums.txt"
    ln -s "$linked" "$staging/client-managed-packages-x86_64-unknown-linux-musl/terraphim-agent-1.0.0-1.x86_64.rpm"
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    expect_fail "managed artifact must be a regular non-symlink file" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ "$(find "$out" -mindepth 1 -maxdepth 1 -type f | wc -l)" -eq 0 ]] ||
        fail "unsafe managed inventory was partially merged"
}

test_zero_length_managed_input_fails_before_merge() {
    local out="$TMP/out11" staging="$TMP/staging11"
    mkdir -p "$out"
    make_complete_target "$staging" x86_64-unknown-linux-musl 1.0.0 amd64 x86_64
    : > "$staging/client-managed-packages-x86_64-unknown-linux-musl/terraphim-agent_1.0.0-1_amd64.deb"
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    expect_fail "managed artifact must not be zero-length" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

# The staging root is itself an exact producer/consumer boundary. A directory
# for any target outside the release matrix must not be mistaken for an absent
# managed stage.
test_wrong_target_managed_staging_dir_fails() {
    local out="$TMP/out12" staging="$TMP/staging12"
    mkdir -p "$out"
    make_stage "$staging/client-managed-packages-riscv64gc-unknown-linux-gnu" \
        "terraphim-agent_1.0.0-1_riscv64.deb"

    expect_fail "unexpected managed staging entry" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

# Unexpected root entries, including hidden files, reject an otherwise
# complete matrix before any managed package is merged.
test_unexpected_managed_staging_entry_fails_before_merge() {
    local out="$TMP/out13" staging="$TMP/staging13"
    mkdir -p "$out"
    make_complete_target "$staging" x86_64-unknown-linux-musl 1.0.0 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64
    printf 'unexpected\n' > "$staging/.unexpected-root-entry"

    expect_fail "unexpected managed staging entry" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ ! -e "$out/terraphim-agent_1.0.0-1_amd64.deb" ]] ||
        fail "managed matrix with an unexpected root entry was partially merged"
}

# A symlink at the staging root must not be followed or ignored, regardless of
# whether it resolves to a directory.
test_managed_staging_root_symlink_entry_fails() {
    local out="$TMP/out14" staging="$TMP/staging14" linked="$TMP/linked-root-dir"
    mkdir -p "$out" "$staging" "$linked"
    ln -s "$linked" "$staging/client-managed-packages-extra"

    expect_fail "unexpected managed staging entry" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

# Once the managed staging root exists it is authoritative. An empty root is
# an incomplete producer result, not an absent/skipped managed-package stage.
test_present_empty_managed_staging_root_fails() {
    local out="$TMP/out15" staging="$TMP/staging15"
    mkdir -p "$out" "$staging"

    expect_fail "managed package matrix incomplete" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

# Both target directories must contain the exact non-empty five-file set.
# Merely creating the expected directory names is not a complete stage.
test_both_managed_target_dirs_empty_fail() {
    local out="$TMP/out16" staging="$TMP/staging16"
    mkdir -p "$out" \
        "$staging/client-managed-packages-x86_64-unknown-linux-musl" \
        "$staging/client-managed-packages-aarch64-unknown-linux-musl"

    expect_fail "managed artifact directory missing package checksum manifest" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

test_present_managed_staging_root_must_be_real_directory() {
    local out="$TMP/out17" file_root="$TMP/staging17-file"
    local link_root="$TMP/staging17-link" link_target="$TMP/staging17-target"
    mkdir -p "$out" "$link_target"
    printf 'not a directory\n' > "$file_root"
    ln -s "$link_target" "$link_root"

    expect_fail "managed staging root must be a regular non-symlink directory" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$file_root" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
    expect_fail "managed staging root must be a regular non-symlink directory" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$link_root" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

test_expected_managed_target_symlink_fails() {
    local out="$TMP/out18" staging="$TMP/staging18" linked="$TMP/staging18-linked"
    mkdir -p "$out" "$staging" "$linked" \
        "$staging/client-managed-packages-aarch64-unknown-linux-musl"
    ln -s "$linked" "$staging/client-managed-packages-x86_64-unknown-linux-musl"

    expect_fail "unexpected managed staging entry (expected a regular target directory)" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
}

# --managed-target requires --managed-staging.
test_managed_target_without_staging_rejected() {
    local out="$TMP/out19"
    mkdir -p "$out"
    expect_fail "--managed-target requires --managed-staging" \
        --output "$out" --expected-version 1.0.0 --managed-target x86_64-unknown-linux-musl
}

# --output must already exist.
test_missing_output_dir_rejected() {
    expect_fail "--output DIR must exist" --output "$TMP/does-not-exist-out"
}

# A package that was truncated/corrupted during the artifact upload/download
# round-trip but kept its expected non-empty filename must fail the checksum
# manifest verification, not slip through on name/size checks alone.
test_tampered_managed_package_fails_checksum_verification() {
    local out="$TMP/out20" staging="$TMP/staging20"
    mkdir -p "$out"
    make_complete_target "$staging" x86_64-unknown-linux-musl 1.0.0 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64

    printf 'corrupted-during-round-trip\n' > \
        "$staging/client-managed-packages-x86_64-unknown-linux-musl/terraphim-agent_1.0.0-1_amd64.deb"

    expect_fail "managed package checksums do not verify after artifact round-trip" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ ! -e "$out/terraphim-agent_1.0.0-1_amd64.deb" ]] ||
        fail "tampered managed package was partially merged despite a checksum failure"
}

# A manifest edited to claim a hash that does not match its own listed file
# (the mirror image of the scenario above: the bytes are untouched but the
# manifest itself was tampered) must fail closed identically.
test_manifest_with_wrong_hash_fails_checksum_verification() {
    local out="$TMP/out21" staging="$TMP/staging21" zeros
    mkdir -p "$out"
    make_complete_target "$staging" x86_64-unknown-linux-musl 1.0.0 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 1.0.0 arm64 aarch64
    printf -v zeros '%064d' 0

    local manifest="$staging/client-managed-packages-x86_64-unknown-linux-musl/terraphim-clients-1.0.0-x86_64-unknown-linux-musl.package-sha256sums.txt"
    sed -i "1s/^[0-9a-f]\{64\}/${zeros}/" "$manifest"

    expect_fail "managed package checksums do not verify after artifact round-trip" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ ! -e "$out/terraphim-agent_1.0.0-1_amd64.deb" ]] ||
        fail "manifest-tampered managed matrix was partially merged"
}

# The managed DEB/RPM matrix ships no package-manager signature and no
# centralized provenance attestation; the assembler must machine-encode that
# boundary alongside the packages so no downstream consumer mistakes them
# for a signed, centrally-attested release channel (#3336).
test_managed_matrix_ships_unsigned_stage_only_marker() {
    local out="$TMP/out22" staging="$TMP/staging22" marker
    mkdir -p "$out"
    make_complete_target "$staging" x86_64-unknown-linux-musl 2.3.4 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 2.3.4 arm64 aarch64

    expect_ok --output "$out" --expected-version 2.3.4 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    marker="$out/DEB-RPM-PACKAGES-UNSIGNED-STAGE-ONLY.txt"
    [[ -f "$marker" ]] || fail "expected unsigned-stage-only marker: $marker"
    grep -Fq "version: 2.3.4" "$marker" || fail "marker missing resolved managed version"
    grep -Fq "status: unsigned" "$marker" || fail "marker missing machine-readable unsigned status"
    grep -Fq "promotion: blocked" "$marker" || fail "marker missing machine-readable promotion-blocked status"
    grep -Fq "https://github.com/terraphim/terraphim-ai/issues/3336" "$marker" ||
        fail "marker missing #3336 tracking issue reference"
}

# No managed DEB/RPM matrix, nothing to warn about: the marker must not be
# fabricated onto a binary-only (tar.gz) release inventory.
test_no_unsigned_marker_without_managed_staging() {
    local out="$TMP/out23"
    mkdir -p "$out"
    : > "$out/terraphim-agent-x86_64-apple-darwin.tar.gz"
    expect_ok --output "$out" --expected-version 1.0.0 --managed-staging "$TMP/staging23-absent" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl
    [[ ! -e "$out/DEB-RPM-PACKAGES-UNSIGNED-STAGE-ONLY.txt" ]] ||
        fail "unsigned-stage-only marker must not appear without a managed package matrix"
}

# #326 P2-4: --expected-version is required.
test_missing_expected_version_rejected() {
    local out="$TMP/out24"
    mkdir -p "$out"
    expect_fail "--expected-version VERSION is required" --output "$out"
}

# #326 P2-4: --expected-version must itself be well-formed (rejects
# injection/malformed input, e.g. shell metacharacters or an empty
# component), independent of whether any managed staging is present.
test_malicious_expected_version_rejected() {
    local out="$TMP/out25"
    mkdir -p "$out"
    local version
    for version in '1.0.0; rm -rf /' '$(id)' '1.0.0/../../etc' '1.0.0 ' ''; do
        [[ -n "$version" ]] || continue
        expect_fail "--expected-version is not a well-formed semver value" \
            --output "$out" --expected-version "$version"
    done
}

# #326 P2-4: a fully self-consistent managed matrix (both targets agree with
# each other AND with the checksum manifests) must still be rejected if it
# was built for a version other than the release's expected current version
# -- this is the exact "stale but internally consistent" release-safety gap
# the version-derived expected_names check alone cannot catch, since two
# stale-but-matching targets never disagree with each other.
test_self_consistent_stale_version_rejected_against_expected_version() {
    local out="$TMP/out26" staging="$TMP/staging26"
    mkdir -p "$out"
    make_complete_target "$staging" x86_64-unknown-linux-musl 0.9.0 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 0.9.0 arm64 aarch64

    expect_fail "managed artifact directory version does not match expected release version: expected=1.0.0 actual=0.9.0" \
        --output "$out" --expected-version 1.0.0 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ ! -e "$out/terraphim-agent_0.9.0-1_amd64.deb" ]] ||
        fail "self-consistent stale-version managed matrix was partially merged"
}

# #326 P2-4: the exact current version, matching --expected-version, is
# accepted end-to-end (positive control for the two tests above).
test_valid_exact_expected_version_accepted() {
    local out="$TMP/out27" staging="$TMP/staging27"
    mkdir -p "$out"
    make_complete_target "$staging" x86_64-unknown-linux-musl 3.1.4 amd64 x86_64
    make_complete_target "$staging" aarch64-unknown-linux-musl 3.1.4 arm64 aarch64

    expect_ok --output "$out" --expected-version 3.1.4 --managed-staging "$staging" \
        --managed-target x86_64-unknown-linux-musl \
        --managed-target aarch64-unknown-linux-musl

    [[ -f "$out/terraphim-agent_3.1.4-1_amd64.deb" ]] ||
        fail "expected merged asset missing for the exact expected version"
}

test_complete_managed_matrix_is_merged
test_managed_binary_basename_conflict_fails
test_partial_managed_matrix_fails
test_absent_managed_stage_is_tolerated
test_incomplete_managed_target_dir_fails
test_managed_only_inventory_succeeds
test_unexpected_managed_inventory_fails_before_merge
test_stale_version_managed_inventory_fails_before_merge
test_unsafe_managed_inputs_fail_before_merge
test_zero_length_managed_input_fails_before_merge
test_wrong_target_managed_staging_dir_fails
test_unexpected_managed_staging_entry_fails_before_merge
test_managed_staging_root_symlink_entry_fails
test_present_empty_managed_staging_root_fails
test_both_managed_target_dirs_empty_fail
test_present_managed_staging_root_must_be_real_directory
test_expected_managed_target_symlink_fails
test_managed_target_without_staging_rejected
test_missing_output_dir_rejected
test_tampered_managed_package_fails_checksum_verification
test_manifest_with_wrong_hash_fails_checksum_verification
test_managed_matrix_ships_unsigned_stage_only_marker
test_no_unsigned_marker_without_managed_staging
test_missing_expected_version_rejected
test_malicious_expected_version_rejected
test_self_consistent_stale_version_rejected_against_expected_version
test_valid_exact_expected_version_accepted

echo "assemble-client-release-inventory tests passed"
