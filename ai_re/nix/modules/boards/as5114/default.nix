# Edgecore AS5114-48X (S/N 511448X2127097 in the lab): Armada 7040, 8 GiB, SATA SSD,
# Prestera Aldrin2 behind an Armada 385 co-CPU (mainline prestera_pci + linux-firmware
# FW 4.1). Notes: ai_re/devices/edge-core-as5114-48x (PROGRESS.md, a385-pcie.md).
# Basic port: console, mgmt, SSD, SPI env access, sensors; the front panel is left
# unconfigured (prestera netdevs come up admin-down).
{ config, lib, pkgs, ... }:
let
  prestera-firmware = pkgs.runCommand "prestera-firmware" { } ''
    mkdir -p $out/lib/firmware/mrvl
    cp -r ${pkgs.linux-firmware}/lib/firmware/mrvl/prestera $out/lib/firmware/mrvl/
  '';
in {
  imports = [ ../../platforms/armada-7040.nix ];

  hardware.armada7040 = {
    dts = builtins.path { path = ./armada-7040-as5114.dts; name = "armada-7040-as5114.dts"; };
    dtbName = "marvell/armada-7040-as5114.dtb";
  };

  hardware.firmware = [ prestera-firmware ];
  boot.kernelModules = [ "prestera_pci" "at24" "lm75" "spi_nor" ];

  # mtd1 = U-Boot env (64 KiB, 64 KiB sectors), as in ONIE's /etc/fw_env.config.
  environment.etc."fw_env.config".text = ''
    /dev/mtd1 0x0 0x10000 0x10000
  '';
  environment.systemPackages = [ pkgs.ubootTools ];
}
