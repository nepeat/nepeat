//! Stalled-rotor recovery ("calm restart"), blinking LEDs and the degraded
//! rotor-fault policy, on a CS8210-style fan CPLD (`fakesys::FakeFanCpld`):
//! three slots plus a "force every fan to full speed" register (0x05) driven
//! as an i2c output and released before every dip.

use fand::config::Config;
use fand::engine::{Engine, ExecMode, Options};
use fand::fakesys::{FakeFanCpld, FakeSys};
use fand::log::Level;
use std::path::PathBuf;
use std::sync::Arc;

const TICK: f64 = 5.0;
const STEP: f64 = 0.25;
const SLOTS: usize = 3;

struct Opts {
    policy: &'static str,
    max_failed: usize,
    per_rotor: f64,
    recovery: bool,
    file: Option<PathBuf>,
    slots: usize,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            policy: "degraded",
            max_failed: 2,
            per_rotor: 0.0,
            recovery: true,
            file: None,
            slots: SLOTS,
        }
    }
}

fn slot(n: usize) -> String {
    let b = 0x20 + 0x10 * (n - 1);
    format!(
        r#"
  top{n}:
    type: i2c
    i2c: {{ bus: 31, address: "0x66", register: "0x{:02x}" }}
    raw_min: 127
    expect: [{{ register: "0x00", value: 3 }}]
    tach:
      i2c: [{{ register: "0x{b:02x}", label: front }}, {{ register: "0x{:02x}", label: rear }}]
      scale: 100
      max_rpm_by_label: {{ front: 19000, rear: 21000 }}
      min_rpm: 3000
    present: {{ register: "0x{:02x}", mask: "0x01", value: 0 }}
    led: {{ register: "0x{:02x}", mask: "0x03", ok: "0x01", fault: "0x02", absent: "0x02", blink: {{ off: "0x03", period: 1s }} }}
    isolate_on_rotor_fault: true
"#,
        b + 2,
        b + 1,
        b + 6,
        b + 4,
    )
}

fn config(o: &Opts, root: &str) -> String {
    let slots: String = (1..=o.slots).map(slot).collect();
    let names = (1..=o.slots).map(|n| format!("top{n}")).collect::<Vec<_>>().join(", ");
    let file = o
        .file
        .as_ref()
        .map_or("null".to_string(), |p| format!("\"{}\"", p.display()));
    let rec = if o.recovery {
        "stall_recovery: { release: [force], hold_raw: 0, hold: 10s, ramp: [{ raw: \"0x30\", for: 5s }], settle: 15s, max_attempts: 3, interval: 60s, max_temp: 45 }\n"
    } else {
        ""
    };
    let degraded = if o.policy == "degraded" {
        format!(
            "
    degraded_max_failed_rotors: {}
    degraded_duty_per_rotor: {}",
            o.max_failed, o.per_rotor
        )
    } else {
        String::new()
    };
    format!(
        r#"
sysfs_root: "{root}"
interval: 5s
profile: none
calm_period: 20s
startup_duty: 60
rotor_fault_policy: {}
runtime: {{ state_file: null, socket: null, calibration_file: null, recovery_file: {file} }}
{rec}sensors:
  board: {{ type: hwmon, hwmon: {{ name: lm75, device: "5-0048" }}, plausible: [-10, 110] }}
outputs:{slots}
  force:
    type: i2c
    i2c: {{ bus: 31, address: "0x66", register: "0x05" }}
    readback: off
    isolation_max_duty: 99
zones:
  board:
    outputs: [{names}, force]
    sensors: [board]
    policy: {{ curve: {{ points: [[40, 49.8], [55, 100]], hysteresis: 2 }} }}{degraded}
"#,
        o.policy
    )
}

struct Rig {
    fs: FakeSys,
    temp: PathBuf,
    cpld: Arc<FakeFanCpld>,
    engine: Engine,
    t: f64,
    next_tick: f64,
}

impl Rig {
    fn new(o: Opts) -> Rig {
        {
            let n = o.slots;
            Self::with(o, FakeSys::new("recovery"), Arc::new(FakeFanCpld::new(31, 0x66, n)))
        }
    }

