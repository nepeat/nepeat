# One I2C channel engine, explained

Date: 2026-11-10. This explains, for a reader who is not a hardware
engineer, what one of the 32 "I2C channel engines" in the switch's I2C FPGA
does, how it is built, and how the 32 copies differ. It summarises layers 6
and 7 (`LAYER6.md`, `channelrtl.py`). Detail views, generated from the
netlist: `mtd5/i2c_channel_B.v` (family B, the fuller one) and
`mtd5/i2c_channel_A.v`; machine-readable analysis in
`mtd5/i2c_channel_{A,B}.json`.

Marks: **V** = checked exactly on the netlist; **I** = inferred (from
simulation of the extracted logic, from the vendor driver, or by role).
Nothing here has run on the real hardware.

## What a channel is for

Each of the 32 QSFP-DD cages has a small I2C bus (two wires, SDA = data and
SCL = clock) to the plug-in module's management chip. The CPU does not drive
these wires itself. It tells the FPGA "read N bytes from register R of the
module at bus address 0x50" and the FPGA's channel engine for that cage
produces the bus waveform. The data goes through a small memory in the FPGA.

In software terms: the FPGA contains one class, `i2c_channel`, instantiated
32 times. Each object owns two pins, one 1 KiB memory, a set of registers the
CPU can write, and three state machines.

## Class diagram

    i2c_channel  (one per cage; 32 objects)
    |
    |-- ports from the shared PCIe core
    |     clk (62.5 MHz), resets, wdata[31:0] (write data of the current PCIe write),
    |     address selects (family B: "my page" + address bits 7:2; family A: finished
    |     per-register strobes), byte enables, read/write qualifiers
    |-- ports back to the core: register read-back bits, status, memory read data
    |
    |-- registers (host writable, base cb = 0x1000 + channel*0x100)       [V offsets]
    |     profile      cb+0x00  three 8-bit clock codes (profile 1, 2, 3)
    |     profsel      cb+0x08  2 bits: which profile is in use
    |     control      cb+0x0C  bit 0 master reset (pulse), bit 31 soft reset (pulse)
    |     command/start cb+0x50, 0x54 (and a second set at 0x58, 0x5C)
    |     cb+0x60 (bits 0, 4, 5 writable), cb+0x68 (bits 0, 1, 4, 5): not used by the driver
    |
    |-- clock divider          16-bit down-counter, reloaded with the selected code [V]
    |-- bit sequencer          6-bit phase register + 4 commands (START, STOP, 0, 1)  [V/I]
    |-- byte sequencer         shifts bytes into/out of the bit sequencer, ACK handling [I]
    |-- transaction controller one-hot state machine, picks a command set, runs it     [I]
    |-- memory                 one RAMB18: 256 words x 32 bits, write + read windows   [V]
    |-- pads                   SDA and SCL, open drain (pull low or let go)           [V]

## What happens in one transaction, step by step

1. **Set the speed** (once, at driver load). The driver writes 0x00660918 to
   `cb+0x00`: byte 0 = 0x18 is profile 1, byte 1 = 0x09 profile 2, byte 2 =
   0x66 profile 3. It writes 0 to `cb+0x08` to choose profile 1. In the
   simulation of the extracted logic, writing 0x00660918 gives exactly these
   three bytes in the three 8-bit register groups (V for the decode, I for
   the byte order, confirmed by this result). Power-up values (from the
   flip-flop INIT bits, V): profile 1 = 0x66, 2 = 0x17, 3 = 0x08; select = 0.
   Select 3 behaves like select 0 (V).
2. **Release reset.** The driver writes 0x80000000 to `cb+0x0C`. Bit 31 of
   that register is a self-clearing one-cycle pulse that resets the channel's
   host-side logic and its memory output (V). Bit 0 is the master reset: it is
   stored only for the duration of the write and resets the whole engine
   through a two-stage synchroniser (V for the structure, I for "pulse").
3. **Put data in the memory** (writes only): the CPU writes the bytes to
   send into the channel's write window (`0x4000 + ch*0x200`). Reads come
   back through the read window (`0x8000 + ch*0x200`). Both windows are
   decoded to this channel's memory (V: the window address checks).
4. **Write the command.** `cb+0x50`: bit 28 = read, bits 15:8 = 7-bit device
   address, bits 7:0 = byte count (driver). The register holds host bits
   15:0, 23:16, 28, 29 and 31 (V). The meaning of 23:16, 29 and 31 is not
   known.
