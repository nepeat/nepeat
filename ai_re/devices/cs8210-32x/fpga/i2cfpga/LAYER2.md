# i2cfpga layers 2 and 3: frames, packing plan, FASM (byte-identical)

Date: 2026-11-10. Offline only: no access to the switch. Layer 1
(`bitasm.py`) was left unchanged and was not found to have any bugs.

## Status

| check | result |
|---|---|
| flash `mtd5-i2cfpga.bin` (32 MiB): image -> packets -> frames -> FASM + unknown -> frames -> packets -> image | **BYTE-IDENTICAL** (`roundtrip.sh`, about 2.5 min) |
| vendor `i2c_fpga_v01.02.0E_20200604.bin` (8 MiB + 60 B), same pipeline | **BYTE-IDENTICAL** |
| frames -> FASM -> frames, all 4 images (flash golden/update, vendor image0/image1) | identical; **3 unknown bits** per image, **4 ECC overrides** per image |
| CRC words (4 in the flash, 4 in the vendor file) | all recomputed and equal to the stored values; the plan stores `crc` and never the value |
| frame addresses vs prjxray xc7a100t fabric | every image writes **9448 of 9448** frames, each exactly once; 0 outside the fabric |

One command:

    cd fpga/i2cfpga && ./roundtrip.sh                    # the flash dump (default)
    ./roundtrip.sh IMAGE [OUTDIR [name1,name2]]          # any other image

Outputs go to `fpga/work/rt-<image>/`: `l1/` (listing+blobs), `l2/`
(`plan.txt`, `<name>.frames`), `l3/` (`<name>.fasm`, `<name>.unknown`,
stats), `l2r/` + `l1r/` (rebuilt), `rebuilt.bin`. The run ends with
`cmp IMAGE rebuilt.bin`.

## Files

Tracked, in `fpga/i2cfpga/`:

- `xc7geom.py`: frame-address geometry built from a prjxray `part.json`: FAR fields, the auto-increment order, and the 2 pad slots at the end of each row.
- `xc7frames.py`: **layer 2**. `decode` turns the listing into frames + plan, `encode` turns the plan + frames back into the listing, and `check` round-trips the listing and blobs. It also holds the CRC32C and the frame ECC code.
- `fasmconv.py`: **layer 3**. `frames2fasm` writes the FASM plus an unknown-bits file, `fasm2frames` goes back, and `check` round-trips. It uses the prjxray python code and database.
- `geomcheck.py`: replays the images against the 35t, 50t, 100t and 200t part.json files and cross-checks tilegrid.json (evidence for the geometry section).
- `fasmstats.py`: first-glimpse statistics plus a diff between two FASM files.
- `roundtrip.sh`: the end-to-end check.
- `mtd5/`: small reference outputs for the flash dump: `plan.txt` (62 KB packing plan), `golden.unknown`, `update.unknown`, and the stats text files. Large outputs (frames at 10.6 MB per image, FASM at about 20 MB per image) stay in `work/`.

Not tracked, in `fpga/work/`:

- `src/prjxray` @ `c9f02d85` (2025-06-05), with these submodules shallow-fetched for the C++ build: abseil, cctz, gflags, googletest, yaml-cpp, sanitizers-cmake.
- `src/prjxray-db` @ `0a0added` (2021-12-14), a sparse checkout of `artix7/` (top-level db files, `mapping/`, `xc7a100t*`, `xc7a50t*`, `xc7a35tcsg324-1`, `xc7a200tfbg484-2`).
- `src/fasm` (chipsalliance/fasm @ `ffafe821`). This is the same commit prjxray pins as a submodule. It runs in pure-python mode with the textx parser.
- `pyenv`: a nix-built python 3.14 with textx, intervaltree, simplejson, pyyaml and numpy. FPGA `fasm` is **not** in nixpkgs; nixpkgs' `fasm` is the x86 assembler.
  `nix build --impure --expr 'let pkgs = (builtins.getFlake "nixpkgs").legacyPackages.x86_64-linux; in pkgs.python3.withPackages (p: with p; [ textx intervaltree simplejson pyyaml numpy ])' -o work/pyenv`
