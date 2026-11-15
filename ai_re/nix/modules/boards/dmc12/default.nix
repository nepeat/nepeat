{ config, lib, pkgs, ... }:

let
  k = import ./kernel.nix { inherit pkgs lib; };
in
{
  imports = [
    ./uboot-menu
    ./fan-control    # services.dmc12-fancontrol (CPLD PWM via I2C)
    ./dmc12-tools    # services.dmc12-tools (LED, utils)
  ];

  config = {
    boot.kernel.enable = lib.mkForce true;
    boot.initrd.enable = lib.mkForce true;
    boot.kernelPackages = k.kernelPackages;
    boot.extraModulePackages = [ k.alEthModule ];
    boot.kernelModules = [ "al_eth" ];
    boot.initrd.compressor = "zstd";

    hardware.deviceTree = {
      enable = true;
      kernelPackage = k.kernelPackage;
      name = "amazon/alpine-v2-dmc12.dtb";
    };

    boot.kernelParams = [
      "console=ttyS0,115200"
      "earlycon=uart,mmio32,0xfd883000"
      "pci=realloc"
      "usbcore.autosuspend=-1"
    ];

    services.dmc12-fancontrol.enable = true;
    services.dmc12-tools.enable = true;

    # Add sensors.d + sensors config for temperatures, taken from the factory image.
    environment.etc."sensors.d/dmc12_sensors".text = builtins.readFile ./conf/dmc12_sensors.conf;

    # Add mapping for (P1..P48) serial ports, also taken from the factory image.
    services.udev.extraRules = builtins.readFile ./conf/serial-ports.rules;
  };
}
