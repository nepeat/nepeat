# Celestica CS8210-32X ("Briggs") hardware profile.
#
# 32x400G Innovium/Teralynx switch with an Annapurna Labs Alpine v3 K2X-N CPU
# card (16x Cortex-A72, 8 GiB). Adapted from https://codeberg.org/dma/dmc12-nixos,
# which targets the Alpine v2 K2C-AB card in the DMC12-48R-AS0. What differs:
#
#   * Board DTS is built on mainline alpine-v3.dtsi (see kernel/dts).
#   * ./uboot-menu is DMC12's bootmenu builder reworked for this U-Boot
#     (2018.05): raw Image + `booti`, mkimage-wrapped initrd, files on the
#     root partition. It hangs off U-Boot's `nos_bootcmd`; if it fails the
#     stock bootcmd falls through to ONIE in SPI flash.
#   * ./fan-control and ./tools are this board's counterparts of DMC12's
#     fan-control and dmc12-tools (different CPLDs, registers and buses).
#     DMC12's console-server udev rules have no equivalent here.
#
# RE notes: nepeat/ai_re/devices/cs8210-32x/PROGRESS.md
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.hardware.cs8210;
  k = import ./kernel.nix {
    inherit pkgs lib;
    buildSystem = cfg.kernelBuildSystem;
  };
in
{
  imports = [
    ./uboot-menu
    ./fan-control # services.cs8210-fancontrol
    ./tools # services.cs8210-tools
    ./vendor-sdk # hardware.cs8210.vendorSdk
  ];

  options.hardware.cs8210 = {
    kernelBuildSystem = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "x86_64-linux";
      description = ''
        Cross-compile the kernel and al_eth from this system instead of
        building them as native aarch64-linux derivations. Useful when the
        only aarch64 builder available is binfmt emulation.
      '';
    };

    pcieErrorsAsAllOnes = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Make the three external PCIe root ports answer failed or unanswered
        reads with 0xFFFFFFFF instead of an AXI slave error. Firmware leaves
        the DesignWare "AMBA error response" register (root-port config space
        0x8D0, the Annapurna HAL's enable_axi_slave_err_resp) at 1, which
        turns any such read -- a dead ASIC, a BAR with memory decode off -- into
        an asynchronous SError and a kernel panic. 0 is the HAL's documented
        alternative. Verified 2026-11-10: a BAR read on a device with memory
        decode off returns 0xffffffff instead of panicking.
      '';
    };

    alEth.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Build and load the out-of-tree al_eth management NIC driver.";
    };
  };

  config = {
    nixpkgs.hostPlatform = "aarch64-linux";

    boot.kernelPackages = k.kernelPackages;
    # ipd is loaded by udev through its PCI alias once the ASIC enumerates.
    boot.extraModulePackages =
      lib.optional cfg.alEth.enable k.alEthModule
      ++ lib.optional config.hardware.cs8210.vendorSdk.enable k.ipdModule;
    # lm75 does not get autoloaded for the DT-described sensors here.
    # spi-mux + mux-gpio sit between the aux SPI controller and the TPM /
    # FPGA flash (see the DTS); nothing autoloads them early enough.
    boot.kernelModules = [
      "lm75"
      "mux-gpio"
      "spi-mux"
    ]
    ++ lib.optional cfg.alEth.enable "al_eth";

    # The root disk controller is built in; don't let the generic module list
    # ask for drivers this kernel was configured without.
    boot.initrd.includeDefaultModules = false;

    # Generic profiles (all-hardware via the sd-image module, etc.) list initrd
    # modules for hardware this trimmed kernel does not build (DRM bridges and
    # friends). Let the initrd module closure skip what is missing.
    nixpkgs.overlays = [
      (final: prev: {
        makeModulesClosure = args: prev.makeModulesClosure (args // { allowMissing = true; });
      })
    ];
    boot.initrd.compressor = "zstd";

    hardware.deviceTree = {
      enable = true;
      kernelPackage = k.kernelPackage;
      dtbSource = k.dtbs;
      name = "amazon/alpine-v3-cs8210.dtb";
    };

    boot.kernelParams = [
      "console=ttyS0,115200n8"
      # No baud here on purpose: an explicit baud makes 8250 earlycon program
      # a divisor for a 1.8432 MHz clock, and this UART runs from 500 MHz.
      "earlycon=uart8250,mmio32,0xfd883000"
      "pci=realloc"
      # SATA link power management makes the Alpine AHCI drop the link
      # (SError PHYRdyChg/CommWake, then every queued command times out,
      # roughly once a minute under load). The stock 4.9 kernel never
      # enabled it. 1 = max_performance.
      "ahci.mobile_lpm_policy=1"
    ];

    systemd.services.cs8210-pcie-errors = lib.mkIf cfg.pcieErrorsAsAllOnes {
      description = "CS8210: PCIe read errors return all-ones instead of SError";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-modules-load.service" ];
      before = [ "cs8210-switch.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        for port in 0001:00:00.0 0002:00:00.0 0004:00:00.0; do
          if [ -e /sys/bus/pci/devices/$port ]; then
            ${pkgs.pciutils}/bin/setpci -s $port 0x8d0.l=0
            echo "$port AMBA error response: $(${pkgs.pciutils}/bin/setpci -s $port 0x8d0.l)"
          fi
        done
      '';
    };

    # SSD wear: journald writes through mmap, and every sync writes back all
    # the pages a few entries touched (entry, data, hash and array objects
    # spread over the file) plus an offline/online header cycle. At 15 s
    # that measured 6-13 MB/min of SSD writes for ~7 messages/min
    # (2026-11-10, wchar 0, write_bytes only). Back to the 5 min default;
    # the serial console (logged on the desktop) still catches panics.
    services.journald.settings.Journal.SyncIntervalSec = "5m";
    # Each journal entry costs ~30 rewritten pages per writeback window, so
    # trickle messages are what wears the SSD (2.2 MB/min at ~7 msg/min, 9 KB
    # per extra message in a burst). The biggest trickle was dhcpcd logging
    # every IPv6 route-information change at info level (~4/min here, two
    # routers announcing the same ULA route). Keep notice and above.
    systemd.services.dhcpcd.serviceConfig.LogLevelMax = "notice";

    services.cs8210-fancontrol.enable = lib.mkDefault true;
    services.cs8210-tools.enable = lib.mkDefault true;
    hardware.cs8210.vendorSdk.enable = lib.mkDefault true;

    # x86 leftovers from the shared base.
    hardware.cpu.intel.updateMicrocode = lib.mkForce false;
    hardware.cpu.amd.updateMicrocode = lib.mkForce false;

    environment.systemPackages = with pkgs; [
      pciutils
      i2c-tools
      lm_sensors
      mtdutils
      ubootTools
      tpm2-tools
      ethtool
      dtc
    ];

    # fw_printenv / fw_setenv against the SPI flash "uboot-env" partition.
    # Same geometry ONIE uses (/dev/mtd1, offset 0, env 0x4000, sector 0x40000).
    environment.etc."fw_env.config".text = ''
      /dev/mtd1 0x0 0x4000 0x40000
    '';
  };
}
