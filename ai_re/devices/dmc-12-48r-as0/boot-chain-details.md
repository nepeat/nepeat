# DMC-12 boot chain details: preloader EEPROM, stage2 DDR, DT objects, APCEA / BOOT_APP

Static analysis, 2026-11-15. Nothing was run on hardware for this file. The cross-device
reference (stage roles, what is board-specific, porting checklist) is
[../../alpine-v2-boot-chain.md](../../alpine-v2-boot-chain.md); this file holds the u01
evidence behind it. Object format, handoff and the U-Boot plan: [spi-uboot-replacement.md](spi-uboot-replacement.md).
Recovery, OTP, CS1: [spi-recovery.md](spi-recovery.md). Boot ROM and UART boot: [uart-boot.md](uart-boot.md).

**Evidence levels:** *proven* = seen on u01 hardware (log or register dump); *code* = read in
the disassembly (addresses below); *inferred* = from code shape, data or the HAL, never observed.
`HAL/` = `devices/cs8210-32x/analysis/research/mikrotik-added/drivers/soc/alpine/HAL/` (v2.6).

**Decoders** (stdlib only, offline):
- `alpine_preload.py dumps/u01/spd-0-0057.bin` decodes the EEPROM; `--strap 0x03bfdef5` decodes the strap.
  It hides MACs/serials unless `--show-ids`. The SPI `EEPROM` object (`.fimg`) works as input too.
- `alpine_dtobj.py dumps/u01/spi-cs0/dt.0.bin` prints a board-cfg summary; `--diff A B`, `--raw A [B]`.
  It also reads the CS8210 (Alpine V3) DT object.

## Ghidra projects (`ghidra/`, git-ignored; open with `tools/ghidra` + `ghidra/scripts/gh_q.py`)
| project | programs | origin |
| --- | --- | --- |
| `dmc12.gpr` | factory U-Boot 2015.07 2.60.4 (AArch64, 0x01100000) | earlier U-Boot work |
| `stg-spi/stg.gpr` | stage2 A `stg2a.bin` (k2 2.60.3), stage2 B `stg2b.bin` (ep 2.60.4), stage3 `stg3.bin`, agent `agent.bin` | `/tmp/dmc12spi` (spi-uboot-replacement) |
| `stg-rec/stg.gpr` | stage2 A, stage3 (recovery-path annotations) | `/tmp/dmc12rec/gh2` (spi-recovery) |
| `stg-cs1/stg.gpr` | CS1 stage2 `ep v2.44` (`bin/cs1-stg2-2.44.bin`) | this file |

`ghidra/scripts/`: `gh_q.py` (decompile by string/address, xrefs), `thumbfn.py` (force a Thumb
function), `gh_import.py`, `carve.py`. Thumb code in stage2 needs `thumbfn.py` at the `push` address.

## 1. The I2C preloader EEPROM (0x57, 24c64, 8 KiB)
**It is both: a hardware preload table and the DRAM SPD, plus stage2's early-init info.**

| offset | content | who reads it |
| --- | --- | --- |
| 0x000–0x11f | preload table: header, 22 register writes, footer (12-byte entries `{u32 data, u64 addr}`, magic `0x1c361c36` / `0x1c361c361c361c36`) | the PBS I2C preloader, before the CPU runs (strap bits 13:12 = 1, bit 23 → 0x57, bits 27:26 → 400 kHz) |
| 0x120–0x31f | 0x200 B, high entropy | nobody found. Not a byte sum and not stage2's RSA key. *Inferred*: a signature for secure-boot units |
| 0x400–0x43f | early-init info, magic `0xaa` | stage2 (offset from the SRAM record, below) |
| 0x440–0x5bf | DDR4 SPD of the soldered-down DRAM | stage2 (`SPD I2C Address: 57, offset 0440`) |
| 0x600 | `al_eeprom_board_data` slot (magic 0xCC: MACs, PCB rev, SN, PN) | **blank (ff) on u01**; APCEA's PCI VPD and stage2 2.44 would read it |

**Preload writes on u01** (full list: `./alpine_preload.py dumps/u01/spd-0-0057.bin`):
- CPU PLL (0xfd860d00) NF=20 → **2000 MHz**; NB PLL (0xfd860c00) NF=32 → 3200 MHz, channel 0 div 3 →
  **1066 MHz DDR clock** (div1 12). The values equal HAL `al_hal_pll_map.h` entries; dev_info prints
  cpu 2000 / ddr 1066 (*proven*).
- SPI master BAUDR (0xfd882014) ← 0xe (SPI flash clock); PBS `cfg_axi_conf_1/2` ← 0x1c000000;
  NB `qos_set_control` ← 0x7171; SGPO +0xc ← 7.
