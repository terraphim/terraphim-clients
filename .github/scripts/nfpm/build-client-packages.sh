#!/usr/bin/env bash
# Wrap qualified terraphim-clients MUSL binaries into managed DEB/RPM
# packages: terraphim-agent and terraphim-grep, DEB and RPM each, for one
# Linux MUSL target triple.
#
# Adapted from the reviewed terraphim_server nFPM producer, generalized to
# two hyphenated client binaries packaged together into one staged,
# all-or-none output directory per target (4 packages + one SHA256SUMS
# manifest). Fails closed on architecture: each produced DEB must declare
# Architecture == DEB_ARCH for the requested target and each produced RPM
# must carry ARCH == RPM_ARCH (consumed from the Docker RPM metadata when
# host rpm tooling is unavailable).

set -euo pipefail

usage() {
    cat >&2 <<'EOF'
Usage: build-client-packages.sh --version VERSION --target TRIPLE \
    --agent-binary PATH --grep-binary PATH --out-dir DIR [--nfpm PATH]

Produces, per target:
  terraphim-agent_VERSION-1_amd64.deb    terraphim-agent-VERSION-1.x86_64.rpm
  terraphim-grep_VERSION-1_amd64.deb     terraphim-grep-VERSION-1.x86_64.rpm
or the arm64/aarch64 equivalents, plus one SHA256SUMS manifest covering all
four package files.
EOF
}

# Single explicit package/version contract (#326 P1-4): this producer packages
# only canonical stable releases. Prerelease identifiers ('-rc.1') and build
# metadata ('+build.1') are rejected fail-closed -- not because nFPM cannot
# represent them (it normalizes '-' to '~' in the DEB/RPM version it embeds),
# but because the managed DEB/RPM channel is scoped to stable releases only,
# matching the same predicate used everywhere else this contract is checked
# (render-client-nfpm.sh, assemble-client-release-inventory.sh, and the
# release workflow's package-stage gate). No leading zeros, matching normal
# semver numeric-identifier rules.
CLIENT_PACKAGE_VERSION_RE='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'

FORBIDDEN_MUSL_DEPS_RE='(^|[[:space:],|])((lib)?c6|glibc|gcc-libs|libstdc\+\+|libstdc\+\+6|libgcc|libgcc_s|libgcc-s1)([[:space:],|]|$)'
DEB_LINT_IMAGE="${DEB_LINT_IMAGE:-debian:bookworm-slim}"
RPM_LINT_IMAGE="${RPM_LINT_IMAGE:-fedora:latest}"
RPM_TOOL_IMAGE="${RPM_TOOL_IMAGE:-fedora:latest}"

docker_available() {
    command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1
}

require_docker_or_fail() {
    local purpose="$1"
    docker_available || {
        echo "BLOCKED: Docker is required for $purpose when host tools are unavailable" >&2
        exit 127
    }
}

validate_qualified_binary() {
    local path="$1"
    local target="$2"
    local label="$3"

    if [[ -L "$path" || ! -f "$path" ]]; then
        echo "$label must be a regular non-symlink file: $path" >&2
        exit 1
    fi
    if [[ ! -s "$path" ]]; then
        echo "$label must not be zero-length: $path" >&2
        exit 1
    fi
    command -v readelf >/dev/null 2>&1 || {
        echo "readelf is required to validate qualified binary ELF architecture" >&2
        exit 127
    }
    if ! LC_ALL=C readelf -h -- "$path" >/dev/null 2>&1; then
        echo "$label is not a valid ELF file: $path" >&2
        exit 1
    fi

    local ident machine expected_machine
    ident="$(od -An -tx1 -N6 -- "$path" | tr -d '[:space:]')"
    machine="$(od -An -tx1 -j18 -N2 -- "$path" | tr -d '[:space:]')"
    [[ "$ident" == "7f454c460201" ]] || {
        echo "$label is not a supported ELF64 little-endian file: $path" >&2
        exit 1
    }
    case "$target" in
        x86_64-unknown-linux-musl) expected_machine="3e00" ;;
        aarch64-unknown-linux-musl) expected_machine="b700" ;;
        *)
            echo "unsupported client package target: $target" >&2
            exit 2
            ;;
    esac
    [[ "$machine" == "$expected_machine" ]] || {
        echo "$label ELF architecture mismatch target=$target expected_machine=$expected_machine actual_machine=$machine" >&2
        exit 1
    }
}