    /// A rig on an existing tree and CPLD (a restart).
    fn with(o: Opts, fs: FakeSys, cpld: Arc<FakeFanCpld>) -> Rig {
        let d = fs.root.join("sys/class/hwmon/hwmon0");
        let temp = if d.exists() {
            d.join("temp1_input")
        } else {
            let d = fs.hwmon(0, "lm75", "platform/i2c/5-0048", None);
            fs.set(&d, "temp1_input", "30000");
            d.join("temp1_input")
        };
        let cfg = Config::parse(&config(&o, fs.root.to_str().unwrap())).expect("config");
        let mut opts = Options::new(&cfg);
        opts.exec_mode = ExecMode::Inline;
        opts.i2c = Some(cpld.clone());
        let engine = Engine::new(cfg, opts).expect("engine");
        Rig {
            fs,
            temp,
            cpld,
            engine,
            t: 0.0,
            next_tick: 0.0,
        }
    }

    /// Run the clock like the daemon: timers every STEP, a tick every TICK.
    fn advance(&mut self, secs: f64) {
        let end = self.t + secs;
        while self.t < end - 1e-9 {
            let t = self.t;
            self.cpld.with(|s| s.now = t);
            if t >= self.next_tick - 1e-9 {
                self.engine.tick(t);
                self.next_tick += TICK;
            } else {
                self.engine.timers(t);
            }
            self.t += STEP;
        }
    }

    fn ticks(&mut self, n: usize) {
        self.advance(n as f64 * TICK);
    }

    fn duties(&self) -> Vec<u8> {
        self.cpld.with(|s| s.duty.clone())
    }

    fn led(&self, slot: usize) -> u8 {
        self.cpld.with(|s| s.led[slot])
    }

    fn writes(&self) -> Vec<(u8, u16)> {
        self.cpld.with(|s| std::mem::take(&mut s.writes))
    }

    fn set_temp(&self, c: f64) {
        std::fs::write(&self.temp, format!("{}\n", (c * 1000.0) as i64)).unwrap();
    }

    fn status(&self) -> serde_json::Value {
        serde_json::to_value(self.engine.status(self.t)).unwrap()
    }

    fn zone(&self) -> String {
        self.status()["zones"]["board"]["state"].as_str().unwrap().to_string()
    }

    /// LED values seen over \`secs\` of clock.
    fn led_values(&mut self, slot: usize, secs: f64) -> Vec<u8> {
        let mut seen = Vec::new();
        let end = self.t + secs;
        while self.t < end {
            self.advance(STEP);
            let v = self.led(slot);
            if !seen.contains(&v) {
                seen.push(v);
            }
        }
        seen.sort_unstable();
        seen
    }
}

fn log(level: Level) -> Vec<String> {
    fand::log::captured()
        .into_iter()
        .filter(|(l, _)| *l == level)
        .map(|(_, m)| m)
        .collect()
}

