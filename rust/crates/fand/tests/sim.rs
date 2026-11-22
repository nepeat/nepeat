//! Closed-loop simulation against a fake sysfs tree: an AS7760-like box
//! with a shared pwm, 12 rotors that follow the written duty, four lm75s and
//! an exec "ASIC" sensor. Asserts duty trajectories, including sensor loss,
//! fan faults, stalls, write failures and recovery.

use fand::config::{Config, DutySpec};
use fand::engine::{Engine, ExecMode, Options};
use fand::fakesys::FakeSys;
use std::collections::BTreeMap;
use std::path::PathBuf;

const DT: f64 = 2.0;

const AS7760: &str = r#"
sysfs_root: "{root}"
interval: 2s
profile: medium
calm_period: 20s
runtime: { state_file: null, socket: null, calibration_file: null }
sensors:
  board_1: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@48" } }
  board_2: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@49" } }
  board_3: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@4a" } }
  cpu_board: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@4b" } }
  asic:
    type: exec
    command: ["/bin/sh", "-c", "cat '{root}/asic.txt'"]
    interval: 2s
    parse: { regex: "current=\\s*([0-9.]+)", aggregate: max }
outputs:
  fans:
    type: hwmon
    hwmon: as7760_fan
    raw_min: 128
    tach:
      auto: true
      max_rpm_by_label: { front: 21000, rear: 18000 }
      min_rpm: 1000
      follow_tolerance: 0.35
      follow_grace: 10s
zones:
  board:
    outputs: [fans]
    sensors: [board_1, board_2, board_3, cpu_board]
    policy: delta-balanced
    ramp_up: 10
    ramp_down: 1
    dwell: 10s
  asic:
    outputs: [fans]
    sensors: [asic]
    policy: delta-asic
"#;

struct Rig {
    fs: FakeSys,
    fan: PathBuf,
    lm75: Vec<PathBuf>,
    engine: Engine,
    t: f64,
    /// Rotor index (0..12) -> speed factor applied by the plant.
    rotor_factor: BTreeMap<usize, f64>,
}

fn rotor_label(i: usize) -> String {
    format!("fan{}_{}", i / 2 + 1, if i % 2 == 0 { "front" } else { "rear" })
}

impl Rig {
    fn new(yaml: &str) -> Rig {
        Self::build(yaml, true, |_| {})
    }

    fn build(yaml: &str, dry_ok: bool, prep: impl FnOnce(&Rig)) -> Rig {
        let fs = FakeSys::new("sim");
        let fan = fs.hwmon(1, "as7760_fan", "platform/soc/i2c-2/2-0066", None);
        fs.set(&fan, "pwm1", "255");
        for i in 0..12 {
            fs.set(&fan, &format!("fan{}_input", i + 1), "0");
            fs.set(&fan, &format!("fan{}_label", i + 1), &rotor_label(i));
            fs.set(&fan, &format!("fan{}_fault", i + 1), "0");
        }
        let mut lm75 = Vec::new();
        for (n, addr) in ["48", "49", "4a", "4b"].iter().enumerate() {
            let d = fs.hwmon(
                10 + n as u32,
                "lm75",
                &format!("platform/soc/i2c-3/3-00{addr}"),
                Some(&format!("soc/i2c@119000/pca9548@76/i2c@1/temperature-sensor@{addr}")),
            );
            fs.set(&d, "temp1_input", "30000");
            lm75.push(d);
        }
        fs.write(
            "asic.txt",
            "temperature monitor 0: current= 55.0, peak= 57.0\ntemperature monitor 1: current= 56.0, peak= 58.0",
        );
        let cfg = Config::parse(&yaml.replace("{root}", fs.root.to_str().unwrap())).expect("config");
        let mut opts = Options::new(&cfg);
        opts.exec_mode = ExecMode::Inline;
        let _ = dry_ok;
        let engine = Engine::new(cfg, opts).expect("engine");
        let mut rig = Rig {
            fs,
            fan,
            lm75,
            engine,
            t: 0.0,
            rotor_factor: BTreeMap::new(),
        };
        prep(&rig);
        rig.plant();
        rig
    }

    fn raw(&self) -> u32 {
        self.fs.get(&self.fan, "pwm1").parse().unwrap()
    }

    /// Fans follow the pwm instantly (linear, 100 rpm resolution like the CPLD).
    fn plant(&mut self) {
        let raw = f64::from(self.raw());
        for i in 0..12 {
            let max = if i % 2 == 0 { 21000.0 } else { 18000.0 };
            let f = self.rotor_factor.get(&i).copied().unwrap_or(1.0);
            let rpm = (max * raw / 255.0 * f / 100.0).round() * 100.0;
            self.fs
                .set(&self.fan, &format!("fan{}_input", i + 1), &format!("{rpm}"));
        }
    }

    fn temps(&self, c: f64) {
        for d in &self.lm75 {
            self.fs.set(d, "temp1_input", &format!("{}", (c * 1000.0) as i64));
        }
    }

    fn asic(&self, c: f64) {
        self.fs.write(
            "asic.txt",
            &format!("temperature monitor 0: current= {c:.1}, peak= {c:.1}"),
        );
    }

    /// One control tick, then the plant reacts. Returns the output duty.
    fn tick(&mut self) -> f64 {
        self.engine.tick(self.t);
        self.t += DT;
        self.plant();
        self.engine.output("fans").unwrap().duty.unwrap()
    }

    fn run(&mut self, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.tick()).collect()
    }

    fn state(&self, zone: &str) -> &'static str {
        self.engine.status(self.t).zones[zone].state
    }

    fn zone_duty(&self, zone: &str) -> f64 {
        self.engine.zone(zone).unwrap().duty
    }
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.05
}

