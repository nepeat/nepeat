# dmc12-metrics

Textfile producer for the Edgecore DMC-12-48R-AS0 console server: the board
CPLD (fans, PSUs, alarms, watchdog flag), labelled board temperatures and the
48 serial ports. Wired in by
[modules/boards/dmc12/monitoring.nix](../../modules/boards/dmc12/monitoring.nix)
(every 30 s, timeout 20 s, as root for `/dev/i2c-4` and `/proc/*/fd`), next to
the upstream node_exporter on `:9100`. Run by hand: `dmc12-metrics`.

**Read-only.** Eleven SMBus byte reads of the CPLD (I2C bus 4 / 0x68,
`smbus2`), hwmon and sysfs reads, `readlink` of `/dev/P*` and `/proc/*/fd`.
It never writes a CPLD register (fan control owns 0x50 and 0x2a,
`dmc12-interruptmon` owns the mask 0x21) and never opens a serial port (that
would toggle DTR/RTS on the attached console). Register map:
`devices/dmc-12-48r-as0/vendor-tooling.md`.

| metric | labels | meaning |
| --- | --- | --- |
| `dmc12_board_info` | vendor, board, model | always 1; model = EEPROM ProductName from `/run/dmc12/eeprom.json` (no serial/MAC) |
| `dmc12_cpld_scrape_success` | | 0 if the CPLD did not answer (then no other `dmc12_cpld_*`/fan/PSU families) |
| `dmc12_cpld_version` | | 0x01; above 134 = v7+, 64 PWM steps (u01: 136) |
| `dmc12_cpld_register` | register (0x11 0x17 0x18 0x20 0x21) | raw power, LED, uplink PHY state, interrupt status and mask |
| `dmc12_fan_pwm_step` | | 0x50, the one PWM register for all fans |
| `dmc12_fan_duty_percent` | | duty for that step (fand's map: v7+ 4.6 % at step 1 to 100 % at 63) |
| `dmc12_fan_rpm` | fan (1..`--fans`) | tach 0x60..0x62 × 100 |
| `dmc12_cpld_wdt_flag` | | 0x2a raw (3 after a reset, 1 once fan control took over) |
| `dmc12_fan_watchdog_failsafe` | | 0x2a bit 1: the CPLD forces full speed |
| `dmc12_psu_present` | psu (1, 2) | 0x11 bits 3/4 |
| `dmc12_dc_input_present` | | 0x11 bit 5, 12 V DC pass-through (PSU alarms then mean nothing) |
| `dmc12_cpld_alarm` | alarm | 1 if the 0x20 bit is asserted (active low), masked or not: `tmp75_alert_1/2`, `fan1..3_failure`, `uplink_change`, `psu1/2_failure` |
| `dmc12_cpld_alarm_masked` | alarm | 0x21 bit (interruptmon masks a handled alarm for 60 s) |
| `dmc12_temperature_celsius` | sensor (`k2_uc`, `sfp_console`, `m2`) | lm75 0x4a / tmp75 0x48 / 0x49 on any bus, the sensors.d labels (node_exporter's hwmon has the same values without names) |
| `dmc12_serial_port_present` | port (1..48) | `/dev/P<port>` resolves to a tty |
| `dmc12_serial_port_info` | port, tty, usb | always 1; usb = CP2108 interface `hub.port:iface` |
| `dmc12_serial_port_open_processes` | port | processes holding the tty open |
| `dmc12_serial_open_scan_complete` | | 0 if some `/proc/<pid>/fd` was unreadable (counts may be low) |
| `textfile_producer_*` | producer="dmc12" | added by the producer wrapper |

Not here: SoC die temperature (node_exporter's `thermal_zone`, once the kernel
has `al-thermal`), INA219 input power (raw writes in `show power`; would need
the `ina2xx` hwmon driver), unit states (node_exporter `systemd`). fand, when it
is the controller, exports `fand_*` itself.

Assumed, not yet checked on hardware: reading 0x20 does not clear a latched
bit (u01 showed the PSU1 bit steady; interruptmon re-runs `setphyspeed` every
15 s anyway); 0x2a bit 1 is the failsafe bit (fand's `arm` treats it so).

Tests (native, offline): `nix build path:.#dmc12-metrics-test`: a fake
`smbus2` that fails on anything but `read_byte_data`, u01's CPLD dump, fake
hwmon, `/dev`, `/sys` and `/proc` trees (missing port, unreadable fd table, a
DN0 with two fans, CPLD absent), and `promtool check metrics`.
`--cpld-regs FILE` replays a dump such as `inventory/u01/cpld-regs.txt`.

```sh
curl -s http://<dmc12>:9100/metrics | grep '^dmc12_'
```
