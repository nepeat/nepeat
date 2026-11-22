//! The long-running service: control loop, signals, exit guard, control
//! socket, state/metrics files and systemd notification.

use crate::calibration::Calibration;
use crate::config::{Config, DutySpec};
use crate::engine::{Engine, ExecMode, Options, Status};
use crate::error::{Context, Error, Result};
use crate::notify::Notifier;
use crate::{log_debug, log_error, log_info, log_notice, log_warn};
use serde::Deserialize;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Write via a temporary file and rename, so readers never see half a file.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(d) = path.parent() {
        if !d.as_os_str().is_empty() {
            fs::create_dir_all(d).ctx(|| format!("creating {}", d.display()))?;
        }
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data).ctx(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).ctx(|| format!("renaming {} to {}", tmp.display(), path.display()))
}

/// Owns the engine and sets every output to the exit duty when dropped,
/// including while unwinding from a panic.
pub struct ExitGuard {
    pub engine: Engine,
    pub duty: f64,
    done: bool,
}

impl ExitGuard {
    pub fn new(engine: Engine, duty: f64) -> ExitGuard {
        ExitGuard {
            engine,
            duty,
            done: false,
        }
    }

    pub fn finish(&mut self) -> Vec<String> {
        if self.done {
            return Vec::new();
        }
        self.done = true;
        let now = self.engine.now();
        self.engine.shutdown(self.duty, now)
    }
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        if !self.done {
            log_error!("unexpected exit: setting fans to the exit duty ({}%)", self.duty);
            self.finish();
        }
    }
}

/// Register terminating signals on a flag. Signals whose default action
/// would kill the process without unwinding (TERM, INT, QUIT, HUP) are all
/// caught so the exit guard always runs.
pub fn install_signals() -> Result<(Arc<AtomicBool>, Arc<AtomicBool>)> {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1};
    let term = Arc::new(AtomicBool::new(false));
    let dump = Arc::new(AtomicBool::new(false));
    for sig in [SIGTERM, SIGINT, SIGQUIT] {
        signal_hook::flag::register(sig, term.clone()).ctx(|| format!("installing handler for signal {sig}"))?;
    }
    for sig in [SIGHUP, SIGUSR1] {
        signal_hook::flag::register(sig, dump.clone()).ctx(|| format!("installing handler for signal {sig}"))?;
    }
    Ok((term, dump))
}

pub fn load_calibration(cfg: &Config) -> Calibration {
    let Some(p) = &cfg.runtime.calibration_file else {
        return Calibration::default();
    };
    match Calibration::load(p) {
        Ok(c) => {
            if !c.outputs.is_empty() {
                log_info!(
                    "loaded RPM calibration for {} output(s) from {}",
                    c.outputs.len(),
                    p.display()
                );
            }
            c
        }
        Err(e) => {
            log_warn!("ignoring calibration: {e}");
            Calibration::default()
        }
    }
}

