#!/usr/bin/env bash
# Publish a crate to the terraphim registry.
#
# The terraphim-native runner's command policy inspects the literal first token
# of every `run:` line in a workflow and rejects anything that is not on its
# allowlist; a multi-line block - with comments, shell conditionals or
# heredocs - is rejected before the first step runs (terraphim-clients#400).
# A checked-in script is the shape the runner accepts (the native-ci lanes
# call `bash ./scripts/ci/...`), so the whole publish path lives here and the
# workflow calls it with a single line.
#
# Inputs (both through the workflow step's `env:`, never through shell text):
#   CARGO_REGISTRIES_TERRAPHIM_TOKEN - the registry credential (required)
#   CRATE                             - the package to publish (required)
#
# PUBLISH_DRY_RUN=1 stops before the publish, so the path can be exercised
# without publishing anything.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

if [ -z "${CARGO_REGISTRIES_TERRAPHIM_TOKEN:-}" ]; then
    printf '%s\n' \
        'error: CARGO_REGISTRIES_TERRAPHIM_TOKEN is required (after sourcing ~/.profile set it to "Bearer $GITEA_TOKEN")' >&2
    exit 2
fi

if [ -z "${CRATE:-}" ]; then
    printf '%s\n' 'error: CRATE is required' >&2
    exit 2
fi

cargo_home="${CARGO_HOME:-$HOME/.cargo}"
mkdir -p "$cargo_home"

# The registry credential and the sparse index. Written with printf rather
# than a heredoc, because a heredoc is one of the constructs the runner's
# policy rejects in a workflow's `run:` block.
{
    printf '[registries.terraphim]\n'
    printf 'token = "%s"\n' "${CARGO_REGISTRIES_TERRAPHIM_TOKEN}"
} >>"$cargo_home/credentials.toml"

{
    printf '\n[registries.terraphim]\n'
    printf 'index = "sparse+https://git.terraphim.cloud/api/packages/terraphim/cargo/"\n'
} >>"$cargo_home/config.toml"

printf '%s\n' "Configured the terraphim registry in ${cargo_home}"

# terraphim-ai owns terraphim_lsp and the terraphim-lsp binary.
case "${CRATE}" in
    terraphim_lsp | terraphim-lsp)
        printf '%s\n' 'ERROR: terraphim-ai owns terraphim_lsp and the terraphim-lsp binary' >&2
        exit 1
        ;;
esac

# The crate directory, without allowing arbitrary paths.
crate_dir="crates/${CRATE}"
if [ ! -d "${crate_dir}" ]; then
    printf '%s\n' "ERROR: crate directory ${crate_dir} not found" >&2
    exit 1
fi

# The pre-publish gate (terraphim-clients#516): the only thing standing
# between a broken package and the registry. It packages and installs
# terraphim_agent against the canonical registry, so it only applies to that
# crate.
if [ "${CRATE}" = "terraphim_agent" ]; then
    printf '%s\n' 'Running the pre-publish package gate'
    bash ./scripts/release/verify-agent-package.sh
fi

if [ "${PUBLISH_DRY_RUN:-0}" = "1" ]; then
    printf '%s\n' "DRY RUN: would publish ${CRATE} from ${crate_dir}"
    exit 0
fi

printf '%s\n' "Publishing ${CRATE} from ${crate_dir}"
cargo publish -p "${CRATE}" --registry terraphim
