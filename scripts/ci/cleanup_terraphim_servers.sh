#!/usr/bin/env bash
set -u

checkout=$(readlink -f "$PWD") || exit 1
pids=()
exes=()
cwds=()
starts=()

process_start_time() {
    local stat rest
    stat=$(<"$1/stat") || return 1
    rest=${stat##*) }
    set -- $rest
    test "$#" -ge 20 || return 1
    printf '%s\n' "${20}"
}

process_matches_snapshot() {
    local pid=$1 expected_exe=$2 expected_cwd=$3 expected_start=$4
    local proc_dir="/proc/$pid" exe cwd start

    exe=$(readlink -f "$proc_dir/exe" 2>/dev/null) || return 1
    cwd=$(readlink -f "$proc_dir/cwd" 2>/dev/null) || return 1
    start=$(process_start_time "$proc_dir") || return 1
    test "$exe" = "$expected_exe" &&
        test "$cwd" = "$expected_cwd" &&
        test "$start" = "$expected_start"
}

for proc_dir in /proc/[0-9]*; do
    pid=${proc_dir#/proc/}
    exe=$(readlink -f "$proc_dir/exe" 2>/dev/null) || continue
    cwd=$(readlink -f "$proc_dir/cwd" 2>/dev/null) || continue
    start=$(process_start_time "$proc_dir") || continue
    test "${exe##*/}" = terraphim_server || continue
    case "$cwd" in
        "$checkout"|"$checkout"/*)
            pids+=("$pid")
            exes+=("$exe")
            cwds+=("$cwd")
            starts+=("$start")
            ;;
    esac
done

# Integration tests use this barrier to replace a scanned process before the
# signal pass. Production CI leaves it unset.
if test -n "${TERRAPHIM_CLEANUP_TEST_BARRIER:-}"; then
    : > "${TERRAPHIM_CLEANUP_TEST_BARRIER}.scanned"
    while ! test -e "${TERRAPHIM_CLEANUP_TEST_BARRIER}.continue"; do
        sleep 0.01
    done
fi

for index in "${!pids[@]}"; do
    if process_matches_snapshot "${pids[$index]}" "${exes[$index]}" "${cwds[$index]}" "${starts[$index]}"; then
        kill -TERM "${pids[$index]}" 2>/dev/null || true
    fi
done

sleep 2

for index in "${!pids[@]}"; do
    if process_matches_snapshot "${pids[$index]}" "${exes[$index]}" "${cwds[$index]}" "${starts[$index]}"; then
        kill -KILL "${pids[$index]}" 2>/dev/null || true
    fi
done