# Canonical release inputs are immutable: this producer packages the exact
# bytes it is handed and never mutates or re-derives them. Debug symbols must
# already be gone before the binary reaches this script (the separate #248
# producer strips once when staging); this only verifies that fact and fails
# closed with a clear diagnostic otherwise, so a not-yet-stripped input can
# never be packaged and hashed as though it were the intended release
# artifact.
validate_stripped_binary() {
    local path="$1"
    local label="$2"

    command -v readelf >/dev/null 2>&1 || {
        echo "readelf is required to validate that $label is already stripped" >&2
        exit 127
    }

    local sections
    sections="$(LC_ALL=C readelf -S -W -- "$path" 2>/dev/null)" || {
        echo "$label: readelf failed to read ELF section headers: $path" >&2
        exit 1
    }

    # `strip --strip-unneeded` removes .symtab and every .debug_*/.zdebug_*
    # section while preserving .dynsym/.dynstr (the dynamic symbol table a
    # dynamically-linked binary needs at runtime); this checks for exactly
    # what that strip removes, so a canonical input that was truly stripped
    # -- static or dynamic -- always passes.
    local -a leftover_sections=()
    mapfile -t leftover_sections < <(
        grep -Eo '\.(symtab|debug[a-zA-Z0-9_]*|zdebug[a-zA-Z0-9_]*)\b' <<<"$sections" | sort -u
    )
    if [[ "${#leftover_sections[@]}" -gt 0 ]]; then
        echo "$label is not stripped (found ELF section(s): ${leftover_sections[*]}); canonical release inputs must already be stripped (e.g. with \`strip --strip-unneeded\`) before they reach this packaging pipeline: $path" >&2
        exit 1
    fi
}

validate_extracted_payload() {
    local path="$1"
    local format="$2"
    if [[ -L "$path" || ! -f "$path" || ! -s "$path" ]]; then
        echo "extracted $format payload must be a non-empty regular non-symlink file: $path" >&2
        exit 1
    fi
}

