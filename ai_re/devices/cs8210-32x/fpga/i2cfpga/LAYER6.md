# i2cfpga layer 6: the 32 I2C channels re-rolled into one module (lossless)

Date: 2026-11-10. Offline only (no switch, serial or power access). Layers
1-5 were not changed (`bitasm.py`, `xc7frames.py`, `fasmconv.py`,
`fasmnet.py`, `xc7bels.py`, `blocks.py` untouched). New tools:
`modgraph.py` (shared analysis graph), `modules.py` (this layer),
`roundtrip6.sh` (end-to-end check), `channelrtl.py` (layer 7, see
`CHANNEL.md`).

Marking as in the other reports: **V** = exact structural check, **I** =
inference.

## Status

| check | result |
|---|---|
| layer-4 netlist -> template + 32 instances + remainder -> netlist (`modules.py extract` / `rebuild`, `fasmnet.py same`) | **IDENTICAL** for the golden and the update image |
| that netlist -> FASM -> frames | `cmp` equal for both images |
| frames -> packets -> 32 MiB flash image (`roundtrip6.sh`, fresh run of layers 1-6 in this session) | **BYTE-IDENTICAL** to `images/spi-2026-11-09/mtd5-i2cfpga.bin` (see "Verification") |

    cd fpga/i2cfpga && ./roundtrip6.sh                # layers 1-5 (roundtrip4.sh), then layer 6; ~15 min
    REUSE=1 ./roundtrip6.sh IMAGE OUTDIR              # reuse OUTDIR/l4, l5 of an earlier roundtrip4.sh

## What layer 6 is, in software terms

Synthesis inlined one I2C-channel module 32 times. Layer 6 re-rolls it:

- a **class** (`template.json`): one canonical list of elements (LUT
  functions, flip-flops, carry chains, the RAMB18, the two pads) with their
  logical configuration and their connections in local names;
- **32 objects** (`chNN.json`): for each channel, which physical site/BEL
  holds each template element, which design nets are bound to the template's
  ports, the per-instance physical data (packing choices, routing) and an
  explicit list of everything that does not follow the template
  (deviations);
- the rest of the design (`remainder.json`): everything that is not a
  channel, as raw layer-4 cells and nets.

Two structural families of 16 channels (layer 5) are kept as two templates,
`A` and `B`, of the same module, plus a recorded correspondence between them
(see "The two families").

## Files

`work/<rt>/l6/<image>.l6/` (git-ignored, about 59 MB per image, mostly routing; the
template itself is 0.56 MB):

| file | content |
|---|---|
| `template.json` | `families.A` / `families.B`: `elements` (id, kind, logical config, inputs as template nets), `order`, `ports` (input ports: reference net, `global_net` when the same net reaches every instance), `outputs`, `members`, `reference`; `family_of`; `family_map_B_to_A` |
| `chNN.json` | `family`, `pads`, `placement` (element -> primitive/BEL node), `bind` (template net -> design nets; first = primary, others replicas), `cells` (per physical cell: generated slots with their element ids, packing lines, raw deviation lines), `nets` (the physical net records whose driver is in this instance: routing), `deviations` |
| `remainder.json` | `cells_raw` (FASM lines of every cell that no instance generates), `nets` (all other routed nets), `unassigned` |
| `meta.json` | netlist header, cell skeleton (name, type, tile, implicit, unknown pins, site prefix), the alias map needed to see logical nets at LUT pins, cells exempt from the default-line rule |

Element config by kind:

- LUT output (`O6` or `O5` of a physical LUT): `k`, `table` (canonical truth
  table over `inputs`, minimal over input permutations), `inputs` (template
  nets in canonical order).
- FF: `prim` (FDCE/FDPE/FDRE/FDSE), `init`, `clkinv`, `ce`, `sr`, `srpin`,
  `ports` (D, CE, CLR/PRE/R/S, C).
- CARRY4: `precyinit`, `ports` (CI, CYINIT, DI[i], S[i]).
- RAMB18E1 / IOBUF: `site_features` (site-relative FASM features of the
  BRAM site, or of the IOB and its OLOGIC/ILOGIC/IDELAY companions), `ports`.

## How the bytes are regenerated

Every FASM line of a SLICE cell is attributed to a **slot** of that cell:
`LUT x` (the INIT line), `FF x`, `5FF x` (ZINI, ZRST, FFMUX/5FFMUX),
`OUTMUX x`, `CARRY4` (PRECYINIT.AX/CIN/C1, xCY0), `CTRL` (CEUSEDMUX,
SRUSEDMUX, FFSYNC, LATCH, CLKINV) and `DEFAULT`. A slot belongs to an
instance when all primitives in it are elements of that instance; then
rebuild generates it:

