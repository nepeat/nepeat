# K2 SoC-card microcontroller (uC) and the factory U-Boot

For future U-Boot work on Alpine V2 "K2" cards (DMC-12 K2C-AB; the CS8210's
K2X is a sibling). Source: factory U-Boot `2015.07-alpine_db-2.60.4` on u01,
`help` (`inventory/u01/uboot-help.txt`, 38 `uc_*` commands), strings of the
carved U-Boot object (`dumps/u01/spi-cs0/uboot.0.bin`, git-ignored). Disassembled
2026-11-15: per-command behaviour, the uC mailbox protocol and the command IDs are in
[uboot-vendor-commands.md](uboot-vendor-commands.md).

## What the uC is
- A management microcontroller on the SoC card, with its own bootloader + app
  (u01: 1.13 / 1.35.0). Reported state on u01: NOT-SECURE, recovery disabled,
  nitro mode disabled, physical-presence 0, watchdog enabled 10 s (never fired
  idle: see PROGRESS).
- **I2C faces:** it emulates an **LM75A at 0x4A** (the "K2 SoC (uC)" sensor at Linux
  5-004a is the uC), and has a **management interface**, default slave **0x46**.
  Both can be switched on and off from U-Boot (`<enable/disable> [i2c slave address]`).
  The uC also has a **FRU interface at I2C 0x30** (reported enabled on u01 by the owner's
  `uc_fru*` query, 2026-11-15).
- The SoC talks to it through a mailbox at 0xfd8a4170 plus a GPIO 37 doorbell, and
  reads its shared data (FRU, voltage/temperature) at 0xfd8a7f00. Protocol:
  [uboot-vendor-commands.md](uboot-vendor-commands.md#uc-command-channel-all-uc_).
  The HAL also has `al_uc_spi_preloader_burn`, but no command reaches it. Recovery and
  safety paths (flash select, watchdog, OTP): [spi-recovery.md](spi-recovery.md).

## Commands (factory U-Boot)
Per-command read/write, risk and port verdict: [uboot-vendor-commands.md](uboot-vendor-commands.md).
`uc_get_flash_booted_from` sends nothing: it always prints "Unsupported command for dev id 1".

## SPI objects (TOC, `flash_contents_toc_print`)
Object IDs (`flash_contents_*` take a name or a number; the instance goes in bits
31:28): 0 BOOT_MODE, 1 STG2, 2 DT, 3 STG2_5, 4 STG3, 5 UBOOT, 6 UBOOT_SCRIPT, 7/8
UBOOT_ENV/_RED, 9 KERNEL, 10 ROOT_FS, 11 2ND_TOC, 12 PRE_BOOT, 13 PRE_BOOT_V2, 0x10
HW_PLD, 0x100/0x101 APCEA/_CFG, 0x200/0x201 BOOT_APP/_CFG. `stg2` (instances at 0x0 and
0x40000; mtd1 also has one at 0x20000) starts with the boot-ROM header `S2`. Every other
object starts with the al-boot header magic `c7 9e 0b 00`. On CS0,
`uboot`, `stg3`, `apcea_cfg` and `uboot_script` are identical in both instances;
`stg2`, `dt` and `bootapp` differ. Both `apcea` slots are blank on CS0; CS1 (mtd1)
has data there. `uboot_script` (`inventory/u01/uboot-script-strings.txt`) runs on
**every** boot. It is guarded only by `boot_app_toc`, so an env reset reroutes boot to
`BOOT_APP`: see the hazards in [uboot-vendor-commands.md](uboot-vendor-commands.md).
`BOOT_APP` (A/B 4.8 MiB slots, a 463 KiB image) is the K2C-AB `eth_fast_path` CVOS NIC firmware, loaded
with `bootm`; `APCEA` is a PCIe-endpoint early app: [boot-chain-details.md](boot-chain-details.md) §4.

## Next
Read `uc_fru_print` / `uc_sdr_print` (read-only, but the FRU output holds identifiers:
save it to `*-private.txt`).
