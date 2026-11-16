# DMC-12 board CPLD: bitstream decode, netlist, readable RTL, behaviour

Lattice **LCMXO3LF-2100C** (IDCODE 0x612BB043; the C variant exists only in **CABGA256**, so the
package is settled by the IDCODE). Source: the u01 config-flash readback of 2026-11-16
(`research-open-items.md` §2). Everything that reproduces Edgecore's design (bitstream, configs,
netlists, RTL, benches) stays in the git-ignored `dumps/u01/cpld/`; this file and the generic tools in
`../../tools/machxo3/` are tracked and contain no design data beyond the behavioural summary below.

Confidence tags: **[proven]** checked byte for byte, by formal equivalence, by exact BDD evaluation of the
decoded logic, or by simulation of it (said which); **[inferred]** follows from structure, not checked
against silicon; **[open]** not known.

## 0. The ladder (each layer rebuilds `cfg.bin` before the next one starts)

| layer | representation | check | result |
| --- | --- | --- | --- |
| L1 packets, L2 frames | `decode/listing.txt`, `frames.txt` (`xo3bit.py`) | image -> L1/L2 -> image | BYTE-IDENTICAL |
| L3 Trellis config | `decode/design.config` + 128 `unknown:` bits (`xo3cfg.py`) | frames -> config -> frames | IDENTICAL |
| L4 lossless netlist | `l4/l4/netlist-l4.json` (`xo3l4.py`, built on `xo3net.py`) | `tools/machxo3/roundtrip4.sh` | config text IDENTICAL, image BYTE-IDENTICAL |
| L5 blocks | `l5/blocks.json`: every one of the 4944 cells -> a block path | `roundtrip5.sh` (split into 248 files, merge) | merged == L4, image BYTE-IDENTICAL |
| L6 templates | `l6/`: 5 templates, 32 instances, deviations, remainder (`xo3l6.py`) | `roundtrip6.sh` | L4 IDENTICAL, image BYTE-IDENTICAL |
| L7 readable RTL | `rtl/out/*.v`: 18 modules, named signals (`xo3lift.py` + `rtl/src/names.py`) | yosys formal equivalence vs the L4 netlist's simulation view | **1042/1042 proven**, not lossless (a view) |

Every image check: sha256 `ff56fa3a44ae647e…` both sides (`l4/roundtrip4.txt`, `l5/roundtrip5.txt`,
`l6/roundtrip6.txt`); L7: `rtl/equiv-20261116.txt`. Commands: `dumps/u01/cpld/build-20261116.txt`.

### L1-L3 (image layout)

