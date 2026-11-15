#! @bash@/bin/sh -e
# DMC-12 U-Boot bootmenu builder. Invoked by NixOS activation as
# `installBootLoader <toplevel>` after every nixos-rebuild switch/boot.
#
# Walks /nix/var/nix/profiles/system-*-link (plus the explicit current
# toplevel), stages each generation's kernel + initrd + DTB into
# /boot/nixos/<hash-name>, and writes /boot/u-boot.menu — a text file
# the U-Boot stage 1 (/boot/boot.scr) loads via `env import -t` and
# hands to the `bootmenu` command.
#
# Output layout:
#   /boot.scr                    (mkimage'd; sources u-boot.menu + runs bootmenu)
#   /boot/u-boot.menu            (env-import text — bootmenu_N entries)
#   /boot/nixos/<hash>-Image     (per-gen kernel)
#   /boot/nixos/<hash>-initrd
#   /boot/nixos/<hash>-…-dmc12.dtb
#
# Modeled on nixpkgs's generic-extlinux-compatible/extlinux-conf-builder.sh.
shopt -s nullglob

export PATH=/empty:@path@

usage() {
    echo "usage: $0 <toplevel> [-d <boot dir>]" >&2
    exit 1
}

default="${1:-}"
[ -z "$default" ] && usage

target=/boot
# -d: stage into another directory (disk-image build: no /boot, no profiles).
if [ "${2:-}" = "-d" ]; then
    [ -n "${3:-}" ] || usage
    target="$3"
fi
timeout=@timeout@
numGenerations=@numGenerations@
dtbName=@dtbName@

mkdir -p "$target/nixos"

# Convert /nix/store/<hash>-<name>/file to <hash>-<name>-<file>.
cleanName() {
    echo "$1" | sed 's|^/nix/store/||' | sed 's|/|-|g'
}

# Stage a file into /boot/nixos/. Idempotent: if the destination
# already exists (same store hash + same wrap), skip. Marks the dst
# as still-referenced via filesCopied so the gc loop at the bottom
# doesn't prune it.
declare -A filesCopied

# Plain copy — used for DTB (raw FDT, no header).
copyToKernelsDir() {
    local src
    src=$(readlink -f "$1")
    local dst="$target/nixos/$(cleanName "$src")"
    if ! test -e "$dst"; then
        local dstTmp="$dst.tmp.$$"
        cp -r "$src" "$dstTmp"
        mv "$dstTmp" "$dst"
    fi
    filesCopied[$dst]=1
    result=$dst
}

# Kernel: gzip the raw aarch64 Image, then mkimage-wrap it as a U-Boot
# legacy kernel uImage at 0x04080000 (the load/entry address the
# Annapurna vendor U-Boot expects for arm64 kernels — see uboot_vars.txt
# loadaddr=0x08000000 vs entry @ 0x04080000). The vendor U-Boot has no
# `booti` command, so raw Image won't boot — `bootm` requires a
# mkimage-wrapped payload.
stageKernel() {
    local src
    src=$(readlink -f "$1")
    local dst="$target/nixos/$(cleanName "$src").uImage"
    if ! test -e "$dst"; then
        local dstTmp="$dst.tmp.$$"
        # Uncompressed (-C none): bootm only memmoves it to 0x04080000, so the
        # factory U-Boot's gunzip size limit never applies (a full-config 7.3
        # Image is 68 MiB). Load plan: kernel 0x10000000, initrd 0x20000000,
        # DTB 0x04078000 (see packages/dmc12-images/netboot-files.nix).
        mkimage -A arm64 -O linux -T kernel -C none \
            -a 0x04080000 -e 0x04080000 \
            -n "DMC-12 NixOS kernel" \
            -d "$src" "$dstTmp" > /dev/null
        mv "$dstTmp" "$dst"
    fi
    filesCopied[$dst]=1
    result=$dst
}

# Initrd: mkimage-wrap as a ramdisk uImage. -C none because the kernel
# (not U-Boot) decompresses the contents — passing -C gzip would make
# U-Boot try to gunzip the cpio, which fails when NixOS chose a
# different compressor. boot.initrd.compressor in nixos/disk.nix is
# pinned to "gzip" so the kernel's CONFIG_RD_GZIP=y handles it on the
# other side either way.
stageInitrd() {
    local src
    src=$(readlink -f "$1")
    local dst="$target/nixos/$(cleanName "$src").uImage"
    if ! test -e "$dst"; then
        local dstTmp="$dst.tmp.$$"
        # > /dev/null — see stageKernel for why mkimage stdout is muted.
        mkimage -A arm64 -O linux -T ramdisk -C none \
            -n "DMC-12 NixOS initrd" \
            -d "$src" "$dstTmp" > /dev/null
        mv "$dstTmp" "$dst"
    fi
    filesCopied[$dst]=1
    result=$dst
}