- `build/prjxray/tools/{bitread,xc7frames2bit,frame_address_decoder}`, built with
  `nix shell nixpkgs#cmake nixpkgs#gcc nixpkgs#gnumake -c sh -c 'cmake $SRC -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DCMAKE_CXX_FLAGS="-Wno-error -include cstdint -include limits" && make bitread xc7frames2bit frame_address_decoder'`.
  These are used only for the cross-check below. The pipeline does not need them.

All python we wrote runs with `python3 -I` (`-B` in roundtrip.sh so no `__pycache__` lands in the tracked tree). The prjxray and fasm packages are put on `sys.path` explicitly by `fasmconv.py`, never through `PYTHONPATH`.

## Layer 2: configuration engine model

Each rule below is needed to reproduce the stream. Together they are
validated by the full-coverage result (every one of the 9448 frames is
written exactly once in all four images) and by `encode` regenerating
the listing and blobs identically.

1. **Start of a write.** A `FAR` write while `CMD = WCFG` starts a new frame write. So does `WCFG` itself; the FAR is the one in effect at the next FDRI. Every FDRI here begins a new write. An FDRI that continued a previous write is not modelled; `decode` refuses it.
2. **Where FDRI frames land.** FDRI data is a series of 101-word frames. Frame *k* lands at `seq[start+k]`. `seq` is the FAR order: minor -> column -> row -> half (top, then bottom) -> block type (CLB_IO_CLK, then BLOCK_RAM), with **2 pad slots after the last frame of each row**. Only 2 FDRI writes per image actually run through a pad slot; their pad frames are zero.
3. **The last frame of every FDRI write is not stored by the FDRI.** It stays in the frame buffer. In each image, 196 (golden) or 163 (update) of these are true dummies, and all of them are zero. The other 31 or 22 are the source frames for MFW.
4. **MFW.** The sequence is `CMD MFW`, `noop 12`, then `MFWR` with 8 zero words. That first MFWR writes the buffered frame to the **auto-incremented address**, which is where it would have landed naturally. After that, each `FAR t` + `MFWR` with 4 zero words writes it to `t`. In BLOCK_RAM groups every MFWR is followed by `noop 2`; CLB groups have no gaps. MFW groups never cross rows: the all-zero frame is re-sent once per row.
5. **CRC.** The CRC is CRC-32C (reflected polynomial 0x82F63B78). It runs over 37-bit `{reg[4:0], data}` for **every word written to any register**: FDRI, MFWR, FAR, CMD, MASK, CTL and so on. `CMD RCRC` resets it, and so does a write to the CRC register. With this rule all 8 stored CRCs match. Excluding CMD writes does not match.
6. **FDRI header form.** A write uses a type-1 header if it has 2047 words or fewer; otherwise it is `hdr1 FDRI 0` followed by a type-2 packet. Observed: the largest type-1 write is 1717 words and the smallest type-2 write is 2323.

### Packing plan (`plan.txt`)

The plan is the layer-1 listing with the frame payloads factored out:

    w1 FAR 00000000
    w1 CMD WCFG
    noop
    image golden                      <- following frame ops use golden.frames
    fdri 15  # CLB_IO_CLK/top/r0/c0/m0 .. CLB_IO_CLK/top/r0/c0/m13 (14 stored)
    w1 CMD MFW
    noop 12
    mfw 0000000e 0000001a..0000001b 00000022..00000025 ...   <- first = auto address
    ...
    mfw gap=2 00800000..008001ff      <- the whole BRAM row: one zero frame copied
    crc                               <- recomputed by encode
    noop 2
    w1 CMD GRESTORE

- `fdri K` writes K frames starting at the current FAR. The first K-1 come from the `.frames` file. The K-th is the MFW source frame (the frame at the first `mfw` target) or a zero dummy.
- `A..B` in an `mfw` line is the inclusive run of consecutive valid frame addresses.
- Non-zero dummies or pad frames would go to `extra.frames` with `dummy=`/`pad=` keys; there are none in these images.
- Every other line is copied verbatim: register writes, NOOP counts, raw headers and fill. The plan is 2418 lines for the 32 MiB flash: 412 `fdri`, 53 `mfw`, 4 `crc`.

