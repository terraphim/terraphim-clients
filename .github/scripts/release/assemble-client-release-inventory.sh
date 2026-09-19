#!/usr/bin/env bash
# Assemble the GitHub release asset inventory from staged client artifact
# downloads.
#
# Enforces the managed-package release contract:
#   * --output is the dedicated, initially-empty managed-package destination
#     in the release workflow, separate from the 20 signed archive assets.
#   * all-or-nothing: client-managed-packages-<target> artifacts must be
#     present for every matrix target, or none are added to the inventory.
#   * duplicate basenames among pre-existing destination entries (normally
#     none in the release workflow), entries across both managed targets, and
#     the generated stage-only marker fail closed instead of clobbering bytes.
#
# Adapted from the reviewed terraphim_server release-inventory assembler,
# generalized to the two-binary (terraphim-agent, terraphim-grep) client
# package matrix and with the legacy cargo-deb merge stage dropped (clients
# has no legacy managed-package publication path to reconcile).
#
# Requires bash 4+ (associative arrays).

set -euo pipefail

usage() {
    cat >&2 <<'EOF'
Usage: assemble-client-release-inventory.sh --output DIR --expected-version VERSION \
    [--managed-staging DIR] [--managed-target TRIPLE ...]

Merges staged managed-package artifacts into DIR. DIR must already exist; the
release workflow supplies a dedicated, initially-empty managed-package
destination that is separate from its 20 signed archive assets. Any
pre-existing destination entries are registered for duplicate protection.
The managed package stage is only merged when the all-or-nothing gate below
passes, and every managed target's derived version (and the assembled
marker's version) must equal --expected-version exactly.
EOF
}

# Single explicit package/version contract (#326 P1-4/P2-4): identical to
# build-client-packages.sh's CLIENT_PACKAGE_VERSION_RE and
# render-client-nfpm.sh's copy -- the managed DEB/RPM channel is scoped to
# canonical stable releases only, checked with the same predicate at every
# stage. Every managed target directory's own version must match this.
CLIENT_PACKAGE_VERSION_RE='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'

# --expected-version is deliberately broader than CLIENT_PACKAGE_VERSION_RE:
# this script also assembles the binary-only (tar.gz) inventory for releases
# whose version the top-level preflight accepts but the managed-package
# channel does not (prerelease/build-metadata versions, #326 P1-4), so its
# own format check only guards against malformed/injected input, not against
# every version this script is legitimately invoked for.
EXPECTED_VERSION_RE='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'

OUTPUT=""
EXPECTED_VERSION=""
MANAGED_STAGING=""
MANAGED_TARGETS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --output)
            OUTPUT="${2:-}"
            shift 2
            ;;
        --expected-version)
            EXPECTED_VERSION="${2:-}"
            shift 2
            ;;
        --managed-staging)
            MANAGED_STAGING="${2:-}"
            shift 2
            ;;
        --managed-target)
            MANAGED_TARGETS+=("${2:-}")
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage
            exit 2
            ;;
    esac
done

[[ -n "$OUTPUT" && -d "$OUTPUT" ]] || {
    echo "assemble-client-release-inventory: --output DIR must exist" >&2
    exit 2
}
[[ -n "$EXPECTED_VERSION" ]] || {
    echo "assemble-client-release-inventory: --expected-version VERSION is required" >&2
    exit 2
}
if [[ ! "$EXPECTED_VERSION" =~ $EXPECTED_VERSION_RE ]]; then
    echo "assemble-client-release-inventory: --expected-version is not a well-formed semver value: $EXPECTED_VERSION" >&2
    exit 2
