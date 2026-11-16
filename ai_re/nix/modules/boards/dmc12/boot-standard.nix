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
{ config, lib, ... }:
{
  config = lib.mkIf (config.hardware.dmc12.bootLoader == "standard") {
    boot.loader.grub.enable = lib.mkForce false;
    boot.loader.systemd-boot.enable = lib.mkForce false;
    boot.loader.generic-extlinux-compatible = {
      enable = true;
      configurationLimit = 10;
    };
    boot.loader.timeout = lib.mkDefault 3;
    # The FDT line names the board DTB (U-Boot's fdtfile is only a fallback).
    assertions = [{
      assertion = config.hardware.deviceTree.name != null;
      message = "dmc12 standard boot: hardware.deviceTree.name must name the board DTB";
    }];
  };
}
