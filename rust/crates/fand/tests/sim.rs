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
    // the hardware minimum (raw 128 = 50.2 %): the output settles at 128.
    assert_eq!(rig.zone_duty("board"), 45.0);
    assert_eq!(rig.raw(), 128);
    assert!(approx(*duties.last().unwrap(), 50.196));
    // The ASIC zone (55-56 C on delta-asic = 35 %) is also at its floor.
    assert_eq!(rig.zone_duty("asic"), 45.0);
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
