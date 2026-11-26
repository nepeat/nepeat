# board controls recovered offline

Sources: captured board DTBs, `root/driver/install.sh`, diagnostic YAML, and
ARM64 disassembly/DWARF of `sys_cpld.ko` and `fan_cpld.ko`. Setter paths were also
executed in Unicorn with memory and I2C calls replaced by local Python stubs.
No hardware transactions occurred. These are binary-behavior findings, not
measured electrical behavior or a validated replacement driver.

## stable mux paths

The board I2C controller is DesignWare at `0xfd880000`, SPI interrupt 21,
400-kHz bus and 380-ns SDA hold time in the packaged DT. Its source clock is
500 MHz. Addresses below are 7-bit I2C addresses; `addr:channel` means a mux
selection. Use physical topology rather than assuming vendor Linux bus numbers
will survive a mainline port.

All paths below start at that controller:

| Component | Mux chain | Address | Stock Linux reference |
| --- | --- | --- | --- |
| System EEPROM | none | `0x57` | Determine live adapter |
| RTC DS1339 | `0x70:3` | `0x68` | Determine live adapter |
| System CPLD | `0x70:2 → 0x74:0 → 0x71:2` | `0x60` | `8-0060` |
| Top fan CPLD | `0x70:2 → 0x74:2` | `0x66` | `31-0066` |
| Bottom fan CPLD | `0x70:2 → 0x74:3` | `0x66` | `40-0066` |
| Switch LED CPLD | `0x70:2 → 0x74:0 → 0x71:5` | `0x64` | `11-0064` |
| MACsec LED CPLD | `0x70:2 → 0x74:1 → 0x73:0` | `0x63` | `23-0063` |
| Switchboard left/center LM75 | `0x70:2 → 0x74:0 → 0x71:1` | `0x48`, `0x4b` | See live adapter map |
| Switchboard right LM75 | `0x70:2 → 0x74:0 → 0x72:7` | `0x49` | See live adapter map |
| Fanboard LM75 sensors | `0x70:2 → 0x74:2 or 3 → 0x73:0` | `0x4d..0x4f` | See live adapter map |
| 21-inch hotswap LTC4282 | `0x70:2 → 0x74:0 → 0x72:1` | `0x58` | Alternate DT only |

The full machine-readable inventory is `analysis/research/i2c-map.json`.
PCA9546/PCA9548 muxes, LM75 sensors and DS1339 RTC have ordinary upstream driver
families. Their integration and board-specific bindings still need validation.
Port EEPROM buses `101..132` are created through the custom acceleration FPGA
and are a separate issue from the main SoC I2C mux tree.

## identification and control registers

| Device/register | Recovered meaning | Confidence/limit |
| --- | --- | --- |
| System CPLD `0x01`, upper nibble | Board revision; zero selects legacy MDIO FPGA driver | Explicit stock install script |
| System CPLD `0x09`, bit 0 | Zero selects 21-inch/12-V diagnostic configuration | Explicit stock install script; physical variant unconfirmed |
| System CPLD `0x05`, bit 7 | `fan_max_speed`: input 1 clears bit; input 0 sets bit | Disassembly plus emulation; preserves other bits |
| System CPLD `0x05`, bit 5 | `fan2_max_speed`: input 1 clears bit; input 0 sets bit | Disassembly plus emulation; preserves other bits |
| System CPLD `0x1c` and `0x1d[3:0]` | System watchdog timeout = seconds × 20, accepted range 0..200 seconds | Disassembly plus emulation |
| System CPLD `0x22[1]`, `0x1d[5:4]` | System watchdog enable and trigger sequence | Multiple writes; do not collapse into one guessed write |
| Fan CPLD `0x0c[0]` | Fan watchdog enable | Disassembly plus emulation |
| Fan CPLD `0x0c[7:1]` | Timeout in 5-second units, floor(seconds/5), accepted 0..635 | Disassembly plus emulation; zero-timeout electrical behavior unknown |
| System CPLD `0x25` | Stock startup writes `0x02` to enable power cycle on reboot | Script comment and command; full bitfield not recovered |

