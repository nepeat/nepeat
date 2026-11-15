# sensors and fan control — 2026-11-15

NixOS has LM75 + I2C mux + hwmon support, a small AS7760 fan CPLD driver,
`sensors` and `as7760-fan`. Since 2026-11-15 fans are run by **fand**
(`rust/crates/fand`, `fand.service`): a temperature curve over the four board
sensors, at the 50% hardware floor when cool, 100% on any fault, on exit, or at
45 °C. ASIC die temperature is **not** an input yet. The CPLD itself powers up at
full speed (the state with no OS driver).

## evidence

The verified `dumps/mtd3.bin` factory diagnostic image is a legacy U-Boot
multi-image. Its second component contains the diagnostic root filesystem.
Regular files were extracted offline under ignored `nixos-build/diag-root/`.
The `cpldutil` and `i2ctmp` binaries contain symbols and device tables. These
provide board-specific evidence; the older public AS7710 ONIE patch alone did
not contain the fan driver.

`cpldutil` contains `cpld_device_tbl`, four 216-byte records. The fourth is
`FAN CPLD`: physical I2C bus 2, PCA9548 at 0x76 channel 0, device 0x66.
`i2c_select_device` confirms the table's channel number is converted to
`1 << channel`. `i2ctmp`'s `tsensor_device_tbl` places LM75_1, LM75_2, LM75_3
and LM75_CPU at 0x48–0x4b behind channel 1 of the same mux.

| fan CPLD register | recovered behavior |
| --- | --- |
| 0x0f | module presence bits 0–5; **zero means present** |
| 0x10 | airflow direction bitmap; retained as evidence, not used for control |
| 0x11 | common fan duty, 0–15 |
| 0x12–0x17 | front rotor tachometers, register × 100 rpm |
| 0x22–0x27 | rear rotor tachometers, register × 100 rpm |
| 0x33 | vendor `CPLD_SetFanSpeed` writes zero before writing duty |

`CPLD_SetFanSpeed` at 0x10001800 writes 0x33=0 and then
0x11=floor(percent*15/100). The diagnostic high/low scripts request 100/50%.
`CPLD_CheckFanSpeed` at 0x10001540 uses the double constant **100.0** at
0x100047e0. Presence string selection at 0x100013b0 maps a set bit to `No`.

The full meaning of 0x33, including any independent hardware watchdog or
automatic controller, is **not established**. We reproduce only the vendor's
documented-by-disassembly manual-duty sequence. The new driver rounds duty
up and rejects requests below 50%; it never intentionally stops a fan.

## live confirmation

See [the serial capture](thermal-uboot-20261114.txt). Through tmux/tio at the
existing U-Boot prompt, selected bus 2 and read the mux's original value 0.
Temporarily selected channel 0, then channel 1. Restored mux value 0 and bus 0.
**Only mux-selection writes were made; no fan, sensor configuration, flash,
EEPROM, or environment writes.**

- 0x0f = 0x00: six modules present.
- 0x10 = 0x3f; 0x11 = 0x0f: maximum duty.
- Front tachometers: d4 d8 d5 d4 d7 d6 → 21,200–21,600 rpm.
- Rear tachometers: b4 b4 b3 b4 b3 b4 → 17,900–18,000 rpm.
- 0x33 = 0x01 in firmware. Its exact mode semantics remain unknown.
- Temperature words: 1ac0, 1a80, 19e0, 1880: roughly 24.5–27 °C.
  They are LM75-compatible; the exact sensor variant/resolution is not yet
  identified. The initial Linux binding uses the conservative LM75 format.

## implementation and use

[Device tree](../../nix/packages/linux-qoriq-t2080/boards/as7760/as7760-vendor.dts) adds mux 0x76 beneath the actual
`/soc@ffe000000/i2c@119000` controller. Its kernel mux driver disconnects idle
channels. The fan and temperature drivers use child adapters, so userspace
does not race the kernel by writing raw mux registers or depend on changing
Linux bus numbers.

