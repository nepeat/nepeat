# CS8210-32X: upstream U-Boot (Alpine V3)

Goal (owner): replace the vendor U-Boot 2018.05 with our own upstream-based one, as on the DMC-12
([../dmc-12-48r-as0/u-boot-port.md](../dmc-12-48r-as0/u-boot-port.md)), for a standard boot. Inputs:
[vendor-uboot.md](vendor-uboot.md) (what the vendor U-Boot does, items #1-27), [boot-chain.md](boot-chain.md)
(where an app can go in SPI), [uart-recovery.md](uart-recovery.md).

## Status (2026-11-16)
- **Milestone 1 (offline) done.** U-Boot 2026.10 RAM build `#uboot-cs8210`: console, DRAM banks from
  the DT, arch timer (firmware sets CNTFRQ = 75 MHz, read from Linux), boot SPI NOR read-only, the
  board's Linux DTS as control DT, env in RAM, booti/bootm/extlinux/boot.scr/bootmenu, AHCI SSD, PSCI
  reset. Tests `#uboot-alpine-v3-tests` pass (static + QEMU). Nothing run on the box yet.
- **Milestone 2 (hardware, RAM only) passed 2026-11-16** ([uboot-m2-20261116.txt](uboot-m2-20261116.txt)): vendor
  prompt → loadx → `booti` → ours at **EL2**, MMU off, 2/8 GiB; SSD, SPI NOR crc32 of mtd0/1/4 = the dumps (incl. the
  region above 16 MiB); NixOS + SONiC (BGP/BFD/LACP) as on the vendor boot. Differences: 00:06.0/00:10.0 left with
  Mem+/BusMaster+ by U-Boot's PCI auto-config, the disabled uart1-3 keep clock 0, 280 KiB less free memory.
  The box is back on the vendor boot.
- Chainloaded from the vendor prompt, Linux needs nothing from us beyond today's path (the vendor
  U-Boot has already done PCIe, AVS, GPIO, MUIO, MAC before its prompt). A standalone build (stage3's
  app) needs the items marked **standalone** in the [checklist](#checklist).

## What exists
- Patches (shared series, [../../nix/packages/uboot/default.nix](../../nix/packages/uboot/default.nix)),
  all new files except two Kconfig hunks and one spi-mem table line; the Alpine V2 builds are unchanged
  (same drvPaths, DMC-12 stage3app `.fimg` d3df4b80):
  - 0024 `alpine-v3`: `TARGET_ALPINE_V3`, board/amazon/alpine-v3 (copy of the V2 model, 0014): PIE, Linux
    Image header with text_offset = 0x08000000 (vendor `booti` runs it in place), MMU-on `go` entry
    handled in `save_boot_params`, mem_map from `/memory`, relocation below `/reserved-memory`,
    `ft_board_setup` copies only the reserved ranges the OS DTB lacks, load addresses = the NixOS
    bootmenu's (below). Banner line `Entry: EL<n>, MMU+caches were …, x0=…`.
  - 0025 `alpine-v3`: `SPI_MEM_READ_ONLY_4B`: the read-only filter (0019) also passes 0x13/0x0c. The
    MX25U25635F (32 MiB) is read with 4-byte opcodes only; without this `sf read` is refused.
  - 0026 `cs8210`: `alpine-v3-cs8210-u-boot.dtsi` (7 unused AHCI functions disabled for U-Boot,
    boot SSI 25 MHz + `spi0` alias) and `alpine_v3_cs8210_defconfig`.
- Builds ([../../nix/packages/uboot/alpine-v3/](../../nix/packages/uboot/alpine-v3/default.nix)):
  `nix build path:./nix#uboot-cs8210` → `u-boot.bin` (399336 B, sha256 `4bcabba8…`, crc32 `1d7676cc`),
  `u-boot.uimage` (kernel type, load = entry 0x08000000, `54cbd80b…`), `u-boot`, `.map`, `.dtb`, `.config`.
  `#uboot-alpine-v3-qemu` = same code + config on a QEMU fixture DT.
- Control DT = `alpine-v3-cs8210.dts`, a byte copy of nepeat/nix `nixos/module/cs8210/kernel/dts/amazon/`
  (a `path:` flake cannot read it). After any edit there, from this directory: `cmp ../../../nix/nixos/module/cs8210/kernel/dts/amazon/alpine-v3-cs8210.dts ../../nix/packages/uboot/alpine-v3/alpine-v3-cs8210.dts`
  and copy. `alpine-v3.dtsi` comes from U-Boot's dts/upstream (identical to Linux 7.3.4's).
