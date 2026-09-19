#!/usr/bin/env bash
# Render the nFPM descriptor for a terraphim-clients managed package.
#
# Adapted from the reviewed terraphim_server nFPM renderer, generalized to
# the two hyphenated client binaries (terraphim-agent, terraphim-grep): the
# package name equals the binary name exactly (no underscore/hyphen split),
# and each binary carries its own SPDX license and description.

set -euo pipefail

# Single-quote a scalar for safe interpolation into the rendered YAML
# (defense-in-depth alongside the upfront VERSION/BINARY validation below):
# YAML single-quoted scalars take no escapes except '' for a literal quote,
# so this is safe for any string that does not itself contain a newline
# (VERSION and BINARY are both rejected earlier if they do).
yaml_squote() {
    local s="$1"
    printf "'%s'" "${s//\'/\'\'}"
}

usage() {
    cat >&2 <<'EOF'
Usage: render-client-nfpm.sh --format deb|rpm --binary-name NAME --version VERSION --target TRIPLE --binary PATH --output PATH

NAME must be one of: terraphim-agent, terraphim-grep.

The target must be a qualified Linux MUSL client binary:
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-musl
EOF
}

FORMAT=""
BINARY_NAME=""
VERSION=""
TARGET=""
BINARY=""
OUTPUT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --format)
            FORMAT="${2:-}"
            shift 2
            ;;
        --binary-name)
            BINARY_NAME="${2:-}"
            shift 2
            ;;
        --version)
            VERSION="${2:-}"
            shift 2
            ;;
        --target)
            TARGET="${2:-}"
            shift 2
            ;;
        --binary)
            BINARY="${2:-}"
            shift 2
            ;;
        --output)
            OUTPUT="${2:-}"
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

if [[ -z "$FORMAT" || -z "$BINARY_NAME" || -z "$VERSION" || -z "$TARGET" || -z "$BINARY" || -z "$OUTPUT" ]]; then
    usage
    exit 2
fi

# Single explicit package/version contract (#326 P1-4/P2-1): the renderer
# independently enforces the same canonical-stable-only predicate as
# build-client-packages.sh's CLIENT_PACKAGE_VERSION_RE, rather than trusting
# its caller. VERSION is interpolated into the rendered YAML descriptor (and
# into the DEB changelog / RPM chglog YAML), so this also closes the YAML
# injection vector a permissive version string would otherwise open (a
# version like $'9.9.9\nprovides: ["INJECTED-PKG"]' would inject a top-level
# YAML key into the nFPM config).
CLIENT_PACKAGE_VERSION_RE='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
if [[ ! "$VERSION" =~ $CLIENT_PACKAGE_VERSION_RE ]]; then
    echo "unsupported client package version (expected canonical stable MAJOR.MINOR.PATCH only, e.g. 1.2.3; prerelease and build-metadata suffixes are rejected): $VERSION" >&2
    exit 2
fi

# OUTPUT is both the descriptor path and the prefix for generated sidecar
# paths that are interpolated into YAML. Reject control characters before
# creating any file so a direct caller cannot inject descriptor keys.
if [[ "$OUTPUT" == *$'\n'* || "$OUTPUT" == *$'\r'* || "$OUTPUT" =~ [[:cntrl:]] ]]; then
    echo "unsupported client package output path (must not contain newline or control characters): $OUTPUT" >&2
    exit 2
fi

# BINARY is a caller-supplied filesystem path interpolated raw into the
# rendered YAML (`src: ${BINARY}`); reject anything that could break out of
# its YAML scalar or that isn't the safe, already-qualified file it claims to
# be. (FORMAT/BINARY_NAME/TARGET are already constrained to a fixed enum by
# the case statements below/above, so they need no additional validation.)
if [[ "$BINARY" == *$'\n'* || "$BINARY" == *$'\r'* || "$BINARY" =~ [[:cntrl:]] ]]; then
    echo "unsupported client package binary path (must not contain newline or control characters): $BINARY" >&2
    exit 2
fi
if [[ -L "$BINARY" ]]; then
    echo "qualified binary must be a regular non-symlink file: $BINARY" >&2
    exit 1
fi

