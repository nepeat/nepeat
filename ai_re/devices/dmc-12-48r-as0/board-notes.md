# DMC-12-48R-AS0: board features and quirks (one line each; detail lives in the linked file)

Edgecore/Accton 48-port serial console server on an Annapurna Alpine V2 "K2C-AB" card (4× A57,
4 GiB DDR4 ECC). SoC-generic boot chain: `../../alpine-v2-boot-chain.md`. Evidence: [hw] seen on u01,
[code] from disassembly, [inf] inferred.

## Hardware features
- **Serial ports:** 48 = 12× CP2108 behind 3 USB hubs, behind a Pericom PCIe bridge on external PCIe
  port 0 (gen1 x1). `/dev/P1..P48` map by USB path (`conf/serial-ports.rules`). [hw] `vendor-tooling.md`
- **Board CPLD** (MachXO3, I2C bus 4 / 0x68, no Linux driver): fans, PSU/DC status, LED, uplink PHY
  state, interrupts, WDT flag, board reset. Register map: `vendor-tooling.md`.
- **K2 uC** on the SoC card: watchdog, FRU, emulated LM75 at 0x4A (the "CPU" temperature), mailbox with
  GPIO 37 doorbell. `k2-uc.md`, `uboot-vendor-commands.md`.
- **Two SPI NOR** (MX25U12835F, **1.8 V only**, 16 MiB): CS0 = DMC-12 al-boot 2.60.4 (booted), CS1 =
  generic K2C-AB al-boot 2.44. `spi-recovery.md`.
- **I2C EEPROM 0x57** = boot-ROM preload table + DDR4 SPD; **0x51** = ONIE TLV board EEPROM (Accton,
  serial, MAC). `boot-chain-details.md`.
- **TPM 2.0:** Infineon SLB9670 on SPI SS2 of the boot-flash controller (`tpm_tis_spi`, `/dev/tpmrm0`;
  vendor 0x15D1 dev 0x1B). PCRs are all zero: nothing in our chain measures. The vendor DT's I2C
  tpm@20 is a leftover (nothing answers on bus 3: the old "could not request locality"). [hw] **RTC** DS1339 2-0068. Sensors: lm75 5-004a (uC), tmp75 7-0048/0049.
- **NIC:** al_eth (PCI 00:01.0), RGMII 1G to an "upper/lower PHY pair" the CPLD manages (`setphyspeed`);
  SFP option; no SerDes on this port ("serdes regs base failed" is harmless). [hw]
- **Storage:** M.2 SATA SSD (AHCI 00:08.0). No eMMC/SD, no NVMe. [hw]
- **Unit variants:** AS0 vs DN0 chassis (DN0: no fan 3, fans stop lower, needs the SATA SerDes fix
  `serdes wr 1 p0 pma 21 2 0 5`, detect: `md.l 0xfb600000 1` = 0x8241104c [code]). Factory OS versions differ
  per unit (u01: 1.0.457 booted, 1.0.526 on disk). `units.md`, `vendor-tooling.md`.

## Boot / firmware quirks
- **Secure boot off** in OTP, no keys provisioned; UART boot + JTAG enabled. [hw] `spi-recovery.md`
- **A/B in SPI is not a fallback:** stage2 loads only instance 0 and spins on a bad object. The ROM
  retries stage2 at 16 offsets and then falls back to UART XMODEM by itself. [code] `spi-recovery.md`
- **Stage3 console keys** (`p` skip APCEA, `e` keep early APCEA, `u` force factory U-Boot) are polled
  **once**; send from stage2's "Executing next" line, every 20 ms. [hw 4/4] `uart-boot.md`, `stage3keys.py`
- **Interrupting the factory autoboot extends TPM PCRs** and sets bit 7 of 0xfbff41c1. [hw]
- **Factory env reset is a trap:** an SPI script reroutes `bootcmd` to BOOT_APP (K2 NIC firmware) and
  `saveenv`s when `boot_app_toc` != 10. Never `env default` / `delenv`. [code] `uboot-vendor-commands.md`
- **Factory `sf probe` writes the flash status register** (to 0); our U-Boot never does, and writes only its env window. [code]
- **Factory loader limits:** `initrd_high`/`fdt_high` must be 64-bit all-ones (0xffffffff failed to
  relocate a 298 MB initrd) [hw]; a 68 MiB kernel doesn't fit its default staging → uncompressed uImage
  staged at 0x10000000 [hw, our guard]; kernel at a 2 MiB-aligned 0x04200000 (Linux warned at
  0x04080000) [hw]; long bootargs go in a TFTP'd `boot.scr` (command-line limit ~256 assumed, [inf]);
  ext4 in the factory loader is flaky (owner) → our U-Boot in a raw partition. `reimage.md`, `PROGRESS.md`
- **Write-once registers:** CPU 64-bit select (0xf0072004) and reset address (+0x48). [hw]
- **Stage3 re-entry from RAM** needs the reset request without the WFI wait (a pending SError keeps
  WFI from holding). [hw] `uart-boot.md`
- **al_eth board params + MAC** are written by stage3 (present at the factory prompt before any dhcp);
  our U-Boot keeps or rewrites them. [hw/code] `u-boot-port.md`
