# Edgecore DMC-12-48R-AS0 48-port console server (Alpine V2 K2C-AB).
# First iteration: codeberg dma/dmc12-nixos (imported verbatim in acaaa03); kernel,
# DTS and al_eth now live on the shared layers. Hardware facts:
# ai_re/devices/dmc-12-48r-as0/vendor-tooling.md.
{ config, lib, pkgs, ... }:
{
  imports = [
    ../../platforms/alpine-v2.nix
    ../../thermal/dmc12-fan.nix   # hardware.dmc12Fan: fand (rust flake, root flake only)
    ./dmc12-tools   # show/setled/setphyspeed/onieeeprom + boot oneshots
    ./cpld          # CPLD board reset, interrupt monitor
    ./monitoring.nix   # node_exporter :9100 + dmc12-metrics (read-only)
    ./uboot-env     # fw_printenv + dmc12-uboot-env (our U-Boot's env window in SPI)
    ./firmware      # dmc12-firmware: our U-Boot into the IPXE slots + BOOT_MODE, from Linux
    ./console-server.nix   # consoled: the 48 serial ports over ssh
    ./trial-boot.nix       # board hooks for profile-trial-boot (only when a system enables it)
  ];

  # Stage 1 is plain ext4 on the SSD: no LVM, no LUKS unlock via TPM2/FIDO2 (they pull in
  # lvm2, cryptsetup, openssl, libfido2: ~1/3 of the initrd). zstd -19: smaller /boot copies.
  boot.initrd.services.lvm.enable = lib.mkDefault false;
  boot.initrd.systemd.tpm2.enable = lib.mkDefault false;
  boot.initrd.systemd.fido2.enable = lib.mkDefault false;
  boot.initrd.compressorArgs = lib.mkDefault [ "-19" "-T0" ];

  hardware.alpineV2 = {
    # allnoconfig + explicit (packages/linux-alpine); lean is the fallback.
    kernelVariant = lib.mkDefault "minimal";   # hosts needing more (TUN/WireGuard VPNs): "lean"
    # Board drivers, from what the factory kernel bound on u01 (2026-11-15).
    kernelConfig = with lib.kernel; {
      USB_SUPPORT = yes;
      USB = yes;
      USB_PCI = yes;
      USB_EHCI_HCD = yes; # Pericom 12d8:400f behind PCIe port 0
      USB_EHCI_PCI = yes;
      USB_OHCI_HCD = yes; # 12d8:400e x2
      USB_OHCI_HCD_PCI = yes;
      USB_SERIAL = yes;
      USB_SERIAL_CP210X = yes; # 12x CP2108 = P1..P48
      USB_STORAGE = yes; # for a stick, handy on a console server
      EEPROM_AT24 = yes; # board EEPROM 7-0051, SPD 0-0057
      NVMEM = yes;
      NVMEM_SYSFS = yes;
      SENSORS_LM75 = yes; # lm75 (uC) 5-004a, tmp75 7-0048/49
      RTC_CLASS = yes;
      RTC_DRV_DS1307 = yes; # ds1339 2-0068
      TCG_TPM = yes;
      TCG_TIS_SPI = yes; # SLB9670 on SPI SS2 (no I2C TPM: research-open-items.md 6)
    };
    dtbName = "alpine-v2-dmc12";
    dts = builtins.path {
      path = ../../../packages/linux-alpine/boards/dmc12/alpine-v2-dmc12.dts;
      name = "alpine-v2-dmc12.dts";
    };
  };

  # fand when the monorepo rust flake is passed in (root ai_re flake), else
  # the ported factory script; never both on the CPLD's 0x50.
  services.dmc12-tools.enable = true;

  # Sensor labels and P1..P48 port links, from the factory image.
  environment.etc."sensors.d/dmc12_sensors".source =
    builtins.path { path = ./conf/dmc12_sensors.conf; name = "dmc12_sensors.conf"; };
  services.udev.extraRules = builtins.readFile ./conf/serial-ports.rules;
  users.groups.dialout = { };
}
