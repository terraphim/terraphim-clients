#!/usr/bin/env bash
# Native gate for terraphim-clients nFPM packages (terraphim-agent,
# terraphim-grep; DEB and RPM).
#
# REQUIRE_INSTALL semantics (fail closed) mirror the reviewed
# terraphim_server nFPM native gate:
#   REQUIRE_INSTALL=0  -> install/upgrade/remove lifecycle is skipped with an
#                         explicit qualification message (byte/metadata/lint
#                         checks still run before this point in the gate).
#   REQUIRE_INSTALL=1  -> the install/upgrade/remove lifecycle MUST run.
#                         If the target triple is non-native on this host the
#                         gate fails; there is no successful QUALIFIED skip.
#
# Fixture binaries additionally implement `--version`, `check-update` and
# `update` subcommands that replicate terraphim_update's receipt-driven
# managed-mode contract exactly (crates/terraphim_update/src/policy.rs:
# PackageManager::update_command, policy::guidance): after install, running
# `<bin> check-update`/`<bin> update` must consult the on-disk receipt at
# <prefix>/share/terraphim/package-manager.d/<bin> the same way the real
# binaries do, make zero network connections, and (for `update`) perform zero
# writes to the installed executable. The deeper receipt-parsing edge cases
# (malformed values, CRLF, missing prefix, ...) are covered independently by
# `cargo test -p terraphim_update`; this gate proves the packaging contract
# end-to-end inside a real installed package.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
BUILD="$ROOT/.github/scripts/nfpm/build-client-packages.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/terraphim-client-nfpm-native.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

VERSION_OLD="${VERSION_OLD:-0.0.1}"
VERSION_NEW="${VERSION_NEW:-0.0.2}"
TARGET="${TARGET:-x86_64-unknown-linux-musl}"
NFPM_BIN="${NFPM_BIN:-nfpm}"
REQUIRE_INSTALL="${REQUIRE_INSTALL:-1}"
DEB_LIFECYCLE_IMAGE="${DEB_LIFECYCLE_IMAGE:-debian:bookworm-slim}"
RPM_LIFECYCLE_IMAGE="${RPM_LIFECYCLE_IMAGE:-fedora:latest}"

BIN_NAMES=(terraphim-agent terraphim-grep)

case "$TARGET" in
    x86_64-unknown-linux-musl)
        DEB_ARCH="amd64"
        RPM_ARCH="x86_64"
        NATIVE_MACHINE="x86_64"
        ;;
    aarch64-unknown-linux-musl)
        DEB_ARCH="arm64"
        RPM_ARCH="aarch64"
        NATIVE_MACHINE="aarch64"
        ;;
    *)
        echo "unsupported target for native gate: $TARGET" >&2
        exit 2
        ;;
esac

docker_available() {
    command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1
}

is_native_target() {
    [[ "$(uname -m)" == "$NATIVE_MACHINE" ]]
}

require_install_path() {
    local package_type="$1"
    echo "BLOCKED: $package_type install/upgrade/remove gate requires host root or Docker for native target $TARGET" >&2
    exit 127
}

