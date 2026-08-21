#!/usr/bin/env bash
# Dump everything interesting off the Amazon "yacht"/KFYAWI once adb is authorized.
# Read-only: no writes, no wipe, no OTA. Run from inside the ai_re devshell.
set -uo pipefail

OUT="$(cd "$(dirname "$0")" && pwd)/dumps"
mkdir -p "$OUT"

run() { # run <outfile> <adb args...>
  local f="$OUT/$1"; shift
  echo "==> $f : adb $*"
  { echo "\$ adb $*"; adb "$@" 2>&1; } >"$f"
}

state=$(adb get-state 2>&1 || true)
if [ "$state" != "device" ]; then
  echo "device not ready (state: $state) — unlock screen and accept the USB debugging prompt" >&2
  exit 1
fi

# --- identity: the unblocking facts ---
run getprop.txt            shell getprop
run build.prop.txt         shell cat /system/build.prop
run cpuinfo.txt            shell cat /proc/cpuinfo
run meminfo.txt            shell cat /proc/meminfo
run cmdline.txt            shell cat /proc/cmdline
run version.txt            shell cat /proc/version

# --- what the device was FOR ---
run packages-system.txt    shell pm list packages -s
run packages-3rd.txt       shell pm list packages -3
run packages-all-f.txt     shell pm list packages -f
run features.txt           shell pm list features

# --- enterprise / MDM enrollment ---
run device_policy.txt      shell dumpsys device_policy
run users.txt              shell pm list users
run settings-global.txt    shell settings list global
run settings-secure.txt    shell settings list secure
run accounts.txt           shell dumpsys account

# --- hardware reality check ---
run telephony.txt          shell dumpsys telephony.registry
run power_supply.txt       shell 'cat /sys/class/power_supply/*/uevent'
run partitions.txt         shell cat /proc/partitions
run mounts.txt             shell cat /proc/mounts
run by-name.txt            shell 'ls -l /dev/block/by-name/ /dev/block/platform/*/by-name/ 2>&1'
run input-devices.txt      shell cat /proc/bus/input/devices
run media-camera.txt       shell dumpsys media.camera
run surfaceflinger.txt     shell 'dumpsys SurfaceFlinger | head -50'

# --- logs / misc ---
run kmsg.txt               shell 'cat /proc/kmsg & sleep 3; kill %1'
run logcat-d.txt           logcat -d -v time
run bootloader-props.txt   shell 'getprop | grep -i -E "boot|verified|unlock|secure"'

echo
echo "collected into $OUT"