[Driver](../../nix/packages/as7760-fan/src/as7760_fan.c): hwmon name `as7760_fan`, twelve labeled
`fan*_input`/`fan*_fault` values, one shared `pwm1` (128–255 accepted). Probe is
read-only. Writes follow the vendor's manual sequence, verify duty readback,
and attempt full speed if duty programming fails. An unusable bus cannot be
made safe by software alone.

[Service and tool](../../nix/modules/thermal/as7760-fan.nix), option
`hardware.as7760Fan.controller` = `fand` (default when built from the root
`ai_re` flake, which can reach `rust/`) or `monitor` (the older guard below;
the default for `path:./nix` builds, which cannot):

```sh
sensors
as7760-fan status
fand status                       # JSON: zone state, reasons, readings
journalctl -u fand
fand set 80% --for 10m            # override through the daemon; failsafe still wins
systemctl stop fand               # ExecStopPost sets 100%
```

fand config: zone `board` = max of board_1..3 + cpu_board, `delta-balanced`
policy, `medium` profile, critical 45 °C, interval 2 s; output `pwm1` raw
128–255, readback "at least" (the CPLD has 16 steps), tach follow check
(front 21k / rear 18k rpm max). Metrics go to
`/run/prometheus-textfile/fand.prom` (`fand_*`, scraped with node-exporter).

Measured (ASIC initialised, idle; VictoriaMetrics, 10 s scrape):

| policy | duration | rotors | hottest board sensor |
| --- | --- | --- | --- |
| monitor at 80% | 8 min | ~16.9k rpm | avg 29.7 °C, slope −0.08 °C/min |
| fand floor (pwm 136, 53%) | 10 min test unit, then deployed | ~11–14k rpm | avg 31.0 °C (peak 31.5), flat after a ~1.3 °C rise |

fand asks for less than the floor (zone duty 45%) at these temperatures, so it
sits at the floor; the curve only matters when the board warms. Not yet tried
under traffic load.

**Older policy (`controller = "monitor"`):** `as7760-thermal` holds a quiet duty (default 80%, option
`hardware.as7760Fan.duty`) while all four board sensors are healthy and at or
below 40 °C, and goes to 100% on any error or at 45 °C (`hotC`/`coolC`). At
80%: rotors about 15,100–18,800 rpm, board 29–31.5 °C with the ASIC
initialised and idle. Not a tuned curve; ASIC die temperature is not used yet.
It still sets 100% on exit via ExecStopPost.
It reports missing/unreadable board sensors, a missing/stopped rotor, RPM below
500, or board temperature ≥60 °C. This is monitoring with a fixed safe initial
setting, not a verified thermal model or autonomous ASIC cooling algorithm.

Validation so far: driver compiled/linked/modposted with Linux 7.3.4 **x86_64**
headers (API check only); control-tool fixture checks passed for normal sensor
inventory, full duty, invalid duty, missing sensor, stopped rotor, high
temperature and wrong controller. Candidate DT compiles and contains the
expected fan node. **PowerPC module compilation/modpost/link also passed** against the actual
Linux 7.3.4 e6500 kernel. `modinfo` reports `7.3.4 SMP preempt mod_unload`;
the object is ELF64 big-endian PowerPC ELFv2. Linux hardware execution passed on the actual switch: the module loaded,
all twelve rotors reported about 17,900–21,600 RPM, and four LM75 sensors
reported 24.5–26.5 C. A write of 255 to `pwm1` read back 255; all rotors
continued running. (Reduced duty and both services were exercised on
hardware later, see the table above.) No module was loaded on the workstation.

Packaging correction: directly copying the `.ko` passed compilation but failed
NixOS's module aggregation because its per-module `.ko.cer` was missing. Using
kbuild `modules_install` with nixpkgs's common make flags generates both files;
that corrected PowerPC package built successfully at
`nixos-build/result-fan-final`. This is a packaging fix, not a hardware test.

### diagnostic display caveat (2026-11-14)

The RAM probe’s `sensors` prints `pwm1: 128%` while raw hwmon reads 255
(full duty). Treat raw `/sys/class/hwmon/hwmon*/pwm1` and the board control
tool as authoritative; the generic percentage display is not yet corrected.
Twelve RPM readings and four LM75 temperatures are valid live measurements.
