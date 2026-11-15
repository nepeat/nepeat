# DMC-12-48R-AS0 serial console server

## Status
- Phase: **info gathering only** (runbook phases 0–1, below). Nothing written to
  flash, SSD or the U-Boot env.
- Runs the factory OS: `DMC-12 1.0.457.0@5035ece9…` on kernel
  `4.19.32-al-linux-v10.2.0`. Login credentials unknown.
- Console: tmux `serial-dmc12`, tio on the FTDI `B0016Y5Q` (by-id path) at **115200 8N1**,
  confirmed from a readable boot. Captures go to `serial-*.txt` (git-ignored, may
  hold env/keys). The AS7760's console is cabled to **DMC-12 port 48**: read-only
  use only, and ask the AS7760 session first.
- Power: lab plug 2, `lab-power 2 …`. Plug 1 is the AS7760; don't touch it from here.
- Into U-Boot: `./powercycle_to_uboot.py` (any key). Root shell with nothing mounted
  from disk: at `ALPINE_DB>`, `setenv bootargsextra rdinit=/bin/sh; run dmc12_bootdisk`
  (never `saveenv`). Then mount proc/sys/devtmpfs and run `ifconfig eth0 up; udhcpc -i eth0`.
  Commands go through `./console.py`.
- Before cutting power under the factory OS: serial break + SysRq `s`, `u` (works).
- Recovery: boot stages are A/B in SPI NOR, with a second SPI image (mtd1) holding
  older al-boot. The M.2 SATA SSD is removable. Full images in `dumps/`.
- NixOS module: `ai_re/nix/modules/boards/dmc12` (verbatim import of the first
  iteration; see its README for what still has to change).
- **Left in:** the rdinit shell, powered on, at 10.60.2.10, with nothing mounted from disk.
  So no ser2net, and the AS7760's console on port 48 isn't served. A
  power-cycle returns it to the factory OS.
- Open: CPLD version, USB/PCI inventory from Linux, fan state with no OS,
  mtd1's role, unpacking the images offline.

## Identity
- Board EEPROM (ONIE TLV, 24c02 @ I2C 7-0051): **Accton / Edgecore**,
  `DMC-12-48R-AS0`, part `FU2AOS481000A`, label R02E, platform
  `K2C-AB-T2_DMC-12-48R-AS0`, ONIE V1.0.0.1, diag V1.0.3.0, made 2020-09-10.
  Serial, MAC and service tag are only in `inventory/board-eeprom-private.txt`.
- I2C (factory kernel): SPD 24c64 @0-0057, pca9548 @0-0070, ds1339 RTC @2-0068,
  TPM `tpm_i2c_infineon` @3-0020, pca9546 @4-0071, lm75 @5-004a, tmp75
  @7-0048/49, board EEPROM 24c02 @7-0051. Board CPLD at bus 4 / 0x68 (raw, no driver).
  The live DT also has a SPI TPM (`infineon,slb9670`), sp805 watchdog, pl061 GPIO, gpio-leds.
- Board config `DMC-12 190520a alpine_v2_k2c_ab (rc mode + sata)`. DT model
  `DMC-12 Alpine V2 K2C-AB (RC + SATA)`. SoC: Annapurna Labs Alpine V2,
  4× Cortex-A57 (MIDR 0x411fd073), 4 GiB DDR (SPD at I2C 0x57).
- PCI (U-Boot): 1c36:0001 eth, 1c36:0022 ×2 (crypto, mass storage),
  1c36:0031 AHCI, behind PCIE_0 12d8:400c bridge. 48 ports = 12× CP2108 quad
  UART on USB hubs (`ttyUSB0…47`).
- uC on the SoC card: bootloader 1.13, app 1.35.0. Secure state **NOT-SECURE**,
  recovery disabled, nitro mode disabled, physical-presence 0. Watchdog enabled
  (10 s, reset count 0). It didn't fire after minutes in U-Boot or the
  rdinit shell.
- Interrupting autoboot prints `TPM: u-boot interrupted, extending TPM PCRs!`
  (affects that boot only).

## Versions
| part | version |
| --- | --- |
| stage2 / stage3 | `stage2_loader_k2 v2.60.3` / 2.60.4 (b9e30ac, Apr 1 2019) |
| U-Boot | `2015.07-alpine_db-2.60.4-00030-g1e9d506-HAL-00010-g3eb5ee1` |
| U-Boot env | `DMC-12_UBOOTENV 1.0.81.0@edf7fb1b…` |
| kernel uImage | `DMC-12_Kernel 1.0.99.0@5ed0fb00b` (4.19.32-al-linux-v10.2.0) |
| OS initramfs | `DMC-12_OS 1.0.457.0@5035ece96ca` |