# Fixture binaries.
#
# Native target: a real dynamically-linked executable compiled with the host
# C compiler (not a shell script: rpmlint E: no-binary; not static: lintian
# E: statically-linked-binary). The native install lifecycle executes it
# (`--version` must print the new version after upgrade; `check-update`/
# `update` must reproduce the receipt-driven managed-mode contract).
#
# Cross target: a deterministic minimal ELF for the target architecture
# (correct e_machine, PT_INTERP plus PT_DYNAMIC/DT_NEEDED so lintian and
# rpmlint see a dynamically linked foreign-arch binary). Cross fixtures are
# never executed: cross-target gates QUALIFY byte/metadata/lint only.
make_binary() {
    local path="$1"
    local bin_name="$2"
    local version="$3"

    mkdir -p "$(dirname "$path")"

    if ! is_native_target; then
        write_cross_elf_fixture "$path"
        return 0
    fi
    local cc_bin=""
    local candidate
    for candidate in "${CC:-}" cc gcc clang; do
        if [[ -n "$candidate" ]] && command -v "$candidate" >/dev/null 2>&1; then
            cc_bin="$candidate"
            break
        fi
    done
    [[ -n "$cc_bin" ]] || {
        echo "BLOCKED: a C compiler (CC/cc/gcc/clang) is required to build the native fixture binary" >&2
        exit 127
    }

    local src="$path.fixture.c"
    cat > "$src" <<'EOF'
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
#include <errno.h>
#include <libgen.h>

/* Reproduces terraphim_update's receipt-driven managed-mode contract
 * byte-for-byte, without linking the real crate:
 *   - prefix inference (<prefix>/bin/<bin> -> receipt at
 *     <prefix>/share/terraphim/package-manager.d/<bin>), see
 *     policy::detect_update_policy / policy::inferred_prefix;
 *   - PackageManager::name()/update_command() (policy.rs) supply the
 *     manager name and update command verbatim (the receipt value and the
 *     manager name are the same string for dpkg/rpm/pacman);
 *   - the check-update stdout line reproduces
 *     `impl Display for UpdateStatus::PackageManaged` (lib.rs) exactly:
 *     "[OK] Managed by {manager}; run `{update_command}` to update";
 *   - the update refusal stderr line reproduces
 *     `classify_update_status`'s fail-closed `other` arm (main.rs) exactly:
 *     "{bin_name} update was refused: {the Display line above}".
 * See the file header comment for the wider rationale. */
static const char *update_command_for(const char *value) {
    if (strcmp(value, "dpkg") == 0) return "sudo apt update && sudo apt upgrade";
    if (strcmp(value, "rpm") == 0) return "sudo dnf upgrade";
    if (strcmp(value, "pacman") == 0) return "sudo pacman -Syu";
    return NULL;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--version") == 0) {
        printf("%s %s\n", BIN_NAME, VERSION);
        return 0;
    }
    if (argc > 1 && (strcmp(argv[1], "check-update") == 0 || strcmp(argv[1], "update") == 0)) {
        char self[4096];
        ssize_t n = readlink("/proc/self/exe", self, sizeof(self) - 1);
        if (n < 0) { perror("readlink"); return 1; }
        self[n] = '\0';
        char *self_copy = strdup(self);
        char *bin_dir = dirname(self_copy);
        char *bin_dir_copy = strdup(bin_dir);
        char *prefix = dirname(bin_dir_copy);
        char receipt_path[4096];
        snprintf(receipt_path, sizeof(receipt_path),
                 "%s/share/terraphim/package-manager.d/%s", prefix, BIN_NAME);
        FILE *f = fopen(receipt_path, "r");
        if (!f) {
            printf("[OK] Already running latest version: %s\n", VERSION);
            return 0;
        }
        char value[64] = {0};
        size_t got = fread(value, 1, sizeof(value) - 1, f);
        fclose(f);
        while (got > 0 && (value[got - 1] == '\n' || value[got - 1] == '\r')) {
            value[--got] = '\0';
        }
        const char *cmd = update_command_for(value);
        if (!cmd) {
            printf("[OK] Already running latest version: %s\n", VERSION);
            return 0;
        }
        char status_line[256];
        snprintf(status_line, sizeof(status_line),
                 "[OK] Managed by %s; run `%s` to update", value, cmd);
        if (strcmp(argv[1], "check-update") == 0) {
            printf("%s\n", status_line);
            return 0;
        }
        fprintf(stderr, "%s update was refused: %s\n", BIN_NAME, status_line);
        return 1;
    }
    fprintf(stderr, "usage: %s --version|check-update|update\n", BIN_NAME);
    return 64;
}
EOF
    "$cc_bin" -O2 -s -DVERSION="\"$version\"" -DBIN_NAME="\"$bin_name\"" -o "$path" "$src"
    rm -f "$src"
    chmod 0755 "$path"
}

inspect_deb() {
    local deb="$1"
    local binary="$2"
    local bin_name="$3"
    local expected_sha actual_sha deps arch extract

    extract="$TMP/extract-deb-$(basename "$deb")"
    expected_sha="$(sha256sum "$binary" | awk '{print $1}')"
    mkdir -p "$extract"
    dpkg-deb --extract "$deb" "$extract"
    actual_sha="$(sha256sum "$extract/usr/bin/$bin_name" | awk '{print $1}')"
    [[ "$actual_sha" == "$expected_sha" ]] || {
        echo "DEB payload SHA mismatch expected=$expected_sha actual=$actual_sha" >&2
        exit 1
    }
    grep -qx 'dpkg' "$extract/usr/share/terraphim/package-manager.d/$bin_name"
    arch="$(dpkg-deb --field "$deb" Architecture)"
    [[ "$arch" == "$DEB_ARCH" ]] || {
        echo "DEB arch mismatch expected=$DEB_ARCH actual=$arch" >&2
        exit 1
    }
    deps="$(dpkg-deb --field "$deb" Depends 2>/dev/null || true)"
    if grep -Eiq '(^|[[:space:],|])((lib)?c6|glibc|gcc-libs|libstdc\+\+|libstdc\+\+6|libgcc|libgcc_s|libgcc-s1)([[:space:],|]|$)' <<<"$deps"; then
        echo "MUSL DEB declares forbidden dependency: $deps" >&2
        exit 1
    fi
}

