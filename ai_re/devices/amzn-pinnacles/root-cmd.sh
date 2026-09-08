#!/usr/bin/env bash
# Run one command through yacht's already-installed modprobe usermode helper.
set -u
set -o pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 'command run as root'" >&2
    exit 2
fi

[[ "$(adb get-state 2>/dev/null)" == device ]] || {
    echo "no authorized ADB device" >&2
    exit 1
}
[[ "$(adb shell getenforce | tr -d '\r')" == Permissive ]] || {
    echo "device is not permissive; refusing stale root-channel use" >&2
    exit 1
}
adb shell test -x /data/local/tmp/trig || {
    echo "missing modprobe trigger" >&2
    exit 1
}

adb shell rm -f /data/local/tmp/out
printf '%s\n' "$1" | adb shell 'cat > /data/local/tmp/cmd'
adb shell /data/local/tmp/trig >/dev/null 2>&1 || true

for ((poll = 0; poll < 100; poll++)); do
    if adb shell test -f /data/local/tmp/out && ! adb shell test -f /data/local/tmp/cmd; then
        adb shell cat /data/local/tmp/out
        exit 0
    fi
    sleep 0.1
done

echo "root helper timed out" >&2
exit 1
