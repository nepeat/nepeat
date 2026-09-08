#!/usr/bin/env bash
# Derive a RAM-only recovery DTB from the captured live device FDT.
#
# Reads the live DTB and a ramdisk, writes a new DTB. Touches no partition and
# never talks to the tablet. Uses in-place fdtput edits so the live strings
# block is preserved -- a dtc round-trip would rewrite it and is NOT equivalent.
set -euo pipefail

live=""; ramdisk=""; out=""; display="off"; serialno=""; maxcpus=1
dtb_addr=0x54000000; initrd_addr=0x55000000

usage() {
    cat >&2 <<'USAGE'
usage: make-recovery-dtb.sh --live LIVE.dtb --ramdisk RD.gz --out OUT.dtb
                           [--display on|off] [--serialno SERIAL]
                           [--maxcpus N] [--initrd-addr 0x...]

  --display on   leave /mtkfb@0 at its live default (okay) -- GUI capable
  --display off  set /mtkfb@0 status=disabled            -- headless
USAGE
    exit 2
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --live) live=$2; shift 2 ;;
        --ramdisk) ramdisk=$2; shift 2 ;;
        --out) out=$2; shift 2 ;;
        --display) display=$2; shift 2 ;;
        --serialno) serialno=$2; shift 2 ;;
        --maxcpus) maxcpus=$2; shift 2 ;;
        --initrd-addr) initrd_addr=$2; shift 2 ;;
        *) usage ;;
    esac
done

[[ -n "$live" && -n "$ramdisk" && -n "$out" ]] || usage
[[ "$display" == on || "$display" == off ]] || usage
[[ -r "$live" ]] || { echo "cannot read $live" >&2; exit 1; }
[[ -r "$ramdisk" ]] || { echo "cannot read $ramdisk" >&2; exit 1; }
[[ ! -e "$out" ]] || { echo "refusing to overwrite $out" >&2; exit 1; }

size=$(wc -c <"$ramdisk")
start=$initrd_addr
end=$(printf '0x%x' $(( start + size )))

# The live cmdline boots Fire OS from dm-verity. Everything that selects that
# path (skip_initramfs, root=, dm=, veritymode) is dropped so the kernel uses
# our initramfs instead; the platform tokens are kept verbatim.
args="console=tty0 console=ttyS0,921600n1 vmalloc=496M slub_max_order=0"
args+=" slub_debug=OFZPU androidboot.hardware=mt8183"
args+=" firmware_class.path=/vendor/firmware loop.max_part=7"
args+=" has_battery_removed=0 lcm_id=48 androidboot.hardware.sku=plus"
args+=" androidboot.wpc.support=1 androidboot.nfc.support=1"
args+=" usbcore.autosuspend=5 maxcpus=${maxcpus} bootopt=64S3,32N2,64N2"
args+=" buildvariant=eng"
args+=" veritykeyid=id:4be33f8ba0062faa6f2d75b5f6475b106e02b7aa"
args+=" androidboot.selinux=permissive androidboot.mode=recovery"
args+=" androidboot.force_normal_boot=0 printk.disable_uart=1"
args+=" initcall_debug ignore_loglevel loglevel=8 enforcing=0"
[[ -n "$serialno" ]] && args+=" androidboot.serialno=${serialno}"

tmp=$(mktemp "${TMPDIR:-/tmp}/yacht-dtb.XXXXXX")
trap 'rm -f -- "$tmp"' EXIT
cp "$live" "$tmp"

fdtput -t s "$tmp" /chosen bootargs "$args"
fdtput -t x "$tmp" /chosen linux,initrd-start "$start"
fdtput -t x "$tmp" /chosen linux,initrd-end "$end"

# GCE and M4U must stay up: without GCE disp_probe_1 NULL-derefs (v23), and
# without M4U mtkfb_init hangs in m4u_do_mva_alloc (v24).
fdtput -t s "$tmp" /gce status okay
fdtput -t s "$tmp" /m4u status okay
# MTEE is unused by this payload; DEVAPC raised an efuse_top violation storm
# that reset the warm boot (v21/v22). Both stay off.
fdtput -t s "$tmp" /mtee status disabled
fdtput -t s "$tmp" /devapc status disabled

if [[ "$display" == off ]]; then
    fdtput -t s "$tmp" /mtkfb@0 status disabled
else
    # Live has no status on /mtkfb@0 at all; leaving it untouched reproduces
    # the exact configuration Fire OS boots the panel with.
    fdtput -d "$tmp" /mtkfb@0 status 2>/dev/null || true
fi

mv "$tmp" "$out"
trap - EXIT

echo "wrote $out"
printf 'ramdisk %s bytes -> initrd %s..%s\n' "$size" "$start" "$end"
for node in /gce /m4u /mtee /devapc /mtkfb@0; do
    printf '  %-10s %s\n' "$node" "$(fdtput -h >/dev/null 2>&1; fdtget -t s "$out" "$node" status 2>/dev/null || echo '(no status -> okay)')"
done
wc -c <"$out"
shasum -a 256 "$out"
