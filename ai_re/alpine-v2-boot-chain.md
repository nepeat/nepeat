# Annapurna Alpine V2 "K2" boot chain: a reference for other cards

Cross-device notes for Annapurna Labs Alpine V2 (4× Cortex-A57) SoC cards in the "K2" family
(K2C-AB and relatives). Distilled from the DMC-12-48R-AS0 (u01), whose card is a K2C-AB with a
DMC-12 board config. The Alpine V3 "K2X" on the CS8210-32X uses the same framework (§9). Device
evidence and decoders live in the DMC-12 dir:
- [boot-chain-details.md](devices/dmc-12-48r-as0/boot-chain-details.md): preloader EEPROM, DDR, DT diff, APCEA/BOOT_APP;
- [spi-uboot-replacement.md](devices/dmc-12-48r-as0/spi-uboot-replacement.md): object format, stage roles, handoff ABI;
- [spi-recovery.md](devices/dmc-12-48r-as0/spi-recovery.md): A/B, OTP, CS1, write protect;
- [uart-boot.md](devices/dmc-12-48r-as0/uart-boot.md): boot ROM, straps, UART boot, stage3 keys.

**Evidence marks:** **[hw]** seen on DMC-12 hardware · **[code]** read in the disassembly of the
u01 binaries · **[inf]** inferred from code shape, data, strings or the HAL. `HAL/` = the Annapurna
HAL v2.6 from the MikroTik GPL drop, `devices/cs8210-32x/analysis/research/mikrotik-added/drivers/soc/alpine/HAL/`.

## 1. The chain at a glance
| # | stage | where it lives | runs as | does | board-specific input |
| --- | --- | --- | --- | --- | --- |
| 0 | **PBS hardware preloader** | I2C EEPROM (0x57 or 0x50 by strap), or the SPI slave | hardware, before CPU reset release | register writes: PLLs, SPI clock, AXI/QoS, SGPO, an SRAM record for stage2 [hw] | the whole EEPROM |
| 1 | **Boot ROM** v2.3, 16 KiB at 0xfbff0000 | on-die | A32, EL3 | strap → boot device; load `S2` stage2 from SPI CS0 (UART XMODEM, NAND) into SRAM 0xf2200000, check its sum (RSA when secure, [inf] from ROM strings) | straps, OTP |
| 2 | **stage2** (`stage2_loader_k2`/`_ep`) | TOC object STG2, A/B | A32/Thumb in SRAM 0xf2200000 | TOC, instance, DT, TPM, **DDR init + training**, ECC low clear, measured boot, EEPROM update, load STG3 (or PRE_BOOT_V2) to 0x01000000 [code] | DT `dram`, EEPROM early-init + SPD, `gpio-list-s2`, TPM nodes |
| 3 | **stage3** (CVOS based) | STG3 | A32 at 0x01000000 | BOOT_MODE, DT to 0x01100000, APCEA, console keys, SerDes, Ethernet board params, PCIe EP/RC split, DRAM clear/POST, trace, park secondaries, exec via agent [code] | DT `board-cfg` (serdes, ethernet, pcie, dram post) |
| 4 | **aarch64_resume_agent** | inside STG3, copied to DRAM 0x1000 | AArch64 EL3, resident | PSCI 0.1 + SiP SMCs, GIC secure init, ERET to the app at EL2 [code] | none |
| 5 | **app**: UBOOT (default), or what BOOT_MODE names (IPXE, BOOT_APP, ...) + optional **APCEA** | TOC objects | AArch64 EL2 | factory U-Boot `2015.07-alpine_db`: pinmux, gpio, SGPO, external PCIe, eth, UBOOT_SCRIPT, env → Linux or BOOT_APP | DT `pinctrl_init`, `gpio_init`, `sgpo_init`, `pcie/port/pf`, eth retimers, env |
| 6 | **BOOT_APP** (optional) | TOC object, uImage | CVOS bare metal | card firmware, e.g. K2C-AB `eth_fast_path` NIC [inf] | DT as board config |

Typical u01 timings [hw]: stage2 at 2.1 s after power-on, stage3 at 3.3 s, app load at 4.2 s.

