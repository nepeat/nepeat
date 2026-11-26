//! End-to-end tests of the fand binary against a fake sysfs tree.

use fand::fakesys::FakeSys;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_fand");

fn tree(tag: &str) -> (FakeSys, PathBuf, PathBuf) {
    let fs = FakeSys::new(tag);
    let fan = fs.hwmon(1, "as7760_fan", "platform/i2c-2/2-0066", None);
    fs.set(&fan, "pwm1", "128");
    for i in 1..=4 {
        fs.set(
            &fan,
            &format!("fan{i}_input"),
            if i % 2 == 1 { "21000" } else { "18000" },
        );
        fs.set(
            &fan,
            &format!("fan{i}_label"),
            &format!("fan{}_{}", (i + 1) / 2, if i % 2 == 1 { "front" } else { "rear" }),
        );
        fs.set(&fan, &format!("fan{i}_fault"), "0");
    }
    let lm = fs.hwmon(2, "lm75", "platform/i2c-3/3-0048", Some("i2c@1/temperature-sensor@48"));
    fs.set(&lm, "temp1_input", "30000");
    let r = fs.root.display().to_string();
    let cfg = format!(
        r#"
sysfs_root: "{r}"
interval: 200ms
runtime:
  state_file: "{r}/run/state.json"
  socket: "{r}/run/fand.sock"
  prometheus_file: "{r}/run/metrics.prom"
  calibration_file: "{r}/var/calibration.json"
sensors:
  board: {{ type: hwmon, hwmon: {{ name: lm75, of_node: temperature-sensor@48 }} }}
outputs:
  fans:
    type: hwmon
    hwmon: as7760_fan
    raw_min: 128
    tach: {{ auto: true, max_rpm_by_label: {{ front: 21000, rear: 18000 }}, min_rpm: 1000, follow_tolerance: null }}
zones:
  board: {{ outputs: [fans], sensors: [board], policy: delta-balanced, ramp_down: 0, dwell: 1s }}
"#
    );
    let path = fs.write("etc/fand.yaml", &cfg);
    (fs, fan, path)
}