fn all() -> String {
    fand::log::captured()
        .into_iter()
        .map(|(_, m)| m)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn calm_restart_releases_the_force_first_dips_every_fan_and_revives_a_stuck_rotor() {
    fand::log::capture();
    let mut rig = Rig::new(Opts::default());
    rig.ticks(6);
    assert_eq!(rig.zone(), "normal");
    assert_eq!(rig.duties(), vec![127, 127, 127]);
    rig.cpld.with(|s| s.stuck[1][1] = true);
    rig.writes();
    all();
    rig.ticks(1); // the stall is seen and the recovery starts in the same tick
    let w: Vec<(u8, u16)> = rig
        .writes()
        .into_iter()
        .filter(|(r, _)| [0x05, 0x22, 0x32, 0x42].contains(r))
        .collect();
    assert_eq!(
        &w[..4],
        &[(0x05, 0), (0x22, 0), (0x32, 0), (0x42, 0)],
        "force released first: {w:?}"
    );
    let l = all();
    assert!(
        l.contains("stall recovery attempt 1/3 for top2 (top2: rear: 0 rpm is below the minimum 3000 rpm at 50%): releasing force first, 3 fan outputs to raw 0x00 for 10s, then 0x30 for 5s, then normal control and a check after 15s"),
        "{l}"
    );
    let st = rig.status();
    assert_eq!(st["recovery"]["active"], true);
    assert_eq!(st["recovery"]["phase"], "hold");
    assert_eq!(st["recovery"]["targets"], serde_json::json!(["top2"]));
    assert_eq!(st["outputs"]["top2"]["recovering"], true);
    assert_eq!(st["outputs"]["top2"]["recovery_attempts"], 1);
    assert_eq!(st["outputs"]["top1"]["recovering"], false);
    // Blinks while handled; the others stay green; the dip is no fault.
    assert_eq!(rig.led_values(1, 2.0), vec![0xf2, 0xf3], "fault/off toggling");
    assert_eq!(rig.led(0), 0xf1);
    assert_eq!(rig.zone(), "normal", "no rotor faults during the dip");
    assert_eq!(rig.duties(), vec![0, 0, 0]);
    // Hold 10 s, then 0x30 for 5 s, by the timer (between ticks).
    rig.advance(8.0);
    assert_eq!(rig.duties(), vec![0x30, 0x30, 0x30]);
    assert!(all().contains("stall recovery: raw 0x30 for 5s"));
    rig.advance(5.0);
    assert_eq!(rig.duties(), vec![127, 127, 127], "back to normal control");
    assert_eq!(rig.status()["recovery"]["phase"], "settle");
    rig.advance(16.0);
    let n = log(Level::Notice).join("\n");
    assert!(
        n.contains("stall recovery: output top2 recovered after attempt 1 (front"),
        "{n}"
    );
    let st = rig.status();
    assert_eq!(st["recovery"]["active"], false);
    assert_eq!(st["recovery"]["last_result"], "top2 recovered");
    assert_eq!(st["outputs"]["top2"]["recovery_attempts"], 0);
    assert_eq!(rig.led(1), 0xf1);
    let p = fand::daemon::prometheus(&rig.engine.status(rig.t));
    assert!(
        p.contains("fand_recovery_active 0") && p.contains("fand_output_recovery_attempts{output=\"top2\"} 0"),
        "{p}"
    );
}

#[test]
fn a_dead_rotor_is_given_up_after_max_attempts_and_a_restart_does_not_dip_again() {
    fand::log::capture();
    let dir = FakeSys::new("recovery-file");
    let file = dir.root.join("run/recovery.json");
    let mut rig = Rig::new(Opts {
        file: Some(file.clone()),
        ..Opts::default()
    });
    rig.ticks(6);
    rig.cpld.with(|s| s.factor[1][1] = 0.0);
    rig.advance(600.0);
    let cap = fand::log::captured();
    let of = |lv: Level| -> Vec<String> { cap.iter().filter(|(l, _)| *l == lv).map(|(_, m)| m.clone()).collect() };
    let w = of(Level::Warn);
    let starts = w.iter().filter(|l| l.starts_with("stall recovery attempt ")).count();
    assert_eq!(starts, 3, "{w:#?}");
    assert!(w.iter().any(|l| l.contains("still stalled after attempt 1/3")));
    assert!(w.iter().any(|l| l.contains("next attempt in 60s at the earliest")));
    let e = of(Level::Error).join("\n");
    assert!(
        e.contains("stall recovery: output top2 GIVEN UP (rear 0 rpm after 3 attempts); isolating it"),
        "{e}"
    );
    assert!(e.contains("output top2: ISOLATED: stall recovery gave up"), "{e}");
    assert_eq!(rig.duties(), vec![127, 0, 127], "the others under closed-loop control");
    assert_eq!(
        rig.zone(),
        "degraded",
        "two failed rotors are within degraded_max_failed_rotors 2"
    );
    assert_eq!(rig.led_values(1, 2.0), vec![0xf2], "solid red");
    let st = rig.status();
    assert!(
        st["outputs"]["top2"]["recovery_given_up"]
            .as_str()
            .unwrap()
            .contains("after 3 attempts")
    );
    assert_eq!(st["zones"]["board"]["failed_rotors"], 2);
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["top2"]["attempts"], 3);
    // Restart: given up from the file, isolated at once, no dip.
    let (fs, cpld) = (rig.fs, rig.cpld);
    drop(rig.engine);
    all();
    let mut rig = Rig::with(
        Opts {
            file: Some(file.clone()),
            ..Opts::default()
        },
        fs,
        cpld,
    );
    rig.writes();
    rig.advance(400.0);
    assert!(!all().contains("stall recovery attempt"), "no dip after the restart");
    assert!(
        rig.writes().iter().all(|&(r, v)| !(r == 0x22 && v == 0)),
        "top1 never dipped"
    );
    assert_eq!(rig.duties()[1], 0);
    // Reinserted: attempts reset, released, the file forgets it.
    rig.cpld.with(|s| s.present[1] = false);
    rig.ticks(1);
    rig.cpld.with(|s| {
        s.present[1] = true;
        s.factor[1][1] = 1.0;
    });
    rig.ticks(8);
    assert!(rig.engine.output("top2").unwrap().isolated().is_none());
    assert_eq!(rig.led(1), 0xf1);
    assert_eq!(std::fs::read_to_string(&file).unwrap().trim(), "{}");
}

