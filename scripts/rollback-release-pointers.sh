#!/usr/bin/env bash
# Separately authorized, pointers-only rollback for the stable-v2 migration.
set -euo pipefail

if [ "$#" -ne 5 ] || [ "$5" != "--authorized-pointers-only" ]; then
  echo "usage: $0 VERSION STAGED_DIR EXPECTED_SOURCE_SHA CORRELATION_ID --authorized-pointers-only" >&2
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
expected_source_sha="$3"
correlation_id="$4"
base_url="${BASE_URL:-https://downloads.terraphim.ai}"
bucket="${R2_BUCKET:-terraphim-releases}"
r2_read_timeout="${R2_READ_TIMEOUT:-600}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"

[[ "$base_url" == https://* ]] || { echo "ERROR: BASE_URL must use HTTPS" >&2; exit 2; }
[[ "$r2_read_timeout" =~ ^[0-9]+$ ]] && [ "$r2_read_timeout" -ge 30 ] && [ "$r2_read_timeout" -le 3600 ] || {
  echo "ERROR: R2_READ_TIMEOUT must be an integer from 30 through 3600 seconds" >&2
  exit 2
}
for command_name in wrangler curl cmp; do
  command -v "$command_name" >/dev/null || { echo "ERROR: required command not found: $command_name" >&2; exit 2; }
done

verification_dir="$(mktemp -d)"
trap 'rm -rf "$verification_dir"' EXIT

# Revalidate the original sealed stage and its retained pre-promotion state
# before the first remote query or pointer mutation.
"$script_dir/validate-promotion-stage.py" \
  "$version" "$staged_dir" "$expected_source_sha" "$correlation_id" \
  "$verification_dir/immutable-plan.tsv"
"$script_dir/release-pointer-snapshot.py" validate \
  "$version" "$expected_source_sha" "$correlation_id" \
  "$staged_dir/rollback-pointers" "$verification_dir/pointer-plan.tsv"

if awk -F '\t' '$1 ~ /stable-v2\.json$/ && $2 != "absent" { found=1 } END { exit found ? 0 : 1 }' \
  "$verification_dir/pointer-plan.tsv"; then
  echo "ERROR: this migration rollback requires stable-v2.json to have been absent before promotion" >&2
  exit 1
fi

fetch_r2() {
  local object_path="$1" destination="$2" status curl_status
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
  [[ "$status" =~ ^[0-9]{3}$ ]] || {
    rm -f "$destination"
    echo "ERROR: malformed HTTP status for $object_path: '$status'" >&2
    exit 1
  }
  case "$status" in
    200) return 0 ;;
    404) rm -f "$destination"; return 1 ;;
    *) rm -f "$destination"; echo "ERROR: R2 read for $object_path returned HTTP $status" >&2; exit 1 ;;
  esac
}

classify_live_pointer() {
  local object_path="$1" pointer_kind="$2" retained_state="$3"
  local retained="$4" candidate="$5" destination="$6"
  live_classification=""
  if fetch_r2 "$object_path" "$destination"; then
    if cmp -s "$candidate" "$destination"; then
      live_classification="promoted"
      rm -f "$destination"
      return 0
    fi
    if [ "$pointer_kind" = "legacy" ] && [ "$retained_state" = "present" ] && \
      cmp -s "$retained" "$destination"; then
      live_classification="restored"
      rm -f "$destination"
      return 0
    fi
    echo "ERROR: live pointer was not written by this promotion: $object_path" >&2
    exit 1
  fi
  if [ "$pointer_kind" = "strict" ] || [ "$retained_state" = "absent" ]; then
    live_classification="absent"
    return 0
  fi
  echo "ERROR: live pointer was not written by this promotion: $object_path (unexpectedly absent)" >&2
  exit 1
}

candidate_for_pointer() {
  local object_path="$1" pointer_kind="$2"
  local binary="${object_path%%/*}"
  if [ "$pointer_kind" = "legacy" ]; then
    printf '%s\n' "$staged_dir/manifests/$binary.v1.candidate.json"
  else
    printf '%s\n' "$staged_dir/manifests/$binary.v2.candidate.json"
  fi
}

# Classify all six live pointers before the first mutation. A stale or foreign
# pointer aborts the entire rollback rather than leaving a partial rollback.
while IFS=$'\t' read -r object_path state retained; do
  if [[ "$object_path" == */stable.json ]]; then
    pointer_kind="legacy"
  else
    pointer_kind="strict"
  fi
  candidate="$(candidate_for_pointer "$object_path" "$pointer_kind")"
  classify_live_pointer "$object_path" "$pointer_kind" "$state" "$retained" "$candidate" \
    "$verification_dir/live-preflight-${object_path//\//_}"
done < "$verification_dir/pointer-plan.tsv"

restore_pointer() {
  local object_path="$1" retained_state="$2" retained="$3" candidate="$4"
  local existing="$verification_dir/restore-${object_path//\//_}"
  classify_live_pointer "$object_path" legacy "$retained_state" "$retained" "$candidate" "$existing"
  if [ "$live_classification" = "restored" ]; then
    return 0
  fi
  [ "$live_classification" = "promoted" ] || {
    echo "ERROR: live pointer was not written by this promotion: $object_path" >&2
    exit 1
  }
  wrangler r2 object put "$bucket/$object_path" --file "$retained" --content-type application/json --remote
  fetch_r2 "$object_path" "$existing" || {
    echo "ERROR: restored pointer is absent: $object_path" >&2
    exit 1
  }
  cmp "$retained" "$existing" || {
    echo "ERROR: restored pointer readback differs: $object_path" >&2
    exit 1
  }
  rm -f "$existing"
}

delete_pointer() {
  local object_path="$1" pointer_kind="$2" retained_state="$3" retained="$4" candidate="$5"
  local existing="$verification_dir/delete-${object_path//\//_}"
  classify_live_pointer "$object_path" "$pointer_kind" "$retained_state" "$retained" "$candidate" "$existing"
  if [ "$live_classification" = "absent" ]; then
    return 0
  fi
  [ "$live_classification" = "promoted" ] || {
    echo "ERROR: live pointer was not written by this promotion: $object_path" >&2
    exit 1
  }
  wrangler r2 object delete "$bucket/$object_path" --remote
  if fetch_r2 "$object_path" "$existing"; then
    echo "ERROR: deleted pointer remains present: $object_path" >&2
    exit 1
  fi
}

# Restore legacy first. While strict v2 still exists, new clients remain on the
# promoted release and old clients move back. Then delete v2 so new clients use
# the existing GitHub fallback. Both transitions are health-valid below activation.
while IFS=$'\t' read -r object_path state retained; do
  [[ "$object_path" == */stable.json ]] || continue
  candidate="$(candidate_for_pointer "$object_path" legacy)"
  if [ "$state" = "present" ]; then
    restore_pointer "$object_path" "$state" "$retained" "$candidate"
  else
    delete_pointer "$object_path" legacy "$state" "$retained" "$candidate"
  fi
done < "$verification_dir/pointer-plan.tsv"

while IFS=$'\t' read -r object_path state _retained; do
  [[ "$object_path" == */stable-v2.json ]] || continue
  [ "$state" = "absent" ] || { echo "ERROR: unexpected retained v2 state" >&2; exit 1; }
  candidate="$(candidate_for_pointer "$object_path" strict)"
  delete_pointer "$object_path" strict "$state" "$_retained" "$candidate"
done < "$verification_dir/pointer-plan.tsv"

echo "Legacy stable pointers restored and strict v2 pointers removed; new clients fall back to GitHub."
