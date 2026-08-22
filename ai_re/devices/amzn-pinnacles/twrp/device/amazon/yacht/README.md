# TWRP device tree — Amazon `yacht` / KFYAWI (MT8183)

A complete, from-scratch TWRP device tree for this device. **Every value is
derived from the device itself or from its own stock `recovery.img`** (sha256
`6ec64a2d…`) — nothing is copied from a similar device and guessed at.

## ⚠️ Read first: this cannot boot on a locked unit

Two independent blockers, both verified:

1. **LK verifies `recovery` against Amazon's production RSA-2048 key** on a
   locked device (`amzn_image_verify`: *"Only try verify %s with prod key on
   locked production device"*). An unsigned TWRP image fails that check.
2. **There is no way to load it.** `boot` is on LK's *restricted* command table
   (`0x83680`), and `flash:recovery` is not on the locked-hardware allowlist —
   which is exactly `{oem relock, oem flags, flash:unlock, flash:tucert,
   flash:tucode}` (`0x8366c`, verified by reading the table).

So `fastboot boot twrp.img` and `fastboot flash recovery twrp.img` are both
refused, and the only remaining route — `dd` to `mmcblk0p17` with root — would
**destroy the working stock recovery and gain nothing**, because LK still
refuses to boot the result. Net effect: a device with no recovery.

**Do not flash this while the bootloader is locked.** The tree is groundwork for
if an unlock ever lands; it is not usable today.

## Provenance of the values

| value | source |
| --- | --- |
| `BOARD_KERNEL_BASE 0x40078000` | stock recovery header: `kernel_addr 0x40080000` − `0x8000` |
| offsets `0x8000 / 0x14f88000 / 0x13f88000` | header `kernel/ramdisk/tags_addr` − base |
| `BOARD_KERNEL_PAGESIZE 2048`, `header_version 1` | stock recovery header |
| `BOARD_KERNEL_CMDLINE` | verbatim from the stock header (incl. `veritykeyid`) |
| `Image.gz-dtb` | kernel blob is gzip(arm64 `Image`) + **4** appended DTBs at `0xd00dfeed` offsets `0x000000/0x02d1a3/0x05a441/0x087899` |
| `TARGET_ARCH arm` | `ro.product.cpu.abilist64` empty, zygote32, stock `/sbin/recovery` is ELF 32-bit ARM — on an arm64 kernel |
| `TARGET_BOARD_PLATFORM mt8183` | `ro.board.platform`; DTB root `compatible = "mediatek,mt8183"` |
| partition sizes | exact byte counts of the full-partition dumps |
| fstab | converted from the stock recovery ramdisk `/etc/recovery.fstab`, block paths re-verified against `by-name/` on the live device |
| `TW_THEME portrait_hdpi` | `wm size` = 1200x1920 (portrait-native), `wm density` = 240 |
| no `vbmeta`, `AB_OTA_UPDATER false` | partition table has no vbmeta and no `_a/_b` slots |

Deliberately **not** in the fstab: `kb`, `dkb`, `keys` (per-device
Widevine/attestation secrets), `tee1`/`tee2` (unrecoverable if corrupted), and
`preloader` (bricks the device — BROM is fused off, so there is no fallback).

`TW_INCLUDE_CRYPTO` is **off**: the dumped unit was `ro.crypto.state=unencrypted`
but the stock fstab declares metadata encryption with `aes-256-cts` filenames.
Enabling it untested risks failing to mount `/data`.

## Building — not possible on this host

This machine cannot build it, for three separate reasons:

* **macOS.** AOSP/TWRP prebuilt toolchains are `linux-x86` only; darwin support
  is long gone.
* **Case-insensitive filesystem.** Verified: creating `aA` and `Aa` yields one
  file. AOSP requires case sensitivity.
* **31 GB free.** A minimal TWRP tree needs roughly 100 GB to sync and build.

Docker is installed but its daemon is not running, and Docker Desktop on macOS
would still be bounded by the same free space.

### Recipe for a Linux host

```bash
mkdir twrp && cd twrp
repo init -u https://github.com/minimal-manifest-twrp/platform_manifest_twrp_aosp.git -b twrp-9.0
repo sync -j"$(nproc)" --force-sync --no-clone-bundle --no-tags

mkdir -p device/amazon
cp -r <this-tree> device/amazon/yacht
( cd device/amazon/yacht && ./extract-prebuilt-kernel.sh /path/to/recovery.img )

source build/envsetup.sh
lunch twrp_yacht-eng
mka recoveryimage       # -> out/target/product/yacht/recovery.img
```

`twrp-9.0` matches the device's Android 9 / API 28 base (`PRODUCT_TARGET_VNDK_VERSION := 28`).

The prebuilt kernel is **not committed** (10 MB of Amazon's kernel);
regenerate it with `extract-prebuilt-kernel.sh` from your own dump.

## Backup

The stock recovery is preserved at `fw-partitions/backup/recovery.img.orig`
(read-only, 42958848 B, sha256 `6ec64a2d…`), matching the hash recorded in
`dumps/partition-manifest.txt` at dump time — so it is verified faithful to the
original partition read, not merely present. Integrity re-checked by
decompressing the ramdisk to a valid cpio (`070701`).

Restore (requires root, and only if recovery is ever damaged):

```bash
adb push recovery.img.orig /data/local/tmp/
adb shell su -c 'dd if=/data/local/tmp/recovery.img.orig of=/dev/block/by-name/recovery bs=4096'
```
