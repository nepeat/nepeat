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
   engines found (two structural families of 16), register addresses not
   yet tied to logic.

Large outputs go in `work/` (git-ignored).

## Lattice parts

System FPGA, MDIO FPGA, fan and LED CPLDs: `.vme` programming images in
`../analysis/extracted/root/fw/`. Not identified yet.