The sys-CPLD enable setter first updates `0x22[1]`, clears `0x1d[5]`, then sets
or clears `0x1d[5:4]` according to the requested enable value. Disable also pulses
bit 5 high then low. Its register cache/read behavior matters when reproducing
this sequence. The timeout setter preserves the high nibble of `0x1d`.

Examples from emulation with every simulated register initially `0xa5`:

- `fan_max_speed=1`: register `0x05` becomes `0x25`.
- `fan2_max_speed=1`: register `0x05` becomes `0x85`.
- System watchdog 30 seconds: `0x1c=0x58`, `0x1d=0xa2` (600 ticks).
- Fan watchdog 30 seconds: `0x0c=0x0d` (six units, previous enable bit preserved).
- System timeout 201 / fan timeout 636: return `-EINVAL`, no simulated writes.

Reproduce with:

```sh
nix develop ./ai_re --command python3 ai_re/devices/cs8210-32x/scripts/trace_cpld.py
```

The JSON includes module hashes, input values, register write sequences and
return values: `analysis/research/cpld-setter-traces.json`. Full disassemblies
and selected DWARF types are adjacent. This script deliberately targets the
captured binary's offsets; it must be re-audited for another module version.

## PSUs (live, 2026-11-26)

- Two bays (sys CPLD 0x09 bit 0 = 1: the 19-inch variant), two Delta TDPS-1500AB-6 B. Status: sys CPLD 0x60 reg 0x08
  (present b4/b5 active low, input good b2/b3, output good b0/b1). PMBus 0x59 behind mux 0x72: ch1 = PSU1, ch0 = PSU2.
- Feeds: PSU1 = NetBotz 250 outlet `cs8210-psu1` (normal state On since 2026-11-26), PSU2 = lab plug 1.
- `cs8210-psu monitor` (rust/cs8210) is the one PMBus reader: PAGE is its only write; Vin/Iin/Pin, Vout/Iout/Pout of
  the 12 V rail (page 0, VOUT_MODE exponent -9) and the 3.3 V standby (page 1, -7), three temperatures, one fan,
  STATUS_WORD -> `switch_psu_*` + /run/cs8210-psu/psuN.json (sonic_platform's source). No kernel `pmbus` driver: its
  probe of PSU1 came out with exponent 0 (12 V shown as 6176 V) and a varying page count while raw reads were clean.
- **Total draw is the sum of the two Pin readings**: each PSU meters its own input and they share the load
  (idle, three loops up: ~349 + 343 W in, ~330 + 310 W out); one feed alone carries ~701 W in, ~642 W out. A PSU
  without AC still answers PMBus (standby from the other), STATUS_WORD 0x2849, Vin 0 ([power-20261126.txt](evidence/power-20261126.txt)).

## Fan CPLD register map (fan_cpld.ko 1.3.0, 2026-11-26)

Both fan boards: CPLD 0x66 (top = FAN8..14 = Linux bus 31, bottom = FAN1..7 = bus 40; chassis silkscreen FAN1..7 =
bottom slots 1..7, FAN8..14 = top slots 1..7, confirmed by pulls). From the driver's disassembly + DWARF names, checked
against the vendor cplds.yaml / leds.yaml / fans.yaml (no C source in driver.tar.xz). Slot n base = 0x20 + 0x10 (n-1).

| reg | vendor name | meaning |
| --- | --- | --- |
| base+0 / +1 | fanN_input | front / rear tach, raw x 150 rpm (driver; vendor fans.yaml NIDEC max 28000 / 31000 rpm vs our raw 189-191 / 207-211 at PWM 255) |
| base+2 | pwmN ("fan_ctrl_reg") | duty 0..255 for both rotors of the module; nothing else |
| base+4 | fanN_led | bits 1:0: 1 green, 2 red, 3 off (0 "undefined", maybe both = amber, untested); software only, no CPLD auto mode, nothing in the vendor stack drives it (a dead module stays green) |
| base+5 | fanN_eeprom_protect | module FRU EEPROM write protect |
| base+6 | fanN_present / direction | bit 0 = 1 absent (0 present), bit 1 = airflow (0 F2B) |
| (none) | fanN_fault | computed by the driver: absent or tach 0; no fault register |
| 0x00 | cpld_version | 0x03 here |
| 0x09 | fan_board_eeprom_protect | |
| 0x0c | watchdog | bit 0 enable, bits 7:1 seconds / 5 (above) |
| 0x17 | fan_mux_reset | fan-board PCA9548 reset (the driver writes the inverse of the sysfs value) |
| 0x19 | fan_eeprom_power | bit 0: module FRU EEPROM power (not the motors) |
| 0x10 | (unnamed) | bit n-1 set for exactly the slots that were hot-pulled; no vendor code reads, acks or clears it |
| 0x04 / 0x06 / 0x07 / 0x08 / 0x0f | yaml only | scratch / INT STA (bit 0) / INT PRT / HW rev / MISC STA (default 0x1f) |

- No per-slot power enable, reset or ack exists in the driver or the vendor userland (`cel-fan-test` only reads present
  / tach and writes PWM; `install.sh` arms the watchdogs; ONIE `disable_fcs.sh` writes PWM 127). The only larger lever:
  the sys CPLD fan CPLD resets (0x03 bit 3 `fan_reset`, 0x04 bit 5 `fan2_reset`, active low; which board is which: open).
- Reg 0x10 is a latched rotor-stall flag (bit n-1): the fan CPLD reset clears it and it comes back with the stall.
  `fan2_reset` (sys CPLD bus 8 0x60 reg 0x04 bit 5, active low) resets the BOTTOM fan CPLD (LEDs back to green, PWM
  0x3c / 0x4b until fand rewrites it); it revives nothing. 0x19 (FRU EEPROM power) does nothing for the motors.
- **Stalled rotors, 2026-11-26** ([fan-cpld-20261126.txt](evidence/fan-cpld-20261126.txt)): a module hot-swapped into a
  running chassis, or a rotor that stopped, does not start: the other fans' backdraft windmills it backwards (tach 0/0,
  "unpowered" to the eye). What starts it: every fan low together ("calm and restart": all 14 PWM 0x00 ~10 s, 0x30 ~5 s,
  then normal; the sys CPLD force-max (0x05) must be released first or every duty register reads and acts 0xff); 5 of 5
  stalled rotors came back. Low duty heats the ASIC fast (~2 min took it to 82.9 C): keep dips short and guarded. fand
  does this itself now (`stall_recovery`, guards ASIC 70 C / board 45 C, 3 attempts, then the module is isolated at
  duty 0, LED solid red; zones stay degraded on their curve). **Do not hot-swap fan modules while the fans run at full
  speed** (failsafe): the inserted module cannot start until fand's next dip.
