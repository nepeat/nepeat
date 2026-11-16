# DMC-12 board CPLD: bitstream decode, netlist, behaviour

Lattice **LCMXO3LF-2100C** (IDCODE 0x612BB043; the C variant exists only in **CABGA256**, so the
package is settled by the IDCODE). Source: the u01 config-flash readback of 2026-11-16
(`research-open-items.md` §2). Everything that reproduces Edgecore's design (bitstream, decoded
config, netlist, Verilog, simulation benches) stays in the git-ignored `dumps/u01/cpld/`; this file
and the generic tools in `../../tools/machxo3/` are tracked and contain no design data beyond the
behavioural summary below.

Confidence tags: **[proven]** checked byte for byte or by simulation of the decoded netlist;
**[inferred]** follows from structure, not checked against silicon; **[open]** not known.

## 1. Exact base: decode -> re-encode == cfg.bin  [proven]

`cfg.bin` (51168 B, sha256 `ff56fa3a44ae647e…`) round-trips through every representation:

```
cfg.bin --xo3bit disasm--> listing.txt + frames.txt --xo3cfg unpack--> design.config (Trellis text)
design.config --xo3cfg pack--> frames --xo3bit asm (+ listing.txt)--> cfg-rebuilt.bin == cfg.bin
```

Result file: `dumps/u01/cpld/roundtrip-20261116.txt` (both sha256 `ff56fa3a…`). Intermediate hashes:
listing `c5a3ee7c…`, frames `28ac3e87…`, design.config `0bba806a…` (17914 lines, 389 tiles).

Image layout (`decode/listing.txt`); all fields the packer needs:

| bytes | record | meaning |
| --- | --- | --- |
| `FF FF BD B3` | preamble | sync word |
| `FF FF` | 2 dummy | not in CRC |
| `3B 000000` | LSC_RESET_CRC | CRC16 starts after this |
| `02 000000` + 8 B | LSC_WRITE_COMP_DIC | compression dictionary, pattern7..0 = `44 30 09 41 05 50 48 FF` |
| `46 000000` | LSC_INIT_ADDRESS | frame address 0 |
| `B8 E0 01A4` | LSC_PROG_INCR_CMP | 420 frames x 1272 bits, compressed, one CRC at the end (0xE0) |
| 25195 B | frame data | per frame: 8 zero pad bits + 159 B, prefix code (0 = 00, 100xxx one-hot, 101xxx dict, 11+8 literal), byte-aligned per frame; **Diamond's encoder is canonical** (always the shortest code; checked per byte), so re-compression is unique |
| `83 A7` | CRC16 | poly 0x8005, init 0, MSB-first, 16 zero bits flushed (Trellis convention) |
| 8 x `FF` | dummy | Trellis' "security/SED space" |
| `72 10 40 05 80 04 3E 00 00` | **unknown command 0x72** | not in Trellis; it *is* inside the CRC that follows (the USERCODE CRC only matches with it included), so it is a real command, kept verbatim **[open]** |
| `C2 80 0000 00000000 4C59` | ISC_PROGRAM_USERCODE | usercode 0 (matches the chip), CRC16 |
| `22 000000 72000033` | LSC_PROG_CNTRL0 | control register 0 = 0x72000033 (Trellis writes 0x40000000; bits not decoded) **[open]** |
| 4 x `FF`, `5E 000000`, 16 x `FF` | ISC_PROGRAM_DONE | then the flash tail reads 0x00 (pages 1580..3197) |

The flash copy has **no VERIFY_ID (0xE2) and no leading CNTRL0**: that is why `ecpunpack` stopped with
"bitstream before chip was identified". Trellis' own reader does support MachXO3 compression; the
tools here bypass its command parser and feed frames to Trellis' CRAM directly.

Text config: 128 bits are not in the Trellis database (`unknown:` lines, kept, so the config is
lossless): 114 repeat identically in every CIB_PIC_B*/CIB_EBR* tile (fabric defaults Diamond sets),
2 in CIB_CFG2/3 (likely EFB enables), 9 in PIC tiles (PT13/17/18 F1B32/F1B33/F0B33, PR7 F16B58, PB25,
PL14): these complete `BIDIR` on the PIOA/PIOC pads that Trellis decodes as `OUTPUT_MIPI` (see §3).