Frames files use the prjxray `frames` text format (`0xFAR 0xW0,...,0xW100`). Every frame of the device is listed in FAR order, zero frames included.

Layer-2 numbers for the flash dump:

| image | stream offset | FDRI writes | MFW groups | frames via MFW | non-zero frames |
|---|---|---|---|---|---|
| golden | sync @ 0x30 (header has WBSTAR 0x400000 + IPROG) | 227 | 31 | 4011 | 5492 |
| update | sync @ 0x400030 | 185 | 22 | 4502 | 4976 |

The other two sync'd streams carry no frames. One is a stub at 0x3fffd8 with `TIMER 40004000`; the other is at 0x800014 with `TIMER 40000001`. The vendor .bin has the same layout (syncs at 0x30, 0x3fffd8, 0x400030, 0x800014). Its frames differ from both flash images: different md5s, and different designs (see below).

## Device geometry

- **IDCODE.** The images write IDCODE `0x03632093`. In UG470 (from memory, not re-checked here) that is XC7A75T; XC7A100T is `0x03631093`. prjxray-db has no 75t, and its `mapping/devices.yaml` already maps the 35t onto the 50t fabric (the same die family pattern).
- **Replay (`geomcheck.py`).** The same engine replays the listing against each part.json:

  | fabric part.json | result |
  |---|---|
  | xc7a35t / xc7a50t (5408 frames) | fails at the first FDRI to FAR `0x0000161b` (column 44 does not exist) |
  | **xc7a100t (9448 frames)** | golden and update: **9448 stored, 9448 distinct, 0 outside the fabric, 0 never written, 0 written twice**. The vendor images give the same result. |
  | xc7a200t (24060 frames) | fails at FDRI to `0x00001ca6` (row 0 has different column sizes) |

  So the set of addresses written by these images is exactly the xc7a100t address space, with the same frame count per column (part.json lists 4 rows of CLB_IO_CLK, with 58/52 columns at 28 to 42 frames each, and 4 rows of BLOCK_RAM, with 4/3 columns of 128 frames).
- **tilegrid cross-check.** Every one of the 19386 tile bit-blocks in `xc7a100t/tilegrid.json` lies inside part.json. In 220 of the 222 columns that tiles cover, the tile extent equals `frame_count`. The two exceptions are CLB_IO_CLK row 1, column 18, top and bottom, where the tiles use 28 of the 30 frames. 12 columns have no tiles. No set bit in any image falls in those gaps: the only unexplained bits are the 3 listed below.
- **prjxray C++ cross-check.** `xc7frames2bit` (prjxray C++) built an uncompressed bitstream from `update.frames`: 956,434 config words. `bitread` read it back as 9448 frames, and they are identical to ours except for word 50 (ECC) of the 4+2 frames discussed under ECC, because `xc7frames2bit` always recomputes the ECC. This also confirms that our ECC code equals prjxray's `ecc.cc`.

Conclusion (well supported, though not a die marking): the XC7A75T in this switch uses the **xc7a100t fabric** of prjxray-db. Database part used: `xc7a100tfgg484-2`. Only the package-pin mapping depends on the package; see the IO section.

## Layer 3: FASM

`fasmconv.py frames2fasm` does the following:

1. Clears the ECC field.
2. Builds prjxray bitdata and runs `prjxray.fasm_disassembler` (the same path as bit2fasm, but fed from our frames instead of `bitread`, which cannot handle MFW-compressed streams and rejects the IDCODE).
3. Writes the FASM through `merge_and_sort`.
4. Re-assembles that FASM with `prjxray.fasm_assembler`.
5. Writes every difference to the unknown-bits file: `+` for set but unexplained, `-` for produced by FASM but clear in the frames.

`fasm2frames` does the inverse: it assembles, applies the unknown bits, and re-applies the ECC.

