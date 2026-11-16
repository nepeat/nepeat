# CS8210-32X FPGA reverse engineering

Goal (owner, 2026-11-10): reproduce the vendor FPGA images byte-identically
from a representation that people can read, then make that representation
progressively more meaningful without losing byte identity.

## I2C FPGA (`i2cfpga/`)

Xilinx 7-series, IDCODE `0x03632093` (believed XC7A75T, same die as the
XC7A100T). PCIe endpoint `1d0f:8571`: 32 I2C masters for the QSFP-DD cages
plus cage sideband signals. Configuration is in the aux SPI flash
(`mtd5` "i2cfpga"); our copy is `../images/spi-2026-11-09/mtd5-i2cfpga.bin`.

Flash layout (from the packet listing):

| offset | content |
| --- | --- |
| `0x000000` | multiboot header: watchdog `TIMER 426bf8d7`, `WBSTAR 0x400000`, `IPROG` |
| right after | golden image (compressed: FDRI runs + MFW duplicate-frame writes) |
| ~`0x400000` | update image (`WBSTAR 0`, boots straight through); the one that normally runs |
| `0x801000`.. | erased (`0xFF`) |

Layers, each verified to rebuild the exact bytes before the next:

1. **packets** — `i2cfpga/bitasm.py`: image <-> line-based packet listing
   (`sync`, `noop`, `w1 REG ...`, `w2 @blob`, commands by name) plus payload
   blobs. `bitasm.py check IMAGE` -> BYTE-IDENTICAL for the 32 MiB flash
   image and the 8 MiB vendor update file. **Done.**
2. **frames + FASM** — configuration frames by address, an encoder that
   re-packs them exactly as the vendor tool did, and Project X-Ray FASM with
   unexplained bits carried separately. **Done**, byte-identical end to end
   (`i2cfpga/roundtrip.sh`, report `i2cfpga/LAYER2.md`): 9448 frames on the
   xc7a100t fabric, ~458k FASM lines, 3 bits not explained by the database.
   (Layers 2 and 3 share that report.)
3. **FASM + unknown bits** — part of the layer-2 report: prjxray FASM, with
   the 3 bits the database does not explain kept separately. **Done.**
4. **netlist** — `i2cfpga/fasmnet.py`: FASM <-> cells (sites with their
   FASM lines) + nets (driver pin -> sink pins with the pips that realise
   them) + an `unassigned` bucket (empty here); routing graph from
   prjxray-db (`i2cfpga/xc7graph.py`); logical UNISIM-like view and
   structural Verilog (`i2cfpga/xc7bels.py`). **Done**, lossless:
   netlist -> FASM is identical and the image rebuilds byte for byte
   (`i2cfpga/roundtrip4.sh`, report `i2cfpga/LAYER4.md`).
5. **blocks** — `i2cfpga/blocks.py`: every cell/net/primitive annotated
   with a block path (`pcie/*`, `clock/*`, `i2c/chNN`, `sideband/io`,
   `core/*`, `io/*`), with V/I evidence; the netlist split per block
   rebuilds the image byte for byte. Report `i2cfpga/BLOCKS.md`, small
   outputs in `i2cfpga/mtd5/`. **First pass done**: PCIe block + pipe
   clock matched to the Xilinx 7-series PCIe IP, 32 replicated I2C channel
   engines found (two structural families of 16).
6. **module template** — `i2cfpga/modules.py`: the 32 channels re-rolled
   into one module (`i2c_channel`, templates for families A and B with a
   recorded correspondence) + 32 instances (placement, port bindings,
   packing, routing, explicit deviations) + the non-channel remainder. LUT
   INITs, FF/control/carry lines and BRAM/IOB site features are regenerated
   from the template; the rebuilt netlist is identical and the image
   rebuilds byte for byte (`i2cfpga/roundtrip6.sh`, report
   `i2cfpga/LAYER6.md`). **Done.** 66-97% of each instance is pure template
   (after a membership-growth pass into the shared core). Also rebuilds the
   vendor `i2c_fpga_v01.02.0E` file byte for byte.
7. **readable channel** — `i2cfpga/channelrtl.py`: a view (not lossless) of
   the template: register offsets decoded from the logic (`cb+0x00` profile,
   `0x08` select, `0x0C` resets, `0x50..0x5C` command/start, ...), the SCL
   divider (reload = selected profile code, tick every code+1 clocks, 6
   ticks per bit), the bit-level FSM and its four commands, pads, BRAM
   wiring, and the driver channel number of every instance. Pseudo-RTL in
   `i2cfpga/mtd5/i2c_channel_{A,B}.v`; plain-language explanation in
   `i2cfpga/CHANNEL.md`; controller state tables in
   `i2cfpga/mtd5/i2c_channel_fsm.txt`. **Done** for family B (family A's
   register decode partly outside its template).

Simulation (a view on layer 4b, not part of the byte round trip):
`i2cfpga/simnet.py` + `simtb.py` + `simrun.sh` run the whole recovered
design in Verilator with PCIe TLPs in and an I2C device model on one cage
(report `i2cfpga/SIM.md`). Full transactions run end to end (SCL 386 /
868 / 99 kHz for the three profile codes, NACK and status bits, bus clear);
a family-A and a family-B channel behave identically in 25 co-simulated
sequences; the four sideband signals of every port are mapped to pins
(`i2cfpga/mtd5/update.port-pins.txt`).

Large outputs go in `work/` (git-ignored).

## Lattice parts

System FPGA, MDIO FPGA, fan and LED CPLDs: `.vme` programming images in
`../analysis/extracted/root/fw/`. Parts (from the VME IDCODE checks): sys CPLD LCMXO3LF-6900C, LED CPLDs LCMXO3LF-2100C, fan CPLDs LCMXO3LF-1300C, MDIO FPGA ECP5-class; see [../inventory.md](../inventory.md), [../cpld-diff.md](../cpld-diff.md).