#[test]
fn startup_full_then_ramps_down_to_the_floor_with_dwell() {
    let mut rig = Rig::new(AS7760);
    let duties = rig.run(60);
    assert_eq!(duties[0], 100.0, "starts at the startup duty");
    assert_eq!(rig.state("board"), "normal");
    // Dwell: the startup "increase" holds the duty for 10 s (ticks at t=0..8).
    assert!(duties[..5].iter().all(|d| *d == 100.0), "{:?}", &duties[..6]);
    // Then at most 1 %/s down: 2 % per 2 s tick, monotone.
    for w in duties.windows(2) {
        assert!(w[1] <= w[0] + 1e-9 && w[0] - w[1] <= 2.0 + 1e-9, "{w:?}");
    }
    // 30 C on delta-balanced = 35 %, below the medium floor (45 %) and below
    // the hardware minimum (raw 128 = 50.2 %), which the zone target includes:
    // the zone and the output settle at 50.2 % (raw 128).
    assert!(approx(rig.zone_duty("board"), 50.196), "{}", rig.zone_duty("board"));
    assert_eq!(rig.raw(), 128);
    assert!(approx(*duties.last().unwrap(), 50.196));
    // The ASIC zone (55-56 C on delta-asic = 35 %) is also at the minimum.
    assert!(approx(rig.zone_duty("asic"), 50.196));
}

#[test]
fn heating_ramps_up_fast_and_cooling_returns_slowly() {
    let mut rig = Rig::new(AS7760);
    rig.run(60);
    assert_eq!(rig.raw(), 128);
    rig.temps(50.0); // delta-balanced: 65 %
    let up = rig.run(3);
    assert!(approx(rig.zone_duty("board"), 65.0), "{}", rig.zone_duty("board"));
    assert_eq!(up[0], 65.0, "10 %/s up, 2 s tick: from 45 % straight to 65 %");
    rig.temps(61.5); // 58 C: 85 %, 65 C: 100 % -> 61.5 C = 92.5 %
    let up = rig.run(1);
    assert_eq!(up[0], 85.0, "limited to +20 % per tick");
    rig.run(1);
    assert!(approx(rig.zone_duty("board"), 92.5));
    // Small dip within the 2 C hysteresis: no change at all.
    rig.temps(60.0);
    let hold = rig.run(10);
    assert!(hold.iter().all(|d| approx(*d, 92.5)), "{hold:?}");
    // Real cooling: dwell 10 s since the last increase, then 1 %/s down.
    rig.temps(40.0);
    let down = rig.run(20);
    assert!(
        down.windows(2).all(|w| w[1] <= w[0] && w[0] - w[1] <= 2.0 + 1e-9),
        "{down:?}"
    );
    assert!(*down.last().unwrap() > 45.0, "still descending slowly");
}

#[test]
fn lost_sensor_goes_failsafe_immediately_and_recovers_after_calm() {
    let mut rig = Rig::new(AS7760);
    rig.run(60);
    assert_eq!(rig.raw(), 128);
    rig.fs.remove(&rig.lm75[2], "temp1_input");
    let d = rig.tick();
    assert_eq!(d, 100.0, "failsafe on the very next tick");
    assert_eq!(rig.raw(), 255);
    assert_eq!(rig.state("board"), "failsafe");
    let reasons = rig.engine.status(rig.t).zones["board"].reasons.join(" ");
    assert!(reasons.contains("board_3"), "{reasons}");
    rig.run(5);
    assert_eq!(rig.raw(), 255);
    // The device comes back (re-resolved after the retry interval).
    rig.fs.set(&rig.lm75[2], "temp1_input", "30000");
    let mut ticks_at_full = 0;
    for _ in 0..30 {
        if rig.tick() == 100.0 {
            ticks_at_full += 1;
        }
    }
    // Re-resolve happens within 10 s (5 ticks); then 20 s calm (10 ticks);
    // then the dwell (10 s) before ramping down.
    assert!((10..=22).contains(&ticks_at_full), "{ticks_at_full}");
    assert_eq!(rig.state("board"), "normal");
    assert!(rig.engine.output("fans").unwrap().duty.unwrap() < 100.0);
}

#[test]
fn implausible_and_stale_readings_are_faults() {
    let mut rig = Rig::new(AS7760);
    rig.run(30);
    rig.fs.set(&rig.lm75[0], "temp1_input", "-128000"); // a classic bus-error value
    rig.tick();
    assert_eq!(rig.state("board"), "failsafe");
    assert_eq!(rig.raw(), 255);
    assert!(rig.engine.status(rig.t).zones["board"].reasons[0].contains("implausible"));
}

#[test]
fn exec_sensor_drives_its_zone_and_its_failure_is_failsafe() {
    let mut rig = Rig::new(AS7760);
    rig.run(60);
    assert_eq!(rig.raw(), 128);
    rig.asic(85.0); // delta-asic: 80 %
    rig.run(3);
    assert!(approx(rig.zone_duty("asic"), 80.0), "{}", rig.zone_duty("asic"));
    assert_eq!(rig.raw(), 204, "80 % of 255, rounded up");
    // The tool starts printing garbage: the ASIC zone goes failsafe.
    rig.fs.write("asic.txt", "SDK not ready");
    rig.tick();
    assert_eq!(rig.state("asic"), "failsafe");
    assert_eq!(rig.state("board"), "normal");
    assert_eq!(rig.raw(), 255, "max over zones");
}

#[test]
fn fan_fault_flag_and_stalled_rotor_trigger_failsafe() {
    let mut rig = Rig::new(AS7760);
    rig.run(60);
    rig.fs.set(&rig.fan, "fan5_fault", "1");
    rig.tick();
    assert_eq!(rig.state("board"), "failsafe");
    assert_eq!(rig.state("asic"), "failsafe", "both zones drive the faulty output");
    assert_eq!(rig.raw(), 255);
    rig.fs.set(&rig.fan, "fan5_fault", "0");
    rig.run(12);
    assert_eq!(rig.state("board"), "normal", "recovered after 20 s calm");

    // A rotor that stops while commanded: below min_rpm.
    let mut rig = Rig::new(AS7760);
    rig.run(60);
    rig.rotor_factor.insert(7, 0.0);
    rig.plant();
    rig.tick();
    assert_eq!(rig.state("board"), "failsafe");
    let faults = &rig.engine.status(rig.t).outputs["fans"].faults;
    assert!(
        faults
            .iter()
            .any(|f| f.contains("fan4_rear") && f.contains("below the minimum")),
        "{faults:?}"
    );
}