- PBS `cfg_nand_0/1` ← 0xb4510002 / 0x10100000: the ROM's NAND geometry (4 KiB pages, 64-page
  blocks, BCH). Unused on an SPI boot; *inferred* a reference-design default.
- **The SRAM dev-info record** 0xfbff4100–0x411f: `ff39a514 bffffdfd 0400fffb 0 0` + `"v2sil-39-rc1"`.
  Byte 0 ≠ 0 tells stage2 to take the early-init offset from bytes 0xa/0xb (= 0x0400), else 0x400
  (*code*: stage2 A 0xf220c5d8). The string is the preloader image version (*inferred*).
- One entry is unexplained: `[0xf5_7f89_6100] ← 0x20000000`. Bits 39:32 are set, which no V2
  register has. *Inferred*: an attribute/command form of the preloader, not a plain write.
- **Hardware cross-check (*proven*):** `inventory/u01/spi-uboot-regs.txt` shows 0xfd8a8118/811c =
  0x1c000000, 0xfd8a81b4/81b8 = 0xb4510002/0x10100000, SGPO 0xfd8b400c = 7. The preloader status
  `i2c_pld_status_low` (0xfd8a80f4) reads `a0001faf` (meaning not decoded).

**Early-init info** (0x400; *code*: k2 2.60 at 0xf220dd5c, ep 2.44 at 0xf2210580/0xf22106f6):
- +0 magic 0xaa; +1 dimm0 SPD I2C address (0xff = the preload EEPROM); +2 u16 offset (0x0440);
  +4/+5 dimm1 (0 = none). **The 2.60 stage2 reads only these 7 bytes** (every boot) and uses them only when the
  DT has no usable `/soc/board-cfg/dram` (DT first, EEPROM second: 0xf220dbf8).
- 2.44 also reads +0x0e magic 0xcc → +0x10: four 5-byte sets [dram_odt dram_odt_dyn dram_rout phy_odt
  phy_rout] for 1/2 DIMMs × 1/2 ranks. On u01 = `04 00 07 38 22` for one DIMM (= the DT values);
  +0x2c ≠ 0xcd: no pull-up/down/vref block (2.44 uses one only with 2 DIMMs). +0x0b..0x0d =
  `bb ff 00` is read by neither parser.
- So **a CS1 (2.44) boot finds this DRAM** through the EEPROM. That settles spi-recovery.md's
  "no valid dimms" worry for the DRAM step only.