fn fand(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .env_remove("NOTIFY_SOCKET")
        .env_remove("JOURNAL_STREAM")
        .output()
        .expect("run fand");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn pwm(fan: &Path) -> String {
    std::fs::read_to_string(fan.join("pwm1"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn wait_for(what: &str, timeout: Duration, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn examples_pass_the_syntax_check() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    for ex in ["as7760.yaml", "x86-appliance.yaml", "dmc12.yaml"] {
        let (code, out, err) = fand(&["check", "--syntax-only", "--config", dir.join(ex).to_str().unwrap()]);
        assert_eq!(code, 0, "{ex}: {out}{err}");
        assert!(out.contains(": ok ("), "{out}");
    }
    let (code, out, _) = fand(&[
        "check",
        "--syntax-only",
        "--config",
        dir.join("x86-appliance.yaml").to_str().unwrap(),
    ]);
    assert!(!out.contains("warning:"), "{out}");
    let (code2, _, err) = fand(&["check", "--syntax-only", "--config", "/nonexistent/fand.yaml"]);
    assert_eq!(code, 0);
    let code = code2;
    assert_eq!(code, 1);
    assert!(err.contains("/nonexistent/fand.yaml"), "{err}");
}

#[test]
fn syntax_check_prints_hardware_minimum_warnings_and_exits_0() {
    // profile medium (45 %) and delta-balanced's low end are below raw_min 128.
    let (_fs, _fan, cfg) = tree("cli-hwmin");
    let (code, out, err) = fand(&["check", "--syntax-only", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains(": ok ("), "{out}");
    assert!(
        out.contains(
            "\nwarning: profile: profile medium (45%) is below the hardware minimum of output fans \
             (50.2%, raw_min 128 of 255); it runs at 50.2% instead (zone board)\n"
        ),
        "{out}"
    );
    // A built-in curve under the minimum is by design: info, not a warning.
    assert!(
        out.contains(
            "\ninfo: zones.board.policy: curve delta-balanced is flat at the hardware minimum of output fans \
             (50.2%, raw_min 128 of 255) below 42.6 C\n"
        ),
        "{out}"
    );
    assert_eq!(out.matches("warning:").count(), 1, "{out}");
    // The full check lists them too, and they do not count as problems.
    let (code, out, err) = fand(&["check", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("\nhardware minimum:\n  warning: profile: profile medium (45%)"),
        "{out}"
    );
    assert!(
        out.contains("(hardware minimum 50.2%, raw_min 128 of 255, max 100%)"),
        "{out}"
    );
}

#[test]
fn check_resolves_devices_read_only() {
    let (_fs, fan, cfg) = tree("cli-check");
    let (code, out, err) = fand(&["check", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("hwmon2/temp1_input  = 30.0 C"), "{out}");
    assert!(out.contains("fan1_front") && out.contains("max 21000 rpm"), "{out}");
    assert_eq!(pwm(&fan), "128", "check must not write");
}

/// Kills the daemon if the test fails half way, and shows its log.
struct Daemon {
    child: std::process::Child,
    log: PathBuf,
}

impl Daemon {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Wait for exit with a deadline.
    fn wait(&mut self, timeout: Duration) -> Option<std::process::ExitStatus> {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            if let Ok(Some(s)) = self.child.try_wait() {
                return Some(s);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if std::thread::panicking() {
            eprintln!("---- daemon log ----\n{}", self.log());
        }
    }
}

#[test]
fn daemon_notifies_serves_overrides_and_sets_exit_duty_on_sigterm() {
    let (fs, fan, cfg) = tree("cli-run");
    let notify_path = fs.root.join("notify.sock");
    // Drain continuously, like systemd does (the sandbox's per-netns
    // max_dgram_qlen is only 10, and the daemon never blocks on a full queue).
    let notify = UnixDatagram::bind(&notify_path).unwrap();
    notify.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let msgs = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (msgs, stop) = (msgs.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(n) = notify.recv(&mut buf) {
                    msgs.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                }
            }
        })
    };
    let msg = |i: usize| msgs.lock().unwrap().get(i).cloned();
    let log = fs.root.join("daemon.txt");
    let child = Command::new(BIN)
        .args(["run", "--config", cfg.to_str().unwrap()])
        .env("NOTIFY_SOCKET", &notify_path)
        .env("WATCHDOG_USEC", "5000000")
        .env_remove("WATCHDOG_PID")
        .env_remove("JOURNAL_STREAM")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut d = Daemon { child, log };
    wait_for("READY and a WATCHDOG ping", Duration::from_secs(10), || {
        msg(1).is_some()
    });
    let ready = msg(0).unwrap();
    assert!(ready.starts_with("READY=1\nSTATUS="), "{ready}");
    assert_eq!(pwm(&fan), "255", "first tick writes the startup duty");
    assert!(msg(1).unwrap().starts_with("WATCHDOG=1"));

    // A second instance must refuse before touching the fans.
    let (code, _, err) = fand(&["run", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("already running"), "{err}");

    // Ramps down to the hardware minimum (ramp_down 0 = unlimited).
    wait_for("duty to settle at raw 128", Duration::from_secs(10), || {
        pwm(&fan) == "128"
    });
    let (code, out, err) = fand(&["status", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let st: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(st["zones"]["board"]["state"], "normal");
    assert_eq!(st["outputs"]["fans"]["rotors"][0]["label"], "fan1_front");
    // The zone target includes the hardware minimum (45 % floor < raw_min 128).
    assert_eq!(st["zones"]["board"]["duty"], 50.2);
    assert_eq!(st["outputs"]["fans"]["clamped"], false);
    assert_eq!(st["outputs"]["fans"]["demand"], 50.2);
    assert_eq!(st["outputs"]["fans"]["hardware_min"], 50.2);

    // Below the hardware minimum: accepted, clamped, and the reply says so.
    let (code, out, err) = fand(&["set", "30%", "--zone", "board", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("override 30% for 600s on board; below the hardware minimum, raised to it on output fans"),
        "{out}"
    );
    let state = fs.root.join("run/state.json");
    wait_for("the override to be clamped", Duration::from_secs(5), || {
        std::fs::read_to_string(&state).is_ok_and(|s| s.contains("\"clamped\": true"))
    });
    assert_eq!(pwm(&fan), "128");

    let (code, out, err) = fand(&[
        "set",
        "70%",
        "--zone",
        "board",
        "--for",
        "1m",
        "--config",
        cfg.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("override 70% for 60s on board"), "{out}");
    wait_for("override to apply", Duration::from_secs(5), || pwm(&fan) == "179");
    let (code, _, err) = fand(&["set", "70%", "--zone", "nope", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("unknown zone"), "{err}");
    let (code, _, _) = fand(&["set", "clear", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0);
    wait_for("override to clear", Duration::from_secs(5), || pwm(&fan) == "128");
    // "fand release" reaches the daemon; this output does not isolate.
    let (code, _, err) = fand(&["release", "fans", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(
        err.contains("daemon: output fans has no isolate_on_rotor_fault"),
        "{err}"
    );
    let (code, _, err) = fand(&["release", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("usage: fand release <output>"), "{err}");

    let state = std::fs::read_to_string(fs.root.join("run/state.json")).unwrap();
    assert!(state.contains("\"board\""));
    for key in ["present", "led", "isolated"] {
        assert!(
            !state.contains(&format!("\"{key}\"")),
            "no {key} without the config: {state}"
        );
    }
    let prom = std::fs::read_to_string(fs.root.join("run/metrics.prom")).unwrap();
    assert!(
        prom.contains("fand_fan_rpm{output=\"fans\",rotor=\"fan1_front\"} 21000"),
        "{prom}"
    );

    let pid = d.child.id().to_string();
    let kill = Command::new("/bin/sh")
        .args(["-c", &format!("kill -TERM {pid}")])
        .output()
        .unwrap();
    assert!(kill.status.success(), "kill: {}", String::from_utf8_lossy(&kill.stderr));
    let status = d.wait(Duration::from_secs(10)).expect("daemon exits after SIGTERM");
    assert_eq!(status.code(), Some(0), "{}", d.log());
    assert_eq!(pwm(&fan), "255", "exit duty written on SIGTERM");
    std::thread::sleep(Duration::from_millis(300));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    reader.join().unwrap();
    let last = msgs.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(last.starts_with("STOPPING=1"), "{last}");
    assert!(!fs.root.join("run/fand.sock").exists(), "socket removed on exit");
    let log = d.log();
    assert!(log.contains("stopping: setting fans to the exit duty"), "{log}");
    for needle in [
        "fand warn: profile: profile medium (45%) is below the hardware minimum of output fans",
        "fand warn: override 30% for 600s on board; below the hardware minimum",
        "fand warn: output fans: requested 30% (zone board 30% override) is below its hardware minimum \
         (50.2%, raw_min 128 of 255); running at 50.2%",
        "fand info: output fans: requested 70% (zone board 70% override) is at or above its hardware minimum",
    ] {
        assert!(log.contains(needle), "missing {needle:?} in:\n{log}");
    }
    assert!(
        !log.contains("delta-balanced"),
        "built-in curves are not logged:\n{log}"
    );
    // Logging is on state changes, not per tick: a few seconds of running
    // with an override must stay a handful of lines.
    assert!(log.lines().count() < 30, "too chatty:\n{log}");
}

#[test]
fn failsafe_command_and_calibration() {
    let (fs, fan, cfg) = tree("cli-cal");
    let c = cfg.to_str().unwrap();
    let (code, _, err) = fand(&["failsafe", "--config", c]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(pwm(&fan), "255");
    let (code, _, _) = fand(&["failsafe", "--duty", "60", "--config", c]);
    assert_eq!(code, 0);
    assert_eq!(pwm(&fan), "153");
    let (code, _, err) = fand(&["failsafe", "--duty", "10", "--config", c]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(pwm(&fan), "128", "clamped to raw_min");
    assert!(
        err.contains(
            "output fans: 10% is below its hardware minimum (50.2%, raw_min 128 of 255); set to 50.2% (raw 128)"
        ),
        "{err}"
    );

    let (code, out, err) = fand(&[
        "calibrate",
        "--config",
        c,
        "--steps",
        "100,60",
        "--settle",
        "0s",
        "--samples",
        "1",
    ]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(pwm(&fan), "255", "calibration ends at the exit duty");
    let cal: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fs.root.join("var/calibration.json")).unwrap()).unwrap();
    assert_eq!(cal["outputs"]["fans"]["rotors"]["fan1_front"]["max_rpm"], 21000.0);
    assert_eq!(cal["outputs"]["fans"]["rotors"]["fan1_rear"]["points"][0][0], 60.0);
    // The calibration is picked up as the rotor model.
    let (_, out, _) = fand(&["check", "--config", c]);
    assert!(out.contains("calibrated"), "{out}");

    // A hot sensor aborts calibration, with fans left at the exit duty.
    let lm = fs.root.join("sys/class/hwmon/hwmon2");
    fs.set(&lm, "temp1_input", "75000");
    let (code, _, err) = fand(&[
        "calibrate",
        "--config",
        c,
        "--steps",
        "60",
        "--settle",
        "0s",
        "--samples",
        "1",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("aborting calibration"), "{err}");
    assert_eq!(pwm(&fan), "255");
}

#[test]
fn usage_errors() {
    let (code, out, _) = fand(&["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("usage: fand"));
    let (code, _, err) = fand(&["frobnicate"]);
    assert_eq!(code, 1);
    assert!(err.contains("unknown command"));
    let (code, _, err) = fand(&["run", "--bogus"]);
    assert_eq!(code, 1);
    assert!(err.contains("unknown option --bogus"));
    let (code, out, _) = fand(&["curves"]);
    assert_eq!(code, 0);
    assert!(out.contains("delta-balanced"));
}

/// `type: i2c` end to end against the register-file fake (`crate::i2c`):
/// check is read-only and reports identity and arm state; failsafe arms and
/// writes the top step.
#[test]
fn i2c_output_check_and_failsafe() {
    let fs = FakeSys::new("cli-i2c");
    let lm = fs.hwmon(1, "lm75", "platform/i2c-5/5-004a", None);
    fs.set(&lm, "temp1_input", "31000");
    let ver = fs.i2c_reg(4, 0x68, 0x01, 0x88);
    let flag = fs.i2c_reg(4, 0x68, 0x2a, 3);
    let step = fs.i2c_reg(4, 0x68, 0x50, 9);
    for r in 0x60..=0x62 {
        fs.i2c_reg(4, 0x68, r, 0x90);
    }
    let r = fs.root.display().to_string();
    let cfg = fs.write(
        "etc/fand.yaml",
        &format!(
            r#"
sysfs_root: "{r}"
runtime: {{ state_file: null, socket: null, calibration_file: null }}
profile: none
sensors:
  k2: {{ type: hwmon, hwmon: {{ name: lm75, device: 5-004a }} }}
outputs:
  fans:
    type: i2c
    i2c: {{ bus: 4, address: "0x68", register: "0x50" }}
    map: {{ points: [[1, 4.6], [63, 100]], min: 9 }}
    expect: [{{ register: "0x01", min: 135 }}]
    arm: {{ register: "0x2a", mask: "0x02", value: 0 }}
    tach: {{ i2c: [{{ register: "0x60" }}, {{ register: "0x61" }}, {{ register: "0x62" }}], scale: 100, max_rpm: 14400, min_rpm: 1000 }}
zones:
  board: {{ outputs: [fans], sensors: [k2], policy: {{ target: 45, kp: 4.6, ki: 0.015 }} }}
"#
        ),
    );
    let read = |p: &Path| std::fs::read_to_string(p).unwrap().trim().to_string();
    let c = cfg.to_str().unwrap();
    let (code, out, err) = fand(&["check", "--syntax-only", "--config", c]);
    assert_eq!(code, 0, "{out}{err}");
    let (code, out, err) = fand(&["check", "--config", c]);
    assert_eq!(code, 0, "{out}{err}");
    for needle in [
        "fans           i2c    i2c 4-0068:0x50 = 9 (16.9%)  (hardware minimum 16.9%, map.min 9 of 1..63, max 100%)",
        "expect 4-0068:0x01 = 0x88 (ok: 0x87..0xff)",
        "arm 4-0068:0x2a = 0x03 (not armed, want 0x00 under mask 0x02; fand run writes 0x00)",
        "fan1         4-0068:0x60 14400 rpm",
        "k2             hwmon",
    ] {
        assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
    }
    assert_eq!(
        (read(&flag), read(&step)),
        ("3".into(), "9".into()),
        "check writes nothing"
    );
    let (code, out, err) = fand(&["failsafe", "--config", c]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!((read(&flag), read(&step)), ("0".into(), "63".into()));
    assert!(
        err.contains("arm register 4-0068:0x2a read 0x03, wrote 0x00: armed"),
        "{err}"
    );
    // A device that is not the expected one: check fails, failsafe writes nothing.
    fs.i2c_reg(4, 0x68, 0x01, 0x80);
    fs.i2c_reg(4, 0x68, 0x50, 9);
    let (code, out, _) = fand(&["check", "--config", c]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("ERROR expect 4-0068:0x01: reads 0x80"), "{out}");
    let (code, _, err) = fand(&["failsafe", "--config", c]);
    assert_eq!(code, 1, "{err}");
    assert_eq!(read(&step), "9");
    let _ = ver;
}