# Fail-closed lint result policy shared by the host and Docker lint paths.
#
# The only tolerated lint errors are the exact justified static-MUSL
# diagnostic(s) for the current $BIN_NAME at usr/bin/$BIN_NAME (each at most
# once); everything else fails the build. See build-server-packages.sh for
# the full policy rationale (this is a direct generalization of that logic).
#
# lintian additionally tolerates one exact "embedded-library libyaml"
# diagnostic, but ONLY for terraphim-agent: this was verified against the
# real qualified x86_64 MUSL terraphim-agent binary (lintian 2.116.3), which
# bundles a statically-linked copy of libyaml that lintian's embedded-code
# heuristic detects. The real terraphim-grep binary lints clean of this
# diagnostic, so it is intentionally not generalized across $BIN_NAME the
# way statically-linked-binary is -- terraphim-grep must not silently
# inherit an allowance its own real binary never earned.
enforce_lint_policy() {
    local tool="$1"
    local pkg="$2"
    local log="$3"
    local rc="$4"
    local transport="$5"

    echo "----- $tool transport log for $(basename "$pkg") -----"
    cat "$transport" 2>/dev/null || true
    echo "----- $tool raw lint output for $(basename "$pkg") (exit $rc) -----"
    cat "$log" 2>/dev/null || true
    echo "----- end $tool evidence for $(basename "$pkg") -----"

    if [[ ! -f "$log" ]]; then
        echo "$tool produced no lint output file for $pkg" >&2
        exit 1
    fi

    local -a error_rcs=()
    local justified_re=""
    local -a justified_literals=()
    case "$tool" in
        lintian)
            error_rcs=(2)
            justified_literals=("E: ${BIN_NAME}: statically-linked-binary [usr/bin/${BIN_NAME}]")
            if [[ "$BIN_NAME" == "terraphim-agent" ]]; then
                justified_literals+=("E: terraphim-agent: embedded-library libyaml [usr/bin/terraphim-agent]")
            fi
            ;;
        rpmlint)
            error_rcs=(64 65)
            justified_re="^${BIN_NAME}\\.${RPM_ARCH}: E: statically-linked-binary /usr/bin/${BIN_NAME}\$"
            ;;
        *)
            echo "unknown lint tool: $tool" >&2
            exit 1
            ;;
    esac

    local rc_class="invalid"
    if [[ "$rc" -eq 0 ]]; then
        rc_class="clean"
    else
        local candidate
        for candidate in "${error_rcs[@]}"; do
            if [[ "$rc" -eq "$candidate" ]]; then
                rc_class="errors"
            fi
        done
    fi
    if [[ "$rc_class" == "invalid" ]]; then
        printf '%s exited %s for %s: tool/install/transport failure (allowed exits: 0' "$tool" "$rc" "$pkg" >&2
        printf ' %s' "${error_rcs[@]}" >&2
        printf ')\n' >&2
        exit 1
    fi

    if [[ "$tool" == "rpmlint" && ! -s "$log" ]]; then
        echo "rpmlint produced empty lint output for $pkg (a real run always reports a session banner)" >&2
        exit 1
    fi

    local -a e_lines=()
    case "$tool" in
        lintian)
            mapfile -t e_lines < <(grep -E '^E: ' "$log" || true)
            ;;
        rpmlint)
            mapfile -t e_lines < <(grep -E '^[^:[:space:]]+: E: ' "$log" || true)
            ;;
    esac

    local justified=0
    local -A justified_counts=()
    local line ok lit
    for line in "${e_lines[@]}"; do
        ok=0
        if [[ "$tool" == "lintian" ]]; then
            for lit in "${justified_literals[@]}"; do
                if [[ "$line" == "$lit" ]]; then
                    ok=1
                    justified_counts["$lit"]=$(( ${justified_counts["$lit"]:-0} + 1 ))
                    if [[ "${justified_counts[$lit]}" -gt 1 ]]; then
                        echo "$tool reported the justified static-MUSL diagnostic more than once for $pkg:" >&2
                        printf '  %s\n' "$line" >&2
                        exit 1
                    fi
                    break
                fi
            done
        else
            if [[ "$line" =~ $justified_re ]]; then
                ok=1
            fi
        fi
        if [[ "$ok" -eq 1 ]]; then
            justified=$((justified + 1))
            if [[ "$tool" == "rpmlint" && "$justified" -gt 1 ]]; then
                echo "$tool reported the justified static-MUSL diagnostic more than once for $pkg:" >&2
                printf '  %s\n' "$line" >&2
                exit 1
            fi
        else
            echo "$tool reported an unjustified error for $pkg (only the exact static-MUSL diagnostic(s) for ${BIN_NAME} at usr/bin/${BIN_NAME} are tolerated):" >&2
            printf '  %s\n' "$line" >&2
            exit 1
        fi
    done

    if [[ "$rc_class" == "errors" && "$justified" -eq 0 ]]; then
        echo "$tool exited $rc (errors reported) but no error line could be parsed from the lint output for $pkg" >&2
        exit 1
    fi
    if [[ "$rc_class" == "clean" && "$justified" -ne 0 ]]; then
        echo "$tool exited 0 but the lint output contains error lines for $pkg" >&2
        exit 1
    fi

    echo "lint policy satisfied: $tool accepted $(basename "$pkg") with $justified justified static-MUSL diagnostic(s)"
}