#[test]
fn rotor_not_following_its_command_is_detected_after_grace() {
    let mut rig = Rig::new(AS7760);
    rig.run(60);
    // fan2_front spins, but at 40 % of what the duty should give (> min_rpm).
    rig.rotor_factor.insert(2, 0.4);
    rig.plant();
    rig.tick();
    assert_eq!(
        rig.state("board"),
        "failsafe",
        "no duty change for > grace, so it applies at once"
    );
    let faults = rig.engine.status(rig.t).outputs["fans"].faults.join(" ");
    assert!(
        faults.contains("fan2_front") && faults.contains("not following"),
        "{faults}"
    );
    // At 100 % the healthy-but-slow rotor still misses: failsafe holds.
    rig.run(20);
    assert_eq!(rig.state("board"), "failsafe");
    assert_eq!(rig.raw(), 255);
}

#[test]
fn write_failure_and_readback_mismatch_go_failsafe() {
    let mut rig = Rig::new(AS7760);
    rig.run(60);
    // pwm1 disappears (driver unbound): write fails.
    rig.fs.remove(&rig.fan, "pwm1");
    rig.engine.tick(rig.t);
    rig.t += DT;
    rig.engine.tick(rig.t);
    rig.t += DT;
    assert_eq!(rig.state("board"), "failsafe");
    let st = rig.engine.status(rig.t);
    assert!(
        st.outputs["fans"]
            .faults
            .iter()
            .any(|f| f.contains("no attribute pwm1") || f.contains("No such file")),
        "{:?}",
        st.outputs["fans"].faults
    );

    // Readback mismatch: a pwm that swallows writes (/dev/null reads empty).
    let mut rig = Rig::new(AS7760);
    rig.run(10);
    std::fs::remove_file(rig.fan.join("pwm1")).unwrap();
    std::os::unix::fs::symlink("/dev/null", rig.fan.join("pwm1")).unwrap();
    rig.engine.tick(rig.t);
    rig.t += DT;
    rig.engine.tick(rig.t);
    assert_eq!(rig.state("board"), "failsafe");
    let faults = rig.engine.status(rig.t).outputs["fans"].faults.join(" ");
    assert!(faults.contains("readback"), "{faults}");
}

#[test]
fn missing_required_sensor_at_startup_is_failsafe_not_an_exit() {
    let yaml = AS7760.replace("temperature-sensor@4b", "temperature-sensor@4c");
    let mut rig = Rig::new(&yaml);
    let duties = rig.run(20);
    assert!(duties.iter().all(|d| *d == 100.0));
    assert_eq!(rig.state("board"), "failsafe");
    assert_eq!(rig.state("asic"), "normal");
    assert_eq!(rig.raw(), 255);
}

#[test]
fn critical_temperature_and_override_precedence() {
    let yaml = AS7760.replace(
        "    policy: delta-balanced\n",
        "    policy: delta-balanced\n    critical: 55\n",
    );
    let mut rig = Rig::new(&yaml);
    rig.run(60);
    let now = rig.t;
    rig.engine
        .set_override(None, DutySpec::Percent(70.0), 60.0, now)
        .unwrap();
    rig.tick();
    assert_eq!(rig.zone_duty("board"), 70.0);
    assert_eq!(rig.raw(), 179);
    // Override cannot beat failsafe.
    rig.temps(56.0);
    rig.tick();
    assert_eq!(rig.state("board"), "failsafe");
    assert_eq!(rig.raw(), 255);
    rig.temps(30.0);
    rig.run(12);
    assert_eq!(rig.state("board"), "normal");
    assert_eq!(rig.zone_duty("board"), 70.0, "override still active");
    // Expiry (60 s after it was set, t = 180) hands back to the policy.
    rig.run(20);
    assert!(rig.engine.zone("board").unwrap().override_.is_none());
    assert!(
        rig.engine
            .set_override(Some("nope"), DutySpec::Percent(50.0), 10.0, 0.0)
            .is_err()
    );
    assert!(
        rig.engine
            .set_override(None, DutySpec::Percent(50.0), 0.0, 0.0)
            .is_err()
    );
}

#[test]
fn rpm_profile_floor_uses_the_slowest_rotor_model() {
    let yaml = AS7760.replace("profile: medium", "profile: 12000rpm");
    let mut rig = Rig::new(&yaml);
    rig.run(80);
    // Rear rotors (18000 rpm max) need 66.7 % for 12000 rpm; front need 57.1 %.
    assert!(
        (rig.zone_duty("board") - 66.667).abs() < 0.01,
        "{}",
        rig.zone_duty("board")
    );
    let rear = rig.engine.status(rig.t).outputs["fans"].rotors[1].rpm.unwrap();
    assert!(rear >= 12000.0, "{rear}");
}

#[test]
fn target_policy_holds_a_setpoint_without_hunting() {
    // One board sensor whose temperature follows a first-order plant:
    // equilibrium 25 C at 100 % duty, 65 C at 0 %, time constant 20 s.
    let yaml = r#"
sysfs_root: "{root}"
interval: 2s
profile: none
runtime: { state_file: null, socket: null, calibration_file: null }
sensors:
  board: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@48" } }
outputs:
  fans: { type: hwmon, hwmon: as7760_fan, raw_min: 0, min_duty: 20 }
zones:
  board:
    outputs: [fans]
    sensors: [board]
    policy: { target: 40, kp: 4, ki: 0.1 }
    ramp_up: 10
    ramp_down: 2
    dwell: 0s
"#;
    let mut rig = Rig::new(yaml);
    let mut temp = 45.0;
    let mut hist = Vec::new();
    for _ in 0..400 {
        rig.fs
            .set(&rig.lm75[0], "temp1_input", &format!("{}", (temp * 1000.0) as i64));
        let d = rig.tick();
        let eq = 25.0 + 40.0 * (1.0 - d / 100.0);
        temp += (eq - temp) * (1.0 - (-DT / 20.0f64).exp());
        hist.push((temp, d));
    }
    let tail = &hist[300..];
    for (t, d) in tail {
        assert!((t - 40.0).abs() < 0.5, "temperature {t} not held at 40");
        assert!((d - 62.5).abs() < 2.5, "duty {d} not near the 62.5 % equilibrium");
    }
    let (lo, hi) = tail
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), (_, d)| (lo.min(*d), hi.max(*d)));
    assert!(hi - lo < 2.0, "duty hunting between {lo} and {hi}");
}

