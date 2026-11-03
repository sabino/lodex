#!/bin/sh
# Read-only BusyBox-compatible sampling. Never collect prompts, env, or file names.
set -eu
pid=${1:?usage: sample-proc.sh PID [COUNT [INTERVAL_SECONDS]]}
count=${2:-3}
interval=${3:-1}
case "$pid:$count:$interval" in *[!0-9:]*|:*|*::*|*:) exit 2;; esac
[ "$pid" -gt 0 ] && [ "$count" -gt 0 ] && [ "$count" -le 3600 ] || exit 2
i=0
while [ "$i" -lt "$count" ]; do
    [ -r "/proc/$pid/stat" ] || { echo 'sample-error process-unavailable' >&2; exit 1; }
    printf 'sample %s\n' "$(date +%s)"
    printf 'stat '
    cat "/proc/$pid/stat"
    awk '/^(VmRSS|VmHWM|VmSwap|Threads):/ { print "status " $1 " " $2 }' "/proc/$pid/status"
    printf 'end\n'
    i=$((i + 1))
    [ "$i" -ge "$count" ] || sleep "$interval"
done
