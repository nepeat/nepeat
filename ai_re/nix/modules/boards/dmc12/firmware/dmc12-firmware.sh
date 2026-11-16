#!@bash@/bin/bash
# dmc12-firmware: our U-Boot in the DMC-12's boot SPI flash, installed and managed from Linux
# (as whitebox vendors ship firmware updates). The factory U-Boot stays in UBOOT 0/1 untouched:
# stage3's `u` key (serial console, at power-on) always reaches it.
#
#   dmc12-firmware status                    what BOOT_MODE selects, what each IPXE slot holds
#   dmc12-firmware install [--yes]           our U-Boot into the inactive IPXE slot, then BOOT_MODE
#   dmc12-firmware switch --to SLOT [--yes]  SLOT = ipxe0 | ipxe1 | factory (BOOT_MODE only)
#   dmc12-firmware rollback [--yes]          = switch --to factory (erase BOOT_MODE)
#
# Writes touch only the three writable mtd windows the board DT exposes (ipxe0, ipxe1, boot-mode).
# Before any write, the read-only rest of the boot flash must match pinned hashes (a known layout
# and an intact factory U-Boot). Objects are written data first, header last, and read back:
# a torn write leaves a bad header (stage3 skips it), never a good header with bad data.
# BOOT_MODE, the switch, is always written last. Journal: /var/lib/dmc12-firmware/journal.jsonl.
set -euo pipefail
export PATH=@path@
P=@payloads@
JOURNAL=/var/lib/dmc12-firmware/journal.jsonl
HDR=72                    # al-boot object header (0x48 B)
SECTOR=4096               # stage3 reads BOOT_MODE's first sector only

die() { echo "dmc12-firmware: $*" >&2; exit 1; }
say() { echo "dmc12-firmware: $*"; }

# partition name -> /dev/mtdN (character device)
dev() {
    local d
    for d in /sys/class/mtd/mtd[0-9]*; do
        case "$d" in *ro) continue ;; esac
        if [ "$(cat "$d/name")" = "$1" ]; then echo "/dev/${d##*/}"; return; fi
    done
    die "no mtd partition '$1': this kernel's device tree lacks the firmware windows (hardware.dmc12.firmware)"
}
sha_of() { sha256sum | cut -d' ' -f1; }
is_ff() { [ "$(tr -d '\377' | head -c1 | wc -c)" -eq 0 ]; }    # stdin is all 0xFF

journal() {   # action part phase sha256
    mkdir -p "${JOURNAL%/*}"
    printf '{"t":"%s","action":"%s","part":"%s","phase":"%s","sha256":"%s","payload":"%s"}\n' \
        "$(date -Iseconds)" "$1" "$2" "$3" "$4" "$(cat "$P/ipxe.sha256")" >> "$JOURNAL"
    sync "$JOURNAL"
}

# the pinned read-only regions (P/pins: "name sha256" lines)
check_pins() {
    local name want got
    while read -r name want; do
        got=$(sha_of < "$(dev "$name")")
        [ "$got" = "$want" ] || die "read-only region $name does not match its pin ($got): unknown flash layout or a modified factory U-Boot; refusing to write"
    done < "$P/pins"
}

# slot state: blank | payload | other
slot_state() {
    local d n
    d=$(dev "$1"); n=$(stat -c %s "$P/ipxe.fimg")
    if is_ff < "$d"; then echo blank; return; fi
    if [ "$(head -c "$n" "$d" | sha_of)" = "$(cat "$P/ipxe.sha256")" ] && tail -c "+$((n + 1))" "$d" | is_ff; then
        echo payload
    else
        echo other
    fi
}

# BOOT_MODE state: factory | ipxe0 | ipxe1 | other (judged on the sector stage3 reads)
bm_state() {
    local d s n
    d=$(dev boot-mode)
    s=$(head -c "$SECTOR" "$d" | od -An -v -tx1 | tr -d ' \n')
    if [ "$s" = "$(printf 'ff%.0s' $(seq "$SECTOR"))" ]; then echo factory; return; fi
    for n in 0 1; do
        if [ "$(head -c "$SECTOR" "$d" | sha_of)" = "$(cat "$P/bm-ipxe$n.sector.sha256")" ]; then echo "ipxe$n"; return; fi
    done
    echo other
}

