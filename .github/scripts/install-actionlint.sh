#!/usr/bin/env bash
# Install the pinned actionlint binary for the release workflow contracts.
#
# tests/test_release_binaries_workflow_contract.py shells out to actionlint
# (test_workflow_is_parsed_by_actionlint). Hosted runners do not ship it, so
# every ci.yml job that runs an actionlint-using suite must provision the
# tool first (hosted run 35433041935 failed both jobs with
# FileNotFoundError: actionlint).
#
# Fail-closed properties:
#   * exact official release artefact + official SHA-256 (sha256sum -c)
#   * only the `actionlint` member is extracted from the archive
#   * `actionlint -version` must report exactly ${ACTIONLINT_VERSION}
#     before the tool is exposed to the job
#   * private temp working dir removed by an EXIT trap
#   * requires the runner contract (RUNNER_TEMP, GITHUB_PATH); fails closed
#     when either is unset or empty
#   * installed into the job-scoped RUNNER_TEMP directory and appended to
#     $GITHUB_PATH unconditionally
set -euo pipefail

ACTIONLINT_VERSION="1.7.12"
ACTIONLINT_ARCHIVE="actionlint_${ACTIONLINT_VERSION}_linux_amd64.tar.gz"
ACTIONLINT_URL="https://github.com/rhysd/actionlint/releases/download/v${ACTIONLINT_VERSION}/${ACTIONLINT_ARCHIVE}"
# Official SHA-256 of actionlint_1.7.12_linux_amd64.tar.gz from the
# rhysd/actionlint v1.7.12 release notes.
ACTIONLINT_SHA256="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"

# Runner contract: this is a CI-only installer. Failing closed on a missing
# environment beats silently installing to an unpredictable location.
: "${RUNNER_TEMP:?RUNNER_TEMP must point at the job-scoped temporary directory}"
: "${GITHUB_PATH:?GITHUB_PATH must point at the job PATH mutation file}"

# Job-scoped install destination.
bin_dir="${RUNNER_TEMP}/actionlint-${ACTIONLINT_VERSION}-bin"
mkdir -p "$bin_dir"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

archive="$work_dir/$ACTIONLINT_ARCHIVE"
curl -fsSL "$ACTIONLINT_URL" -o "$archive"
printf '%s  %s\n' "$ACTIONLINT_SHA256" "$archive" | sha256sum -c -

# Extract exactly the actionlint binary -- nothing else from the archive
# is placed on disk.
tar -xzf "$archive" -C "$work_dir" actionlint
install -m 0755 "$work_dir/actionlint" "$bin_dir/actionlint"

# Exact-version proof before the tool reaches the job's PATH. The official
# release binary reports the bare version (`1.7.12`) on the first line of
# `actionlint -version`.
version_output="$("$bin_dir/actionlint" -version)"
printf '%s\n' "$version_output"
grep -qx "${ACTIONLINT_VERSION}" <<<"$version_output"

printf '%s\n' "$bin_dir" >> "$GITHUB_PATH"
