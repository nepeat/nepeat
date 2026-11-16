# FPGA/CPLD reverse engineering playbook

From an opaque configuration image to (1) representations that rebuild the vendor bytes exactly,
(2) readable RTL proven to be the same logic, and (3) a bug list with fix routes. Distilled from
two runs: the CS8210 I2C FPGA (Xilinx 7-series; `devices/cs8210-32x/fpga/README.md`, `LAYER*.md`)
and the DMC-12 board CPLD (Lattice MachXO3; `devices/dmc-12-48r-as0/cpld.md`, `tools/machxo3/`).

The one rule: **every layer must rebuild the original image byte for byte before the next layer
is built on it.** Readability is added in steps; byte identity is never traded for it. Only the
last layer (the readable view) is allowed to be lossy, and it must be proven *equivalent* instead.

## 0. Acquire the image without hurting the board

CPLDs on boards usually own power, reset, fans and alarms. Treat their config port like a flash
write path (`switch-porting-runbook.md` rules apply).

1. **Identify the part read-only** (IDCODE, USERCODE, status) before anything else; stop if the
   IDCODE isn't the one expected.
2. **Find the vendor's updater** (`cpldupd`, ispVM `.vme` player, a sysfs driver, U-Boot commands)
   and disassemble its *read/verify* path: the opcodes, framing (e.g. I2C repeated start, 3-byte
   commands), busy polling and mode it uses on a live board. Copy that path, minus every write.
3. **Use the background/transparent mode** (Lattice `ISC_ENABLE_X 0x08`; Xilinx readback via
   JTAG/ICAP) so the user logic keeps running. Never: erase, program, done, refresh/reload,
   offline mode, security/OTP. A refresh reloads the chip that runs the board.
4. **Allowlist tool**: every byte sequence sent is matched exactly against a table; `--dry-run`
   prints the transactions; an `--id-only` stage comes first. Watch the board's own health
   (fan duty, watchdog flag, the services that talk to the chip) before and after.
5. **Read twice; the hashes must match.** Check the security bit: a locked device reads blank.
6. Other sources: update files in the vendor OS/ONIE (`.vme`, `.jed`, `.bit`, `.mcs`), the SPI
   flash the FPGA boots from, release notes. Decode them too: they give versions to diff against.
7. **Records**: the image and everything that reproduces the vendor design (decoded config,
   netlists, RTL, sims) live in git-ignored `dumps/<unit>/<chip>/`. Tracked: generic tools (no
   design data embedded) and one topic doc with findings, hashes and confidence tags.

## 1. The ladder

Each layer = a tool pair (decode, encode) + a `roundtripN` script that runs all layers up to N
from the original image and `cmp`s the result with it, printing both sha256. Record the result in
the topic doc.

| L | representation | what it adds | gotchas seen |
|---|---|---|---|
| 1 | **packets** | image <-> line-based listing of every command/register write; payloads as blobs | preamble/dummy bytes outside the CRC; vendor-only commands (MachXO3 `0x72`): keep verbatim |
| 2 | **frames** | the configuration memory by frame address; an encoder that re-packs *exactly as the vendor tool did* | compression must be canonical (Diamond: always the shortest code, so unique); multi-frame writes, golden/update images; CRC conventions; ECC quirks (Vivado computes ECC with SRL/LUTRAM INIT zeroed; GTP frames store 0) |
| 3 | **database features + unknown bits** | frames <-> FASM / Trellis text config, with bits the database doesn't explain kept in a separate list | database gaps are normal (CS8210: 3 bits; DMC-12: 128): keep them, explain later |
| 4 | **lossless netlist** | cells (sites/BELs + their features) and nets (driver -> sinks + the pips that realise them); every config bit owned by a cell, a net or an explicit `unassigned` bucket; netlist -> features must be identical | routing models miss wires (MachXO3: edge and secondary clock wires): name them yourself and say it's inferred; LUT fracturing, inverted site naming, zero-bit features |
| 5 | **blocks** | every cell/net tagged with a block path (`i2c_slave`, `regfile`, `pwm`, `pcie/*`, `io/*`) with evidence; the split netlist rebuilds identically | start from pads and hard blocks, follow the cones |
| 6 | **templates** | replicated structures re-rolled into one module + instances (placement, port bindings, routing) + recorded deviations; regenerated features identical | synthesis inlines and optimises each copy differently: grow membership, record "extra/missing/connection" deviations, accept 2 families if that's the truth |
| 7 | **readable RTL** (lossy view) | human-named Verilog: register decode, FSMs, counters, state tables | must be **proven equivalent** to L4 (formal or exhaustive co-sim), not just "looks right" |

