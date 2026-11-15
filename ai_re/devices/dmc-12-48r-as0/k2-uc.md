# K2 SoC-card microcontroller (uC) and the factory U-Boot

For future U-Boot work on Alpine V2 "K2" cards (DMC-12 K2C-AB; the CS8210's
K2X is a sibling). Source: factory U-Boot `2015.07-alpine_db-2.60.4` on u01,
`help` (`inventory/u01/uboot-help.txt`, 38 `uc_*` commands), strings of the
carved U-Boot object (`dumps/u01/spi-cs0/uboot.0.bin`, git-ignored). Not
disassembled yet.

## What the uC is
- A management microcontroller on the SoC card, with its own bootloader + app
  (u01: 1.13 / 1.35.0). Reported state on u01: NOT-SECURE, recovery disabled,
  nitro mode disabled, physical-presence 0, watchdog enabled 10 s (never fired
  idle: see PROGRESS).
- **I2C faces:** it emulates an **LM75A at 0x4A** (the "K2 SoC (uC)" sensor at Linux
  5-004a is the uC), and has a **management interface**, default slave **0x46**.
  Both can be switched on and off from U-Boot (`<enable/disable> [i2c slave address]`).
- U-Boot links Annapurna's HAL for it: `al_uc_cmd_*` (command channel with a
  GPIO handshake: `uc_cmd_gpio`), `al_uc_shared_*` (shared data: FRU and "vt
  sensor" voltage/temperature records with valid flags), `al_uc_fru_burn`,
  `al_uc_update`, **`al_uc_spi_preloader_burn`**, a watchdog with "timer expired" /
  "power down" actions, and "max delay before uc reset".

## Commands (factory U-Boot)
Read-only, safe: `uc_get_version`, `uc_get_secure`, `uc_get_recovery`,
`uc_get_nitro_mode`, `uc_get_pp`, `uc_get_secure_log`, `uc_watchdog_get_status`,
`uc_fru_print`, `uc_fru_valid_get`, `uc_sdr_print`, `uc_sdr_get_version`, `uc_thermal`,
`uc_mgmt` (status), `uc_gpio_read`, `uc_gpio_get_dir`. `uc_get_flash_booted_from`
says "Unsupported command for dev id 1" on u01.

**Never without a plan**: `uc_update`, `uc_set_spi_buffer` (feeds the SPI
preloader burn), `uc_set_secure`, `uc_set_recovery`, `uc_set_nitro_mode`,
`uc_set_pp`, `uc_fru_burn/erase/unlock`, `uc_clear_secure_log`, `uc_reset`,
`uc_sdr_reset/set_fw_update/set_mask_reg`, `uc_thermal_shutdown*`,
`uc_watchdog_set_enabled`, `uc_gpio_write/set_dir`, `otp`. Some of these are one-way
(secure mode, OTP), and some rewrite the boot flash from the uC side.

## SPI objects (TOC, `flash_contents_toc_print`)
Every object starts with the al-boot header magic `c7 9e 0b 00`. On CS0,
`uboot`, `stg3`, `apcea_cfg` and `uboot_script` are identical in both instances;
`stg2`, `dt` and `bootapp` differ. Both `apcea` slots are blank on CS0; CS1 (mtd1)
has data there. `uboot_script` is a one-time env migration ("load boot app using
TOC object": `app_updspi`, `load_boot_app_spi`; `inventory/u01/uboot-script-strings.txt`).
`BOOT_APP` (A/B, about 4.8 MiB) is a separately bootable app loaded with `bootm`,
not identified yet.

## Next
Disassemble `al_uc_cmd_*` / `al_uc_shared_*` in `uboot.0.bin` to recover the wire
protocol (GPIO lines, shared-memory address). Read `uc_fru_print` / `uc_sdr_print`
(read-only, but FRU output holds identifiers, so save it to `*-private.txt`). Identify `BOOT_APP` and
`APCEA` (CS1).
