# CS8210 CPLD diff: system CPLD v13 -> v15, switch LED CPLD v1 -> v2

Owner goal (2026-11-15): a general diff between the CPLD images we have (sys v0x13, SWLED v1) and the ones the
board runs (sys 0x15, SWLED 2), for which no file exists. Built offline from the old `.vme` files, every vendor
tool that touches the CPLDs, and the vendor release notes. **No hardware access.** The live register dump (below)
and an optional JTAG readback are plans for the parent to run.
Tags: **[img]** decoded from a `.vme`, **[code]** vendor binary (disassembly/emulation), **[doc]** vendor
notes/configs, **[hw]** live read, **[inf]** inferred.

Tools (own code, `scripts/`): `vme_decode.py` (ispVM VME 12.x decoder: transcript, MachXO2/3 flow summary,
page dumps; file CRC re-computed and matching for all 5 vendor files), `cpld_attr_map.py` (emulates every
`sys_cpld.ko` sysfs attribute in Unicorn to get its register bits), `cpld_regdump.py` (U-Boot dump commands +
diff against the expected table), `vme_readback.py` (read-only readback VME generator + output parser).
Outputs (git-ignored, vendor-derived): `analysis/cpld-diff/` (`*.json`, `*.svf.txt`, page dumps,
`sys_cpld_attrs.json`, ONIE .015 kernel/DT/initramfs extracts, `readback-*.vme`).

## Bottom line

- **No changelog exists for sys v14/v15 or SWLED v2.** The last vendor release notes are DIAGOS 0.1.1
  (2021-04-01, SSD `/etc/releaseNotes`), whose newest sys image is v13; SWLED is v01 in every entry since 0.0.1
  (Aug 2020). ONIE .015 (Jun 2021, the one that put v15/v2 there) ships tools only. [doc]
- **The register interface did not visibly change.** Every vendor tool dated after v13 (diag 0.1.3 `cplds.yaml`,
  driver 0.0.8 `sys_cpld.ko` 1.7 = same source, srcversion `704733AA…`, U-Boot .015) uses the same sys/LED
  registers, masks and defaults as the v13-era ones. The only post-v13 change touching the sys CPLD is in the
  voltage-margin scripts (0xf8-0xff, below). [doc, code]
- **A JTAG readback of v15/v2 looks possible**: the v13/v1 images set no security bit and are programmed in
  transparent mode; the vendor player and chain mux are on the board. It is risky (sys CPLD = power/reset
  controller); plan and risks at the end. The I2C register dump is the cheap, low-risk first step.

## The old images, decoded [img]

| | sys `briggs_sysfpga_v13_0330.vme` (`684ab604…`) | SWLED `SW_LED_CPLD_TOP_v01.vme` (`091e9f5e…`) |
| --- | --- | --- |
| built | Diamond Deployment Tool 3.11, 2021-03-30 14:50:34; VME 12.1 compressed, file CRC 0x621E ok | DDT 3.10, 2020-08-21 15:49:54; CRC 0x94A2 ok |
| IDCODE check | 0x612BD043 LCMXO3LF-6900C | 0xE12BC043 LCMXO3LF-2100C; alone on its chain (no HIR/HDR) |
| mode | ISC_ENABLE_X 0x08 (transparent: user logic keeps running) | same |
| erase | 0x04 = CFG only (UFM and feature row kept) | 0x0E = CFG + UFM + feature row |
| CFG | 9212 pages x 128 bit, 4791 non-zero; page-dump sha256 `bf7dc1c8…` | 5758 pages, 2433 non-zero; `0fcc40cb…` |
| UFM | not touched | 768 pages, all zero (no user data) |
| USERCODE | 0x00000000 (loaded, programmed, verified) | 0x00000000 |
| feature row / FEABITS | not written; aborts unless FEABITS reads 0x0AC0 under mask 0xFFF2 (set at the factory) | writes feature 0x0000100000000000, FEABITS 0x0220 |
| pre-checks | status b5 = 0 before enable, b6/b5 = 0 after (password/OTP lock bits [inf]) | status b16, b6/b14/b17 = 0 |
| security | **no ISC_PROGRAM_SECURITY (0xCE/0xCF)**; the file verifies every page by read-back | none |
| end | PROGRAM_DONE, ISC_DISABLE, **no refresh**: v13 loads at power cycle or via `xfr.vme` (IDCODE + LSC_REFRESH 0x79, `dd048bb1…`) | same |

