# NXP QorIQ T2080 (e6500) platform: minimal kernel, legacy uImage for U-Boot's
# bootm, serial console, no initrd modules. Shared by every T2080 board; the
# board module adds its kernel patches via hardware.qoriqT2080.kernelPatches
# and its device tree.
{ config, lib, pkgs, ... }:
let
  baseKernel = import ../../packages/linux-qoriq-t2080 {
    inherit pkgs lib;
    boardPatches = config.hardware.qoriqT2080.kernelPatches;
    # CONFIG_RUST (packages/kernel-rust-ppc64be), the default since 2026-11-23
    rust = config.hardware.qoriqT2080.rust;
  };
  # Also install a legacy uImage: U-Boot on PowerPC boots extlinux entries with
  # bootm, which needs a U-Boot image header rather than a bare vmlinux.
  kernel = baseKernel.overrideAttrs (o: {
    nativeBuildInputs = (o.nativeBuildInputs or [ ]) ++ [ pkgs.buildPackages.ubootTools ];
    postInstall = (o.postInstall or "") + ''
      ${pkgs.stdenv.cc.targetPrefix}objcopy -O binary $out/vmlinux vmlinux.bin
      gzip -n -9 < vmlinux.bin > vmlinux.bin.gz
      SOURCE_DATE_EPOCH=1 mkimage -A ppc -O linux -T kernel -C gzip -a 0 -e 0 \
        -n "Linux ${o.version}" -d vmlinux.bin.gz $out/uImage
    '';
  });
in {
  options.hardware.qoriqT2080.kernelPatches = lib.mkOption {
    type = lib.types.listOf lib.types.attrs;
    default = [ ];
    description = "Board kernel patches ({ name, patch }), applied to the shared T2080 kernel.";
  };
  options.hardware.qoriqT2080.rust = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "CONFIG_RUST in the T2080 kernel (the ppc64 big-endian series of packages/kernel-rust-ppc64be), for the kernel modules written in Rust (the fan driver of thermal/as7760-fan.nix needs it). Default since 2026-11-23 (proven on the AS7772: packages/kernel-rust-ppc64be/README.md); false gives the kernel without the series.";
  };
  config = {
  boot.kernelPackages = pkgs.linuxPackagesFor kernel;
  system.boot.loader.kernelFile = lib.mkForce "uImage";
  boot.kernelParams = [ "console=ttyS0,115200n8" "panic=10" "pcie_aspm.policy=performance" ];
  boot.initrd = {
    includeDefaultModules = false;
    availableKernelModules = [ ];
    kernelModules = [ ];
    compressor = "gzip";
  };
  # No PS/2 keyboard on this board; loop is built in, the fan driver is added by thermal/as7760-fan.nix.
  boot.kernelModules.atkbd = lib.mkForce false;
  boot.loader.grub.enable = false;
  boot.loader.systemd-boot.enable = false;
  boot.loader.efi.canTouchEfiVariables = false;
  # No pretend EFI/extlinux support on the factory 2014 U-Boot. The image
  # packager supplies legacy uImage/uInitrd for explicit bootm commands.
  boot.loader.external = {
    enable = true;
    installHook = pkgs.writeShellScript "as7760-no-firmware-write" ''
      echo 'AS7760: boot files must be staged using the workstation image builder.'
      echo 'No U-Boot environment or flash was changed.'
    '';
  };
  };
}
