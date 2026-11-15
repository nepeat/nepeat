# QSFP28 module control lines — 2026-11-15

Research only: nothing here has been read or written on the switch yet.
All 32 cages' ResetL, IntL and ModPrsL lines sit in **CPLD#1**. **No LPMODE
register was found** in any vendor or sibling source. The test plan is at the
end.

## where

| device | bus (vendor "bus 2" = `i2c@119000`, cell-index 2) | PCA9548 0x76 channel | addr | QSFP role |
| --- | --- | --- | --- | --- |
| CPLD#1 | i2c@119000 | 2 | **0x60** | reset, interrupt, presence (all 32 ports) |
| CPLD#2 | i2c@119000 | 3 | 0x62 | none found (version at 0x01 only) |
| CPLD#3 | i2c@119000 | 4 | 0x64 | none found (version at 0x01 only) |
| FAN CPLD | i2c@119000 | 0 | 0x66 | see [thermal.md](thermal.md) |

This is the same tree as the x86 **AS7716-32X** (0x76 ch0 fan 0x66, ch1 LM75s,
ch2 cpld1 0x60, ch3 0x62, ch4 0x64; SONiC `accton_as7716_util.py`) and the
AS7712-32X. On both of those, CPLD2/3 are bound as plain CPLDs with no port
attributes. Module EEPROMs are on the other controller (`i2c@119100`, muxes
0x72–0x75). That mapping lives in `nix/packages/as7760-qsfp/qsfp.py`.

Under NixOS the 0x76 mux belongs to the kernel (`i2c-mux-idle-disconnect`,
[device tree](../../nix/packages/linux-qoriq-t2080/boards/as7760/as7760-vendor.dts)).
Reach CPLD#1 through the channel-2 child adapter, and never by writing the
mux register from userspace (the fan and LM75 drivers share that mux). No
kernel driver binds 0x60; userspace readers are as7760-qsfp and, under SONiC,
the platform package's psud/xcvrd (combined transfers; [sonic-platform.md](sonic-platform.md)).

## hardware results (2026-11-15, `as7760-qsfp --status/--reset/--lpmode`)

- Read-only first: CPLD#1 is `i2c-6` (0x76 channel 2, created by the mux
  driver without a DT node), model ID reg 0x00 = 0x03, version 0x09; CPLD#2/#3
  version 0x08. ModPrsL 0x30 = 0xfe, 0x33 = 0x7f: ports 1 and 32 present,
  exactly what the EEPROM reader finds. ResetL 0x04-0x07 = 0xff (all running),
  IntL 0x10-0x13 = 0xff. Bit order and polarity confirmed.
- `--reset 1` (passive DAC, lab loop only): pulse done read-modify-write,
  register reads back 0xff, module EEPROM readable right after. IntL never
  asserted, so latched vs level is still unknown (passive copper has no IntL
  source).
- `--lpmode 1 on|off`: the passive DAC keeps byte 93 at 0x00; the tool reports
  the readback mismatch as an error. **Software low power is untested on an
  active module**; needs an optic/AOC.

## CPLD#1 register map

Port p is 1..32, using front-panel numbering (`sfpN` in the vendor device tree).
Each 4-byte group uses **reg = base + (p-1)/8, bit = (p-1)%8**: port 1 is
base bit 0 and port 32 is base+3 bit 7. Every line is **active low**.

| reg | meaning | access | polarity / notes |
| --- | --- | --- | --- |
| 0x00 | bits 3:2 = model ID | R | `CPLD_GetModelID` |
| 0x01 | CPLD code version | R | read on all four CPLDs by `do_cpld_code_version` |
| 0x02 | PSU status bits | R | `CPLD_GetPowerModuleStatus`; the diag kernel's PSU driver reads it too |
| **0x04–0x07** | **ResetL** | **RW** | **0 = held in reset, 1 = running**. Expected power-on value 0xff (see defaults) |
| **0x10–0x13** | **IntL** | R (assumed) | 0 = module asserts IntL. Level or latched: unknown |
| **0x30–0x33** | **ModPrsL** | R (assumed) | **0 = module present** |
| 0x41 | front-panel/system LEDs | RW | the diag kernel's LED driver does read-modify-write. Leave it alone |
| — | LPMode, ModSelL | — | **no register in any source**, see below |

"Assumed" read-only means nothing anywhere writes these registers. Writing
to them has not been tried and should not be.

### LPMODE

None of the vendor diag tools, the diag kernel or the public AS7712/AS7716
drivers controls an LPMode pin. The siblings' SONiC `sfputil.py`
(as7712, as7716) does low-power mode **in software**, through SFF-8636 byte 93
(Power_override bit 0 + Power_set bit 1: 0x3 = low power, 0x1 = high power).
How the pin is strapped on this board is unknown. Test 2 below shows its
effective state. The same goes for ModSelL: with one mux channel per
cage it is most likely strapped asserted, but nothing confirms that.

