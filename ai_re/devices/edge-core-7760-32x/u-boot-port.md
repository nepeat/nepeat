# upstream U-Boot on the AS7760 — feasibility (2026-11-14)

**Short answer: yes, it looks portable.** Secure boot is not fused, upstream
still supports the T2080RDB, the NOR layout matches the RDB exactly, and a
GPL sibling port (Accton AS7710-32X in ONIE) documents most board deltas.
**The open problem is recovery, not porting:** NOR is the only boot source
we know of, and the CPLD "altbank" safety net looks absent. Nothing has been
written to NOR.

## gate 1: secure boot — clear

Read from Linux via /dev/mem (`CCSR 0xffe000000`):
- SFP OSPR (`+0xE8200`) = 0, so **ITS = 0**. SRK hash `+0xE823C..` = all zero (no OEM key).
- RCWSR7 SB_EN = 0.
- SVR `0x85300011` = **non-E T2080 ("without security")**; no secure boot possible.
- SEC monitor HPSR SSM_ST = 0x9 (non-secure).

An unsigned upstream U-Boot will be executed.

## gate 2: upstream support — present

nixpkgs U-Boot **2026.10** has `TARGET_T2080RDB` (`board/nxp/t208xrdb`, DM +
OF_CONTROL, `t2080rdb.dts`) and defconfigs for NOR/SD/SPI/NAND and revD.
`T2080RDB_defconfig` NOR layout vs this board's NOR map:

| item | upstream RDB | AS7760 NOR |
| --- | --- | --- |
| U-Boot | TEXT_BASE 0xEFF40000, 768 KiB | mtd7 `uboot` 0x7f40000, 768 KiB |
| env | 0xEFF20000, size 0x2000, sector 0x20000 | mtd6 `uboot-env`, fw_env `0x0 0x2000 0x20000` |
| FMan ucode | 0xEFF00000 | mtd5 `Fman-FW` 0x7f00000 |
| RCW | NOR offset 0 | mtd0 `RCW` (kept as-is; NOR boot, SB_EN=0) |

A port only replaces mtd7 (and probably resets mtd6). RCW/FMan ucode stay.

## board deltas (from ONIE `machine/accton/accton_as7710_32x/u-boot`)

That patch is a full board port on fsl-sdk-v1.7 U-Boot, the same base as our
vendor `2014.07 … 1.7.0.10`. 45 of its 78 board string literals appear in the
AS7760 vendor binary, incl. the checkboard format and `cpld reset altbank`.
Its RCW matches ours except word 6 (`fc027000` here vs `ec104000`).

- **DDR**: 2× DDR3 UDIMM, SPD at I2C0 **0x52/0x53**, ECC; board
  clk_adjust/wrlvl table; vendor hack forces `sdram_cfg = 0xe7040008`
  (our vendor banner prints `ddr_sdram_cfg = 0xe7040000` + "Prob-D").
  Highest-risk area: wrong leveling = unstable RAM.
- **VID must be off.** RDB enables `CONFIG_VID` (IR36021 core-voltage
  adjust over I2C); AS7710 has none. Leaving it on could write to an unknown
  I2C device or misprogram VDD.
- **CPLD**: RDB board code reads/writes a CPLD at IFC CS3 (`0xffdf0000`)
  for board rev, boot bank, resets, SerDes clocks. AS7760 vendor banner reads
  `Board rev: 0xff CPLD ver: 0xff` → nothing RDB-like answers there.
  Needs stubbing (patch) or a minimal own target.
- **Ethernet**: SerDes1 0x6c; DTSEC1 SGMII, BCM54616S at MDIO 0x1f; no
  Cortina/Aquantia 10G PHYs.
- **I2C**: 4 controllers; muxes 0x77/0x75/0x76 (AS7710).
- **MAC/serial**: ONIE TLV lives in NOR `hw-info` (0xe8020000), not I2C;
  upstream TLV support is I2C-EEPROM based → set ethaddr in env or a small patch.
- AS7710 also patched IFC clock reporting (+3 divider) and widened fsl_i2c to
  4 buses (upstream DM I2C already handles 4).

## risk / recovery (decide before any NOR write)

- Bad U-Boot in mtd7 = board doesn't boot = no Linux to reflash from.
- **altbank**: vendor CLI has it, but CPLD reads 0xff → probably not wired.
  Check read-only from U-Boot: `md.b ffdf0000 20`. (Linux /dev/mem EPERMs there.)
