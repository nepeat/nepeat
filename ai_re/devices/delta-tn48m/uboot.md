# TN48M U-Boot reference

Captured 2026-11-11 from this unit. Raw captures: `uboot-reference-raw.txt`
(`version`, `help`, `bdinfo`, `coninfo`, `i2c bus`, `pci`), `uboot-config.txt`
(output of the `config` command, 1258 lines), and the full env in
`onie-env-dmesg.txt`.

## build

`U-Boot 2018.03-devel-1.2.0 (Mar 01 2021 - 18:52:29 +0000) TN48M/TN48M-P/TN4810M/TN48M2 V06_test`,
built with Ubuntu/Linaro gcc 7.5.0 and binutils 2.30. The env's
`onie_uboot_version` says `Oct 13 2020 ... V06`, so U-Boot was replaced
after ONIE was installed. ATF BL31 v2.2 (Marvell-10.0.0) was built the same day.

## getting a prompt

- Banner: `Type password to STOP autoboot`, then `bootdelay=10` s.
- **Password: `@wa23`** (verified 2026-11-11). Type it during the countdown.
  No Enter is needed; U-Boot matches the keys as they arrive. Prompt: `Marvell>>`.
- Source: the compiled default stop string in rodata, right after the
  `bootdelaykey`/`bootstopkey` env names (mtd0 offset 0x14a07c). Neither env
  var is set, so the default applies. Setting `bootstopkey` would change it, so
  **don't**. `123` (from the dentOS wiki) is for other builds; spaces do nothing.
- The `config` command's embedded `.config` says `# CONFIG_AUTOBOOT_KEYED is not
  set` and `CONFIG_BOOTDELAY=2`, which doesn't match the binary's behaviour.
  Treat the embedded `.config` as approximate.
- tmux tip: send it with `tmux send-keys -t serial-tn48m -l '@wa23'` as soon as
  the banner shows. **Don't type ahead while `sf read` runs**: input during
  long SPI reads gets partly dropped.

## secure-ish boot: required FIT signature

The control DTB (mtd0 offset 0x18593c, saved as
`dumps/extracted/uboot-fdt-18593c.dtb`) has:

```
signature { ihm_dent_image_signing {
    required = "conf"; algo = "sha1,rsa2048";
    k-gen = "ubpubkey/1.0a/1613080609";
    k-sha256-fp = "3f66d8e891c69639ecbd9db88e3b86c7e70ed0f48f1d36764d5df68e5bf06c92";
    k-src = "cert/ihm_dent_image_signing.crt"; }; };
```

with `CONFIG_FIT_SIGNATURE=y`. So **`bootm` of a FIT only works if its
configuration is signed by Amazon's IHM DENT key** (the ONIE FIT is:
`sha1,rsa2048:ihm_dent_image_signing+ OK`). This doesn't affect:

- `booti` of a raw arm64 `Image` (+ separate DTB and initrd),
- `sysboot`/`pxe` extlinux of a raw `Image` (2018.03 pxe uses `booti` when
  the kernel isn't a recognised image format),
- `bootefi`.

The NixOS path (extlinux -> raw Image) is not blocked. The check lives only in
U-Boot, which sits in SPI. Whether the BootROM checks U-Boot (efuse secure
boot) is unknown; **don't reflash U-Boot** without a tested recovery path.

## capabilities worth knowing (help + config)

- Boot: `bootm` (FIT, signature-checked), `booti`, `bootefi` (`EFI_LOADER=y`),
  `sysboot`, `pxe`, `bootp/dhcp/tftpboot/nfs`, `usbboot`, `scsiboot`,
  `bootelf`, `bootvx`, `go`.
- Storage: `scsi` (AHCI SATA; `scsi info` printed nothing before `scsi scan`/`scsi reset`),
  `usb` + `USB_STORAGE`, `mmc` (no MMC found), `nand` (0 MiB), `sf` (16 MiB w25q128bv).
- Filesystems: `ext2load/ext4load/ext4ls/ext4size/ext4write`, `fat*`, `load`,
  `ls`, `save`, `size`, `fstype`, `part`. `gzwrite` writes images to block devices.
- Images: `fdt`, `imxtract`, `iminfo`, `unzip`, `lzmadec`, `crc32`, `md5sum`.
- Serial transfer: `loadb` (kermit), `loadx`, `loady`, `loads`.
- Net: `dhcp`, `ping`, `tftpboot` (no `tftpput`), `mdio`, `mii`, eth0-2 mvpp2,
  `ethprime=eth2` (mgmt).
- Board/SoC: `i2c` (bus 0 `i2c@701000` = cp0 i2c0 with 0x56 EEPROM, 0x2e
  ADT7473; bus 1 `i2c@701100`; bus 2 `i2c@511000` = AP i2c, where the CPLD lives),
  `gpio`, `pci`, `map` (address windows), `tsen` (SoC temp), `ir` (register
  read/modify), `mv_get_counters`, `rx_training`, `phy_fw_down`.
- **Dangerous / irreversible:** `fuse` (efuses), `SatR`/`sar` (sample-at-reset
  straps), `avs` (CPU voltage), `bubt` (burn U-Boot to flash), `sys_eeprom`
  (writes the ONIE TLV EEPROM), `saveenv`, `sf write/erase`, `ext4write`,
  `gzwrite`, `mw/mm/nm` on device registers.

## board info (`bdinfo`)

DRAM bank 0 `0x0` + 3 GiB, bank 1 `0x1_0000_0000` + 5 GiB (8 GiB total).
U-Boot relocates to `0x7FF06000`. Console `serial@512000`, 115200.

## recipes

### boot ONIE with a fixed DTB (safe bulk I/O), RAM only

ONIE's DTB doesn't reserve the ATF/PSCI area, and Linux crashes once the page
cache reaches it (see PROGRESS 2026-11-11). This one-boot fix writes nothing
persistent. Send **one line at a time** and wait for the prompt:

```
sf probe; sf read $kernel_addr_r $onie_spi_flash_offset $onie_spi_flash_size
imxtract $kernel_addr_r dtb 0x6f00000
fdt addr 0x6f00000
fdt resize 0x1000
fdt mknode / reserved-memory
fdt set /reserved-memory "#address-cells" <2>
fdt set /reserved-memory "#size-cells" <2>
fdt set /reserved-memory ranges
fdt mknode /reserved-memory psci-area@4000000
fdt set /reserved-memory/psci-area@4000000 reg <0 0x4000000 0 0x200000>
fdt set /reserved-memory/psci-area@4000000 no-map
run onie_args; bootm $kernel_addr_r $kernel_addr_r 0x6f00000
```

Check it worked: ONIE's dmesg `Memory: .../8386560K available` (2 MiB less
than 8 GiB) and `/proc/device-tree/reserved-memory/psci-area@4000000` exists.
FIT sub-image names: `kernel`, `initramfs`, `dtb`; configs `delta_tn48m`,
`delta_tn48m-poe`.

### recover from a broken NOS boot

Stop autoboot with `@wa23`, then `run onie_rescue` (or `run onie_bootcmd`) for
one boot. To make ONIE the default again from ONIE: `fw_setenv nos_bootcmd true`
(that's the as-received value).