- Tests `nix build path:./nix#uboot-alpine-v3-tests`:
  - **static**: ELF/Image/uImage headers, footprint < 2 MiB below 0x10000000, smc/hvc only in the SMCCC
    helpers (DT method smc), config (no env storage, no SF write/lock/SFDP/soft-reset code, no net/EFI/MMC,
    4-byte reads on), symbols, DT (4 banks, 4 firmware ranges, UART0 500 MHz pre-reloc, PSCI, AHCI, SSI).
  - **qemu-virt** (Cortex-A72, EL2): booti-style entry; 2 DRAM banks; relocation below a reserved top;
    `bootflow scan -lb` → AHCI (disabled function not probed) → MBR → bootable ext4 → `/boot/boot.scr`
    with the NixOS launcher's own commands → `env import -t` → `bootmenu` → `booti` of a probe with a
    mkimage-wrapped initrd: probe sees EL2, a valid FDT, the missing reserved range and not the present
    one; then MMU-on `go` entry; `reset` through PSCI back to a banner. QEMU proves code paths, not caches.
  - Not offline-testable: the DW SSI with 4-byte reads (QEMU/sandbox model none), the 16550 at 500 MHz,
    the real PSCI monitor: M2 covers them.

## Memory and entry
- Entered at EL2 (expected: V2 measured EL2; Linux reports "started at EL2"; the PSCI v0.2 monitor sits
  in secmon@0). If the banner ever says EL3, stop: our vectors would shadow the monitor.
- Vendor side: U-Boot at 0x01100000, relocated ~0x03ac0000-0x04000000. Ours is loaded at 0x08000000 and
  relocates to the top of bank 0 (below 0x80000000); `DRAM:  2 GiB (total 8 GiB)`.
- Env (board/amazon/alpine-v3/alpine-v3.env): scriptaddr 0x07000000 (the launcher puts the menu at
  0x07100000), pxefile 0x07200000, kernel 0x08000000, fdt 0x10000000, initrd 0x20000000 (used in place,
  initrd_high = ~0), kernel_comp 0x40000000 + 128 MiB. fdt_high is unset: the FDT is copied below
  U-Boot (2026.10 warns about ~0).
- The NixOS launcher (nepeat/nix `module/cs8210/uboot-menu`) runs unchanged; its `scsi init` is a
  vendor-only verb and prints the `scsi` usage here (harmless; `scsi scan` works on both).

## Checklist
Item numbers = [vendor-uboot.md](vendor-uboot.md). "Chainload" = ours started from the vendor prompt
(M2); "standalone" = ours as stage3's app (no vendor U-Boot). ✓ done · — not needed · ☐ to do.

| # | item | chainload | standalone | plan / note |
| --- | --- | --- | --- | --- |
| 1 | entry, DRAM banks | ✓ | ✓ | `/memory` 4 banks; EL printed |
| 2 | SerDes | — (stage3) | — (stage3) | |
| 3 | external PCIe RC + link | — (vendor trained, we never touch ports 0/1/4) | ☐ | V3 HAL PCIe init as a separate HAL copy (MikroTik GPL drop, V3 platform), driver on `amazon,al-alpine-v3-pcie` like 0017; port params from `pci_init_board_external` 0x0110d720 (port 0 Gen3 x4, 1 Gen2 x1, 4 Gen1 x1, CRS 30 s, 4 retrains) |
| 4 | PCI state at handoff | — | — | Linux enables decode; no device has FLR |
| 5 | ASIC AVS | — (vendor wrote it before its prompt; 11-15 read: VOUT 0x0333) | ☐ | needs #3 (fuses at BAR0 + 0x11178); PMBus PAGE 0 + VOUT_COMMAND on 0x62 behind 0x70 ch2 → 0x74 ch0 → 0x72 ch2; read-compare-write. Or a Linux oneshot before the SDK (owner's call) |
| 6 | 31 board GPIOs | — | ☐ | DT table + PL061 replay as V2 0020 (value, dir, value) |
| 7 | SPI mux 52/54 | — | — | Linux `gpio-mux` drives it; #6 leaves 52=0/54=1 |
| 8 | SSI CS override | ✓ (boot SSI, 0018) | ✓ | aux SSI is Linux's |
| 9 | MUIO pinmux | — | ☐ | vendor `pinctrl_init` list (uart_1, spim_aux, i2c_gen, NAND, SGPO pins); V3 MUIO layout from the HAL |
| 10 | SB PLL ch 9 = 25 MHz | — | ☐ | one HAL call; consumer unknown; compare `md.l 0xfd860b00 0x40` with vendor-uboot-checks |
| 11-12 | I2C controllers, mux tree | ✓ (DM from the DT) | ✓ | |
| 13 | ONIE vars / env saveenv | — | — | our build never writes the env (no env storage) |
| 14 | base MAC in EC entry 0 | — (vendor eth init stored it) | ☐ | TLV 0x24 (0x56 behind 0x74 ch0 → 0x71 ch7) → EC MAC entry 0, as V2 0015 does for V2 |
| 15-17 | al_eth params, PHY, retimers | — | — | stage3 params; Linux marvell driver; no retimers |
| 18 | SATA / AHCI | ✓ | ✓ | 00:0b.0 only (port 3 = SSD) |
| 19 | sys LED, console logger bit | — | ☐ optional | CPLD 0x60 reg 0x20 = 0x7d, reg 0x40 bit 6 |
| 20-21 | LM75 limits, fault log | — | — | power-on defaults; diagnostic only |
| 22-23 | kernel DT fixups, CCU | — | — | the DTS carries the real clocks; no `dma-coherent` |
| 24-25 | TPM, SGPO | — | — / optional | |
| 26 | fans | — | ☐ optional | the quiet recipe at U-Boot time (PROGRESS fans note), owner's call |
| 27 | boot flash, env | ✓ read-only (0019 + 0025), env RAM | ✓ | write window stays off |

