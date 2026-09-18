#!/usr/bin/env bash
# Privileged operator handoff for an already sealed release artifact.
# Never invoke this script from release-binaries.yml.
set -euo pipefail

if [ "$#" -ne 5 ]; then
  echo "usage: $0 VERSION STAGED_DIR TARGET_REPO EXPECTED_SOURCE_SHA CORRELATION_ID" >&2
  exit 2
fi

command -v python3 >/dev/null || {
  echo "ERROR: required command not found: python3" >&2
  exit 2
}
python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 9) else 1)' || {
  echo "ERROR: Python 3.9 or newer is required" >&2
  exit 2
}

version="$1"
staged_dir="$(cd "$2" 2>/dev/null && pwd -P)" || {
  echo "ERROR: staged directory does not exist: $2" >&2
  exit 2
}
target_repo="$3"
expected_source_sha="$4"
correlation_id="$5"
tag="v${version}"
repo="terraphim/${target_repo}"
base_url="${BASE_URL:-https://downloads.terraphim.ai}"
bucket="${R2_BUCKET:-terraphim-releases}"
r2_read_timeout="${R2_READ_TIMEOUT:-600}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"

[[ "$base_url" == https://* ]] || {
  echo "ERROR: BASE_URL must use HTTPS" >&2
  exit 2
}
[[ "$target_repo" =~ ^terraphim-(clients|ai)$ ]] || {
  echo "ERROR: unsupported target repository '$target_repo'" >&2
  exit 2
}
[[ "$r2_read_timeout" =~ ^[0-9]+$ ]] && [ "$r2_read_timeout" -ge 30 ] && [ "$r2_read_timeout" -le 3600 ] || {
  echo "ERROR: R2_READ_TIMEOUT must be an integer from 30 through 3600 seconds" >&2
  exit 2
}

for command_name in gh wrangler curl cmp; do
  command -v "$command_name" >/dev/null || {
    echo "ERROR: required command not found: $command_name" >&2
    exit 2
  }
done

verification_dir="$(mktemp -d)"
snapshot_tmp=""
cleanup() {
  rm -rf "$verification_dir"
  if [ -n "$snapshot_tmp" ] && [ -d "$snapshot_tmp" ]; then
    rm -rf "$snapshot_tmp"
  fi
}
trap cleanup EXIT

# This is intentionally the first substantive operation. It rejects missing,
# duplicate, mixed, or unauthorized provenance before any remote query/write.
"$script_dir/validate-promotion-stage.py" \
  "$version" "$staged_dir" "$expected_source_sha" "$correlation_id" \
  "$verification_dir/r2-objects.tsv"

release_state="$(gh release view "$tag" --repo "$repo" --json assets,isDraft,isPrerelease,tagName)"
RELEASE_STATE="$release_state" RELEASE_TAG="$tag" python3 - "$verification_dir/github-assets" <<'PY'
import json
import os
import pathlib
import sys

state = json.loads(os.environ["RELEASE_STATE"])
required = {"isDraft", "isPrerelease", "tagName"}
if not required <= set(state) or not set(state) <= required | {"assets"}:
    sys.exit("release state response has unexpected keys")
if state["tagName"] != os.environ["RELEASE_TAG"]:
    sys.exit("release tag does not match promotion version")
if state["isDraft"] or state["isPrerelease"]:
    sys.exit("draft or prerelease cannot advance stable manifests")
assets = state.get("assets", [])
if not isinstance(assets, list) or any(
    not isinstance(asset, dict) or not isinstance(asset.get("name"), str)
    for asset in assets
):
    sys.exit("release assets response has unexpected shape")
names = [asset["name"] for asset in assets]
if len(names) != len(set(names)):
    sys.exit("release contains duplicate asset names")
pathlib.Path(sys.argv[1]).write_text("".join(f"{name}\n" for name in sorted(names)))
PY

fetch_r2() {
  local object_path="$1"
  local destination="$2"
  local status curl_status
  rm -f "$destination"
  mkdir -p "$(dirname "$destination")"
  if status="$(curl --silent --show-error --connect-timeout 15 \
    --max-time "$r2_read_timeout" --max-redirs 0 \
    --output "$destination" --write-out '%{http_code}' \
    "$base_url/$object_path")"; then
    :
  else
    curl_status=$?
    rm -f "$destination"
    echo "ERROR: R2 read transport failure for $object_path (curl $curl_status)" >&2
    exit 1
  fi
  if [[ ! "$status" =~ ^[0-9]{3}$ ]]; then
    rm -f "$destination"
    echo "ERROR: malformed HTTP status for $object_path: '$status'" >&2
    exit 1
  fi
  case "$status" in
    200) return 0 ;;
    404) rm -f "$destination"; return 1 ;;
    *)
      rm -f "$destination"
      echo "ERROR: R2 read for $object_path returned HTTP $status; only 404 means absent" >&2
      exit 1
      ;;
  esac
}

github_plan="$verification_dir/github-upload.tsv"
r2_plan="$verification_dir/r2-upload.tsv"
: > "$github_plan"
: > "$r2_plan"
mapfile -t assets < <(find "$staged_dir/release-assets" -maxdepth 1 -type f -print | LC_ALL=C sort)

