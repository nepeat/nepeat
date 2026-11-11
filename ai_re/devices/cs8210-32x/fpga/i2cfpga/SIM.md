# Full-design simulation of the I2C FPGA (update image)

Date: 2026-11-10 (PROGRESS entry (l)). Offline only: the recovered netlist
runs in a simulator on the workstation. Nothing touched the switch.

Marks: **V** = exact structural fact of the netlist; **I** = depends on the
simulation model (below) or on interpretation. Everything the simulation
shows is at best "V for this netlist under these assumptions". The FPGA
itself never ran any of it.

## What is simulated

`simnet.py` turns the layer-4b logical view of the flash update image into
one Verilog module (113k lines, 4.8 MB). All 21,087 user-clock flip-flops,
30,993 LUT outputs, 270 CARRY4 and the 32 channel BRAMs, i.e. the shared
PCIe core, all 32 I2C channels and the sideband logic. `simtb.py` writes a
testbench around it. `verilator --binary --timing` compiles both in about a
minute and simulates 10,000-20,000 clocks per second of wall time.

    cd fpga/i2cfpga && ./simrun.sh        # build, 15 scenarios x 2 families, 10 random seeds, sideband map,
                                          # 19 behaviour tests (summary work/sim/tests.txt)

Testbench, by part:

| part | what | assumption |
|---|---|---|
| PCIe hard block | not simulated; its fabric-facing pins are driven by the testbench | TRN interface as in Xilinx 7-series (active high; 64-bit; TLP DW0 in `TRNRD[63:32]` of the first beat; a beat is taken on a rising edge with `TRNRDSTRDY`; `TRNRBARHIT0` = BAR0) (I) |
| host writes | 3DW MWr, 1 DW, first BE 0xF; payload bytes big-endian within the dword | the byte order was already confirmed in layer 7 (profile register) and is again here (read data returns in host order) |
| host reads | 3DW MRd; the completion is taken from `TRNTD` (`TRNTDSTRDY0` = 1) | (I) |
| other pins | link up, `USERRSTN` released after 30 clocks, LTSSM L0, all pads pulled up, presence/IntL inputs high (absent) unless a test drives them | (I) |
| asynchronous clear/preset | applied at the clock edge (`--sync-reset`) | **I.** With true asynchronous behaviour Verilator never settles after reset: several FFs clear themselves through logic, e.g. the `cb+0x0C` bit-31 pulse. With the synchronous form a self-clearing pulse lasts one clock instead of a few ns. |
| BRAM model | 512 x 32 behavioural, synchronous read on port A, byte write on port B; byte k is enabled by `WEBWE[2k]`; RAMB18-type sites read out on `DOADO`/`DOBDO`, FIFO18-type sites on `DO0..31` | the WE mapping is the engine's own write pattern (it raises pin pairs 2k, 2k+1 per byte) (I) |
| clocks | one 62.5 MHz clock (16 ns); FFs on other clocks (PIPE/DRP side) held at INIT | (I) |
| I2C device | 7-bit address 0x50 (`+dev`), ACKs its address and written bytes, serves `mem[i] = i ^ 0xA5` on reads, no clock stretching | ideal device (I) |
| bus monitor | samples SDA on every SCL rise and frames by START/STOP: START, 9-bit groups (byte + ACK bit), STOP | independent of the device model |

## Results (V for this netlist under the model above)

### A transaction, end to end

Driver sequence (`reviews/2026-11-10-i2c-fpga-protocol.md`): probe writes
`cb+0x0C = 0x80000000`, `cb+0x08 = 0`, `cb+0x00 = 0x00660918`; a read is
`cb+0x50 = 1<<28 | addr<<8 | len`, `cb+0x54 = 1<<31 | offset`, poll `cb+0x54`
bit 31, read `cb+0x60`, read the window. Channel 17 (pad pair ch21), read
4 bytes from offset 0x10 of device 0x50, profile 1:

    [2215]  start written (0x54 bit 31)
    [2424]  wire: START
    [3875]  wire: 0xa0 ACK            address 0x50 + W
    [5333]  wire: 0x10 ACK            register offset
    [5627]  wire: START               repeated START
    [7078]  wire: 0xa1 ACK            address 0x50 + R
    [8536]  wire: 0xb5 ACK            data (master ACK)
    [9994]  wire: 0xb4 ACK
    [11452] wire: 0xb7 ACK
    [12910] wire: 0xb6 NACK           last byte: master NACK
    [13154] wire: STOP
    [13501] poll 0x54 -> 0x00000010   busy bit 31 clear (it read 0x80000010 while running)
            status 0x60 = 0x00000001  read window word 0 = 0xb6b7b4b5 (bytes in host order)

