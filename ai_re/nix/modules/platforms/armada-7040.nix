# Marvell Armada 7040 (AP806 + CP110) boards booted by a vendor (secure-boot
# fused) U-Boot: stock nixpkgs kernel, board DT compiled against the kernel's
# own marvell/*.dtsi, serial console on the AP UART, and an extlinux.conf on a
# FAT /boot that the factory U-Boot's `sysboot` reads (raw Image + initrd +
# DTB: not FIT-signature checked). The factory U-Boot applies its DT fixups
# (memory banks, ethernet MACs via aliases) to the DTB it loads.
# Boards: AS5114-48X (boards/as5114). The TN48M (nepeat/nix module/tn48m) is
# the same SoC and can move onto this module later.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.armada7040;
  buildPkgs = pkgs.buildPackages;
  dtbs = buildPkgs.runCommand "armada7040-board-dtbs" {
    nativeBuildInputs = [ buildPkgs.dtc ];
    kernelSrc = config.boot.kernelPackages.kernel.src;
    dts = cfg.dts;
  } ''
    set -euo pipefail
    mkdir -p inc dts
    tar xf "$kernelSrc" --strip-components=1 -C inc --wildcards '*/include/dt-bindings'
    tar xf "$kernelSrc" --strip-components=6 -C dts --wildcards '*/arch/arm64/boot/dts/marvell/*'
    ${buildPkgs.stdenv.cc}/bin/cpp -nostdinc -undef -x assembler-with-cpp \
      -I dts -I inc/include -o board.dts "$dts"
    mkdir -p $out/$(dirname ${cfg.dtbName})
    dtc -I dts -O dtb -o $out/${cfg.dtbName} board.dts
  '';
in {
  options.hardware.armada7040 = {
    dts = lib.mkOption {
      type = lib.types.path;
      description = "Board DTS; may #include the kernel's marvell/armada-70x0.dtsi files.";
    };
    dtbName = lib.mkOption {
      type = lib.types.str;
      example = "marvell/armada-7040-as5114.dtb";
      description = "Path of the compiled DTB inside the dtbs directory (extlinux FDT line).";
    };
  };

  config = {
    nixpkgs.hostPlatform = lib.mkDefault "aarch64-linux";
    boot.kernelParams = [ "console=ttyS0,115200n8" "earlycon=uart8250,mmio32,0xf0512000" ];
    boot.initrd.availableKernelModules = [ "ahci" "sd_mod" "ext4" "vfat" "nls_cp437" "nls_iso8859-1" ];
    boot.initrd.kernelModules = [ "mvpp2" "marvell" ];
    hardware.deviceTree = { enable = true; dtbSource = dtbs; name = cfg.dtbName; };
    boot.loader.grub.enable = false;
    boot.loader.generic-extlinux-compatible = { enable = true; configurationLimit = lib.mkDefault 5; };
  };
}