#[test]
fn dry_run_never_writes_and_shutdown_writes_exit_duty() {
    let yaml = AS7760.replace("profile: medium", "profile: medium\ndry_run: true");
    let mut rig = Rig::new(&yaml);
    rig.fs.set(&rig.fan, "pwm1", "170");
    rig.plant();
    let d = rig.run(40);
    assert_eq!(rig.raw(), 170, "dry run wrote to pwm1");
    assert!(d.last().unwrap() < &100.0, "but it computed a duty");
    assert!(rig.engine.shutdown(100.0, rig.t).is_empty());
    assert_eq!(rig.raw(), 170);

    let mut rig = Rig::new(AS7760);
    rig.run(60);
    assert_eq!(rig.raw(), 128);
    assert!(rig.engine.shutdown(100.0, rig.t).is_empty());
    assert_eq!(rig.raw(), 255);
}

#[test]
fn optional_sensors_and_stop_capable_outputs() {
    let yaml = r#"
sysfs_root: "{root}"
runtime: { state_file: null, socket: null, calibration_file: null }
profile: none
sensors:
  a: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@48" }, required: false }
  b: { type: file, path: /optics/max_temp, required: false }
outputs:
  fans: { type: hwmon, hwmon: as7760_fan, stop: true, min_duty: 20, readback: exact }
zones:
  z:
    outputs: [fans]
    sensors: [a, b]
    min_sensors: 0
    policy: { curve: { points: [[40, 0], [41, 30], [60, 100]] } }
    ramp_down: 0
    ramp_up: 0
    dwell: 0s
"#;
    let mut rig = Rig::new(yaml);
    rig.run(2);
    assert_eq!(rig.raw(), 0, "30 C demands 0 % and the output may stop");
    assert!(
        !rig.engine.status(rig.t).outputs["fans"].clamped,
        "0 is legal with stop: true"
    );
    rig.fs.remove(&rig.lm75[0], "temp1_input");
    rig.run(1);
    assert_eq!(
        rig.state("z"),
        "normal",
        "optional sensors gone and min_sensors 0: no fault"
    );
    assert_eq!(rig.raw(), 0);
    rig.fs.write("optics/max_temp", "50.5");
    rig.run(1);
    assert_eq!(
        rig.raw(),
        166,
        "the file sensor alone drives the zone (65 % -> 165.75, rounded up)"
    );
}

#[test]
fn panic_unwinding_sets_exit_duty() {
    let mut rig = Rig::new(AS7760);
    rig.run(60);
    assert_eq!(rig.raw(), 128);
    let Rig { fs, fan, engine, .. } = rig;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _guard = fand::daemon::ExitGuard::new(engine, 100.0);
        panic!("simulated bug in the control loop");
    }));
    assert!(r.is_err());
    assert_eq!(fs.get(&fan, "pwm1"), "255");
}

