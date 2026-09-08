#!/usr/bin/env bash
# Build a first-boot TWRP ramdisk that exposes no block devices.
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: [YACHT_PROFILE=smoke|gui] $0 INPUT_RAMDISK.gz OUTPUT_RAMDISK.gz" >&2
    exit 2
fi

# smoke: the validated v26 headless payload -- byte-frozen, do not change.
# gui:   adds display/input diagnostics and a manual TWRP launcher for the
#        display-enabled boot. TWRP is still never started automatically.
profile="${YACHT_PROFILE:-smoke}"
hold="${YACHT_HOLD_SECONDS:-}"
case "$profile" in
    smoke) hold="${hold:-20}" ;;
    gui)   hold="${hold:-300}" ;;
    *) echo "unknown YACHT_PROFILE=$profile" >&2; exit 2 ;;
esac

input=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
output=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
work=$(mktemp -d "${TMPDIR:-/tmp}/yacht-smoke-ramdisk.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
normalizer=$(cd "$(dirname "$0")" && pwd)/normalize-newc.py
mt8183_overlay=$(cd "$(dirname "$0")" && pwd)/init.recovery.mt8183.smoke.rc

[[ -r "$input" ]] || { echo "cannot read $input" >&2; exit 1; }
[[ -r "$mt8183_overlay" ]] || { echo "cannot read $mt8183_overlay" >&2; exit 1; }
[[ ! -e "$output" ]] || { echo "refusing to overwrite $output" >&2; exit 1; }

mkdir "$work/root"
(
    cd "$work/root"
    gzip -dc "$input" | cpio -idm --quiet
)

for fstab in \
    "$work/root/etc/recovery.fstab" \
    "$work/root/system/etc/twrp.fstab"
do
    [[ -f "$fstab" ]] || { echo "missing expected $fstab" >&2; exit 1; }
    printf '%s\n' '# yacht first-boot smoke test: no block devices exposed' >"$fstab"
    chmod 0644 "$fstab"
    touch -t 197001010000 "$fstab"
done

# TWRP's generic recovery USB rules target the legacy android_usb sysfs API,
# while yacht's kernel uses configfs. Import the minimal stock-derived MT8183
# configfs setup, with its eMMC boot-region force_ro mutation intentionally
# omitted from the smoke payload.
cp "$mt8183_overlay" "$work/root/init.recovery.mt8183.rc"
chmod 0644 "$work/root/init.recovery.mt8183.rc"

# The stock MT8183 recovery overlay defaults the configfs gadget to an inert
# HID function.  Start the already-defined recovery adbd service explicitly;
# its FunctionFS-ready property makes that overlay switch the gadget to adb.
printf '%s\n' \
    '' \
    '# yacht RAM-only recovery: start FunctionFS adb explicitly' \
    'on boot' \
    '    setprop service.adb.root 1' \
    '    start set_permissive' \
    '    start adbd' \
    '    start yacht_smoke' \
    '' \
    'service yacht_smoke /sbin/yacht-smoke.sh' \
    '    oneshot' \
    '    seclabel u:r:recovery:s0' >>"$work/root/init.rc"

# Leave a target-kernel marker, retry permissive mode/adbd from recovery's own
# domain, then reboot normally before the module's hardware watchdog can route
# through stock recovery and overwrite ramoops.
if [[ "$profile" == smoke ]]; then
printf '%s\n' \
    '#!/sbin/sh' \
    'echo "<6>yacht-smoke: target userspace entered" > /dev/kmsg' \
    'setenforce 0' \
    'echo "<6>yacht-smoke: setenforce rc=$? state=$(getenforce)" > /dev/kmsg' \
    'setprop service.adb.root 1' \
    'setprop ctl.start adbd' \
    "sleep $hold" \
    'echo "<6>yacht-smoke: requesting controlled normal reboot" > /dev/kmsg' \
    'setprop sys.powerctl reboot' \
    >"$work/root/sbin/yacht-smoke.sh"
else
# Diagnostics are emitted before the hold so that a hard reboot during the
# window still leaves the display verdict in ramoops.
printf '%s\n' \
    '#!/sbin/sh' \
    'R=/tmp/yacht-gui-probe.txt' \
    'log() {' \
    '    echo "<6>yacht-gui: $*" > /dev/kmsg' \
    '    echo "yacht-gui: $*" >> "$R" 2>/dev/null' \
    '    echo "yacht-gui: $*" > /dev/pmsg0 2>/dev/null' \
    '}' \
    'log "target userspace entered"' \
    'setenforce 0' \
    'log "setenforce rc=$? state=$(getenforce)"' \
    'setprop service.adb.root 1' \
    'setprop ctl.start adbd' \
    'for n in /dev/graphics/fb0 /dev/fb0; do' \
    '    if [ -e "$n" ]; then log "fbdev $n PRESENT"; else log "fbdev $n absent"; fi' \
    'done' \
    'for f in /sys/class/graphics/fb0/name \' \
    '         /sys/class/graphics/fb0/virtual_size \' \
    '         /sys/class/graphics/fb0/bits_per_pixel \' \
    '         /sys/class/graphics/fb0/stride \' \
    '         /sys/class/graphics/fb0/blank \' \
    '         /sys/class/graphics/fb0/modes; do' \
    '    if [ -r "$f" ]; then log "$f = $(cat "$f" 2>/dev/null | tr "\n" " ")"; fi' \
    'done' \
    'log "leds: $(ls /sys/class/leds 2>/dev/null | tr "\n" " ")"' \
    'log "backlight: $(ls /sys/class/backlight 2>/dev/null | tr "\n" " ")"' \
    'log "input: $(ls /dev/input 2>/dev/null | tr "\n" " ")"' \
    'log "diagnostics complete; TWRP NOT started -- run /sbin/twrp-start.sh"' \
    "log \"holding $hold s before controlled reboot\"" \
    "sleep $hold" \
    'log "requesting controlled normal reboot"' \
    'setprop sys.powerctl reboot' \
    >"$work/root/sbin/yacht-smoke.sh"

# Deliberately NOT referenced by any init .rc: task gate is a stable adb shell
# first, TWRP only afterwards and only by hand.
printf '%s\n' \
    '#!/sbin/sh' \
    '# Manual TWRP launcher. Run from an adb shell, never from init.' \
    'echo "<6>yacht-gui: manual TWRP start requested" > /dev/kmsg' \
    'exec /sbin/recovery' \
    >"$work/root/sbin/twrp-start.sh"
chmod 0750 "$work/root/sbin/twrp-start.sh"
fi
chmod 0750 "$work/root/sbin/yacht-smoke.sh"

# TWRP exits immediately when the deliberately empty fstabs expose no storage,
# and Android init turns that exit into a reboot.  Keep PID 1 alive instead so
# the first RAM-only adb session is stable; TWRP can then be launched manually.
printf '%s\n' \
    'on boot' \
    '' \
    'service recovery /sbin/sleep 3600' \
    '    seclabel u:r:recovery:s0' \
    >"$work/root/init.recovery.service.rc"

# Extraction mutates parent-directory mtimes. Normalize every archive entry so
# repeated builds are byte-identical; -h changes symlink timestamps themselves.
find "$work/root" -exec touch -h -t 197001010000 {} +

(
    cd "$work/root"
    LC_ALL=C find . -print | LC_ALL=C sort | \
        cpio -o --quiet --format newc --owner 0:0 | \
        python3 "$normalizer" | gzip -9n >"$work/result.gz"
)
mv "$work/result.gz" "$output"

# Prove the output still contains recovery and only inert fstabs.
gzip -t "$output"
listing=$(gzip -dc "$output" | cpio -it 2>/dev/null)
grep -qx '\./sbin/recovery' <<<"$listing"
mkdir "$work/check"
(
    cd "$work/check"
    gzip -dc "$output" | cpio -idm --quiet
)
for fstab in ./etc/recovery.fstab ./system/etc/twrp.fstab; do
    actual=$(<"$work/check/$fstab")
    [[ "$actual" == '# yacht first-boot smoke test: no block devices exposed' ]]
done

echo "created $output"
wc -c <"$output"
shasum -a 256 "$output"
