# Which on-disk boot path the DMC-12 uses (see disk.nix).
{ lib, ... }:
{
  options.hardware.dmc12.bootLoader = lib.mkOption {
    type = lib.types.enum [ "uboot-menu" "chainload" ];
    default = "uboot-menu";
    description = ''
      How the factory U-Boot 2015.07 boots NixOS from sda1 (both via /boot.scr):
      "uboot-menu": legacy uImages + bootmenu in the factory loader (first iteration).
      "chainload": /boot.scr chainloads upstream U-Boot 2026.10 from sda1, which
      boots a raw Image through extlinux (boot-chainload.nix, u-boot-port.md).
      Default stays uboot-menu until chainload has booted Linux on hardware.
    '';
  };
}
