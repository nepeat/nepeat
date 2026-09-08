#!/usr/bin/env bash
# Submit one command to yacht's detached root worker.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 'command run as root'" >&2
    exit 2
fi

base=/data/local/tmp/yacht-rootd
token="$(date +%s)-$$-$RANDOM"

[[ "$(adb get-state 2>/dev/null)" == device ]] || {
    echo "no authorized ADB device" >&2
    exit 1
}
adb shell test -f "$base/ready" || {
    echo "root worker is not ready" >&2
    exit 1
}
adb shell test ! -f "$base/cmd" || {
    echo "root worker already has a queued command" >&2
    exit 1
}

{
    printf '%s\n' "$token"
    printf '%s\n' "$1"
} | adb shell "cat >$base/cmd.tmp"
adb shell "mv $base/cmd.tmp $base/cmd"

for ((poll = 0; poll < 600; poll++)); do
    status=$(adb shell "cat $base/status 2>/dev/null" | tr -d '\r' || true)
    if [[ "$status" == "$token "* ]]; then
        adb shell "cat $base/out 2>/dev/null" || true
        rc=${status##* }
        [[ "$rc" =~ ^[0-9]+$ ]] || exit 1
        exit "$rc"
    fi
    sleep 0.1
done

echo "root worker command timed out" >&2
exit 1
