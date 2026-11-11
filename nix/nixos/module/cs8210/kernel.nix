# CS8210-32X kernel: nixpkgs' latest mainline plus a board DTS, one small SPI
# patch and the out-of-tree al_eth NIC driver. No vendor kernel fork.
#
# Structure borrowed from https://codeberg.org/dma/dmc12-nixos (the Alpine v2
# K2C-AB sibling of this board's Alpine v3 K2X-N CPU card).
#
# `buildSystem`: when set to something other than the host platform (e.g.
# "x86_64-linux"), the kernel and al_eth are cross-compiled from there instead
# of being built natively/under emulation. The result is still an aarch64
# kernel; only the store paths differ from a native build.
{
  pkgs,
  lib,
  buildSystem ? null,
}:

let
  hostSystem = pkgs.stdenv.hostPlatform.system;

  kpkgs =
    if buildSystem == null || buildSystem == hostSystem then
      pkgs
    else
      import pkgs.path {
        localSystem = buildSystem;
        crossSystem = hostSystem;
      };

  baseKernel = kpkgs.linuxPackages_latest.kernel;

  # Patches on top of the upstream ipd source (kernel API drift, etc.).
  ipdPatches = [
    # Old-userspace ABI (ipd 0.12.x, what the CS8210's 2020 vendor SDK was
    # built against) behind the module parameter legacy_abi=1. Without the
    # parameter the driver behaves as upstream.
    ./kernel/patches/ipd-legacy-abi.patch
    ./kernel/patches/ipd-kernel-compat.patch
    # arm64: wmb() before the TX/RX descriptor doorbells (CPU TX stall).
    ./kernel/patches/ipd-arm64-doorbell-wmb.patch
    # ethtool speed/duplex so 802.3ad bonding can use the fpp ports.
    ./kernel/patches/ipd-ethtool-link-ksettings.patch
    ./kernel/patches/ipd-rx-gro-napi.patch
    # Round-2 performance patches (Fable, 2026-11-11) are NOT applied:
    #   ipd-tx-flow-control-napi.patch, ipd-rx-page-pool-refill.patch,
    #   ipd-tx-sg-gso-optin.patch
    # They gave 2.2-2.3 Gb/s each way, but an SDK restart under traffic
    # soft-locked CPU 0 in the SDK's PORT_BRUP thread holding rtnl (hard
    # power-off needed), most likely the ring teardown/re-init path. Keep
    # them out until that is understood. See PROGRESS 2026-11-11 (z).
  ];

  kernel =
    (baseKernel.override {
      ignoreConfigErrors = true;

      kernelPatches = (baseKernel.kernelPatches or [ ]) ++ [
        {
          name = "spi-dw-mmio-allow-irqless";
          patch = ./kernel/patches/spi-dw-mmio-allow-irqless.patch;
        }
      ];

      structuredExtraConfig = with lib.kernel; {
        # Alpine SoC plumbing: keep it built in.
        ARCH_ALPINE = lib.mkForce yes;
        ALPINE_MSI = lib.mkForce yes; # MSI-X domain for al_eth / external PCIe
        PCI_HOST_GENERIC = lib.mkForce yes; # internal bus (ecam-generic)
        PCIE_AL = lib.mkForce yes; # external ports (pcie-al)
        PCIE_DW_HOST = lib.mkForce yes;

        # Console and root disk never depend on a module.
        SERIAL_8250 = lib.mkForce yes;
        SERIAL_8250_CONSOLE = lib.mkForce yes;
        SERIAL_OF_PLATFORM = lib.mkForce yes;
        ATA = lib.mkForce yes;
        SATA_AHCI = lib.mkForce yes;
        EXT4_FS = lib.mkForce yes;

        # Board buses: DesignWare I2C + SPI, boot flash, TPM.
        I2C_DESIGNWARE_CORE = lib.mkForce yes;
        I2C_DESIGNWARE_PLATFORM = lib.mkForce yes;
        I2C_MUX = lib.mkForce yes;
        I2C_MUX_PCA954x = lib.mkForce yes;
        SPI_DESIGNWARE = lib.mkForce yes;
        SPI_DW_MMIO = lib.mkForce yes;
        MTD = lib.mkForce yes;
        MTD_SPI_NOR = lib.mkForce yes;
        TCG_TPM = lib.mkForce yes;
        TCG_TIS_SPI = lib.mkForce yes;

        # Things this box will never have.
        DRM = lib.mkForce no;
        FB = lib.mkForce no;
        SOUND = lib.mkForce no;
        WLAN = lib.mkForce no;
        CFG80211 = lib.mkForce no;
        MAC80211 = lib.mkForce no;
        WWAN = lib.mkForce no;
        IIO = lib.mkForce no;
        BT = lib.mkForce no;
        INFINIBAND = lib.mkForce no;
        CAN = lib.mkForce no;
        MEDIA_SUPPORT = lib.mkForce no;
        INPUT_TOUCHSCREEN = lib.mkForce no;
        INPUT_JOYSTICK = lib.mkForce no;
        INPUT_TABLET = lib.mkForce no;
      };
    });

  # The board DTB is compiled on its own, against the SoC .dtsi and dt-bindings
  # headers from the same kernel source, so a DTS edit does not rebuild the
  # kernel.
  dtsIncludes = pkgs.runCommand "cs8210-dts-includes-${kernel.version}" { } ''
    mkdir -p $out
    tar -xf ${kernel.src} -C $out --strip-components=1 --wildcards \
      '*/arch/arm64/boot/dts/amazon/*' \
      '*/include/dt-bindings/interrupt-controller/*'
  '';

  dtbs =
    pkgs.runCommand "cs8210-dtbs-${kernel.version}"
      {
        nativeBuildInputs = [
          pkgs.buildPackages.dtc
          pkgs.buildPackages.stdenv.cc
        ];
      }
      ''
        mkdir -p $out/amazon
        $CC -E -nostdinc -undef -D__DTS__ -x assembler-with-cpp \
          -I ${dtsIncludes}/arch/arm64/boot/dts/amazon \
          -I ${dtsIncludes}/include \
          ${./kernel/dts/amazon/alpine-v3-cs8210.dts} -o cs8210.pp.dts
        dtc -I dts -O dtb -@ -o $out/amazon/alpine-v3-cs8210.dtb cs8210.pp.dts
      '';

  # Vendored Annapurna Labs al_eth (QNAP source drop, via delroth and
  # dmc12-nixos), patched here for the Alpine v3 integrated NIC.
  alEthModule = kpkgs.stdenv.mkDerivation {
    pname = "al_eth";
    inherit (kernel) version;
    src = ./kernel/al_eth/src;

    # makeFlags must reach bash as an array: the kernel's flags contain
    # spaces-by-$(space) tricks that do not survive string flattening.
    __structuredAttrs = true;
    strictDeps = true;

    hardeningDisable = [ "all" ];
    nativeBuildInputs = kernel.moduleBuildDependencies;

    makeFlags = kernel.commonMakeFlags ++ [
      "-C"
      "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
      "KCFLAGS=-DCONFIG_AL_ETH_ALPINE_V2"
    ];

    buildPhase = ''
      runHook preBuild
      make "''${makeFlags[@]}" M=$(pwd) modules
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      make "''${makeFlags[@]}" M=$(pwd) INSTALL_MOD_PATH=$out modules_install
      runHook postInstall
    '';

    meta.description = "Annapurna Labs al_eth NIC, out-of-tree, for the CS8210 kernel";
  };
  # Marvell's GPL "ipd" PCIe driver for Innovium/Teralynx switch ASICs
  # (github.com/Marvell-switching/mrvl-teralynx, the driver SONiC builds).
  # It gives the userspace switch SDK register access, DMA rings and
  # interrupts, and creates the front-panel netdevs. The CS8210's ASIC is
  # 1d98:1b58 rev 0x10 (Teralynx 7).
  ipdModule = kpkgs.stdenv.mkDerivation {
    pname = "ipd";
    version = "0.16.2-${kernel.version}";

    src = kpkgs.fetchFromGitHub {
      owner = "Marvell-switching";
      repo = "mrvl-teralynx";
      rev = "53d8dcf6270719e7bdc42619e182015bb2245ac7";
      hash = "sha256-fbBVgWRFxnBuWj9LGsSNr4qmD1xDhnXjGV7yLjC3u08=";
    };
    sourceRoot = "source/drivers/ipd";
    patches = ipdPatches;

    __structuredAttrs = true;
    strictDeps = true;
    hardeningDisable = [ "all" ];
    nativeBuildInputs = kernel.moduleBuildDependencies;

    makeFlags = kernel.commonMakeFlags ++ [
      "-C"
      "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    ];

    buildPhase = ''
      runHook preBuild
      make "''${makeFlags[@]}" M=$(pwd) modules
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      make "''${makeFlags[@]}" M=$(pwd) INSTALL_MOD_PATH=$out modules_install
      runHook postInstall
    '';

    meta.description = "Marvell/Innovium Teralynx switch PCIe driver (ipd), out-of-tree";
  };
in
{
  inherit ipdModule;
  kernelPackage = kernel;
  kernelPackages = kpkgs.linuxPackagesFor kernel;
  inherit dtbs;
  inherit alEthModule;
  configFile = kernel.configfile;
}