/// One-line summary for logs and systemd STATUS=.
pub fn summary(s: &Status) -> String {
    let mut parts = Vec::new();
    for (name, z) in &s.zones {
        let t = z.temperature.map_or("--".to_string(), |t| format!("{t:.1}C"));
        let state = match z.state {
            "normal" => String::new(),
            other => format!(" {}", other.to_uppercase()),
        };
        let ov = if z.override_duty.is_some() { " (override)" } else { "" };
        let wait = if z.waiting.is_empty() {
            String::new()
        } else {
            format!(" (waiting for {})", z.waiting.join(", "))
        };
        parts.push(format!("{name} {t} -> {:.0}%{state}{ov}{wait}", z.duty));
    }
    for (name, o) in &s.outputs {
        if let Some(d) = o.duty {
            let rpm: Vec<f64> = o.rotors.iter().filter_map(|r| r.rpm).collect();
            let rpm = if rpm.is_empty() {
                String::new()
            } else {
                format!(
                    " {:.0}-{:.0}rpm",
                    rpm.iter().cloned().fold(f64::INFINITY, f64::min),
                    rpm.iter().cloned().fold(0.0, f64::max)
                )
            };
            parts.push(format!("{name}={d:.0}%{rpm}"));
        }
    }
    parts.join("; ")
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Prometheus text exposition (for the node_exporter textfile collector).
pub fn prometheus(s: &Status) -> String {
    let mut out = String::new();
    let mut metric = |name: &str, help: &str, kind: &str, rows: Vec<(String, f64)>| {
        let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {kind}");
        for (labels, v) in rows {
            if labels.is_empty() {
                let _ = writeln!(out, "{name} {v}");
            } else {
                let _ = writeln!(out, "{name}{{{labels}}} {v}");
            }
        }
    };
    let zl = |z: &str| format!("zone=\"{}\"", esc(z));
    metric(
        "fand_up",
        "1 while the daemon is running",
        "gauge",
        vec![(String::new(), 1.0)],
    );
    metric(
        "fand_zone_failsafe",
        "1 if the zone is in failsafe (or recovering)",
        "gauge",
        s.zones
            .iter()
            .map(|(n, z)| (zl(n), f64::from(u8::from(z.state != "normal"))))
            .collect(),
    );
    metric(
        "fand_zone_temperature_celsius",
        "aggregated zone temperature",
        "gauge",
        s.zones
            .iter()
            .filter_map(|(n, z)| z.temperature.map(|t| (zl(n), t)))
            .collect(),
    );
    metric(
        "fand_zone_duty_percent",
        "duty requested by the zone",
        "gauge",
        s.zones.iter().map(|(n, z)| (zl(n), z.duty)).collect(),
    );
    let sl = |n: &str| format!("sensor=\"{}\"", esc(n));
    metric(
        "fand_sensor_temperature_celsius",
        "sensor reading",
        "gauge",
        s.sensors
            .iter()
            .filter_map(|(n, x)| x.value.map(|v| (sl(n), v)))
            .collect(),
    );
    metric(
        "fand_sensor_ok",
        "1 if the last sensor reading was usable",
        "gauge",
        s.sensors
            .iter()
            .map(|(n, x)| (sl(n), f64::from(u8::from(x.error.is_none()))))
            .collect(),
    );
    metric(
        "fand_sensor_in_grace",
        "1 while a failing sensor's last good value is held (grace)",
        "gauge",
        s.sensors
            .iter()
            .map(|(n, x)| (sl(n), f64::from(u8::from(x.in_grace))))
            .collect(),
    );
    metric(
        "fand_sensor_waiting",
        "1 while a sensor has had no reading since the start and is within its startup_grace",
        "gauge",
        s.sensors
            .iter()
            .map(|(n, x)| (sl(n), f64::from(u8::from(x.waiting))))
            .collect(),
    );
    let ol = |n: &str| format!("output=\"{}\"", esc(n));
    metric(
        "fand_output_duty_percent",
        "commanded output duty",
        "gauge",
        s.outputs
            .iter()
            .filter_map(|(n, o)| o.duty.map(|d| (ol(n), d)))
            .collect(),
    );
    metric(
        "fand_output_demand_percent",
        "duty requested by the zones before the hardware limits",
        "gauge",
        s.outputs
            .iter()
            .filter_map(|(n, o)| o.demand.map(|d| (ol(n), d)))
            .collect(),
    );
    metric(
        "fand_output_clamped",
        "1 while the request is below the hardware minimum and raised to it",
        "gauge",
        s.outputs
            .iter()
            .map(|(n, o)| (ol(n), f64::from(u8::from(o.clamped))))
            .collect(),
    );
    metric(
        "fand_output_raw",
        "raw value written",
        "gauge",
        s.outputs
            .iter()
            .filter_map(|(n, o)| o.raw.map(|r| (ol(n), f64::from(r))))
            .collect(),
    );
    metric(
        "fand_output_faults",
        "number of active output faults",
        "gauge",
        s.outputs.iter().map(|(n, o)| (ol(n), o.faults.len() as f64)).collect(),
    );
    let rl = |n: &str, r: &str| format!("output=\"{}\",rotor=\"{}\"", esc(n), esc(r));
    let rotors = || s.outputs.iter().flat_map(|(n, o)| o.rotors.iter().map(move |r| (n, r)));
    metric(
        "fand_fan_rpm",
        "measured rotor speed",
        "gauge",
        rotors()
            .filter_map(|(n, r)| r.rpm.map(|v| (rl(n, &r.label), v)))
            .collect(),
    );
    metric(
        "fand_fan_expected_rpm",
        "expected rotor speed at the commanded duty",
        "gauge",
        rotors()
            .filter_map(|(n, r)| r.expected.map(|v| (rl(n, &r.label), v)))
            .collect(),
    );
    metric(
        "fand_fan_fault",
        "rotor fault flag",
        "gauge",
        rotors()
            .map(|(n, r)| (rl(n, &r.label), f64::from(u8::from(r.fault))))
            .collect(),
    );
    out
}

// ---------------------------------------------------------------- socket

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase", deny_unknown_fields)]
pub enum Request {
    Status,
    Set {
        zone: Option<String>,
        duty: String,
        seconds: f64,
    },
    Clear {
        zone: Option<String>,
    },
}

fn bind_socket(path: &Path) -> Result<UnixListener> {
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(Error(format!(
                "another fand is already running (socket {} answers)",
                path.display()
            )));
        }
        fs::remove_file(path).ctx(|| format!("removing stale socket {}", path.display()))?;
    }
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).ctx(|| format!("creating {}", d.display()))?;
    }
    let l = UnixListener::bind(path).ctx(|| format!("binding {}", path.display()))?;
    l.set_nonblocking(true).ctx(|| "making the socket non-blocking")?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).ctx(|| format!("chmod {}", path.display()))?;
    Ok(l)
}

