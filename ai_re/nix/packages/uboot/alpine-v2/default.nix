{ pkgs, uboots }:
# Upstream U-Boot (nixpkgs buildUBoot, 2026.10) for Annapurna Labs Alpine V2
# boards, started from RAM by the vendor U-Boot 2015.07 ("bootm" of the
# uImage, or "go"). Patches: the "alpine" + "alpine-v2" + "dmc12" scopes of the shared
# series in ../default.nix. Plan, load addresses and test status:
# devices/dmc-12-48r-as0/u-boot-port.md.
#
# The board DTS is the Linux one (packages/linux-alpine/boards/<board>/),
# copied into arch/arm/dts at build time; U-Boot-only nodes live in the
# patch series' <board>-u-boot.dtsi.
let
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  patches = uboots.lib.patchesFor [ "alpine" "alpine-v2" "dmc12" ];
  # Link/load address: the vendor "loadaddr". Below it: the vendor U-Boot
  # (relocated to 0x03F0A000, stack ~0x03B04CD0), its DTB copy at
  # 0x04078000, firmware at 0x0 and 0x02000000. Above it: vendor initrd
  # staging at 0x0fd00000. The image (code + BSS + 512 KiB early stack) is
  # well under 2 MiB; tests.nix checks that.
  loadAddr = "0x08000000";

  dmc12Dts = builtins.path {
    path = ../../linux-alpine/boards/dmc12/alpine-v2-dmc12.dts;
    name = "alpine-v2-dmc12.dts";
  };

  # Kconfig of dmc12-selfinit on top of alpine_v2_dmc12_defconfig.
  selfinitConfig = ''
    CONFIG_LOCALVERSION="-dmc12-selfinit"
    CONFIG_PCIE_ALPINE_V2=y
    CONFIG_CMD_ALPINE_V2_PCIE=y
    CONFIG_ALPINE_V2_MUIO_INIT=y
    CONFIG_ALPINE_V2_GPIO_INIT=y
    CONFIG_ALPINE_V2_CCU_IO_COHERENCY=y
    CONFIG_CMD_ALPINE_V2=y
    CONFIG_MTD=y
    # CONFIG_CMD_MTD is not set
    # CONFIG_CMD_MTDPARTS is not set
    # CONFIG_MTD_NOR_FLASH is not set
    # CONFIG_DM_MTD is not set
    CONFIG_SPI=y
    CONFIG_DM_SPI=y
    CONFIG_DESIGNWARE_SPI=y
    CONFIG_SPI_FLASH=y
    CONFIG_DM_SPI_FLASH=y
    CONFIG_SPI_FLASH_MACRONIX=y
    # CONFIG_SPI_FLASH_LOCK is not set
    # CONFIG_SPI_FLASH_UNLOCK_ALL is not set
    # CONFIG_SPI_FLASH_SFDP_SUPPORT is not set
    # CONFIG_SPI_FLASH_SOFT_RESET is not set
    # CONFIG_SPI_FLASH_BAR is not set
    # CONFIG_SPI_FLASH_MTD is not set
    CONFIG_SPI_FLASH_READ_ONLY=y
    CONFIG_CMD_SF=y
    # CONFIG_CMD_SF_TEST is not set
    # CONFIG_CMD_SPI is not set
    # Persistent env in CS0, redundant, in the one writable window (patch 0019):
    # 0xf90000-0xfaffff, the blank tail of the BOOT_APP 1 slot (its object ends at
    # 0xb53a11, the slot at 0xfb0000). The factory env pair at 0xfd0000/0xfe0000
    # stays the vendor U-Boot's. Bad CRC / blank: the compiled-in default env.
    CONFIG_SPI_FLASH_WRITE_WINDOW=y
    CONFIG_SPI_FLASH_WRITE_WINDOW_BUS=0
    CONFIG_SPI_FLASH_WRITE_WINDOW_CS=0
    CONFIG_SPI_FLASH_WRITE_WINDOW_START=0xf90000
    CONFIG_SPI_FLASH_WRITE_WINDOW_SIZE=0x20000
    CONFIG_SPI_FLASH_WRITE_WINDOW_JEDEC=0xc22538
    CONFIG_SPI_FLASH_WRITE_WINDOW_CHIP_SIZE=0x1000000
    # CONFIG_ENV_IS_NOWHERE is not set
    CONFIG_ENV_IS_IN_SPI_FLASH=y
    CONFIG_ENV_REDUNDANT=y
    CONFIG_ENV_OFFSET=0xf90000
    CONFIG_ENV_OFFSET_REDUND=0xfa0000
    CONFIG_ENV_SIZE=0x10000
    CONFIG_ENV_SECT_SIZE=0x10000
    CONFIG_ENV_SPI_BUS=0
    CONFIG_ENV_SPI_CS=0
    CONFIG_ENV_SPI_MAX_HZ=25000000
    CONFIG_ENV_SPI_MODE=0x0
    CONFIG_CMD_SAVEENV=y
    # Size: the stage3 app must fit the 512 KiB IPXE slot (spi-upgrade.py). Unused here: the
    # kernel is a raw Image on ext4; loadb/loady stay for serial recovery. FAT and LZ4 stay
    # (selected by the bootstd defaults).
    # CONFIG_CMD_FDT is not set
    # CONFIG_CMD_LZMADEC is not set
    # CONFIG_LZMA is not set
    # CONFIG_CMD_UNZIP is not set
    # CONFIG_CMD_LOADS is not set
    # CONFIG_CMD_MII is not set
    # CONFIG_CMD_ELF is not set
    # CONFIG_CMD_XIMG is not set
  '';

  mk = { defconfig, dtsName, dts, extraConfig ? "", uimageName }:
    (cross.buildUBoot {
      inherit defconfig extraConfig;
      extraPatches = patches;
      extraMeta.platforms = [ "aarch64-linux" ];
      filesToInstall = [ "u-boot.bin" "u-boot" "u-boot.map" "u-boot.dtb" ".config" ];
    }).overrideAttrs (o: {
      postPatch = o.postPatch + ''
        cp ${dts} arch/arm/dts/${dtsName}.dts
      '' + uboots.lib.reproducibleVersion;
      # Kernel-type legacy uImage: the vendor bootm then enters us like Linux
      # (MMU and caches off, EL2, x0 = its DTB). -T standalone would not.
      postInstall = ''
        ./tools/mkimage -A arm64 -O linux -T kernel -C none \
          -a ${loadAddr} -e ${loadAddr} -n "${uimageName}" \
          -d u-boot.bin $out/u-boot.uimage
        ./tools/mkimage -l $out/u-boot.uimage > $out/u-boot.uimage.txt
      '';
    });
  alboot = builtins.path { path = ../../alboot/alboot.py; name = "alboot.py"; };
