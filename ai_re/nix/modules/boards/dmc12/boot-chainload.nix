# DMC-12 on-disk boot through upstream U-Boot 2026.10, chainloaded from the SSD.
# Alternative to ./uboot-menu (uImages + bootmenu run by the factory U-Boot).
#
#   factory U-Boot 2015.07  bootcmd = run nixos_boot; run dmc12_bootdisk
#     nixos_boot  = scsi init; ext4load scsi 0:1 0x07000000 /boot.scr && source 0x07000000
#                   (the reimage.md env change; the same for both loaders)
#   /boot.scr (this module)  fans/WDT flag as the factory preamble, then
#                   ext4load /u-boot-dmc12.uimage -> bootm 0x10000000 - ${fdtaddr}
#                   (previous image /u-boot-dmc12.prev.uimage if that fails)
#   upstream U-Boot (RAM)    bootcmd = bootflow scan -lb
#   /extlinux/extlinux.conf  nixpkgs generic-extlinux-compatible: raw Image,
#                   initrd, FDT -> booti at EL2, generations menu.
#
# Paths: U-Boot sees sda1's root, NixOS mounts it at /boot ("/boot.scr" on
# the partition is /boot/boot.scr here).
#
# Usage: import this instead of ./uboot-menu (disk.nix imports uboot-menu
# today; the two both force system.build.installBootLoader). It provides the
# same system.build.dmc12BootBuilderNative interface disk.nix calls:
#   <builder> <toplevel> -d <dir>
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

  # Runs in the factory U-Boot 2015.07 (hush, no `exit`, no booti). Also
  # visible to the upstream U-Boot's script bootmeth if extlinux fails: the
  # kernel_comp_addr_r guard (only in the upstream default env) stops it from
  # chainloading itself in a loop.
  bootCmd = pkgs.writeText "dmc12-chainload-boot.cmd" ''
    if test -n "''${kernel_comp_addr_r}"; then
      echo "boot.scr: already in upstream U-Boot, not chainloading again"
    else
      echo "== DMC-12: chainloading upstream U-Boot from sda1 =="
      run clear_wdt_flag
      run fan_speed_70
      scsi init
      if ext4load scsi 0:1 0x10000000 /u-boot-dmc12.uimage && iminfo 0x10000000; then
        bootm 0x10000000 - ''${fdtaddr}
      fi
      echo "boot.scr: /u-boot-dmc12.uimage failed, trying the previous one"
      if ext4load scsi 0:1 0x10000000 /u-boot-dmc12.prev.uimage && iminfo 0x10000000; then
        bootm 0x10000000 - ''${fdtaddr}
      fi
      echo "boot.scr: no upstream U-Boot booted; falling back to the factory chain"
    fi
  '';
  bootScr = bp.runCommand "dmc12-chainload-boot.scr" { nativeBuildInputs = [ bp.ubootTools ]; } ''
    mkimage -A arm64 -O linux -T script -C none -a 0 -e 0 \
      -n "DMC-12 chainload upstream U-Boot" -d ${bootCmd} $out
  '';

  # p = pkgs (on the box, at activation) or pkgs.buildPackages (disk image).
  mkBuilder = p:
    let
      extlinux = import (p.path + "/nixos/modules/system/boot/loader/generic-extlinux-compatible/extlinux-conf-builder.nix") {
        inherit lib; pkgs = p;
      };
    in p.writeShellScript "dmc12-chainload-install" ''
      set -euo pipefail
      export PATH=${lib.makeBinPath [ p.coreutils p.diffutils ]}
      toplevel=$1; shift
      target=/boot
      if [ "''${1:-}" = "-d" ]; then target=$2; fi
      # Same call as nixpkgs' generic-extlinux-compatible (FDT line names the
      # board DTB explicitly, so U-Boot's fdtfile is only a fallback).
      ${extlinux} -g ${toString generations} -t ${toString timeout} \
        -n ${config.hardware.deviceTree.name} -c "$toplevel" -d "$target"
      # U-Boot images: replace only when changed, keep one previous copy.
      put() { # src dst
        if ! cmp -s "$1" "$target/$2"; then
          if [ -e "$target/$2" ] && [ "$2" = u-boot-dmc12.uimage ]; then
            cp "$target/$2" "$target/u-boot-dmc12.prev.uimage.tmp"
            mv "$target/u-boot-dmc12.prev.uimage.tmp" "$target/u-boot-dmc12.prev.uimage"
          fi
          cp "$1" "$target/$2.tmp"; mv "$target/$2.tmp" "$target/$2"
        fi
      }
      put ${uboot}/u-boot.uimage u-boot-dmc12.uimage
      put ${bootScr} boot.scr
      sync -f "$target/boot.scr" 2>/dev/null || sync
    '';
in
{
  config = {
    # No generic-extlinux-compatible.enable: its installBootLoader would clash
    # with ours. We call the same builder (same extlinux.conf layout).
    system.build.installBootLoader = lib.mkForce (mkBuilder pkgs);
    system.build.dmc12BootBuilderNative = mkBuilder bp;
    system.boot.loader.id = lib.mkForce "dmc12-uboot-chainload";
    system.build.dmc12UbootChainload = bp.runCommand "dmc12-uboot-chainload" { } ''
      mkdir -p $out
      cp ${uboot}/u-boot.uimage ${uboot}/u-boot.bin $out/
      cp ${bootCmd} $out/boot.cmd; cp ${bootScr} $out/boot.scr
    '';
    boot.loader.grub.enable = lib.mkForce false;
    boot.loader.systemd-boot.enable = lib.mkForce false;
  };
}