A write (`len` 3, offset 0x20, window word 0x33221100) puts
`S A0 20 00 11 22 P` on the wire, every byte ACKed, status 0x00000001.

### SCL timing (confirms layer 7)

Measured rising edge to rising edge over all bits of real transfers:

| select | code | SCL period | frequency | layer-7 prediction |
|---|---|---|---|---|
| 0 (profile 1) | 0x18 | 162 clocks | 385.8 kHz | 162 |
| 1 (profile 2) | 0x09 | 72 clocks | 868.1 kHz | 72 |
| 2 (profile 3) | 0x66 | 631-632 clocks | 98.9 kHz | 632 |

A byte takes exactly 9 SCL periods and bytes follow each other with no
gap (for example 3875 -> 5333 = 1,458 = 9 x 162). A 4-byte register read
takes 10,730 clocks (172 us) from START to STOP at profile 1, a 3-byte write
7,527 clocks; the engine starts the bus about 210 clocks after the start
write.

### Status, busy and command fields

| field | result | how |
|---|---|---|
| `cb+0x54` bit 31 | busy: 1 from the start write until the transaction is finished, then 0 (the offset bits read back) | polled |
| `cb+0x60` bit 0 | slot 0 finished without NACK | read after each case |
| `cb+0x60` bit 1 | slot 0 NACK (address NACK; set with STOP right after the NACKed address byte) = the driver's -ENXIO bit | device at 0x51 absent |
| `cb+0x60` bits 4, 5 | the same for slot 1 (`cb+0x58`/`0x5C`) | `+ctx=1` |
| clearing | bits clear when the next start of that slot is written (status read 0x00000002 before, 0x00000000 right after the start); also write-1-to-clear (0x02 clears bit 1, 0x01 does not) | `nack_then_ok`, `w1c` |
| command bit 28 | read: `S A+W off Sr A+R data... P` | |
| command bits 15:8 | 7-bit device address (0x50 -> 0xA0/0xA1 on the wire) | |
| command bits 7:0 | byte count. Read with 0: `S A+W off Sr A+R P`, no data. Write with 0: does not stop; at least 18 data bytes were sent before the testbench stopped polling (presumably 256; I) | |
| command bit 29 | on a write: **bus clear** - no START, 9 SCL pulses with SDA released (seen as `0xff NACK`), then STOP; on a read: no visible effect | `cmdx=20000000` |
| command bits 31, 23:16 | stored (layer 7), no visible effect on reads or writes | `cmdx=80ff0000` |
| `cb+0x54` bits 7:0 | register offset, sent as the first byte after the address | |
| second slot | `cb+0x58`/`0x5C` run the same way, status in bits 4/5 | |

### Engine internals seen in simulation

- After reset, and again after a `cb+0x0C` bit-31 write, the channel wipes
  its BRAM (all 256 words written with 0). The flag that ends the wipe
  (`B0952`, set by a 9-bit address counter) also gates host window writes:
  **host writes to the data window in the first ~512 clocks (8 us) after
  that reset are dropped** (V structurally, seen in simulation). The vendor
  driver writes window data only at transaction time, long after the probe.
- Read data goes to BRAM words 128-255 (the read window); each received byte
  is written with one byte enable, so word 128 = bytes 0-3.
- The controller state tables (transaction controller, byte sequencer, bit
  sequencer) are in `mtd5/i2c_channel_fsm.txt` and in the B view.

### Family A = family B (co-simulation)

`simrun.sh` runs the same 15 directed scenarios and 10 random register-write
sequences (seeds 1-10, 40 operations each, then a read-back of all 64
register offsets) on a family-A channel (ch26, driver channel 1) and a
family-B channel (ch21, driver channel 17). The logs (every bus event with
its clock cycle, status, read-window data, register read-backs) are
**identical in all 25 runs** (`mtd5/sim-cosim.txt`). This is equivalence
by simulation on these stimuli, not a proof.

### Sideband per port (step 5)