inspect_rpm() {
    local rpm_pkg="$1"
    local binary="$2"
    local bin_name="$3"
    local expected_sha actual_sha deps arch extract metadata

    extract="$TMP/extract-rpm-$(basename "$rpm_pkg")"
    metadata="$extract.metadata"
    expected_sha="$(sha256sum "$binary" | awk '{print $1}')"
    mkdir -p "$extract"

    if command -v rpm2cpio >/dev/null 2>&1 && command -v rpm >/dev/null 2>&1 && command -v cpio >/dev/null 2>&1; then
        # --no-absolute-filenames keeps absolute RPM payload member names
        # (Ubuntu 24.04 rpm2cpio / nFPM 2.47) private to $extract instead of
        # writing toward the host's real /usr. Pipeline status is deliberately
        # not the success criterion: rpm 4.17's rpm2cpio (Ubuntu 24.04 and
        # Pop!_OS, i.e. every runner this gate runs on) exits 1 on nFPM 2.47
        # RPMs while writing a complete, correct payload. Note the status,
        # then let the payload-presence and SHA-256 checks below stay
        # fail-closed.
        local extract_log="$extract.cpio.log"
        if ! (cd "$extract" && rpm2cpio "$rpm_pkg" | cpio --no-absolute-filenames -idmv) >"$extract_log" 2>&1; then
            echo "NOTE: rpm2cpio|cpio returned nonzero for $rpm_pkg; verifying extracted payload" >&2
            sed 's/^/  /' "$extract_log" >&2
        fi
        {
            printf 'arch='
            rpm -qp --qf '%{ARCH}' "$rpm_pkg"
            printf '\nrequires<<EOF\n'
            rpm -qpR "$rpm_pkg" 2>/dev/null || true
            printf '\nEOF\nfile_digest='
            rpm -qp --qf '%{FILEDIGESTALGO}' "$rpm_pkg"
            printf '\n'
        } > "$metadata"
    elif docker_available; then
        : > "$metadata"
        docker run --rm \
            -v "$(realpath "$rpm_pkg"):/pkg.rpm:ro" \
            -v "$(realpath "$extract"):/extract" \
            -v "$(realpath "$metadata"):/metadata" \
            "$RPM_LIFECYCLE_IMAGE" \
            sh -euxc '
                if ! command -v rpm2cpio >/dev/null 2>&1 || ! command -v cpio >/dev/null 2>&1; then
                    if command -v dnf >/dev/null 2>&1; then
                        dnf install -y rpm cpio
                    elif command -v microdnf >/dev/null 2>&1; then
                        microdnf install -y rpm cpio
                    else
                        echo "no RPM package manager available in inspection image" >&2
                        exit 127
                    fi
                fi
                cd /extract
                # --no-absolute-filenames keeps absolute RPM payload member
                # names (Ubuntu 24.04 rpm2cpio / nFPM 2.47) private to
                # /extract. Pipeline status is deliberately not the success
                # criterion: the rpm 4.17 rpm2cpio exits 1 on nFPM 2.47 RPMs
                # while writing a complete, correct payload. Note the status,
                # then let the payload-presence check stay fail-closed (the
                # host side SHA-compares the extracted binary afterwards).
                if ! rpm2cpio /pkg.rpm | cpio --no-absolute-filenames -idmv >/tmp/rpm-extract.log 2>&1; then
                    echo "NOTE: rpm2cpio|cpio returned nonzero for /pkg.rpm; verifying extracted payload" >&2
                    sed "s/^/  /" /tmp/rpm-extract.log >&2
                fi
                if ! test -f "/extract/usr/bin/$1"; then
                    echo "RPM payload extraction produced no /extract/usr/bin/$1 (rpm2cpio | cpio --no-absolute-filenames -idmv):" >&2
                    sed "s/^/  /" /tmp/rpm-extract.log >&2
                    exit 1
                fi
                {
                    printf "arch="
                    rpm -qp --qf "%{ARCH}" /pkg.rpm
                    printf "\nrequires<<EOF\n"
                    rpm -qpR /pkg.rpm || true
                    printf "\nEOF\nfile_digest="
                    rpm -qp --qf "%{FILEDIGESTALGO}" /pkg.rpm
                    printf "\n"
                } > /metadata
                chmod -R a+rwX /extract /metadata
            ' sh "$bin_name"
    else
        echo "BLOCKED: RPM inspection requires host rpm/rpm2cpio/cpio or Docker" >&2
        exit 127
    fi

    # Fail-closed payload judgement for both branches: rpm2cpio|cpio status is
    # only advisory (NOTE above), so the extracted binary itself is the
    # criterion, SHA-compared immediately after.
    [[ -f "$extract/usr/bin/$bin_name" ]] || {
        echo "RPM payload extraction produced no $bin_name for $rpm_pkg" >&2
        exit 1
    }
    actual_sha="$(sha256sum "$extract/usr/bin/$bin_name" | awk '{print $1}')"
    [[ "$actual_sha" == "$expected_sha" ]] || {
        echo "RPM payload SHA mismatch expected=$expected_sha actual=$actual_sha" >&2
        exit 1
    }
    grep -qx 'rpm' "$extract/usr/share/terraphim/package-manager.d/$bin_name"
    arch="$(sed -n 's/^arch=//p' "$metadata")"
    [[ "$arch" == "$RPM_ARCH" ]] || {
        echo "RPM arch mismatch expected=$RPM_ARCH actual=$arch" >&2
        exit 1
    }
    deps="$(sed -n '/^requires<<EOF$/,/^EOF$/p' "$metadata" | sed '1d;$d')"
    if grep -Eiq '(^|[[:space:],|])((lib)?c6|glibc|gcc-libs|libstdc\+\+|libstdc\+\+6|libgcc|libgcc_s|libgcc-s1)([[:space:],|]|$)' <<<"$deps"; then
        echo "MUSL RPM declares forbidden dependency: $deps" >&2
        exit 1
    fi

    local file_digest
    file_digest="$(sed -n 's/^file_digest=//p' "$metadata")"
    [[ "$file_digest" == "8" ]] || {
        echo "RPM file digest metadata does not prove SHA-256: FILEDIGESTALGO=$file_digest" >&2
        exit 1
    }
}