fi
if [[ ${#MANAGED_TARGETS[@]} -gt 0 && -z "$MANAGED_STAGING" ]]; then
    echo "assemble-client-release-inventory: --managed-target requires --managed-staging" >&2
    exit 2
fi

declare -A SEEN=()

register_file() {
    local path="$1"
    local base
    base="$(basename "$path")"
    if [[ -n "${SEEN[$base]:-}" ]]; then
        echo "::error::duplicate release asset basename: $base (${SEEN[$base]} and $path)" >&2
        exit 1
    fi
    SEEN["$base"]="$path"
}

register_dir() {
    local dir="$1"
    local f
    while IFS= read -r -d '' f; do
        register_file "$f"
    done < <(find "$dir" -maxdepth 1 -type f -print0)
}

MANAGED_VERSION=""
MANAGED_FILES=()

BIN_NAMES=(terraphim-agent terraphim-grep)

validate_managed_dir() {
    local dir="$1"
    local target="$2"
    local deb_arch rpm_arch
    case "$target" in
        x86_64-unknown-linux-musl)
            deb_arch="amd64"
            rpm_arch="x86_64"
            ;;
        aarch64-unknown-linux-musl)
            deb_arch="arm64"
            rpm_arch="aarch64"
            ;;
        *)
            echo "::error::unsupported managed package target: $target" >&2
            exit 1
            ;;
    esac

    local -a entries=()
    local path base version=""
    while IFS= read -r -d '' path; do
        if [[ -L "$path" || ! -f "$path" ]]; then
            echo "::error::managed artifact must be a regular non-symlink file: $path" >&2
            exit 1
        fi
        if [[ ! -s "$path" ]]; then
            echo "::error::managed artifact must not be zero-length: $path" >&2
            exit 1
        fi
        entries+=("$path")
        base="$(basename "$path")"
        if [[ "$base" == terraphim-clients-*"-$target.package-sha256sums.txt" ]]; then
            if [[ -n "$version" ]]; then
                echo "::error::unexpected managed artifact (duplicate checksum manifest): $path" >&2
                exit 1
            fi
            version="${base#terraphim-clients-}"
            version="${version%-"$target".package-sha256sums.txt}"
        fi
    done < <(find "$dir" -mindepth 1 -maxdepth 1 -print0)

    if [[ -z "$version" ]]; then
        echo "::error::managed artifact directory missing package checksum manifest: $dir" >&2
        exit 1
    fi
    if [[ ! "$version" =~ $CLIENT_PACKAGE_VERSION_RE ]]; then
        echo "::error::managed artifact directory has an unsupported version form: $version" >&2
        exit 1
    fi
    if [[ "$version" != "$EXPECTED_VERSION" ]]; then
        echo "::error::managed artifact directory version does not match expected release version: expected=$EXPECTED_VERSION actual=$version target=$target dir=$dir" >&2
        exit 1
    fi

    # CLIENT_PACKAGE_VERSION_RE guarantees no '-' can appear, so the DEB/RPM
    # package version (see build-client-packages.sh's PKG_VERSION) is always
    # the checksum manifest's version verbatim.
    local pkg_version="$version"

    local -a expected_names=()
    local bin_name
    for bin_name in "${BIN_NAMES[@]}"; do
        expected_names+=(
            "${bin_name}_${pkg_version}-1_${deb_arch}.deb"
            "${bin_name}-${pkg_version}-1.${rpm_arch}.rpm"
        )
    done
    expected_names+=("terraphim-clients-${version}-${target}.package-sha256sums.txt")

    declare -A expected=()
    local name
    for name in "${expected_names[@]}"; do
        expected["$name"]=1
    done

    declare -A actual=()
    for path in "${entries[@]}"; do
        base="$(basename "$path")"
        actual["$base"]="$path"
        if [[ -z "${expected[$base]:-}" ]]; then
            echo "::error::unexpected managed artifact (stale-version, wrong-target, or extra): $path" >&2
            exit 1
        fi
    done

    for name in "${expected_names[@]}"; do
        [[ -n "${actual[$name]:-}" ]] || {
            echo "::error::managed artifact directory missing $name: $dir" >&2
            exit 1
        }
    done
    [[ "${#entries[@]}" -eq "${#expected_names[@]}" ]] || {
        echo "::error::managed artifact directory must contain exactly ${#expected_names[@]} files: $dir" >&2
        exit 1
    }

    if [[ -n "$MANAGED_VERSION" && "$MANAGED_VERSION" != "$version" ]]; then
        echo "::error::managed package matrix contains stale-version mismatch: expected=$MANAGED_VERSION actual=$version target=$target" >&2
        exit 1
    fi
    MANAGED_VERSION="$version"

    # Verify the shipped checksum manifest against the actual staged bytes
    # after the artifact upload/download round-trip, not just at producer
    # build time: a truncated or corrupted package that keeps its expected
    # filename and non-zero length would otherwise pass every check above.
    local manifest_name="terraphim-clients-${version}-${target}.package-sha256sums.txt"
    if ! ( cd "$dir" && sha256sum --strict -c "$manifest_name" >/dev/null 2>&1 ); then
        echo "::error::managed package checksums do not verify after artifact round-trip: $dir" >&2
        exit 1
    fi

    for name in "${expected_names[@]}"; do
        register_file "${actual[$name]}"
        MANAGED_FILES+=("${actual[$name]}")
    done
}

# 1. Register any pre-existing destination entries. The release workflow's
# dedicated managed-package destination is initially empty, but registration
# keeps the script fail-closed for other callers that pre-populate it.
register_dir "$OUTPUT"

# 2. Managed DEB/RPM matrix artifacts: all-or-nothing gate.
if [[ -n "$MANAGED_STAGING" && ! -e "$MANAGED_STAGING" && ! -L "$MANAGED_STAGING" ]]; then
    # The workflow only downloads managed artifacts when the managed package
    # job succeeded; a missing staging directory means the stage is absent.
    echo "NOTE: managed staging directory absent (job skipped): $MANAGED_STAGING" >&2
    MANAGED_STAGING=""