in rec {
  inherit loadAddr patches;
  inherit (uboots.lib) reproducibleVersion;

  # RAM-only second stage for the DMC-12. Never flashed.
  dmc12-chainload = mk {
    defconfig = "alpine_v2_dmc12_defconfig";
    dtsName = "alpine-v2-dmc12";
    dts = dmc12Dts;
    uimageName = "U-Boot 2026.10 dmc12 chainload";
    extraConfig = ''
      CONFIG_LOCALVERSION="-dmc12-chainload"
    '';
  };

  # The chainload build plus what the vendor U-Boot does for us today
  # (patches 0017-0020): external PCIe port 0 RC init (kept if the
  # firmware already trained the link, "alpcie reinit" redoes it), MUIO
  # pinmux replay, SoC GPIO 22 high, CCU writes before Linux, read-only SPI NOR (sf probe/read
  # only, non-read SPI commands refused) except the env window (0019, saveenv). One binary
  # for both entries: chainloaded in RAM after the vendor U-Boot (test) and
  # as al-boot stage3's application (dmc12-stage3app), flashed by spi-upgrade.py.
  dmc12-selfinit = mk {
    defconfig = "alpine_v2_dmc12_defconfig";
    dtsName = "alpine-v2-dmc12";
    dts = dmc12Dts;
    uimageName = "U-Boot 2026.10 dmc12 selfinit";
    extraConfig = selfinitConfig;
  };

  # dmc12-selfinit's u-boot.bin, byte for byte, as an al-boot application
  # object for stage3 (spi-uboot-replacement.md section 5): id 0x110 (IPXE,
  # the blank 512 KiB slot), load = exec = 0x08000000 (the RAM-tested
  # address, no PIE fixups), plus the same at 0x01100000 (the factory
  # U-Boot's address, fallback if stage3 cannot reach 0x08000000). Written to
  # the IPXE 0/1 slots by devices/dmc-12-48r-as0/spi-upgrade.py (the -08000000 one).
  dmc12-stage3app = pkgs.runCommand "uboot-dmc12-stage3app" {
    nativeBuildInputs = [ pkgs.python3 ];
  } ''
    set -euo pipefail
    mkdir $out
    cp ${dmc12-selfinit}/u-boot.bin ${dmc12-selfinit}/u-boot.dtb ${dmc12-selfinit}/.config $out/
    python3 ${alboot} wrap --id 0x110 --load 0x08000000 --version 2026.10.0 \
      ${dmc12-selfinit}/u-boot.bin $out/u-boot-ipxe-08000000.fimg | tee $out/wrap.txt
    python3 ${alboot} wrap --id 0x110 --load 0x01100000 --version 2026.10.0 \
      ${dmc12-selfinit}/u-boot.bin $out/u-boot-ipxe-01100000.fimg | tee -a $out/wrap.txt
    python3 ${alboot} check $out/*.fimg | tee $out/check.txt
    (cd $out && sha256sum u-boot.bin *.fimg > SHA256SUMS)
  '';

  # Same board code on QEMU virt (test fixture DT; see ./qemu-virt.dts).
  qemu-virt = mk {
    defconfig = "alpine_v2_dmc12_defconfig";
    dtsName = "alpine-v2-qemu-virt";
    dts = builtins.path { path = ./qemu-virt.dts; name = "alpine-v2-qemu-virt.dts"; };
    uimageName = "U-Boot 2026.10 alpine-v2 qemu";
    extraConfig = ''
      CONFIG_LOCALVERSION="-alpine-v2-qemu"
      CONFIG_DEFAULT_DEVICE_TREE="alpine-v2-qemu-virt"
      CONFIG_DEFAULT_FDT_FILE="test.dtb"
      # CONFIG_DEBUG_UART is not set
      CONFIG_PL01X_SERIAL=y
    '';
  };
}