#[test]
fn guards_postpone_and_abort_and_the_interval_limits_dips() {
    fand::log::capture();
    let mut rig = Rig::new(Opts::default());
    rig.ticks(6);
    rig.set_temp(46.0);
    rig.ticks(1);
    rig.cpld.with(|s| s.stuck[0][0] = true);
    rig.ticks(3);
    let w = log(Level::Warn);
    let post: Vec<&String> = w.iter().filter(|l| l.contains("postponed")).collect();
    assert_eq!(post.len(), 1, "logged once: {w:#?}");
    assert!(post[0].contains(
        "stall recovery for top1 postponed: zone board: hottest sensor at 46.0 C, at or above the limit 45 C"
    ));
    assert!(rig.duties().iter().all(|&d| d > 0x30), "no dip while hot");
    assert_eq!(rig.led_values(0, 1.0), vec![0xf2, 0xf3], "pending: blinks");
    assert!(rig.status()["recovery"]["postponed"].as_str().is_some());
    // Cool: it starts; heat during the hold aborts it at the next tick.
    rig.set_temp(30.0);
    rig.ticks(1);
    assert_eq!(rig.duties(), vec![0, 0, 0]);
    rig.set_temp(47.0);
    rig.ticks(1);
    let w = log(Level::Warn).join("\n");
    assert!(
        w.contains("stall recovery attempt 1 for top1 aborted after 5s: zone board: hottest sensor at 47.0 C"),
        "{w}"
    );
    assert!(w.contains("(the attempt does not count)"));
    assert!(
        rig.duties().iter().all(|&d| d > 0x30),
        "normal control at once: {:?}",
        rig.duties()
    );
    assert_eq!(rig.status()["outputs"]["top1"]["recovery_attempts"], 0);
    // The interval runs from the abort.
    rig.set_temp(30.0);
    rig.ticks(10);
    assert!(
        rig.duties().iter().all(|&d| d > 0x30),
        "50 s: still within the 60 s interval"
    );
    rig.ticks(3);
    assert!(all().contains("stall recovery attempt 1/3 for top1"));
}

#[test]
fn a_missing_sensor_is_failsafe_and_blocks_recoveries() {
    fand::log::capture();
    let mut rig = Rig::new(Opts::default());
    rig.ticks(6);
    std::fs::remove_file(&rig.temp).unwrap();
    rig.cpld.with(|s| s.stuck[2][1] = true);
    rig.ticks(3);
    assert_eq!(
        rig.zone(),
        "failsafe",
        "sensor loss stays failsafe under the degraded policy"
    );
    let w = log(Level::Warn).join("\n");
    assert!(
        w.contains("stall recovery for top3 postponed: sensor board has no reading"),
        "{w}"
    );
    assert!(!w.contains("stall recovery attempt"));
    assert_eq!(rig.duties(), vec![255, 255, 255]);
}

#[test]
fn a_force_that_will_not_release_aborts_the_dip_and_counts() {
    fand::log::capture();
    let mut rig = Rig::new(Opts::default());
    rig.cpld.with(|s| {
        s.force = 0xff;
        s.force_stuck = true;
    });
    rig.ticks(6);
    rig.cpld.with(|s| s.stuck[1][0] = true);
    rig.ticks(1);
    let w = log(Level::Warn).join("\n");
    assert!(
        w.contains("aborted after 0s: output top1 reads raw 255 after writing 0 (is a force-to-max still on?); back to normal control"),
        "{w}"
    );
    assert_eq!(rig.status()["outputs"]["top2"]["recovery_attempts"], 1, "counted");
}