**SPD** (*proven* by stage2's log line and the valid base CRC 0x5cf9): DDR4, key byte "SO-DIMM", x8, one
rank, 64-bit + 8-bit ECC, 15 row / 10 column bits, 4 BG × 4 banks → **4 GiB**, tCKmin 833 ps (DDR4-2400
part run at 2133). Bytes 0/1 and the density nibble (byte 4 = 0x80, "256 Mb") are wrong; stage2 sizes
from the row/column/bank bits (*code* 0xf220d7c4). Part number blank.

**Who writes the EEPROM.** Stage2 does, from a valid SPI `EEPROM` object (0x700, same instance as
stage2), inside the measured-boot routine after DDR init (*code*: k2 0xf220d380 → 0xf220c2d8 → 0xf220d5e4):
- object data ≤ 0x400 B and a multiple of 32, compared with EEPROM offset 0 → "EEPROM image is the
  same. Skipping update" or a 32-byte page write + read-back verify per differing page.
  So **only the preload region (0x000–0x3ff) can be rewritten this way**, never the SPD or board data.
- Before writing it drives **GPIO 34** (bank 4 at 0xfd88b000, bit 2) low and afterwards high. The generic
  K2C-AB DT drives GPIO 34 high in U-Boot. *Inferred*: GPIO 34 = this EEPROM's write protect, active high.
  The DMC-12 DT does not touch GPIO 34.
- An invalid object is invalidated (first 4 bytes zeroed) only on the "secure" path (2.60 ep: "Skipping
  update and invalidating EEPROM (only if secure)"). Both CS0 objects are blank (ff): no update runs.

**Board-specific vs SoC-generic.** The table format, the SRAM record and the early-init pointer are SoC
(PBS hardware + stage2). The values are per SKU/board: PLLs (CPU speed bin, DDR clock), SPI clock,
the SPD (memory-down part), the SPD location. The generic K2C-AB DT names the same SPD location
(0x57/0x440), so **this EEPROM is most likely the stock K2C-AB image**, not a DMC-12 one (*inferred*).

## 2. Stage2 DDR on u01 (k2 2.60.3, main 0xf220bd88; DDR 0xf220d7c4)
1. Bootstrap parse; if I2C preload is on, read the early-init info (above).
2. TOC, DT instance n. `ddr_board_cfg_init`: DT `/soc/board-cfg/dram` (u01: `addrmap-type
   l2-bank-interleaved`, dimm0 `on-board`, SPD 0x57/0x440, impedance sets), else the EEPROM early info.
3. Read the SPD (256 B); if no address, scan I2C 0x50–0x58 ("SPD scan failed!"). Parse DDR4 (DDR3 refused).
4. Frequency: keep the preloader's NB PLL unless DT `ddr-pll-freq` differs or the SPD's tCKmin is too
   slow (`pll_fixup`; 1200 and 1333 MHz unsupported). u01: no change (no "change freq" line).
5. `al_ddr_init` (HAL controller + PHY training) with up to 1000 retries; "DRAM ch 0: 4GB".
   DT `size-limit` can cap it.
6. ECC on when the SPD reports the ECC byte lane (*code*: flag at cfg+0x118). Then stage2 zero-fills
   DRAM 0–32 MiB with CPU stores.
7. Stage3 `dram_clear_compare_ext` (RAID-DMA memset + memcmp; DT `post`, `init-pat` 0xf,
   `post-failure-halt`, `scrubber-write-init`) clears and checks the rest: 0x02000000–0xc0000000 and
   0x2_0000_0000–0x2_4000_0000 (*proven*, log).
8. **Layout (*proven*: live DT memory node):** 4 × 1 GiB at 0x0, 0x4000_0000, 0x8000_0000, 0x2_0000_0000.
   DRAM 3–4 GiB sits above 8 GiB because 0xc000_0000–0xffff_ffff is I/O (HAL `AL_DRAM_3_HIGH`).

**What varies by board:** the dram node (DIMM vs on-board, SPD address/offset, channel count, impedance,
size limit, pll freq), the SPD itself, and the preloader PLLs. The stage2 code is SoC-generic, but the
build flavour differs (k2 = TPM/measured boot, no EP; ep = PCIe endpoint early link-up).

## 3. DT objects: dt.0 (DMC-12) vs dt.1 (generic K2C-AB)
Both are plain FDTs (26460 / 26384 B) behind the al-boot header. `./alpine_dtobj.py --diff
dumps/u01/spi-cs0/dt.1.bin dumps/u01/spi-cs0/dt.0.bin`; a `--raw` diff shows **nothing differs outside
`/soc/board-cfg`, the model string and one I2C branch.** That diff is what makes the card a DMC-12:

| item | generic K2C-AB (dt.1, also CS1 2.44) | DMC-12 (dt.0) |
| --- | --- | --- |
| id / model | `alpine_v2_k2c_ab` | `DMC-12 190520a alpine_v2_k2c_ab (rc mode + sata)` |
| SerDes group 0 / 1 | `pcie_g3x8` over both groups (x8, group 1 ref "left") | g0 `pcie_g3x4`, g1 **`sata`** lane 0 |
| SerDes group 2 / 3 | `pcie_g3x4` each | `skip` |
| PCIe | `ep-ports <0>` (port 0 = endpoint to the host), `rc-manual-init`, port0 gen3 x8, ports 2/3 enabled | **no EP port** (all RC), port0 gen1 x1, ports 2/3 disabled |
| gpio_init `gpio-list` `<gpio dir val>` | 23–33, 35, 36, 38–41 inputs; **34 out = 1** | **22 out = 1** only |
| I2C | — | mux 0x70 ch4 → PCA9546 0x71: LM75 0x4a (uC), 24c02 0x51 (board EEPROM), TMP75 0x48/0x49 |

Same in both: pinctrl `if_uart_1, if_spim_a_ss_1, if_spim_a_ss_2`; SerDes group 4 (the 2 25G lanes) `skip`;
eth port1 RGMII 1G (ports 0/2, 25G with DS25 retimers, disabled); DRAM node, TPM at mux ch2 0x20, RTC 0x68,
24c64 at 0x57, leds, trace. CS1's 2.44 DT = dt.1 without the dram channel node and without 0x57/RTC,
and `crs-timeout`, plus `if_pci_ep_int_a` in pinctrl. *Code*: U-Boot parses triplets as `<gpio, dir (0 = in), value>`
(0x01107f50); k2 stage2 reads only `gpio-list-s2`, which neither DT has (0xf220c818).

## 4. APCEA, BOOT_APP and the other objects
**APCEA (0x100, CS1 only: v1.15.10, 253 KiB, AArch64, load/exec 0xb8000000).** A CVOS-style
bare-metal app that keeps a **PCIe endpoint** link to a host: strings "EndPoint", "APCEA is up!",
`ltssm %x->%x`, "restart link", "lane reset", "Compliance mode", host PERST (`h_prst_0/1`),
"Live upgrade", and it reads the TOC (`APCEA_CFG`, `EEPROM`). Built Jul 30 2018. The name is not
expanded anywhere in the binaries.
- Stage3 looks it up (n) right after DT (*code* 0x010285a8). At exec it loads it to 0xb8000000, prints
  "Executing APCEA & application...", and hands its entry to the agent in 0xfbff40fc next to the app
  entry in 0xfbff40f8. The agent stores both in CPU0's context (*code* agent 0x5f58). Which core runs
  APCEA was not traced.
- **"Early APCEA":** an instance already running when stage3 starts. Its control word is in the
  cpu-resume area: +0x14 running, +0x10 bit31 stop request / bit30 stopped. Stage3 stops it before exec
  unless `e` was pressed (*code* 0x010292f8). *Inferred*: a warm reset/live upgrade with the host link kept up.
- The Linux DT reserves 0xb0000000 and 0xb8000000 (128 MiB each) (*proven*, factory boot log).
- **On CS0 both APCEA slots are blank**, so `p`/`e` change nothing on u01. A DMC-12 is the RC, and an
  APCEA would only matter with `ep-ports`.

**APCEA_CFG (0x101, 0x7600 B, CS0 v2.7.1 = CS1 v1.15.10 except ~10 bytes).** The PCIe endpoint
personality APCEA presents (*inferred* from the layout):
- per-PF config-header templates: vendor 0x1c36 (Annapurna), device 0x1031, class 0x018002, BAR masks;
- a VPD template "Annapurna Labs Alpine AL5+" with PN/SN/EC/V0–V5 fields. Each field points into the EEPROM
  board data: 0x631/15 B = `pn`, 0x621/16 B = `sn`, 0x61a/7 B = `pcb_rev_id`, 0x602+6n = MAC n. That
  is exactly `al_eeprom_board_data`.

**BOOT_APP (0x200, A/B, CS0 v3.15.14 Apr 2019; CS1 v3.14.6).** A U-Boot legacy uImage ("test", ARM64,
OS Linux, load 0x1f0000, entry 0x200000) around a CVOS app from Jenkins job
`k2_firmware_bin_release_V2_k2c_ab_r0b`: `apps/eth_fast_path` on `apps/common/boot_app_base`.
- It brings up the 25G Ethernet ports and retimers from the DT, the PCIe EP BARs (`efp_init_pci_bars`),
  thermal and a health LED, with a coredump hook and a UART CLI (`rd`/`wr`/`rdr`/`wrr`/`vrr`, `reset`, `quit`).
  It is the K2C-AB's "Ethernet fast path" NIC firmware, not a Linux.
- Only U-Boot loads it (`bootm` from the UBOOT_SCRIPT macros, with `$fdtaddr` = the DT object as its
  board config). With either DT on this flash it would find the 25G ports disabled (group 4 `skip`) and, with dt.0, no EP. The box would sit in a CVOS
  CLI with no console-server function. The reroute hazard is in uboot-vendor-commands.md.
- BOOT_APP_CFG is blank on CS0 (instance 1 zero on CS1). The macros' `boot_app_args_spi_loc 0x480000` is stale:
  that is the UBOOT 1 slot in this layout.

**UBOOT_SCRIPT:** env migration only (known). **2ND_TOC, IPXE, BOOT_MODE, EEPROM:** blank on CS0.
**SERDES_25G_FW (0x500):** looked up by stage3, absent from the TOC.

## 5. Read-only questions for the main agent (none are needed to proceed)
At `ALPINE_DB>` (save to `inventory/u01/`; no identifiers expected):
1. `md.l 0xfd8a80f0 2` once more after a cold boot: check that `i2c_pld_status_low` = `a0001faf` is stable.
2. `gpio status` or `md.l 0xfd88b000 0x100`/`md.l 0xfd88b400 1`: the GPIO 34 direction/level at the prompt (EEPROM WP theory).
3. `i2c dev 0; i2c md 0x57 0x0.2 0x10`: the first preload bytes match `spd-0-0057.bin` (same bus/address as the dump).
4. `md.l 0xfbff4100 8`: the SRAM record (already in spi-recovery-queries; repeat only if a second unit appears).
Owner: a photo of the 0x57 EEPROM and of the GPIO 34 net (pull-up?) on the K2 card, if visible.
