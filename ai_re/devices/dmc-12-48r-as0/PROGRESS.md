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
  older al-boot. The M.2 SATA SSD is removable. Full images in `dumps/u01/`.
- NixOS: generic image `nix/configs/dmc12` on `boards/dmc12` + `platforms/alpine-v2` +
  `packages/{linux-alpine,al-eth}` (mainline 7.3.4, new DTS, CPLD reboot hook,
  interrupt monitor). Outputs `dmc12-netboot` (TFTP RAM boot) and `dmc12-disk-image`.
  Reimaging: `reimage.py` / `reimage.md`. **RAM-booted on u01 2026-11-15** (TFTP, kernel
  variant `trimmed`): system `running`, 0 failed units; eth0 al_eth 1G up via DHCP; 48 cp210x +
  /dev/P1..48; I2C numbering = factory; lm75/tmp75 read; GPIO lines named; both SPI NOR
  read-only; sp805 bound; fancontrol PI holds step 21 (3200 RPM); interruptmon logs + masks
  the PSU1 alarm. Disk install not done yet. Raw report: `inventory/u01/nixos-first-boot-private.txt`.
  Open from that boot: kernel was misaligned at 0x04080000 (now 0x04200000); `show power`
  INA219 on i2c-7 reads 0 (unverified); tpm_i2c_infineon "could not request locality";
  al_eth "serdes regs base failed" (harmless, RGMII); `reboot` not yet tested.
- Monitoring (built + offline-tested, not yet imported or run on u01): `boards/dmc12/monitoring.nix` =
  upstream node_exporter :9100 + read-only `dmc12_*` producer (`nix/packages/dmc12-metrics`), Grafana `ai-re-dmc12`.
- **Upstream U-Boot 2026.10** chainloads into RAM from the factory `bootm` and works on
  u01 (console, I2C/CPLD, PCIe, AHCI/SSD, CPLD reset): `u-boot-port.md`, `chainload_uboot.py`.
  Nothing flashed.
- **Left in:** the rdinit shell, powered on, at 10.60.2.10, with nothing mounted from disk.
  So no ser2net, and the AS7760's console on port 48 isn't served. A
  power-cycle returns it to the factory OS.
- **How this unit differs from the first iteration's:** see "This unit vs the
  first iteration" below. In short: it was deployed in a fleet, boots an older OS
  (1.0.457, while the old module ported ≥1.0.526), and only PSU2 is detected.
- **Images:** `ai-re-images` (tools/README) catalogs, tags and verifies every dump across devices;
  tags for this device are in `images.jsonl`. `export-qcow2` makes a zstd qcow2 for read-only nbd attach.
- **Units:** many more boxes are coming in. `units.md` has per-unit versions and the
  collection steps; serials are in `units-private.txt` (ignored). This box is **u01**.
- **K2 uC + factory U-Boot internals** (for future U-Boot patches): `k2-uc.md`.
- **Factory U-Boot vendor commands** (disassembled), what the boot path does for Linux, and
  port verdicts: `uboot-vendor-commands.md`. Before any on-disk boot, read its Ethernet
  board-params gap. Also: never reset the env, and uC GPIO 2.2 is the likely CS1 select.
- **Factory hardware tooling vs the module:** `vendor-tooling.md` (CPLD register map,
  reboot via CPLD/sp805, interrupt monitor, uplink re-sync, GPIO lines). Read it before
  reworking `board-dmc12`.
- Reboot: the **CPLD reset works** (0x29←1, 0x28←1 from Linux: full board reset, stage2 in
  under 1 s, U-Boot in about 7 s). Whether mainline's own `reboot` works is still to test.
- U-Boot netboot: `setenv autoload no; dhcp` (lab LAN lease) reaches the TFTP host
  10.60.2.208. No env writes needed for RAM boots.
- Open: what selects CS1 (mtd1), the PSU1 alarm (CPLD IRQ line gpio465 is held low by it).
  **Next kernel:** trim to allnoconfig + explicit (as linux-qoriq-t2080), from the drivers
  the first boot binds. Owner: the full nixpkgs module set is unnecessary.
  **After NixOS:** back up and analyse the CPLD (MachXO3 JTAG readback, cf. `cpldupd`), the uC
  firmware and the SPI `APCEA`/`APCEA_CFG` objects.