## 2. Flash layout and objects
- **One format for every object**, byte-sum checksums only: TOC at a 128 KiB-aligned offset found by
  probing (0x80000 on the DMC-12, 0x40000 on the CS8210), 0x48-byte object headers (magic `c7 9e 0b 00`);
  stage2 additionally has the ROM's `S2` header. Field-level spec and `alboot.py list/check/wrap`
  (in `nix/packages/alboot`): [spi-uboot-replacement.md §1](devices/dmc-12-48r-as0/spi-uboot-replacement.md).
- **Object IDs** (`HAL/services/flash_contents/al_flash_contents.h`): 0 BOOT_MODE, 1 STG2, 2 DT,
  3 STG2_5, 4 STG3, 5 UBOOT, 6 UBOOT_SCRIPT, 7/8 UBOOT_ENV(_RED), 9 KERNEL, 0xa ROOT_FS, 0xb 2ND_TOC,
  0xc/0xd PRE_BOOT(_V2), 0x10 HW_PLD, 0x11/0x12 ATF_BL31/32, 0x13 UEFI, 0x100/0x101 APCEA(_CFG),
  0x110 IPXE, 0x200/0x201 BOOT_APP(_CFG), 0x210–0x241 APP_0..3(_CFG), 0x300 CRASH_DUMP, 0x400 VPD,
  0x500 SERDES_25G_FW, 0x510/0x520/0x530 SerDes AVG FW (V3), 0x600–0x602 PUK0–2, 0x700 EEPROM,
  0x701 SPIS_PLD, 0x1000–0x1fff user. The instance goes in bits 31:28.
- **A/B is per instance, not per object** [code]: the ROM-chosen stage2 instance (SRAM word
  0xfbff4000) is used for every later object. A bad object spins or falls into a legacy path; nothing
  tries the other instance. Details and the only real switch: [spi-recovery.md §2](devices/dmc-12-48r-as0/spi-recovery.md).
- **No signatures are checked by these builds** [code], even though the library has PUK/SIG code.
  Only the ROM-level RSA blob on stage2 exists, and OTP secure boot was off on u01 [hw].

## 3. Stage 0: the hardware preloader
- Straps (`boot_strap`, PBS 0xfd8a8110; `HAL/pbs/src/al_hal_bootstrap_map.h`) [code]: bits 13:12 select
  I2C (1/3) or SPI-slave (2) preload. Bit 23 picks the I2C address (V2: 1 → 0x57, 0 → 0x50). Bits 27:26
  set the I2C speed (0 = 400 kHz). Decode: `alpine_preload.py --strap 0x...`.
- **Table format** (PBS hardware; the same as the `HW_PLD` object, `al_flash_hw_pld_validate`) [hw]:
  - 12-byte entries `{u32 data, u64 addr}` (LE);
  - a header and a footer entry with data `0x1c361c36`, addr `0x1c361c361c361c36`;
  - every entry in between is a 32-bit write. u01 has 22 (decoded in boot-chain-details.md §1), and the
    live registers match.
