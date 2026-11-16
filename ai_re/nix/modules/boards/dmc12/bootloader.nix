# Which on-disk boot path the DMC-12 uses (see disk.nix).
{ lib, ... }:
{
  options.hardware.dmc12.bootLoader = lib.mkOption {
    type = lib.types.enum [ "standard" "uboot-menu" "chainload" ];
    default = "standard";
    description = ''
      How NixOS boots from the SSD (disk.nix):
      "standard" (default): our U-Boot 2026.10 in the SPI flash (spi-upgrade.py)
      runs standard boot (`bootflow scan`), which finds nixpkgs'
      generic-extlinux-compatible on an ext4 sda1 (boot-standard.nix). Needs the
      SPI install; a unit without it uses "chainload".
      Through the factory U-Boot 2015.07 (units with the factory SPI):
      "uboot-menu": /boot.scr on an ext4 sda1, legacy uImages + bootmenu in the
      factory loader (first iteration).
      "chainload": the factory env reads upstream U-Boot 2026.10 from a raw sda1
      (scsi read, slots A/B) and bootm's it; that boots a raw Image through
      extlinux on sda2 (boot-chainload.nix, u-boot-port.md). Built as the
      dmc12-disk-image-chainload output.
    '';
  };
}