USERCODE is 0 in all five vendor files: it carries no version, so a JTAG USERCODE read will not identify v15.
For calibration: SWLED v1 and MACLED v1 (same part, same project family) differ in 2547 of 5758 pages, so a page
diff of a v15 readback against v13 will measure extent, not meaning (no MachXO3 bitstream database here).
The fan CPLD file is the odd one: offline ISC_ENABLE 0xC6 + PRELOAD + SRAM erase, i.e. the fan CPLD stops while
being programmed; sys/LED files never do that.

## Sys CPLD history (release notes, condensed) [doc]

v04 (Oct 2020) LED/cold-reset/3.3 V bleed fixes · v09 (Jan 2021) R0B clock gen, ASC2/ASC0 trims to closed loop,
JTAG glitch fix · v0A power-down order, 19-inch support (power up only with 2 PSUs) · v0C ASC trip points,
1 ms delay before hard reset · v0E 12 V/3.3 V/0.9 V thresholds, riser/MACsec I2C-mux reset on GPIO A43 ·
v0F CPU power state machine: shutdown condition removed · v10 1 ms before power cycle, ASC3_Trim1 to CLT,
PowerGood pull-downs removed · **v13**: console logger default baud 115200 -> 9600, **new bit 0xA5[7]
XP3R3V_PLL_PG**, ASC1_trim3 CLT on, I2C-FPGA 3.3 V CLT change undone, ASC2_VMON5 label fix.
Pattern: most versions change ASC10 trim/threshold settings plus power-sequence details, paired with new
`Phoenix_ASC*.hex` files; registers are rarely added (one bit in v13). v14/v15: nothing.

## Diff table

| item | v13 / v1 | v15 / v2 | conf |
| --- | --- | --- | --- |
| sys version regs | 0x00 = 0x13, 0x01[3:0] = 0 (`fw.cfg` "0x0013") | 0x00 = 0x15, 0x01[3:0] = 0 (U-Boot banner prints `((r1&0xf)<<8)\|r0` as 15) [hw] | high |
| SWLED version 0x00 | 0x01 | 0x02 [hw] | high |
| documented sys registers (names, masks, defaults) | diag `cplds.yaml` 1.0.2 | diag 1.0.3 (2021-06-03) changes only fan-CPLD Fan7; the sys table, `leds.yaml`, `logs.yaml` are byte-identical, same `cel-cpld-test` binary | medium: vendor still expected the same bits on v15 boards |
| sysfs map (`sys_cpld.ko` 1.7) | same source in driver 0.0.6 and 0.0.8 | | high |
| 0xA5[7] XP3R3V_PLL_PG | added in v13 | presumably kept [inf] | low |
| console logger baud 0x40[6] | v13 default 9600; U-Boot sets it from `baudrate` | 0x40 = 0x40 at the U-Boot prompt [hw] | high |
| ASC1 Trim3 margining (0xf8/0xfa/0xfb) | v13 made it closed-loop | margin scripts of diag 0.1.3 comment it out (0.1.0 still drove it) | medium (follows from v13) |
| margin regs 0xf8-0xff read-back | diag 0.1.0 scripts write only | diag 0.1.2+ verify each write with `i2cget`; R01 boards also drive ASC3 Trim1 via bit 4 of 0xfc-0xff | low: v15 feature or always readable; the dump settles readability |
| fault/console log engine (0x30-0x35, 0x40-0x45), timestamp 0x58-0x5b | used by U-Boot .015 (code) | U-Boot .011 (Apr 2021) has the same log/RESET_REASON strings; accesses not re-traced there | medium |
| version-gated code | none for CS8210: the only version test in `sys_cpld.ko` is reg0 == 0x0b for the CS8000 (probe); installers compare version strings only to decide reflashing | | high |
| SWLED register map | driver sweeps 0x00-0x0f; uses 0x00, 0x09[0], 0x0a[2:0]; diag 0x01 scratch 0xde | unchanged tools; MACLED (same family) stayed v1, so v2 is specific to the switch-side board [inf] | medium |
| feature row / FEABITS / UFM / security | see table above | unknown: readback settles | |
| CFG bitstream | sha `bf7dc1c8…` / `0fcc40cb…` | unknown: readback settles (extent only) | |

## Register maps (merged)

Sys CPLD @0x60 (U-Boot bus 3: 0x74 ch0 -> 0x71 ch2). Sources: D diag table (compared mask/default), K
`sys_cpld.ko` emulation (`analysis/cpld-diff/sys_cpld_attrs.json`), U vendor U-Boot .015, S scripts, N ONIE/DIAGOS
kernel. Bits active low unless noted. Full table with names: `scripts/cpld_regdump.py` (also the dump expectations).

