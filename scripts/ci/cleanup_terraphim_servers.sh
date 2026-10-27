#!/usr/bin/env bash
set -u

checkout=$(readlink -f "$PWD") || exit 1
pids=()

for proc_dir in /proc/[0-9]*; do
    pid=${proc_dir#/proc/}
    exe=$(readlink -f "$proc_dir/exe" 2>/dev/null) || continue
    cwd=$(readlink -f "$proc_dir/cwd" 2>/dev/null) || continue
    test "${exe##*/}" = terraphim_server || continue
    case "$cwd" in
        "$checkout"|"$checkout"/*) pids+=("$pid") ;;
    esac
done

for pid in "${pids[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
done

sleep 2

for pid in "${pids[@]}"; do
    proc_dir="/proc/$pid"
    test -d "$proc_dir" || continue
    exe=$(readlink -f "$proc_dir/exe" 2>/dev/null) || continue
    cwd=$(readlink -f "$proc_dir/cwd" 2>/dev/null) || continue
    test "${exe##*/}" = terraphim_server || continue
    case "$cwd" in
        "$checkout"|"$checkout"/*) kill -KILL "$pid" 2>/dev/null || true ;;
    esac
done
