# fand

A vendor-neutral fan control daemon for network appliances (switches,
embedded control planes) and small servers. Written for NixOS, with no
dependencies beyond a handful of pure-Rust crates (serde, serde_json,
serde-saphyr, regex-lite, signal-hook, libc), no async runtime and no C
libraries, so it cross-compiles to embedded targets such as big-endian
ppc64.

Inputs are **sensors** (hwmon, plain files, commands, thermal zones);
outputs are **fans** (hwmon pwm, plain files, commands such as IPMI tools).
**Zones** connect sensors to outputs through a **policy** (curve, target
temperature, or fixed duty). An output driven by several zones runs at the
highest duty any of them asks for.

**Safety is the point.** Anything unexpected sends the affected zones to a
failsafe duty (100 % by default); fans are set to an exit duty on every way
out of the process; a systemd watchdog and an ExecStopPost safety net cover
the cases the process cannot handle itself. See [Safety model](#safety-model).

- [Commands](#commands)
- [Configuration reference](#configuration-reference)
- [Built-in curves](#built-in-curves)
- [Safety model](#safety-model)
- [Examples](#examples)
- [NixOS module](#nixos-module)
- [Observability](#observability)
- [Building, testing, cross compiling](#building-testing-cross-compiling)
- [Limitations](#limitations)

## Commands

| command | what it does |
|---|---|
| `fand run [--dry-run]` | the daemon (systemd `Type=notify`); `--dry-run` computes and logs but never writes |
| `fand check [--syntax-only] [--exec]` | validate the config and print resolved devices and current readings, read-only. `--syntax-only` touches nothing (used at NixOS build time); `--exec` also runs exec sensors once. Exit 2 if something does not resolve. Both print [hardware minimum](#safety-model) warnings, which never change the exit code |
| `fand status` | the running daemon's state as JSON (control socket; falls back to the state file) |
| `fand set 80% [--zone Z] [--for 10m]` | manual override through the daemon (also `60`, `12000rpm`); `fand set clear` ends it. Failsafe still wins; capped by `max_override`; a duty below an output's hardware minimum is accepted and raised to it, and the reply says so |
| `fand calibrate [--output O] [--steps 100,80,60] [--settle 8s] [--samples 3] [--max-temp 70] [--write PATH\|-] [--force]` | step an output's duty, record every rotor's RPM, write the calibration file. Refuses to run while the daemon's socket answers |
| `fand failsafe [--duty N]` | set every output to the exit duty now (the systemd `ExecStopPost` safety net). Touches no sensor |
| `fand curves` | print the built-in curves |

All commands take `--config PATH` (default `/etc/fand/config.yaml` or
`$FAND_CONFIG`) and `--debug`.

## Configuration reference

YAML, parsed strictly: unknown keys are errors, so a typo cannot silently
become a default. Every problem found is reported at once. Durations are a
number of seconds or `500ms`, `2s`, `5m`, `1h`. A *duty* is a percentage
(`60`, `"60%"`) or, where noted, a fan speed (`"9000rpm"`) converted per
output through its RPM model.

### Top level

| key | default | meaning |
|---|---|---|
| `interval` | `2s` | control loop period (0.1..60 s) |
| `profile` | `medium` | global speed floor: `none`, `slow`, `medium`, `fast`, or a duty (percent or rpm). Applied after the policy |
| `profiles` | `{slow: 30, medium: 45, fast: 65}` | duties behind the profile names (percent or rpm) |
| `failsafe_duty` | `100` | duty of a zone in failsafe (and of an output whose write failed). This, `exit_duty` and `startup_duty` above an output's `max_duty` are config errors |
| `exit_duty` | `100` | duty written on exit and by `fand failsafe` |
| `startup_duty` | `failsafe_duty` | duty every zone starts from; it ramps down from there after the dwell time |
| `calm_period` | `60s` | fault-free time before a zone leaves failsafe |
| `dry_run` | `false` | compute only (also honoured by `failsafe`) |
| `max_override` | `1h` | longest `fand set` override |
| `sysfs_root` | `/` | prefix for every device path (tests, simulation) |
| `curves` | `{}` | named user curves, `{name: {points: [[t, d], ...], hysteresis: h}}` |
| `runtime.state_file` | `/run/fand/state.json` | JSON status every tick (`null` disables) |
| `runtime.socket` | `/run/fand/fand.sock` | control socket (mode 0600; `null` disables) |
| `runtime.prometheus_file` | none | Prometheus text file (node_exporter textfile collector) |
| `runtime.calibration_file` | `/var/lib/fand/calibration.json` | written by `fand calibrate`, read at start |
| `runtime.log_interval` | `5m` | period of the summary log line |
| `sensors`, `outputs`, `zones` | required | below |

### Sensors (`sensors.<name>`)

| key | applies to | default | meaning |
|---|---|---|---|
| `type` | all | required | `hwmon`, `file`, `exec`, `thermal_zone` |
| `hwmon` | hwmon | required | device selector, see below |
| `input` / `label` | hwmon | `temp1` | attribute (`temp2`, `temp2_input`) or the `tempN` whose `tempN_label` equals this |
| `path` | file | required | file holding a number |
| `command` | exec | required | argv, no shell (use `[sh, -c, "..."]` if needed) |
| `interval` | exec | `10s` | how often the command runs (in its own thread; a slow tool never stalls the loop) |
| `timeout` | exec | `5s` | the command's process group is killed after this |
| `parse` | exec | first number | `{regex: "...", aggregate: max}` (every match; group 1 if present) or `{json: "ports[].temp", aggregate: max}` |
| `zone` | thermal_zone | required | zone `type` (`x86_pkg_temp`), `thermal_zone3` or `3` |
| `scale` | all | `0.001` hwmon/thermal, else `1` | multiply the raw number |
| `offset` | all | `0` | add after scaling (C) |
| `plausible` | all | `[-20, 150]` | readings outside are errors |
| `required` | all | `true` | a failing required sensor puts its zones in failsafe; an optional one is left out of the aggregation |
| `max_age` | all | exec: `3*interval + timeout` | older readings are stale (errors) |
| `critical` | all | none | at/above this, every zone using the sensor goes failsafe |

JSON paths: dot-separated keys; `[]`, `[*]` or `*` for every element of an
array or every value of an object; `[N]` for an index; a leading `$` is
ignored. Numbers and numeric strings count; `null` (an empty cage) is
skipped. `aggregate` is `max`, `min`, `mean` or `first`.

**hwmon selectors.** hwmonN numbers depend on probe order, so a selector is
a driver name plus optional disambiguation; a plain string means `{name: ...}`.

| key | matches |
|---|---|
| `name` | the hwmon `name` attribute exactly |
| `device` | substring of the resolved `device` path (e.g. `"3-0048"`, `"0000:01:00.0"`) |
| `of_node` | substring of the resolved `device/of_node` path (devicetree boards) |
| `path` | an explicit `/sys/class/hwmon/hwmonN` or a device directory with one hwmon child |
| `index` | pick the Nth of several matches (sorted by device path) |

Several matches without `index` is an error, never a silent pick.
Unresolvable devices are retried every 10 s, so modules loading late are
picked up; until then they are faults.

### Outputs (`outputs.<name>`)

| key | default | meaning |
|---|---|---|
| `type` | required | `hwmon`, `file` or `exec` |
| `hwmon`, `pwm` | –, `pwm1` | hwmon: selector and pwm attribute |
| `path` | – | file: the raw duty file |
| `command`, `timeout` | –, `5s` | exec: argv template with `{percent}` (integer, rounded up) and `{raw}` |
| `raw_max` | `255` | raw value meaning 100 % |
| `raw_min` | `0` | lowest raw value the hardware accepts (e.g. `128`) |
| `min_duty` / `max_duty` | from `raw_min` / `100` | never command outside this. The **hardware minimum** is `raw_min` as a percentage, or `min_duty` if higher |
| `stop` | `false` | the fan may stop (0) when the demand is 0; otherwise `min_duty` is the bottom |
| `enable` | none | value written to `pwmN_enable` (or `<path>_enable`) before duties, and re-asserted if firmware changes it (1 = manual on most drivers) |
| `exit_enable` | none | written after the exit duty (e.g. 2 or 5 = automatic on many Super I/O chips) |
| `readback` | `at_least` | after each write: `at_least` (read ≥ written − tolerance; drivers that round up), `exact`, `off` |
| `readback_tolerance` | `0` | raw units of slack |
| `reassert` | sysfs: every tick, exec: `60s` | rewrite an unchanged value this often |
| `tach` | none | RPM feedback, below |

Percent → raw rounds *up* (never less air than asked for) and is clamped to
`[raw_min, raw_max]`.

**Tach (`outputs.<name>.tach`).**

| key | default | meaning |
|---|---|---|
| `hwmon` | the output's | device with the `fanN_input` files (required for file/exec outputs) |
| `auto` / `inputs` | – | every `fanN_input` of the device, or a list (`[fan1, fan2]`) |
| `max_rpm` | none | RPM at 100 % for rotors without a better model |
| `max_rpm_by_label` | `{}` | by substring of `fanN_label`, longest match wins: `{front: 21000, rear: 18000}` for counter-rotating pairs |
| `min_rpm` | none | a rotor below this while commanded ≥ `check_above_duty` is a fault |
| `check_above_duty` | `0` | (any running duty) |
| `follow_tolerance` | `0.35` | a rotor below expected × (1 − tolerance) is "not following its command"; `null` disables |
| `follow_grace` | `20s` | time after a duty change before that check applies |
| `fault_attrs` | `true` | read `fanN_fault` / `fanN_alarm` when present (non-zero = fault) |
| `on_fault` | `failsafe` | or `warn`: tach problems are reported but do not cause failsafe |

RPM model per rotor, best first: the calibration file (measured
duty→rpm points, interpolated), then `max_rpm_by_label`, then `max_rpm`
(linear 0..max, which PWM fans exceed at mid duty, so it is a conservative
expectation). The model converts both ways: expected RPM for the follow
check and status, and rpm duties (`profile: 9000rpm`, `fixed: 12000rpm`,
`fand set 12000rpm`) to percent, taking the *largest* duty any rotor of the
output needs.

### Zones (`zones.<name>`)

| key | default | meaning |
|---|---|---|
| `outputs` | required | outputs this zone drives |
| `sensors` | `[]` | names, or `{sensor, offset, weight, target}` |
| `aggregate` | `max` | `max`, `min`, `mean`, `weighted_mean`, `first` over `value + offset` |
| `policy` | required | a curve name, or exactly one of `curve`, `target`, `fixed` (below) |
| `profile` | global | per-zone floor override |
| `ramp_up` / `ramp_down` | `10` / `1` %/s; target: `0` | slew limits (0 = unlimited). Up fast, down slowly |
| `dwell` | `10s`; target: `0s` | after any increase, hold this long before decreasing |
| `smoothing` | `0s` | EMA time constant on the aggregated temperature |
| `hysteresis` | the curve's | deadband (C) for curve policies |
| `critical` | none | aggregated temperature at/above which the zone goes failsafe |
| `failsafe_duty`, `calm_period` | global | per-zone overrides |
| `min_sensors` | `1` (0 without sensors) | fewer usable sensors is a fault. Set `0` for a zone of optional sensors that may all be absent: it then asks only for its floor |

Policies:

- `policy: delta-balanced` or `policy: {curve: <name or {points, hysteresis}>}`:
  piecewise linear, flat beyond the ends. Points must have increasing
  temperatures and non-decreasing duties. Hysteresis is an input deadband:
  rising temperatures apply at once, a falling one only once it has dropped
  more than the band below the level held.
- `policy: {target: 45, kp: 4, ki: 0.05, kd: 0, d_filter: 5s}`: PI(D) that holds a
  temperature. Error = temperature − target (hot → faster). The integral
  carries the operating point and stays within the output range; the
  derivative acts on the filtered measurement; output is limited to
  [floor, 100]. Anti-windup is back-calculation: each tick the duty the
  fans actually ran at (after ramp, max over zones and output clamp; of
  several outputs, the least driven one) is fed back, and the integral
  moves by (applied − output) · dt / Ti, Ti = kp/ki. That tracking time
  cancels the P term, so while another zone, an override or a ramp holds
  the fans, the integral relaxes toward the applied duty instead of winding
  down (or up, from a large negative P), and the zone takes over from where
  the fans are, without overshoot. While its own output runs it is a plain
  PI. Ramp and dwell default to off here: inside the loop they make it
  hunt. Per-sensor setpoints:
  `sensors: [{sensor: asic, target: 85}, {sensor: board, target: 45}]` with
  `target: 45` on the zone turn into offsets, so the zone tracks whichever
  sensor is furthest above its own setpoint.
- `policy: {fixed: 60}` (or `"12000rpm"`).

Per tick a zone computes: policy demand → max with the profile floor and
the highest hardware minimum of its outputs (`stop: true` outputs excluded)
→ ramp/dwell → zone duty; target zones then get the applied duty back. A
manual override replaces the policy and floor (not the output's hardware
minimum); failsafe replaces everything.

## Built-in curves

Tuned for high-flow, high-static-pressure (Delta-style) fans: they move a
lot of air even at 40-50 %, are loud above ~70 % and often have a high
minimum stable duty, so the curves start higher than desktop curves and
reach 100 % well before parts are at their limits. The first three expect a
board/inlet-class temperature, `delta-asic` a switch-chip or CPU die
temperature.

| curve | points (C → %) | hysteresis | intent |
|---|---|---|---|
| `delta-quiet` | 35→30, 45→40, 55→60, 65→85, 70→100 | 3 C | lowest noise, relies on good airflow |
| `delta-balanced` | 30→35, 40→45, 50→65, 58→85, 65→100 | 2 C | default trade-off |
| `delta-cool` | 25→50, 35→60, 45→80, 52→100 | 2 C | keeps parts cold, loud |
| `delta-asic` | 60→35, 75→55, 85→80, 95→100 | 3 C | die temperatures |

## Safety model

Fail safe, never fail silent. A zone enters **failsafe** (its
`failsafe_duty`, written immediately, bypassing ramps) on any of:

- a required sensor that cannot be resolved, read or parsed, an exec
  command that fails or times out, a stale reading (older than `max_age`),
  a reading outside `plausible`;
- fewer than `min_sensors` usable sensors;
- a sensor or the zone at/above its `critical` temperature;
- on any output it drives: an unresolvable pwm, a failed write, a readback
  mismatch, a fan fault/alarm flag, a rotor below `min_rpm` while commanded
  above `check_above_duty`, or a rotor not following its command (below the
  expected RPM by more than `follow_tolerance`, after `follow_grace`).

It leaves failsafe only after `calm_period` without any of these, then
ramps down normally (after the dwell). Every transition is logged with the
reasons. A failed write additionally tries the failsafe duty on the spot.

**Startup.** Missing devices are faults, not fatal errors: the daemon
starts, writes the failsafe duty where it can and keeps retrying
resolution every 10 s. Every zone starts at `startup_duty` (100 %) and
holds it for its dwell time (none by default for target zones). The only
startup failures that exit are an unparsable config (prevented by the
build-time check under NixOS) and a second instance (the control socket
answers), which exits before touching any fan.

**Exit.** On SIGTERM, SIGINT or SIGQUIT the loop stops and every output
gets `exit_duty` (then `exit_enable`). The engine lives in a guard whose
destructor does the same, so a panic (the release profile unwinds) also
ends at the exit duty. SIGHUP and SIGUSR1 only log a status line, so no
default signal action can kill the process without the exit path.

**What the process cannot cover** (SIGKILL, a hang, the OOM killer, a
kernel-level stall) is covered by systemd: `Type=notify` with `WatchdogSec`
(the loop pings once per tick only after a full tick completed, so a hung
sysfs read stops the pings), `Restart=always` without a start limit, and
`ExecStopPost=fand failsafe`, which sets the exit duty after *every* stop,
crash or watchdog kill. If you run fand outside the NixOS module, keep that
`ExecStopPost` line.

**Hardware limits.** Outputs are never commanded below their hardware
minimum (`raw_min` as a percentage, or `min_duty` if higher) or above
`max_duty` (`raw_max`); 0 only with `stop: true`. Overrides cannot go below
the hardware minimum and lose against failsafe. Zone targets already
include the minimum, so running at it is normal and silent. fand reports
only explicit settings below it:

- once at start (WARN) and in `fand check` (also `--syntax-only`): a
  profile floor (named or a percentage, with what it resolves to), fixed
  duties, points of inline or `curves:` curves, `failsafe_duty`,
  `startup_duty`, `exit_duty`. A built-in curve's low end under the minimum
  is by design: one `info:` line in `fand check` only. rpm values are not
  checked.
- at run time, one WARN line when an output's request (max over its zones)
  starts being raised (an override, or a failsafe/startup duty below the
  minimum) and one INFO line when it ends; `fand set` says so in its reply.
  0 on a `stop: true` output, and a request that rounds up to the same raw
  value (50 % with `raw_min: 128`), do not count.

**Robustness.** No `unwrap` on input, configuration errors carry their
path, all byte handling is text (big-endian clean), and the only `unsafe`
is one `kill(2)` to end a timed-out command's process group.

## Examples

- [examples/as7760.yaml](examples/as7760.yaml): Accton AS7760-32X. One
  shared pwm (raw 128..255 only) for 12 counter-rotating rotors
  (`max_rpm_by_label: {front: 21000, rear: 18000}`), four `lm75` told apart
  by `of_node`, the ASIC die temperature from a command (regex), optics
  from a JSON tool (optional). Three zones on the same output, max wins.
- [examples/x86-appliance.yaml](examples/x86-appliance.yaml): Super I/O
  pwm channels with `enable`/`exit_enable`, coretemp by label, an optional
  NVMe sensor with an offset, a PI target zone for the CPU, and a
  commented IPMI exec output.

Shape, in short:

```yaml
interval: 2s
profile: medium
sensors:
  board_1: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@48" } }
  asic:    { type: exec, command: [/etc/fand/asic-temp], interval: 10s,
             parse: { regex: "current=\\s*([0-9.]+)", aggregate: max } }
outputs:
  fans:
    type: hwmon
    hwmon: as7760_fan
    raw_min: 128
    tach: { auto: true, max_rpm_by_label: { front: 21000, rear: 18000 }, min_rpm: 1000 }
zones:
  board: { outputs: [fans], sensors: [board_1], policy: delta-balanced, critical: 60 }
  asic:  { outputs: [fans], sensors: [asic], policy: { target: 85 } }
```

## NixOS module

`nix/module.nix` is self-contained:

```nix
imports = [ ./rust/crates/fand/nix/module.nix ];
services.fand = {
  enable = true;
  profile = "medium";
  settings = {
    sensors.board = { type = "hwmon"; hwmon = { name = "lm75"; of_node = "temperature-sensor@48"; }; };
    outputs.fans = { type = "hwmon"; hwmon = "as7760_fan"; raw_min = 128;
                     tach = { auto = true; max_rpm_by_label = { front = 21000; rear = 18000; }; min_rpm = 1000; }; };
    zones.board = { outputs = [ "fans" ]; sensors = [ "board" ]; policy = "delta-balanced"; };
  };
};
```

| option | default | meaning |
|---|---|---|
| `services.fand.enable` | `false` | |
| `services.fand.package` | `callPackage ./package.nix {}` | |
| `services.fand.settings` | `{}` | the YAML config as an attrset (freeform; `interval`, `sensors`, `outputs`, `zones` are declared), rendered with `pkgs.formats.yaml` to `/etc/fand/config.yaml` |
| `services.fand.profile` | `null` | sets `settings.profile` (`mkDefault`) |
| `services.fand.exitDuty` | `100` | sets `settings.exit_duty` (`mkDefault`) |
| `services.fand.failsafeDuty` | `100` | sets `settings.failsafe_duty` (`mkDefault`) |
| `services.fand.watchdogSec` | `30` | `WatchdogSec` |
| `services.fand.checkConfig` | `true` | run `fand check --syntax-only` on the rendered file at build time |
| `services.fand.checkPackage` | `buildPackages.callPackage ./package.nix {}` | the checker when cross compiling (the target binary cannot run on the build machine) |
| `services.fand.hardening` | `true` | sandboxing that keeps `/sys` writable |
| `services.fand.extraReadWritePaths` | `[]` | more writable paths under `ProtectSystem=strict` |

The unit: `Type=notify`, `NotifyAccess=main`, `WatchdogSec`, `Restart=always`
(`RestartSec=2s`, no start limit), `ExecStopPost=fand failsafe`,
`RuntimeDirectory=fand` (socket, state, metrics), `StateDirectory=fand`
(calibration), `OOMScoreAdjust=-900`, and with `hardening`:
`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `NoNewPrivileges`,
`ProtectKernelModules`, `ProtectKernelLogs`, `ProtectControlGroups`,
`ProtectClock`, `ProtectHostname`, `RestrictRealtime`, `RestrictSUIDSGID`,
`RestrictNamespaces`, `LockPersonality`, `SystemCallArchitectures=native`,
`RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET AF_INET6`. Deliberately
*not* set: `ProtectKernelTunables` (it makes `/sys` read-only),
`PrivateDevices` and a capability bounding set (vendor tools behind exec
sensors/outputs may need `/dev` and privileges). The service runs as root,
because hwmon attributes are root-owned. The prometheus file's directory
is added to `ReadWritePaths` automatically.

## Observability

- Logs go to stderr; under systemd each line carries a `<N>` priority prefix
  for journald. Lines are written on state changes (failsafe enter /
  recovering / leave with reasons, sensor errors and recoveries, output
  faults set/cleared, overrides, outputs held at their hardware minimum)
  plus one summary line every `runtime.log_interval` (or on SIGHUP/SIGUSR1),
  never every tick.
- `STATUS=` of the unit shows the summary (`systemctl status fand`).
- `fand status`: JSON with every zone (state, reasons, temperature,
  demand, floor, duty, override), sensor (value, error, path, age) and
  output (duty, demand before the clamp, hardware_min, clamped, raw
  written/observed, faults, rotors with rpm, expected rpm, max rpm, percent
  of max, calibrated, fault).
- `runtime.state_file`: the same JSON, rewritten atomically every tick.
- `runtime.prometheus_file`: `fand_zone_duty_percent`,
  `fand_zone_temperature_celsius`, `fand_zone_failsafe`,
  `fand_sensor_temperature_celsius`, `fand_sensor_ok`,
  `fand_output_duty_percent`, `fand_output_demand_percent` (requested,
  before the hardware limits), `fand_output_clamped` (1 while raised to the
  hardware minimum), `fand_output_raw`, `fand_output_faults`,
  `fand_fan_rpm`, `fand_fan_expected_rpm`, `fand_fan_fault`, `fand_up`.

## Building, testing, cross compiling

```sh
cd rust && cargo test            # unit + simulation + end-to-end CLI tests
```

Tests use a fake sysfs tree in a temporary directory (`sysfs_root` is
injectable): device resolution, curves, PI, ramps, hysteresis, failsafe
transitions, config parsing, and a closed-loop simulation of an
AS7760-like box (12 rotors following the pwm, four lm75s, an exec ASIC
sensor) covering sensor loss, implausible values, exec failures, fan fault
flags, stalls, rotors not following, write failures, readback mismatches,
overrides, rpm floors, hardware-minimum warnings, dry run, PI convergence
and anti-windup (taking over from a higher zone, ramp/dwell holds) and
panic unwinding. The CLI tests run the real binary under a fake `NOTIFY_SOCKET` and check READY,
WATCHDOG, the control socket, SIGTERM → exit duty, single-instance, failsafe
and calibration. They need only `/bin/sh`.

Nix (same nixpkgs as the targets):

```sh
nix build --impure --expr 'let f = builtins.getFlake "path:/path/to/ai_re/nix";
  p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
  in p.callPackage ./rust/crates/fand/nix/package.nix { }'
# cross, big-endian ppc64 ELFv2 (e6500):
#   ... crossSystem = import .../configs/as7760/platform.nix; ...
```

nixpkgs maps `powerpc64-unknown-linux-gnuabielfv2` to Rust's
`powerpc64-unknown-linux-gnuelfv2` target (tier 3; nixpkgs builds its std
from source), and the dependency set has no build scripts that compile C.

## Limitations

- No live config reload; the NixOS unit restarts on config changes, which
  is cheap and passes through the exit duty.
- A single failed exec sensor run is a fault immediately (no "hold the last
  value" grace); use `required: false` plus another zone sensor if a tool
  is flaky.
- Exec outputs have no readback; rely on tach feedback.
- The follow check assumes RPM rises with duty; uncalibrated models are
  linear, so keep `follow_tolerance` generous until `fand calibrate` has run.
- Calibration samples steady-state RPM only (no spin-up or stall-point
  search) and never goes below an output's `min_duty`.