| reg | meaning | src |
| --- | --- | --- |
| 0x00 / 0x01 | version low / [7:4] board_version (1 = R0B, SDK picks `isg_R01.yaml` at 2), [3:0] version high | K U S |
| 0x02-0x06 | resets: 0x03 b6 I2C, b5 **ASIC**, b3 fan, b2 LED CPLDs; 0x04 b5 fan2; 0x06 b2 MDIO FPGA; D ff | K D |
| 0x05 | b7 fan_max_speed (0 = max; probe sets 1), b6 system EEPROM WP (U `sys_eeprom protect`), b5 fan2_max_speed; D a0/e0 | K U D |
| 0x07 | misc interrupt (D default c8, not compared) | D |
| 0x08 / 0xa8 | PSU1-2 / PSU3-4: present b5/4, input ok b3/2, output ok b1/0; 0x08 b7 voltage fault, b6 voltage up | K |
| 0x09 | b0 1 = 19-inch (U picks DT, install.sh picks diag configs); live 0x05 | U S H |
| 0x0c-0x0e, 0x50-0x55, 0x74-0x79 | rail power-good / under-voltage (D all compared) | D |
| 0x1c, 0x1d[3:0] | watchdog, 50 ms ticks; 0x1d[5:4] trigger; 0x22 b1 enable | K |
| 0x20 | [2:0] system LED (0 off, 1 red, 2 amber, 3/4/5 blink red/amber/green, 6 green), [4:3] PWR1, [6:5] PWR2 (0 off, 1 red, 2 green, 3 amber); U writes 0x7d, probe sets [2:0]=6 | K U H |
| 0x21 | [1:0] fan LED, b4/b5/b7 PWR1/PWR2/fan LED test mode | K |
| 0x23 / 0x24 | over-temp / voltage-issue action (D 3f/00) | D |
| 0x25 / 0x26 | reset-on-reboot select (cold 0x24, warm 0x18) / **software reset: writing 0x24 or 0x18 resets now**; 0x26 b0 port LED flash; install.sh writes 0x25 = 0x02 (matches neither encoding) | K S |
| 0x30-0x35 | fault log: 0x30 b7 pause, b5 reset, b1:0 read command/busy; 0x31-0x33 address (17 bit, 128 KiB); 0x34 write data; **0x35 read data**; 32-byte records framed 0xa5..0x5a | K U |
| 0x40-0x45 | console log: 0x40 b7 pause, b6 115200, b5 reset; 0x41-0x43 address; 0x44 write; **0x45 read data** | K U |
| 0x56/0x57/0x7a/0x7b | ASC0-3 temperature interrupt (D 3f/00) | D |
| 0x58-0x5b | 32-bit timestamp, 50 ms ticks | U |
| 0x7e | JTAG chain: b2 fan1, b3 fan2, b4 "full chain"; vmetool values 0x00 sys, 0x04/0x0c fan top/bottom, 0x10 SWLED, 0x12 MDIO FPGA + MACLED, 0x20 CPU JTAG buffer off (release note 0.0.4) | K S |
| 0xa1 | b7 I2C-FPGA LM75 int, b5 I2C-FPGA present, **b3 aux SPI flash (`i2cfpga` mtd) to the SoC** (N: mtdchar open sets, close and the flash scan clear it), b2 I2C-FPGA EEPROM WP (K); updater writes 0xd7 after `flashcp`, margin scripts 0xd6; D f0/d3 | K N S D |
| 0xa4 | b6 MACsec card present, b5 MACsec LM75 int | K |
| 0xa5 | MACsec DC VRM (D 3f/3f), b7 XP3R3V_PLL_PG since v13 | D doc |
| 0xf8-0xff | ASC trim margin control (0xf9 ASC0/1 enables, 0xfc-0xff ASC2 trims, 0xf8/0xfa/0xfb ASC1 Trim3); **never write** | S |

The ONIE .015 kernel evidence for 0xa1 b3: `Image` code at 0x6b1eb8 (`orr #0x8`, open), 0x6b1cd0 and
0x6c0b70 (`and #~0x8`), both SMBus byte-data on `i2c-8` 0x60 reg 0xa1 (the .011 kernel has the same at
0x6b18f0/0x6b1708). So mainline reads the aux flash as all ones because b3 = 0 leaves it to the FPGA.

LED CPLDs (SWLED @0x64: 0x74 ch0 -> 0x71 ch5; MACLED @0x63: 0x74 ch1 -> 0x73 ch0; both "celestica,led_cpld",
handled by `sys_cpld.ko`): 0x00 version, 0x01 scratch (D de), 0x09 b0 `led_enable` (1 = CPU test colour), 0x0a[2:0]
colour (6 red, 3 green, 5 blue, 0 white, 7 off); everything else undocumented. LED CPLD reset = sys 0x03 b2.