#[test]
fn degraded_rides_out_failed_modules_until_too_many_and_adds_per_rotor_duty() {
    fand::log::capture();
    let mut rig = Rig::new(Opts {
        recovery: false,
        per_rotor: 5.0,
        ..Opts::default()
    });
    rig.ticks(8);
    rig.cpld.with(|s| s.present[0] = false);
    rig.ticks(6);
    assert_eq!(rig.zone(), "degraded");
    let w = log(Level::Warn).join("\n");
    assert!(
        w.contains("zone board: DEGRADED (2 failed rotors): output top1: module absent; keeping curve (inline) with the remaining fans"),
        "{w}"
    );
    // 49.8 % + 2 x 5 % = 59.8 % -> raw 153.
    assert_eq!(rig.duties()[1], 153);
    let p = fand::daemon::prometheus(&rig.engine.status(rig.t));
    assert!(p.contains("fand_zone_degraded{zone=\"board\"} 1"), "{p}");
    // degraded is not failsafe for the consumers of the metric (ring-soak, dashboards)
    assert!(p.contains("fand_zone_failsafe{zone=\"board\"} 0"), "{p}");
    assert!(fand::daemon::summary(&rig.engine.status(rig.t)).contains("DEGRADED"));
    // A second module out: 4 > 2 failed rotors, failsafe.
    rig.cpld.with(|s| s.present[2] = false);
    rig.ticks(5);
    assert_eq!(rig.zone(), "failsafe");
    let r = rig.status()["zones"]["board"]["reasons"].to_string();
    assert!(
        r.contains("failed rotors, more than degraded_max_failed_rotors 2"),
        "{r}"
    );
    assert_eq!(rig.duties()[1], 255);
    // Both back: normal after the calm period and the ramp.
    rig.cpld.with(|s| {
        s.present[0] = true;
        s.present[2] = true;
    });
    rig.ticks(30);
    assert_eq!(rig.zone(), "normal");
    assert_eq!(rig.duties(), vec![127, 127, 127]);
}

#[test]
fn failsafe_policy_is_unchanged_by_default() {
    let mut rig = Rig::new(Opts {
        policy: "failsafe",
        recovery: false,
        ..Opts::default()
    });
    rig.ticks(6);
    rig.cpld.with(|s| s.present[0] = false);
    rig.ticks(1);
    assert_eq!(rig.zone(), "failsafe");
    assert!(rig.status()["zones"]["board"].get("failed_rotors").is_none());
}

#[test]
fn exit_turns_a_blinking_led_solid_and_leaves_the_others() {
    let mut rig = Rig::new(Opts::default());
    rig.ticks(6);
    rig.cpld.with(|s| s.stuck[1][1] = true);
    rig.ticks(1);
    assert!(rig.engine.output("top2").unwrap().led_blinking());
    rig.writes();
    rig.engine.shutdown(100.0, rig.t);
    let leds: Vec<(u8, u16)> = rig
        .writes()
        .into_iter()
        .filter(|(r, _)| [0x24, 0x34, 0x44].contains(r))
        .collect();
    assert_eq!(leds, vec![(0x34, 0xf2)], "only the blinking one, to solid fault");
}

#[test]
fn startup_follows_the_first_observation_not_the_power_on_green() {
    let mut rig = Rig::new(Opts::default());
    rig.cpld.with(|s| {
        s.led = vec![0xf1; SLOTS];
        s.factor[2][0] = 0.0;
    });
    rig.ticks(1);
    // A rotor that should come up after the start: blinking, not green.
    assert_eq!(rig.led_values(2, 2.0), vec![0xf2, 0xf3]);
    assert_eq!(rig.led(0), 0xf1);
    // Within one blink phase the timer does not touch the bus; a phase
    // change costs the identity check and one LED write (RMW: one read).
    let (r0, w0) = rig.cpld.with(|s| (s.reads, s.writes.len()));
    let t0 = rig.t - STEP; // the last timer call
    rig.engine.timers(t0 + 0.01);
    rig.engine.timers(t0 + 0.02);
    assert_eq!(rig.cpld.with(|s| (s.reads, s.writes.len())), (r0, w0));
    // Nothing blinks: the timer writes nothing between ticks.
    let mut quiet = Rig::new(Opts::default());
    quiet.ticks(2);
    quiet.writes();
    quiet.advance(4.0);
    assert!(quiet.writes().iter().all(|(r, _)| ![0x24, 0x34, 0x44].contains(r)));
}

