#!/system/bin/sh
# Launch the built TWRP userspace without changing the boot chain or exposing
# any internal partitions. Must run as root on a live, SELinux-permissive boot.
set -eu

MODE="${1:-launch}"
TWRP_ROOT=/mnt/twrp-root
RAMDISK=/data/local/tmp/twrp-ramdisk.gz
LOG=/data/local/tmp/twrp-userspace.txt
PIDFILE=/data/local/tmp/twrp-userspace.pid
ANDROID_PIDFILE=/data/local/tmp/twrp-android-pids
TIMEOUT="${TWRP_TIMEOUT:-90}"

is_mounted() {
    grep -q " $1 " /proc/mounts
}

cleanup() {
    if [ -f "$PIDFILE" ]; then
        kill "$(cat "$PIDFILE")" 2>/dev/null || true
        rm -f "$PIDFILE"
    fi

    if [ -f "$ANDROID_PIDFILE" ]; then
        for android_pid in $(cat "$ANDROID_PIDFILE"); do
            kill -CONT "$android_pid" 2>/dev/null || true
        done
        rm -f "$ANDROID_PIDFILE"
    fi

    umount "$TWRP_ROOT/tmp" 2>/dev/null || true
    umount "$TWRP_ROOT/proc" 2>/dev/null || true
    umount "$TWRP_ROOT/sys" 2>/dev/null || true
    umount "$TWRP_ROOT/dev" 2>/dev/null || true
    umount "$TWRP_ROOT" 2>/dev/null || true
}

prepare() {
    [ "$(id -u)" = 0 ] || {
        echo "must run as root" >&2
        exit 1
    }
    [ -r "$RAMDISK" ] || {
        echo "missing $RAMDISK" >&2
        exit 1
    }

    mkdir -p "$TWRP_ROOT"
    if ! is_mounted "$TWRP_ROOT"; then
        mount -t tmpfs -o size=128m,mode=0755 twrp-smoke "$TWRP_ROOT"
    fi

    cd "$TWRP_ROOT"
    gzip -dc "$RAMDISK" | cpio -idm
    [ -x sbin/recovery ] || {
        echo "ramdisk has no executable sbin/recovery" >&2
        exit 1
    }

    # Deliberately hide every internal partition for the first UI-only test.
    # The built, full fstab remains in the image and is validated separately.
    cp etc/recovery.fstab etc/recovery.fstab.full
    cp system/etc/twrp.fstab system/etc/twrp.fstab.full
    printf '%s\n' '# yacht TWRP userspace smoke test: no block devices exposed' \
        > etc/recovery.fstab
    printf '%s\n' '# yacht TWRP userspace smoke test: no block devices exposed' \
        > system/etc/twrp.fstab

    mkdir -p dev proc sys tmp
    is_mounted "$TWRP_ROOT/dev" || mount -o bind /dev "$TWRP_ROOT/dev"
    is_mounted "$TWRP_ROOT/proc" || mount -o bind /proc "$TWRP_ROOT/proc"
    is_mounted "$TWRP_ROOT/sys" || mount -o bind /sys "$TWRP_ROOT/sys"
    is_mounted "$TWRP_ROOT/tmp" || mount -t tmpfs -o size=32m,mode=0775 twrp-tmp "$TWRP_ROOT/tmp"
}

case "$MODE" in
    prepare)
        prepare
        ;;
    cleanup)
        cleanup
        ;;
    launch)
        cleanup
        prepare
        : > "$LOG"

        android_pids="$(pidof system_server) $(pidof surfaceflinger)"
        printf '%s\n' "$android_pids" > "$ANDROID_PIDFILE"

        # Recovery takes over the framebuffer, which makes Android's
        # system_server watchdog reboot yacht. Arm rollback before suspending
        # either Android process; the independent watchdogd keeps running.
        (
            sleep "$TIMEOUT"
            if [ -f "$PIDFILE" ]; then
                kill "$(cat "$PIDFILE")" 2>/dev/null || true
            fi
            for android_pid in $android_pids; do
                kill -CONT "$android_pid" 2>/dev/null || true
            done
        ) &
        safety_pid=$!

        for android_pid in $android_pids; do
            kill -STOP "$android_pid"
        done

        chroot "$TWRP_ROOT" /sbin/recovery >>"$LOG" 2>&1 &
        recovery_pid=$!
        printf '%s\n' "$recovery_pid" > "$PIDFILE"

        # On an early recovery exit, cancel the timer and restore Android now.
        (
            while kill -0 "$recovery_pid" 2>/dev/null; do sleep 1; done
            kill "$safety_pid" 2>/dev/null || true
            for android_pid in $android_pids; do
                kill -CONT "$android_pid" 2>/dev/null || true
            done
        ) &

        echo "TWRP smoke test pid=$recovery_pid timeout=${TIMEOUT}s log=$LOG"
        ;;
    *)
        echo "usage: $0 [prepare|launch|cleanup]" >&2
        exit 2
        ;;
esac