write_cross_elf_fixture() {
    local path="$1"
    case "$TARGET" in
        aarch64-unknown-linux-musl)
            printf '%b' \
                '\x7f\x45\x4c\x46\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\xb7\x00\x01\x00\x00\x00\x60\x01\x40\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x40\x00\x38\x00\x04\x00\x40\x00\x00\x00\x00\x00\x01\x00\x00\x00\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x85\x01\x00\x00\x00\x00\x00\x00\x85\x01\x00\x00\x00\x00\x00\x00\x00\x10\x00\x00\x00\x00\x00\x00\x03\x00\x00\x00\x04\x00\x00\x00\x60\x01\x00\x00\x00\x00\x00\x00\x60\x01\x40\x00\x00\x00\x00\x00\x60\x01\x40\x00\x00\x00\x00\x00\x1b\x00\x00\x00\x00\x00\x00\x00\x1b\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x02\x00\x00\x00\x06\x00\x00\x00\x20\x01\x00\x00\x00\x00\x00\x00\x20\x01\x40\x00\x00\x00\x00\x00\x20\x01\x40\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x00\x00\x08\x00\x00\x00\x00\x00\x00\x00\x51\xe5\x74\x64\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x05\x00\x00\x00\x00\x00\x00\x00\x7b\x01\x40\x00\x00\x00\x00\x00\x0a\x00\x00\x00\x00\x00\x00\x00\x0a\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x2f\x6c\x69\x62\x2f\x6c\x64\x2d\x6c\x69\x6e\x75\x78\x2d\x61\x61\x72\x63\x68\x36\x34\x2e\x73\x6f\x2e\x31\x00\x6c\x69\x62\x63\x2e\x73\x6f\x2e\x36\x00' > "$path"
            ;;
        x86_64-unknown-linux-musl)
            printf '%b' \
                '\x7f\x45\x4c\x46\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\x3e\x00\x01\x00\x00\x00\x60\x01\x40\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x40\x00\x38\x00\x04\x00\x40\x00\x00\x00\x00\x00\x01\x00\x00\x00\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x86\x01\x00\x00\x00\x00\x00\x00\x86\x01\x00\x00\x00\x00\x00\x00\x00\x10\x00\x00\x00\x00\x00\x00\x03\x00\x00\x00\x04\x00\x00\x00\x60\x01\x00\x00\x00\x00\x00\x00\x60\x01\x40\x00\x00\x00\x00\x00\x60\x01\x40\x00\x00\x00\x00\x00\x1c\x00\x00\x00\x00\x00\x00\x00\x1c\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x02\x00\x00\x00\x06\x00\x00\x00\x20\x01\x00\x00\x00\x00\x00\x00\x20\x01\x40\x00\x00\x00\x00\x00\x20\x01\x40\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x00\x00\x40\x00\x00\x00\x00\x00\x00\x00\x08\x00\x00\x00\x00\x00\x00\x00\x51\xe5\x74\x64\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x05\x00\x00\x00\x00\x00\x00\x00\x7c\x01\x40\x00\x00\x00\x00\x00\x0a\x00\x00\x00\x00\x00\x00\x00\x0a\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x2f\x6c\x69\x62\x36\x34\x2f\x6c\x64\x2d\x6c\x69\x6e\x75\x78\x2d\x78\x38\x36\x2d\x36\x34\x2e\x73\x6f\x2e\x32\x00\x6c\x69\x62\x63\x2e\x73\x6f\x2e\x36\x00' > "$path"
            ;;
        *)
            echo "unsupported cross fixture target: $TARGET" >&2
            exit 2
            ;;
    esac
    chmod 0755 "$path"
}

