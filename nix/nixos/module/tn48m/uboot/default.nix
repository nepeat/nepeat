# Mainline U-Boot for the Delta TN48M, built to be CHAINLOADED by the factory
# U-Boot (the SoC is secure-boot fused, so nothing in SPI can be replaced).
#
#   factory U-Boot 2018.03 --(load u-boot.bin; dcache off; icache off; go)-->
#   this U-Boot --(bootstd, EFI)--> systemd-boot --> NixOS
#
# * POSITION_INDEPENDENT: runs from wherever the factory U-Boot loads it.
# * ENV_IS_NOWHERE: never touches the factory env in SPI (0x3f0000).
# * NO_NET + ID_EEPROM: no U-Boot networking; MACs come from the TLV EEPROM
#   and reach Linux through the EFI DT fixup.
# * 0001-...patch: the TN48M control DTS (factory pinmux/comphy/MDIO) and a
#   fix mapping all DRAM in the MMU; upstream only maps ~2 GiB while EFI gets
#   all banks, which crashes the first EFI payload on an 8 GiB board.
{ buildUBoot }:

buildUBoot {
  defconfig = "mvebu_db_armada8k_defconfig";
  extraPatches = [ ./0001-armada8k-map-all-dram-and-add-tn48m-dts.patch ];
  extraConfig = ''
    CONFIG_DEFAULT_DEVICE_TREE="armada-7040-tn48m"
    CONFIG_POSITION_INDEPENDENT=y
    # CONFIG_ENV_IS_IN_SPI_FLASH is not set
    CONFIG_ENV_IS_NOWHERE=y
    CONFIG_SPI_FLASH_WINBOND=y
    CONFIG_BOOTDELAY=3
    CONFIG_SYS_PROMPT="TN48M=> "
    CONFIG_IDENT_STRING=" tn48m-chainload"
    CONFIG_SCSI=y
    CONFIG_DM_SCSI=y
    CONFIG_CMD_SCSI=y
    CONFIG_SCSI_AHCI=y
    CONFIG_FS_FAT=y
    CONFIG_FAT_WRITE=y
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
    # MACs for the Linux DT from the ONIE TLV EEPROM (see the DTS).
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
  filesToInstall = [
    "u-boot.bin"
    "u-boot"
    ".config"
  ];
}
