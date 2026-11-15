# as7760-metrics

Textfile producer for the AS7760-32X: optics, fans and board temperatures
that node_exporter does not label. Wired in by
[modules/boards/as7760/monitoring.nix](../../modules/boards/as7760/monitoring.nix)
(every 30 s, timeout 20 s, as root for `/dev/i2c-N`), so it is part of
`as7760-system` and appears on `http://<switch>:9100/metrics`. Run by hand:
`as7760-metrics` (prints the same text).

Names are board-neutral (`switch_*`) so dashboards can be shared across
boards; the board is in `switch_board_info`. Units are base units
(PromQL for dBm: `10 * log10(switch_optic_rx_power_watts * 1000)`).

| metric | labels | meaning |
| --- | --- | --- |
| `switch_board_info` | vendor, board, asic | always 1 |
| `switch_optic_present` | port (1..32) | 1 if the cage's EEPROM (0x50) answers |
| `switch_optic_info` | port, type, vendor, part, rev, compliance, media | always 1, present ports only |
| `switch_optic_temperature_celsius` | port | module temperature (QSFP; SFP if DDM) |
| `switch_optic_supply_voltage_volts` | port | module Vcc |
| `switch_optic_rx_power_watts` | port, lane | per-lane RX power, active optics only |
| `switch_optic_tx_bias_amperes` | port, lane | per-lane laser bias, active optics only |
| `switch_optic_tx_power_watts` | port, lane | per-lane TX power, if the module monitors it |
| `switch_optic_scrape_success` | | 0 if the module I2C adapter could not be found |
| `switch_fan_controller_present` | | 1 if the `as7760_fan` hwmon is bound |
| `switch_fan_pwm` | | shared PWM read back from the fan CPLD, 0..255 |
| `switch_fan_duty_percent` | | the same as percent |
| `switch_fan_rpm` | fan (1..6), rotor (front/rear) | rotor speed |
| `switch_fan_fault` | fan, rotor | 1 if the module is absent or the rotor reads 0 |
| `switch_fan_control_active` | | 1 if `as7760-thermal.service` is active |
| `switch_fan_control_quiet_duty_percent` | | configured `hardware.as7760Fan.duty` |
| `switch_fan_full_speed_forced` | | 1 if duty is above the quiet duty or the service is stopped (guard tripped: hot, sensor/fan error, first minute after start) |
| `switch_board_temperature_celsius` | sensor (board_1, board_2, board_3, cpu_board) | the four LM75s, sensors.d labels |
| `textfile_producer_last_success_timestamp_seconds`, `textfile_producer_duration_seconds` | producer="as7760" | added by the producer wrapper |

Not exported: PSU state (no register or PMBus address known yet; TODO in
metrics.py), SoC/ASIC die temperature (the T2080 kernel has no thermal zone;
`cpu_board` is a board LM75 near the CPU, not the die).

Cost: each run reads 64 bytes of the lower page per cage (one I2C
transaction; empty cages NACK at once), plus A2h for SFPs with DDM. Vendor
/part fields (page-select write + 128-byte upper page) are cached in
`/run/prometheus-textfile-as7760/optics-static.json` and re-read when a module
appears, its identifier changes, it was not ready, or after 10 minutes. A
module swapped inside one 30 s interval keeps the old identity labels until
the next refresh. Fans and temperatures come from hwmon sysfs (the kernel
drivers do the I2C).

Tests (native, offline): `nix build path:.#as7760-metrics-test`: fake EEPROM
images (QSFP28 SR4, passive DAC, SFP with DDM) behind a fake I2C layer, the
cache (second run touches no upper page), `as7760-qsfp`'s JSON shape, a fake
hwmon tree, and `promtool check metrics` (strict lint) on the output.
Shared-decoder changes in `../as7760-qsfp/qsfp.py` are additive: new
`tx_power_mw` (QSFP, if supported) and SFP DDM fields in `--json`.

```sh
curl -s http://<switch>:9100/metrics | grep '^switch_'
curl -s http://<switch>:9100/metrics | grep -E '^switch_optic_(present|temperature)' | grep -v ' 0$'
```