Tools (generic, `ai_re/tools/machxo3/`; run with Trellis' python, e.g.
`nix shell github:NixOS/nixpkgs/<flake.lock rev>#python3 …#trellis` and `PYTHONPATH=$TRELLIS/lib/trellis`;
the store copy `f46bk…` is built against another glibc and does not load):
`xo3bit.py` (image <-> listing + frames, `check` = in-memory round trip), `xo3cfg.py` (frames <-> Trellis
config, `check`), `xo3net.py` (config -> netlist JSON with an accounting of every arc and setting),
`xo3vlog.py` (netlist -> structural Verilog; `--sim` splits pads into `_in/_out/_oe`).

## 2. Netlist (`dumps/u01/cpld/netlist/`)

Trellis' MachXO2/3 routing graph drops edge wires and has no model of the secondary nets, so
`xo3net.py` names wires itself. Two pieces are **[inferred]** from this bitstream and cross-checked by
the fact that they leave *1 of 11427 configured arcs unexplained* (a duplicate of another arc) and
*0 undriven nets*:

- **Edge reflection:** an x2/x6 wire whose origin tile lies off the fabric (x 0..25, y 1..14) is the
  opposite-direction wire mirrored about the edge (x -> -1-x / 51-x, y -> 1-y / 29-y). 242 wire
  references use it.
- **Secondary (HFSN) nets:** VSRX0n00 = SCLKn; in column x VSTX0200 carries SCLK((x+2)%4) and VSTX0300
  SCLK(4+(x+2)%4); a tile's HSBX0000/0400 drives row track (x+2)%4 / 4+(x+2)%4 over columns x-1..x+2;
  tap HSBX0k01 reads track k. Every one of the ~230 secondary taps finds exactly one same-class driver
  under this rule, and the trunk-enable (B2T) columns equal the driver columns exactly. The VSRX index
  -> track index identity is the one untested assumption.
- Also modelled: primary clock n = one net; PIO data/tristate source (fabric JPADDO/JPADDT or IOLOGIC
  by the DATAMUX/TRIMUX enums); PIC direct input JQn/DI = JDIx (the database files the delay-select bit
  as the only option of that mux); intra-slice carry (K0.FCO -> K1.FCI on the same wire).

Ambiguities the database cannot resolve (reported, not guessed silently): IOLOGIC CLKIMUX/CLKOMUX
CLK vs INV share one bit (clock polarity of the 15 IO registers unknown, rising assumed); PIOA/C
`OUTPUT_MIPI` = OUTPUT + PULLMODE NONE bits (the unknown bit above makes them BIDIR); 200 fixed-wire
conflicts are mode-dependent IOLOGIC gearing outputs and EFB-PLL links (first one kept: non-gearing).

Semantics checks: all 1132 configured logic LUTs depend only on routed inputs and use every routed
input; CCU2 cells follow yosys' CCU2D (LUT2 = ~INIT[15:12]) with unrouted inputs = 0, which makes every
CCU2 INIT in the design a sane incrementer/adder (the TRELLIS_COMB model, LUT2 = INIT[3:0], would not).

### Resources (live = in the cone of a pad or hard block)

| resource | used (live) | of |
| --- | --- | --- |
| LUT4 (logic) | 1082 | 2112 |
| CCU2 halves (35 carry chains: 3x26, 1x18, 2x14, 21x12, 10x10, 1x8 bits) | 420 | |
| FF | 921 (all rising edge of the OSC; 397 async-reset, 145 sync-reset, 379 none) | 2112 |
| latches (LUT-gated, transparent-low, with async clear) | 8 | |
| PFUMX / L6MUX | 32 / 10 | |
| IOLOGIC | 9 tristate registers (open-drain ports), 6 input registers | |
| pads | 74 (incl. JTAG, config SPI, EFB I2C) | 206 |
| OSCH | 24.18 MHz, the only clock (primary clock 0, 1793 sinks) | |
| EFB | only the hardened I2C1 (config port, 4-0x10) and the config SPI pins; WISHBONE unconnected | |
| GSR | driven from fabric, ACTIVE_HIGH (see §4) | |
| EBR, PLL, distributed RAM, JTAG user logic, UFM | not used | |