# Emit one `bootmenu_N=label=cmd` line for a generation. Caller picks
# the index ($3); bootmenu_0 = default = current toplevel.
addEntry() {
    local path
    path=$(readlink -f "$1")
    local tag="$2"   # "default" or generation number
    local idx="$3"   # bootmenu_N index

    if ! test -e "$path/kernel" -a -e "$path/initrd"; then
        return
    fi

    stageKernel       "$path/kernel";            local kernel; kernel=$(basename "$result")
    stageInitrd       "$path/initrd";            local initrd; initrd=$(basename "$result")
    copyToKernelsDir  "$path/dtbs/$dtbName";     local dtb;    dtb=$(basename "$result")

    local timestampEpoch
    timestampEpoch=$(stat -L -c '%Z' "$path")
    local timestamp
    timestamp=$(date "+%Y-%m-%d %H:%M" -d @"$timestampEpoch")
    local nixosLabel
    nixosLabel=$(cat "$path/nixos-version")
    local extraParams
    extraParams=$(cat "$path/kernel-params")

    local label
    if [ "$tag" = "default" ]; then
        label="NixOS - Default ($timestamp - $nixosLabel)"
    else
        label="NixOS - Gen $tag ($timestamp - $nixosLabel)"
    fi

    # The whole command is ONE U-Boot env var value. U-Boot env values
    # are size-limited (~1KB typical) so keep this compact; no echoing
    # progress, no comments, just the chain. The single-quoted bootargs
    # block survives U-Boot's hush parser even when extraParams contains
    # spaces — the inner quotes don't conflict because the outer
    # bootmenu_N= record is a single env var, NOT a shell word.
    local cmd
    cmd="ext4load scsi 0:1 0x10000000 /nixos/$kernel"
    cmd="$cmd; ext4load scsi 0:1 0x20000000 /nixos/$initrd"
    cmd="$cmd; ext4load scsi 0:1 0x04078000 /nixos/$dtb"
    cmd="$cmd; setenv bootargs 'init=$path/init $extraParams'"
    cmd="$cmd; setenv initrd_high 0xffffffff"
    cmd="$cmd; setenv fdt_high 0xffffffff"
    cmd="$cmd; bootm 0x10000000 0x20000000 0x04078000"

    printf 'bootmenu_%d=%s=%s\n' "$idx" "$label" "$cmd"
}

tmpFile="$target/u-boot.menu.tmp.$$"
{
    echo "# Auto-generated by NixOS dmc12-uboot-menu — do not edit."
    echo "# Consumed by /boot.scr via 'ext4load … ; env import -t addr size'."
    echo "bootmenu_delay=$timeout"
    addEntry "$default" default 0
    idx=1
    if [ "$numGenerations" -gt 0 ] && [ -d /nix/var/nix/profiles ]; then
        for generation in $(
                (cd /nix/var/nix/profiles && ls -d system-*-link) \
                | sed 's/system-\([0-9]\+\)-link/\1/' \
                | sort -n -r \
                | head -n "$numGenerations"); do
            link="/nix/var/nix/profiles/system-$generation-link"
            # Skip if it's the same store path as $default — already at idx 0.
            if [ "$(readlink -f "$link")" = "$(readlink -f "$default")" ]; then
                continue
            fi
            addEntry "$link" "$generation" "$idx"
            idx=$((idx + 1))
        done
    fi
} > "$tmpFile"

mv -f "$tmpFile" "$target/u-boot.menu"

# Static /boot.scr — pre-built mkimage script that ext4loads u-boot.menu
# + runs bootmenu. Idempotent copy.
install -m 0644 -T @bootScr@ "$target/boot.scr" 2>/dev/null \
    || cp -f @bootScr@ "$target/boot.scr"

# GC: drop /boot/nixos/* not referenced by any active generation.
for fn in "$target/nixos"/*; do
    if ! test "${filesCopied[$fn]}" = 1; then
        echo "[dmc12-uboot-menu] removing stale boot file: $fn"
        chmod +w -- "$fn"
        rm -rf -- "$fn"
    fi
done
