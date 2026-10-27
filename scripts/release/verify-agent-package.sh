#!/usr/bin/env bash
# Release-only packaged install verification for terraphim_agent.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

if [[ -z "${CARGO_REGISTRIES_TERRAPHIM_TOKEN:-}" ]]; then
    printf '%s\n' 'error: CARGO_REGISTRIES_TERRAPHIM_TOKEN is required; after sourcing ~/.profile set it to "Bearer $GITEA_TOKEN"' >&2
    exit 2
fi

printf '%s\n' 'Verifying the terraphim_agent package against the canonical registry.'
printf '%s\n' 'A missing workspace dependency version is a release-blocking failure.'
exec cargo test -p terraphim_agent --test agent_package_release_gate -- --ignored --nocapture
