# CS8210-32X: upstream U-Boot (Alpine V3)

Goal (owner): replace the vendor U-Boot 2018.05 with our own upstream-based one, as on the DMC-12
([../dmc-12-48r-as0/u-boot-port.md](../dmc-12-48r-as0/u-boot-port.md)), for a standard boot. Inputs:
[vendor-uboot.md](vendor-uboot.md) (what the vendor U-Boot does, items #1-27), [boot-chain.md](boot-chain.md)
(where an app can go in SPI), [uart-recovery.md](uart-recovery.md).

## Status (2026-11-17)
- **Milestone 1 (offline) done.** U-Boot 2026.10 RAM build `#uboot-cs8210`: console, DRAM banks from
  the DT, arch timer (firmware sets CNTFRQ = 75 MHz, read from Linux), boot SPI NOR read-only, the
  board's Linux DTS as control DT, env in RAM, booti/bootm/extlinux/boot.scr/bootmenu, AHCI SSD, PSCI
  reset. Tests `#uboot-alpine-v3-tests` pass (static + QEMU). Nothing run on the box yet.
- **Milestone 2 (hardware, RAM only) passed 2026-11-16** ([uboot-m2-20261116.txt](uboot-m2-20261116.txt)): vendor
  prompt → loadx → `booti` → ours at **EL2**, MMU off, 2/8 GiB; SSD, SPI NOR crc32 of mtd0/1/4 = the dumps (incl. the
  region above 16 MiB); NixOS + SONiC (BGP/BFD/LACP) as on the vendor boot. Differences: 00:06.0/00:10.0 left with
  Mem+/BusMaster+ by U-Boot's PCI auto-config, the disabled uart1-3 keep clock 0, 280 KiB less free memory.
  The box is back on the vendor boot.
- **Milestone 3 (standalone code, RAM rehearsal) 2026-11-16** ([uboot-standalone-20261116.txt](uboot-standalone-20261116.txt)):
  `#uboot-cs8210-selfinit` does the vendor board init itself (MUIO, 32 GPIOs, SB PLL ch 9, base MAC, external
  PCIe RC init with AVG SerDes iCal, decode off at handoff), env in blank SPI. From the vendor prompt: every part
  idempotent (0 changed), all three links torn down and trained by our init (Gen3 x4 / Gen2 x1 / Gen1 x1), then
  NixOS + SONiC (BGP/BFD/LACP) with lspci identical to the vendor boot; PSCI reset works. One bug found and fixed
  (MDIO FPGA BAR, below).
- **A' installed 2026-11-17** (slot A, installer agent, spi-upgrade-install-20261117.txt): stage3 enters ours at EL2,
  x0 = 0; MUIO/GPIO/PLL/MAC write paths ran (22/17/1 changed, MAC stored); AVS oneshot enables the ASIC itself.
  Cold-boot bug: the MDIO FPGA had no BARs in Linux (Credo ports down). Cause and fix
  ([uboot-standalone-20261117.txt](uboot-standalone-20261117.txt)): its BAR 0 keeps only bits 31:29/21 and resets to
  0xe0200000 = its sizing read-back, which Linux rejects; the vendor writes e0000000 first. Our U-Boot now writes the
  vendor's partner BARs (`amazon,endpoint-bars`), RAM-tested from the forced cold value: NixOS + SONiC + Credo as
  on the vendor boot. Fixed `#uboot-cs8210-stage3app` (.fimg 58e4a4f0…) waits for the slot B update; slot A is the
  old one. Rescue proven: power cycle + `u` → factory U-Boot (3 times), warm reset + `u` likewise.
- **Never `alpcie reinit 4` twice per power-on:** repeated retrains wedge the MDIO FPGA link until a power cycle
  (even the factory U-Boot then reports "PCIE_4 no link found"). A port without a link is now disabled in the OS DTB
  (Linux pcie-al panicked on its DBI otherwise); that path is not hardware-tested.

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
  - 0027/0028 (installer agent): write windows (6 ranges, 4-byte chips, sf erase/write), `cs8210-installer`.
  - 0029 `alpine-v3`: `board/amazon/alpine-v3/soc-init.c`, one `amazon,alpine-v3-soc-init` node: MUIO (8 mux_sel
    registers), GPIO list (pin to MUIO fn 0, then value/dir/value), PLL channel dividers (HAL relock sequence), base
    MAC (TlvInfo 0x24, CRC checked) into EC MAC entry 0 of 00:01.0; `alpine state|muio|gpio|pll|mac`. Each part
    writes only fields that differ.
  - 0030 `alpine-v3`: `pcie_alpine_v3` on `amazon,al-alpine-v3-pcie`, a HAL copy of its own
    (`drivers/pci/al_pcie_v3`, MikroTik GPL HAL built for V3: PCIe rev 4, AVG SerDes lanes, IOFIC; no SBus firmware),
    `al_init_pcie_v3_rc.c` = vendor `al_init_pcie` RC path with the V3 template the vendor binary carries (EQ table
    0x011cd1f0). Firmware link kept, else init with the vendor fallback (x4 -> x2 -> x1 -> Gen1); pre-delay 100 ms,
    link-up 100 ms, CRS 30 s; SError drain as V2; link partner COMMAND = 0 at OS handoff; `alpcie status|reinit`.
    `PCIE_ALPINE_V3_FORCE_INIT` (test only): init every port at probe.
  - 0031 `cs8210`: DT for the above (26 MUIO pins, the 32 gpio-list triplets verbatim, PLL ch 9 divider 120, the
    TlvInfo EEPROM, ports 0/1/4 gen/width/SerDes lanes 0-3/8/4), link partners and 00:06.0/00:10.0 disabled for
    U-Boot, `pci0` alias; all under `#ifdef`, so `#uboot-cs8210` stays byte-identical (4bcabba8).
- `#uboot-cs8210-selfinit` (alpine-v3/default.nix `selfinitConfig`): + redundant env 2 x 16 KiB at 0x38000/0x3c000
  (blank in the 0x01c000-0x040000 gap, clear of the TOC2 and BOOT_MODE sectors), the one write window 0x38000+0x8000
  (4-byte), trimmed (FDT/LZMA/ELF/XIMG/LOADS/MEMINFO cmds) to fit slot B. `#uboot-cs8210-stage3app`: that
  u-boot.bin wrapped by alboot.py (id 0x110, load = exec 0x08000000), 0x66a5c B, **1444 B spare in slot B**.
  The installer's windows do not include the env (0x38000+0x8000): add it there if the installer should seed it.
- Builds ([../../nix/packages/uboot/alpine-v3/](../../nix/packages/uboot/alpine-v3/default.nix)):
  `nix build path:./nix#uboot-cs8210` → `u-boot.bin` (399336 B, sha256 `4bcabba8…`, crc32 `1d7676cc`),
  `u-boot.uimage` (kernel type, load = entry 0x08000000, `54cbd80b…`), `u-boot`, `.map`, `.dtb`, `.config`.
  `#uboot-alpine-v3-qemu` = same code + config on a QEMU fixture DT. `#uboot-cs8210-installer` (0027/0028:
  boot SPI writable only in the scheme A' windows, prompt `cs8210-installer> `) and its sandbox twin are
  the SPI writer of [spi-upgrade-plan.md](spi-upgrade-plan.md). Installed 2026-11-17 as stage3's app; since 03:47Z slot B
  (BAR fix 93662b1) boots NixOS + SONiC healthy cold and warm ([spi-upgrade-update-20261117.txt](spi-upgrade-update-20261117.txt)).
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
  - **pcie-init** (./pcie-test): `al_init_pcie_v3_rc.c` + the HAL copy out of 0030 on a host register model (rev-4
    unit, PBS, 4 AVG lanes with a core-interrupt mailbox): cold Gen3 x4 in the vendor order incl. per-lane FW
    CRC/rev reads, the 8 iCal steps interleaved, PCS interrupts on; x2; skip-iCal; "already started"; reinit;
    stuck link (teardown).
  - **selfinit**: config, symbols, DT of the selfinit build; the chainload DT has none of it; the app object =
    u-boot.bin verbatim and fits slot B.

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
| 3 | external PCIe RC + link | ✓ kept (0030) | ✓ RAM-rehearsed | 0030; vendor `pci_init_board_external(teardown 1, reinit 0, ical on)`: no retry (rc-link-retry absent), on failure halve lanes then Gen1. Torn down + our init on all three, Linux/SONiC fine |
| 4 | PCI state at handoff | ✓ | ✓ | partners: the vendor's BARs written once the link is up (`amazon,endpoint-bars`; the MDIO FPGA needs it), COMMAND 0 at OS prepare (vendor: FPGAs 0, ASIC 6 for its AVS); no U-Boot auto-config of the endpoints; ports without a link disabled in the OS DTB; no FLR |
| 5 | ASIC AVS | — (vendor wrote it before its prompt; 11-15 read: VOUT 0x0333) | — (Linux) | owner: Linux oneshot before the SDK (avs.md); our U-Boot leaves ASIC COMMAND 0, so the oneshot must enable the device before reading resource0 |
| 6 | 32 board GPIOs | ✓ (2 changed: 52/54 after the vendor booti) | ✓ | 0029/0031; the vendor list has 32 triplets (not 31) |
| 7 | SPI mux 52/54 | — | — | Linux `gpio-mux` drives it; #6 leaves 52=0/54=1 |
| 8 | SSI CS override | ✓ (boot SSI, 0018) | ✓ | aux SSI is Linux's |
| 9 | MUIO pinmux | ✓ (0 changed) | ✓ | 0029/0031, 26 pins; = the vendor's mux_sel words |
| 10 | SB PLL ch 9 = 25 MHz | ✓ (0 changed) | ✓ | divider 120 from the DT (vendor result 0x7078); consumer still unknown |
| 11-12 | I2C controllers, mux tree | ✓ (DM from the DT) | ✓ | |
| 13 | ONIE vars / env saveenv | — | — | our build never writes the env (no env storage) |
| 14 | base MAC in EC entry 0 | ✓ (kept) | ✓ | 0029: TLV read + CRC + EC BAR 4 +0x868 proven equal to the vendor's store; the write path itself not yet exercised |
| 15-17 | al_eth params, PHY, retimers | — | — | stage3 params; Linux marvell driver; no retimers |
| 18 | SATA / AHCI | ✓ | ✓ | 00:0b.0 only (port 3 = SSD) |
| 19 | sys LED, console logger bit | — | ☐ optional | CPLD 0x60 reg 0x20 = 0x7d, reg 0x40 bit 6 |
| 20-21 | LM75 limits, fault log | — | — | power-on defaults; diagnostic only |
| 22-23 | kernel DT fixups, CCU | — | — | the DTS carries the real clocks; no `dma-coherent` |
| 24-25 | TPM, SGPO | — | — / optional | |
| 26 | fans | — | ☐ optional | not done: the quiet recipe at U-Boot time (PROGRESS fans note), owner's call |
| 27 | boot flash, env | ✓ read-only (0019 + 0025), env RAM | ✓ selfinit: env SPI 0x38000/0x3c000 | one window (env) via 0027; `saveenv` never run yet (zero SPI writes) |

Permanent scheme after M2 (owner's choice, [boot-chain.md](boot-chain.md) §7): C = the vendor boot.scr
loads our u-boot.bin from the SSD and `booti`s it (no SPI write). Our U-Boot finds the same
`/boot/boot.scr`, so the launcher must branch on a vendor-only variable (e.g. `dt_is_from_toc`) or ours
needs its own boot file (extlinux), or it chainloads itself again. A' (TOC2 + our app): the standalone
build exists and passed its RAM rehearsal (milestone 3); open are the AVS oneshot in Linux and the install.

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

## Standalone: rehearsal and what is still unproven
- RAM rehearsal (milestone 3, repeatable, zero SPI writes): vendor prompt → `loadx 0x8000000` the selfinit
  u-boot.bin → `booti 0x8000000 - ${fdtaddr}` → expect the `MUIO:/GPIO:/PLL:/MAC:` lines with 0 changed (GPIO 2:
  52/54), links "firmware init, kept". `alpcie reinit 4` is the last-probed port; reinit of 0 or 1 renumbers U-Boot's
  bus (root port bus regs no longer 1): `reset` after it, never boot an OS. Full cold-path rehearsal of all three
  links + Linux: a dev build with `CONFIG_PCIE_ALPINE_V3_FORCE_INIT=y` (run 2B of the evidence file).
- Stage3 entry contract (boot-chain.md §2/§7): stage3 loads the object to its header load address and the AArch64
  agent enters exec; on V2 that was EL2, MMU off, x0 = 0, on V3 not observed. Our entry handles EL2 with MMU on
  or off and prints `Entry:`; an EL3 banner means stop (our vectors would shadow the monitor).
- Route A (stage3's "UART recovery" RAM reader at 0x01800000) as a rehearsal: assessed, not built. It needs stage3
  re-run in AArch32 on CPU0 (the Route R warm-reset path, never run on hardware), the SPI_LOAD/XMODEM_LOAD_ENFORCE
  flags located in stage3 (not traced yet), and stage3 would redo the SerDes firmware download and the DRAM clear
  under live links. Cheaper partial step if wanted: re-enter the AArch64 agent (exec word 0xfbfe00f8 + CPU0 reset)
  to observe the real entry state. First real stage3 run = the A' install with `u` as the escape (boot-chain.md §7).

## Open
- Hardware, not run yet: the `go` entry; our U-Boot as stage3's app; `saveenv` into the 0x38000 window; the
  MUIO/PLL/MAC write paths from stage3's state; the 280 KiB less free memory (cosmetic).
- ASIC AVS before the SDK (Linux oneshot, other agent) is a precondition for a vendor-free boot.
- Slot B margin is 1444 B: growth means trimming (FAT, bootmeth distro) or slot A only.