fn respond(engine: &mut Engine, line: &str) -> serde_json::Value {
    let now = engine.now();
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => return serde_json::json!({"ok": false, "error": format!("bad request: {e}")}),
    };
    let result = match req {
        Request::Status => return serde_json::json!({"ok": true, "status": engine.status(now)}),
        Request::Set { zone, duty, seconds } => DutySpec::parse(&duty)
            .map_err(Error)
            .and_then(|d| engine.set_override(zone.as_deref(), d, seconds, now)),
        Request::Clear { zone } => engine.clear_override(zone.as_deref()),
    };
    match result {
        Ok(msg) => serde_json::json!({"ok": true, "message": msg}),
        Err(e) => serde_json::json!({"ok": false, "error": e.0}),
    }
}

fn handle(stream: UnixStream, engine: &mut Engine) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let mut line = String::new();
    let read = BufReader::new((&stream).take(64 * 1024)).read_line(&mut line);
    if let Err(e) = read {
        log_debug!("control socket read: {e}");
        return;
    }
    let mut resp = respond(engine, line.trim()).to_string();
    resp.push('\n');
    let _ = (&stream).write_all(resp.as_bytes());
}

fn serve(l: &UnixListener, engine: &mut Engine) {
    for _ in 0..8 {
        match l.accept() {
            Ok((s, _)) => handle(s, engine),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return,
            Err(e) => {
                log_debug!("control socket accept: {e}");
                return;
            }
        }
    }
}