case "$FORMAT" in
    deb)
        RECEIPT_VALUE="dpkg"
        ;;
    rpm)
        RECEIPT_VALUE="rpm"
        ;;
    *)
        echo "unsupported package format: $FORMAT" >&2
        exit 2
        ;;
esac

case "$TARGET" in
    x86_64-unknown-linux-musl)
        DEB_ARCH="amd64"
        RPM_ARCH="x86_64"
        ;;
    aarch64-unknown-linux-musl)
        DEB_ARCH="arm64"
        RPM_ARCH="aarch64"
        ;;
    *)
        echo "unsupported client package target: $TARGET" >&2
        echo "only qualified MUSL targets are accepted" >&2
        exit 2
        ;;
esac

case "$BINARY_NAME" in
    terraphim-agent)
        SPDX_LICENSE="Apache-2.0"
        LICENSE_BASENAME="LICENSE-Apache-2.0"
        SUMMARY="Terraphim AI Agent CLI"
        DESCRIPTION_LINES=$'Terraphim AI Agent CLI.\n  Command-line interface with interactive REPL and ASCII graph visualization.'
        ;;
    terraphim-grep)
        SPDX_LICENSE="MIT"
        LICENSE_BASENAME="LICENSE-MIT"
        SUMMARY="Intelligent hybrid grep with knowledge-graph boosting"
        DESCRIPTION_LINES=$'Terraphim Grep.\n  Intelligent hybrid grep with RLM fallback and KG curation.'
        ;;
    *)
        echo "unsupported client package binary name: $BINARY_NAME" >&2
        echo "only terraphim-agent and terraphim-grep are accepted" >&2
        exit 2
        ;;
esac

if [[ ! -f "$BINARY" ]]; then
    echo "missing qualified binary: $BINARY" >&2
    exit 1
fi

case "$FORMAT" in
    deb) ARCH="$DEB_ARCH" ;;
    rpm) ARCH="$RPM_ARCH" ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
LICENSE_FILE="$ROOT/$LICENSE_BASENAME"
README_FILE="$ROOT/README.md"
if [[ ! -f "$LICENSE_FILE" ]]; then
    echo "missing license file: $LICENSE_FILE" >&2
    exit 1
fi
if [[ ! -f "$README_FILE" ]]; then
    echo "missing readme file: $README_FILE" >&2
    exit 1
fi

if [[ -z "${SOURCE_DATE_EPOCH:-}" ]]; then
    echo "SOURCE_DATE_EPOCH is required to render deterministic package metadata" >&2
    exit 1
fi

OUTPUT_DIR="$(dirname "$OUTPUT")"
mkdir -p "$OUTPUT_DIR"
RECEIPT_FILE="${OUTPUT}.${BINARY_NAME}.receipt"
printf '%s\n' "$RECEIPT_VALUE" > "$RECEIPT_FILE"
chmod 0644 "$RECEIPT_FILE"

COPYRIGHT_FILE="${OUTPUT}.copyright"
CHANGELOG_FILE="${OUTPUT}.changelog.Debian"
CHANGELOG_GZ="${CHANGELOG_FILE}.gz"
CHANGELOG_YAML="${OUTPUT}.changelog.yaml"
MANPAGE_FILE="${OUTPUT}.${BINARY_NAME}.1"
MANPAGE_GZ="${MANPAGE_FILE}.gz"

# Machine-readable Debian copyright that references the common license
# instead of embedding the full license text.
cat > "$COPYRIGHT_FILE" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: terraphim-ai
Upstream-Contact: Terraphim Team <team@terraphim.ai>
Source: https://github.com/terraphim/terraphim-ai

Files: *
Copyright: 2024, Terraphim Contributors
License: ${SPDX_LICENSE}
 On Debian systems, the complete text of the ${SPDX_LICENSE} license, can be
 found in /usr/share/common-licenses/${SPDX_LICENSE}.
EOF

cat > "$CHANGELOG_FILE" <<EOF
${BINARY_NAME} (${VERSION}-1) stable; urgency=medium

  * Build managed package from a qualified ${BINARY_NAME} MUSL binary.

 -- Terraphim Contributors <team@terraphim.ai>  $(date -u -d "@${SOURCE_DATE_EPOCH}" '+%a, %d %b %Y %H:%M:%S +0000')