| image | set bits (excl. ECC) | explained by FASM features | unknown `+` | `-` | FASM lines | ECC overrides |
|---|---|---|---|---|---|---|
| flash golden | 1,626,375 | 1,626,372 | 3 | 0 | 458,269 | 4 |
| flash update | 1,626,234 | 1,626,231 | 3 | 0 | 458,693 | 4 |
| vendor image0 | 1,865,477 | 1,865,474 | 3 | 0 | 524,481 | 4 |
| vendor image1 | 1,860,514 | 1,860,511 | 3 | 0 | 522,556 | 4 |

**Unknown bits.** The same 3 bits appear in all four images: frames `0x0040091A` word 68 bit 20, and `0x0040091B` word 68 bits 21 and 22. That is CLB_IO_CLK, bottom row 0, column 18, minors 26/27, in tile **CFG_CENTER_MID_X46Y84**. As tile bit offsets these are `26_2196 27_2197 27_2198`. The db's `CFG_CENTER_MID.STARTUP.PROG_USR` is `26_2197 26_2198 27_2196`: the same three bit offsets with the frames swapped. ICAP_WIDTH and DCIRESET use the same three-bit pattern. So this is almost certainly a CFG_CENTER (STARTUP/ICAP-class) setting the db does not decode. One plausible guess is a STARTUPE2 instance used to drive CCLK to the SPI flash for in-band updates. That is **inference, not verified**.

**BRAM.** prjxray decodes BRAM content (`INIT_xx`/`INITP_xx`) and none is set: in both flash images every BRAM content frame is zero (each BRAM row is a single `mfw` of the zero frame). The 48 used RAMB18 halves are uninitialised RAMs or FIFOs.

**ECC.** Word 50 bits [12:0] are derived, not data. Rule found here (verified on all 4 images): Vivado stores the ECC computed with the **INIT bits of SRL/LUTRAM LUTs zeroed**. Frames `0x21920`/`0x21921` contain SRLs (CLBLM_L_X50Y164/Y172); their stored ECC equals the masked computation and not the plain one. `fasmconv` implements this as `dynamic_mask` (taken from the FASM `.SRL`/`.RAM` features). The 4 remaining overrides are frames `0x2199C..0x2199F` (top row 1, column 51, minors 28 to 31: the used GTP quad, GTP_CHANNEL_0_X130Y162 / GTP_COMMON). There the stored ECC is **0x0000** in all images although the frames are not zero, perhaps because GTP DRP-accessible content is masked. That is not modelled; these frames are kept as explicit `ecc` lines. For a modified design, delete those lines to get the plain computed ECC.

## First glimpse: flash update image (the one that normally runs)

Counts are inferred from feature presence (see `fasmstats.py`) and are approximate. Full output: `mtd5/stats-update-vs-golden.txt`, `mtd5/stats-vendor-image1-vs-flash-update.txt`.

- **Logic:**
  - about 25.1k LUTs with non-zero INIT (route-through LUTs included), about 21.6k FFs, 15 SRL/LUTRAM LUTs, 270 slices using carry, and 7716 slices with any feature.
  - 322k routing pip features.
  - 48 RAMB18 halves in use, none initialised. **0 DSP48.**
- **Clocking:**
  - 5 BUFGCTRL (3 bottom, 2 top), 11 BUFHCE, and one **MMCME2_ADV** (CMT_TOP_R_LOWER_B_X8Y61).
  - The MMCM's counters decode to M=10, D=1, CLKOUT0=/8, CLKOUT1=/4, CLKOUT2=/16 (plus disabled outputs). With a 100 MHz reference that gives VCO 1 GHz and 125/250/62.5 MHz. That is the Xilinx `pcie_7x` pipe-clock MMCM, and the PCIe block's `USER_CLK_FREQ=1` means a 62.5 MHz user clock. This is inference.