Permanent scheme after M2 (owner's choice, [boot-chain.md](boot-chain.md) §7): C = the vendor boot.scr
loads our u-boot.bin from the SSD and `booti`s it (no SPI write). Our U-Boot finds the same
`/boot/boot.scr`, so the launcher must branch on a vendor-only variable (e.g. `dt_is_from_toc`) or ours
needs its own boot file (extlinux), or it chainloads itself again. A' (TOC2 + our app) needs the
standalone items first.

## Milestone 2 runbook
Run 2026-11-16 (results above); repeat only after the parent's go (SONiC tests and DMC-12 console reboots coordinated). RAM only; every
step below writes nothing to SPI or the SSD. Hang anywhere: power-cycle lab plug 1 (factory boot).
1. Build (from ai_re): `R=$(nix build --no-link --print-out-paths path:./nix#uboot-cs8210)`.
2. Baseline on today's vendor boot: `sudo devices/cs8210-32x/scripts/uboot_chainload.py capture vendor`.
3. `sudo devices/cs8210-32x/scripts/uboot_chainload.py run $R`:
   ssh reboot → `123` at "Type 123<ENTER> to STOP autoboot" → `ALPINE_DB>` → `loadx 0x8000000` +
   XMODEM (`uart_recovery.py`, ~40 s) → `crc32 0x8000000 0x617e8` = the file's → `booti 0x8000000 - ${fdtaddr}`
   (`--entry go` for `go 0x8000000`) → stop our autoboot → checks: banner EL2 and 2/8 GiB, `pci`,
   `scsi scan` (SSD), `part list`, `ls scsi 0:2 /boot`, `sf probe 0:0` + crc32 of mtd0 `73264376`, mtd1
   `8b4a0ac9`, mtd4 `d6937f49` (= the live /dev/mtd on 2026-11-16; mtd4 crosses 16 MiB), GPIO bank 6, `bootflow scan -l`
   → `boot` → bootmenu → Linux → login prompt → cage guard line in the journal. `--no-boot` stops at our prompt.
4. `sudo …/uboot_chainload.py capture ours`, then diff `analysis/uboot-compare/{vendor,ours}/`: dmesg
   (minus timestamps), lspci -vvv, the live DT, links + MACs, AVS VOUT, GPIO, SONiC (BGP/BFD/LACP), boot timeline.
Manual fallback: the same commands over `sudo ssh -t console@10.60.2.10 cs8210` (^] w takes write).
Expected differences: the vendor's DT fixup messages and TPM extend errors are gone; `/chosen` gets
U-Boot 2026.10 properties.

## Open
- Hardware, not run yet: PSCI SYSTEM_RESET on the real monitor, the `go` entry.
- Cosmetic: disable 00:06.0 and 00:10.0 in the U-Boot DT too (nothing in U-Boot uses them); the 280 KiB less free memory.
- Standalone items (☐ above), smallest first: GPIO + MUIO + SB PLL, then PCIe RC (V3 HAL), then AVS, MAC.