# Managed-mode check-update/update assertions run inside the already-running
# container (host or docker) after install: zero network (--network none for
# the docker path; the host path has no network dependency in the fixture
# either way) and zero writes to the installed executable.
managed_mode_assertions_deb_docker() {
    local image="$1"
    local old_deb="$2"
    local bin_name="$3"

    docker run --rm --network none \
        -v "$(realpath "$old_deb"):/old.deb:ro" \
        -e BIN_NAME="$bin_name" \
        "$image" \
        sh -euxc '
            dpkg -i /old.deb
            before_sha="$(sha256sum "/usr/bin/$BIN_NAME" | awk "{print \$1}")"
            "/usr/bin/$BIN_NAME" check-update | grep -Fx "[OK] Managed by dpkg; run \`sudo apt update && sudo apt upgrade\` to update"
            if "/usr/bin/$BIN_NAME" update 2>/tmp/update.stderr; then
                echo "update must refuse under a dpkg receipt" >&2
                exit 1
            fi
            grep -Fxq "$BIN_NAME update was refused: [OK] Managed by dpkg; run \`sudo apt update && sudo apt upgrade\` to update" /tmp/update.stderr
            after_sha="$(sha256sum "/usr/bin/$BIN_NAME" | awk "{print \$1}")"
            test "$before_sha" = "$after_sha"
        '
}

managed_mode_assertions_rpm_docker() {
    local image="$1"
    local old_rpm="$2"
    local bin_name="$3"

    docker run --rm --network none \
        -v "$(realpath "$old_rpm"):/old.rpm:ro" \
        -e BIN_NAME="$bin_name" \
        "$image" \
        sh -euxc '
            if ! command -v rpm >/dev/null 2>&1; then
                if command -v dnf >/dev/null 2>&1; then dnf install -y rpm
                elif command -v microdnf >/dev/null 2>&1; then microdnf install -y rpm
                else echo "no RPM package manager available in lifecycle image" >&2; exit 127
                fi
            fi
            rpm -Uvh /old.rpm
            before_sha="$(sha256sum "/usr/bin/$BIN_NAME" | awk "{print \$1}")"
            "/usr/bin/$BIN_NAME" check-update | grep -Fx "[OK] Managed by rpm; run \`sudo dnf upgrade\` to update"
            if "/usr/bin/$BIN_NAME" update 2>/tmp/update.stderr; then
                echo "update must refuse under an rpm receipt" >&2
                exit 1
            fi
            grep -Fxq "$BIN_NAME update was refused: [OK] Managed by rpm; run \`sudo dnf upgrade\` to update" /tmp/update.stderr
            after_sha="$(sha256sum "/usr/bin/$BIN_NAME" | awk "{print \$1}")"
            test "$before_sha" = "$after_sha"
        '
}

install_upgrade_remove_deb_host() {
    local old_deb="$1"
    local new_deb="$2"
    local bin_name="$3"
    # expected_version: substring the installed `--version` output must
    # contain after the upgrade. Defaults to VERSION_NEW (baked into the
    # synthetic fixture binaries by make_binary()). expected_pkg_version:
    # when set, the exact "<version>-1" dpkg Version field the upgraded
    # package must report (proves the upgrade transition happened at the
    # package-manager level, independent of the installed binary's own
    # --version output, which is identical before/after when old and new
    # wrap the same real binary -- see test_client_nfpm_native_actual.sh).
    local expected_version="${4:-$VERSION_NEW}"
    local expected_pkg_version="${5:-}"

    dpkg -i "$old_deb"
    dpkg-query -S "/usr/bin/$bin_name" >/dev/null
    dpkg-query -S "/usr/share/terraphim/package-manager.d/$bin_name" >/dev/null
    [[ "$(stat -c '%U:%G %a' "/usr/bin/$bin_name")" == "root:root 755" ]]
    grep -qx 'dpkg' "/usr/share/terraphim/package-manager.d/$bin_name"
    local before_sha after_sha
    before_sha="$(sha256sum "/usr/bin/$bin_name" | awk '{print $1}')"
    "/usr/bin/$bin_name" check-update | grep -Fx "[OK] Managed by dpkg; run \`sudo apt update && sudo apt upgrade\` to update"
    if "/usr/bin/$bin_name" update 2>"$TMP/update-$bin_name.stderr"; then
        echo "update must refuse under a dpkg receipt" >&2
        exit 1
    fi
    grep -Fxq "$bin_name update was refused: [OK] Managed by dpkg; run \`sudo apt update && sudo apt upgrade\` to update" "$TMP/update-$bin_name.stderr"
    after_sha="$(sha256sum "/usr/bin/$bin_name" | awk '{print $1}')"
    [[ "$before_sha" == "$after_sha" ]] || { echo "managed update wrote to $bin_name" >&2; exit 1; }
    dpkg -i "$new_deb"
    "/usr/bin/$bin_name" --version | grep -F "$expected_version"
    if [[ -n "$expected_pkg_version" ]]; then
        dpkg-query -W -f '${Version}' "$bin_name" | grep -Fxq "${expected_pkg_version}-1" ||
            { echo "dpkg Version field did not advance to ${expected_pkg_version}-1 after upgrade" >&2; exit 1; }
    fi
    dpkg -r "$bin_name"
    # Explicit fail-closed form (not a bare `test`/`grep` relying on `set -e`
    # propagation): this function is invoked from callers that `source` this
    # file (test_client_nfpm_policy.sh, test_client_nfpm_native_actual.sh),
    # and bash has a documented errexit-after-conditional quirk where a prior
    # `if`/function-body conditional executed anywhere earlier in the current
    # shell's history can desensitize `set -e` for everything that follows --
    # including in an unrelated later function call. #326 P2-2's uninstall
    # absence check must not depend on that.
    [[ ! -e "/usr/share/terraphim/package-manager.d/$bin_name" ]] || {
        echo "receipt still present after dpkg -r $bin_name: /usr/share/terraphim/package-manager.d/$bin_name" >&2
        exit 1
    }
    [[ ! -e "/usr/bin/$bin_name" ]] || {
        echo "binary still present after dpkg -r $bin_name: /usr/bin/$bin_name" >&2
        exit 1
    }
}