lint_deb() {
    local pkg="$1"
    local log
    log="$WORK_DIR/lintian-$(basename "$pkg").log"
    local transport="$log.transport"
    local rc=0

    : > "$log"
    : > "$transport"

    if command -v lintian >/dev/null 2>&1; then
        # --tag-display-limit 0 keeps lintian from hiding error lines
        # behind its per-tag display cap: the parser must see every E:.
        lintian --fail-on error --tag-display-limit 0 "$pkg" >"$log" 2>&1 || rc=$?
    else
        require_docker_or_fail "DEB linting"
        docker run --rm \
            -v "$(realpath "$pkg"):/pkg.deb:ro" \
            -v "$(realpath "$log"):/lint.log" \
            "$DEB_LINT_IMAGE" sh -euxc '
                export DEBIAN_FRONTEND=noninteractive
                apt-get update
                apt-get install -y --no-install-recommends lintian
                rc=0
                lintian --fail-on error --tag-display-limit 0 /pkg.deb >/lint.log 2>&1 || rc=$?
                exit "$rc"
            ' >"$transport" 2>&1 || rc=$?
    fi

    enforce_lint_policy lintian "$pkg" "$log" "$rc" "$transport"
}

lint_rpm() {
    local pkg="$1"
    # Mount with the real package basename so rpmlint sees a coherent
    # name-version-release.arch.rpm filename.
    local base
    base="$(basename "$pkg")"
    local log="$WORK_DIR/rpmlint-$base.log"
    local transport="$log.transport"
    local rc=0

    : > "$log"
    : > "$transport"

    if command -v rpmlint >/dev/null 2>&1; then
        rpmlint "$pkg" >"$log" 2>&1 || rc=$?
    else
        require_docker_or_fail "RPM linting"
        docker run --rm \
            -v "$(realpath "$pkg"):/$base:ro" \
            -v "$(realpath "$log"):/lint.log" \
            "$RPM_LINT_IMAGE" sh -euxc '
                if ! command -v rpmlint >/dev/null 2>&1; then
                    if command -v dnf >/dev/null 2>&1; then
                        dnf install -y rpmlint
                    elif command -v microdnf >/dev/null 2>&1; then
                        microdnf install -y rpmlint
                    else
                        echo "no RPM package manager available in lint image" >&2
                        exit 127
                    fi
                fi
                rc=0
                rpmlint "/$1" >/lint.log 2>&1 || rc=$?
                exit "$rc"
            ' sh "$base" >"$transport" 2>&1 || rc=$?
    fi

    enforce_lint_policy rpmlint "$pkg" "$log" "$rc" "$transport"
}

docker_rpm_tool() {
    local pkg="$1"
    local extract="$2"
    local expected_sha="$3"
    local metadata="$4"
    local bin_name="$5"
    local abs_pkg abs_extract abs_metadata

    abs_pkg="$(realpath "$pkg")"
    abs_extract="$(realpath "$extract")"
    abs_metadata="$(realpath "$metadata")"
    require_docker_or_fail "RPM payload and metadata verification"

    docker run --rm \
        -v "$abs_pkg:/pkg.rpm:ro" \
        -v "$abs_extract:/extract" \
        -v "$abs_metadata:/metadata" \
        "$RPM_TOOL_IMAGE" \
        sh -euxc '
            if ! command -v rpm2cpio >/dev/null 2>&1 || ! command -v cpio >/dev/null 2>&1; then
                if command -v dnf >/dev/null 2>&1; then
                    dnf install -y rpm cpio
                elif command -v microdnf >/dev/null 2>&1; then
                    microdnf install -y rpm cpio
                else
                    echo "no RPM package manager available in verification image" >&2
                    exit 127
                fi
            fi
            cd /extract
            # --no-absolute-filenames keeps RPM payload members with
            # absolute names (Ubuntu 24.04 rpm2cpio / nFPM 2.47) private to
            # /extract; keep the log off the mounted volume so the host-side
            # cleanup trap never meets a root-owned file, and surface it on
            # failure instead of discarding stderr.
            if ! rpm2cpio /pkg.rpm | cpio --no-absolute-filenames -idmv >/tmp/rpm-extract.log 2>&1; then
                echo "RPM payload extraction failed for /pkg.rpm (rpm2cpio | cpio --no-absolute-filenames -idmv):" >&2
                sed "s/^/  /" /tmp/rpm-extract.log >&2
                exit 1
            fi
            payload="/extract/usr/bin/$2"
            if test -L "$payload" || ! test -f "$payload" || ! test -s "$payload"; then
                echo "extracted RPM payload must be a non-empty regular non-symlink file: $payload" >&2
                exit 1
            fi
            actual_sha="$(sha256sum "$payload" | awk "{print \$1}")"
            test "$actual_sha" = "$1"
            grep -qx rpm "/extract/usr/share/terraphim/package-manager.d/$2"
            {
                printf "arch="
                rpm -qp --qf "%{ARCH}" /pkg.rpm
                printf "\nrequires<<EOF\n"
                rpm -qpR /pkg.rpm || true
                printf "\nEOF\nfile_digest="
                rpm -qp --qf "%{FILEDIGESTALGO}" /pkg.rpm
                printf "\n"
            } > /metadata
            # Container writes are root-owned; keep the host-side cleanup trap
            # able to remove them.
            chmod -R a+rwX /extract /metadata
        ' sh "$expected_sha" "$bin_name"
}

