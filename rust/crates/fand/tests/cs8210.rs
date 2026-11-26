//! Fan modules with presence, a tray LED and stalled-rotor isolation, in
//! the style of the Celestica CS8210-32X (one fan CPLD per fan board, a
//! duty / tach / LED / presence register set per slot, see
//! `fakesys::FakeFanCpld`), with an exec "CPLD force" output that
//! would drive every fan at full speed at 100 % (`isolation_max_duty`).
//! Mirrors the config nix/nixos/module/cs8210/fan-control generates, on
//! three slots.

use fand::config::Config;
use fand::engine::{Engine, ExecMode, Options};
use fand::fakesys::{FakeFanCpld, FakeSys};
use fand::log::Level;
use std::sync::Arc;

const DT: f64 = 5.0;
const SLOTS: usize = 3;
const LED_REGS: [u8; SLOTS] = [0x24, 0x34, 0x44];

#[derive(Clone, Copy)]
struct Opts {
    isolate: bool,
    led_exit: bool,
}

const ISO: Opts = Opts {
    isolate: true,
    led_exit: false,
};
const NO_ISO: Opts = Opts {
    isolate: false,
    led_exit: false,
};

fn slot(n: usize, o: Opts) -> String {
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
    led: {{ register: "0x{:02x}", mask: "0x03", ok: "0x01", fault: "0x02", absent: "0x03"{} }}
{}"#,
        b + 2,
        b + 1,
        b + 6,
        b + 4,
        if o.led_exit { r#", exit: "0x03""# } else { "" },
        if o.isolate {
            "    isolate_on_rotor_fault: true\n"
        } else {
            ""
        },
    )
}

fn config(o: Opts) -> String {
    let slots: String = (1..=SLOTS).map(|n| slot(n, o)).collect();
    let names: Vec<String> = (1..=SLOTS).map(|n| format!("top{n}")).collect();
    let cap = if o.isolate { "    isolation_max_duty: 99\n" } else { "" };
    format!(
        r#"
sysfs_root: "{{root}}"
interval: 5s
profile: none
calm_period: 20s
startup_duty: 60
runtime: {{ state_file: null, socket: null, calibration_file: null }}
sensors:
  board: {{ type: hwmon, hwmon: {{ name: lm75, device: "5-0048" }}, plausible: [-10, 110] }}
outputs:{slots}
  force:
    type: exec
    command: ["/bin/sh", "-c", "echo $0 > '{{root}}/force.txt'", "{{percent}}"]
    timeout: 3s
    reassert: 5s
{cap}zones:
  board:
    outputs: [{}, force]
    sensors: [board]
    policy: {{ curve: {{ points: [[40, 49.8], [55, 100]], hysteresis: 2 }} }}
"#,
        names.join(", ")
    )
}

struct Rig {
    fs: FakeSys,
    cpld: Arc<FakeFanCpld>,
    engine: Engine,
    t: f64,
}

impl Rig {
    fn new(o: Opts, prep: impl FnOnce(&FakeFanCpld)) -> Rig {
        let fs = FakeSys::new("cs8210");
        let d = fs.hwmon(0, "lm75", "platform/i2c/5-0048", None);
        fs.set(&d, "temp1_input", "30000");
        let cpld = Arc::new(FakeFanCpld::new(31, 0x66, SLOTS));
        prep(&cpld);
        let cfg = Config::parse(&config(o).replace("{root}", fs.root.to_str().unwrap())).expect("config");
        let mut opts = Options::new(&cfg);
        opts.exec_mode = ExecMode::Inline;
        opts.i2c = Some(cpld.clone());
        let engine = Engine::new(cfg, opts).expect("engine");
        Rig {
            fs,
            cpld,
            engine,
            t: 0.0,
        }
    }

    fn tick(&mut self) {
        self.engine.tick(self.t);
        self.t += DT;
    }

    fn run(&mut self, n: usize) {
        for _ in 0..n {
            self.tick();
        }
    }

    fn duty(&self, slot: usize) -> u8 {
        self.cpld.with(|s| s.duty[slot])
    }

    fn led(&self, slot: usize) -> u8 {
        self.cpld.with(|s| s.led[slot])
    }

    fn led_writes(&self) -> Vec<(u8, u16)> {
        self.cpld.with(|s| {
            let w: Vec<(u8, u16)> = s.writes.iter().copied().filter(|(r, _)| LED_REGS.contains(r)).collect();
            s.writes.clear();
            w
        })
    }

