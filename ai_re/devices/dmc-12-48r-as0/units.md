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
| now (2026-11-15) | **our U-Boot in SPI (IPXE) + NixOS standard boot** (`inventory/u01/spi-upgrade.jsonl`, `reimage.jsonl`); factory SSD in `dumps/u01/`; lab console server |

Kernel 1.0.99.0 vs 1.0.101.0: the same source rebuilt (66 bytes differ:
build host/date). The real changes between OS 457 and 526 are in userspace (see PROGRESS).

## Production units (read-only inventory, no reboot)
- **p01** (2026-11-16; identity in `units-private.txt`): NixOS first-iteration image (7.2.3, comin), up 41 d.
  Same board rev as u01 (R02E, device ver 9, ONIE V1.0.0.1, diag V1.0.3.0; made 2021-06-18), **CPLD flash
  byte-identical to u01** (`ff56fa3a`), same preload EEPROM, USB/PCI revisions. Differs: SSD Innodisk
  `M.2 (S42) 3TE7` fw S19C04B (27.5 GB). After the 2026-11-16 deploy (our module, rev e56b987; booted 7.3.4, USB/TPM/SPI fine) its SPI was read
  (`dumps/p01/mtd{0,1}.bin.zst`, 2 identical reads + box sha256): **CS1 identical to u01's; CS0 identical
  to u01's factory dump except the factory env pair** (0xfd0000/0xfe0000, per-unit). Same al-boot 2.60.4,
  U-Boot 2015.07-alpine_db-2.60.4, BOOT_MODE -> UBOOT 0, IPXE 0/1 blank. uC not read. `inventory/p01/firmware.json`; collected with `fw-inventory.py collect --busybox /dev/shm/... --no-sudo`
  and `cpld-readback.py --emit-sh` (static busybox in RAM, removed after).
  **2026-11-16 14:03: our U-Boot in IPXE 0** (`dmc12-firmware install`, payload d3df4b80, proven; IPXE 1 blank;
  disk still `both`, so the factory U-Boot (`u` at stage3) boots it too). Journal: `/var/lib/dmc12-firmware/journal.jsonl` on p01.
  Seen there: `/run` 100% full (journald volatile 961 MB, no cap) and the USB INTx bug (EHCI 1 irq in
  41 d: serial byte loss under sustained output; fix = 494d188).

## New units
1. Hands: unit id sticker; the box on a switchable lab plug (`lab-power`, tools/README.md; plug 1 = CS8210,
   plug 2 = u01, so a new unit needs a third plug or u01 off its plug); the FTDI console cable
   on its front console port (tmux `serial-dmc12`); mgmt port on the lab LAN.
2. `./provision.py --unit u02 --plug N` (resumable, `--yes` for agents; steps in its docstring):
   factory capture (env, uC/TOC info, mtd0/mtd1/sda, DTB/EEPROM/SPD/config) → standard image on
   the SSD → `spi-upgrade.py` rehearse + install → smoke. Each write asks for the unit id.
   It assumes the box's DHCP lease is the same in the rdinit shell and in NixOS (same MAC: true
   on u01); if not, rerun with `--ip`.
   Its `inventory` step records every component (`fw-inventory.py`: `table` for the fleet,
   `diff uA uB --bytes` for byte ranges of differing SPI objects/CPLD flash, `drift` for what
   changed since the factory dump).
3. Service tag from the chassis label into `units-private.txt`. CPLD registers as in
   `inventory/u01/cpld-regs.txt` (`i2cget -y 4 0x68 …` from NixOS: read-only).
4. Offline: image headers (`dumpimage -l`) of every uImage on sda1, `ls` of
   `/boot/*`, check for operator config (dirty or clean), `dmc12_diag_log` results.
   Copy any image not already archived (compare sha256) to `dumps/<unit>/factory-os/`.
5. Fill a new column here; identifiers go in `units-private.txt`.