## Plan 1: live I2C dump (vendor U-Boot prompt, read-only) — recommended

Read-safety basis: `sys_cpld.ko` refreshes its cache by reading **every sys register 0x00-0xff** (nregs 255,
`cpld_update_device`) at most every 375 jiffies whenever an attribute is used; ONIE's `install.sh` writes
attributes at every boot, so each ONIE boot of this board (11-09, 11-10, running v15) already did full sweeps.
LED CPLDs: driver sweep 0x00-0x0f. Still kept out of the bulk pass: **0x35 and 0x45** (log read-data ports; U-Boot
only reads them after arming a read through 0x30, so a bare read may advance a pointer) — not read at all.
Read last, one byte each: 0x07, 0x56, 0x57, 0x7a, 0x7b (interrupt latch candidates). Nothing is written
except the mux selects U-Boot itself uses (same form as the 2026-11-15 checks).

Stop autoboot at the vendor prompt, capture the console to `evidence/` as `.txt`, paste
(`python3 scripts/cpld_regdump.py uboot` prints the same):

```
i2c dev 3
echo === sys-cpld ===; i2c mw 0x74 0 1; i2c mw 0x71 0 4
i2c md 0x60 0x00 0x7
i2c md 0x60 0x08 0x2d
i2c md 0x60 0x36 0xf
i2c md 0x60 0x46 0x10
i2c md 0x60 0x58 0x22
i2c md 0x60 0x7c 0x84
echo === sys-cpld-late ===
i2c md 0x60 0x07 1
i2c md 0x60 0x56 1
i2c md 0x60 0x57 1
i2c md 0x60 0x7a 1
i2c md 0x60 0x7b 1
i2c mw 0x71 0 0; i2c mw 0x74 0 0
echo === swled-cpld ===; i2c mw 0x74 0 1; i2c mw 0x71 0 0x20; i2c md 0x64 0x00 0x10; i2c md 0x64 0x10 0xf0; i2c mw 0x71 0 0; i2c mw 0x74 0 0
echo === macled-cpld ===; i2c mw 0x74 0 2; i2c mw 0x73 0 1; i2c md 0x63 0x00 0x10; i2c md 0x63 0x10 0xf0; i2c mw 0x73 0 0; i2c mw 0x74 0 0
```

Expected output: `i2c md` lines `00NN: xx ...`; sys 0x00 = 15, 0x01 = 10, 0x09 = 05, 0x20 = 7d, 0x40 = 40;
SWLED 0x00 = 02, 0x01 = de; MACLED 0x00 = 01. If a 16-byte line repeats one value, the CPLD does not
auto-increment: re-read that line one byte at a time (the 2-byte banner read suggests it does).
Then `python3 scripts/cpld_regdump.py diff CAPTURE.txt` flags masked-bit differences from the v13-era
expectations, changes since the 11-15 reads, and undocumented registers with non-trivial values.

What it settles: whether v15 keeps every documented bit (compat), which undocumented offsets are implemented
(0x0f-0x1b, 0x27-0x2f, 0x5c-0x73, 0x80-0x9f, 0xa9-0xf7: candidate v14/v15 additions), 0xa5 b7, 0xa1 b3/b2 at
boot (aux flash owner), 0x7e idle value, margin regs readable, timestamp running (compare two dumps), and for
SWLED v2 whether anything appears beyond 0x0f. It cannot show logic changes behind unchanged registers.

## Plan 2: JTAG readback of the running v15 / v2 — possible, risky, optional

Path: sys CPLD 0x7e selects the chain (I2C write); TAP bit-banged by the SoC on `/dev/gpiochip3` = PL061
@0xfd88a000 lines 2 TMS, 3 TCK, 4 TDO, 5 TDI = SoC GPIO 26-29 (vendor `vme` `gpio_jtag_init`; U-Boot's gpio-list
parks them 26 high, 27/29 low, 28 input). V2 vs V3: the DMC-12 updates its CPLD with `cpldupd` on one GPIO line;
the CS8210 has a 4-wire GPIO JTAG plus an I2C chain mux in the sys CPLD.

Why it should work: v13/v1 set no security bit and verify by reading back; their status pre-checks refuse to run
on a password/OTP-locked part, and v15/v2 were presumably installed by the same flow [inf]; transparent mode (ISC_ENABLE_X) is how the vendor programs these parts "online". If v15 is secured, the
pages read as zero and nothing else happens.

