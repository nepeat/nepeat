# Annapurna Labs Alpine V2 (4x Cortex-A57): mainline kernel + al_eth, a board
# DTB compiled apart from the kernel, serial console. Boards set the DTS.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.alpineV2;
  linuxAlpine = import ../../packages/linux-alpine {
    inherit pkgs lib;
    boardConfig = cfg.kernelConfig;
    variant = cfg.kernelVariant;
    rust = cfg.kernelRust;
    rustNet = cfg.kernelRustNet;
    debug = cfg.kernelDebug;
  };
  alEthRust = linuxAlpine.kernelPackages.callPackage ../../packages/al-eth/rust-module.nix { };
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
    kernelRust = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "CONFIG_RUST in the minimal kernel, to load modules written in Rust (trimmed and lean have it from nixpkgs). Another kernel derivation; packages/kernel-rust/README.md.";
    };
    kernelRustNet = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "The net bindings patch (packages/kernel-rust/0001-rust-net-bindings-and-helpers.patch) in the minimal Rust kernel, for Rust NIC drivers (the Rust al_eth). Needs kernelRust; another kernel derivation.";
    };
    kernelDebug = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Test-kernel debug options (linux-alpine debug: KFENCE, SLUB debug, lockdep, DEBUG_ATOMIC_SLEEP, DEBUG_OBJECTS, DMA-API debug, kmemleak, DEBUG_NET with refcount trackers, Rust debug assertions). Needs kernelRust (minimal variant); boot with slub_debug=FZPU. For trial boots, not for a default generation.";
    };
    ethDriver = lib.mkOption {
      type = lib.types.enum [ "c" "rust" ];
      default = "c";
      description = "al_eth implementation: the vendor C module (packages/al-eth, every mode) or the Rust one (packages/al-eth/rust-module.nix: RGMII fixed link only, i.e. the DMC-12; refuses other boards' modes at probe). Same module name. \"rust\" turns on kernelRust and kernelRustNet.";
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
    boot.extraModulePackages = [ (if cfg.ethDriver == "rust" then alEthRust else linuxAlpine.alEth) ];
    hardware.alpineV2.kernelRust = lib.mkIf (cfg.ethDriver == "rust") true;
    hardware.alpineV2.kernelRustNet = lib.mkIf (cfg.ethDriver == "rust") true;
    # The Rust al_eth under the test name al_eth_rs, for this system's kernel (needs
    # kernelRustNet): insmod beside the C al_eth (devices/dmc-12-48r-as0/nic-swap.sh).
    system.build.alEthRs = linuxAlpine.kernelPackages.callPackage ../../packages/al-eth/rust-module.nix { moduleName = "al_eth_rs"; };
    # The same with every register access recorded (debugfs al_eth_rs/trace), for diffing against the C's trace.
    system.build.alEthRsTrace = linuxAlpine.kernelPackages.callPackage ../../packages/al-eth/rust-module.nix { moduleName = "al_eth_rs"; trace = true; };
    boot.kernelModules = [ "al_eth" ];
    # Boot-critical drivers are built in (packages/linux-alpine), so the initrd
    # carries no modules; listing built-ins here would fail the initrd build.
    boot.initrd.availableKernelModules = lib.mkForce [ ];
    # NixOS's default initrd module is dm_mod (LVM); the minimal kernel has no
    # device mapper and nothing here uses LVM.
    boot.initrd.includeDefaultModules = lib.mkIf (cfg.kernelVariant == "minimal") false;
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
