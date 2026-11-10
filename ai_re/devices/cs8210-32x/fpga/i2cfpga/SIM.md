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

    cd fpga/i2cfpga && ./simrun.sh        # build, 15 scenarios x 2 families, 10 random seeds, sideband map

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
- Not simulated: clock stretching by the device, arbitration, a NACK on a
  data byte (the device model always ACKs data), reads longer than 4 bytes,
  the interrupt path (`CFGINTERRUPT*` not acknowledged by the testbench).
