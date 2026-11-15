# Edgecore DMC-12-48R-AS0 48-port console server (Alpine V2 K2C-AB).
# First iteration: codeberg dma/dmc12-nixos (imported verbatim in acaaa03); kernel,
# DTS and al_eth now live on the shared layers. Hardware facts:
# ai_re/devices/dmc-12-48r-as0/vendor-tooling.md.
{ config, lib, pkgs, ... }:
{
  imports = [
    ../../platforms/alpine-v2.nix
    ../../thermal/dmc12-fan.nix   # hardware.dmc12Fan: fand (rust flake) or the python script
    ./fan-control   # services.dmc12-fancontrol (CPLD PWM via I2C)
    ./dmc12-tools   # show/setled/setphyspeed/onieeeprom + boot oneshots
    ./cpld          # CPLD board reset, interrupt monitor
    ./monitoring.nix   # node_exporter :9100 + dmc12-metrics (read-only)
  ];

  hardware.alpineV2 = {
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
      TCG_TIS_I2C_INFINEON = yes; # 3-0020
    };
    dtbName = "alpine-v2-dmc12";
    dts = builtins.path {
      path = ../../../packages/linux-alpine/boards/dmc12/alpine-v2-dmc12.dts;
      name = "alpine-v2-dmc12.dts";
    };
  };

  # fand when the monorepo rust flake is passed in (root ai_re flake), else
  # the ported factory script; never both on the CPLD's 0x50.
  services.dmc12-fancontrol.enable = config.hardware.dmc12Fan.controller == "python";
  services.dmc12-tools.enable = true;

  # Sensor labels and P1..P48 port links, from the factory image.
  environment.etc."sensors.d/dmc12_sensors".source =
    builtins.path { path = ./conf/dmc12_sensors.conf; name = "dmc12_sensors.conf"; };
  services.udev.extraRules = builtins.readFile ./conf/serial-ports.rules;
  users.groups.dialout = { };
}
