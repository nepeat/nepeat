# DMC-12 units

One row per physical box. Identifiers (serial, base MAC, service tag, factory
hostname) live only in `units-private.txt` (git-ignored), keyed by unit id.
Put the unit id on the chassis (sticker). Dumps go in `dumps/<unit>/`, small
reads in `inventory/<unit>/`.

## Versions and state

| field | u01 |
| --- | --- |
| first seen | 2026-11-15 |
| origin | liquidation, **dirty** (operator config 2022) |
| EEPROM label rev / device ver / mfg date | R02E / 9 / 2020-09-10 |
| part / platform | FU2AOS481000A / K2C-AB-T2_DMC-12-48R-AS0 |
| ONIE / diag (EEPROM) | V1.0.0.1 / V1.0.3.0 |
| CPLD (reg 0x01) | 0x88 (v7+, 64 PWM steps) |
| uC bootloader / app | 1.13 / 1.35.0; NOT-SECURE |
| SPI CS0 (mtd0, booted) | al-boot 2.60.4, U-Boot `2015.07-alpine_db-2.60.4-00030-g1e9d506` |
| SPI CS1 (mtd1) | generic al-boot 2.44, U-Boot `2015.07-alpine_db-2.44-13429-g55b29e4` |
| U-Boot env | `DMC-12_UBOOTENV 1.0.81.0@edf7fb1b`, stock bootcmd |
| booted (`/boot/primary`) | OS 1.0.457.0@5035ece9 + kernel 1.0.99.0@5ed0fb00b |
| `/boot/secondary` | empty |
| other images on sda1 `/` | OS 1.0.526.0@eec143b9 + kernel 1.0.101.0@5ed0fb00 |
| SSD | KINGSTON RBUSNS4180S332GJ rev SBFK, 32 GB |
| PSU (CPLD 0x11) | PSU2 only; PSU1 alarm (0x20 bit 6) |
| burn-in anomalies | port 30 failed 1 of 3 serial loopback runs |
| sda.img sha256 | `46dac8f9…` (`dumps/u01/SHA256SUMS`) |
| mtd0 / mtd1 sha256 | `b66dbca5…` / `9c3a9556…` |
| now (2026-11-15) | **NixOS on SSD** via chainloaded U-Boot (`inventory/u01/reimage.jsonl`); factory SSD in `dumps/u01/` |

Kernel 1.0.99.0 vs 1.0.101.0: the same source rebuilt (66 bytes differ:
build host/date). The real changes between OS 457 and 526 are in userspace (see PROGRESS).

## Collecting a new unit (read-only)
1. `lab-power N …` for its plug (add it to `tools/lab_power.py`), console in
   `serial-dmc12`, cold boot captured.
2. `./powercycle_to_uboot.py` → `printenv` to `inventory/<unit>/uboot-printenv-private.txt`;
   `version`, `uc_get_version`, `uc_get_secure`, `uc_watchdog_get_status`,
   `flash_contents_toc_print`.
3. rdinit shell (PROGRESS Status). Then `acquire.py --unit <unit> --host <ip>`.
   EEPROM, DTB, SPD and `/proc/config.gz` via `fetch_small.py`. CPLD
   registers as in `inventory/u01/cpld-regs.txt`.
4. Offline: image headers (`dumpimage -l`) of every uImage on sda1, `ls` of
   `/boot/*`, check for operator config (dirty or clean), `dmc12_diag_log` results.
   Copy any image not already archived (compare sha256) to `dumps/<unit>/factory-os/`.
5. Fill a new column here; identifiers go in `units-private.txt`.
