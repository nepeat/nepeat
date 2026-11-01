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