#[test]
fn startup_resolution_of_ambiguous_hwmon_is_a_fault() {
    let yaml = AS7760.replace(r#"{ name: lm75, of_node: "temperature-sensor@48" }"#, "lm75");
    let mut rig = Rig::new(&yaml);
    rig.tick();
    let st = rig.engine.status(rig.t);
    assert!(
        st.sensors["board_1"]
            .error
            .as_deref()
            .unwrap_or("")
            .contains("4 hwmon devices match")
    );
    assert_eq!(st.zones["board"].state, "failsafe");
}

fn lines(level: fand::log::Level) -> Vec<String> {
    fand::log::captured()
        .into_iter()
        .filter(|(l, _)| *l == level)
        .map(|(_, m)| m)
        .collect()
}

#[test]
fn zone_targets_include_the_hardware_minimum_and_clamps_are_logged_once() {
    use fand::log::Level;
    let mut rig = Rig::new(AS7760);
    fand::log::capture();
    // Idle at the hardware minimum is normal: no warning, nothing clamped.
    rig.run(60);
    assert!(lines(Level::Warn).is_empty());
    let st = rig.engine.status(rig.t);
    assert_eq!(st.zones["board"].duty, 50.2);
    assert!(!st.outputs["fans"].clamped);
    assert_eq!(st.outputs["fans"].demand, Some(50.2));
    assert_eq!(st.outputs["fans"].hardware_min, 50.2);
    let prom = fand::daemon::prometheus(&st);
    assert!(prom.contains("fand_output_clamped{output=\"fans\"} 0"), "{prom}");
    assert!(
        prom.contains("fand_output_demand_percent{output=\"fans\"} 50.2"),
        "{prom}"
    );

    // An override below the minimum is accepted, clamped, and says so.
    let now = rig.t;
    let msg = rig
        .engine
        .set_override(None, DutySpec::Percent(30.0), 3600.0, now)
        .unwrap();
    assert_eq!(
        msg,
        "override 30% for 3600s on asic, board; below the hardware minimum, raised to it on output fans \
         (50.2%, raw_min 128 of 255)"
    );
    assert_eq!(lines(Level::Warn), [msg]);
    // One line when the clamp starts, none while it lasts (here 20 min).
    rig.run(600);
    assert_eq!(rig.raw(), 128);
    assert_eq!(
        lines(Level::Warn),
        [
            "output fans: requested 30% (zones asic 30% override, board 30% override) is below its hardware minimum \
          (50.2%, raw_min 128 of 255); running at 50.2%"
        ]
    );
    let st = rig.engine.status(rig.t);
    assert!(st.outputs["fans"].clamped);
    assert_eq!(st.outputs["fans"].demand, Some(30.0));
    assert!(fand::daemon::prometheus(&st).contains("fand_output_clamped{output=\"fans\"} 1"));

    // And one info line when it ends: ramping up from 30 % (+20 % per tick),
    // 50 % already writes raw 128.
    rig.engine.clear_override(None).unwrap();
    rig.tick();
    let info = lines(Level::Info);
    assert_eq!(
        info,
        ["output fans: requested 50% (zones asic 50%, board 50%) is at or above its hardware minimum (50.2%) again"],
        "{info:?}"
    );
    assert!(!rig.engine.status(rig.t).outputs["fans"].clamped);
}

/// First-order thermal plant for the PI tests: equilibrium 25 C at 100 %
/// duty, 65 C at 0 % (40 C needs 62.5 %), time constant 20 s.
fn plant_step(temp: f64, duty: f64) -> f64 {
    let eq = 25.0 + 40.0 * (1.0 - duty / 100.0);
    temp + (eq - temp) * (1.0 - (-DT / 20.0f64).exp())
}

const PI_RIG: &str = r#"
sysfs_root: "{root}"
interval: 2s
profile: none
runtime: { state_file: null, socket: null, calibration_file: null }
sensors:
  board: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@48" } }
  other: { type: hwmon, hwmon: { name: lm75, of_node: "temperature-sensor@49" } }
outputs:
  fans: { type: hwmon, hwmon: as7760_fan, raw_min: 0, min_duty: 20 }
zones:
  pi:
    outputs: [fans]
    sensors: [board]
    policy: { target: 40, kp: 4, ki: 0.1 }
  other:
    outputs: [fans]
    sensors: [other]
    policy: { curve: { points: [[30, 0], [50, 85]] } }
"#;

impl Rig {
    /// One tick with the PI zone's sensor following `plant_step`.
    fn plant_tick(&mut self, temp: &mut f64) -> f64 {
        self.fs
            .set(&self.lm75[0], "temp1_input", &format!("{}", (*temp * 1000.0) as i64));
        let d = self.tick();
        *temp = plant_step(*temp, d);
        d
    }
}

#[test]
fn pi_zone_takes_over_from_a_higher_zone_without_overshoot() {
    let mut rig = Rig::new(PI_RIG);
    let mut temp = 35.0;
    // Another zone holds 85 % for 10 minutes: the PI zone sits 9 C below its
    // setpoint the whole time. Its integral follows the applied duty instead
    // of sinking to the 20 % floor.
    rig.fs.set(&rig.lm75[1], "temp1_input", "55000");
    for _ in 0..300 {
        rig.plant_tick(&mut temp);
    }
    assert_eq!(rig.zone_duty("other"), 85.0);
    let pi = rig.engine.zone("pi").unwrap();
    assert!(pi.demand.unwrap() > 85.0 - 4.0 * 9.0 - 1.0, "{:?}", pi.demand);
    // The other zone lets go (its default dwell, then 1 %/s down). The PI
    // zone takes over at the duty the fans are at, before its sensor passes
    // the setpoint.
    rig.fs.set(&rig.lm75[1], "temp1_input", "20000");
    let hist: Vec<(f64, f64)> = (0..300).map(|_| (rig.plant_tick(&mut temp), temp)).collect();
    let peak = hist.iter().map(|(_, t)| *t).fold(f64::MIN, f64::max);
    assert!(peak < 40.5, "overshoot to {peak} C");
    for (d, t) in &hist[200..] {
        assert!((t - 40.0).abs() < 0.5, "temperature {t} not held at 40");
        assert!((d - 62.5).abs() < 2.5, "duty {d} not near the 62.5 % equilibrium");
    }
}

#[test]
fn pi_integral_does_not_wind_up_while_ramp_and_dwell_hold() {
    // Explicit ramp/dwell on a target zone (they default to off there): the
    // startup duty is held for 60 s, then descends at 0.5 %/s.
    let yaml = PI_RIG
        .replace("ki: 0.1 }\n", "ki: 0.1 }\n    ramp_down: 0.5\n    dwell: 60s\n")
        .replace("[30, 0], [50, 85]", "[60, 0], [70, 85]");
    let mut rig = Rig::new(&yaml);
    let mut temp = 45.0;
    let mut hist = Vec::new();
    for k in 0..400 {
        let d = rig.plant_tick(&mut temp);
        hist.push((d, temp));
        let z = rig.engine.zone("pi").unwrap();
        if k < 30 {
            assert_eq!(d, 100.0, "held by the dwell");
            // The operating point (output minus P) never sinks while the fans
            // are held at 100 %: it relaxes toward 100 from its bumpless start
            // at 80 (100 % minus P at 45 C).
            let (demand, pv) = (z.demand.unwrap(), z.smoothed.unwrap());
            if demand < 100.0 && demand > 20.0 {
                let op = demand - 4.0 * (pv - 40.0);
                assert!(op > 80.0 - 1e-6, "integral wound down to {op} at tick {k}");
            }
        }
    }
    // After the hold the fans come down to the PI's level and stop there:
    // no undershoot of the duty, so no temperature overshoot.
    let first_cool = hist.iter().position(|(_, t)| *t < 40.0).unwrap();
    let peak = hist[first_cool..].iter().map(|(_, t)| *t).fold(f64::MIN, f64::max);
    assert!(peak < 40.5, "overshoot to {peak} C after the hold");
    let low = hist.iter().map(|(d, _)| *d).fold(f64::MAX, f64::min);
    assert!(low > 62.5 - 2.5, "duty undershot to {low}");
    for (d, t) in &hist[300..] {
        assert!((t - 40.0).abs() < 0.5, "temperature {t} not held at 40");
        assert!((d - 62.5).abs() < 2.5, "duty {d}");
    }
}

#[test]
fn curve_and_fixed_zones_keep_their_ramp_and_dwell_defaults() {
    // asic (delta-asic) and a fixed zone use the defaults (10 %/s up,
    // 1 %/s down, 10 s dwell), next to a target zone (no ramp, no dwell by
    // default) on the same output.
    let yaml = format!(
        "{AS7760}  fixed:\n    outputs: [fans]\n    policy: {{ fixed: 60 }}\n  \
         pi:\n    outputs: [fans]\n    sensors: [board_1]\n    policy: {{ target: 45 }}\n"
    );
    let mut rig = Rig::new(&yaml);
    let mut asic = Vec::new();
    let mut fixed = Vec::new();
    let mut pi = Vec::new();
    for _ in 0..40 {
        rig.tick();
        asic.push(rig.zone_duty("asic"));
        fixed.push(rig.zone_duty("fixed"));
        pi.push(rig.zone_duty("pi"));
    }
    for z in [&asic, &fixed] {
        assert!(z[..5].iter().all(|d| *d == 100.0), "dwell: {:?}", &z[..6]);
        assert!(approx(z[5], 98.0) && approx(z[6], 96.0), "1 %/s down: {:?}", &z[..8]);
    }
    assert_eq!(fixed[39], 60.0);
    assert!(approx(asic[39], 50.196));
    // The target zone (30 C, 15 C below target) leaves the startup duty on
    // the first tick, straight to the hardware minimum.
    assert!(approx(pi[0], 50.196), "{:?}", &pi[..3]);
    // Heating: +20 % per 2 s tick; cooling: the 10 s dwell again, then 2 % per tick.
    rig.asic(85.0); // delta-asic: 80 %
    rig.tick();
    assert!(approx(rig.zone_duty("asic"), 70.196), "{}", rig.zone_duty("asic"));
    rig.tick();
    assert!(approx(rig.zone_duty("asic"), 80.0));
    rig.asic(55.0);
    let down: Vec<f64> = (0..7)
        .map(|_| {
            rig.tick();
            rig.zone_duty("asic")
        })
        .collect();
    assert!(down[..4].iter().all(|d| approx(*d, 80.0)), "{down:?}");
    assert!(approx(down[4], 78.0) && approx(down[5], 76.0), "{down:?}");
}

/// The AS7760 ASIC sensor as deployed: a file its writer rewrites, which is
/// briefly missing whenever the writer restarts or hands over.
const GRACE_RIG: &str = r#"
sysfs_root: "{root}"
interval: 2s
profile: medium
calm_period: 60s
runtime: { state_file: null, socket: null, calibration_file: null }
sensors:
  asic: { type: file, path: /run/asic/temperature, grace: 15s }
outputs:
  fans: { type: hwmon, hwmon: as7760_fan, raw_min: 128 }
zones:
  asic:
    outputs: [fans]
    sensors: [asic]
    policy: delta-asic
"#;

impl Rig {
    fn asic_file(&self, c: Option<f64>) {
        match c {
            Some(c) => {
                self.fs.write("run/asic/temperature", &format!("{c:.1}"));
            }
            None => self.fs.remove(&self.fs.root.join("run/asic"), "temperature"),
        }
    }

    fn in_grace(&self, sensor: &str) -> bool {
        self.engine.status(self.t).sensors[sensor].in_grace
    }
}

fn grace_rig(yaml: &str, c: f64) -> Rig {
    let mut rig = Rig::new(yaml);
    rig.asic_file(Some(c));
    rig.run(60);
    rig
}

#[test]
fn grace_rides_out_a_short_gap_without_failsafe_or_duty_change() {
    use fand::log::Level;
    let mut rig = grace_rig(GRACE_RIG, 80.0); // delta-asic: 63.3 %
    let steady = rig.tick();
    assert!(steady > 60.0 && steady < 100.0, "{steady}");
    fand::log::capture();
    // The file is gone for ~5 s: the ticks 2 s and 4 s after the last good
    // reading miss it.
    rig.asic_file(None);
    for _ in 0..2 {
        assert_eq!(rig.tick(), steady);
        assert_eq!(rig.state("asic"), "normal");
        assert!(rig.in_grace("asic"));
    }
    let st = rig.engine.status(rig.t);
    assert_eq!(st.sensors["asic"].value, Some(80.0), "the last good value is held");
    assert!(st.sensors["asic"].error.is_some(), "and the failure is visible");
    let prom = fand::daemon::prometheus(&st);
    assert!(prom.contains("fand_sensor_in_grace{sensor=\"asic\"} 1"), "{prom}");
    assert!(prom.contains("fand_sensor_ok{sensor=\"asic\"} 0"), "{prom}");
    rig.asic_file(Some(80.0));
    for _ in 0..10 {
        assert_eq!(rig.tick(), steady);
        assert_eq!(rig.state("asic"), "normal");
    }
    assert!(!rig.in_grace("asic"));
    let log = fand::log::captured();
    assert!(!log.iter().any(|(l, _)| *l <= Level::Warn), "{log:?}");
    let info: Vec<&String> = log.iter().filter(|(l, _)| *l == Level::Info).map(|(_, m)| m).collect();
    assert_eq!(info.len(), 2, "{info:?}");
    assert!(
        info[0].starts_with("sensor asic: no reading (") && info[0].ends_with("), holding 80.0 C for up to 15s"),
        "{}",
        info[0]
    );
    assert_eq!(info[1], "sensor asic: reading back after 6s, 80.0 C");
}

#[test]
fn grace_runs_out_15s_after_the_last_good_reading() {
    use fand::log::Level;
    let mut rig = grace_rig(GRACE_RIG, 80.0);
    let steady = rig.tick();
    let last_good = rig.t - DT;
    fand::log::capture();
    rig.asic_file(None);
    let mut first_fault = None;
    for _ in 0..10 {
        let now = rig.t;
        let d = rig.tick();
        if rig.state("asic") == "failsafe" {
            first_fault.get_or_insert((now - last_good, d));
        } else {
            assert_eq!(d, steady);
        }
    }
    assert_eq!(
        first_fault,
        Some((16.0, 100.0)),
        "held through 14 s, failsafe on the 16 s tick"
    );
    assert!(!rig.in_grace("asic"));
    let log = fand::log::captured();
    let ran_out = log
        .iter()
        .position(|(l, m)| *l == Level::Info && m == "sensor asic: grace of 15s ran out, no good reading for 16s")
        .expect("grace ran out line");
    let warn = log
        .iter()
        .position(|(l, m)| *l == Level::Warn && m.starts_with("sensor asic: "))
        .expect("the normal sensor warning");
    assert!(ran_out < warn, "{log:?}");
    assert!(log.iter().any(|(l, m)| *l == Level::Error && m.contains("FAILSAFE")));
}

#[test]
fn no_grace_for_a_value_at_a_critical() {
    // At the sensor's own critical: already failsafe, and a gap is not held.
    let yaml = GRACE_RIG.replace("grace: 15s }", "grace: 15s, critical: 85 }");
    let mut rig = grace_rig(&yaml, 86.0);
    assert_eq!(rig.state("asic"), "failsafe");
    rig.asic_file(None);
    rig.tick();
    assert!(!rig.in_grace("asic"));
    let st = rig.engine.status(rig.t);
    assert_eq!(st.sensors["asic"].value, None);
    assert!(
        st.zones["asic"].reasons.iter().any(|r| r.starts_with("sensor asic: ")),
        "{:?}",
        st.zones["asic"].reasons
    );

    // A zone critical counts too, through the zone's offset for the sensor:
    // 76 + 5 is at the zone's critical of 80, 74 + 5 is not.
    let yaml = GRACE_RIG
        .replace("sensors: [asic]", "sensors: [{ sensor: asic, offset: 5 }]")
        .replace("policy: delta-asic", "policy: delta-asic\n    critical: 80");
    let mut rig = grace_rig(&yaml, 76.0);
    assert_eq!(rig.state("asic"), "failsafe");
    rig.asic_file(None);
    rig.tick();
    assert!(!rig.in_grace("asic"));
    let mut rig = grace_rig(&yaml, 74.0);
    assert_eq!(rig.state("asic"), "normal");
    rig.asic_file(None);
    rig.tick();
    assert!(rig.in_grace("asic"));
    assert_eq!(rig.state("asic"), "normal");
}

#[test]
fn no_grace_before_the_first_good_reading() {
    let mut rig = Rig::new(GRACE_RIG);
    assert_eq!(rig.tick(), 100.0);
    assert_eq!(rig.state("asic"), "failsafe");
    assert!(!rig.in_grace("asic"));
}

/// A sensor that is "not online yet" at start (the AS7772 ASIC: nothing can
/// read the chip until the PHY bring-up has the SDK up), with the CPU die as
/// the stand-in that keeps the fans honest meanwhile.
const WAIT_RIG: &str = r#"
sysfs_root: "{root}"
interval: 2s
profile: none
calm_period: 60s
runtime: { state_file: null, socket: null, calibration_file: null }
sensors:
  cpu_die: { type: hwmon, hwmon: max1617, input: temp2 }
  asic: { type: file, path: /run/asic/temperature, max_age: 20s, grace: 15s, startup_grace: 5m, critical: 100 }
outputs:
  fans: { type: hwmon, hwmon: as7760_fan, raw_min: 128 }
zones:
  cpu:
    outputs: [fans]
    sensors: [cpu_die]
    policy: { curve: { points: [[65, 53], [80, 75], [90, 100]], hysteresis: 3 } }
  asic:
    outputs: [fans]
    sensors: [asic]
    policy: delta-asic
"#;

fn wait_rig(yaml: &str) -> (Rig, PathBuf) {
    let mut cpu = PathBuf::new();
    let rig = Rig::build(yaml, true, |r| {
        cpu = r.fs.hwmon(20, "max1617", "platform/soc/i2c-1/1-0018", None);
        r.fs.set(&cpu, "temp2_input", "35000");
    });
    (rig, cpu)
}

impl Rig {
    fn waiting(&self, sensor: &str) -> bool {
        self.engine.status(self.t).sensors[sensor].waiting
    }
}

#[test]
fn a_sensor_that_is_not_online_yet_is_not_a_fault_and_the_other_zone_cools() {
    use fand::log::Level;
    fand::log::capture();
    let (mut rig, cpu) = wait_rig(WAIT_RIG);
    // No ASIC file for the first 70 s: no failsafe; the usual startup ramp
    // from 100 % runs down to the hardware minimum.
    let duties = rig.run(35);
    assert_eq!(duties[0], 100.0, "startup duty, not failsafe");
    // The fans end at the CPU zone's idle point (53 %); the waiting zone
    // itself asks only for the hardware minimum.
    assert!(approx(*duties.last().unwrap(), 53.0), "{duties:?}");
    assert!(approx(rig.zone_duty("asic"), 50.196), "{}", rig.zone_duty("asic"));
    assert_eq!(rig.state("asic"), "normal");
    assert_eq!(rig.state("cpu"), "normal");
    assert!(rig.waiting("asic"));
    let st = rig.engine.status(rig.t);
    assert_eq!(st.zones["asic"].waiting, vec!["asic".to_string()]);
    assert_eq!(st.zones["asic"].temperature, None);
    assert!(
        st.sensors["asic"].error.is_some(),
        "why there is no reading stays visible"
    );
    assert!(!st.failsafe);
    let prom = fand::daemon::prometheus(&st);
    assert!(prom.contains("fand_sensor_waiting{sensor=\"asic\"} 1"), "{prom}");
    assert!(prom.contains("fand_zone_failsafe{zone=\"asic\"} 0"), "{prom}");
    assert!(fand::daemon::summary(&st).contains("(waiting for asic)"));
    // The stand-in zone does the cooling meanwhile: a hot CPU raises the fans.
    rig.fs.set(&cpu, "temp2_input", "80000");
    rig.run(3);
    assert!(approx(rig.zone_duty("cpu"), 75.0), "{}", rig.zone_duty("cpu"));
    assert!(approx(rig.engine.output("fans").unwrap().duty.unwrap(), 75.0));
    rig.fs.set(&cpu, "temp2_input", "35000");
    // First reading: online, the zone follows its curve from then on.
    rig.asic_file(Some(90.0)); // delta-asic: 90 %
    rig.run(3);
    assert!(!rig.waiting("asic"));
    assert!(approx(rig.zone_duty("asic"), 90.0), "{}", rig.zone_duty("asic"));
    assert_eq!(rig.state("asic"), "normal");
    let log = fand::log::captured();
    assert!(!log.iter().any(|(l, _)| *l <= Level::Warn), "{log:?}");
    let info: Vec<&String> = log.iter().filter(|(l, _)| *l == Level::Info).map(|(_, m)| m).collect();
    assert!(
        info.iter().any(|m| m.starts_with("sensor asic: not online yet (")
            && m.ends_with("not a fault for up to 300s after the start")),
        "{info:?}"
    );
    assert!(
        info.iter()
            .any(|m| *m == "zone asic: waiting for asic (not online yet): only its floor meanwhile"),
        "{info:?}"
    );
    assert!(
        info.iter().any(|m| *m == "sensor asic: online after 76s, 90.0 C"),
        "{info:?}"
    );
    assert!(
        info.iter()
            .any(|m| *m == "zone asic: all sensors online, resuming curve delta-asic"),
        "{info:?}"
    );
}

#[test]
fn startup_grace_is_bounded_then_missing_data_is_a_fault() {
    use fand::log::Level;
    fand::log::capture();
    let (mut rig, _cpu) = wait_rig(&WAIT_RIG.replace("startup_grace: 5m", "startup_grace: 60s"));
    let mut first_fault = None;
    for _ in 0..40 {
        let now = rig.t;
        let d = rig.tick();
        if rig.state("asic") == "failsafe" {
            first_fault.get_or_insert((now, d));
        }
    }
    assert_eq!(
        first_fault,
        Some((62.0, 100.0)),
        "not a fault through 60 s, failsafe on the 62 s tick"
    );
    assert!(!rig.waiting("asic"));
    assert_eq!(rig.state("cpu"), "normal");
    let log = fand::log::captured();
    assert!(log.iter().any(
        |(l, m)| *l == Level::Info && m == "sensor asic: still no reading 62s after the start (startup_grace 60s)"
    ));
    assert!(
        log.iter()
            .any(|(l, m)| *l == Level::Warn && m.starts_with("sensor asic: "))
    );
    assert!(
        log.iter()
            .any(|(l, m)| *l == Level::Error && m.contains("zone asic: FAILSAFE"))
    );
    assert!(!log.iter().any(|(_, m)| m.contains("all sensors online")), "{log:?}");
    // It then recovers like any sensor: reading, calm period, curve.
    rig.asic_file(Some(50.0));
    rig.run(45);
    assert_eq!(rig.state("asic"), "normal");
}

#[test]
fn once_online_a_lost_sensor_is_a_fault_again_after_its_grace_only() {
    let (mut rig, _cpu) = wait_rig(WAIT_RIG);
    rig.asic_file(Some(50.0));
    rig.run(40);
    assert!(!rig.waiting("asic"));
    let steady = rig.tick();
    let last_good = rig.t - DT;
    // Hand-over gap (writer A stops, writer B starts): held by `grace`.
    rig.asic_file(None);
    for _ in 0..3 {
        assert_eq!(rig.tick(), steady);
        assert!(rig.in_grace("asic"));
    }
    rig.asic_file(Some(51.0));
    rig.run(5);
    assert_eq!(rig.state("asic"), "normal");
    assert!(!rig.in_grace("asic"));
    // Gone for good, still inside startup_grace (5 min after the start):
    // failsafe once the 15 s grace is over, never "waiting" again.
    assert!(rig.t < 300.0 && last_good < rig.t);
    rig.asic_file(None);
    let mut states = Vec::new();
    for _ in 0..10 {
        rig.tick();
        assert!(!rig.waiting("asic"));
        states.push(rig.state("asic"));
    }
    assert_eq!(states.iter().filter(|s| **s == "normal").count(), 7, "{states:?}");
    assert_eq!(*states.last().unwrap(), "failsafe");
    assert_eq!(rig.raw(), 255);
}

#[test]
fn an_early_loss_within_startup_grace_is_still_a_fault() {
    // Online at 10 s, lost at 20 s: startup_grace (60 s) must not hide that.
    let (mut rig, _cpu) = wait_rig(&WAIT_RIG.replace("grace: 15s, ", ""));
    rig.run(5);
    rig.asic_file(Some(50.0));
    rig.run(5);
    assert!(!rig.waiting("asic"));
    rig.asic_file(None);
    assert_eq!(rig.tick(), 100.0);
    assert_eq!(rig.state("asic"), "failsafe");
    assert!(!rig.waiting("asic"));
}

#[test]
fn waiting_duty_and_implausible_first_readings() {
    // Without a stand-in zone: a configured duty while waiting.
    let yaml = WAIT_RIG.replace("    policy: delta-asic", "    policy: delta-asic\n    waiting_duty: 70");
    let (mut rig, _cpu) = wait_rig(&yaml);
    rig.run(40);
    assert!(rig.waiting("asic"));
    assert!(approx(rig.zone_duty("asic"), 70.0), "{}", rig.zone_duty("asic"));
    assert_eq!(rig.engine.status(rig.t).zones["asic"].demand, Some(70.0));
    rig.asic_file(Some(50.0));
    rig.run(40);
    assert!(approx(rig.zone_duty("asic"), 50.196), "{}", rig.zone_duty("asic"));
    // A reading that is there but implausible is a fault at once.
    let (mut rig, _cpu) = wait_rig(WAIT_RIG);
    rig.asic_file(Some(400.0));
    assert_eq!(rig.tick(), 100.0);
    assert_eq!(rig.state("asic"), "failsafe");
    assert!(!rig.waiting("asic"));
    // min_sensors: a zone whose only other sensors are waiting is not short.
    let yaml = WAIT_RIG.replace("sensors: [asic]", "sensors: [asic, cpu_die]\n    min_sensors: 2");
    let (mut rig, _cpu) = wait_rig(&yaml);
    rig.run(10);
    assert_eq!(rig.state("asic"), "normal");
    assert_eq!(rig.engine.status(rig.t).zones["asic"].temperature, Some(35.0));
}

#[test]
fn configs_without_startup_grace_behave_as_before() {
    // The deployed AS7760 / CS8210 / DMC-12 / TN48M shape: no startup_grace,
    // so a sensor without a reading is a fault from the first tick.
    let mut rig = Rig::new(GRACE_RIG);
    assert_eq!(rig.tick(), 100.0);
    assert_eq!(rig.state("asic"), "failsafe");
    assert!(!rig.waiting("asic"));
    let st = rig.engine.status(rig.t);
    assert!(st.zones["asic"].waiting.is_empty());
    let c = Config::parse(&GRACE_RIG.replace("{root}", "/x")).unwrap();
    assert!(c.sensors["asic"].startup_grace.is_none());
    assert!(c.zones["asic"].waiting_duty.is_none());
    let err = Config::parse(
        &GRACE_RIG
            .replace("{root}", "/x")
            .replace("grace: 15s }", "grace: 15s, startup_grace: 2h }"),
    )
    .unwrap_err()
    .0;
    assert!(
        err.contains("sensors.asic: startup_grace 7200s is above 3600s"),
        "{err}"
    );
}