install_upgrade_remove_deb_docker() {
    local old_deb="$1"
    local new_deb="$2"
    local bin_name="$3"
    local expected_version="${4:-$VERSION_NEW}"
    local expected_pkg_version="${5:-}"

    docker run --rm \
        -v "$(realpath "$old_deb"):/old.deb:ro" \
        -v "$(realpath "$new_deb"):/new.deb:ro" \
        -e EXPECTED_VERSION="$expected_version" \
        -e EXPECTED_PKG_VERSION="$expected_pkg_version" \
        -e BIN_NAME="$bin_name" \
        "$DEB_LIFECYCLE_IMAGE" \
        sh -euxc '
            dpkg -i /old.deb
            dpkg-query -S "/usr/bin/$BIN_NAME" >/dev/null
            dpkg-query -S "/usr/share/terraphim/package-manager.d/$BIN_NAME" >/dev/null
            test "$(stat -c "%U:%G %a" "/usr/bin/$BIN_NAME")" = "root:root 755"
            grep -qx dpkg "/usr/share/terraphim/package-manager.d/$BIN_NAME"
            dpkg -i /new.deb
            "/usr/bin/$BIN_NAME" --version | grep -F "$EXPECTED_VERSION"
            if [ -n "$EXPECTED_PKG_VERSION" ]; then
                dpkg-query -W -f "\${Version}" "$BIN_NAME" | grep -Fxq "${EXPECTED_PKG_VERSION}-1"
            fi
            dpkg -r "$BIN_NAME"
            test ! -e "/usr/share/terraphim/package-manager.d/$BIN_NAME"
            test ! -e "/usr/bin/$BIN_NAME"
        '
    managed_mode_assertions_deb_docker "$DEB_LIFECYCLE_IMAGE" "$old_deb" "$bin_name"
}

install_upgrade_remove_rpm_host() {
    local old_rpm="$1"
    local new_rpm="$2"
    local bin_name="$3"
    local expected_version="${4:-$VERSION_NEW}"
    local expected_pkg_version="${5:-}"

    rpm -Uvh "$old_rpm"
    rpm -qf "/usr/bin/$bin_name" >/dev/null
    rpm -qf "/usr/share/terraphim/package-manager.d/$bin_name" >/dev/null
    [[ "$(stat -c '%U:%G %a' "/usr/bin/$bin_name")" == "root:root 755" ]]
    grep -qx 'rpm' "/usr/share/terraphim/package-manager.d/$bin_name"
    local before_sha after_sha
    before_sha="$(sha256sum "/usr/bin/$bin_name" | awk '{print $1}')"
    "/usr/bin/$bin_name" check-update | grep -Fx "[OK] Managed by rpm; run \`sudo dnf upgrade\` to update"
    if "/usr/bin/$bin_name" update 2>"$TMP/update-$bin_name.stderr"; then
        echo "update must refuse under an rpm receipt" >&2
        exit 1
    fi
    grep -Fxq "$bin_name update was refused: [OK] Managed by rpm; run \`sudo dnf upgrade\` to update" "$TMP/update-$bin_name.stderr"
    after_sha="$(sha256sum "/usr/bin/$bin_name" | awk '{print $1}')"
    [[ "$before_sha" == "$after_sha" ]] || { echo "managed update wrote to $bin_name" >&2; exit 1; }
    rpm -Uvh "$new_rpm"
    "/usr/bin/$bin_name" --version | grep -F "$expected_version"
    if [[ -n "$expected_pkg_version" ]]; then
        rpm -q --qf '%{VERSION}' "$bin_name" | grep -Fxq "$expected_pkg_version" ||
            { echo "rpm VERSION tag did not advance to $expected_pkg_version after upgrade" >&2; exit 1; }
    fi
    rpm -e "$bin_name"
    # See the matching comment in install_upgrade_remove_deb_host: explicit
    # fail-closed form, not a bare `test` relying on `set -e` propagation.
    [[ ! -e "/usr/share/terraphim/package-manager.d/$bin_name" ]] || {
        echo "receipt still present after rpm -e $bin_name: /usr/share/terraphim/package-manager.d/$bin_name" >&2
        exit 1
    }
    [[ ! -e "/usr/bin/$bin_name" ]] || {
        echo "binary still present after rpm -e $bin_name: /usr/bin/$bin_name" >&2
        exit 1
    }
}

