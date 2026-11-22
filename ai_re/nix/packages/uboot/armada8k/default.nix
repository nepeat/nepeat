{ pkgs, uboots }:
# Mainline U-Boot (nixpkgs buildUBoot, 2026.10) for Marvell Armada 7040/8040
# (AP806 + CP110) boards whose SoC is secure-boot fused, so the SPI boot
# image can never be replaced: this U-Boot is CHAINLOADED, from RAM, by the
# factory U-Boot and never touches SPI.
#
#   factory U-Boot --(load u-boot.bin; dcache off; icache off; go <addr>)-->
#   this U-Boot --(bootstd, EFI)--> systemd-boot --> NixOS
#
# SETTLED, NOT TO BE REVISITED: no build of this package may write SPI flash.
# On these boards (TN48M, AS5114-48X: same fused Amazon KAK) a bad SPI boot
# image is an unrecoverable brick without an external programmer. So:
# CMD_MVEBU_BUBT is never built, the SPI NOR layer is read-only (patch 0019,
# no write window), raw SPI ("sspi") is out, and the environment is nowhere.
# checkChainloadConfig below fails the build if a board's extraConfig turns
# any of that back on. The image is RAM-only; it is never written to SPI by
# anyone. (`mw`/`go` can of course still run arbitrary code; what this
# guarantees is that no SPI-write path is built in.)
#
# Patches: the "armada8k" + "spi-nor-ro" scopes of the shared series in
# ../default.nix:
#   0019 (spi-nor-ro) read-only SPI NOR: sf probe/read only, spi-mem refuses
#        every non-read opcode below all drivers and commands
#   0032 (armada8k) map all DRAM in the MMU (upstream maps ~2 GiB; EFI gets
#        all banks, so the first EFI payload crashed on an 8 GiB board)
#   0033 (armada8k) build with NO_NET and without an MMC env
# The board's control DTS is copied into arch/arm/dts at build time (no
# per-board patch). Linux gets its own DT through systemd-boot.
#
# The recipe is the Delta TN48M's, proven on hardware (RAM `go` chainload,
# then bootflow -> systemd-boot -> NixOS), whose working copy lives in the
# separate nepeat/nix repo (nixos/module/tn48m/uboot/). This package is
# written so the TN48M can adopt it later (`mkChainload` with its own DTS,
# prompt and SPI_FLASH_WINBOND) -- deliberately NOT done here: that code was
# not touched or migrated. Differences from that recipe, all on the safe
# side: read-only SPI NOR (0019), no CMD_SPI or saveenv, no USB in the board DTS, the
# config check below; DM_SCSI dropped (gone from 2026.10 Kconfig, SCSI is DM-only).
let
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  patches = uboots.lib.patchesFor [ "spi-nor-ro" "armada8k" ];

  # Board-agnostic part of every chainload build. Why each line is there:
  chainloadConfig = ''
    # Runs from wherever the factory U-Boot loads it (link address 0).
    CONFIG_POSITION_INDEPENDENT=y
    # Never touches the factory env in SPI: the env lives in RAM only and
    # there is no saveenv target.
    # CONFIG_ENV_IS_IN_SPI_FLASH is not set
    CONFIG_ENV_IS_NOWHERE=y
    # Nowhere to save to; no command that pretends there is.
    # CONFIG_CMD_SAVEENV is not set
    CONFIG_BOOTDELAY=3
    # The SSD: AHCI behind SCSI, FAT for the ESP. FAT_WRITE is for the EFI
    # variable file (EFI_VARIABLE_FILE_STORE) on the ESP, not for SPI.
    CONFIG_SCSI=y
    CONFIG_CMD_SCSI=y
    CONFIG_SCSI_AHCI=y
    CONFIG_FS_FAT=y
    CONFIG_FAT_WRITE=y
    # EFI + standard boot: `bootflow scan` finds systemd-boot on the ESP.
    CONFIG_EFI_LOADER=y
    CONFIG_CMD_BOOTEFI=y
    CONFIG_CMD_EFIDEBUG=y
    CONFIG_EFI_VARIABLE_FILE_STORE=y
    CONFIG_CMD_BOOTFLOW=y
    # No networking: the mvpp2 driver aborted on re-probe when bootstd's
    # ethernet bootdev came around (mvpp2_bm_pool_use reading a pointer out
    # of the relocated DTB). Not needed: this U-Boot only boots the SSD, and
    # the factory U-Boot and ONIE keep TFTP for recovery.
    CONFIG_NO_NET=y
    # CONFIG_NET_LEGACY is not set
    # `bubt` writes a boot image to SPI. On this secure-boot-fused SoC that
    # bricks the box (only Amazon-signed images boot), so leave it out.
    # CONFIG_CMD_MVEBU_BUBT is not set
    # Read-only SPI NOR (patch 0019, no write window): `sf` is probe/read
    # only and spi-mem passes only read opcodes. LOCK/UNLOCK_ALL off (they
    # write status registers; READ_ONLY depends on both being off), no SFDP
    # or soft reset (opcodes the read-only filter would refuse anyway).
    CONFIG_SPI_FLASH_READ_ONLY=y
    # CONFIG_SPI_FLASH_LOCK is not set
    # CONFIG_SPI_FLASH_UNLOCK_ALL is not set
    # CONFIG_SPI_FLASH_SFDP_SUPPORT is not set
    # CONFIG_SPI_FLASH_SOFT_RESET is not set
    # CONFIG_SPI_FLASH_BAR is not set
    # CONFIG_SPI_FLASH_WRITE_WINDOW is not set
    # `sspi` clocks raw bytes onto any SPI bus (WREN + page program
    # included), below the spi-mem filter.
    # CONFIG_CMD_SPI is not set
    # MACs for the Linux DT from the ONIE TLV EEPROM (see the board DTS).
    CONFIG_I2C_EEPROM=y
    CONFIG_CMD_TLV_EEPROM=y
    CONFIG_ID_EEPROM=y
    CONFIG_USE_BOOTCOMMAND=y
    # EFI only: /extlinux/extlinux.conf on the ESP is the factory U-Boot's
    # fallback and must not be picked up here. `bootflow scan` boots the
    # first bootflow found (-lb would need BOOTSTD_FULL).
    # CONFIG_BOOTMETH_EXTLINUX is not set
    # CONFIG_BOOTMETH_EXTLINUX_PXE is not set
    CONFIG_BOOTCOMMAND="bootflow scan"
  '';

  # Fails the build unless the final .config still has every safety line
  # above (Kconfig drops a symbol whose dependencies a later line broke,
  # silently), and the ELF has no bubt/sspi command.
  checkChainloadConfig = ''
    set -e
    must() { grep -qx "$1" .config || { echo "chainload safety: missing '$1' in .config" >&2; exit 1; }; }
    mustnot() { if grep -q "^$1=" .config; then echo "chainload safety: $1 is set" >&2; exit 1; fi; }
    must CONFIG_POSITION_INDEPENDENT=y
    must CONFIG_ENV_IS_NOWHERE=y
    must CONFIG_NO_NET=y
    must CONFIG_SPI_FLASH_READ_ONLY=y
    must CONFIG_SPI_MEM_READ_ONLY=y
    for s in CMD_MVEBU_BUBT CMD_SPI CMD_SAVEENV CMD_MTD SPI_FLASH_WRITE_WINDOW \
             SPI_FLASH_LOCK SPI_FLASH_UNLOCK_ALL SPI_FLASH_MTD \
             ENV_IS_IN_SPI_FLASH ENV_IS_IN_MMC ENV_IS_IN_FAT ENV_IS_IN_EXT4 \
             DFU FASTBOOT; do
      mustnot CONFIG_$s
    done
    if $NM u-boot | grep -Ewq 'do_bubt|do_spi'; then
      echo "chainload safety: bubt/sspi command linked in" >&2; exit 1
    fi
  '';

  # One board: `board` names the build, `dtsName` is the control DTS's name
  # in arch/arm/dts (= CONFIG_DEFAULT_DEVICE_TREE), `dts` its source,
  # `prompt`/`ident` tell it apart from the factory U-Boot on the console,
  # extraConfig carries board deltas (e.g. the SPI NOR vendor: the defconfig
  # already has Macronix, Spansion, ST).
  mkChainload = { board, dtsName, dts, prompt, ident, extraConfig ? "" }:
    (cross.buildUBoot {
      defconfig = "mvebu_db_armada8k_defconfig";
      extraPatches = patches;
      extraConfig = chainloadConfig + ''
        CONFIG_DEFAULT_DEVICE_TREE="${dtsName}"
        CONFIG_SYS_PROMPT="${prompt}"
        CONFIG_IDENT_STRING="${ident}"
      '' + extraConfig;
      extraMeta.platforms = [ "aarch64-linux" ];
      filesToInstall = [ "u-boot.bin" "u-boot" "u-boot.map" "u-boot.dtb" ".config" ];
    }).overrideAttrs (o: {
      pname = "uboot-${board}-chainload";
      postPatch = o.postPatch + ''
        cp ${dts} arch/arm/dts/${dtsName}.dts
      '' + uboots.lib.reproducibleVersion;
      postBuild = (o.postBuild or "") + ''
        NM=${cross.stdenv.cc.targetPrefix}nm
        ${checkChainloadConfig}
      '';
      postInstall = (o.postInstall or "") + ''
        (cd $out && sha256sum u-boot.bin u-boot.dtb > SHA256SUMS)
      '';
    });
in {
  # For other boards (the TN48M, later): the builder and its parts.
  lib = { inherit mkChainload chainloadConfig checkChainloadConfig patches; };

  # Edgecore AS5114-48X (Accton AS4224). RAM only; see
  # devices/edge-core-as5114-48x/uboot-chainload.md.
  as5114-chainload = mkChainload {
    board = "as5114";
    dtsName = "armada-7040-as5114";
    dts = builtins.path { path = ./armada-7040-as5114.dts; name = "armada-7040-as5114.dts"; };
    prompt = "AS5114=> ";
    ident = " as5114-chainload";
  };
}