/// A recovery file that says top1 was given up (its rear is dead), and a
/// rig of four slots on it whose top1 rear is indeed dead.
fn with_top1_given_up(max_failed: usize) -> (FakeSys, Rig) {
    let dir = FakeSys::new("swap");
    let file = dir.root.join("run/recovery.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        r#"{ "top1": { "attempts": 3, "failed": "rear 0 rpm after 3 attempts" } }"#,
    )
    .unwrap();
    let rig = Rig::new(Opts {
        file: Some(file),
        slots: 4,
        max_failed,
        ..Opts::default()
    });
    rig.cpld.with(|s| s.factor[0][1] = 0.0);
    (dir, rig)
}

/// Pull top2 and top3, then put them back unpowered (all four rotors
/// stuck: they only start when every fan is slow).
fn hot_swap(rig: &mut Rig) {
    rig.cpld.with(|s| {
        s.present[1] = false;
        s.present[2] = false;
    });
    rig.ticks(1);
    rig.cpld.with(|s| {
        s.present[1] = true;
        s.present[2] = true;
        s.stuck[1] = [true, true];
        s.stuck[2] = [true, true];
    });
}

fn failed_rotors(rig: &Rig) -> u64 {
    rig.status()["zones"]["board"]["failed_rotors"].as_u64().unwrap()
}

#[test]
fn hot_swapped_unpowered_modules_are_revived_promptly_and_the_zone_ends_degraded() {
    fand::log::capture();
    let (_d, mut rig) = with_top1_given_up(4);
    rig.ticks(6);
    assert!(
        rig.engine.output("top1").unwrap().isolated().is_some(),
        "given up before: isolated at once"
    );
    assert_eq!(rig.zone(), "degraded");
    assert_eq!(failed_rotors(&rig), 2);
    // Pulled: 2 + 4 absent rotors > 4, failsafe (cpld force at 0xff).
    hot_swap(&mut rig);
    assert_eq!(rig.zone(), "failsafe");
    assert_eq!(
        rig.cpld.with(|s| s.force),
        253,
        "99 %: isolation_max_duty while top1 is isolated"
    );
    rig.writes();
    all();
    // Back in, unpowered: pending, not failed; a dip within insert_delay
    // (5 s) plus a tick, although the zone is failsafe and an attempt for
    // top1 was made long ago (no interval for a reinsertion).
    rig.ticks(1);
    assert_eq!(failed_rotors(&rig), 2, "the reinserted rotors are pending");
    assert_eq!(rig.led_values(1, 1.0), vec![0xf2, 0xf3], "blinking");
    rig.ticks(1);
    let l = all();
    assert!(l.contains("stall recovery attempt 1/3 for top2, top3"), "{l}");
    let w: Vec<(u8, u16)> = rig
        .writes()
        .into_iter()
        .filter(|(r, _)| [0x05, 0x32, 0x42, 0x52].contains(r)) // top1 (0x22) is isolated at 0
        .collect();
    let start = w
        .iter()
        .position(|&(r, v)| r == 0x05 && v == 0)
        .expect("force released");
    assert!(
        w[..start].iter().all(|&(r, v)| r == 0x05 || v == 0xff),
        "no dip write before the release: {w:?}"
    );
    assert_eq!(&w[start + 1..start + 3], &[(0x32, 0), (0x42, 0)], "{w:?}");
    // The dip revives both; then normal control, the calm period, degraded.
    rig.advance(35.0);
    let n = log(Level::Notice).join("\n");
    assert!(n.contains("output top2 recovered after attempt 1"), "{n}");
    assert!(n.contains("output top3 recovered after attempt 1"), "{n}");
    rig.ticks(30);
    assert_eq!(rig.zone(), "degraded", "{}", rig.status()["zones"]["board"]);
    assert_eq!(failed_rotors(&rig), 2);
    assert_eq!(
        (rig.led(0), rig.led(1), rig.led(2), rig.led(3)),
        (0xf2, 0xf1, 0xf1, 0xf1)
    );
    assert_eq!(rig.duties(), vec![0, 127, 127, 127]);
}