# write_obj PART FILE ERASE_BLOCKS: erase, data part, verify, header, verify. Returns 1 on any
# failure (every step is checked explicitly: errexit does not apply in a function used with ||).
write_obj() {
    local part=$1 file=$2 blocks=$3 d n
    d=$(dev "$part") || return 1
    n=$(stat -c %s "$file") || return 1
    journal write "$part" intent "$(sha_of < "$file")" || return 1
    flash_erase -q "$d" 0 "$blocks" || { echo "$part: erase failed" >&2; return 1; }
    head -c "$n" "$d" | is_ff || { echo "$part: not blank after erase" >&2; return 1; }
    dd if="$file" of="$d" bs=4096 iflag=skip_bytes oflag=seek_bytes skip="$HDR" seek="$HDR" status=none \
        || { echo "$part: data write failed" >&2; return 1; }
    cmp -s <(tail -c "+$((HDR + 1))" "$file") <(head -c "$n" "$d" | tail -c "+$((HDR + 1))") \
        || { echo "$part: data read-back differs (header not written: stage3 ignores the object)" >&2; return 1; }
    dd if="$file" of="$d" bs="$HDR" count=1 status=none || { echo "$part: header write failed" >&2; return 1; }
    cmp -s "$file" <(head -c "$n" "$d") || { echo "$part: read-back differs after the header write" >&2; return 1; }
    journal write "$part" done "$(sha_of < "$file")" || return 1
}

erase_bm() {
    local d; d=$(dev boot-mode)
    journal erase boot-mode intent -
    flash_erase -q "$d" 0 1
    head -c "$SECTOR" "$d" | is_ff || die "boot-mode: not blank after erase"
    journal erase boot-mode done -
}

confirm() {   # $1 = word to type
    [ "$YES" = 1 ] && return
    [ -t 0 ] || die "not a terminal: pass --yes"
    read -r -p "type '$1' to write the boot flash: " ans
    [ "$ans" = "$1" ] || die "not confirmed"
}

status() {
    local bm
    bm=$(bm_state)
    echo "boot-mode: $bm$( [ "$bm" = factory ] && echo ' (stage3 runs the factory U-Boot)')"
    echo "ipxe0:     $(slot_state ipxe0)"
    echo "ipxe1:     $(slot_state ipxe1)"
    echo "payload:   U-Boot $(cat "$P/version") ($(cut -c1-16 "$P/ipxe.sha256"))"
    if ( check_pins ) 2>/dev/null; then echo "read-only regions: match their pins"; else echo "read-only regions: DO NOT match their pins (writes refused)"; fi
}

[ "$(id -u)" = 0 ] || die "run as root"
cmd=${1:-status}; shift || true
YES=0; TO=
while [ $# -gt 0 ]; do
    case "$1" in
        --yes) YES=1 ;;
        --to) TO=${2:-}; shift ;;
        *) die "unknown argument $1" ;;
    esac
    shift
done
exec 9>/run/dmc12-firmware.lock
flock -n 9 || die "another dmc12-firmware is running"
[ "$(cat "$P/ipxe.sha256")" = "$(sha_of < "$P/ipxe.fimg")" ] || die "payload does not match its own hash"

case "$cmd" in
status) status ;;
install)
    check_pins
    bm=$(bm_state)
    case "$bm" in
        factory) target=0 ;;
        ipxe0) target=1 ;;
        ipxe1) target=0 ;;
        *) die "BOOT_MODE holds something unknown: run 'dmc12-firmware rollback' first" ;;
    esac
    st=$(slot_state "ipxe$target")
    say "BOOT_MODE: $bm; writing IPXE $target (now: $st), then BOOT_MODE -> ipxe$target"
    confirm install
    if [ "$st" = payload ]; then
        say "ipxe$target already holds this payload"
    else
        write_obj "ipxe$target" "$P/ipxe.fimg" 0 || die "IPXE $target write failed; BOOT_MODE untouched ($bm still boots)"
    fi
    [ "$(slot_state "ipxe$target")" = payload ] || die "ipxe$target does not hold the payload after writing"
    write_obj boot-mode "$P/bm-ipxe$target.bin" 1 || { erase_bm; die "BOOT_MODE write failed: erased it (factory U-Boot boots)"; }
    [ "$(bm_state)" = "ipxe$target" ] || { erase_bm; die "BOOT_MODE state wrong after writing: erased it (factory U-Boot boots)"; }
    say "done: the next boot runs U-Boot $(cat "$P/version") from IPXE $target."
    say "recovery: 'dmc12-firmware rollback' from Linux, or hold 'u' at stage3 on the serial console"
    ;;
switch|rollback)
    [ "$cmd" = rollback ] && TO=factory
    case "$TO" in
        factory) confirm rollback; erase_bm; say "BOOT_MODE erased: the factory U-Boot boots next" ;;
        ipxe0|ipxe1)
            check_pins
            [ "$(slot_state "$TO")" = payload ] || die "$TO does not hold this payload"
            confirm switch
            n=${TO#ipxe}
            write_obj boot-mode "$P/bm-ipxe$n.bin" 1 || { erase_bm; die "BOOT_MODE write failed: erased it"; }
            [ "$(bm_state)" = "$TO" ] || { erase_bm; die "BOOT_MODE state wrong after writing: erased it"; }
            say "BOOT_MODE -> $TO" ;;
        *) die "--to factory|ipxe0|ipxe1" ;;
    esac
    ;;
*) die "usage: dmc12-firmware status|install|switch --to SLOT|rollback [--yes]" ;;
esac