    fn zone(&self) -> &'static str {
        self.engine.status(self.t).zones["board"].state
    }

    fn isolated(&self, out: &str) -> bool {
        self.engine.output(out).unwrap().isolated().is_some()
    }

    fn force(&self) -> String {
        std::fs::read_to_string(self.fs.root.join("force.txt"))
            .unwrap_or_default()
            .trim()
            .to_string()
    }
}

fn lines(level: Level) -> Vec<String> {
    fand::log::captured()
        .into_iter()
        .filter(|(l, _)| *l == level)
        .map(|(_, m)| m)
        .collect()
}

fn all_lines() -> String {
    fand::log::captured()
        .into_iter()
        .map(|(_, m)| m)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn led_follows_faults_and_presence_and_writes_only_on_change() {
    fand::log::capture();
    let mut rig = Rig::new(NO_ISO, |_| {});
    rig.tick();
    let w = rig.led_writes();
    assert_eq!(
        w,
        vec![(0x24, 0xf1), (0x34, 0xf1), (0x44, 0xf1)],
        "green, upper bits kept (RMW)"
    );
    assert!(!all_lines().contains("LED"), "the first ok is not logged");
    rig.run(5);
    assert!(rig.led_writes().is_empty(), "no change, no write");
    assert_eq!(rig.zone(), "normal");

    // Rear rotor of top2 stalls: red, failsafe for everyone (as before).
    rig.cpld.with(|s| s.factor[1][1] = 0.0);
    rig.tick();
    assert_eq!(rig.led_writes(), vec![(0x34, 0xf2)]);
    assert!(all_lines().contains("output top2: LED ok -> fault (0x02)"));
    assert_eq!(rig.zone(), "failsafe");
    assert_eq!(
        (rig.duty(0), rig.duty(1), rig.duty(2)),
        (255, 255, 255),
        "no isolation configured"
    );
    rig.run(3);
    assert!(rig.led_writes().is_empty());

    // Pulled: absent (off here), a fault of the output, rotors not checked.
    rig.cpld.with(|s| s.present[1] = false);
    rig.tick();
    assert_eq!(rig.led_writes(), vec![(0x34, 0xf3)]);
    let log = all_lines();
    assert!(
        log.contains("output top2: module absent (31-0066:0x36 = 0x03)"),
        "{log}"
    );
    assert!(log.contains("output top2: LED fault -> absent (0x03)"), "{log}");
    let st = rig.engine.status(rig.t);
    let top2 = &st.outputs["top2"];
    assert_eq!(top2.present, Some(Some(false)));
    assert_eq!(
        top2.faults,
        vec!["module absent".to_string()],
        "no rotor faults while absent"
    );
    assert_eq!(
        top2.led.as_ref().map(|l| (l.state, l.raw, l.applied)),
        Some(("absent", 3, true))
    );
    assert!(
        st.zones["board"]
            .reasons
            .iter()
            .any(|r| r == "output top2: module absent")
    );

    // Back in, healthy: green; the zone leaves failsafe after its calm period.
    rig.cpld.with(|s| {
        s.factor[1] = [1.0, 1.0];
        s.present[1] = true;
    });
    rig.tick();
    assert_eq!(rig.led_writes(), vec![(0x34, 0xf1)]);
    let log = all_lines();
    assert!(log.contains("output top2: module present again"), "{log}");
    assert!(log.contains("output top2: LED absent -> ok (0x01)"), "{log}");
    rig.run(6);
    assert_eq!(rig.zone(), "normal");
    rig.run(20); // dwell, then down at 1 %/s
    assert_eq!(rig.duty(1), 127);
}

#[test]
fn led_is_reasserted_after_a_readback_mismatch() {
    fand::log::capture();
    let mut rig = Rig::new(NO_ISO, |_| {});
    rig.run(2);
    rig.led_writes();
    rig.cpld.with(|s| s.led[0] = 0x52); // someone else wrote red (and other bits)
    rig.run(fand::engine::LED_READBACK_TICKS as usize + 1);
    assert_eq!(rig.led(0), 0x51, "green again, the other bits kept");
    assert_eq!(rig.led_writes(), vec![(0x24, 0x51)], "one rewrite, only that LED");
    let n = lines(Level::Notice);
    assert!(
        n.iter().any(|l| l
            == "output top1: LED register 31-0066:0x24 reads 0x52, want 0x01 (ok) under mask 0x03: writing it again"),
        "{n:?}"
    );
}

#[test]
fn dry_run_and_a_failing_expect_write_no_led() {
    let mut rig = Rig::new(ISO, |c| c.with(|s| s.factor[1][1] = 0.0));
    rig.engine.dry_run = true;
    rig.run(10);
    assert!(rig.cpld.with(|s| s.writes.is_empty()), "nothing at all in a dry run");
    let st = rig.engine.status(rig.t);
    let l = st.outputs["top2"].led.as_ref().unwrap();
    assert_eq!((l.state, l.applied), ("fault", false), "computed, not written");
    assert_eq!(st.outputs["top2"].isolated, Some(true), "isolation is computed too");
    assert_eq!(rig.led(1), 0xf3);

    let mut rig = Rig::new(NO_ISO, |c| c.with(|s| s.version = 4));
    rig.run(5);
    assert!(
        rig.cpld.with(|s| s.writes.is_empty()),
        "an unexpected CPLD gets no LED write either"
    );
    assert_eq!(rig.zone(), "failsafe");
}

#[test]
fn a_stalled_rotor_isolates_its_module_and_the_rest_run_failsafe() {
    fand::log::capture();
    let mut rig = Rig::new(ISO, |_| {});
    rig.run(6);
    assert_eq!(rig.zone(), "normal");
    assert_eq!((rig.duty(1), rig.force().as_str()), (127, "50"));
    rig.cpld.with(|s| s.factor[1][1] = 0.0);
    rig.tick();
    assert!(rig.isolated("top2"));
    assert_eq!((rig.duty(0), rig.duty(1), rig.duty(2)), (255, 0, 255));
    assert_eq!(rig.force(), "99", "the CPLD force must not drive the isolated module");
    assert_eq!(rig.led(1), 0xf2);
    let e = lines(Level::Error).join("\n");
    assert!(
        e.contains(
            "output top2: ISOLATED: rear: 0 rpm is below the minimum 3000 rpm at 50%; writing raw 0 to it instead of any zone or failsafe duty"
        ),
        "{e}"
    );
    let st = rig.engine.status(rig.t);
    let top2 = &st.outputs["top2"];
    assert_eq!(top2.isolated, Some(true));
    assert_eq!(top2.isolated_since, Some(30.0));
    assert!(top2.isolated_reason.as_deref().unwrap().contains("rear: 0 rpm"));
    assert_eq!(top2.duty, Some(0.0));
    let j = serde_json::to_value(&st).unwrap();
    assert_eq!(j["outputs"]["top2"]["present"], serde_json::json!(true));
    assert_eq!(j["outputs"]["top2"]["led"]["state"], "fault");
    assert!(
        j["outputs"]["force"].get("isolated").is_none(),
        "absent keys for outputs without the features"
    );
    assert!(j["outputs"]["force"].get("present").is_none());
    let sum = fand::daemon::summary(&st);
    assert!(sum.contains("top2=0% ") && sum.contains("rpm ISOLATED"), "{sum}");

    // Latched: at duty 0 it reads 0 rpm, which no longer counts as a rotor
    // fault, but the isolation itself keeps the zone failsafe.
    rig.cpld.with(|s| s.factor[1][1] = 1.0);
    rig.run(20);
    assert!(rig.isolated("top2"));
    assert_eq!((rig.duty(0), rig.duty(1)), (255, 0));
    assert_eq!(rig.zone(), "failsafe");
    assert!(
        rig.engine.status(rig.t).zones["board"]
            .reasons
            .iter()
            .any(|r| r.starts_with("output top2: isolated: rear"))
    );

    // Pulled: still isolated (released on the way back in), LED absent.
    rig.cpld.with(|s| s.present[1] = false);
    rig.tick();
    assert!(rig.isolated("top2"));
    assert_eq!(rig.led(1), 0xf3);

    // Reinserted, but unpowered (both rotors 0): not isolated during the
    // grace, the zone's failsafe duty goes to it; after the grace, isolated.
    rig.cpld.with(|s| {
        s.factor[1] = [0.0, 0.0];
        s.present[1] = true;
    });
    fand::log::captured();
    rig.tick();
    let n = lines(Level::Notice).join("\n");
    assert!(
        n.contains("output top2: isolation released (module reinserted); 15s grace"),
        "{n}"
    );
    assert!(!rig.isolated("top2"));
    assert_eq!(rig.duty(1), 255);
    assert_eq!(rig.led(1), 0xf2, "red: its rotors fault");
    rig.run(2);
    assert!(!rig.isolated("top2"), "10 s after the edge: still proving itself");
    rig.tick();
    assert!(rig.isolated("top2"), "15 s: isolated again");
    assert_eq!(rig.duty(1), 0);

    // Out, and in again powered: released, healthy, everything back to normal.
    rig.cpld.with(|s| s.present[1] = false);
    rig.tick();
    rig.cpld.with(|s| {
        s.factor[1] = [1.0, 1.0];
        s.present[1] = true;
    });
    rig.tick();
    assert!(!rig.isolated("top2"));
    rig.run(30);
    assert!(!rig.isolated("top2"));
    assert_eq!(rig.zone(), "normal");
    assert_eq!((rig.duty(0), rig.duty(1), rig.duty(2)), (127, 127, 127));
    assert_eq!(rig.force(), "50");
    assert_eq!((rig.led(0), rig.led(1), rig.led(2)), (0xf1, 0xf1, 0xf1));
    let n = lines(Level::Notice).join("\n");
    assert!(
        n.contains("output force: no output isolated, isolation_max_duty no longer applies"),
        "{n}"
    );
}

#[test]
fn no_isolation_within_the_grace_after_the_start() {
    let mut rig = Rig::new(ISO, |c| c.with(|s| s.factor[2][0] = 0.0));
    rig.run(3); // t = 0, 5, 10
    assert!(!rig.isolated("top3"));
    assert_eq!(rig.duty(2), 255, "failsafe meanwhile, as without isolation");
    rig.tick(); // t = 15
    assert!(rig.isolated("top3"));
    assert_eq!(rig.duty(2), 0);
}

#[test]
fn release_by_hand_gives_the_grace_again() {
    let mut rig = Rig::new(ISO, |_| {});
    rig.run(4);
    rig.cpld.with(|s| s.factor[0][0] = 0.0);
    rig.tick();
    assert!(rig.isolated("top1"));
    let t = rig.t;
    assert_eq!(
        rig.engine.release("top1", t).unwrap(),
        "output top1: isolation released"
    );
    assert!(rig.engine.release("top1", t).unwrap_err().0.contains("is not isolated"));
    assert!(rig.engine.release("top9", t).unwrap_err().0.contains("unknown output"));
    assert!(
        rig.engine
            .release("force", t)
            .unwrap_err()
            .0
            .contains("no isolate_on_rotor_fault")
    );
    rig.run(3);
    assert!(!rig.isolated("top1"));
    assert_eq!(rig.duty(0), 255);
    rig.tick();
    assert!(rig.isolated("top1"), "still stalled after the grace");
}

#[test]
fn exit_keeps_an_isolated_module_off_and_leaves_leds_unless_exit_is_set() {
    let mut rig = Rig::new(ISO, |_| {});
    rig.run(4);
    rig.cpld.with(|s| s.factor[1][0] = 0.0);
    rig.tick();
    assert!(rig.isolated("top2"));
    rig.led_writes();
    let errs = rig.engine.shutdown(100.0, rig.t);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!((rig.duty(0), rig.duty(1), rig.duty(2)), (255, 0, 255));
    assert!(rig.led_writes().is_empty(), "LEDs left as they are");
    assert_eq!(rig.led(1), 0xf2);

    let mut rig = Rig::new(
        Opts {
            isolate: false,
            led_exit: true,
        },
        |_| {},
    );
    rig.run(2);
    rig.engine.shutdown(100.0, rig.t);
    assert_eq!((rig.led(0), rig.led(1), rig.led(2)), (0xf3, 0xf3, 0xf3), "exit value");
}

#[test]
fn unreadable_presence_is_unknown_not_a_fault() {
    fand::log::capture();
    let mut rig = Rig::new(ISO, |c| c.with(|s| s.present_unreadable[0] = true));
    rig.run(5);
    assert_eq!(rig.zone(), "normal");
    let st = rig.engine.status(rig.t);
    assert_eq!(st.outputs["top1"].present, Some(None));
    assert_eq!(
        serde_json::to_value(&st).unwrap()["outputs"]["top1"]["present"],
        serde_json::Value::Null
    );
    assert_eq!(rig.led(0), 0xf1, "checked as present");
    let w = lines(Level::Warn);
    assert_eq!(
        w.iter().filter(|l| l.contains("output top1: presence unknown")).count(),
        1,
        "logged once: {w:?}"
    );
    rig.cpld.with(|s| s.present_unreadable[0] = false);
    rig.tick();
    assert!(
        lines(Level::Info)
            .iter()
            .any(|l| l == "output top1: presence readable again: present")
    );
}

/// The prometheus file gains the new metrics only for outputs that have them.
#[test]
fn metrics_for_presence_isolation_and_leds() {
    let mut rig = Rig::new(ISO, |_| {});
    rig.run(2);
    let p = fand::daemon::prometheus(&rig.engine.status(rig.t));
    assert!(p.contains("fand_output_present{output=\"top1\"} 1"), "{p}");
    assert!(p.contains("fand_output_isolated{output=\"top1\"} 0"), "{p}");
    assert!(p.contains("fand_output_led_fault{output=\"top1\"} 0"), "{p}");
    assert!(!p.contains("fand_output_present{output=\"force\"}"), "{p}");
}