With the presence-change interrupt masked (`0x158 = 0xffffffff`, as the
probe does), writing one-hot values to `0x178` (ResetL) and `0x170`
(LPMode), and pulling each input low while reading `0x140` (present) and
`0x160` (IntL), gives a complete one-to-one map (`mtd5/update.port-pins.txt`):

- 32 ResetL outputs (bit k = port k+1), 32 LPMode outputs, 32 presence
  inputs, 32 IntL inputs; together with the channel map, every port's six
  pins (SDA, SCL, ModPrsL, IntL, LPMode, ResetL) are known by package pin
  (FGG484 assumed, as in layer 5).
- **LPMode follows its register only while the module is present**, and
  **IntL is only visible in 0x160 while the module is present** (with all
  presence inputs high, no LPMode output moved and no input changed 0x160)
  (I: from this test; the gating logic itself is in the sideband LUTs that
  mix register bits with input pads, layer 5).
- One input, `IOB_X0Y50` (pin N15), stops the PCIe core from accepting TLPs
  while it is low: a reset-like input (I). Seven other inputs showed no
  effect in this test.

## Hardware comparison: an empty cage (entry (m))

Prediction for the live test (I2C read/write on a cage with no module; SDA
and SCL pulled up, nobody answers). Simulated on driver channel 17 (pad pair
ch21); every channel behaves the same (co-simulation). Driver-style
sequences from the vendor driver: read = `cb+0x50 = 0x10000000 | 0x50<<8 |
1`, `cb+0x54 = 0x80000000 | offset`; write of 1 byte = window word 0 =
data, `cb+0x50 = 0x50<<8 | 1`, `cb+0x54 = 0x80000000 | offset`; then poll
`cb+0x54`, read `cb+0x60`, read the window. Logs `work/sim/tests/empty_*.txt`.

| | read 1 byte, offset 0x00 | write 1 byte (0x03), offset 0x7F |
|---|---|---|
| bus | START, `0xA0` NACK, STOP (nothing else) | START, `0xA0` NACK, STOP (no offset, no data byte) |
| START on the bus after the start write | 204 clocks (3.3 us) | 215 clocks (3.4 us) |
| STOP after the start write | 1,899 clocks (30.4 us) | 1,910 clocks (30.6 us) |
| busy (`cb+0x54` bit 31) clear, first poll that saw it | 1,909 clocks = 30.5 us after the start write (poll gap 10 + ~13 clocks per read) | 1,932 clocks = 30.9 us |
| `cb+0x54` while busy / after | `0x80000000` / `0x00000000` (offset bits read back) | `0x8000007F` / `0x0000007F` |
| `cb+0x60` status after | `0x00000002` (bit 1 = NACK; the driver returns -ENXIO) | `0x00000002` |
| `cb+0x50` reads back | `0x10005001` (as written) | `0x00005001` |
| read window (`0x8000 + ch*0x200`) | unchanged: the engine writes nothing on an address NACK. After a reset wipe it is 0; after an earlier successful read it still holds that read's bytes (checked: `0xB6B7B4B5` survived a following NACK) | unchanged |

With the vendor driver's polling (`usleep_range(1000, 2000)` before the
first status read), the first poll already sees "not busy"; the call returns
-ENXIO after about 1-2 ms.

Register values as the host reads them (channel block, `cb = 0x1000 + ch*0x100`):

| offset | after power-up (configuration + PCIe reset) | after `0x0C = 0x80000000` | after the driver's probe writes | after an empty-cage read |
|---|---|---|---|---|
| `0x00` profile | `0x00081766` (profile 1 = 0x66, 2 = 0x17, 3 = 0x08) | `0x00081766` (the bit-31 pulse **restores** these defaults: checked by writing `0x0C` again after the probe) | `0x00660918` | `0x00660918` |
| `0x04` | `0x00000000` | same | same | same |
| `0x08` select | `0x00000000` | `0x00000000` | `0x00000000` | `0x00000000` |
| `0x0C` | reads `0x00000000` | `0x00000000` | `0x00000000` | `0x00000000` |
| `0x10`-`0x4C` | `0x00000000` | same | same | same |
| `0x50` | `0x00000000` | `0x00000000` | `0x00000000` | `0x10005001` |
| `0x54` | `0x00000000` | `0x00000000` | `0x00000000` | `0x00000000` |
| `0x58`, `0x5C` | `0x00000000` | same | same | same |
| `0x60` status | `0x00000000` | `0x00000000` | `0x00000000` | `0x00000002` |
| `0x64` | `0x00000000` | same | same | same |
| `0x68` | `0x00000033` | `0x00000033` | `0x00000033` | `0x00000033` |