# Complete global preflight: compare every existing immutable on both surfaces
# before performing even one upload.
for local_asset in "${assets[@]}" "$staged_dir/SHA256SUMS"; do
  name="$(basename "$local_asset")"
  if grep -Fqx -- "$name" "$verification_dir/github-assets"; then
    mkdir -p "$verification_dir/github/$name.dir"
    gh release download "$tag" --repo "$repo" --pattern "$name" --dir "$verification_dir/github/$name.dir"
    downloaded="$verification_dir/github/$name.dir/$name"
    cmp "$local_asset" "$downloaded" || {
      echo "ERROR: immutable GitHub asset differs: $name" >&2
      exit 1
    }
    rm -f "$downloaded"
    rmdir "$verification_dir/github/$name.dir"
  else
    printf '%s\n' "$local_asset" >> "$github_plan"
  fi
done

while IFS=$'\t' read -r object_path local_path; do
  remote_path="$verification_dir/r2-preflight/${object_path//\//_}"
  if fetch_r2 "$object_path" "$remote_path"; then
    cmp "$local_path" "$remote_path" || {
      echo "ERROR: immutable R2 object differs: $object_path" >&2
      exit 1
    }
    rm -f "$remote_path"
  else
    printf '%s\t%s\n' "$object_path" "$local_path" >> "$r2_plan"
  fi
done < "$verification_dir/r2-objects.tsv"

# GitHub rejects a newly appeared same-name asset because --clobber is never
# used. Each successful upload is downloaded and compared byte-for-byte.
while IFS= read -r local_asset; do
  [ -n "$local_asset" ] || continue
  name="$(basename "$local_asset")"
  gh release upload "$tag" "$local_asset" --repo "$repo"
  mkdir -p "$verification_dir/github-readback/$name.dir"
  gh release download "$tag" --repo "$repo" --pattern "$name" --dir "$verification_dir/github-readback/$name.dir"
  downloaded="$verification_dir/github-readback/$name.dir/$name"
  cmp "$local_asset" "$downloaded"
  rm -f "$downloaded"
  rmdir "$verification_dir/github-readback/$name.dir"
done < "$github_plan"

# Wrangler does not expose an atomic if-none-match put for this command. Re-read
# immediately before each put, skip an identical race winner, and fail on a
# differing winner. A sub-request race between the final 404 and put remains a
# documented provider limitation; every put is nevertheless read back exactly.
while IFS=$'\t' read -r object_path local_path; do
  [ -n "$object_path" ] || continue
  immediate="$verification_dir/r2-immediate/${object_path//\//_}"
  if fetch_r2 "$object_path" "$immediate"; then
    cmp "$local_path" "$immediate" || {
      echo "ERROR: immutable R2 object appeared with different bytes: $object_path" >&2
      exit 1
    }
    rm -f "$immediate"
    continue
  fi
  wrangler r2 object put "$bucket/$object_path" --file "$local_path" --remote
  readback="$verification_dir/r2-readback/${object_path//\//_}"
  fetch_r2 "$object_path" "$readback" || {
    echo "ERROR: uploaded R2 object is absent: $object_path" >&2
    exit 1
  }
  cmp "$local_path" "$readback" || {
    echo "ERROR: immutable R2 readback differs: $object_path" >&2
    exit 1
  }
  rm -f "$readback"
done < "$r2_plan"

snapshot_dir="$staged_dir/rollback-pointers"
snapshot_plan="$verification_dir/pointer-snapshot.tsv"
if [ -e "$snapshot_dir" ]; then
  "$script_dir/release-pointer-snapshot.py" validate \
    "$version" "$expected_source_sha" "$correlation_id" "$snapshot_dir" "$snapshot_plan"
else
  snapshot_tmp="$(mktemp -d "$staged_dir/.rollback-pointers.tmp.XXXXXX")"
  : > "$snapshot_plan"
  for binary in terraphim-agent terraphim-cli terraphim-grep; do
    for pointer in stable.json stable-v2.json; do
      object_path="$binary/$pointer"
      retained="$snapshot_tmp/objects/$object_path"
      if fetch_r2 "$object_path" "$retained"; then
        printf '%s\tpresent\t%s\n' "$object_path" "objects/$object_path" >> "$snapshot_plan"
      else
        printf '%s\tabsent\t-\n' "$object_path" >> "$snapshot_plan"
      fi
    done
  done
  "$script_dir/release-pointer-snapshot.py" create \
    "$version" "$expected_source_sha" "$correlation_id" "$snapshot_tmp" "$snapshot_plan"
  mv "$snapshot_tmp" "$snapshot_dir"
  snapshot_tmp=""
fi

advance_pointer() {
  local object_path="$1"
  local local_path="$2"
  local existing="$verification_dir/pointer-${object_path//\//_}"
  if fetch_r2 "$object_path" "$existing"; then
    if cmp -s "$local_path" "$existing"; then
      rm -f "$existing"
      return 0
    fi
    rm -f "$existing"
  fi
  wrangler r2 object put "$bucket/$object_path" --file "$local_path" --content-type application/json --remote
  fetch_r2 "$object_path" "$existing" || {
    echo "ERROR: stable pointer readback is absent: $object_path" >&2
    exit 1
  }
  cmp "$local_path" "$existing" || {
    echo "ERROR: stable pointer readback differs: $object_path" >&2
    exit 1
  }
  rm -f "$existing"
}

# Forward migration order is deliberate: strict v2 first and legacy last.
# The retained pre-promotion snapshot is complete before either loop starts.
for binary in terraphim-agent terraphim-cli terraphim-grep; do
  advance_pointer "$binary/stable-v2.json" "$staged_dir/manifests/$binary.v2.candidate.json"
done
for binary in terraphim-agent terraphim-cli terraphim-grep; do
  advance_pointer "$binary/stable.json" "$staged_dir/manifests/$binary.v1.candidate.json"
done

echo "Stable v2 and legacy manifests advanced to $version from the authorized sealed stage."
