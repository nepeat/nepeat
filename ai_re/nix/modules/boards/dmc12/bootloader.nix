# Which on-disk boot path the DMC-12 uses (see disk.nix).
{ lib, ... }:
{
  options.hardware.dmc12.bootLoader = lib.mkOption {
    type = lib.types.enum [ "uboot-menu" "chainload" ];
    default = "uboot-menu";
    description = ''
      How the factory U-Boot 2015.07 boots NixOS from the SSD (disk.nix):
      "uboot-menu": /boot.scr on an ext4 sda1, legacy uImages + bootmenu in the
      factory loader (first iteration).
      "chainload": the factory env reads upstream U-Boot 2026.10 from a raw sda1
      (scsi read, slots A/B) and bootm's it; that boots a raw Image through
      extlinux on sda2 (boot-chainload.nix, u-boot-port.md). Built as the
      dmc12-disk-image-chainload output.
      Default stays uboot-menu until chainload has booted from disk on hardware.
    '';
  };
}