render_and_build() {
    local format="$1"
    local config="$WORK_DIR/$BIN_NAME-$format.yaml"

    "$RENDER" \
        --format "$format" \
        --binary-name "$BIN_NAME" \
        --version "$VERSION" \
        --target "$TARGET" \
        --binary "$BINARY" \
        --output "$config" >/dev/null

    "$NFPM_BIN" pkg --packager "$format" --config "$config" --target "$PACKAGE_DIR"
}

cleanup_stage() {
    local rc=$?
    trap - EXIT
    if [[ -n "${STAGE_ROOT:-}" && -n "${STAGE_PARENT:-}" &&
        "$(dirname -- "$STAGE_ROOT")" == "$STAGE_PARENT" &&
        "$(basename -- "$STAGE_ROOT")" == .terraphim-client-nfpm.* &&
        ( -e "$STAGE_ROOT" || -L "$STAGE_ROOT" ) ]]; then
        # rm does not dereference a symlink supplied as its command-line
        # operand. The constrained mktemp basename prevents a broad target.
        rm -rf -- "$STAGE_ROOT" || true
    fi
    exit "$rc"
}

require_safe_empty_output_dir() {
    if [[ -L "$OUT_DIR" || ( -e "$OUT_DIR" && ! -d "$OUT_DIR" ) ]]; then
        echo "unsafe output directory (must be a regular directory, not a symlink): $OUT_DIR" >&2
        exit 1
    fi
    if [[ -d "$OUT_DIR" ]] && find "$OUT_DIR" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
        echo "output directory must be empty; refusing to delete pre-existing data: $OUT_DIR" >&2
        exit 1
    fi
}

validate_publish_inventory() {
    local path base count=0
    declare -A expected=()
    local name
    for name in "${EXPECTED_BASENAMES[@]}"; do
        expected["$name"]=1
    done

    while IFS= read -r -d '' path; do
        if [[ -L "$path" || ! -f "$path" || ! -s "$path" ]]; then
            echo "staged package output must be a non-empty regular non-symlink file: $path" >&2
            exit 1
        fi
        base="$(basename "$path")"
        if [[ -z "${expected[$base]:-}" ]]; then
            echo "unexpected staged package output: $path" >&2
            exit 1
        fi
        count=$((count + 1))
    done < <(find "$PACKAGE_DIR" -mindepth 1 -maxdepth 1 -print0)

    [[ "$count" -eq "${#EXPECTED_BASENAMES[@]}" ]] || {
        echo "staged package inventory is incomplete" >&2
        exit 1
    }
    for name in "${EXPECTED_BASENAMES[@]}"; do
        [[ -f "$PACKAGE_DIR/$name" ]] || {
            echo "staged package inventory is missing $name" >&2
            exit 1
        }
    done
}