Global registers after power-up, as read in the same run: `0x000` =
**`0x010b0009`**, the version the driver reads on the live unit (an
independent check of the netlist and the simulation set-up); `0x004` = 0;
`0x140` = 0 (no module present); `0x150` = 0; `0x158` = `0xFFFFFFFF`
(presence interrupt masked); `0x160` = 0; `0x168` = `0xFFFFFFFF`; `0x170` =
0; `0x178` = **`0xFFFFFFFF` (every module held in reset)** until the driver's
probe writes 0.

Caveats (I): power-up here means "configuration INIT values, then 30 clocks
of PCIe user reset"; the live switch has run the driver probe and possibly
other transactions, so `0x50`/`0x54`/`0x60` and the window will show the
last transaction, and `0x00`/`0x08` whatever the driver set.

## Further behaviour (entry (m))

**Clock stretching** (device holds SCL low after acknowledging a byte;
`work/sim/tests/stretch_*.txt`):
- The engine waits while SCL is held low (it releases SCL and the divider
  keeps reloading until SCL reads high, V structurally); transfers complete
  with stretches of 500 clocks after every byte, 1,000 and up to 63,000
  clocks after the address byte.
- **Timeout:** a stretch of 63,300 clocks or more (about 1.01 ms) after the
  address byte makes the engine give up (63,000 passes, 63,300 / 63,600 /
  63,900 / 64,000 fail): it sends STOP after the current
  byte and reports status bit 1 (the same bit as a NACK). The threshold is
  the same for profile selects 0 and 1 and within a tick for select 2, so it
  counts clocks, not SCL ticks (I: a 16-bit counter = 65,536 clocks would fit
  with the time between the engine's release of SCL and the stretch start).
- SCL held low forever: busy clears after about 130,700 clocks (2.1 ms),
  status `0x2`, nothing more on the wire.

**NACK of a written data byte:** **ignored**. The engine sends all remaining
data bytes and STOP and reports status `0x1` (success). A NACK of the
address byte or of the register-offset byte is detected (STOP right away,
status bit 1). So a write can fail silently if the module rejects data.

**Long reads and the window layout:**
- A 128-byte read puts the bytes in read-window bytes 0-127 in bus order
  (word 0 = bytes 0-3, little endian); 3.07 ms at profile 1.
- The length field is 8 bits (255 maximum; 0 = no data on a read). A
  255-byte read clocks 255 bytes over the bus, but each slot owns only 128
  bytes of window: bytes 128-254 overwrite window bytes 0-126 (byte 127
  survives). **128 bytes is the largest useful read**, which matches the
  driver's 128-byte chunks.
- Slot 0 uses window bytes 0x000-0x07F, slot 1 bytes 0x080-0x0FF, for reads
  (read window) and writes (write window: slot 1's data starts at write
  window byte 0x80). Window reads at 0x100-0x1FF return the last word read
  again (not decoded, I).

**Queued second slot:** a slot-1 start written while slot 0 is busy waits;
slot 1 starts about 200 clocks after slot 0's STOP. Status ends `0x11`, each
slot's data in its own window region. Reversed (slot 1 first, slot 0 queued)
the running one finishes first again: first come, first served for two
transactions (I for more than two pending, where the slot scan decides).

**Presence interrupt** (port 18, presence input `IOB_X1Y126`, pin H4, index 42 of
the testbench; `work/sim/tests/irq_presence_port18.txt`):
- `0x158` mask = `0xFFFFFFFF` at power-up. With the mask cleared, a module
  insertion sets `0x140` bit 17 and `0x150` bit 17 and the core raises one
  interrupt request: `CFGINTERRUPTN` low for one clock with
  `CFGINTERRUPTASSERTN` = 0 and `CFGINTERRUPTDI` = 1 (an MSI request; the
  fabric never reads `CFGINTERRUPTRDYN`, V).
- While masked, a change still sets `0x150` but raises no request;
  unmasking with a change pending raises one.
- **`0x150` clears with a write of ones only while the bit is masked in
  `0x158`**; unmasked, writes (ones or zeros) and reads leave it set. The
  vendor interrupt handler does exactly that: mask `0x158`, read `0x150`,
  write `0xFFFFFFFF`, unmask (protocol review §5).

## Formal check of the two templates (entry (m))

`equivcheck.py` builds the comparable region: the fan-in cone of the two pad
drivers inside the family-B template, through elements with a family-A
counterpart, 132 elements (72 LUT, 60 FF: the bit sequencer, the pad
drivers, the byte-phase flip-flops and their logic). The A/B correspondence
is re-derived by aligning the two reference instances with the pads seeded
by role (the template's family map, which keeps the larger of two pad
orientations, pairs the symmetric pad structures crosswise; 43 pairs
differ). Inputs entering the region become primary inputs; yosys `miter
-equiv` + `sat -tempinduct` compares all 132 corresponding signals.

    R=fpga/work/rt6-mtd5
    equivcheck.py $R/l6/update.l6 OUT [--strict] --realign $R/l4/update.net.json $R/l4/update.logic.json $R/l5/update.blocks.json
    cd OUT && yosys -s eq.ys          # in the ai_re devshell; reports in mtd5/equiv/

| variant | cut inputs | result |
|---|---|---|
| lenient: every pin that differs structurally is cut and fed the same input on both sides (124 such pins: reset replicas, a port on one side vs an FF on the other, 14 pins with different internal drivers) | 149 | **proven** (base case and induction step at length 1) |
| strict: the 14 pins whose drivers are inside the region on both sides but do not correspond stay connected to each side's own driver | 135 | **fails**: counterexample in the base case within 40 steps |

What this means: the lenient proof shows that the 132 common elements
implement the same next-state functions on corresponding signals. It
**assumes** equality at the 124 differing pins, so it is not an equivalence
proof of the channels. The strict failure says those 14 pins really are fed
by different flip-flops in the two families. Whether the difference is
compensated elsewhere (the co-simulation says the channels behave the same)
or the counterexample is spurious because the cut frees inputs that are
constrained in the real design was not resolved.

A meaningful full equivalence would need the shared core: two copies of the
whole 52k-primitive design (or the one design with two channels), with the
inputs equal except the TLP address page (driver channel 1 vs 17), and the
property "channel 1's pads in copy 1 equal channel 17's pads in copy 2". A
transfer takes about 10,000 clocks, so bounded model checking is out of
reach, and k-induction would need invariants over the other 30 channels and
the core's state. That was not attempted.

## Failures and corrections on the way (logged honestly)

- The first simulation never got past reset: Verilator does not settle
  with the netlist's asynchronous self-clearing resets; fixed by modelling
  CLR/PRE at the clock edge (an assumption, see above).
- Testbench bugs found and fixed: the TX ready pin is `TRNTDSTRDY0` (the
  first run held it at 0, so no completion ever left the core and the RX
  side stalled); zsh did not split the argument string, so early
  multi-argument scenarios ran with defaults; the I2C model shifted its own
  transmit register (the wire showed 0xCB for 0xB5; the bus monitor exposed
  it); the BRAM model used the wrong byte-enable pins and the wrong output
  pins for RAMB18-type sites (760 nets tied to 0 before the fix, 184 after
  it); the first window write came during the BRAM wipe; the family-A pad
  roles came from the B->A element map, which matched the two pads
  crosswise after the growth pass. They now come from the pad driver
  structure (SDA's release FF has a 3-input OR as next state, SCL's a
  4-input function).
- An earlier conclusion that register writes did not land was wrong: the
  probe was read two clocks before the write strobe.
- Vendor file `i2c_fpga_v01.02.0E`: layer 7 could not decode its channel
  map. Its core captures the TLP address one register stage later (`TRNRD`
  at 3 stages); after generalising that rule the page decode was still not
  found. Not resolved. The flash image's analysis is unchanged by the
  generalisation (re-run, same register map, same channel map).
- Entry (m): `0x150` writes looked broken until the mask dependence was
  found (clears only while masked). A `+nackdata` test replacement in the
  generator silently did not apply once (Python `str.replace` with a
  non-matching pattern); found because the expected lines were missing.
- Not simulated: arbitration with another master, a NACK on a data byte
  during a read (the master NACKs, not the device), INTx/legacy interrupt
  handling, more than two queued slots, the vendor file.