Layer discipline:
- Never adjust a lower layer to make a higher one work; fix the higher one. Unknowns are carried
  verbatim, never dropped.
- Keep decoders deterministic and the tools per layer separate, so a later layer can't silently
  break an earlier one (rerun all roundtrips after any tool change).
- Tag findings **V/proven** (checked byte for byte, formally or by simulation of the decoded
  netlist), **I/inferred** (structure), **open**.

## 2. Verification techniques

- **Byte round trip** (L1-L6): `cmp` + sha256 against the original, from a fresh run.
- **Netlist identity**: a canonical form of the netlist compared before and after a transform.
- **Formal equivalence** (L7 vs L4): yosys `equiv_make` / `equiv_simple` / `equiv_induct`, or a
  miter + `sat -tempinduct`; state the coverage (which outputs, with which state mapping).
- **Co-simulation**: Verilator/iverilog of the decoded structural netlist with device models
  (an I2C slave on a cage, PCIe TLPs, a tach input) and directed + random stimulus; compare
  replicated instances against each other.
- **Against the board**: compare simulated reset values with a live register snapshot. Live reads
  only for registers proven side-effect-free from the decoded logic (clear-on-read latches!).
- **Against the vendor software**: every register the drivers/tools touch must have logic behind
  it with matching bits.
- Slow timers (debounce at ~1 kHz, watchdogs): force internal counters in sim rather than waiting.

## 3. Understanding the design

Order that worked: pads and hard blocks (IO standards, pulls, the EFB/PCIe/oscillator) -> the host
interface (I2C/SPI slave, PCIe BARs) -> address decode -> register file (every bit's source,
sink, reset value, write and read side effects) -> per-function blocks (PWM, tach, watchdog,
reset sequencing, interrupts, LEDs, downstream buses) -> board signals (match register bits to
pins, pins to the vendor DT/schematic knowledge, behaviour to the vendor tools).

## 4. Bug-hunting checklist

- Asynchronous inputs (SCL/SDA, presence, alarms, reset) used without a synchroniser, especially
  fanning out into many flip-flops.
- Reset release not synchronised to the clock; resets or clocks generated in LUTs; latches opened
  by LUT outputs (glitches).
- PWM/timer values that take effect mid-period (runt pulses); clock-select changes that stretch
  or glitch a derived clock.
- Alarms that aren't latched (missed), status bits cleared on read (races with other readers).
- Power-up defaults and fail-safe behaviour (does the fan go to full speed with no host?).
- Host-interface robustness: stuck-bus recovery, clock stretching, pointer wrap, unimplemented
  addresses.
- Anything the vendor software works around (retries, delays, double reads) points at a bug.

For each finding: evidence (cells/nets), user-visible impact on this board, and route: fixable in
our software / only by reprogramming / not worth it.

## 5. Fixing: two routes

1. **Minimal patch at L4-L6**: edit the netlist (or one template), re-encode through the ladder.
   The new image differs from the vendor one only in the edited tiles, so the diff is reviewable,
   and everything else is the proven-identical base. Prove the change with formal/co-sim against
   the original (equal except where intended).
2. **Re-synthesis from L7** (yosys + nextpnr/Trellis, or the vendor tool): a maintainable source,
   but byte identity with the vendor image is gone; then L7's equivalence proof is what ties it to
   the original behaviour.

Programming is a separate, owner-approved step: the vendor-equivalent flow (copied like the read
path), background mode + the board's hitless/TransFR hold, a rehearsal on a spare unit, a known
recovery path (JTAG/clip, the vendor image to write back), never first on the in-service unit.

## 6. Tooling by family

| family | bit database / tools | ours |
|---|---|---|
| Lattice MachXO2/XO3, ECP5 | Project Trellis (MachXO3 db incl. LCMXO3-2100), nextpnr | `tools/machxo3/` (xo3bit: image <-> packets + frames, canonical compression, CRC; xo3cfg: frames <-> Trellis config; xo3net: netlist; xo3vlog: Verilog) |
| Lattice Nexus | prjoxide | - |
| Xilinx 7-series | Project X-Ray (prjxray-db, fasm2bels) | `devices/cs8210-32x/fpga/i2cfpga/` (bitasm, xc7frames, fasmconv, fasmnet, blocks, modules, channelrtl, equivcheck, simnet) |
| iCE40 / Gowin | IceStorm / Apicula | - |

When a part isn't in a database but a sibling die is, check whether the vendor maps them onto one
fabric (prjxray maps 35t onto 50t; the CS8210 75t onto 100t) before fuzzing anything.