- JTAG/COP header: unknown, needs a physical look + an e6500-capable probe.
- External programmer: NOR is parallel (ID 0x89/0x227e, CFI cmd set 0002) →
  TSOP56 desolder/socket job.
- RCW source strapping to SD: unknown without a schematic.

## chainload results on hardware (2026-11-14)

`#uboot-as7760-chainload` = T2080RDB_SDCARD (RAM-linked second stage, TEXT_BASE
0x00200000) + patches in [ai_re/nix/packages/uboot/patches](../../nix/packages/uboot/patches) + config
fragment in [ai_re/nix/packages/uboot/default.nix](../../nix/packages/uboot/default.nix). Harness:
[chainload_uboot.py](chainload_uboot.py) (TFTP → flush L2 → `go` → checks).

**Reached the `=>` prompt on the real T2080**: CPU0 T2080 v1.1, e6500 v2.0,
4×1800 MHz, CCB 600, DDR 1600 MT/s, RCW decoded, `16 GiB (total 2 GiB)` from
both UDIMM SPDs (sized only, DDR not re-initialised), 42 DM devices, MMC sees
the SanDisk SDHC, I2C buses enumerate, FMan ucode loads from NOR, 16 KiB memory
fill/compare OK. Expected/benign: "CPU up timeout" (secondaries already
released by the factory loader), RDB PHYs/EEPROM not found, `md.b ffdf0000`
= all 0xff (**no RDB CPLD → no altbank**). The harness's last verdict line
said FAIL only because its DRAM regex expected one line (fixed since).

Issues found, in order (each fixed):
1. start.S checks CCSRBAR at the reset default, which after the factory
   relocation aliases DDR → `infinite_debug_loop`. **0001** adds
   `CHAINLOAD_CCSR_RELOCATED` (test-only Kconfig).
2. e6500 start.S flash-invalidates cluster L2 → harness sets `L2CSR0[L2FL]`
   from the factory prompt first (L1D is write-through). No patch.
3. RDB board code calls `adjust_vdd()`/`pci_init()` unconditionally → **0002**
   guards them (upstreamable). VID/PCI/misc_init_r (CPLD writes) are off.
4. Console garbage: `SYS_CLK_FREQ` default 66.66 MHz (board: 100 MHz) and
   **DM ns16550 used `get_bus_freq(0)` instead of /2** (T208xRDB.h only defines
   the /2 for !DM_SERIAL; factory divisor 0xa3 = 300 MHz confirms). **0003**
   (upstreamable bug fix) + `CONFIG_SYS_CLK_FREQ=100000000`.
5. RAMBOOT still reads SPD pre-reloc: **0004** `bootph-all` on i2c0
   (upstreamable), **0005** SPD 0x52/0x53 (AS7760 hack), `DIMM_SLOTS_PER_CTLR=2`,
   `SYS_MALLOC_F_LEN=0x1000` ("alloc space exhausted").

Tests (`#uboot-tests`, no board needed): static (ELF32 BE PPC, entry
0xeffff000 NOR / 0x200000 chainload, -mcpu=e6500, 0 SPE opcodes, NOR image
768 KiB + reset vector, chainload safety config) and QEMU `ppce500 -cpu e6500`
boot + shell + memory compare. Both pass.

## boots NixOS end to end (2026-11-14, still chainloaded; NOR untouched)