- **PCIe hard block in use** (PCIE_BOT_X104Y167, 104 features):
  - `LINK_CAP_MAX_LINK_SPEED=2` (5 GT/s, gen2) and `LINK_CAP_MAX_LINK_WIDTH=1` (x1). This matches the observed gen2 x1.
  - `CLASS_CODE=0x058000` (memory controller, other); `BAR0..2 = 0xFFF00000` (three 1 MiB 32-bit memory BARs).
  - Capabilities: MSI (64-bit, `MULTIMSGCAP=2`) + MSI-X, AER, DSN, VSEC (`ID 0x1234`), RBAR; `INTERRUPT_PIN=1` (INTA).
  - The 7-series PCIE_2_1 takes vendor/device IDs through fabric ports, so `1d0f:8571` lives in tie-off routing, not in attributes.
  - One GTP lane is used: GTP_CHANNEL_0_X130Y162 (top quad, bank 216).
- **IO:**
  - 218 used IOB sites: 86 in, 67 out, 65 inout.
  - The 65 inouts are LVCMOS33, drive feature `I12_I8`, SLOW, no pull. 64 of them fit **32 I2C masters × SDA/SCL** as open-drain bidirectionals (inference).
  - Outputs are LVCMOS33 `I12_I16` SLOW; 71 inputs are LVCMOS25/33; 13 are `IN_ONLY`. There are 20 more IOB sites with only pull/termination configuration.
  - Used IOBs per bank: 13: 25, 14: 17, 15: 32, 16: 50, 34: 48, 35: 46.
  - **Package:** every used IOB is bonded in xc7a100t **FGG484 and FGG676**, but not in CSG324 or FTG256. The pin names in the stats file assume FGG484 and are unconfirmed until the package marking is read.
- **Golden vs update:**
  - The IOB configuration, PCIe attributes, MMCM and BUFG set are identical, and BRAM content is identical (empty).
  - Placement and routing differ completely: about 305k INT features changed in each of INT_L and INT_R, and 26 to 57k per CLB tile type.
  - LUTs 25140 -> 25105, slices 7868 -> 7716, BUFHCE 12 -> 11, GTP_INT_INTERFACE features 16 -> 13.
  - So: the same pinout and interface, with a re-implemented (probably slightly changed) core.
- **Vendor v01.02.0E image1 vs flash update:**
  - The vendor build is larger: 28.6k LUTs, 24.4k FFs, 394 carry slices, 8778 slices.
  - In the vendor build, all 64 I2C-like inout pins have drive `I12_I16`; in the flash update they are `I12_I8`.
  - P20 is an output in the vendor build and an input in the flash update.
  - 15 inputs switch between plain `IN` and `IN_ONLY` features.
  - PCIe: the vendor build has only BAR0, while the flash update adds **BAR1 and BAR2** (0xFFF00000 each).
  - The 80 IOB differences are listed in the stats file.
  - So the flash holds a newer, different build, consistent with "flash newer than the 2020 vendor file".

## Verified vs inferred

Verified by exact comparison:

- Both full images rebuild byte-for-byte through every layer.
- The frame coverage and validity against xc7a100t.
- The CRC rule.
- The ECC rule for SRL frames.
- Agreement with the prjxray C++ round trip.
- The FASM counts.

Inferred:

- XC7A75T = xc7a100t fabric. The die identity comes from IDCODE and geometry fit, not from a datasheet check done here.
- What the 3 CFG_CENTER bits mean.
- Why the GTP frames have ECC 0.
- The I2C, PCIe-clocking and package interpretations.
- Resource counts (heuristics on feature names).
- Hardware behaviour of the dummy frame. We assume it is discarded, and it is always zero here, so either behaviour gives the same configuration.

## Limitations / next steps

- The plan is explicit (one line per FDRI run and per MFW group). Deriving it automatically (re-implementing Vivado's compressor: walk frames in order, write a row's repeated contents once and MFW the rest) would let a *modified* FASM be packed without a hand-edited plan. Today, a design change that changes which frames are equal needs the plan regenerated. Any frame-content change that keeps the duplicate structure re-encodes directly, with correct CRCs.
- MFWR groups with non-zero payload, FDRI continuation without a new WCFG/FAR, and frames written twice are rejected with a clear error rather than modelled.
- Next on the meaning side:
  - Trace the PCIe user interface to the I2C engines (routing from PCIE_BOT ports into the fabric).
  - Identify the 64 I2C pins against the board's QSFP-DD sideband nets.
  - Confirm the package from the chip marking.