- Two modules have a dead REAR rotor (it follows the module through slot swaps; no dip, reseat, CPLD reset or cold
  power cycle revives it): failed hardware, in FAN5 (bottom5) and FAN12 (top5) since the owner's FAN5 / FAN6 swap; FAN12's was the 10:38 stall.
  A third, FAN6 (bottom6) FRONT, stalls at every boot since 13:32 and no dip (fand's or 12 s + 6 s by hand) revives it.
  Three isolated modules = 6 rotors, more than `degraded_max_failed_rotors` 4: fand is in failsafe 100 % by policy.

## cooling implications for bring-up

Stock startup enables both fan watchdogs, disables the system watchdog, sets
both `fan*_max_speed` attributes to 0, and may start the diagnostic fan-control
process when ONIE tools are present. The YAML uses a temperature-dependent fan
policy and two fan-controller paths. The ordinary package describes 14 dual
fans/28 rotors; the 21-inch package uses 12 dual fans/24 rotors. These are
software descriptions, pending physical confirmation.

We now have a likely way to request maximum fans through the stock sysfs API,
but the labels and inverted bits are not sufficient to assert what the physical
fans will do across a new-kernel handoff. Confirm RPM response and watchdog
expiry behavior on stock software first. The serial-only kernel has no cooling
controller or watchdog service. Its omission of board drivers prevents their
initialization; it does not prove that existing CPLD timers stop running.

For first live collection, record the named sysfs values and RPM/temperature
telemetry, and identify mux topology from sysfs. Prefer those driver-backed
reads to an indiscriminate I2C scan. Retain current register values before any
later approved control experiments. No raw-write command sequence is being
presented as hardware-validated here.