Factory U-Boot → `go` → upstream 2026.10 → **Linux with 8 CPUs, 16 GiB, both
BCM56930 PCIe functions, correct MAC, 0 failed units**, via three routes:
1. `bootm` of the SD files (boot.scr's commands),
2. **the factory environment's own ONIE `bootcmd`** after
   `env import -c 0xeff20000 0x2000` (boot_diag → check_boot_reason →
   nos_bootcmd → boot.scr): upstream U-Boot is env-compatible,
3. **standard boot: `bootflow scan -lb` → `/extlinux/extlinux.conf` on SD p1**
   (NixOS generic-extlinux generations menu → default entry).

More fixes, as patches/config:
6. **0006** AS7760 control DT (`as7760-32x.dts`): DTSEC1 SGMII + BCM54616S @
   MDIO 0x1f (generic PHY driver), unused MACs/SPI/RTC off. DHCP/ping/TFTP
   (8.9 MiB/s) work.
7. Linux oopsed at `smp_setup_cpu_maps`: secondaries were still spinning in
   the factory loader's overwritten spin loop. **0007** pulses their MPIC reset
   after BSTR points at the new boot page. Finding: **PIR reset bits are per
   hardware thread** (holding bit 1 hangs core 0); mask 0xfc = cores 1–3.
   Holding them from `cpu_init_early_f()` hung before the banner, so the
   patch pulses late instead.
8. Memory node was 2 GiB (`gd->ram_size` = U-Boot mapping). **0008**:
   `ft_board_setup()` uses the SPD size when `bootm_size` is unset (a value
   saved in `dram_init()` doesn't survive relocation).
9. Standard boot needs `CONFIG_BOOTSTD_FULL` (listing) and
   `CONFIG_SUPPORT_RAW_INITRD` (extlinux passes a raw initrd to bootm).
   Load addresses used: kernel 0x10000000, ramdisk 0x20000000, fdt 0x08000000,
   pxefile 0x07000000, script 0x07100000 (set by hand in the test).

No UEFI: U-Boot's EFI_LOADER is ARM/x86/RISC-V only and PowerPC Linux has no
EFI stub. extlinux/standard boot is the "modern standard" here; NixOS writes
it with its own `generic-extlinux-compatible` builder (see ai_re/nix/modules/boards/as7760/sd-install.nix).

## stage 2 is the default boot; DDR settings vetted (2026-11-14)

**Default boot now:** factory U-Boot 2014 (DDR init, first stage) →
`nos_bootcmd` = `mmc rescan; if fatload mmc 0:1 0x00200000 u-boot.bin; then
mw.l fec20000 c0000800; sleep 1; go 0x00200000; fi; fatload … boot.scr && source …`
(saved) → **upstream U-Boot 2026.10 from SD p1** (bootdelay 2) → imports
ethaddr/eth1addr/eth2addr/serial# from the factory env → `bootflow scan -lb`
(extlinux) → NixOS; falls back to boot.scr. Verified on power-on and on
`systemctl reboot`. `u-boot.bin` is installed by the NixOS bootloader hook
(nix-built, same derivation as `#uboot-as7760-chainload`).
Escape hatches: press a key at the stage-2 prompt; `123` at the factory one;
deleting `u-boot.bin` from p1 makes nos_bootcmd fall back to boot.scr.

**DDR vetting (`ddrcmp`, patch 0009):** computed-from-SPD vs live factory
setup. Stock RDB options: 15 registers differ (1T vs factory 2T, CS
interleave, write-leveling starts, turnarounds, 40 vs 34 Ω DIC, HSE …).
With **0010** (trrt/dic overrides) + **0011** (`CONFIG_AS7760_DDR`: 2T, HSE,
no CS interleave, RWT 3, RRT 2, DIC RZQ/7, BSTOPRE 0x100, DQS_CFG 0, wrlvl
7/0x0608060d/0x090c0a09) only ODT_PD_EXIT (1 vs 0) and DLL_RST_DIS remain.

**Cold DDR init without NOR (in progress):** `spltest` (0012) turns the CPC
into 512 KiB SRAM @0xfff80000, holds secondaries in reset, disables boot page
translation, copies the SDCARD SPL (entry _start 0xfffff000) and jumps. Dry run
verified (BSTR off, CPC SRAM, image intact). The real jump faults inside the
SPL's reset path (trap PC 0x87ac, MSR 0) before its TLB cleanup; not yet
understood. Main U-Boot for the SPL is in the SD gap at sector 520 (verified).

**Recovery hardware:** the boot flash is parallel CFI NOR (0x89/0x227e,
TSOP-56 class), not SPI: a SOIC-8 clip will not reflash it.

## flashed to NOR (2026-11-14)

Pre-flash review (Fable-class model) changes: board CPO honoured in A-009942
(0009), DDR trained like the factory sequence then post-init pokes (0010),
A-008109 skipped, memory fixup from `gd->ram_size` (0008). IFC CS0/CS2 live
registers equal the RDB defines. Chainload hang root cause: L2 flash
invalidate destroying dirty image lines after `fatload` (0001 keeps the L2).

Flash, from the factory `LOADER=>` prompt (CFI write path it already uses for
saveenv), image must be exactly 0xc0000 bytes:

```
tftpboot 0x1000000 as7760-probe/u-boot-as7760-nor.bin ; crc32 1000000 c0000
tftpboot 0x2000000 as7760-probe/mtd7-factory.bin      ; crc32 2000000 c0000
cmp.b eff40000 2000000 c0000        # proves the address map before erasing
protect off eff40000 efffffff ; erase eff40000 efffffff
cp.b 1000000 eff40000 c0000 ; cmp.b 1000000 eff40000 c0000 ; crc32 eff40000 c0000
protect on eff40000 efffffff
```

That was the first flash (build `1e644c49…`). **Use
[flash_uboot.py](flash_uboot.py) now**; it runs this sequence with hardware
checks, a verified backup and rollback (`--dry-run` first, `--restore` to go
back). Second flash (build `2c0d7387…`, patch 0013) was done with it.

The new U-Boot reads the factory env unchanged (hwconfig there has no
bank_intlv/ecc overrides, so DDR options equal the tested defaults). Its
`bootcmd` chain ends in `nos_bootcmd`, now `bootflow scan -lb`; with a missing
env the built-in bootcmd is the same. Boot methods: extlinux, boot.scr, PXE.

### testing a build in RAM first

`powercycle_to_uboot.py`, then `chainload_uboot.py <uboot-as7760-chainload>/u-boot.bin`
(`go 0x00200000`; same board code and config as the NOR build apart from the
reset path and cold DDR init). Started from upstream U-Boot it needed three
fixes, all in patch 0001:

| symptom | cause | fix |
| --- | --- | --- |
| dies in the cache-as-RAM stack setup, intermittently | L2 kept enabled and full of the parent's lines | flush L2 (L2FL), then the normal invalidate + enable |
| silent hang when returning to AS0 | parent's DDR TLB (slot 8, IPROT, shrunk to 4K) overlaps our DDR entry; the factory loader's slot happened to be overwritten | drop all TLB1 entries except the AS1 temporaries before loading the table |
| hang right after `cpu_init_f` | `invalidate_cpc()` with DDR live and modified lines in the CPC | flush the CPC before invalidating |

Debugging notes worth keeping: early markers written straight to the UART THR
get dropped once the 16-byte FIFO is full (wait for LSR.THRE), and a debug TLB0
mapping of the UART overlaps the 16M CCSR entry in AS0 (multi-hit) — both
produced false "it dies here" readings.

Open: `WARNING: adjusting available memory` (stock lowmem cap, harmless);
restoring the factory image is unsupported (flash_uboot.py refuses it); SPL `spltest`
fault (moot); U-Boot update from Linux (mtd7 write path unproven).

## proposed path (nixpkgs `buildUBoot`, patches only)

0. **done** `#uboot-t2080rdb` ([ai_re/nix/packages/uboot/default.nix](../../nix/packages/uboot/default.nix)) — stock
   T2080RDB via nixpkgs `buildUBoot` 2026.10, `pkgsCross.ppc32` with
   `stdenvNoLibc`. `u-boot.bin` = 786432 bytes = mtd7 size; ends in a
   reset-vector branch like the vendor image. Stock config still has
   `CONFIG_VID=y`. Toolchain notes: ppc-embedded (eabi) libgcc refuses to
   link with -mrelocatable (crtsavgpr); ppc32 glibc in this pin fails its
   install (libc.so.6 rule runs twice in manual/), hence no-libc stdenv.
1. Minimal patch series on top: AS7760 defconfig (from T2080RDB, VID off),
   DT tweaks, CPLD stubs, DDR table/sdram_cfg from the AS7710 port.
2. **done** — reversible chainload trial reaches the prompt (above).
   Networking, Linux boot, env compatibility and extlinux done (above).
   Remaining before a NOR image: (a) NOR-boot build = T2080RDB_defconfig +
   0002–0006/0008 + the same fragment minus chainload bits, **with DDR init
   from SPD** (only path not yet exercised; consider testing via the SDCARD
   SPL running from CPC SRAM first); (b) a default env / `nos_bootcmd` that
   sets the load addresses and tries `bootflow scan -lb` before boot.scr;
   (c) fold 0005's SPD addresses and the board quirks into a real AS7760
   board target; (d) agree a recovery path (no CPLD altbank; JTAG/programmer?).
3. Only with an agreed recovery path: write mtd7 (+ env) from Linux, keeping
   the verified `dumps/mtd7.bin`/`mtd6.bin` backups.
