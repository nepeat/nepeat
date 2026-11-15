# Edgecore DMC-12-48R-AS0 48-port console server (Alpine V2 K2C-AB).
# First iteration: codeberg dma/dmc12-nixos (imported verbatim in acaaa03); kernel,
# DTS and al_eth now live on the shared layers. Hardware facts:
# ai_re/devices/dmc-12-48r-as0/vendor-tooling.md.
{ config, lib, pkgs, ... }:
{
  imports = [
    ../../platforms/alpine-v2.nix
    ./fan-control   # services.dmc12-fancontrol (CPLD PWM via I2C)
    ./dmc12-tools   # show/setled/setphyspeed/onieeeprom + boot oneshots
    ./cpld          # CPLD board reset, interrupt monitor
  ];

  hardware.alpineV2 = {
    dtbName = "alpine-v2-dmc12";
    dts = builtins.path {
      path = ../../../packages/linux-alpine/boards/dmc12/alpine-v2-dmc12.dts;
      name = "alpine-v2-dmc12.dts";
    };
  };

  services.dmc12-fancontrol.enable = true;
  services.dmc12-tools.enable = true;

  # Sensor labels and P1..P48 port links, from the factory image.
  environment.etc."sensors.d/dmc12_sensors".source =
    builtins.path { path = ./conf/dmc12_sensors.conf; name = "dmc12_sensors.conf"; };
  services.udev.extraRules = builtins.readFile ./conf/serial-ports.rules;
  users.groups.dialout = { };
}