install_upgrade_remove_rpm_docker() {
    local old_rpm="$1"
    local new_rpm="$2"
    local bin_name="$3"
    local expected_version="${4:-$VERSION_NEW}"
    local expected_pkg_version="${5:-}"

    docker run --rm \
        -v "$(realpath "$old_rpm"):/old.rpm:ro" \
        -v "$(realpath "$new_rpm"):/new.rpm:ro" \
        -e EXPECTED_VERSION="$expected_version" \
        -e EXPECTED_PKG_VERSION="$expected_pkg_version" \
        -e BIN_NAME="$bin_name" \
        "$RPM_LIFECYCLE_IMAGE" \
        sh -euxc '
            if ! command -v rpm >/dev/null 2>&1; then
                if command -v dnf >/dev/null 2>&1; then dnf install -y rpm
                elif command -v microdnf >/dev/null 2>&1; then microdnf install -y rpm
                else echo "no RPM package manager available in lifecycle image" >&2; exit 127
                fi
            fi
            rpm -Uvh /old.rpm
            rpm -qf "/usr/bin/$BIN_NAME" >/dev/null
            rpm -qf "/usr/share/terraphim/package-manager.d/$BIN_NAME" >/dev/null
            test "$(stat -c "%U:%G %a" "/usr/bin/$BIN_NAME")" = "root:root 755"
            grep -qx rpm "/usr/share/terraphim/package-manager.d/$BIN_NAME"
            rpm -Uvh /new.rpm
            "/usr/bin/$BIN_NAME" --version | grep -F "$EXPECTED_VERSION"
            if [ -n "$EXPECTED_PKG_VERSION" ]; then
                rpm -q --qf "%{VERSION}" "$BIN_NAME" | grep -Fxq "$EXPECTED_PKG_VERSION"
            fi
            rpm -e "$BIN_NAME"
            test ! -e "/usr/share/terraphim/package-manager.d/$BIN_NAME"
            test ! -e "/usr/bin/$BIN_NAME"
        '
    managed_mode_assertions_rpm_docker "$RPM_LIFECYCLE_IMAGE" "$old_rpm" "$bin_name"
}

install_upgrade_remove_deb() {
    local old_deb="$1"
    local new_deb="$2"
    local bin_name="$3"
    local expected_version="${4:-$VERSION_NEW}"
    local expected_pkg_version="${5:-}"

    if [[ "$REQUIRE_INSTALL" == "0" ]]; then
        if ! is_native_target; then
            echo "QUALIFIED: $TARGET DEB byte/metadata/lint checks passed; install lifecycle skipped for non-native target (REQUIRE_INSTALL=0)"
        else
            echo "SKIP: DEB install/upgrade/remove gate disabled by REQUIRE_INSTALL=0 for native target $TARGET"
        fi
        return 0
    fi

    if ! is_native_target; then
        echo "BLOCKED: REQUIRE_INSTALL=1 requires the DEB install/upgrade/remove gate, but target $TARGET is non-native on $(uname -m); cross-target byte/metadata/lint-only qualification must be requested explicitly with REQUIRE_INSTALL=0" >&2
        exit 1
    fi

    if [[ "$(id -u)" -eq 0 ]] && command -v dpkg >/dev/null 2>&1; then
        install_upgrade_remove_deb_host "$old_deb" "$new_deb" "$bin_name" "$expected_version" "$expected_pkg_version"
    elif docker_available; then
        install_upgrade_remove_deb_docker "$old_deb" "$new_deb" "$bin_name" "$expected_version" "$expected_pkg_version"
    else
        require_install_path "DEB"
    fi
}