#[test]
fn a_reinserted_module_that_stays_dead_is_given_up_then_counted() {
    for (max_failed, end) in [(4, "degraded"), (3, "failsafe")] {
        fand::log::capture();
        let (_d, mut rig) = with_top1_given_up(max_failed);
        rig.ticks(6);
        hot_swap(&mut rig);
        rig.cpld.with(|s| s.factor[2][1] = 0.0); // top3's rear is dead too
        rig.advance(600.0);
        let w = log(Level::Warn);
        let starts: Vec<&String> = w.iter().filter(|l| l.starts_with("stall recovery attempt ")).collect();
        assert_eq!(starts.len(), 3, "{starts:#?}");
        assert!(starts[1].contains("attempt 2/3 for top3"), "{starts:#?}");
        let st = rig.status();
        assert!(st["outputs"]["top3"]["recovery_given_up"].as_str().is_some());
        assert_eq!(st["outputs"]["top3"]["isolated"], true);
        assert_eq!(failed_rotors(&rig), 4, "counted once given up");
        assert_eq!(rig.zone(), end, "max_failed {max_failed}");
        assert_eq!(rig.led(1), 0xf1);
        assert_eq!(rig.led_values(2, 2.0), vec![0xf2], "solid red");
        // Known dead: no attempts and no dips any more.
        rig.advance(900.0);
        assert!(!all().contains("stall recovery attempt"), "max_failed {max_failed}");
    }
}

#[test]
fn a_failsafe_zone_still_gets_a_guarded_dip_for_a_new_insertion() {
    fand::log::capture();
    // top1 absent: 2 failed rotors > 1, failsafe for good, the force at 0xff
    // pins every duty register.
    let mut rig = Rig::new(Opts {
        slots: 4,
        max_failed: 1,
        ..Opts::default()
    });
    rig.cpld.with(|s| s.present[0] = false);
    rig.ticks(6);
    assert_eq!(rig.zone(), "failsafe");
    assert_eq!(rig.cpld.with(|s| s.force), 0xff);
    rig.cpld.with(|s| s.present[1] = false);
    rig.ticks(1);
    rig.cpld.with(|s| {
        s.present[1] = true;
        s.stuck[1] = [true, true];
    });
    rig.writes();
    rig.ticks(2);
    assert!(all().contains("stall recovery attempt 1/3 for top2"));
    let w: Vec<(u8, u16)> = rig
        .writes()
        .into_iter()
        .filter(|(r, _)| [0x05, 0x22, 0x32, 0x42, 0x52].contains(r))
        .collect();
    let start = w
        .iter()
        .position(|&(r, v)| r == 0x05 && v == 0)
        .expect("force released");
    assert_eq!(
        &w[start + 1..start + 5],
        &[(0x22, 0), (0x32, 0), (0x42, 0), (0x52, 0)],
        "{w:?}"
    );
    assert_eq!(rig.duties(), vec![0, 0, 0, 0], "the dip really happens (not pinned)");
    rig.advance(40.0);
    assert!(
        log(Level::Notice)
            .iter()
            .any(|l| l.contains("output top2 recovered after attempt 1"))
    );
    assert_eq!(rig.zone(), "failsafe", "back to failsafe after the dip");
    assert_eq!(rig.cpld.with(|s| s.force), 0xff, "and its force");
    assert_eq!(rig.led(1), 0xf1);
}

#[test]
fn a_hot_chassis_blocks_the_insertion_dip() {
    fand::log::capture();
    let (_d, mut rig) = with_top1_given_up(4);
    rig.ticks(6);
    rig.set_temp(46.0);
    hot_swap(&mut rig);
    rig.ticks(12);
    let w = log(Level::Warn);
    assert!(!w.iter().any(|l| l.starts_with("stall recovery attempt")), "{w:#?}");
    assert_eq!(
        w.iter()
            .filter(|l| l.contains("postponed: zone board: hottest sensor at 46.0 C"))
            .count(),
        1,
        "{w:#?}"
    );
    assert_eq!(failed_rotors(&rig), 2, "still pending, not failed");
    assert_eq!(rig.led_values(1, 1.0), vec![0xf2, 0xf3]);
    rig.set_temp(30.0);
    rig.ticks(2);
    assert!(all().contains("stall recovery attempt 1/3 for top2, top3"));
}
