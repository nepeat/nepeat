//! Closed-loop simulation of an Edgecore DMC-12-48R-AS0: fans on a board
//! CPLD at I2C 4-0068 (`type: i2c` output with `map`, `arm`, `expect` and
//! i2c tachs, see `fakesys::FakeCpld`), three lm75/tmp75 hwmon sensors in a
//! fake sysfs. Covers the WDT-flag handshake (at start, after a CPLD reset,
//! a flag that will not clear), an unexpected CPLD version, a dead bus, a
//! stalled fan, exit duty and the PI zone.

use fand::config::Config;
use fand::engine::{Engine, ExecMode, Options};
use fand::fakesys::{FakeCpld, FakeSys};
use fand::log::Level;
use std::path::PathBuf;
use std::sync::Arc;

const DT: f64 = 2.0;

/// The configuration the NixOS module (ai_re/nix/modules/thermal/dmc12-fan.nix)
/// generates, with a short calm period.
const DMC12: &str = r#"
sysfs_root: "{root}"
interval: 2s
profile: none
calm_period: 20s
runtime: { state_file: null, socket: null, calibration_file: null }
sensors:
  k2_uc:       { type: hwmon, hwmon: { name: lm75, device: "5-004a" }, plausible: [-10, 110] }
  sfp_console: { type: hwmon, hwmon: { name: tmp75, device: "7-0048" }, plausible: [-10, 110] }
  m2:          { type: hwmon, hwmon: { name: tmp75, device: "7-0049" }, plausible: [-10, 110] }
outputs:
  fans:
    type: i2c
    i2c: { bus: 4, address: "0x68", register: "0x50" }
    map: { points: [[1, 4.6], [63, 100]], min: 9 }
    expect: [{ register: "0x01", min: 135 }]
    arm: { register: "0x2a", mask: "0x02", value: 0 }
    tach:
      i2c: [{ register: "0x60", label: fan1 }, { register: "0x61", label: fan2 }, { register: "0x62", label: fan3 }]
      scale: 100
      max_rpm: 14400
      min_rpm: 1000
      follow_tolerance: 0.6
      follow_grace: 10s
zones:
  board:
    outputs: [fans]
    sensors: [k2_uc, sfp_console, m2]
    policy: { target: 45, kp: 4.6, ki: 0.015 }
    critical: 60
"#;

struct Rig {
    _fs: FakeSys,
    temps: Vec<PathBuf>,
    cpld: Arc<FakeCpld>,
    engine: Engine,
    t: f64,
}

impl Rig {
    fn new(prep: impl FnOnce(&FakeCpld)) -> Rig {
        let fs = FakeSys::new("dmc12");
        let mut temps = Vec::new();
        for (i, (name, dev)) in [("lm75", "5-004a"), ("tmp75", "7-0048"), ("tmp75", "7-0049")]
            .iter()
            .enumerate()
        {
            let d = fs.hwmon(i as u32, name, &format!("platform/i2c/{dev}"), None);
            fs.set(&d, "temp1_input", "30000");
            temps.push(d);
        }
        let cpld = Arc::new(FakeCpld::new(4, 0x68));
        prep(&cpld);
        let cfg = Config::parse(&DMC12.replace("{root}", fs.root.to_str().unwrap())).expect("config");
        let mut opts = Options::new(&cfg);
        opts.exec_mode = ExecMode::Inline;
        opts.i2c = Some(cpld.clone());
        let engine = Engine::new(cfg, opts).expect("engine");
        Rig {
            _fs: fs,
            temps,
            cpld,
            engine,
            t: 0.0,
        }
    }

    fn tick(&mut self) -> f64 {
        self.engine.tick(self.t);
        self.t += DT;
        self.engine.output("fans").unwrap().duty.unwrap_or(f64::NAN)
    }

    fn run(&mut self, n: usize) {
        for _ in 0..n {
            self.tick();
        }
    }

    fn step(&self) -> u8 {
        self.cpld.with(|s| s.step)
    }

    fn flag(&self) -> u8 {
        self.cpld.with(|s| s.flag)
    }

    fn state(&self) -> &'static str {
        self.engine.status(self.t).zones["board"].state
    }

    fn reasons(&self) -> String {
        self.engine.status(self.t).zones["board"].reasons.join("; ")
    }

    fn set_temp(&self, c: f64) {
        for d in &self.temps {
            std::fs::write(d.join("temp1_input"), format!("{}\n", (c * 1000.0) as i64)).unwrap();
        }
    }

    /// Registers written since the last call.
    fn writes(&self) -> Vec<(u8, u16)> {
        self.cpld.with(|s| std::mem::take(&mut s.writes))
    }
}

fn lines(level: Level) -> Vec<String> {
    fand::log::captured()
        .into_iter()
        .filter(|(l, _)| *l == level)
        .map(|(_, m)| m)
        .collect()
}