## Interfaces and storage
- Mgmt: al_eth `eth0`, RGMII at 1 Gb/s. SFP not connected. DHCP in the rdinit shell
  gave 10.60.2.10; U-Boot's `bootp` (with vendor class) got 10.60.30.12 instead.
- SPI NOR MX25U12835F, 16 MiB (`mtd0`). Flash TOC from `flash_contents_toc_print`: A/B
  stg2, dt, stg3, apcea(+cfg), uboot, uboot-script, ipxe, bootapp(+cfg),
  eeprom ×2 at 0xfc0000/0xfc1000, env 0xfd0000, redundant env 0xfe0000.
- `mtd1`: a second 16 MiB SPI image. Almost every TOC region differs from mtd0,
  and it holds generic al-boot 2.44 (Jul 2018), U-Boot `2.44-13429-g55b29e4`, no
  DMC-12 env strings. Role unconfirmed (second chip, maybe factory/recovery).
- SSD: KINGSTON RBUSNS4180S332GJ (rev SBFK), 62533296 × 512 B. ATA security
  supported, not enabled, **not frozen**. `sda1` (15.0 GiB, ext4:
  `/boot/{primary,secondary}`, `recoverymode` flag) and `sda2` (14.8 GiB). Boot
  tries primary, then secondary, then `/`.
- U-Boot env: `uboot-printenv-private.txt` (git-ignored). Interesting vars: `bootcmd=run dmc12_bootdisk`,
  `bootdelay=5`, `$bootargsextra` hook, fan presets via CPLD (I2C bus 4, 0x68).

## Dumps (`dumps/`, git-ignored; zstd per the runbook)
`dumps/manifest.jsonl` + `dumps/SHA256SUMS` hold raw and `.zst` hashes.
- `mtd0.bin` sha256 `b66dbca586b8189a4ea0e2241d7a07642223154ea6466d7570e384c6f4462b39`
- `mtd1.bin` sha256 `9c3a95563313cdc4ae9c9840e2f1b75cfe5ffdeec2aa7052cd61c0f860178705`
- `sda.img` sha256 `46dac8f9585ee113c13e250317f07e6724d1464c2ea8e2f479148d786feedffa`
  (32,017,047,552 B → 1.05 GB zstd)
- Also copied: `live.dtb` (+ `live.dts`), board EEPROM, SPD, each hash-checked
  against the box (`fetch_small.py`).

## Bring-up checklist (switch-porting-runbook.md, phases 0–1)
Phase 0
- [x] logged tmux/tio session, by-id path; [x] baud from a readable boot
- [x] paced input (`console.py`); [x] scripted power + power-cycle to U-Boot
- [ ] logic levels: the console is the external port on the user's cable; no internal header probed
- [ ] fans with no OS: the bootcmd preamble sets `fan_speed_70`; behaviour when stopped
  before the preamble is unmeasured
- [x] network: mgmt on the lab LAN (DHCP); [x] recovery inventory (above; JTAG unknown)
- [x] dir 0700 + `.gitignore`
- Gate: met, except for the two open safety notes above.

Phase 1
- [x] clean cold boot: `serial-20261115-initial-boot.txt` (sha256 `9d8d47aa…`)
- [x] versions (table), board EEPROM; [ ] CPLD version, uC FRU
- [~] inventory: I2C devices (sysfs) and live DTB done; lspci, USB tree to do
- [x] SSD identify (`hdparm -I`)
- [x] dumps: mtd0, mtd1, sda, all triple-hashed and zstd-verified
- [ ] unpack offline (uImages, initramfs, DT, ext4 via read-only tools)
- [~] secure boot: uC says NOT-SECURE; OTP unread

## Log (newest first)
- 2026-11-15 03:00: SSD image done. The first run verified the raw image, but its zstd step
  (`-T0`) died at 02:56 in a host-wide OOM: a `cc1plus` nix build plus zstd. The OOM
  killer took `sd-pam` and with it the whole user session (tmux, tio, every Claude
  session). Recompressed from the re-hashed raw with `acquire.py --compress` (now
  `-T6`, niced). The box stayed in the rdinit shell throughout (uptime >30 min, so the
  uC watchdog doesn't fire there). Then: I2C sysfs map, `hdparm -I`, live DTB,
  EEPROMs.
- 2026-11-15: info-gathering pass. Moved the FTDI session over from `serial-7760`
  (told that session; the AS7760 is SSH-only now). The first power-cycle missed
  autoboot because of tio log buffering; the helper now watches the pane. Got into
  U-Boot, saved the env privately, ran read-only `uc_get_*` and the flash TOC, booted
  rdinit shell, imaged mtd0/mtd1 (triple-hashed, zstd). Imported the first
  iteration's NixOS module verbatim into `ai_re/nix`.
- 2026-11-15: `lab-power` now needs a plug number. Plug 2 on/off tested,
  plug 1 unchanged (HA state only).