verify_deb() {
    local pkg="$1"

    [[ -f "$pkg" ]] || { echo "missing DEB output: $pkg" >&2; exit 1; }

    # Fail closed unless the produced package declares the architecture that
    # was requested for the target triple.
    local pkg_arch
    pkg_arch="$(dpkg-deb --field "$pkg" Architecture)"
    [[ "$pkg_arch" == "$DEB_ARCH" ]] || {
        echo "DEB arch mismatch expected=$DEB_ARCH actual=$pkg_arch package=$pkg" >&2
        exit 1
    }

    local tmp="$WORK_DIR/deb-extract-$BIN_NAME"
    mkdir -p "$tmp"
    dpkg-deb --extract "$pkg" "$tmp"

    validate_extracted_payload "$tmp/usr/bin/$BIN_NAME" DEB
    local actual_sha
    actual_sha="$(sha256sum "$tmp/usr/bin/$BIN_NAME" | awk '{print $1}')"
    [[ "$actual_sha" == "$EXPECTED_SHA" ]] || {
        echo "DEB payload SHA mismatch expected=$EXPECTED_SHA actual=$actual_sha" >&2
        exit 1
    }

    # Explicit fail-closed form (not a bare command relying on `set -e`
    # propagation): a bare `grep -qx ... || exit`-free statement several
    # function-call frames deep can silently stop enforcing under bash's
    # documented errexit-after-conditional quirk (a prior `if`/function-body
    # conditional executed anywhere earlier in the call chain can desensitize
    # `set -e` for the remainder of the current shell). The receipt is the
    # terraphim_update managed-mode contract's own trust anchor, so its check
    # must never depend on that.
    grep -qx 'dpkg' "$tmp/usr/share/terraphim/package-manager.d/$BIN_NAME" || {
        echo "DEB package-manager receipt missing or does not read exactly 'dpkg': $tmp/usr/share/terraphim/package-manager.d/$BIN_NAME" >&2
        exit 1
    }

    local deps
    deps="$(dpkg-deb --field "$pkg" Depends 2>/dev/null || true)"
    if grep -Eiq "$FORBIDDEN_MUSL_DEPS_RE" <<<"$deps"; then
        echo "MUSL DEB declares forbidden glibc/gcc runtime dependency: $deps" >&2
        exit 1
    fi

    lint_deb "$pkg"
}

verify_rpm() {
    local pkg="$1"
    local tmp="$WORK_DIR/rpm-extract-$BIN_NAME"
    local metadata="$WORK_DIR/rpm.metadata-$BIN_NAME"

    [[ -f "$pkg" ]] || { echo "missing RPM output: $pkg" >&2; exit 1; }
    mkdir -p "$tmp"
    : > "$metadata"

    if command -v rpm2cpio >/dev/null 2>&1 && command -v rpm >/dev/null 2>&1 && command -v cpio >/dev/null 2>&1; then
        # --no-absolute-filenames is mandatory: Ubuntu 24.04's rpm2cpio
        # emits nFPM 2.47 RPM payload members with absolute names
        # (/usr/bin/<bin>, ...), and copy-in without the option then either
        # fails outright or writes toward the host's real /usr. Extraction must
        # stay private to $tmp, fail closed on any nonzero status, and
        # surface the rpm2cpio/cpio diagnostics instead of discarding them.
        local extract_log="$WORK_DIR/rpm-extract-$BIN_NAME.log"
        if ! (cd "$tmp" && rpm2cpio "$pkg" | cpio --no-absolute-filenames -idmv) >"$extract_log" 2>&1; then
            echo "RPM payload extraction failed for $pkg (rpm2cpio | cpio --no-absolute-filenames -idmv):" >&2
            sed 's/^/  /' "$extract_log" >&2
            exit 1
        fi
    else
        docker_rpm_tool "$pkg" "$tmp" "$EXPECTED_SHA" "$metadata" "$BIN_NAME"
    fi

    validate_extracted_payload "$tmp/usr/bin/$BIN_NAME" RPM

    # Fail closed unless the produced package carries the architecture that
    # was requested for the target triple. The arch is consumed from the
    # Docker RPM metadata when host rpm tooling produced it, or queried from
    # the host rpm otherwise.
    local pkg_arch
    if [[ -s "$metadata" ]]; then
        pkg_arch="$(sed -n 's/^arch=//p' "$metadata")"
    else
        pkg_arch="$(rpm -qp --qf '%{ARCH}' "$pkg")"
    fi
    [[ "$pkg_arch" == "$RPM_ARCH" ]] || {
        echo "RPM arch mismatch expected=$RPM_ARCH actual=$pkg_arch package=$pkg" >&2
        exit 1
    }

    local actual_sha
    actual_sha="$(sha256sum "$tmp/usr/bin/$BIN_NAME" | awk '{print $1}')"
    [[ "$actual_sha" == "$EXPECTED_SHA" ]] || {
        echo "RPM payload SHA mismatch expected=$EXPECTED_SHA actual=$actual_sha" >&2
        exit 1
    }

    # See the matching comment in verify_deb: explicit fail-closed form, not
    # a bare command relying on `set -e` propagation.
    grep -qx 'rpm' "$tmp/usr/share/terraphim/package-manager.d/$BIN_NAME" || {
        echo "RPM package-manager receipt missing or does not read exactly 'rpm': $tmp/usr/share/terraphim/package-manager.d/$BIN_NAME" >&2
        exit 1
    }

    local deps
    if [[ -s "$metadata" ]]; then
        deps="$(sed -n '/^requires<<EOF$/,/^EOF$/p' "$metadata" | sed '1d;$d')"
    else
        deps="$(rpm -qpR "$pkg" 2>/dev/null || true)"
    fi
    if grep -Eiq "$FORBIDDEN_MUSL_DEPS_RE" <<<"$deps"; then
        echo "MUSL RPM declares forbidden glibc/gcc runtime dependency: $deps" >&2
        exit 1
    fi

    local pkg_digest_algo
    if [[ -s "$metadata" ]]; then
        pkg_digest_algo="$(sed -n 's/^file_digest=//p' "$metadata")"
    else
        pkg_digest_algo="$(rpm -qp --qf '%{FILEDIGESTALGO}' "$pkg")"
    fi
    [[ "$pkg_digest_algo" == "8" ]] || {
        echo "RPM file digest metadata does not prove SHA-256: FILEDIGESTALGO=$pkg_digest_algo" >&2
        exit 1
    }

    lint_rpm "$pkg"
}