install_upgrade_remove_rpm() {
    local old_rpm="$1"
    local new_rpm="$2"
    local bin_name="$3"
    local expected_version="${4:-$VERSION_NEW}"
    local expected_pkg_version="${5:-}"

    if [[ "$REQUIRE_INSTALL" == "0" ]]; then
        if ! is_native_target; then
            echo "QUALIFIED: $TARGET RPM byte/metadata/lint checks passed; install lifecycle skipped for non-native target (REQUIRE_INSTALL=0)"
        else
            echo "SKIP: RPM install/upgrade/remove gate disabled by REQUIRE_INSTALL=0 for native target $TARGET"
        fi
        return 0
    fi

    if ! is_native_target; then
        echo "BLOCKED: REQUIRE_INSTALL=1 requires the RPM install/upgrade/remove gate, but target $TARGET is non-native on $(uname -m); cross-target byte/metadata/lint-only qualification must be requested explicitly with REQUIRE_INSTALL=0" >&2
        exit 1
    fi

    if [[ "$(id -u)" -eq 0 ]] && command -v rpm >/dev/null 2>&1; then
        install_upgrade_remove_rpm_host "$old_rpm" "$new_rpm" "$bin_name" "$expected_version" "$expected_pkg_version"
    elif docker_available; then
        install_upgrade_remove_rpm_docker "$old_rpm" "$new_rpm" "$bin_name" "$expected_version" "$expected_pkg_version"
    else
        require_install_path "RPM"
    fi
}

main() {
    command -v "$NFPM_BIN" >/dev/null 2>&1 || {
        echo "BLOCKED: nFPM is required for native gate: $NFPM_BIN" >&2
        exit 127
    }

    export SOURCE_DATE_EPOCH=1700000000

    local -A OLD_BIN NEW_BIN
    for bin_name in "${BIN_NAMES[@]}"; do
        OLD_BIN[$bin_name]="$TMP/v-old/$bin_name"
        NEW_BIN[$bin_name]="$TMP/v-new/$bin_name"
        make_binary "${OLD_BIN[$bin_name]}" "$bin_name" "$VERSION_OLD"
        make_binary "${NEW_BIN[$bin_name]}" "$bin_name" "$VERSION_NEW"
    done

    "$BUILD" --version "$VERSION_OLD" --target "$TARGET" \
        --agent-binary "${OLD_BIN[terraphim-agent]}" --grep-binary "${OLD_BIN[terraphim-grep]}" \
        --out-dir "$TMP/out-old" --nfpm "$NFPM_BIN"
    "$BUILD" --version "$VERSION_NEW" --target "$TARGET" \
        --agent-binary "${NEW_BIN[terraphim-agent]}" --grep-binary "${NEW_BIN[terraphim-grep]}" \
        --out-dir "$TMP/out-new-a" --nfpm "$NFPM_BIN"
    "$BUILD" --version "$VERSION_NEW" --target "$TARGET" \
        --agent-binary "${NEW_BIN[terraphim-agent]}" --grep-binary "${NEW_BIN[terraphim-grep]}" \
        --out-dir "$TMP/out-new-b" --nfpm "$NFPM_BIN"

    for bin_name in "${BIN_NAMES[@]}"; do
        local old_deb new_deb_a new_deb_b old_rpm new_rpm_a new_rpm_b
        old_deb="$TMP/out-old/${bin_name}_${VERSION_OLD}-1_${DEB_ARCH}.deb"
        new_deb_a="$TMP/out-new-a/${bin_name}_${VERSION_NEW}-1_${DEB_ARCH}.deb"
        new_deb_b="$TMP/out-new-b/${bin_name}_${VERSION_NEW}-1_${DEB_ARCH}.deb"
        old_rpm="$TMP/out-old/${bin_name}-${VERSION_OLD}-1.${RPM_ARCH}.rpm"
        new_rpm_a="$TMP/out-new-a/${bin_name}-${VERSION_NEW}-1.${RPM_ARCH}.rpm"
        new_rpm_b="$TMP/out-new-b/${bin_name}-${VERSION_NEW}-1.${RPM_ARCH}.rpm"

        inspect_deb "$old_deb" "${OLD_BIN[$bin_name]}" "$bin_name"
        inspect_deb "$new_deb_a" "${NEW_BIN[$bin_name]}" "$bin_name"
        inspect_rpm "$old_rpm" "${OLD_BIN[$bin_name]}" "$bin_name"
        inspect_rpm "$new_rpm_a" "${NEW_BIN[$bin_name]}" "$bin_name"

        cmp "$new_deb_a" "$new_deb_b"
        cmp "$new_rpm_a" "$new_rpm_b"

        install_upgrade_remove_deb "$old_deb" "$new_deb_a" "$bin_name"
        install_upgrade_remove_rpm "$old_rpm" "$new_rpm_a" "$bin_name"
    done

    echo "client nFPM native gate passed for $TARGET"
}

# Policy regression tests source this script with
# TERRAPHIM_CLIENT_NFPM_NATIVE_SOURCED=1 to drive
# install_upgrade_remove_deb/rpm directly on fixture packages.
if [[ "${TERRAPHIM_CLIENT_NFPM_NATIVE_SOURCED:-0}" != "1" ]]; then
    main "$@"
fi
