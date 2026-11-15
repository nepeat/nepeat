# DMC-12 on-disk boot through upstream U-Boot 2026.10, chainloaded from a raw
# partition. hardware.dmc12.bootLoader = "chainload" (bootloader.nix).
#
#   sda1  8 MiB, type 0xda, raw: slot A at LBA 2048, slot B at LBA 10240
#         (4 MiB each), each a kernel-type uImage of the upstream U-Boot.
#         B holds the image A had before the last update.
#   sda2  ext4 /boot (label DMC12BOOT, bootable flag), any ext4 features:
#         only upstream U-Boot reads it. extlinux + kernels/initrds/DTBs.
#   sda3  ext4 / (DMC12ROOT).
#
#   factory U-Boot 2015.07   bootcmd = run nixos_boot; run dmc12_bootdisk
#     nixos_boot (factoryEnv below; reimage.py sets it; no ext4 in this loader):
#       run dmc12_boot_preamble; scsi init; then slot A, then slot B:
#       scsi read 0x10000000 <lba> 0x2000 && iminfo 0x10000000 && bootm 0x10000000 - ${fdtaddr}
#     (scsi read takes hex: address, start block, block count.)
#   upstream U-Boot (RAM)    bootcmd = bootflow scan -lb
#   /extlinux/extlinux.conf  nixpkgs' extlinux builder: raw Image, initrd, FDT
#                            -> booti at EL2, generations menu.
#
# The installer (activation, and disk.nix's `dmc12BootBuilderNative <toplevel>
# -d <dir>`) writes /boot; on the box it also updates slot A (old A -> B),
# after checking that sda1 is exactly this layout. disk.nix writes both
# slots into the image.
{ config, lib, pkgs, ... }:
let
  bp = pkgs.buildPackages;
  # Bootloaders are built for the build host (cross to aarch64 inside).
  uboot = (import ../../../packages/uboot/alpine-v2 {
    pkgs = bp;
    uboots = import ../../../packages/uboot { pkgs = bp; };
  }).dmc12-chainload;
  generations = 10;
  timeout = 3; # extlinux menu, seconds

  layout = rec {
    partStart = 2048; # LBA of sda1 (1 MiB)
    partSectors = 16384; # 8 MiB
    slotSectors = 8192; # 4 MiB per slot
    slotA = partStart;
    slotB = partStart + slotSectors;
    bootStart = partStart + partSectors; # sda2
    loadAddr = "0x10000000"; # factory side; bootm copies to 0x08000000
  };
  hex = n: "0x" + lib.toLower (lib.toHexString n);
  tryslot = lba:
    "scsi read ${layout.loadAddr} ${hex lba} ${hex layout.slotSectors} && "
    + "iminfo ${layout.loadAddr} && bootm ${layout.loadAddr} - \${fdtaddr}";
  # Factory env, exactly as it must be stored (${fdtaddr} literal: set it with
  # single quotes). The fallback stays the factory chain. Typed at the factory
  # prompt as `setenv nixos_boot '<value>'`: keep that line under 254 chars
  # (CONFIG_SYS_CBSIZE is not known; 256 is the smallest plausible).
  factoryEnv = {
    # The factory preamble first: DN0 SATA SerDes fix, WDT flag, fans 70%,
    # LED, uplink sync and bootp, whose al_eth halt stores the board params +
    # MAC that Linux al_eth needs (the factory env has autoload=n, so bootp
    # loads nothing). It ends with scsi init; the explicit one is cheap.
    nixos_boot = lib.concatStringsSep "; " [
      "run dmc12_boot_preamble"
      "scsi init"
      (tryslot layout.slotA)
      (tryslot layout.slotB)
    ];
    bootcmd = "run nixos_boot; run dmc12_bootdisk";
  };

  # p = pkgs (on the box, at activation) or pkgs.buildPackages (disk image).
  mkBuilder = p:
    let
      extlinux = import (p.path + "/nixos/modules/system/boot/loader/generic-extlinux-compatible/extlinux-conf-builder.nix") {
        inherit lib; pkgs = p;
      };
    in p.writeShellScript "dmc12-chainload-install" ''
      set -euo pipefail
      export PATH=${lib.makeBinPath [ p.coreutils p.diffutils p.util-linux ]}
      toplevel=$1; shift
      target=/boot; image=0
      if [ "''${1:-}" = "-d" ]; then target=$2; image=1; fi
      # Same call as nixpkgs' generic-extlinux-compatible (FDT line names the
      # board DTB explicitly, so U-Boot's fdtfile is only a fallback).
      ${extlinux} -g ${toString generations} -t ${toString timeout} \
        -n ${config.hardware.deviceTree.name} -c "$toplevel" -d "$target"
      [ "$image" = 1 ] && exit 0   # disk.nix writes the slots itself

      # U-Boot slot A on sda1 of the disk that holds /boot.
      src=${uboot}/u-boot.uimage
      bootdev=$(findmnt -no SOURCE /boot)
      disk=$(lsblk -no PKNAME "$bootdev")
      part=/dev/''${disk}1
      sys=/sys/class/block/''${disk}1
      if [ "$(cat $sys/start)" != ${toString layout.partStart} ] ||
         [ "$(cat $sys/size)" != ${toString layout.partSectors} ] ||
         [ "$(blkid -p -s PART_ENTRY_TYPE -o value "$part")" != 0xda ]; then
        echo "dmc12-chainload: $part is not the raw U-Boot partition; not writing it" >&2
        exit 1
      fi
      n=$(stat -c %s "$src")
      if [ "$n" -gt $(( ${toString layout.slotSectors} * 512 )) ]; then
        echo "dmc12-chainload: U-Boot image too big for a slot" >&2; exit 1
      fi
      if cmp -s -n "$n" "$src" "$part"; then exit 0; fi
      # Keep the current image as the fallback (slot B), then write A.
      dd if="$part" of="$part" bs=512 count=${toString layout.slotSectors} \
        seek=${toString layout.slotSectors} conv=notrunc,fsync status=none
      dd if="$src" of="$part" bs=512 conv=notrunc,fsync status=none
      cmp -n "$n" "$src" "$part"
      echo "dmc12-chainload: U-Boot slot A updated (previous image in slot B)"
    '';
in
{
  config = lib.mkIf (config.hardware.dmc12.bootLoader == "chainload") {
    # No generic-extlinux-compatible.enable: its installBootLoader would clash
    # with ours. We call the same builder (same extlinux.conf layout).
    assertions = [{
      assertion = lib.stringLength "setenv nixos_boot '${factoryEnv.nixos_boot}'" < 254;
      message = "dmc12 chainload: nixos_boot is too long to type at the factory prompt";
    }];
    system.build.installBootLoader = lib.mkForce (mkBuilder pkgs);
    system.build.dmc12BootBuilderNative = mkBuilder bp;
    system.boot.loader.id = lib.mkForce "dmc12-uboot-chainload";
    # For disk.nix and reimage.py: layout, the image, the factory env.
    system.build.dmc12Chainload = {
      inherit layout factoryEnv;
      uimage = "${uboot}/u-boot.uimage";
    };
    system.build.dmc12UbootChainload = bp.runCommand "dmc12-uboot-chainload" { } ''
      mkdir -p $out
      cp ${uboot}/u-boot.uimage ${uboot}/u-boot.bin $out/
      cat > $out/factory-env.txt <<'EOF'
      nixos_boot=${factoryEnv.nixos_boot}
      bootcmd=${factoryEnv.bootcmd}
      EOF
    '';
    boot.loader.grub.enable = lib.mkForce false;
    boot.loader.systemd-boot.enable = lib.mkForce false;
  };
}