`cfg.bin` (51168 B) = `FF FF BD B3` preamble, 2 dummy, `3B` LSC_RESET_CRC, `02` compression dictionary
(`44 30 09 41 05 50 48 FF`), `46` INIT_ADDRESS, `B8 E0 01A4` 420 compressed frames x 1272 bits (prefix code
0 / 100xxx / 101xxx / 11+8; **Diamond's encoder is canonical**, so re-compression is unique), CRC16 `83 A7`
(poly 0x8005, init 0, MSB first, 16 zero bits flushed), 8 x `FF`, then:

- `72 10 40 05 80 04 3E 00 00`: **EFB register preload** [inferred, one external RE source]: opcode 0x72,
  flags 0x10 (no own CRC; counted by the next record's CRC, as observed), select 0x40 = primary I2C block,
  length 5, payload = I2C_1_CR 0x80 (enabled), CMDR 0x04 [open], BR0/BR1 0x3E/0x00 (prescale 62), TXDR 0.
  Not in Lattice's public command table (FPGA-TN-02055 Table 8.3), prjtrellis or openFPGALoader;
  documented by the third-party github.com/awtoau/pluribus (`docs/cmd-0x72.md`) from single-attribute
  Diamond builds. Not an SED CRC (that is opcode 0xA2 in every family that has one).
- `C2 80 …` USERCODE 0 with CRC16 `4C59`; `22 000000 72000033` CNTRL0; `5E` PROGRAM_DONE; tail reads 0x00.
- **CNTRL0 = 0x72000033** (FPGA-TN-02055 rev 3.5 Table 8.5) [inferred from the table]: [31:30]=01 config
  clock = OSC/4 (Trellis' default); bit 29 Wake Up (GOE/GSR held during reconfiguration), bit 28 NDR =
  TransFR (I/O frozen), bit 25 TranEdit (transparent SRAM programming): together `ENABLE_TRANSFR`; bits
  27/26 clear (no background-reconfig bulk-erase override); [5:0] = 0x33 master-SPI clock code (MCCLK_FREQ,
  code table not public) [open].

The flash copy has no VERIFY_ID and no leading CNTRL0 (why `ecpunpack` refused it). 128 config bits are
not in the Trellis database: 117 repeat in every CIB_PIC_B*/CIB_EBR* tile (fabric defaults), 2 in
CIB_CFG2/3, 3 in PL14, 1 ambiguous (PB25 F5B38: DIFFRESISTOR of R15C24 PIOA or BASE_TYPE of PIOB), 5 are
the BIDIR option bits of the open-drain I2C pads E6, C8, A9, C9, H14.

### L4: lossless netlist [proven]

`xo3l4.py build` = the `xo3net.py` netlist (same cells, nets, names) plus ownership of every config line:
`arc:` -> the net on whose driver path it lies (stubs and clock enables -> the net feeding them; Diamond's
branch enables of the unused primary clocks 1-3 -> driverless nets `undriven:PCLK1..3`); `enum:/word:` ->
the cell (slice-shared settings on K0/FF0); SYSCONFIG -> tile pseudo-cells; `unknown:` -> the pad whose
BIDIR option uses the bit, else `unassigned` with a reason. Partition of 17131 lines (389 tiles): nets 11427,
cells 5577, tile 4, unassigned 123 (all unknown bits); 0 duplicates/missing/extra; `tocfg` text identical
(sha256 `0bba806a…`). Negative test: dropping one line fails `check`.

Netlist modelling that is inferred, not given by Trellis (it leaves 1 of 11427 arcs unexplained, a
duplicate, and 0 undriven nets): edge-wire reflection for x2/x6 wires whose origin lies off the fabric;
secondary nets VSRX0n00 = SCLKn with the column rule for VSTX/HSBX taps (the VSRX index -> track identity is
the one untested assumption); IOLOGIC clock polarity (rising assumed: CLK vs INV share one database bit);
CCU2 = yosys CCU2D semantics (LUT2 = ~INIT[15:12]; the only reading that makes every carry chain a sane
adder or comparator).

### L5: blocks [proven for the rebuild, inferred for the meaning]

`rtl/src/l5blocks.py` gives every L4 cell a path: the lifted RTL's module for every live register, LUT and
carry half (2122 cells), `io/<ball>` for pads and IOLOGIC, `hard/*` for EFB/OSCH/GSR etc., `dead/empty` or
`dead/configured` for cells outside the live cone. Seeds and evidence: `rtl/src/blocks.py` (L6 instances,
the register-file decode, the SCL/SDA cone, named anchors), the rest by majority of register neighbours.

### L6: templates [proven]

`xo3l6.py`: 1021 cells re-rolled, 1807 of 5581 cell config lines regenerated from templates (32 %):

| template | elements | instances | what it is |
| --- | --- | --- | --- |
| `iol2a` | 90 | 4 | I2C bus-recovery engine on E6/D7, C8/A8, T7/R8, T9/P9 (identical) |
| `iol0a` (variant `--iol-merge`) | 67 core | 5 | the same engine incl. the bus-4 one (SDA from fabric) |
| `chain10a` | 18 | 5 | per-engine stuck-time counter (one per engine) |
| `chain10b` | 24 | 3 | fan tach edge counter (B5, C5, B6) |
| `chain12a` | 20 | 17 | 12-bit timers: ~10 ms pulse timers of 0x15/0x16/0x2a, filters |
| `chain26a` | 53 | 3 | 25-bit tach gate/measurement chains |

25 deviations kept raw (21 FF reset-polarity variants among the 12-bit timers, 2 pad DIFFRESISTOR,
2 unresolvable). Negative test: one INIT bit flipped in a template changes 17 cells and fails `same`.

### L7: readable RTL [proven equivalent]

`rtl/lift.sh`: `names.py` (names, blocks, comments with evidence) + `tools/machxo3/xo3lift.py` ->
`rtl/out/<block>.v` (one module per block, ports derived from use) + `top.v`. Each register's async reset,
sync reset, enable and data are minimised sums of products over named signals (<= 8 leaves), otherwise a
tree of `w_<lut>` wires that follows the netlist's LUTs; plain incrementer chains become `vec + 1`; every
register keeps its netlist cell in a comment. About 160 signals carry functional names, 330 more are named by
their L6 template element (`rec0_e14` … `rec4_e14` = the same element in the five recovery engines;
`tmr<k>_e*`, `tach1cnt_e*`, …); 452 of 944 registers still read `r_<cell>` inside their named module.

**Equivalence method** (`tools/machxo3/xo3equiv.py`, yosys 0.68): gold = the decoded netlist
(`netlist/sim.v`, the simulation view of the L4 netlist) with every register renamed to its RTL name; gate
= the RTL flattened + `splitnets`; `equiv_make` pairs all same-named wires (ports + 944 registers + 8 latch
hold states), `equiv_simple -seq 1`, `equiv_induct`, `equiv_status -assert`. Register correspondence makes
it an induction over the whole state: **1042 of 1042 pairs proven** (every pad output, every register's
next state). Async resets: `async2sync` on both sides alike. The 8 latches get the same cycle model on
both sides (`q = r ? v : en ? d : q_hold`), so their reset/enable/data functions are proven, their timing is
the same primitive. Negative test: removing one literal from `stop_det`'s enable gives 1 unproven pair.

## 1. Architecture (what the 921 FFs do)

24.18 MHz OSCH is the only clock (plus the PWM's LUT-muxed divided clock). Blocks (`rtl/out/`):

- **i2c_slave** (bus 4, H16/H14, address 0x68): SCL/SDA sampled once and filtered (`scl_f`/`sda_f` follow the
  sample only when the raw pad agrees), START/STOP detectors, a **one-hot bit sequencer**: 10 address states
  that advance only while each received bit matches 0x68 (so the address compare is the sequencer itself),
  8 pointer-byte states, 8 data-byte states (`byte_done` set with the 8th bit of a data byte only), ACK
  state; shared 8-bit shift register; `ptr` loads from the first data byte of a write and **increments
  after every data byte (read or write)**; `slave_timeout` (14-bit counter) resets the slave on a hung
  bus. [proven, sim `sim/st-out.txt` + logic]
- **regfile**: write strobe = `byte_done & ack_drive & ~rw_bit & ptr == addr` (the ACK clock of a data
  byte); read data = mux(ptr) registered every cycle into `rdata`, loaded into the shift register for the
  next byte. [proven]
- **timebase**: prescalers giving the ~103 ms recovery trigger, the **1.034 s** watchdog/recovery tick and
  the blink and filter ticks. [proven sim, periods measured]
- **wdt / reset**: GSR = `D6 & (~g11_ok | ~psu_ok | ~p5_ok | wdt_reset)`; D6 low also forces most outputs
  high. The 0x29 countdown is 8 Diamond "dual-async" flip-flops (two FFs + a latch + mux each: a FF with both
  async set and async clear, which the fabric lacks; these are the 8 latches). A power-up sequencer
  (`pwr_seq_en` -> 3 stages, read back in 0x10) and two delays (`out_en_a` after G11, `out_en_b` after
  `sys_ready`) release the output groups ~1.03-1.24 s after configuration. [proven sim; meanings inferred]
- **pwm**: one 65-step PWM on C4/A4/A5, clock from 0x51 via a LUT clock mux, high time (0x50+2)/65.
- **tach**: edges on B5/C5/B6 (IO input registers) counted in a ~0.309 s window, latched into 0x60-0x62.
- **i2c_master0..4**: bus-recovery engines (section 4).
- **serial_p5**: a receiver on P5 (own bit-timing counters, 60-bit shift register, frame check `p5_ok`
  that gates GSR; status in 0x80). Source of P5 on the board and the frame format **[open]**.
- **led**, **psu_irq**, **timers** (17 x 12-bit), **misc** (16 regs not yet attributed).

## 2. Register map (bit level)

Bit numbers b7..b0. "pin" = raw pad, read unsynchronised. Reset = value after configuration (sim).
GSR = cleared/set by the CPLD's internal reset (WDT expiry, D6, …); "keeps" = not on GSR.

| reg | bits | reset | access | semantics [proven unless tagged] |
| --- | --- | --- | --- | --- |
| 0x00 | b2 P15, b1 J14, b0 J16; b7..b3 = 0 | pins | RO | board strap pins (u01 0x05: J14 low) [meaning inferred] |
| 0x01 | 0x88 | const | RO | version |
| 0x10 | b6 K14, b5/b4 J15/K16, b3 F8, b2..b0 D9/B9/A10 output states; b7 = 0 | 0 -> 0x3f | RO | power-sequencer stages (stage 1 b2..b0, 2 b3, 3 b5/b4; b6 = stage 3 & PSU state) [meaning inferred] |
| 0x11 | b5 A11, b4 D10, b3 E10, b2 C11, b1 F10, b0 D11 | pins | RO | b5 DC in, b4 PSU2 ok, b3 PSU1 ok (vendor); b2..b0 [open] |
| 0x12 | b2 D6, b1 C7, b0 F7 | pins | RO | b2 = the enable input itself; C7/F7 [open] |
| 0x13 | b0 | 0, GSR | RW | drives E8 directly [function open] |
| 0x14 | b5..b0 | 0x1f, GSR | RW | uplink PHY control: b5 = 0 hardware mode (K11, L13, P16, H11, J13 follow the PHY inputs R16, J11, L12, J12), b5 = 1 software mode: K11 = ~b4, L13 = ~b3, H11 = b2, P16 = b1, J13 = ~b0 |
| 0x15 | b0..b7 -> E16 F15 F14 F16 G15 G14 G16 H15 | 0xff | RW | **self-restoring pulse outputs**: writing 0 to a bit drives its pin low; a 12-bit timer sets the bit back to 1 after **~10 ms** (sim: 9.9 ms). Pin = ~D6 \| (out_en_a & out_en_b & bit) (low until ~1.24 s after configuration). Probably reset lines of downstream devices [inferred] |
| 0x16 | b0 -> E14 (same mechanism); b1 | 0x03 | RW | b1 = self-restoring too (~10 ms); while b1 = 0: 0x28/0x29 are cleared/reset to 0x00/0xff, the countdown is held loaded and `wdt_fired` is cleared |
| 0x17 | b1:0 M16, b3:2 G12, b5:4 L15, b7:6 no pin | 0x10, GSR | RW | LED fields: 00 off, 01 on, 10 blink A, 11 blink B (pins active low); L15 also off until `sys_ready`. Vendor: 0x01 yellow, 0x04 red, 0x10 green, 0x20 green blink |
| 0x18 | b4 ~R9, b3 ~T10, b2 ~R16, b1 ~L12, b0 ~J11 | pins | RO | uplink PHY pair state (inverted pins) |
| 0x20 | b7 D10, b6 E10, b5 `uplink_evt`, b4/b3/b2 tach3/2/1 != 0, b1 B4, b0 A3; each OR the 0x21 bit | – | RO | active-low status; **E7 = AND of the 8 bits** (BDD-proven) = the interrupt line, low = interrupt. Level, not latched (except b5's source) |
| 0x21 | b7..b0 | 0, GSR | RW | mask, 1 = masked (a masked bit reads 1) |
| 0x28 | b0 enable, b1 [open] | 0, GSR | RW | watchdog control: b0 = 1 starts the 0x29 countdown (also cleared while 0x16.b1 = 0) |
| 0x29 | b7..b0 | 0xff, GSR | RW | write = load; read = current count (also reloaded while 0x2a.b0 = 0 or 0x16.b1 = 0, so writing 0x2a = 0 kicks it); counts down every **1.034 s** while 0x28.b0 = 1; at 0 -> internal reset (1-cycle GSR: all outputs to their reset state and re-sequenced; `sys_ready` again ~1.03 s later), count reloads 0xff. Vendor reboot = 0x29<-1, 0x28<-1 (reset within ~1 s). Reads do not kick it. [proven sim `sim/wdt2-out.txt`] |
| 0x2a | b0 self-restoring (~10 ms), b1 `wdt_fired` | 0x01, keeps | b0 RW, b1 RO | b1 sets on a watchdog reset, is async-cleared while b0 = 0 (write 0x2a = 0) or 0x16.b1 = 0; **b1 = 1 forces the fans full**. After a WDT reset 0x2a = 0x03 (sim, and on u01) |
| 0x50 | b5..b0 | 0x09, GSR | RW | fan step; PWM high time = (step+2)/65 |
| 0x51 | b3..b0 | 0x03, GSR | RW | PWM clock select (3 -> 23.25 kHz) |
| 0x60-0x62 | 8 bit | 0xff, then measured | RO | fan 1/2/3: rising tach edges per ~0.309 s (2-pulse fan: value ~ rpm/97, "x100 rpm"); no pulses -> 0x00 within 0.8 s and 0x20 = 0xe3 (sim: 240/120/480 Hz -> 0x4a/0x25/0x95, `sim/tach-out.txt`) |
| 0x80 | b7 P5 (IO register), b6..b0 P5-receiver status | – | RO | [open] |
| others | 0xcc | | RO | unimplemented (0x02-0x0f, 0x19-0x1f, 0x22-0x27, 0x2b-0x4f, 0x52-0x5f, 0x63-0x7f, 0x81-0xff) |

**Read side effects: none except the pointer.** [proven, BDD] Every FF outside the slave whose next state
depends on slave state is one of: the 8 `rdata` registers (pure function of `ptr`, refreshed every cycle),
a register file bit written under `~rw_bit`, or the bus-4 recovery engine's bus monitor. With `rw_bit = 1`
(a read), the enable, sync and async reset of every one of those registers is independent of all slave
state (exact BDD cofactor over the decoded logic, `ana/q10.py`). So `S 68W reg Sr 68R data… P` changes
only the slave's own state and the pointer (pointer = reg + bytes read). The pointer byte never sets
`byte_done`, so it never strobes a register. 0x20 and 0x2a have no clear-on-read.

## 3. Pins (every used ball)

| ball(s) | function | evidence |
| --- | --- | --- |
| H16 / H14 | bus 4 SCL / SDA: slave 0x68 + recovery engine 4 | [proven] |
| E6/D7, C8/A8, T7/R8, T9/P9 | SCL/SDA of four I2C buses watched by recovery engines 0-3 (which buses [open]) | [proven] sim |
| A9 / C9 | EFB I2C1 = config port 4-0x10 (0x11/0x13: the EFB's other addresses [inferred]) | fixed pins |
| D6 | core enable: low = GSR + most outputs forced high + slave held | [proven] logic |
| G11 | power-good-type input, 11-bit filtered; low = GSR | [proven] logic, meaning [inferred] |
| A11, D10, E10 | DC in, PSU2 ok, PSU1 ok (0x11, 0x20, PSU state -> GSR, K14) | vendor + logic |
| C11, F10, D11, C7, F7 | 0x11 b2..b0, 0x12 b1..b0 (C11/F10/D11 also feed a timer) | [open] |
| A3, B4 | TMP75 ALERT 1/2, active low: 0x20 b0/b1 **and force the fans full** | logic + vendor |
| B5, C5, B6 | fan 1/2/3 tach | [proven] sim |
| C4, A4, A5 | fan PWM (one signal) = ~D6 \| ~A3 \| ~B4 \| pwm_hi \| wdt_fired | [proven] |
| E7 | interrupt output = AND(0x20), push-pull; the SoC line of `interruptmon` (GPIO 41) [inferred] | [proven] logic |
| E8 | = 0x13 b0 | [proven] |
| D9 B9 A10, F8, J15 K16, K14 | power-sequence outputs, read back in 0x10 (forced high while D6 low) | [proven] logic, meaning [inferred] |
| E16 F15 F14 F16 G15 G14 G16 H15, E14 | 0x15 b0..b7, 0x16 b0 pulse outputs | [proven] |
| H13 | `sys_ready & core_ok`: high ~1.03 s after configuration / reset | [proven] sim, meaning [inferred: a reset release] |
| K11 L13 P16 H11 J13 | uplink PHY control (0x14) | [proven] |
| R9 T10 R16 L12 J11, J12 | uplink PHY status inputs (0x18; J12 only into the hardware-mode outputs) | [proven] |
| M16, G12, L15 | LEDs (0x17) | [proven] |
| P15, J14, J16 | straps (0x00) | [proven] |
| P5 | serial input to the P5 receiver | [proven] structure, [open] meaning |
| A6/A7/B8, P6/T6/P13/R12/R5 | JTAG TDI/TCK/TMS; config SPI MCLK/SO/SI/SN/CSSPIN | dedicated |

Twelve more balls (B7 C15 C16 F12 F13 N15 P4 R3 T2 T3 T4 T5) are routed to a constant 0 with no IO type:
unused. Full table: `dumps/u01/cpld/netlist/pads.txt`; per-pin cones: `dumps/u01/cpld/ana/pins.txt`.

## 4. The five I2C "masters" are bus-recovery engines [proven, sim]

None of them starts a transaction on its own (3 s and 20 s idle runs: no pad activity; `sim/long*.txt`).
Each watches its bus (`busy = ~SCL | ~SDA`, registered); a per-engine counter counts the ~103 ms trigger
while the bus stays busy; after **~1.03 s of SDA held low** the engine clocks **9 SCL pulses** (~95 kHz)
and repeats every 1.034 s while it stays stuck (`sim/e0f2.txt`: SDA of E6/D7 forced low for 3 s -> bursts
at +1.034 s and +2.068 s). SCL stuck low: not tested beyond 0.2 s [open].
Engine 4 does the same on the SoC's own bus 4. Which buses E6/D7 … T9/P9 are is **[open]** (needs the board).

## 5. Fans, the failsafe, and bug candidate 7 (resolved: not a bug)

- Polarity [proven, hardware]: lab metrics (VictoriaMetrics, u01, 2026-11-16 07:51-08:24) give fan 1 at
  step 48 -> 11000 rpm, 39 -> 7800, 33 -> 6400, 27 -> 4900, 22 -> 3200, 18 -> 2200, <= 16 -> 2000 (floor):
  longer PWM high time = faster. So the pin high = drive, and "forced high" = full speed.
- **#7 was a simulation artifact.** The idle benches (`sim/idle*.vvp`, built 00:57/01:00) were compiled
  from the netlist written before the CCU2 model fix (01:05). With the current netlist the PWM runs 352 µs
  after configuration, no I2C needed, at the reset step 9 (16.9 %) [proven, sim `sim/mst-out.txt`].
- Full speed comes from: `wdt_fired` (0x2a.b1, set by any CPLD watchdog reset including the reboot
  driver's, cleared by writing 0x2a = 0), A3/B4 low (TMP75 alerts), or D6 low. So after a reboot fans run
  full until fand clears 0x2a (u01: 0x2a = 3 and ~14.4k rpm at step 9). After a cold power-up the logic
  predicts 0x2a = 0x01 and fans at step 9 (~2000 rpm), not full [inferred; not observed on hardware].
- 0x2a.b1 survives the CPLD's own reset (no GSR) but not a reconfiguration.

## 6. Bugs and weak spots, ranked by what a user would see

| # | finding | user-visible impact | route |
| --- | --- | --- | --- |
| 1 | **No fan failsafe on host death unless the CPLD watchdog is armed**: nothing times out on host silence; fans stay at the last step forever. Only TMP75 ALERT (A3/B4) forces full speed | a hung OS at a low step + rising load = overheating, no alarm | **software**: program the TMP75 T_OS/T_HYST (lm75 driver `temp1_max`/`_hyst`) to a safe limit so the hardware path forces full fans; optionally arm 0x29/0x28 and refresh 0x29 from fand (expiry = board reset, so use a generous timeout) |
| 2 | **Status 0x20 is level, not latched**: a PSU/DC/alert glitch shorter than the host's reaction pulls E7 low and is gone when 0x20 is read | "interrupt, no cause"; missed PSU dropouts | **software**: on an edge with 0x20 = all-1, log a transient; read 0x20 immediately in the IRQ path. CPLD: sticky status, write-1-to-clear |
| 3 | Fan alarm = tach exactly 0 (b2-b4) | a slow/failing fan never raises the hardware alarm | **software** (fand/monitoring already threshold rpm); CPLD: compare against a threshold register |
| 4 | **SCL used unsynchronised** in 59 FFs (filter compares raw pad with one sample) | rare misframed byte on a noisy bus 4; worst case a write lands in the wrong register (0x28/0x29 = reset, 0x15 = reset pulses) | software: verify writes by read-back, keep bus 4 at 100 kHz; CPLD: 2-FF synchronise SCL and SDA, derive all edges from synced copies (dropping the raw-pad terms, as the equivalence negative test did, is the sketch) |
| 5 | Duty change mid-period (0x50 write) | one runt/stretched PWM period per fand write: inaudible | not worth fixing |
| 6 | PWM clock from a LUT mux (0x51) | glitch only when 0x51 is written; nobody writes it | not worth fixing |
| 7 | 0x29 countdown built from latch-based dual-async FFs with LUT-generated async loads | a glitch on the load LUT could corrupt the count while armed (early/late reset) | CPLD: synchronous load. Low priority |
| 8 | D6, P5, D10, E10, A11 used unsynchronised (D6 in the GSR path: async reset release) | partial-reset states only at D6/power edges | CPLD: reset synchroniser. Low |
| 9 | P5 frame check gates GSR | a corrupted P5 stream would reset the CPLD (fans, LEDs, outputs) | [open] until P5 is known |
| – | bug 7 of the old list (PWM low until first I2C access) | none | **not a bug** (§5) |

## 7. Safety of extra hardware reads

**Proven side-effect-free (§2): any `i2cget -y 4 0x68 <reg>` byte read of any address.** The only state a
read changes is the slave's pointer and sequencer (the next read sets the pointer again), and the bus-4
recovery engine's busy monitor, which needs SDA low for ~1 s to act. Do not read 4-0x10/0x11/0x13 (config
port). No new reads were made in this pass.

## 8. Open

- Board meaning of: the four recovered buses, P5 (and its frame format), C7, F7, C11, F10, D11, E8, the
  power-sequence and pulse outputs, 0x28.b1, 0x80 b6..b0, 0x14 inputs J12/J11 debounce. Needs the
  schematic or probing (owner).
- MCCLK code 0x33; the 0x72 payload's CMDR byte.
- Naming: 452 of 944 registers (timebase, P5 receiver, PSU state machine, misc) still read `r_<cell>`;
  the recovery-engine and timer states are named only by template element.
- SCL-stuck-low behaviour of the recovery engines beyond 0.2 s; cold-power-up 0x2a on hardware.