build_one_binary() {
    local bin_name="$1"
    local source_binary="$2"

    BIN_NAME="$bin_name"
    validate_qualified_binary "$source_binary" "$TARGET" "qualified $bin_name binary"
    validate_stripped_binary "$source_binary" "qualified $bin_name binary"

    # Capture the qualified input into private storage without following a
    # source symlink raced into place. Validate and package only this copy so
    # later source-path changes cannot alter the package payload. This copy
    # is never mutated: the packaged payload SHA binds to the exact bytes the
    # caller supplied, which is why the input must already be stripped.
    local validated_binary="$WORK_DIR/qualified-$bin_name"
    cp -P --reflink=never -- "$source_binary" "$validated_binary"
    validate_qualified_binary "$source_binary" "$TARGET" "qualified $bin_name binary"
    validate_qualified_binary "$validated_binary" "$TARGET" "staged qualified $bin_name binary"
    if ! cmp -s -- "$source_binary" "$validated_binary"; then
        echo "qualified $bin_name binary changed while creating the validated private copy: $source_binary" >&2
        exit 1
    fi
    validate_stripped_binary "$validated_binary" "staged qualified $bin_name binary"
    BINARY="$validated_binary"

    EXPECTED_SHA="$(sha256sum "$BINARY" | awk '{print $1}')"

    render_and_build deb
    render_and_build rpm

    local deb_base rpm_base
    deb_base="${bin_name}_${PKG_VERSION}-1_${DEB_ARCH}.deb"
    rpm_base="${bin_name}-${PKG_VERSION}-1.${RPM_ARCH}.rpm"

    verify_deb "$PACKAGE_DIR/$deb_base"
    verify_rpm "$PACKAGE_DIR/$rpm_base"

    EXPECTED_BASENAMES+=("$deb_base" "$rpm_base")
    printf 'package payload ok %s %s %s\n' "$bin_name" "$TARGET" "$EXPECTED_SHA"
}

