# Mainline kernel for Annapurna Labs Alpine boards, plus board DTBs compiled
# on their own against the same source, so a DTS edit doesn't rebuild the kernel.
# Pattern and SoC config from the CS8210 port (nepeat/nix/nixos/module/cs8210,
# Alpine v3) and codeberg dma/dmc12-nixos (Alpine v2).
#   linuxAlpine = import ./linux-alpine { inherit pkgs lib; };
#   linuxAlpine.kernelPackages, linuxAlpine.alEth, linuxAlpine.dtbs { name; dts; }
# TODO (owner, 2026-11-15): this is nixpkgs' full config (thousands of modules).
# Next: allnoconfig + explicit enables like packages/linux-qoriq-t2080, using
# the drivers the first DMC-12 boot actually binds (lsmod + /sys/bus/*/drivers).
{ pkgs, lib, kernelPackages ? pkgs.linuxPackages_latest }:
let
  kernel = kernelPackages.kernel.override {
    ignoreConfigErrors = true;
    structuredExtraConfig = with lib.kernel; {
      # Alpine SoC plumbing: built in.
      ARCH_ALPINE = lib.mkForce yes;
      ALPINE_MSI = lib.mkForce yes; # MSI-X domain for al_eth / external PCIe
      PCI_HOST_GENERIC = lib.mkForce yes; # internal bus (ecam-generic)
      PCIE_AL = lib.mkForce yes; # external ports (pcie-al)
      PCIE_DW_HOST = lib.mkForce yes;
      # Console, root disk and board buses never depend on a module.
      SERIAL_8250 = lib.mkForce yes;
      SERIAL_8250_CONSOLE = lib.mkForce yes;
      SERIAL_OF_PLATFORM = lib.mkForce yes;
      ATA = lib.mkForce yes;
      SATA_AHCI = lib.mkForce yes;
      SCSI = lib.mkForce yes;
      BLK_DEV_SD = lib.mkForce yes;
      EXT4_FS = lib.mkForce yes;
      # RAM boots (profile-netboot-root): squashfs store + overlay, no initrd modules.
      BLK_DEV_LOOP = lib.mkForce yes;
      SQUASHFS = lib.mkForce yes;
      SQUASHFS_ZSTD = lib.mkForce yes;
      OVERLAY_FS = lib.mkForce yes;
      I2C_CHARDEV = lib.mkForce yes;
      I2C_DESIGNWARE_CORE = lib.mkForce yes;
      I2C_DESIGNWARE_PLATFORM = lib.mkForce yes;
      I2C_MUX = lib.mkForce yes;
      I2C_MUX_PCA954x = lib.mkForce yes;
      SPI_DESIGNWARE = lib.mkForce yes;
      SPI_DW_MMIO = lib.mkForce yes;
      MTD = lib.mkForce yes;
      MTD_SPI_NOR = lib.mkForce yes;
      GPIO_PL061 = lib.mkForce yes;
      # sp805: the platform restart path (firmware PSCI v0.1 has no SYSTEM_RESET).
      WATCHDOG = lib.mkForce yes;
      ARM_SP805_WATCHDOG = lib.mkForce yes;
      TCG_TPM = lib.mkForce yes;
      TCG_TIS_SPI = lib.mkForce yes;
      # Things these boxes will never have.
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
  };
  kp = pkgs.linuxPackagesFor kernel;

  dtsIncludes = pkgs.runCommand "alpine-dts-includes-${kernel.version}" { } ''
    mkdir -p $out
    tar -xf ${kernel.src} -C $out --strip-components=1 --wildcards \
      '*/arch/arm64/boot/dts/amazon/*' \
      '*/include/dt-bindings/*'
  '';
in
{
  inherit kernel;
  kernelPackages = kp;
  alEth = kp.callPackage ../al-eth { };
  # One board DTB in an `amazon/` dir, the layout hardware.deviceTree expects.
  dtbs = { name, dts }:
    pkgs.runCommand "${name}-dtbs-${kernel.version}" {
      nativeBuildInputs = [ pkgs.buildPackages.dtc pkgs.buildPackages.stdenv.cc ];
    } ''
      mkdir -p $out/amazon
      $CC -E -nostdinc -undef -D__DTS__ -x assembler-with-cpp \
        -I ${dtsIncludes}/arch/arm64/boot/dts/amazon \
        -I ${dtsIncludes}/include \
        ${dts} -o board.pp.dts
      dtc -I dts -O dtb -@ -W no-unique_unit_address -o $out/amazon/${name}.dtb board.pp.dts
    '';
}