## Identity
- Board EEPROM (ONIE TLV, 24c02 @ I2C 7-0051): **Accton / Edgecore**,
  `DMC-12-48R-AS0`, part `FU2AOS481000A`, label R02E, platform
  `K2C-AB-T2_DMC-12-48R-AS0`, ONIE V1.0.0.1, diag V1.0.3.0, made 2020-09-10.
  Serial, MAC and service tag are only in `inventory/u01/board-eeprom-private.txt`.
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
- CPLD (I2C 4/0x68, read-only SMBus reads, `inventory/u01/cpld-regs.txt`): version 0x88,
  fan PWM reg 0x50 = 0x2a (step 41 ≈ 68%, U-Boot's `fan_speed_70`), fans 1–3 = 0x57
  (≈ 8700 RPM at the old tool's ×100 scale), WDT flag 0x2a = 0x01, LED 0x17 = 0x20.
- PCI (Linux): al_eth, 2× al-ssm, AHCI, and a Pericom 12d8:400c bridge to OHCI ×2 + EHCI
  (12d8:400e/400f) feeding the USB hubs. USB tree: `inventory/u01/usb-tree.txt`.
- uC on the SoC card: bootloader 1.13, app 1.35.0. Secure state **NOT-SECURE**,
  recovery disabled, nitro mode disabled, physical-presence 0. Watchdog enabled
  (10 s, reset count 0). It didn't fire after minutes in U-Boot or the
  rdinit shell.
- Interrupting autoboot prints `TPM: u-boot interrupted, extending TPM PCRs!`
  (affects that boot only).

## This unit vs the first iteration (2026-11-15, offline from the SSD image)
- **A deployed fleet unit, not new-in-box.** sda1 has operator config dated Feb 2022:
  hostname, hosts, ntp, `ser2net.conf`, `users/`, `passwd`/`shadow` (changed
  2022-02-09), ssh. Not reproduced here (private). The first iteration's notes describe
  sealed liquidation units. Its env is **stock** (`bootcmd=run dmc12_bootdisk`); the
  first unit's had been rewritten to load `/boot.scr`.
- **Older OS than the one ported.** It boots `/boot/primary` = OS **1.0.457.0**
  (Mar 2020) + kernel 1.0.99.0. `/boot/secondary` is **empty**. sda1's root `/`
  (boot fallback 3) holds a newer **1.0.526.0** (Jul 2020) + kernel 1.0.101.0 that never
  boots while primary is good. The old module's fan floor (step 9 ≈ 2000 RPM, "AS0")
  and `setphyspeed -q` exist only in 1.0.526, so the first unit ran ≥1.0.526.
  1.0.526 adds nftables, sysctl and the factory tools `changemac`, `eepromer`, `fru_gen_bin`.
  It changes `fancontrol`, `setphyspeed`, `interruptmon`, `updbootenv`, `show`, `envmon` and `bindserial`.
- **Two SPI NOR chips** (live DT: dw-apb-ssi CS0/CS1). CS0 = mtd0 = booted DMC-12
  al-boot 2.60.4. CS1 = mtd1 = generic Annapurna al-boot 2.44 (Jul 2018). Unknown
  whether the first unit had the same.
- **Power:** CPLD reg 0x11 = 0x17 → PSU2 detected, **PSU1 not detected**, 12 V
  pass-through not detected. It could just be one PSU cabled; check physically.
- **Hardware otherwise matches** the old module: CPLD at bus 4 / 0x68 is version
  **0x88** (v7+, 64 PWM steps), the USB tree is 3× hub (05e3:0608) × 4 CP2108 = P1–P48 exactly as
  `serial-ports.rules` has it, and the sensors/EEPROM sit on the same buses (factory enumeration).
- Factory burn-in log on sda1 (`/dmc12_diag_log`, serial matches the EEPROM):
  **port 30 failed one serial loopback run**, then passed two reruns (48/48).
  Treat port 30 as suspect if it misbehaves.
- sda2 (14.8 GiB) is empty apart from `lost+found` and flags "needs journal recovery".

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

## Dumps (`dumps/u01/`, git-ignored; zstd per the runbook)
`dumps/u01/manifest.jsonl` + `dumps/u01/SHA256SUMS` hold raw and `.zst` hashes.
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
- [x] fans with no OS: **full speed**. After reset the CPLD WDT flag 0x2a = 3 and the tachs read
  0x90/0x8f/0x8f (≈14.4k RPM) while PWM 0x50 = 0x09, i.e. the CPLD fails safe until software
  clears 0x2a. The bootcmd preamble then sets 70%.
- [x] network: mgmt on the lab LAN (DHCP); [x] recovery inventory (above; JTAG unknown)
- [x] dir 0700 + `.gitignore`
- Gate: met, except for the two open safety notes above.

Phase 1
- [x] clean cold boot: `serial-20261115-initial-boot.txt` (sha256 `9d8d47aa…`)
- [x] versions (table), board EEPROM; [ ] CPLD version, uC FRU
- [x] inventory: I2C (sysfs), PCI, USB tree, CPLD registers, live DTB
- [x] SSD identify (`hdparm -I`)
- [x] dumps: mtd0, mtd1, sda, all triple-hashed and zstd-verified
- [x] unpack offline: GPT, both ext4 (debugfs, read-only), all 4 uImages, both initramfs
- [~] secure boot: uC says NOT-SECURE; OTP unread

## Log (newest first)
- 2026-11-15 03:40: factory tooling pass (`vendor-tooling.md`). Found the CPLD reboot
  driver (0x29/0x28, disassembled from the vendor kernel via vmlinux-to-elf), the
  interrupt monitor + GPIO lines, and that uplink re-sync is ≥526-only. Kept the factory OS
  images, symbol ELF and burn-in logs in `dumps/u01/factory-os/`. Moved everything to
  per-unit `dumps/u01/` and `inventory/u01/`, added `units.md`.
- 2026-11-15 03:20: inventory + offline analysis (above). Also learned: plain `.zst`
  can't be mounted. nbdkit 1.48 has no zstd decompress filter, but its `qcow2dec` (nixpkgs build) and
  qemu read zstd-compressed qcow2, so that's the random-access format (runbook).
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