| slot | generated from | per-instance data |
|---|---|---|
| LUT INIT | template function(s) + the template net seen on each physical pin (pin -> routed net -> logical net -> bound template net), replicated over unused pins; LUT6_2 = O6 in the upper and O5 in the lower 32 bits | none (the pin assignment is implied by the routing) |
| FF / 5FF | `ZINI` iff INIT = 0, `ZRST` iff FDCE/FDRE/LDCE | `xFFMUX.*` / `x5FFMUX.*` (packing) |
| CTRL | FFSYNC (FDRE/FDSE), CEUSEDMUX (CE not tied), SRUSEDMUX (SR not tied), CLKINV, from the slice's FF elements | none |
| CARRY4 | `PRECYINIT` from the template | `CARRY4.xCY0` (packing) |
| OUTMUX | | `xOUTMUX.*` (packing) |
| RAMB18 / IOB / IOI site | the template's site-relative features + the instance's site prefix | none |
| DEFAULT (all slices) | `NOCLKINV` unless `CLKINV`; `PRECYINIT.C0` unless another PRECYINIT; in SLICEM `A/B/CLUT.DI1MUX.*` defaults unless the LUT uses its own DI | none |

Rules checked on the whole design before relying on them (V): every one of
the 25,237 LUT INITs equals its O6/O5 functions replicated over the unused
inputs (all 5,756 LUT6_2 have A6 = VCC); the CTRL lines of all 7,074 slices
with FFs follow from the FF configuration; the default lines hold for every
slice (exceptions would be kept raw; there are none).

A slot that mixes an instance with other logic (another channel or the core;
753 slots) stays raw in the remainder. A slot of an instance whose element
does not match the template, or whose derived lines differ from the original,
is stored raw in that instance as a **deviation**. Extract checks that every
generated slot reproduces the original lines exactly; rebuild then needs no
check of its own, and `fasmnet.py same` compares the whole netlist.

Routing is physical: each instance keeps the net records it drives (pips,
ppips, sinks) and the binding says which template net each realises.

## Method

1. **Membership**: layer-5 channel blocks plus the pad pair. Growing into
   the shared core was tried: once a core node may belong to only one channel,
   0-18 nodes per channel remain, so the per-channel logic is essentially the
   layer-5 block (the rest is shared decode, read mux and routing LUTs).
2. **Pairwise alignment** (`Aligner`): graph matching on the logical
   analysis graph (LUT6_2 halves split; LUT inputs labelled by their orbit
   under the function's input symmetries). Seeded from the pad pair (both
   orientations tried); neighbours matched when (port role, neighbour role,
   label) is unique on both sides; when stuck, signatures over matched
   neighbours and shared nets; then a label-free pass (same kind, same
   neighbourhood, other function -> a recorded *function deviation*); then
   tie-breaks (counted). Edge consistency of the matched parts (edges between
   matched nodes that exist with the same roles on both sides) was
   95.7-99.9% in a test run with references ch00/ch03 (without the label-free
   pass).
3. **Families**: each channel goes to the layer-5 family reference it matches
   best; the **medoid** of each family (largest total match) becomes its
   reference: A = ch26, B = ch21 (update image).
4. **Template** = the reference's nodes matched in at least half the
   family. Element ids `A0000..`, `B0000..` in breadth-first order from the
   pads.
5. **Binding** per instance: template outputs bind to the instance's nets;
   inputs are checked against them. A second net on the same template net is
   a *replica* when its driver is structurally equivalent (same function, same
   inputs, two levels; Vivado fanout replication), otherwise a *connection
   deviation*. LUT inputs use the canonical permutation that agrees with the
   bindings.

## Statistics (update image)

Full table: `mtd5/update.modules.txt` (golden: `mtd5/golden.modules.txt`).

| | A (16 channels, ref ch26) | B (16 channels, ref ch21) |
|---|---|---|
| template elements | 921 (520 LUT, 392 FF, 6 CARRY4, 2 IOBUF, 1 RAMB18) | 1,279 (748 LUT, 520 FF, 8 CARRY4, 2 IOBUF, 1 RAMB18) |
| input ports (of them shared by all channels) | 222 (28) | 141 (54) |
| template elements present per instance | median 92% (min 76%, ch01) | median 96% (min 91%) |
| **pure** (present, connections as in the template, every slot regenerated) | median 75.8% (68.5-94.9%) | median 85.5% (78.5-95.7%) |
| deviations, summed over the 16 instances: missing / function / connection / extra / replica | 1,385 / 83 / 1,036 / 2,495 / 32 | 819 / 42 / 1,058 / 1,951 / 52 |

The reference instance itself is 95% pure: the rest are its own extra nodes
(below the family threshold) sharing slices with template elements.

Lines (133,986 cell FASM lines in the update image):

| bucket | lines |
|---|---|
| regenerated from the templates (incl. 543 default-rule lines) | 48,647 |
| per-instance packing choices (FFMUX/OUTMUX/CY0) | 19,556 |
| per-instance deviations, raw | 7,577 |
| remainder (not channel logic, or slots shared with other logic) | 58,206 |
| pip lines: in instance nets / in the remainder | 179,107 / 145,600 |

Generated slots by kind: LUT 14,554, FF 11,078, OUTMUX 5,623, CTRL 4,268,
5FF 2,809, SITE 274, CARRY4 214.

What the deviations are, as far as checked:

