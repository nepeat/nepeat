#!/usr/bin/env bash
# One controlled yacht root attempt. Never loops the exploit across a reboot.
set -u
set -o pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 /path/to/exploit_trona.yacht" >&2
    exit 2
fi

EXPLOIT=$1
REMOTE=/data/local/tmp/exploit_trona_yacht
REMOTE_LOG=/data/local/tmp/exploit_trona_yacht.log
REMOTE_PID=/data/local/tmp/exploit_trona_yacht.pid
LOG_DIR=devices/amzn-pinnacles/twrp/out/root-attempts
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
HOST_LOG=$LOG_DIR/$STAMP.log

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    else
        shasum -a 256 "$1"
    fi
}

[[ -f "$EXPLOIT" ]] || {
    echo "missing exploit: $EXPLOIT" >&2
    exit 1
}
command -v adb >/dev/null || {
    echo "adb is not in PATH; enter the repository Nix shell" >&2
    exit 1
}
[[ "$(adb get-state 2>/dev/null)" == device ]] || {
    echo "no authorized ADB device" >&2
    exit 1
}

mkdir -p "$LOG_DIR"
BOOT_BEFORE=$(adb shell cat /proc/sys/kernel/random/boot_id | tr -d '\r')
ENFORCE_BEFORE=$(adb shell getenforce | tr -d '\r')
{
    echo "utc=$STAMP"
    echo "boot_before=$BOOT_BEFORE"
    echo "enforce_before=$ENFORCE_BEFORE"
    sha256_file "$EXPLOIT"
} > "$HOST_LOG"

if [[ "$ENFORCE_BEFORE" == Permissive ]]; then
    echo "device is already permissive; refusing to spend another exploit attempt" | tee -a "$HOST_LOG"
    exit 0
fi

adb push "$EXPLOIT" "$REMOTE" | tee -a "$HOST_LOG"
adb shell chmod 0755 "$REMOTE"

echo "running probe-only startup (no JIT allocation)" | tee -a "$HOST_LOG"
if ! adb shell "$REMOTE 0x40080000 probe" 2>&1 | tee -a "$HOST_LOG"; then
    echo "probe failed; exploit was not started" | tee -a "$HOST_LOG" >&2
    exit 1
fi

adb shell "rm -f /data/local/tmp/pwned2 '$REMOTE_LOG' '$REMOTE_PID'"
adb shell "nohup '$REMOTE' 0x40080000 root >'$REMOTE_LOG' 2>&1 & echo \$! >'$REMOTE_PID'"
ATTEMPT_PID=$(adb shell cat "$REMOTE_PID" | tr -d '\r')
echo "started exactly one root attempt: pid=$ATTEMPT_PID" | tee -a "$HOST_LOG"

RESULT=timeout
for ((poll = 0; poll < 180; poll++)); do
    if [[ "$(adb get-state 2>/dev/null)" != device ]]; then
        RESULT=disconnected
        break
    fi
    if adb shell test -f /data/local/tmp/pwned2; then
        RESULT=root
        break
    fi
    if ! adb shell "kill -0 '$ATTEMPT_PID'" >/dev/null 2>&1; then
        RESULT=exited
        break
    fi
    sleep 2
done

echo "result=$RESULT" | tee -a "$HOST_LOG"
if [[ "$RESULT" != disconnected ]]; then
    adb shell cat "$REMOTE_LOG" >> "$HOST_LOG" 2>&1 || true
    BOOT_AFTER=$(adb shell cat /proc/sys/kernel/random/boot_id | tr -d '\r')
    echo "boot_after=$BOOT_AFTER" | tee -a "$HOST_LOG"
    adb shell getenforce | tee -a "$HOST_LOG"
    adb shell cat /data/local/tmp/pwned2 2>/dev/null | tee -a "$HOST_LOG" || true
fi

echo "host log: $HOST_LOG"
case "$RESULT" in
    root) exit 0 ;;
    disconnected) exit 75 ;;
    *) exit 1 ;;
esac