/// Client side: one request, one response.
pub fn request(socket: &Path, req: &serde_json::Value) -> Result<serde_json::Value> {
    let s = UnixStream::connect(socket).ctx(|| format!("connecting to {}", socket.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .ctx(|| "socket timeout")?;
    s.set_write_timeout(Some(Duration::from_secs(5)))
        .ctx(|| "socket timeout")?;
    let mut line = req.to_string();
    line.push('\n');
    (&s).write_all(line.as_bytes()).ctx(|| "sending request")?;
    let mut resp = String::new();
    BufReader::new((&s).take(16 << 20))
        .read_line(&mut resp)
        .ctx(|| "reading response")?;
    serde_json::from_str(&resp).ctx(|| "parsing response")
}

// ---------------------------------------------------------------- run

pub fn run(cfg: Config, force_dry_run: bool) -> Result<i32> {
    let interval = cfg.interval.0;
    let runtime = cfg.runtime.clone();
    let exit_duty = cfg.exit_duty.0;
    let mut opts = Options::new(&cfg);
    opts.dry_run |= force_dry_run;
    opts.exec_mode = ExecMode::Threaded;
    opts.calibration = load_calibration(&cfg);
    let dry = opts.dry_run;

    let (term, dump) = install_signals()?;
    let notifier = Notifier::from_env();
    if let Some(wd) = notifier.watchdog() {
        if wd.as_secs_f64() < 2.0 * interval {
            log_warn!(
                "watchdog timeout {:.1}s is less than two control intervals ({interval}s)",
                wd.as_secs_f64()
            );
        }
    }
    // Bind before touching any fan: a second instance must not fight the first.
    let listener = match &runtime.socket {
        Some(p) => Some(bind_socket(p)?),
        None => None,
    };

    let min_warnings = crate::hwmin::static_warnings(&cfg);
    let engine = Engine::new(cfg, opts)?;
    for w in min_warnings.iter().filter(|w| w.warn) {
        log_warn!("{w}");
    }
    log_notice!(
        "starting{}: {} zone(s), {} sensor(s), {} output(s), interval {interval}s, exit duty {exit_duty}%",
        if dry { " (dry run: nothing is written)" } else { "" },
        engine.zones.len(),
        engine.sensors.len(),
        engine.outputs.len(),
    );
    let mut guard = ExitGuard::new(engine, exit_duty);
    guard.engine.start();

    let mut first = true;
    let mut next = guard.engine.now();
    let mut last_summary = f64::NEG_INFINITY;
    let mut file_warned = false;
    'run: loop {
        let now = guard.engine.now();
        guard.engine.tick(now);
        let status = guard.engine.status(now);
        let line = summary(&status);
        if first {
            notifier.ready(&line);
            log_notice!("running: {line}");
            first = false;
            last_summary = now;
        } else {
            notifier.ping(&line);
        }
        if dump.swap(false, Ordering::Relaxed) || now - last_summary >= runtime.log_interval.0 {
            log_info!("{line}");
            last_summary = now;
        }
        let mut file_errs = Vec::new();
        if let Some(p) = &runtime.state_file {
            match serde_json::to_vec_pretty(&status) {
                Ok(mut v) => {
                    v.push(b'\n');
                    if let Err(e) = write_atomic(p, &v) {
                        file_errs.push(e.0);
                    }
                }
                Err(e) => file_errs.push(e.to_string()),
            }
        }
        if let Some(p) = &runtime.prometheus_file {
            if let Err(e) = write_atomic(p, prometheus(&status).as_bytes()) {
                file_errs.push(e.0);
            }
        }
        if !file_errs.is_empty() && !file_warned {
            log_warn!(
                "cannot write state/metrics (fan control is unaffected): {}",
                file_errs.join("; ")
            );
            file_warned = true;
        }

        next += interval;
        if next < now {
            next = now + interval;
        }
        loop {
            if term.load(Ordering::Relaxed) {
                break 'run;
            }
            if let Some(l) = &listener {
                serve(l, &mut guard.engine);
            }
            let now = guard.engine.now();
            if now >= next {
                break;
            }
            thread::sleep(Duration::from_secs_f64((next - now).min(0.05)));
        }
    }

    notifier.stopping();
    log_notice!("stopping: setting fans to the exit duty ({exit_duty}%)");
    let errs = guard.finish();
    if let Some(p) = &runtime.socket {
        let _ = fs::remove_file(p);
    }
    Ok(if errs.is_empty() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prometheus_escapes_and_has_types() {
        let mut s = Status {
            version: "0",
            uptime: 1.0,
            dry_run: false,
            ticks: 1,
            failsafe: false,
            zones: Default::default(),
            sensors: Default::default(),
            outputs: Default::default(),
        };
        s.sensors.insert(
            "we\"ird".into(),
            crate::engine::SensorStatus {
                value: Some(41.5),
                error: None,
                required: true,
                path: None,
                age: None,
                in_grace: false,
                waiting: false,
            },
        );
        let p = prometheus(&s);
        assert!(p.contains("# TYPE fand_sensor_temperature_celsius gauge"));
        assert!(
            p.contains("fand_sensor_temperature_celsius{sensor=\"we\\\"ird\"} 41.5"),
            "{p}"
        );
        assert!(p.contains("fand_up 1"));
    }

    #[test]
    fn atomic_write_replaces() {
        let fs = crate::fakesys::FakeSys::new("atomic");
        let p = fs.root.join("run/fand/state.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        assert!(!fs.root.join("run/fand/state.json.tmp").exists());
    }
}