5. **Start.** `cb+0x54`: bits 7:0 = register offset inside the module, bit
   31 = start/busy. In family B, writing bit 31 at `0x54` sets a "go" flag
   (V). There is a second, identical register pair at `0x58`/`0x5C` with its
   own "go" flag (V); the vendor driver never uses it. A 2-bit pointer
   selects which pair the controller runs (V for the mux, I for "two queued
   transactions").
6. **The transaction controller** loads the selected command into a working
   register. It then has the byte sequencer produce the I2C sequence: START,
   address+write, ACK, offset, then for a read a repeated START, address+read
   and the data bytes, or for a write the data bytes, then STOP (I; the
   sequence is the driver's description, matched by the commands below).
7. **The bit sequencer** turns each bit-level command into pin movements. It
   is a 6-bit phase register that advances once per divider "tick" (V). From
   the idle state each command is a fixed walk through the phases (V, from
   the next-state logic):

   | command | phases (ticks) | what the pins do (I, simulation) |
   |---|---|---|
   | START | 0-4-32-36-1-5-33-37-8-12-40-0 (11) | SDA falls while SCL is high, then SCL low |
   | STOP | 0-44-9-13-41-45-0 (6) | SCL rises, then SDA rises while SCL is high |
   | bit, SDA released | 0-7-35-39-10-14-0 (6) | one SCL pulse with SDA let go (send 1 / read) |
   | bit, SDA low | 0-2-6-34-54-19-0 (6) | one SCL pulse with SDA pulled low (send 0) |

8. **The clock divider** sets the pace. It is a 16-bit counter that counts
   down and, at zero, reloads with the selected profile code (bits 7:0; the
   upper bits load 0) (V). So a tick comes every *code + 1* clock cycles of
   the 62.5 MHz clock (V). While SCL has been let go but still reads low
   (a slow module holding the clock: "clock stretching"), the counter keeps
   reloading, so the engine waits (V).

   One bit takes 6 ticks. Simulating the extracted logic with a bit command
   held gives these SCL periods (I):

   | select | code | SCL period | SCL frequency | driver's label |
   |---|---|---|---|---|
   | 0 (profile 1) | 0x18 | 162 clocks | 386 kHz | 400 kHz |
   | 1 (profile 2) | 0x09 | 72 clocks | 868 kHz | 1 MHz |
   | 2 (profile 3) | 0x66 | 632 clocks | 98.9 kHz | 100 kHz |

   The periods are 6 x (code + 1) plus 12-14 clocks for the
   wait-until-SCL-is-high check. With a live bus the byte sequencer and ACK
   handling may add time between bits, so the real rate can be slightly
   lower. The code formula is not 62.5 MHz / (4 x (code + 1)); the "40 MHz /
   (4 (code + 1))" guess in the protocol review fits the labels but not this
   clock. Profile select 0 also uses a different split between SCL high and
   low time than selects 1 and 2 (V: the SCL logic reads the select bits).
9. **The pins** are open drain (V): the FPGA output is always 0. A
   flip-flop decides between "pull the wire low" and "let go" (the pull-up
   resistor then makes it high). Both flip-flops start in "let go" and are
   forced there by reset or abort (V). SDA follows a one-flip-flop pipeline
   from the byte sequencer; SCL is let go only when two phase-derived
   conditions agree (V).
10. **Done.** The go flag clears and the driver's poll of `cb+0x54` bit 31
   sees "not busy". A NACK from the module sets status `cb+0x60` bit 1 (driver,
   not located in the logic yet). The read data is in the read window.

## The memory

One RAMB18 per channel in "simple dual port, 36 bits" mode: 8 of the 9
address bits are used, so 256 words of 32 bits (V). Port A reads, port B
writes with 4 byte-write enables (V). Address and data inputs are 2:1
choices between a registered copy of the CPU's access and the engine's own
pointer and shift data. One flip-flop (`B0944` in family B) selects between
them, presumably "engine busy" (I). The memory's output register is reset by
the PCIe reset or the `cb+0x0C` bit 31 pulse (V).

## How the 32 copies differ

- **Which cage.** Decoding each instance's register page gives its driver
  channel number (V, `mtd5/update.channel-map.txt`). The pad-pair order used
  so far (`ch00..ch31`) is not the driver order: for example ch21 is driver
  channel 17 (port 18) and ch25 is driver channel 0 (port 1).
- **Two families of 16.** About 776 of the logic elements are the same in
  both families: the bit sequencer, the commands, the pins, the memory and
  most command registers. The difference is how the netlist is cut (V).
  Family B channels get raw address bits and decode their own registers;
  family A channels get ready-made register strobes from the shared core.
  In family A part of the divider and profile register sits outside the
  channel's block. Family A = driver channels 0-9, 21 and 27-31, family B =
  10-20 and 22-26, the same in both FPGA builds in the flash. So this is a
  synthesis choice about where shared logic goes, not a different I2C
  behaviour (I; no formal equivalence check).
- **Physical placement.** Every copy sits elsewhere on the chip, so its
  wiring, its packing of logic into slices and some optimiser decisions
  differ. Layer 6 records this per copy. The logic that follows the template
  exactly ("pure") is 69-95% per copy in family A and 79-96% in family B
  (`LAYER6.md`).

## What is still open

- The byte sequencer and the transaction controller are identified as
  structures (their flip-flops and next-state equations are in the `.v`
  view) but not yet written as readable state tables.
- Status register `cb+0x60` bit 1 (NACK) was not located; the writable
  bits 0, 4, 5 at `0x60` and the register at `0x68` are unexplained.
- Family A's register map is only partly decoded (0x50-0x5C and 0x68
  strobes found; the profile register mostly outside the template).
- A full simulation of one transaction (including the shared PCIe core)
  was not done. The forced-command simulation above drives the bit
  sequencer directly.