- **extra** (about 120-190 per instance): mostly LUT1 route-throughs and
  buffers Vivado inserted in some instances and not others, plus logic
  layer 5 absorbed into a channel that belongs to a neighbour (layer 5 shows
  410 channel-to-channel connections) (I for the breakdown).
- **missing**: the counterpart sits outside the instance's layer-5 block
  (often in `core/i2c_glue`) or was merged by the optimizer; ch01 (the
  channel whose BRAM layer 5 did not attach) misses 224 of 921.
- **connection**: an input fed by a different net than in the template,
  typically because the driver is an element that is itself missing or
  extra.

## The two families

`template.json` maps B elements to A elements (reference ch21 aligned to
ch26; same label): **776 elements are common** (399 LUT, 370 FF, 4 CARRY4,
both pads, the BRAM); 503 are only in B (349 LUT, 150 FF, 4 CARRY4), 145
only in A (121 LUT, 22 FF, 2 CARRY4).

What the layer-7 analysis (`CHANNEL.md`) shows about the difference:

- **The I2C engine is the same** (V on the common part): the bit-level FSM
  (6-bit phase register and its four commands START, STOP, bit-0, bit-1), the
  tick register, the pad drivers, most of the command registers
  `cb+0x50..0x5C` and the BRAM are common elements.
- **The register decode sits on different sides of the boundary** (V):
  family B channels receive raw address information from the shared core
  (`sel_page` = "a[19:8] is my page" and `sel_page & a[k]` for k = 2..7)
  and decode the register offset themselves; family A channels receive
  finished per-register strobes (`== cb+0x50`, `0x54`, `0x58`, `0x5C`,
  `0x68`, `0x00`) decoded in the core.
- **The SCL divider and the profile register are inside the B template but
  partly outside the A template** (V): the B template contains the 16-bit
  divider chain and all 24 profile bits; in A the equivalent divider (same
  reload/enable structure, loaded from a profile-select mux) has the low end
  of its carry chain outside the instance (its CARRY4 `CI` is a port), and
  only 3 profile bits were located inside.
- **The split is not a function of the channel number** (V): decoding each
  instance's page gives family A = driver channels 0-9, 21, 27-31 and family
  B = 10-20, 22-26. It is identical in the golden image (same 16 pad pairs,
  layer 5), so it is a deterministic synthesis/optimisation choice, not
  place-and-route noise (I).

Conclusion (I): one RTL module, two netlist shapes. The difference is where
synthesis put the shared decode and register logic relative to the channel
boundary (and therefore what layer 5's membership caught), not a different
I2C function. Not proven: no formal equivalence check of the two templates
was run.

## Verification

Fresh run in this session (`roundtrip6.sh`, layers 1-6 from the flash dump;
log `mtd5/roundtrip6-mtd5.txt`):

`./roundtrip6.sh` (15 min 16 s): `roundtrip.sh` (layers 1-3) and `roundtrip4.sh`
(layers 4/5) re-ran from the image and checked themselves, then for each of
the two images in the flash:

| image | families (reference) | template elements A / B, correspondence | generated lines | netlist rebuilt from l6 | FASM | frames |
|---|---|---|---|---|---|---|
| golden | A (ch01), B (ch18) | 902 / 1,277, 759 common | 47,930 + 534 default | IDENTICAL | `cmp` equal | `cmp` equal |
| update | A (ch26), B (ch21) | 921 / 1,279, 776 common | 48,104 + 543 default | IDENTICAL | `cmp` equal | `cmp` equal |

    BYTE-IDENTICAL: .../mtd5-i2cfpga.bin == .../work/rt6-mtd5/rebuilt.bin (33554432 bytes)           (layers 1-3)
    BYTE-IDENTICAL (layer 4/5): .../mtd5-i2cfpga.bin == .../work/rt6-mtd5/rebuilt4.bin (33554432 bytes)
    BYTE-IDENTICAL (layer 6): .../mtd5-i2cfpga.bin == .../work/rt6-mtd5/rebuilt6.bin (33554432 bytes)

Checked again by hand after the run: `cmp images/spi-2026-11-09/mtd5-i2cfpga.bin
fpga/work/rt6-mtd5/rebuilt6.bin` prints nothing (exit 0); SHA-256 of both
`fa559a0276e3a3391c0a...`. The family assignment is the same in both images
(16 / 16); the golden image's references differ (ch01, ch18) because the
medoid is recomputed per image. The vendor file
(`i2c_fpga_v01.02.0E_20200604.bin`) was **not** run through layer 6 in this
session.

## Limits

- The template is only as good as the matching: about 5-25% of each
  instance is stored as explicit deviations, and part of the "extra" logic is
  layer-5 misassignment rather than a real per-instance difference. Lossless
  either way.
- Family A's template boundary cuts through its divider and profile
  register (see above); a better membership (grow each family's template
  into `core/i2c_glue` with the exclusive-claim rule) would make A and B
  closer.
- Ports are design nets, not yet named signals of the core (layer 7 names
  the ones it understands).
- Only the I2C channels are re-rolled. Sideband IO, the per-channel slices
  of the shared register file and the PCIe buffer BRAM groups were not done.