#[test]
fn arms_after_a_cpld_reset_then_settles_at_the_step_9_floor() {
    fand::log::capture();
    // Power-on state: flag 3, the CPLD runs the fans at full speed.
    let mut rig = Rig::new(|_| {});
    rig.tick();
    let w = rig.writes();
    assert_eq!(w.len(), 2, "{w:?}");
    assert_eq!(w[0], (0x2a, 0), "arm first");
    assert_eq!(
        w[1].0, 0x50,
        "then the duty (a target zone starts from 100 % and moves at once)"
    );
    assert_eq!(rig.flag(), 1, "cleared (reads 1 afterwards, as on u01)");
    let info = lines(Level::Info).join("\n");
    assert!(
        info.contains("output fans: arm register 4-0068:0x2a read 0x03, wrote 0x00: armed"),
        "{info}"
    );
    assert!(
        lines(Level::Error).is_empty(),
        "the first arm is normal: {:?}",
        lines(Level::Error)
    );
    assert_eq!(rig.state(), "normal");
    // 30 C, target 45: the PI goes to its floor, the hardware minimum.
    rig.run(60);
    assert_eq!(rig.step(), 9, "step 9 = 16.9 %");
    let o = rig.engine.output("fans").unwrap();
    assert!((o.duty.unwrap() - 16.91).abs() < 0.01, "{:?}", o.duty);
    assert!((o.min_duty - 16.91).abs() < 0.01);
    assert!(!o.clamped, "running at the minimum is normal");
    assert_eq!(rig.state(), "normal");
    // Flag 1 holds under mask 0x02: no further arm writes, only the duty.
    rig.writes();
    rig.run(5);
    let w = rig.writes();
    assert!(!w.is_empty() && w.iter().all(|&(r, v)| r == 0x50 && v == 9), "{w:?}");
    let st = rig.engine.status(rig.t);
    let fans = &st.outputs["fans"];
    assert_eq!(fans.path.as_deref(), Some("i2c 4-0068:0x50"));
    assert_eq!(fans.observed_raw, Some(9));
    let rpm: Vec<f64> = fans.rotors.iter().map(|r| r.rpm.unwrap()).collect();
    assert_eq!(rpm, vec![2400.0; 3]);
    assert_eq!(fans.rotors[0].input, "4-0068:0x60");
    assert_eq!(fans.rotors[2].label, "fan3");
}

#[test]
fn uboot_cleared_flag_is_left_alone() {
    let mut rig = Rig::new(|c| c.with(|s| s.flag = 1));
    rig.run(60);
    let w = rig.writes();
    assert!(
        w.iter().all(|&(r, _)| r == 0x50),
        "only the duty register is written: {w:?}"
    );
    assert_eq!(rig.step(), 9);
}

#[test]
fn heat_raises_the_duty_and_critical_is_failsafe() {
    let mut rig = Rig::new(|c| c.with(|s| s.flag = 1));
    // Long enough idle for the integral to relax to the floor (Ti = kp/ki
    // = 307 s; before that, heat starts from a higher operating point).
    rig.run(600);
    assert_eq!(rig.step(), 9);
    rig.set_temp(50.0); // 5 C over target: kp 4.6 -> +23 % at once
    rig.tick();
    let d = rig.engine.output("fans").unwrap().duty.unwrap();
    assert!(d > 35.0 && d < 50.0, "{d}");
    rig.run(60);
    let d2 = rig.engine.output("fans").unwrap().duty.unwrap();
    assert!(d2 > d, "the integral keeps pushing while hot: {d} -> {d2}");
    rig.set_temp(61.0);
    rig.tick();
    assert_eq!(rig.state(), "failsafe");
    assert_eq!(rig.step(), 63);
}

#[test]
fn a_cpld_reset_mid_run_is_rearmed_and_held_failsafe_for_the_calm_period() {
    fand::log::capture();
    let mut rig = Rig::new(|c| c.with(|s| s.flag = 1));
    rig.run(60);
    assert_eq!(rig.step(), 9);
    rig.writes();
    rig.cpld.reset(); // flag 3, step 9: the CPLD drives the fans at full speed
    rig.tick();
    let w = rig.writes();
    assert_eq!(
        &w[..2],
        &[(0x2a, 0), (0x50, 9)],
        "re-armed before the duty write: {w:?}"
    );
    assert_eq!(*w.last().unwrap(), (0x50, 63), "then the failsafe duty on the spot");
    assert_eq!(rig.flag(), 1);
    let err = lines(Level::Error).join("\n");
    assert!(
        err.contains("arm register 4-0068:0x2a read 0x03 (want 0x00 under mask 0x02): the hardware had taken control"),
        "{err}"
    );
    rig.tick();
    assert_eq!(rig.state(), "failsafe");
    assert!(rig.reasons().contains("had taken control"), "{}", rig.reasons());
    assert_eq!(rig.step(), 63);
    // Calm period 20 s, then back to the floor.
    rig.run(20);
    assert_eq!(rig.state(), "normal");
    rig.run(40);
    assert_eq!(rig.step(), 9);
}

