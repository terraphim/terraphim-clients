#!/usr/bin/env bash
# Resolve both published and draft GitHub releases by their exact tag.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "Usage: GITHUB_REPOSITORY=owner/repo $0 <tag>" >&2
    exit 2
fi

repository="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
tag="$1"

# GitHub's GET /releases/tags/{tag} endpoint excludes draft releases. Listing
# releases is the documented authenticated path that includes drafts; slurp
# keeps pagination correct for repositories with more than 100 releases.
gh api --paginate --slurp "repos/$repository/releases?per_page=100" \
    | jq -cer --arg tag "$tag" '
        [.[][] | select(.tag_name == $tag)]
        | if length == 1 then .[0]
          elif length == 0 then error("release tag not found: " + $tag)
          else error("duplicate release tag: " + $tag)
          end
    '
