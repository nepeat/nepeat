# DMC-12 kernel.
# thanks to:
# - Amazon for engineering the box, lolol
# - Amazon Fresh for liquidating new in box and completely wrapped factory units on eBay
# - Amazon + QNAP for open sourcing the kernels used in a QNAP NAS using the same Alpine V2 system
# - delroth@github for pulling the al_eth module from a QNAP NAS
{ pkgs, lib }:

let
  kernel = (pkgs.linuxPackages_latest.kernel.override {
    ignoreConfigErrors = true;

    structuredExtraConfig = with lib.kernel; {
      # Alpine V2 SoC modules, it's probably best to keep all of this hard baked in
      ARCH_ALPINE = lib.mkForce yes;
      ALPINE_MSI  = lib.mkForce yes;   # al_eth/AHCI MSI-X domain
      PCI_HOST_GENERIC = lib.mkForce yes;   # internal bridge (ecam-generic)
      PCIE_AL          = lib.mkForce yes;   # external bridge (pcie-al)
      PCIE_DW_HOST     = lib.mkForce yes;

      # Always guarantee storage modules are hard baked in
      ATA       = lib.mkForce yes;
      SATA_AHCI = lib.mkForce yes;
      EXT4_FS   = lib.mkForce yes;

      # Disable modules that we will never see on this hardware.
      DRM      = lib.mkForce no;        # Video: GPU / DRM
      FB       = lib.mkForce no;        # Video: framebuffer
      SOUND    = lib.mkForce no;        # Audio: ALSA / SoC / HDA
      WLAN     = lib.mkForce no;        # WLAN: 802.11 adapters
      CFG80211 = lib.mkForce no;        # WLAN: cfg80211 core
      MAC80211 = lib.mkForce no;        # WLAN: softmac stack
      WWAN     = lib.mkForce no;        # Cellular: WWAN modems
      IIO           = lib.mkForce no;   # industrial-IO ADCs (temp probes are hwmon)
      BT            = lib.mkForce no;   # Bluetooth core + BTUSB
      INFINIBAND    = lib.mkForce no;   # IB/RoCE HCAs + RDMA ULPs
      CAN           = lib.mkForce no;   # CAN bus
      MEDIA_SUPPORT = lib.mkForce no;   # V4L2/DVB/cameras/TV/IR/codecs — all of it

      # No keyboard/mouse/touch
      INPUT_TOUCHSCREEN = lib.mkForce no;
      INPUT_JOYSTICK    = lib.mkForce no;
      INPUT_TABLET      = lib.mkForce no;
      USB_SERIAL        = lib.mkForce yes;
      USB_SERIAL_CP210X = lib.mkForce yes;
    };
  }).overrideAttrs (old: {
    # DMC-12 DTS needs to be patched into the kernel source given that this is a cursed ARM device
    postPatch = (old.postPatch or "") + ''

      install -m 0644 ${./kernel/dts/amazon/alpine-v2-dmc12.dts} \
        arch/arm64/boot/dts/amazon/alpine-v2-dmc12.dts
      echo 'dtb-$(CONFIG_ARCH_ALPINE) += alpine-v2-dmc12.dtb' \
        >> arch/arm64/boot/dts/amazon/Makefile
    '';
  });

  # Bundle in the vendored al_eth module from QNAP + Amazon + extraction from delroth@github
  alEthModule = pkgs.stdenv.mkDerivation {
    pname = "al_eth";
    inherit (kernel) version;
    src = ./kernel/al_eth/src;

    hardeningDisable = [ "all" ];
    nativeBuildInputs = kernel.moduleBuildDependencies;

    buildPhase = ''
      runHook preBuild
      make -C ${kernel.dev}/lib/modules/${kernel.modDirVersion}/build \
        M=$(pwd) modules KCFLAGS=-DCONFIG_AL_ETH_ALPINE_V2
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      make -C ${kernel.dev}/lib/modules/${kernel.modDirVersion}/build \
        M=$(pwd) INSTALL_MOD_PATH=$out modules_install
      runHook postInstall
    '';

    meta.description =
      "Annapurna Labs al_eth NIC as an out-of-tree module for the DMC-12 kernel";
  };
in {
  kernelPackage = kernel;
  kernelPackages = pkgs.linuxPackagesFor kernel;
  inherit alEthModule;
  # The fully-resolved .config nixpkgs generated, for inspection.
  configFile = kernel.configfile;
}