#[test]
fn a_flag_that_will_not_clear_keeps_the_zone_failsafe_and_still_writes_full() {
    let mut rig = Rig::new(|c| c.with(|s| s.stuck_flag = true));
    rig.run(20);
    assert_eq!(rig.state(), "failsafe");
    assert!(
        rig.reasons().contains("the hardware keeps control"),
        "{}",
        rig.reasons()
    );
    assert_eq!(rig.step(), 63, "the failsafe duty is written anyway");
    assert_eq!(rig.flag(), 3);
}

#[test]
fn unexpected_cpld_version_writes_nothing() {
    // A pre-v7 CPLD (16 steps): the 64-step map does not apply.
    let mut rig = Rig::new(|c| c.with(|s| s.version = 0x80));
    rig.run(10);
    assert!(rig.writes().is_empty(), "not even the arm register");
    assert_eq!(rig.flag(), 3, "the CPLD's own failsafe stays in charge");
    assert_eq!(rig.state(), "failsafe");
    assert!(
        rig.reasons()
            .contains("expect 4-0068:0x01: reads 0x80, expected 0x87..0xff; not writing to this device"),
        "{}",
        rig.reasons()
    );
    assert!(rig.engine.shutdown(100.0, rig.t).len() == 1, "exit duty fails too");
    assert!(rig.writes().is_empty());
}

#[test]
fn i2c_errors_are_failsafe_and_recover() {
    let mut rig = Rig::new(|c| c.with(|s| s.flag = 1));
    rig.run(60);
    rig.cpld.with(|s| s.dead = true);
    rig.tick();
    rig.tick();
    assert_eq!(rig.state(), "failsafe");
    let r = rig.reasons();
    assert!(r.contains("Remote I/O error"), "{r}");
    let st = rig.engine.status(rig.t);
    assert!(st.outputs["fans"].rotors.iter().all(|r| r.error.is_some()));
    rig.cpld.with(|s| s.dead = false);
    rig.tick();
    assert_eq!(rig.step(), 63, "failsafe duty as soon as the bus answers");
    rig.run(25);
    assert_eq!(rig.state(), "normal");
}

#[test]
fn stalled_fan_is_failsafe() {
    let mut rig = Rig::new(|c| c.with(|s| s.flag = 1));
    rig.run(60);
    rig.cpld.with(|s| s.fan_factor[2] = 0.0);
    rig.tick();
    rig.tick();
    assert_eq!(rig.state(), "failsafe");
    assert!(
        rig.reasons().contains("fan3: 0 rpm is below the minimum 1000 rpm"),
        "{}",
        rig.reasons()
    );
    assert_eq!(rig.step(), 63);
}

#[test]
fn exit_duty_arms_and_writes_the_top_step() {
    let mut rig = Rig::new(|c| c.with(|s| s.flag = 1));
    rig.run(60);
    rig.writes();
    rig.cpld.reset();
    assert!(rig.engine.shutdown(100.0, rig.t).len() == 1, "the re-arm is reported");
    assert_eq!(rig.writes(), vec![(0x2a, 0), (0x50, 63)]);
    // Below the hardware minimum: raised to step 9.
    rig.engine.shutdown(5.0, rig.t);
    assert_eq!(rig.step(), 9);
}

#[test]
fn dry_run_reads_but_never_writes() {
    let mut rig = Rig::new(|_| {});
    rig.engine.dry_run = true;
    rig.run(10);
    assert!(rig.writes().is_empty());
    let st = rig.engine.status(rig.t);
    assert_eq!(st.outputs["fans"].observed_raw, Some(9));
    assert_eq!(st.outputs["fans"].rotors[0].rpm, Some(14400.0), "CPLD at full speed");
}

#[test]
fn measured_fan_curve_does_not_trip_the_follow_check() {
    // u01: 3200 rpm at step 22 (36.9 %), 60 % of the linear 14400 rpm model.
    let mut rig = Rig::new(|c| c.with(|s| s.flag = 1));
    rig.run(60);
    let t = rig.t;
    rig.engine
        .set_override(None, fand::config::DutySpec::Percent(36.9), 600.0, t)
        .unwrap();
    let real = 3200.0 / (14400.0 * 0.3694);
    rig.cpld.with(|s| s.fan_factor = vec![real; 3]);
    rig.run(30);
    assert_eq!(rig.step(), 22);
    assert_eq!(rig.state(), "normal", "{}", rig.reasons());
    let rpm = rig.engine.status(rig.t).outputs["fans"].rotors[0].rpm;
    assert_eq!(rpm, Some(3200.0));
    // A fan at a quarter of the model is still caught.
    rig.cpld.with(|s| s.fan_factor[1] = 0.25);
    rig.run(10);
    assert_eq!(rig.state(), "failsafe");
    assert!(
        rig.reasons().contains("fan2") && rig.reasons().contains("not following"),
        "{}",
        rig.reasons()
    );
}
