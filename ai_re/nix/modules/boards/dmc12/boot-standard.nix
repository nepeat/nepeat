# DMC-12 on-disk boot through U-Boot standard boot: our U-Boot 2026.10 runs from
# the SPI flash (stage3's IPXE app, devices/dmc-12-48r-as0/spi-upgrade.py) and its
# `bootflow scan -lb` finds /extlinux/extlinux.conf on the bootable sda1.
# hardware.dmc12.bootLoader = "standard" (bootloader.nix).
#
#   sda1  ext4 /boot (DMC12BOOT, bootable flag): nixpkgs' generic-extlinux-compatible
#         (raw Image, initrd, FDT = the board DTB), generations menu.
#   sda2  ext4 / (DMC12ROOT).
#
# No raw U-Boot partition and no factory env: the U-Boot lives in SPI and is
# updated with spi-upgrade.py, not by nixos-rebuild. The factory U-Boot (the `u`
# key at stage3) can't boot this disk; it stays a recovery prompt (netboot).
{ config, lib, pkgs, modulesPath, ... }:
let
  cfg = config.boot.loader.generic-extlinux-compatible;
  extlinuxBuilder = import (modulesPath + "/system/boot/loader/generic-extlinux-compatible/extlinux-conf-builder.nix") { inherit lib pkgs; };
  timeout = if config.boot.loader.timeout == null then "-1" else toString config.boot.loader.timeout;
in
{
  config = lib.mkIf (config.hardware.dmc12.bootLoader == "standard") {
    boot.loader.grub.enable = lib.mkForce false;
    boot.loader.systemd-boot.enable = lib.mkForce false;
    boot.loader.generic-extlinux-compatible = {
      enable = true;
      configurationLimit = 10;
    };
    boot.loader.timeout = lib.mkDefault 3;
    # nixpkgs' installer (same builder and arguments as generic-extlinux-compatible), then gzip
    # the kernels it copied: our U-Boot boots them compressed (compress-kernels.nix).
    system.build.installBootLoader = lib.mkForce (pkgs.writeScript "install-dmc12-extlinux.sh" ''
      #!${pkgs.runtimeShell}
      set -e
      ${extlinuxBuilder} -g ${toString cfg.configurationLimit} -t ${timeout} -n ${config.hardware.deviceTree.name} -d /boot -c "$1"
      ${import ./compress-kernels.nix { inherit pkgs; }} /boot
    '');
    # The FDT line names the board DTB (U-Boot's fdtfile is only a fallback).
    assertions = [{
      assertion = config.hardware.deviceTree.name != null;
      message = "dmc12 standard boot: hardware.deviceTree.name must name the board DTB";
    }];
  };
}