Global settings: `SYSCONFIG.ENABLE_TRANSFR=ENABLE` (TransFR: I/O states held during a background
update, the "hitless" path), `I2C_PORT=ENABLE`, `MASTER_SPI_PORT=ENABLE`,
`SDM_PORT=PROGRAMN_DONE_INITN`. Feature row/FEABITS: see `research-open-items.md` §2.

## 3. Pins (CABGA256 balls from Trellis' iodb; IO types are 3.3 V LVTTL/LVCMOS33 [inferred from the type bits])

| ball(s) | site (Trellis R<row>C<col>) | role | evidence |
| --- | --- | --- | --- |
| **H16 / H14** | R7C25 PIOB / PIOA | **SoC I2C bus 4: SCL / SDA** of the 0x68 register slave; also driven by an internal I2C master engine (SCL via an IO tristate register, SDA via fabric) | [proven] sim: ACKs 0x68 only |
| A9 / C9 | R0C17 PIOC/D | EFB I2C1 SCL/SDA = the config port 4-0x10 (0x11/0x13 are the EFB's other function addresses **[inferred]**) | fixed EFB pins |
| E6/D7, C8/A8, T7/R8, T9/P9 | R0C12 C/D, R0C16 A/B, R15C10 A/B, R15C15 A/B | **four more I2C master ports**, open-drain via IO tristate registers (output data tied low) | [inferred] |
| D6 | R0C11 PIOA | master enable/reset input: low forces ~20 outputs high and blocks the slave; high + "not ready" asserts GSR | [proven] in sim |
| A6, A7, B8 | R0C11 D, R0C15 C/D | JTAG TDI/TCK/TMS (TDO not in the used-pad list) | dedicated |
| P6, T6, P13, R12 | R15C7 A/B, R15C24 B/A | config SPI MCLK/SO/SI/SN (MASTER_SPI_PORT kept) | dedicated |
| C4, A4, A5 | R0C8/9/10 PIOA | fan PWM (one signal, three pins) | [proven] sim |
| A3, B4 | R0C10 PIOC/D | force fan full speed when low (fan fault / presence [inferred]) | logic |
| E8 | R0C15 PIOB | = reg 0x13 bit 0 | [proven] sim |
| P16, J13, H11, K11 | R13C25 D, R9C25 D/C, R12C25 C | = reg 0x14 bits (high at reset) | [proven] sim |
| G12, M16 | R3C25 D, R12C25 B | status LED (reg 0x17) | [proven] sim |
| other outputs (E14, E16, F14..H15, J15, K16, K14, L15, L13, E7, D9, B9, A10, F8) | R2..R12 C25, R0 C11/17/18 | ~17 of them forced high while D6 is low; functions [open] | logic |
| ~25 inputs (D10, E10, A11, C11, F10, D11, G11, J12, J16, J14, J11, L12, P15, R16, P5, R9, T10, B5, C5, B6, A3, B4, C7, F7, …) | | presence/alarm/tach/strap inputs **[open]** per pin; 16 of them go through 12-bit debounce counters on a 1 kHz tick | |

Full per-pad table (site, ball, bank, IO type, pull, driver net): `dumps/u01/cpld/netlist/pads.txt`.

## 4. Behaviour found so far

All "sim" statements: iverilog simulation of `netlist/sim.v` (the decoded netlist), OSC 24.18 MHz,
every pad a wired-AND with a pull-up, D6 = 1, benches in `dumps/u01/cpld/sim/` (`regs-out.txt`,
`wr-out.txt`).

- **Register slave at 0x68** on H16 (SCL) / H14 (SDA) [proven, sim]: `S addr+W reg [data…] P` and
  `S addr+W reg Sr addr+R data… P`; the pointer is a 10-bit counter (R9C17..R9C18) that **auto-increments**
  after each byte. Unimplemented addresses read **0xCC**. No other 7-bit address ACKs on that port
  (scan 0x08..0x77), so the PCA954x at 4-0x71 is a separate chip, not the CPLD [proven for this port].
  The slave needs SDA hold time after SCL falls (a simultaneous edge is taken as START/STOP) — normal
  I2C timing, but see bug 1.
- **Fan PWM** [proven, sim]: one PWM drives three pins C4/A4/A5 (R0C8/9/10 PIOA), 65 steps of 16 OSC
  cycles at 0x51 = 3: **23.25 kHz**, high time = (0x50 + 2) x 16 cycles (0x50 = 9 -> 11/65 = 16.9 %). Forced
  high (full speed) by D6 low, by input A3 or B4 low (R0C10 PIOC/D: fan-fail or "force full" inputs
  [inferred]) and by a sticky FF that is set only while the WDT flag 0x2a reads 3 and cleared otherwise
  (the documented "3 after a WDT reset, full speed until cleared"). In an idle simulation with no I2C
  traffic the PWM stays low; it starts with the first I2C access [proven, sim; meaning open].
  The PWM block's 8 FFs are clocked by a LUT/mux-selected divided clock (0x51 selects it): 0x51 = 3 ->
  1040-cycle period, 0x0a -> 4160 cycles.
- **Five I2C master engines** with their own prescaler ticks (~189 kHz measured) and open-drain IO
  registers (data tied low, enable registered): E6/D7, C8/A8, T7/R8, T9/P9 and one on the SoC's own bus 4
  (H16/H14, multi-master) [inferred]. What they talk to (fan controller, PSUs, sensors?) is **[open]**;
  their state depends on the bus-4 slave several FFs deep (host-commanded, not free-running).
- **Input conditioning:** 16 x 12-bit debounce/filter counters on a common ~1 kHz tick; 3 x 26-bit
  counters (tach measurement or timeouts [inferred]), 2 x 14-bit and one 18-bit prescaler.
- **Reset:** GSR = D6 & !ready (ready = sync(G11) & state incl. latch-fed logic); in sim a one-cycle pulse
  at start-up. D6 low also forces ~20 outputs high (`out = ~D6 | …`) and holds the slave in reset.

## 5. Register map (sim reset values; "live" = u01 snapshot 2026-11-15)

Write behaviour from writing 0x5A, 0xA5 and the old value back to each address [proven, sim]. Two reads
of all 256 addresses gave identical values (static inputs, so this does **not** exclude clear-on-read
latches that need an input event).

| reg | sim reset | live | access (sim) | meaning (vendor-tooling.md + sim) |
| --- | --- | --- | --- | --- |
| 0x00 | 0x07 | 0x05 | RO | board/strap inputs [inferred] |
| 0x01 | 0x88 | 0x88 | RO | version (constant) |
| 0x10 | 0x00 | 0x3f | RO | input status [open] |
| 0x11 | 0x3f | 0x17 | RO | power: b3 PSU1, b4 PSU2, b5 DC (inputs) |
| 0x12 | 0x07 | 0x07 | RO | [open] |
| 0x13 | 0x00 | 0x00 | RW b0 | drives E8 (R0C15 PIOB) directly [proven] |
| 0x14 | 0x1f | 0x1f | RW b5:0 | output enables/resets: bits 0/2 -> P16, J13; bits {1,3,4} include H11, K11 (all high at reset; active-low resets [inferred]) |
| 0x15 | 0xff | 0xff | RW 8 bit | no pad change seen [open] |
| 0x16 | 0x03 | 0x03 | RW b1:0 | no pad change seen [open] |
| 0x17 | 0x10 | 0x20 | RW 8 bit | status LED; drives G12 and M16 [proven]; blink not checked |
| 0x18 | 0x00 | 0x12 | RO | uplink PHY pair state (inputs) |
| 0x20 | 0xff | 0xbf | RO | interrupt status, active low |
| 0x21 | 0x00 | 0x00 | RW 8 bit | interrupt mask |
| 0x28 | 0x00 | 0x00 | RW b1:0 | board reset trigger (with 0x29 = 0xff no pad moved) |
| 0x29 | 0xff | 0xff | RW 8 bit | reset arm/key (write 1 first, per the reboot driver) |
| 0x2a | 0x01 | 0x01 | b0 RW, b1 set by hardware only | WDT flag (writing 1 to b1 does not stick) |
| 0x50 | 0x09 | 0x2a | RW b5:0 | fan PWM step (high time = (step+2)/65) |
| 0x51 | 0x03 | – | RW b3:0 | **new**: fan PWM clock select (period 1040 cycles at 3, 4160 at 0x0a) |
| 0x60–0x62 | 0xff | 0x57 | RO | fan1–3 tach (0xff with no pulses in sim) |
| 0x80 | 0xff | – | RO (writes ignored) | **new**, meaning [open] |
| all others | 0xcc | 0xcc | – | unimplemented (0x30–0x4f, 0x52–0x5f, 0x63–0x7f, 0x81–0xff included) |

## 6. Bug candidates

| # | finding | evidence | confidence | fix | needs CPLD reprogram |
| --- | --- | --- | --- | --- | --- |
| 1 | **SCL (H16) is used unsynchronised**: the raw pad feeds the next-state logic of 59 FFs of the slave and master (only one FF copy is a synchroniser) | static cone count, `dumps/u01/cpld/sim` | high (structure), medium (impact) | 2-FF synchronise SCL once and derive everything from the synced copy (SDA already goes through an IO input register) | yes |
| 2 | **D6 (enable/reset) is used unsynchronised in 284 FFs' inputs and in the GSR**: reset release is asynchronous to the 24 MHz clock across the whole design (recovery/removal race, partial-reset states) | static cone count | high (structure), medium (impact) | synchronise D6 (async assert, sync de-assert) before GSR and logic | yes |
| 3 | P5, D10, E10, A11 also reach 6..13 FFs each without a synchroniser | static | high/medium | synchronise | yes |
| 4 | **8 latches with LUT-generated gates** (transparent while the gate is low) in the reset/"ready" logic: classic unintended `always @*` latches; LUT gates can glitch | netlist `REGMODE=LATCH`, gates are LUT outputs | high (exists), medium (impact) | rewrite as clocked FFs | yes |
| 5 | **The fan PWM is clocked by a LUT-built clock mux** (8 FFs on R3C23 OFX, select = 0x51): switching 0x51 produced a 27784-cycle runt/stretched period in sim; mux glitches can also corrupt the PWM counter | netlist (only non-OSC clock), sim | high (exists), low (impact: 0x51 is rarely written) | one OSC clock + clock-enable prescaler | yes |
| 6 | **Duty changes apply mid-period**: writing 0x50 gave 400- and 640-cycle periods (short pulses) before settling | sim | high | load the duty at the period boundary | yes |
| 7 | Fan PWM stays low until the first I2C access after configuration (idle sim, no traffic): if a low PWM means "slow", fans stop when no host has talked to the CPLD yet | sim | medium (needs a hardware check of the PWM polarity / fan behaviour at power-on; owner remembers full speed with no OS, which points to the WDT-flag path) | — | — |
| 8 | Register reads auto-increment the pointer; read side effects (clear-on-read) **not yet established** | sim | open | — | — |

## 7. Safety of extra hardware reads

No register beyond those in `vendor-tooling.md` / `inventory/u01/cpld-regs.txt` may be read on u01 until
the read path's side effects are decoded (§8). Reading the 0x68 slave is a write of the pointer plus a
read; the pointer auto-increments, which the factory tools already live with.

## 8. Open / next

- Bit-level register sources: freeze the pointer FFs to constants and simplify the read mux (yosys),
  or simulate per-input toggles (debounce times are ms..s at a 1 kHz tick: force the counters).
- Read side effects (interrupt status 0x20, WDT 0x2a): find FFs whose CE/LSR depends on the read strobe.
- The four downstream I2C ports: run their masters in simulation and log the transactions.
- Command 0x72 and CNTRL0 = 0x72000033 in the image; the 114 repeated unknown CIB bits.
- Map every output pin to a function: single-bit writes for 0x14/0x15/0x16/0x17, 0x29 = 1 then 0x28
  (find the board-reset pin), interrupt output to SoC GPIO 41, tach inputs (drive pulses into candidate
  inputs and watch 0x60–0x62), what 0x80 reports.
- Why the PWM only starts after I2C traffic (bug 7): trace the start condition.