fi
if [[ -n "$MANAGED_STAGING" ]]; then
    if [[ -L "$MANAGED_STAGING" || ! -d "$MANAGED_STAGING" ]]; then
        echo "::error::managed staging root must be a regular non-symlink directory: $MANAGED_STAGING" >&2
        exit 1
    fi

    declare -A REQUESTED_MANAGED_TARGETS=()
    for target in "${MANAGED_TARGETS[@]}"; do
        case "$target" in
            x86_64-unknown-linux-musl|aarch64-unknown-linux-musl)
                if [[ -n "${REQUESTED_MANAGED_TARGETS[$target]:-}" ]]; then
                    echo "::error::duplicate managed package target: $target" >&2
                    exit 1
                fi
                REQUESTED_MANAGED_TARGETS["$target"]=1
                ;;
            *)
                echo "::error::unsupported managed package target: $target" >&2
                exit 1
                ;;
        esac
    done
    if [[ ${#REQUESTED_MANAGED_TARGETS[@]} -ne 2 ||
        -z "${REQUESTED_MANAGED_TARGETS[x86_64-unknown-linux-musl]:-}" ||
        -z "${REQUESTED_MANAGED_TARGETS[aarch64-unknown-linux-musl]:-}" ]]; then
        echo "::error::managed staging root requires exactly the x86_64 and aarch64 MUSL targets" >&2
        exit 1
    fi

    managed_root_entries=0
    while IFS= read -r -d '' path; do
        managed_root_entries=$((managed_root_entries + 1))
        base="$(basename "$path")"
        case "$base" in
            client-managed-packages-x86_64-unknown-linux-musl|client-managed-packages-aarch64-unknown-linux-musl)
                if [[ -L "$path" || ! -d "$path" ]]; then
                    echo "::error::unexpected managed staging entry (expected a regular target directory): $path" >&2
                    exit 1
                fi
                ;;
            *)
                echo "::error::unexpected managed staging entry: $path" >&2
                exit 1
                ;;
        esac
    done < <(find "$MANAGED_STAGING" -mindepth 1 -maxdepth 1 -print0)

    if [[ "$managed_root_entries" -ne 2 ]]; then
        missing_targets=()
        for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
            if [[ ! -d "$MANAGED_STAGING/client-managed-packages-$target" ]]; then
                missing_targets+=("$target")
            fi
        done
        printf '::error::managed package matrix incomplete; missing targets: %s (all-or-nothing)\n' "${missing_targets[*]}" >&2
        exit 1
    fi

    for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
        dir="$MANAGED_STAGING/client-managed-packages-$target"
        if [[ -L "$dir" || ! -d "$dir" ]]; then
            echo "::error::managed artifact target must be a regular directory: $dir" >&2
            exit 1
        fi
        validate_managed_dir "$dir" "$target"
    done

    # Machine-encoded promotion boundary (#3336): the managed DEB/RPM
    # packages carry no package-manager signature and no centralized
    # provenance attestation beyond the per-target SHA-256 manifest verified
    # above. This is a stage-only producer -- it must not be mistaken for a
    # signed, centrally-attested release channel by any downstream consumer
    # (e.g. an apt/dnf repository publisher) until that issue is resolved.
    SIGNING_STATUS_DIR="$(mktemp -d "${TMPDIR:-/tmp}/terraphim-client-signing-status.XXXXXX")"
    trap 'rm -rf "$SIGNING_STATUS_DIR"' EXIT
    SIGNING_STATUS_FILE="$SIGNING_STATUS_DIR/DEB-RPM-PACKAGES-UNSIGNED-STAGE-ONLY.txt"
    cat > "$SIGNING_STATUS_FILE" <<EOF
terraphim-clients DEB/RPM managed packages: UNSIGNED, STAGE-ONLY

version: ${MANAGED_VERSION}

These DEB and RPM packages are produced by an automated CI producer. They
carry no package-manager signature (no dpkg-sig/debsigs, no rpm --addsign,
no detached GPG signature) and no centralized provenance attestation beyond
the per-target SHA-256 manifest verified against the staged bytes by this
release-assembly step.

status: unsigned
promotion: blocked
promotion_blocked_until: https://github.com/terraphim/terraphim-ai/issues/3336

Public promotion of these packages -- publishing them to an apt/dnf/yum
repository, a Homebrew tap, or any other package index advertised to end
users as an official terraphim-clients source -- is BLOCKED until central
signing, checksum, and provenance review is complete (issue #3336). Do not
promote, mirror, or advertise these DEB/RPM files as a signed,
centrally-attested release channel until that issue is resolved.
EOF
    register_file "$SIGNING_STATUS_FILE"
    MANAGED_FILES+=("$SIGNING_STATUS_FILE")
fi

# Merge the staged artifacts into the inventory.
for path in "${MANAGED_FILES[@]}"; do
    if [[ -L "$path" || ! -f "$path" || ! -s "$path" ]]; then
        echo "::error::validated managed artifact changed before merge: $path" >&2
        exit 1
    fi
done
for path in "${MANAGED_FILES[@]}"; do
    cp -f "$path" "$OUTPUT/"
done

printf 'release inventory assembled: %s files in %s\n' "$(find "$OUTPUT" -maxdepth 1 -type f | wc -l)" "$OUTPUT"