main() {
    local VERSION="" TARGET="" AGENT_BINARY="" GREP_BINARY="" OUT_DIR=""
    local NFPM_BIN="${NFPM_BIN:-nfpm}"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --version)
                VERSION="${2:-}"
                shift 2
                ;;
            --target)
                TARGET="${2:-}"
                shift 2
                ;;
            --agent-binary)
                AGENT_BINARY="${2:-}"
                shift 2
                ;;
            --grep-binary)
                GREP_BINARY="${2:-}"
                shift 2
                ;;
            --out-dir)
                OUT_DIR="${2:-}"
                shift 2
                ;;
            --nfpm)
                NFPM_BIN="${2:-}"
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

    if [[ -z "$VERSION" || -z "$TARGET" || -z "$AGENT_BINARY" || -z "$GREP_BINARY" || -z "$OUT_DIR" ]]; then
        usage
        exit 2
    fi

    if [[ ! "$VERSION" =~ $CLIENT_PACKAGE_VERSION_RE ]]; then
        echo "unsupported client package version (expected canonical stable MAJOR.MINOR.PATCH only, e.g. 1.2.3; prerelease and build-metadata suffixes are rejected): $VERSION" >&2
        exit 2
    fi
    # CLIENT_PACKAGE_VERSION_RE guarantees no '-' or '+' can appear, so the
    # package version is always the input version verbatim.
    local PKG_VERSION="$VERSION"

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
            exit 2
            ;;
    esac

    validate_qualified_binary "$AGENT_BINARY" "$TARGET" "qualified terraphim-agent binary"
    validate_qualified_binary "$GREP_BINARY" "$TARGET" "qualified terraphim-grep binary"

    if ! command -v "$NFPM_BIN" >/dev/null 2>&1; then
        echo "nFPM is required for managed package production; not found: $NFPM_BIN" >&2
        exit 127
    fi

    local ROOT OUT_PARENT OUT_BASE
    ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
    RENDER="$ROOT/.github/scripts/nfpm/render-client-nfpm.sh"
    OUT_PARENT="$(dirname -- "$OUT_DIR")"
    OUT_BASE="$(basename -- "$OUT_DIR")"
    mkdir -p "$OUT_PARENT"
    OUT_DIR="$OUT_PARENT/$OUT_BASE"
    require_safe_empty_output_dir

    # Stage on the output filesystem so publishing can replace the empty
    # destination with one directory rename after every validation passes.
    STAGE_PARENT="$OUT_PARENT"
    STAGE_ROOT="$(mktemp -d "$STAGE_PARENT/.terraphim-client-nfpm.XXXXXX")"
    WORK_DIR="$STAGE_ROOT/work"
    PACKAGE_DIR="$STAGE_ROOT/packages"
    mkdir -m 0700 "$WORK_DIR" "$PACKAGE_DIR"
    trap cleanup_stage EXIT

    if [[ -z "${SOURCE_DATE_EPOCH:-}" ]]; then
        if git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
            export SOURCE_DATE_EPOCH
            SOURCE_DATE_EPOCH="$(git -C "$ROOT" log -1 --format=%ct)"
        else
            echo "SOURCE_DATE_EPOCH is required outside a git worktree" >&2
            exit 1
        fi
    fi

    EXPECTED_BASENAMES=()
    build_one_binary terraphim-agent "$AGENT_BINARY"
    build_one_binary terraphim-grep "$GREP_BINARY"

    local sums_base="terraphim-clients-${VERSION}-${TARGET}.package-sha256sums.txt"
    EXPECTED_BASENAMES+=("$sums_base")
    (cd "$PACKAGE_DIR" && sha256sum "${EXPECTED_BASENAMES[@]:0:4}" > "$sums_base")
    validate_publish_inventory

    # Recheck immediately before publication. GNU mv -T treats OUT_DIR as the
    # exact destination and atomically replaces an empty directory without
    # traversing a raced symlink or nesting packages inside a raced directory.
    require_safe_empty_output_dir
    mv -T -- "$PACKAGE_DIR" "$OUT_DIR"
    printf 'client managed packages staged ok %s\n' "$TARGET"
}

# Tests source this script with TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED=1 to
# exercise verify_deb/verify_rpm on tampered fixtures without running the
# production pipeline.
if [[ "${TERRAPHIM_BUILD_CLIENT_PACKAGES_SOURCED:-0}" != "1" ]]; then
    main "$@"
fi