### power-on defaults

- ResetL: **released (1s), inferred, not read yet.** Nothing on the vendor
  side writes 0x04–0x07 at boot: the diag kernel has no such caller, and
  `cpldutil` writes only inside its manual test. Our U-Boot and NixOS never
  touch CPLD#1, yet module EEPROMs answer and `as7760-metrics` exports optics
  values. Test 1 confirms it (expect 0xff ×4).
- IntL / ModPrsL follow the cages.
- CPLD registers are presumed volatile, so a power cycle restores them. This
  is an inference, not a test.

## evidence

Vendor files are under ignored `nixos-build/`. Addresses below are for the
binaries named.

- **`diag-root/usr/sbin/cpldutil`** (PPC32, symbols + DWARF):
  - `cpld_device_tbl` @0x10015138: four 216-byte records. Index 0 =
    `CPLD#1` (bus 2, mux 0x76 ch 2, 0x60), 1 = `CPLD#2` (ch 3, 0x62),
    2 = `CPLD#3` (ch 4, 0x64), 3 = `FAN CPLD` (ch 0, 0x66).
    `CPLD_read`/`CPLD_write` take that index.
  - `do_cpld_transceiver` @0x10001d84 (`cpldutil -trans <first> [count]`,
    run by the diag recipe `usr/bin/qsfp-int`, "Reset and INT Test"):
    1. writes FAN CPLD 0x33 = 0
    2. builds four bytes that start at 0xff and clears bit (p-1)%8 of byte
       (p-1)/8 for each tested port
    3. **writes them whole** to CPLD#1 0x04..0x07, with no read-modify-write
    4. sleeps 1 s
    5. reads 0x10+i ("INT[i]") and 0x30+i ("present[i]")
    6. counts a failure for every tested port whose INT bit is **1** ("No
       interrupt") or whose present bit is **1** ("Not present")
    7. sleeps 1 s and writes **0xff to 0x04..0x07**

    So 0 = reset asserted, 0 = present, 0 = interrupt. The test expects IntL
    low while ResetL is held low. That only makes sense with manufacturing
    loopback plugs that tie ResetL to IntL (an interpretation, not
    confirmed).
  - `CPLD_GetModelID` @0x10000f90 reads 0x00 and keeps bits 3:2.
    `do_cpld_code_version` @0x10001904 reads 0x01 on every CPLD.
    `CPLD_GetPowerModuleStatus` @0x1000100c reads 0x02.
- **`diag-root/usr/bin/i2c_map-T2080-i2c3.csv`**: the same bus/mux/channel/address
  rows for CPLD1–3, the fan CPLD, the LM75s and the board EEPROM.
- **Diag device tree** `diag-part2` (dtc): `accton_i2c_cpld` nodes at 0x60/0x62/0x64
  under 0x76 ch 2/3/4. The `as7700_32x_sfpN` nodes sit on the same 0x72–0x75
  channels as `qsfp.py`'s `FACTORY_BUS`.
- **Diag kernel** `diag-part0` (gunzip; Linux 3.12.19-rt30, ppc64 big-endian,
  VMA = 0xc000000000000000 + file offset, disassembled with the cross objdump
  as a raw binary):
  - The `as7700_32x_sfp` driver's update routine @…324fe8 calls
    `accton_i2c_cpld_read(0x60, 0x30+i)` for i = 0..3. It packs the results
    into one word (reg 0x30 is bits 0–7) and refreshes every HZ+HZ/2 (375
    jiffies).
  - `sfp_is_present` @…3251d8 returns the inverted bit of that word at the
    port index.
  - The i2c_device_id table at file offset 0x7722d0 maps `as7700_32x_sfpN`
    to index N-1.
  - The whole kernel has five callers of `accton_i2c_cpld_read`/`_write`
    (@…52c75c / @…52c834): this presence read, the PSU driver (0x60/0x02,
    @…52b7f4) and the LED driver (0x60/0x41, read and write, @…57e05c and
    @…57e344–e398). **No kernel code writes ResetL or anything QSFP-related.**
  - The structure and the 1.5 s cache match SONiC's
    `as7716-32x/modules/accton_as7716_32x_sfp.c`.
- **Public sibling code** (sonic-buildimage `platform/broadcom/sonic-platform-modules-accton/`):
  - `as7716-32x/modules/accton_as7716_32x_cpld1.c`: `module_present_N`
    = 0x30–0x33, `module_reset_N` = 0x04–0x07, `version` = 0x01, same bit
    order, both active low, reset read-modify-write under a mutex.
  - `as7712-32x/modules/accton_as7712_32x_sfp.c`: CPLD 0x60 present
    0x30–0x33 and reset 0x04–0x07. Its comment says CPLD 0 = reset state and
    1 = normal. The file also has a commented-out AS6712 block that writes
    0x62/0x64 regs 4/5. **That is a different board's layout; don't copy it.**
  - `as7712-32x/modules/accton_as7712_32x_fan.c`: writes fan CPLD **0x33 = 0
    to disable the fan speed watchdog**. That explains cpldutil's 0x33 write
    (still listed as unknown in [thermal.md](thermal.md)).
  - ONL `as7712-32x`/`as7716-32x` `sfpi.c` use presence only.

## don'ts (for a driver and for manual tests)

- **Always read-modify-write 0x04–0x07** under one lock. Each register covers
  8 cages, and a whole-byte write (what `cpldutil -trans` does) resets 8 modules
  and drops their links.
- Never write CPLD#1 registers outside 0x04–0x07. 0x41 belongs to the LEDs,
  and everything unmapped could be board-level control.
- Don't write fan CPLD 0x33 from QSFP code (fan watchdog).
- Before asserting reset or low power on a port, ask the owner which front
  ports are cabled. A port carrying the uplink or lab traffic will flap.
- Hold reset at least 10 µs (SFF-8679). Allow up to 2 s (t_init) before
  touching the module again.

## driver shape (proposal)

One I2C client at 0x76 ch2 / 0x60 (new compatible, e.g. `accton,as7760-cpld1`).
It would expose per-port `present` (RO), `interrupt` (RO) and `reset` (RW,
read-modify-write, 1 = in reset), or the same lines as a 96-line gpiochip
(ModPrsL/IntL inputs, ResetL outputs). Low-power mode stays in software
(byte 93), as on the siblings, unless test 2 shows a hardware pin is needed.
Probe is read-only.

## safe hardware test plan

Over ssh on NixOS (`i2c-tools` is in the appliance profile). Save the
transcript as `qsfp-control-<date>.txt`. Find the bus numbers first:

```sh
P=$(for a in /sys/bus/i2c/devices/i2c-*; do
      case $(readlink -f $a/of_node) in *i2c@119000) [ -e $a/mux_device ] || basename $a;; esac
    done | sed 's/i2c-//')
C=$(basename $(readlink -f /sys/bus/i2c/devices/$P-0076/channel-2) | sed 's/i2c-//')
```

**1. Reads only (no writes at all).**
`for r in 0x00 0x01 0x02 0x04 0x05 0x06 0x07 0x10 0x11 0x12 0x13 0x30 0x31 0x32 0x33; do i2cget -y $C 0x60 $r; done`,
and `i2cget -y <ch3/ch4 bus> 0x62|0x64 0x01`. Gates:

- reset regs = 0xff
- the zero bits of 0x30–0x33 match exactly the ports `as7760-qsfp` lists as
  populated
- note the INT bits per populated port

Any mismatch: stop and document. An `i2cdump` of 0x60 is optional (some CPLDs
clear latched status on read). Its only use would be to look for an
undocumented LPMode group. Don't write to anything it finds.

**2. One LPMODE toggle, one port** (software LPMode, SFF-8636 byte 93):

1. **Pick the port.** The owner picks a port holding an optical or AOC module
   that carries no traffic. A passive DAC shows nothing.
2. **Baseline.** Find its EEPROM bus with `bus_for(p)` from `qsfp.py`.
   Record byte 93 (`i2cget -y -f $B 0x50 93`) and `as7760-qsfp <p>`
   (tx_bias, tx_power).
   - If byte 93 = 0 and tx bias is non-zero, the LPMode pin is already low
     (high power allowed). That is the strap answer.
3. **Low power.** `i2cset -y -f $B 0x50 93 $((orig | 0x03))`, wait 2 s, read
   back. Expect byte 93 to read back and TX bias/power to drop to ~0.
4. **Restore.** `i2cset -y -f $B 0x50 93 $orig`, wait 2 s. Expect the baseline
   values back, and the CPLD#1 registers unchanged from test 1.
5. **If it doesn't restore:** write `orig` again. If that fails, reseat the
   module (its EEPROM settings are volatile).

**3. Later, separate approval: one ResetL pulse on the same port.**

1. Read 0x04+(p-1)/8 and clear only bit (p-1)%8 (read-modify-write).
2. Wait 100 ms, then set the bit again.
3. Check that the register is back to its test-1 value and the EEPROM answers
   within 2 s.
4. Watch the port's INT bit through the pulse. It shows whether 0x10–0x13 is
   a live level or latched.

Recovery for anything odd: write the saved byte back, else power-cycle the
switch (CPLD defaults, inferred).
