# Annapurna Labs Alpine V2 (4x Cortex-A57): mainline kernel + al_eth, a board
# DTB compiled apart from the kernel, serial console. Boards set the DTS.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.alpineV2;
  linuxAlpine = import ../../packages/linux-alpine {
    inherit pkgs lib;
    boardConfig = cfg.kernelConfig;
    variant = cfg.kernelVariant;
  };
in
{
  options.hardware.alpineV2 = {
    dtbName = lib.mkOption {
      type = lib.types.str;
      example = "alpine-v2-dmc12";
      description = "Board DTB name (without .dtb); built from `dts`.";
    };
    dts = lib.mkOption {
      type = lib.types.path;
      description = "Board .dts; may #include \"alpine-v2.dtsi\" and dt-bindings.";
    };
    kernelVariant = lib.mkOption {
      type = lib.types.enum [ "trimmed" "lean" "minimal" ];
      # trimmed booted on DMC-12 u01 (2026-11-15); lean is the next run's.
      default = "lean";
      description = "packages/linux-alpine variant: trimmed (nixpkgs minus foreign SoCs), lean (+ per-family cuts), minimal (allnoconfig + explicit).";
    };
    kernelConfig = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      default = { };
      example = lib.literalExpression "with lib.kernel; { USB_SERIAL_CP210X = yes; }";
      description = "Board drivers on top of the minimal Alpine kernel (structuredExtraConfig).";
    };
    console = lib.mkOption {
      type = lib.types.str;
      default = "ttyS0,115200";
    };
  };

  config = {
    boot.kernelPackages = linuxAlpine.kernelPackages;
    boot.extraModulePackages = [ linuxAlpine.alEth ];
    boot.kernelModules = [ "al_eth" ];
    # Boot-critical drivers are built in (packages/linux-alpine), so the initrd
    # carries no modules; listing built-ins here would fail the initrd build.
    boot.initrd.availableKernelModules = lib.mkForce [ ];
    # The kernel, not U-Boot, unpacks the initrd (legacy uImage via the factory
    # loader, or raw via upstream U-Boot's booti), so any compressor it has works.
    boot.initrd.compressor = "zstd";
    boot.kernelParams = [
      "console=${cfg.console}"
      "earlycon=uart,mmio32,0xfd883000"
      "pci=realloc"
      "usbcore.autosuspend=-1"
    ];
    hardware.deviceTree = {
      enable = true;
      name = "amazon/${cfg.dtbName}.dtb";
      dtbSource = linuxAlpine.dtbs { name = cfg.dtbName; inherit (cfg) dts; };
    };
    system.build.alpineDtbs = config.hardware.deviceTree.dtbSource;
  };
}