- **Cold start (no factory U-Boot): GPIO 22 is an input** (pulled up: the CPLD's hitless-update hold) and 3 MUIO pins differ; our U-Boot sets both,
  and does its own PCIe RC init. [hw, first SPI boot]
- **I2C 4-0x10 is the CPLD's config port** (and 0x11/0x13 answer): any write there is a CPLD config
  command (erase/refresh/offline: fans, PSU sense, reset). No `sensors-detect` or write-mode probes on
  bus 4. CS0's APCEA slots (0x110000, 0x3d0000) must stay blank: stage3 runs any valid object there. [code] `research-open-items.md`
- **What picks CS1** (the "recovery" flash): the K2 uC, GPIO port 2 bit 2 (HAL); not straps or CPLD. [code] `research-open-items.md`
- **Fans fail safe:** with no OS the CPLD runs them full speed until the WDT flag 0x2a is cleared. [hw]
- **Our U-Boot's env:** redundant 2 × 64 KiB at 0xf90000/0xfa0000 (blank tail of BOOT_APP 1), the only
  SPI range our U-Boot can write (patch 0019). MX25U12835F: min 100,000 erase/program cycles per sector
  (datasheet Rev. 1.8 §14) = 200k saves; never write it periodically. Linux: `dmc12-uboot-env`. `u-boot-port.md`
- **SPI upgrade path:** our U-Boot in IPXE 0/1 (ours for good, owner 2026-11-15; UBOOT 0/1 stay factory) + BOOT_MODE (64 KiB TOC slot, first sector used); factory
  UBOOT stays as the `u` escape. Installed on u01 [hw]; `spi-upgrade.py`, `spi-upgrade-plan.md`.

## Linux / NixOS quirks
- Firmware PSCI is **v0.1** (cpu_on/off only, no SYSTEM_RESET). sbclk is **500 MHz** (mainline dtsi
  says 1 MHz). I2C bus numbers pinned by DT aliases to the factory numbering. [hw] DTS in
  `nix/packages/linux-alpine/boards/dmc12/`.
- **Reboot:** full board reset via CPLD 0x29/0x28 from a kernel reboot notifier (`dmc12_cpld_reset`);
  systemd-shutdown hooks don't run in NixOS's exitrd. Fallback sp805 = SoC-only reset. [hw]
- **GPIO 37 is the uC mailbox doorbell: never drive it** (named `UC_MAILBOX_DOORBELL_DO_NOT_USE`).
- **PSU1 alarm** on single-PSU units holds the CPLD interrupt line low; interruptmon masks it, and a
  masked alarm reads "not asserted" in 0x20 (use `dmc12_psu_present`). [hw]
- **K2 uC sensor runs warm** (~50 °C idle, passively cooled): it gets its own 60 °C target; board
  sensors keep the stock 45 °C. [hw] `nix/modules/thermal/dmc12-fan.nix`
- **Fans are nonlinear** (≈2000 RPM at step 9 floor, 3200 at 37 %, 14400 max); AS0 fans alarm below
  2000 RPM. [hw]
- **Disk writes at idle:** ~0.6 MiB per 5 min after quieting the journal (standing alarms logged once,
  timer units `LogLevelMax=notice`); was 9.6 MiB. [hw] Check with `/proc/diskstats` before adding timers.
- **Route A ≠ cold SPI boot:** it can't show SPI controller state left by stage3. [hw] `uart-boot.md`
- **USB/serial: INTx only through the PCIe port's IOFIC.** The Pericom OHCI/EHCI (no MSI: kernel quirk)
  raise INTx messages; on the x8 port they set cause bits 7..4 (INTA..D) of app interrupt group A at
  0xfd803000, which raises GIC SPI 47 (`amazon,al-fic`, `AL_FIC`; DT in `linux-alpine/boards/dmc12`).
  Mapped straight to a GIC line they never fired (2026-11-15): USB ran on EHCI's 100 ms watchdog, so
  sustained 115200 input lost ~62 % (CP2108 overrun; consoled or plain `cat`), enumeration took ~21 s
  and opening all 48 ports ~56 s. Fixed (494d188): enumeration 4 s, 48 opens 0.45 s. A sibling Alpine
  board with external-PCIe INTx needs the same node. Check: `grep hcd /proc/interrupts` counts up. [hw]
- **USB serial enumeration takes ~4 s** after boot; wait for 48 `/dev/P*` before tests. [hw]
- **Kernel:** nixpkgs' aarch64 config builds ~50 foreign SoC families; `linux-alpine` minimal (u01) or lean.
- Boot timeline: SPI install (stage3 → ours) login ≈ 38 s; the old chainload path (factory countdown +
  BOOTP preamble, still the `u` path) ≈ 64 s. [hw] `inventory/u01/soak.jsonl`

## Lab / tooling quirks (bit us here)
- tio block-buffers its log: watch the tmux pane for prompts; `clear-history` keeps the visible
  screen, so count matches against a baseline. `console.py` clears history per command.
- zsh doesn't word-split `$VAR`; `pkill -f`/`pgrep -f` can match the calling shell.
- Host firewall: HTTP for U-Boot `wget` on 8000/8080 (8608 blocked). U-Boot `dhcp` overwrites
  `serverip`: set it after `dhcp`.
- Dirty liquidation units carry operator config (keep images local; identifiers in `*-private.txt`).