- What a K2 table sets (u01) [hw]:
  - CPU PLL (2000 MHz bin), NB PLL + channel divider (1066 MHz DDR clock), SPI-master BAUDR (flash clock);
  - PBS AXI conf, NB QoS, SGPO, ROM NAND geometry;
  - the **SRAM dev-info record** 0xfbff4100–0x411f (preloader version string, e.g. `v2sil-39-rc1`, and the
    EEPROM offset of stage2's early-init info).
- **Security-relevant:** OTP reports "OTP shadow override by preloader: enabled" [hw], and the preloader
  writes registers before any code runs. A writable preload EEPROM is a pre-ROM code path. Treat it like
  stage2: never write it casually.
- **Who rewrites it:** stage2, from a valid SPI `EEPROM` object (0x700) [code]. It writes ≤ 1 KiB to
  offset 0, in 32-byte pages with verify, only if the content differs. It drops the write protect while
  writing (GPIO 34 on the K2C-AB [inf]). The factory U-Boot's `eepromupd*` env scripts
  (`i2c write` to `pld_i2c_addr`) are the other writer. **Keep the EEPROM objects blank** in any image you write.
- The rest of the same EEPROM (K2C-AB layout) [code/hw]: early-init info (0x400, magic 0xaa), the DDR4 SPD
  of memory-down boards (0x440), `al_eeprom_board_data` (0x600, magic 0xCC: 4–8 MACs, PCB rev, serial, PN;
  `HAL/services/eeprom/al_eeprom_data.h`).

## 4. Stage 1: boot ROM (pointer)
16 KiB at 0xfbff0000 (alias 0xfd8a0000, high vectors 0xffff0000), "Annapurna Labs' Boot ROM ver 2.3".
Strap boot device (bits 17:15): SPI mode 0/3, NAND, UART at 115200/1M/2M baud (XMODEM), or the UART CLI.
It leaves the stage2 offset/instance in SRAM 0xfbff4000. Its flow, resume path and UART boot are in
[uart-boot.md](devices/dmc-12-48r-as0/uart-boot.md), which is still being extended.

## 5. Stage 2: DRAM and the early board
Main flow (k2 2.60.3) [code]:
1. bootstrap parse and shared data (0xfbff4180);
2. if I2C preload is on, read the early-init info;
3. TOC → instance → DT n;
4. `gpio-list-s2` (stage2's own GPIO list; U-Boot reads `gpio-list`);
5. TPM probe from DT (`/soc/i2c-pld/i2c_mux`, `/soc/spi_aux`);
6. **DDR**;
7. measured boot (SHA-1 of the TOC objects into a TPM PCR) and the EEPROM-object update;
8. load PRE_BOOT_V2 or STG3, jump to 0x01000000.

**DDR parameters** [code]:
- **DT first:** `/soc/board-cfg/dram` (`ddr-pll-freq`, `size-limit`, per-channel `addrmap-type`, `crc-en`,
  `parity-en`, `dimmN {on-board, spd {i2c-addr, offset, num-bytes-addr}}`, `impedance_ctrl/<dimms>_<ranks>`).
- **Else** the EEPROM early-init info: SPD locations only in 2.60. 2.44 also takes impedance sets from it.
- SPD: DDR4 only; if no address is given, scan 0x50–0x58.
- Frequency: kept from the preloader unless DT or SPD tCKmin force a `pll_fixup` (1200/1333 MHz unsupported).
- `al_ddr_init`: HAL controller + PHY training, retried up to 1000×.

**ECC and clearing:**
- ECC is on when the SPD has the ECC lane [code]. Stage2 zero-fills 0–32 MiB [code].
- Stage3 clears the rest with the RAID DMA and compares it as a POST (DT `post`, `init-pat`,
  `post-failure-halt`, `scrubber-write-init`) [hw: log].

**Physical map, 4 GiB** [hw]:
- 0–3 GiB at 0x0;
- the 4th GiB at **0x2_0000_0000** (`HAL` `AL_DRAM_3_HIGH`; 3–4 GiB is the I/O hole);
- 4–8 GiB would be at 0x1_0000_0000 (`AL_DRAM_2_HIGH`) [inf].
- Keep 0x0 +1 MiB reserved (the resident agent).

**Build flavours** (same HAL; pick by the string `stage2_loader_*`) [code]:
- `k2` (Amazon build): TPM, measured boot, uC I2C recovery, no PCIe-EP code;
- `ep` (al-boot release): early PCIe **endpoint** link-up in stage2 ("EP ready @ %u usec!");
- `ep` 2.44: also copies the EEPROM MACs into SRAM.

## 6. Stage 3, agent and handoff
- Stage3 [code]:
  - reads BOOT_MODE (app ID; default 5 = UBOOT), DT n, APCEA n, SERDES_25G_FW n;
  - polls UART0 once for `p` (skip APCEA), `e` (keep the running early APCEA), `u` (force UBOOT)
    [hw: `p`/`e` 4/4, uart-boot.md];
  - looks up the app, then copies the DT to 0x01100000 and prints its `board-cfg/id` ("Board config ID");
  - DT init: PCIe `ep-ports`, Ethernet ports + `al_eth_board_params_store`, SerDes groups, general shared
    data, DRAM clear/POST, trace to 0x02004000;
  - powers down the secondaries;
  - loads the app (and APCEA) and resets CPU0 into the agent.
- Handoff ABI (AArch64 EL2, MMU off, x0 = 0, no DT passed; CCU/coherency undone; PSCI resident):
  [spi-uboot-replacement.md §3](devices/dmc-12-48r-as0/spi-uboot-replacement.md).
- **APCEA** (`0x100`, AArch64, load 0xb8000000 on the K2C-AB) [inf from strings + code]:
  - a resident early application that owns the **PCIe endpoint link to the host** (LTSSM, PERST, live
    upgrade) while the main OS boots;
  - stage3 passes its entry to the agent in 0xfbff40fc beside the app's (0xfbff40f8);
  - `APCEA_CFG` (0x101) holds its PF config headers and VPD template, filled from the EEPROM board data;
  - only meaningful on EP-mode cards.

## 7. The DT object: who reads what
The `DT` object is a plain FDT ("board config", not a Linux DT). U-Boot puts it in `$fdtaddr`, and
BOOT_APP/CVOS gets it as its board config. Decode/diff: `devices/dmc-12-48r-as0/alpine_dtobj.py`.

| node / property | stage2 | stage3 | U-Boot | notes |
| --- | --- | --- | --- | --- |
| `/soc/board-cfg/id` | | prints | | the board/SKU name: first thing to read |
| `board-cfg/dram/**` | **DDR** | `post`, `init-pat`, `post-failure-*`, `scrubber-write-init` | | §5 |
| `board-cfg/gpio_init` | `gpio-list-s2` | | `gpio-list` | triplets `<gpio, dir 0=in/1=out, value>` [code, both] |
| `board-cfg/serdes/groupN` | | **SerDes** `interface` (pcie_g3x4/x8, sata, eth modes, `skip`), `ref-clock`, `active-lanes`, `inv-*`, lane params | | the biggest board difference |
| `board-cfg/pcie` | | `ep-ports` (which ports are endpoints) | `portN` gen/width/status, `pfN/pci-header/barN`, `msix-cap`, `sris-params` | EP vs RC card |
| `board-cfg/ethernet/portN` | | mode, serdes grp/lane, retimer, ext_phy, i2c | serdes-lane retimers | |
| `board-cfg/pinctrl_init`, `sgpo_init` | | | MUIO pinmux (UART1, SPI CS1/CS2, NAND, ...), SGPO | |
| `/soc/i2c-pld/**`, `/soc/spi*` | TPM, uC I2C clear | I2C | GPIO expander, mux, SPI flash | |
| `/soc/trace`, `general_shared_data`, `leds`, `/hypervisor/` | | trace buffer, shared data, LEDs, secondary-core mode | | |

## 8. Board-specific vs SoC-generic
| piece | generic (same on any V2 card) | board / SKU specific |
| --- | --- | --- |
| boot ROM, PBS preloader engine, TOC/object format, agent | all | none |
| preload EEPROM | format, SRAM record, early-init pointer | PLL values (CPU bin, DDR clock), SPI clock, SGPO, SPD (memory-down), SPD location, board data (per unit) |
| stage2 | DDR/HAL code | build flavour (k2/ep), whatever DT/EEPROM tells it |
| DT object | node schema | **everything under `board-cfg`**: SerDes use, EP/RC, eth ports, DRAM, GPIO, pinmux, I2C tree |
| stage3, U-Boot | code, keys, handoff | none in code (read the DT); U-Boot env + UBOOT_SCRIPT per product |
| APCEA(_CFG), BOOT_APP | CVOS | the card's purpose (EP NIC firmware, PCI identity/VPD) |
On u01 the DT `dt.0` differs from the generic K2C-AB `dt.1` **only** under `board-cfg`, the model string
and one I2C branch [code]: the DMC-12 is a K2C-AB reconfigured from PCIe endpoint (x8 to a host) to root
complex (x4 + SATA, the console server's USB/SSD).

## 9. Identify a card variant from its dumps (read-only)
1. `alboot.py list mtdN.bin`: the TOC layout, the versions, which objects exist (APCEA? BOOT_APP? SIG/PUK
   IDs → stop, a secure-boot unit).
2. `strings` of STG2: `stage2_loader_k2|ep vX.Y` and the build path; of STG3: "Stage 3 version".
3. `alpine_dtobj.py dt.N.bin`: `board-cfg id`, model, SerDes interfaces, `ep-ports`, the dram node.
   `--diff` against `dt.1`-style generic DTs shows the board's own changes.
4. BOOT_APP: the uImage name and Jenkins job (`k2_firmware_bin_release_V2_<card>`) name the card.
5. The preload EEPROM (`i2c md` or Linux `eeprom`), then `alpine_preload.py`: CPU/DDR clocks, preloader
   version, SPD part, board-data presence. The strap (`md.l 0xfd8a8110 1`): boot device, preload source.
6. Factory U-Boot `dev_info` + `otp show` (only `show`!): secure boot, minimal SW version, debug enables.

## 10. Porting checklist for another K2 card
- [ ] Console + power scripted; full dumps of every SPI chip select, the preload EEPROM, the board EEPROM,
      the disk; hashes recorded; vendor blobs git-ignored (CLAUDE.md, switch-porting-runbook.md phase 0).
- [ ] §9 identification done; OTP secure boot off and no SIG/PUK objects (else stop and ask).
- [ ] Note the stage2 instance and A/B reality: whether the instances really differ (on u01 they did).
- [ ] Decode the DT object and diff it against a generic one; list SerDes groups, EP/RC ports, eth ports, I2C tree.
- [ ] Record the DRAM source (DT `dram` vs EEPROM early info), the SPD location, ECC, size; check the
      memory node in the live Linux DT (the 4th GiB at 0x2_0000_0000).
- [ ] Find what the factory U-Boot does beyond the DT (external PCIe link-up, pinmux, CCU writes at `bootm`,
      env scripts). This is what a replacement U-Boot must redo (DMC-12 list: spi-uboot-replacement.md §4).
- [ ] Linux: mainline `alpine` platform + an own DTS; memory, PCIe (internal + external), SATA, al_eth.
- [ ] Prefer a RAM chainload from the factory `bootm` first. In SPI use a blank app slot + BOOT_MODE,
      and keep UBOOT 0 for the `u` key escape. Never write STG2/STG3/DT/TOC/EEPROM objects or the preload EEPROM.
- [ ] Check that the card has no early APCEA it depends on (EP cards): `p` skips it, and an EP card in a host may need it.
- [ ] Add new gotchas here and in the device's PROGRESS.

## 11. Alpine V3 (CS8210 K2X) differences seen so far
- Same TOC/object format: `alboot.py list devices/cs8210-32x/images/spi-2026-11-09/mtd0-uboot.bin`
  works [code].
  - TOC at 0x40000, 9 objects, a single instance each;
  - stage2 `stage2_loader v3.65.1`, exec 0xff000020;
  - SerDes AVG firmware objects 0x510/0x520/0x530.
- The DT object decodes with `alpine_dtobj.py` (`board-cfg id` "CS8210"). It uses both `gpio-list-s2`
  (stage2) and `gpio-list` (U-Boot), and separate SPD EEPROMs (0x52/0x54/0x55, all `on-board`) on two DDR channels [code].
- V3 SRAM dev-info record is at 0xfbfe01c0 (HAL), the PBS internal memory at 0xfbfe0000. V3 adds a
  SPI-master preload mode (strap preload 0). The 0x57 EEPROM is blank on that unit (CS8210 PROGRESS).

## 12. Open
- The preload entry with address bits 39:32 set (`0xf5_7f89_6100`), and the 0x200-byte trailer after
  the preload footer: meaning unknown. The ROM disassembly (uart-boot.md) may tell whether it checks the trailer.
- Who runs APCEA (which core, x0 to the app?) and how an "early APCEA" survives into stage3.
- Whether GPIO 34 is the preload EEPROM's write protect on every K2C-AB (one stage2 code path + the generic DT).