Tooling: the vendor player `vme` from the ONIE .015 initramfs (`usr/bin/vme`, static aarch64, sha256
`31bfb8bd…`, libgpiod; prints `Display Data: 0x…` for reads with an all-zero mask) running our read-only file
from `vme_readback.py gen {swled,sys}` (local: `analysis/cpld-diff/readback-swled.vme` `e4affcb5…` with UFM,
`readback-sys.vme` `70d992e9…`). The generator re-decodes its output and enforces an IR allow-list
(E0 3C C0 19 74 FB E7 46 47 73 26 FF): no erase 0E, program 70/C2/E4/F8, done 5E, refresh 79, security CE/CF,
offline enable C6. Flow: IDCODE compare (the player aborts on mismatch, so a wrong chain stops before
anything else), status, USERCODE, TraceID (0x19 [inf]), ISC_ENABLE_X 0x08, FEABITS, feature row,
LSC_INIT_ADDRESS + LSC_READ_INCR_NV for every page, status, BYPASS + ISC_DISABLE (no refresh), exactly the
vendor's verify path minus program/done. `vme_readback.py parse` diffs the output against the v13/v1 page dumps.

Run it under ONIE in RAM (`run onie_bootcmd`, as on 11-10: vendor kernel, bus numbering and `vmetool_arm`
conventions), SWLED first as the rehearsal:

```
i2cset -f -y 8 0x60 0x7e 0x10; vme /tmp/readback-swled.vme > /tmp/rb-swled.txt; i2cset -f -y 8 0x60 0x7e 0x20
i2cset -f -y 8 0x60 0x7e 0x00; vme /tmp/readback-sys.vme   > /tmp/rb-sys.txt;   i2cset -f -y 8 0x60 0x7e 0x20
```

Expected: no "verification failed"; 7 header/status lines plus 6526 (SWLED) or 9212 (sys) page lines; ~10-30 s each.

Risks, stated plainly:
- **The sys CPLD sequences every rail and reset.** Anything that takes it out of user mode (offline enable 0xC6,
  refresh 0x79, an erase followed by a power cycle) powers the board down under the SoC that is driving the
  JTAG. An erased sys CPLD means a board that does not power on; recovery needs an external JTAG programmer
  on U51 (header location unknown) and we only have v13 (a downgrade). The SWLED failure case is dark port LEDs,
  recoverable with the v1 file.
- **Bit errors in IR shifts.** Bit-reversed 0x73 is 0xCE (PROGRAM_SECURITY), so a home-made bit-banger with the
  wrong bit order would lock readback; that is why the vendor player (bit order proven by every vendor update)
  is used. A single-bit flip of 0x46 gives 0xC6 (offline mode: sys CPLD logic stops, board drops). The exposure is
  the same class as a vendor update, minus all erase/program commands.
- **I2C writes around it**: the only writes are to 0x7e; a typo hitting 0x25/0x26 resets the board.
- **GPIO ownership** under NixOS: `/dev/gpiochip3` must be the 0xfd88a000 bank and lines 2-5 free
  (`gpioinfo`), otherwise `vme` drives other pins. ONIE avoids this.
- Do not use `xfr.vme`, `vmetool_arm` with any vendor image, or the SSD ONIE updater (downgrades everything).

Payoff: a backup of v15/v2 (no copy exists anywhere; the updater would overwrite them), the real feature row,
FEABITS, USERCODE and TraceID, and a page-level extent diff. Recommendation: run Plan 1 first; Plan 2 only with
the owner's explicit OK, SWLED first, sys only if SWLED behaves exactly as expected.

## Open questions

1. What v14/v15 and SWLED v2 changed in logic: no notes; needs the readback plus a MachXO3 bitstream database.
2. 0x25: `install.sh` writes 0x02 ("power cycle on reboot") but the driver encodes cold/warm as 0x24/0x18.
3. 0xa1 b2: the driver calls it I2C-FPGA EEPROM WP, the updater comment calls 0xd7 "switch spi mux back to
   fpga"; the kernels use b3 as the mux.
4. Where the 128 KiB fault/console log lives (17-bit address space; the 6900's UFM is smaller). FEABITS 0x0AC0
   has b11 set, which openFPGALoader's (unverified) naming calls master-SPI enable: an external flash on the
   CPLD is possible [inf].
5. FEABITS bit names, the 0x19 TraceID opcode and the 6900 UFM page count (2046) are from public Lattice/openFPGALoader
   knowledge, not checked against a datasheet here.