EOF
gzip -9n -c "$CHANGELOG_FILE" > "$CHANGELOG_GZ"

# chglog YAML consumed by nFPM for the native RPM changelog tags.
cat > "$CHANGELOG_YAML" <<EOF
- semver: ${VERSION}
  date: $(date -u -d "@${SOURCE_DATE_EPOCH}" '+%Y-%m-%dT%H:%M:%SZ')
  packager: Terraphim Contributors <team@terraphim.ai>
  changes:
    - commit: ""
      note: Build managed package from a qualified ${BINARY_NAME} MUSL binary.
EOF

cat > "$MANPAGE_FILE" <<EOF
.TH ${BINARY_NAME^^} 1
.SH NAME
${BINARY_NAME} \\- ${SUMMARY}
.SH SYNOPSIS
.B ${BINARY_NAME}
.RI [ options ]
.SH DESCRIPTION
${SUMMARY}.
.SH SEE ALSO
Project documentation is available at https://terraphim.ai.
EOF
gzip -9n -c "$MANPAGE_FILE" > "$MANPAGE_GZ"

touch -d "@${SOURCE_DATE_EPOCH}" \
    "$RECEIPT_FILE" \
    "$COPYRIGHT_FILE" \
    "$CHANGELOG_FILE" \
    "$CHANGELOG_GZ" \
    "$CHANGELOG_YAML" \
    "$MANPAGE_FILE" \
    "$MANPAGE_GZ"
chmod 0644 "$COPYRIGHT_FILE" "$CHANGELOG_GZ" "$MANPAGE_GZ"

# The native RPM changelog tags come from the chglog YAML; the DEB keeps the
# deterministic hand-rendered changelog.Debian.gz instead.
CHANGELOG_CONFIG=""
if [[ "$FORMAT" == "rpm" ]]; then
    CHANGELOG_CONFIG="changelog: ${CHANGELOG_YAML}"
fi

cat > "$OUTPUT" <<EOF
name: ${BINARY_NAME}
arch: ${ARCH}
platform: linux
version: $(yaml_squote "$VERSION")
release: "1"
section: utils
priority: optional
maintainer: Terraphim Contributors <team@terraphim.ai>
vendor: Terraphim
homepage: https://terraphim.ai
license: ${SPDX_LICENSE}
${CHANGELOG_CONFIG}
description: |-
  ${DESCRIPTION_LINES}
rpm:
  group: Applications/System
  summary: ${SUMMARY}
  compression: xz
deb:
  compression: xz
contents:
  - src: $(yaml_squote "$BINARY")
    dst: /usr/bin/${BINARY_NAME}
    type: file
    file_info:
      mode: 0755
  - src: ${RECEIPT_FILE}
    dst: /usr/share/terraphim/package-manager.d/${BINARY_NAME}
    type: file
    file_info:
      mode: 0644
  - src: ${LICENSE_FILE}
    dst: /usr/share/doc/${BINARY_NAME}/LICENSE
    type: file
    file_info:
      mode: 0644
    packager: deb
  - src: ${LICENSE_FILE}
    dst: /usr/share/licenses/${BINARY_NAME}/${LICENSE_BASENAME}
    type: license
    file_info:
      mode: 0644
    packager: rpm
  - src: ${README_FILE}
    dst: /usr/share/doc/${BINARY_NAME}/README.md
    type: doc
    file_info:
      mode: 0644
    packager: rpm
  - src: ${COPYRIGHT_FILE}
    dst: /usr/share/doc/${BINARY_NAME}/copyright
    type: file
    file_info:
      mode: 0644
    packager: deb
  - src: ${CHANGELOG_GZ}
    dst: /usr/share/doc/${BINARY_NAME}/changelog.Debian.gz
    type: file
    file_info:
      mode: 0644
    packager: deb
  - src: ${MANPAGE_GZ}
    dst: /usr/share/man/man1/${BINARY_NAME}.1.gz
    type: file
    file_info:
      mode: 0644
    packager: deb
  - src: ${MANPAGE_GZ}
    dst: /usr/share/man/man1/${BINARY_NAME}.1.gz
    type: doc
    file_info:
      mode: 0644
    packager: rpm
overrides:
  deb:
    depends: []
  rpm:
    depends: []
EOF

echo "$OUTPUT"
