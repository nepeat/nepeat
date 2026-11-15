//! The control engine: sensors -> zones -> outputs, one tick at a time.
//!
//! Every tick:
//! 1. read all sensors (scale, offset, plausibility, staleness);
//! 2. read output feedback (tach RPM, fault flags) against the duty that
//!    was commanded, and collect output faults (including the last write);
//! 3. each zone aggregates its sensors, collects faults, runs its failsafe
//!    guard, then its policy, floor and ramp, producing a duty;
//! 4. each output takes the max duty of the zones driving it, clamps it to
//!    its hardware range and writes it (with readback).
//!
//! Time is passed in explicitly (seconds since an epoch) so the engine can
//! be driven by a simulated clock in tests.

use crate::calibration::{Calibration, OutputCal, RotorModel};
use crate::config::{
    Aggregate, Config, DEFAULT_KD, DEFAULT_KI, DEFAULT_KP, DutySpec, FaultAction, HwmonSelector, OutputConfig,
    OutputKind, Profile, ProfileDuties, RawMap, Readback, SensorConfig, SensorKind, ZoneConfig,
};
use crate::control::{Ema, FailGuard, Hysteresis, Pid, Ramp, Transition};
use crate::curves::Curve;
use crate::error::{Error, Result};
use crate::exec::{self, Parser};
use crate::hwmin;
use crate::i2c::{self, Bus};
use crate::sysfs::{self, ReadError};
use crate::{log_error, log_info, log_notice, log_warn};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// Seconds between attempts to resolve a missing device.
pub const RESOLVE_RETRY: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    /// Run exec sensors synchronously from `tick` when their interval is due
    /// (tests, `check`, `calibrate`).
    Inline,
    /// One worker thread per exec sensor; `tick` only reads the latest
    /// result, so a slow vendor tool never stalls the control loop.
    Threaded,
}

pub struct Options {
    pub root: PathBuf,
    pub dry_run: bool,
    pub exec_mode: ExecMode,
    pub calibration: Calibration,
    /// Time origin for `now` values (worker threads timestamp with it).
    pub epoch: Instant,
    /// I2C backend for `type: i2c` (default: i2c-dev under `root`).
    pub i2c: Option<Arc<dyn Bus>>,
}

impl Options {
    pub fn new(cfg: &Config) -> Options {
        Options {
            root: cfg.sysfs_root(),
            dry_run: cfg.dry_run,
            exec_mode: ExecMode::Inline,
            calibration: Calibration::default(),
            epoch: Instant::now(),
            i2c: None,
        }
    }
}

/// A fault with a stable key (for change detection / logging) and a message.
#[derive(Debug, Clone, PartialEq)]
pub struct Fault {
    pub key: String,
    pub msg: String,
}

impl Fault {
    fn new(key: impl Into<String>, msg: impl Into<String>) -> Fault {
        Fault {
            key: key.into(),
            msg: msg.into(),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Caches a resolved path; retries a failed resolution every RESOLVE_RETRY s.
#[derive(Debug, Default)]
struct Resolver {
    resolved: Option<PathBuf>,
    last_try: Option<f64>,
    err: String,
}

impl Resolver {
    fn get(
        &mut self,
        now: f64,
        f: impl FnOnce() -> std::result::Result<PathBuf, String>,
    ) -> std::result::Result<PathBuf, String> {
        if let Some(p) = &self.resolved {
            return Ok(p.clone());
        }
        if self.last_try.is_none_or(|t| now - t >= RESOLVE_RETRY || now < t) {
            self.last_try = Some(now);
            match f() {
                Ok(p) => {
                    self.resolved = Some(p.clone());
                    self.err.clear();
                    return Ok(p);
                }
                Err(e) => self.err = e,
            }
        }
        Err(self.err.clone())
    }

    fn invalidate(&mut self) {
        self.resolved = None;
        self.last_try = None;
    }
}

// ---------------------------------------------------------------- sensors

enum Locator {
    Hwmon {
        sel: HwmonSelector,
        input: Option<String>,
        label: Option<String>,
    },
    Thermal(String),
    File(PathBuf),
}

impl Locator {
    fn resolve(&self, root: &Path) -> std::result::Result<PathBuf, String> {
        match self {
            Locator::Hwmon { sel, input, label } => {
                let dir = sysfs::find_hwmon(root, sel)?;
                sysfs::find_temp_input(&dir, input.as_deref(), label.as_deref())
            }
            Locator::Thermal(z) => sysfs::find_thermal_zone(root, z),
            Locator::File(p) => Ok(sysfs::rebase(root, p)),
        }
    }
}

type Shared = Arc<Mutex<Option<(f64, std::result::Result<f64, String>)>>>;

struct ExecSensor {
    argv: Vec<String>,
    interval: f64,
    timeout: f64,
    parser: Arc<Parser>,
    shared: Shared,
    stop: Arc<AtomicBool>,
    threaded: bool,
    started: bool,
    last_run: Option<f64>,
}

impl ExecSensor {
    fn run_once(argv: &[String], timeout: f64, parser: &Parser) -> std::result::Result<f64, String> {
        exec::run(argv, Duration::from_secs_f64(timeout)).and_then(|out| parser.parse(&out))
    }

    fn start(&mut self, epoch: Instant, name: &str) {
        if !self.threaded || self.started {
            return;
        }
        self.started = true;
        let (argv, interval, timeout) = (self.argv.clone(), self.interval, self.timeout);
        let (parser, shared, stop) = (self.parser.clone(), self.shared.clone(), self.stop.clone());
        let spawned = thread::Builder::new().name(format!("exec-{name}")).spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let r = Self::run_once(&argv, timeout, &parser);
                *lock(&shared) = Some((epoch.elapsed().as_secs_f64(), r));
                let until = Instant::now() + Duration::from_secs_f64(interval);
                while !stop.load(Ordering::Relaxed) {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    thread::sleep(left.min(Duration::from_millis(100)));
                }
            }
        });
        if let Err(e) = spawned {
            *lock(&self.shared) = Some((
                epoch.elapsed().as_secs_f64(),
                Err(format!("cannot start worker thread: {e}")),
            ));
        }
    }

    fn read(&mut self, now: f64) -> std::result::Result<(f64, f64), String> {
        if !self.threaded && self.last_run.is_none_or(|t| now - t >= self.interval || now < t) {
            self.last_run = Some(now);
            let r = Self::run_once(&self.argv, self.timeout, &self.parser);
            *lock(&self.shared) = Some((now, r));
        }
        match &*lock(&self.shared) {
            None => Err("waiting for the first reading".into()),
            Some((at, Ok(v))) => Ok((*v, *at)),
            Some((_, Err(e))) => Err(e.clone()),
        }
    }
}

impl Drop for ExecSensor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

enum Source {
    Sysfs(Locator, Resolver),
    Exec(ExecSensor),
    I2c(i2c::Reg, Arc<dyn Bus>),
}

pub struct SensorRt {
    pub name: String,
    pub kind: SensorKind,
    src: Source,
    scale: f64,
    offset: f64,
    lo: f64,
    hi: f64,
    pub required: bool,
    max_age: Option<f64>,
    pub critical: Option<f64>,
    pub value: Option<f64>,
    pub error: Option<String>,
    error_kind: Option<&'static str>,
    pub age: Option<f64>,
    /// Seconds a last good value may be held after failed readings.
    grace: f64,
    /// Lowest critical this sensor's value can trip (its own, or a zone's
    /// minus the zone's offset for it): a held value at/above it gets no grace.
    grace_ceiling: Option<f64>,
    /// Last good value and when it was read.
    last_good: Option<(f64, f64)>,
    /// Failing, but holding `last_good`.
    pub in_grace: bool,
}

impl SensorRt {
    fn new(name: &str, c: &SensorConfig, grace: f64, mode: ExecMode, bus: &Arc<dyn Bus>) -> Result<SensorRt> {
        let sel = || c.hwmon.clone().map(|s| s.0).unwrap_or_default();
        let src = match c.kind {
            SensorKind::Hwmon => Source::Sysfs(
                Locator::Hwmon {
                    sel: sel(),
                    input: c.input.clone(),
                    label: c.label.clone(),
                },
                Resolver::default(),
            ),
            SensorKind::File => Source::Sysfs(Locator::File(c.path.clone().unwrap_or_default()), Resolver::default()),
            SensorKind::ThermalZone => Source::Sysfs(
                Locator::Thermal(c.zone.clone().unwrap_or_default()),
                Resolver::default(),
            ),
            SensorKind::Exec => Source::Exec(ExecSensor {
                argv: c.command.clone().unwrap_or_default(),
                interval: c.exec_interval(),
                timeout: c.exec_timeout(),
                parser: Arc::new(
                    Parser::from_spec(c.parse.as_ref()).map_err(|e| Error(format!("sensors.{name}: {e}")))?,
                ),
                shared: Arc::new(Mutex::new(None)),
                stop: Arc::new(AtomicBool::new(false)),
                threaded: mode == ExecMode::Threaded,
                started: false,
                last_run: None,
            }),
            SensorKind::I2c => Source::I2c(
                c.i2c
                    .as_ref()
                    .ok_or_else(|| Error(format!("sensors.{name}: no i2c register")))?
                    .to_reg(None)
                    .map_err(|e| Error(format!("sensors.{name}.i2c: {e}")))?,
                bus.clone(),
            ),
        };
        let (lo, hi) = c.plausible();
        Ok(SensorRt {
            name: name.to_string(),
            kind: c.kind,
            src,
            scale: c.scale(),
            offset: c.offset,
            lo,
            hi,
            required: c.required,
            max_age: c.max_age(),
            critical: c.critical,
            value: None,
            error: None,
            error_kind: None,
            age: None,
            grace,
            grace_ceiling: c.critical,
            last_good: None,
            in_grace: false,
        })
    }

    /// Resolved path (sysfs-backed sensors only).
    pub fn path(&self) -> Option<&Path> {
        match &self.src {
            Source::Sysfs(_, r) => r.resolved.as_deref(),
            Source::Exec(_) | Source::I2c(..) => None,
        }
    }

    pub fn command(&self) -> Option<&[String]> {
        match &self.src {
            Source::Exec(x) => Some(&x.argv),
            Source::Sysfs(..) | Source::I2c(..) => None,
        }
    }

    /// Where the value comes from: the resolved path, or "i2c 4-0068:0x50".
    pub fn location(&self) -> Option<String> {
        match &self.src {
            Source::I2c(r, _) => Some(format!("i2c {r}")),
            _ => self.path().map(|p| p.display().to_string()),
        }
    }

    /// Resolve without reading (for `check`).
    pub fn resolve(&mut self, now: f64, root: &Path) -> std::result::Result<Option<PathBuf>, String> {
        match &mut self.src {
            Source::Sysfs(loc, res) => res.get(now, || loc.resolve(root)).map(Some),
            Source::Exec(_) | Source::I2c(..) => Ok(None),
        }
    }

    pub fn update(&mut self, now: f64, root: &Path) {
        let raw: std::result::Result<(f64, f64), (&'static str, String)> = match &mut self.src {
            Source::Sysfs(loc, res) => match res.get(now, || loc.resolve(root)) {
                Err(e) => Err(("resolve", e)),
                Ok(p) => match sysfs::read_number(&p) {
                    Ok(v) => Ok((v, now)),
                    Err(ReadError::NotFound(m)) => {
                        res.invalidate();
                        Err(("read", m))
                    }
                    Err(ReadError::Other(m)) => Err(("read", m)),
                },
            },
            Source::Exec(x) => x.read(now).map_err(|e| ("exec", e)),
            Source::I2c(r, bus) => r.read(bus.as_ref()).map(|v| (v, now)).map_err(|e| ("i2c", e)),
        };
        let checked = raw.and_then(|(raw, at)| {
            let age = (now - at).max(0.0);
            self.age = Some(age);
            if let Some(ma) = self.max_age {
                if age > ma {
                    return Err((
                        "stale",
                        format!("stale: last reading is {age:.0}s old (max_age {ma:.0}s)"),
                    ));
                }
            }
            let v = raw * self.scale + self.offset;
            if !v.is_finite() || v < self.lo || v > self.hi {
                return Err((
                    "implausible",
                    format!("implausible reading {v:.1} C (plausible {}..{} C)", self.lo, self.hi),
                ));
            }
            Ok((v, at))
        });
        match checked {
            Ok((v, at)) => {
                if self.in_grace {
                    self.in_grace = false;
                    self.error = None;
                    let gap = self.last_good.map_or(0.0, |(_, t)| now - t);
                    log_info!("sensor {}: reading back after {gap:.0}s, {v:.1} C", self.name);
                } else if self.error.take().is_some() {
                    log_info!("sensor {}: recovered, {v:.1} C", self.name);
                }
                self.error_kind = None;
                self.value = Some(v);
                self.last_good = Some((v, at));
            }
            Err((kind, msg)) => {
                // Grace: hold the last good value for a short gap (a writer
                // restarting), counted from when it was read. Never before the
                // first good reading, never for a value at/above a critical,
                // never again once the fault has been raised.
                let hold = self.last_good.filter(|&(v, at)| {
                    self.grace > 0.0
                        && self.error_kind.is_none()
                        && now - at <= self.grace
                        && self.grace_ceiling.is_none_or(|c| v < c)
                });
                if let Some((v, _)) = hold {
                    if !self.in_grace {
                        self.in_grace = true;
                        log_info!(
                            "sensor {}: no reading ({msg}), holding {v:.1} C for up to {}s",
                            self.name,
                            self.grace
                        );
                    }
                    self.error = Some(msg);
                    self.value = Some(v);
                    return;
                }
                if self.in_grace {
                    self.in_grace = false;
                    let gap = self.last_good.map_or(0.0, |(_, t)| now - t);
                    log_info!(
                        "sensor {}: grace of {}s ran out, no good reading for {gap:.0}s",
                        self.name,
                        self.grace
                    );
                }
                if self.error_kind != Some(kind) {
                    let level = if self.required {
                        crate::log::Level::Warn
                    } else {
                        crate::log::Level::Notice
                    };
                    crate::log::log(level, &format!("sensor {}: {msg}", self.name));
                }
                self.error_kind = Some(kind);
                self.error = Some(msg);
                self.value = None;
            }
        }
    }
}

// ---------------------------------------------------------------- outputs

enum OutLoc {
    Hwmon { sel: HwmonSelector, pwm: String },
    File(PathBuf),
}

impl OutLoc {
    fn resolve(&self, root: &Path) -> std::result::Result<PathBuf, String> {
        match self {
            OutLoc::Hwmon { sel, pwm } => {
                let dir = sysfs::find_hwmon(root, sel)?;
                sysfs::attr(&dir, pwm).ok_or_else(|| format!("{}: no attribute {pwm}", dir.display()))
            }
            OutLoc::File(p) => {
                let p = sysfs::rebase(root, p);
                if p.exists() {
                    Ok(p)
                } else {
                    Err(format!("{}: no such file", p.display()))
                }
            }
        }
    }
}

enum Sink {
    Sysfs(OutLoc, Resolver),
    Exec { argv: Vec<String>, timeout: f64 },
    I2c(Box<I2cSink>),
}

/// A `type: i2c` output: the duty register, its handshake and identity checks.
struct I2cSink {
    reg: i2c::Reg,
    bus: Arc<dyn Bus>,
    arm: Option<Arm>,
    expect: Vec<Expect>,
}

/// `arm`: a register that must hold `value` (under `mask`) for the duty
/// register to take effect. Checked before every duty write; when it does
/// not hold, `write` is written. The first time that is normal (the
/// hardware's power-on state); later it means the hardware took control
/// back, which is an error for that write (so the zone goes failsafe).
struct Arm {
    reg: i2c::Reg,
    mask: u32,
    value: u32,
    write: u32,
    checked: bool,
}

impl Arm {
    fn holds(&self, v: u32) -> bool {
        v & self.mask == self.value & self.mask
    }

    fn want(&self) -> String {
        format!("0x{:02x} under mask 0x{:02x}", self.value, self.mask)
    }

    fn assert(&mut self, bus: &dyn Bus, output: &str) -> std::result::Result<(), String> {
        let cur = self.reg.read_raw(bus)?;
        if self.holds(cur) {
            self.checked = true;
            return Ok(());
        }
        self.reg.write(bus, self.write)?;
        let after = self.reg.read_raw(bus)?;
        if !self.holds(after) {
            return Err(format!(
                "arm register {}: wrote 0x{:02x} but it reads 0x{after:02x} (want {}); the hardware keeps control",
                self.reg,
                self.write,
                self.want()
            ));
        }
        let first = !self.checked;
        self.checked = true;
        if first {
            log_info!(
                "output {output}: arm register {} read 0x{cur:02x}, wrote 0x{:02x}: armed",
                self.reg,
                self.write
            );
            Ok(())
        } else {
            Err(format!(
                "arm register {} read 0x{cur:02x} (want {}): the hardware had taken control; wrote 0x{:02x}, armed again",
                self.reg,
                self.want(),
                self.write
            ))
        }
    }
}

/// `expect`: a read-only identity check; while it fails nothing is written.
struct Expect {
    reg: i2c::Reg,
    mask: u32,
    lo: u32,
    hi: u32,
}

impl Expect {
    fn desc(&self) -> String {
        let m = if self.mask == self.reg.width.max() {
            String::new()
        } else {
            format!(" under mask 0x{:02x}", self.mask)
        };
        if self.lo == self.hi {
            format!("0x{:02x}{m}", self.lo)
        } else {
            format!("0x{:02x}..0x{:02x}{m}", self.lo, self.hi)
        }
    }

    fn check(&self, bus: &dyn Bus) -> std::result::Result<u32, String> {
        let v = self.reg.read_raw(bus)? & self.mask;
        if (self.lo..=self.hi).contains(&v) {
            Ok(v)
        } else {
            Err(format!(
                "expect {}: reads 0x{v:02x}, expected {}; not writing to this device",
                self.reg,
                self.desc()
            ))
        }
    }
}

impl I2cSink {
    fn new(c: &OutputConfig, bus: &Arc<dyn Bus>) -> std::result::Result<Option<I2cSink>, String> {
        let Some(base) = &c.i2c else { return Ok(None) };
        let reg = base.to_reg(None)?;
        let arm = match &c.arm {
            None => None,
            Some(a) => {
                let value = a.value.map_or(0, |v| v.0) as u32;
                Some(Arm {
                    reg: a.to_reg(Some(base))?,
                    mask: a.mask_or_all(),
                    value,
                    write: a.write.map_or(value, |v| v.0 as u32),
                    checked: false,
                })
            }
        };
        let expect = c
            .expect
            .iter()
            .map(|x| {
                let r = x.to_reg(Some(base))?;
                let (lo, hi) = match x.value {
                    Some(v) => (v.0 as u32, v.0 as u32),
                    None => (
                        x.min.map_or(0, |v| v.0 as u32),
                        x.max.map_or(r.width.max(), |v| v.0 as u32),
                    ),
                };
                Ok(Expect {
                    reg: r,
                    mask: x.mask_or_all(),
                    lo,
                    hi,
                })
            })
            .collect::<std::result::Result<Vec<_>, String>>()?;
        Ok(Some(I2cSink {
            reg,
            bus: bus.clone(),
            arm,
            expect,
        }))
    }
}

/// A bus that fails every transfer (unit tests of non-i2c outputs).
#[cfg(test)]
struct NoBus;

#[cfg(test)]
impl Bus for NoBus {
    fn read(&self, _: i2c::Target, _: u8, _: i2c::Width) -> std::result::Result<u16, String> {
        Err("no i2c backend".into())
    }
    fn write(&self, _: i2c::Target, _: u8, _: i2c::Width, _: u16) -> std::result::Result<(), String> {
        Err("no i2c backend".into())
    }
}

#[derive(Debug, Clone)]
enum RotorSrc {
    Path(PathBuf),
    I2c(i2c::Reg),
}

#[derive(Debug, Clone)]
pub struct Rotor {
    pub input: String,
    pub label: String,
    src: RotorSrc,
    fault_paths: Vec<PathBuf>,
    pub model: Option<RotorModel>,
    pub rpm: Option<f64>,
    pub fault_flag: bool,
    pub error: Option<String>,
    pub expected: Option<f64>,
}

pub struct Tach {
    sel: HwmonSelector,
    res: Resolver,
    discovered_for: Option<PathBuf>,
    auto: bool,
    inputs: Vec<String>,
    default_max: Option<f64>,
    by_label: BTreeMap<String, f64>,
    cal: Option<OutputCal>,
    min_rpm: Option<f64>,
    check_above: f64,
    follow_tol: Option<f64>,
    grace: f64,
    fault_attrs: bool,
    pub on_fault: FaultAction,
    pub rotors: Vec<Rotor>,
    /// i2c tach: fixed rotors read through this bus, RPM = value * scale.
    i2c: Option<(Arc<dyn Bus>, f64)>,
}

impl Tach {
    fn discover(&mut self, dir: &Path) -> std::result::Result<(), String> {
        let list: Vec<(String, PathBuf)> = if self.auto {
            sysfs::list_fans(dir)
        } else {
            self.inputs
                .iter()
                .map(|i| {
                    let a = sysfs::normalize_input(i);
                    let short = a.strip_suffix("_input").unwrap_or(&a).to_string();
                    sysfs::attr(dir, &a)
                        .map(|p| (short, p))
                        .ok_or_else(|| format!("{}: no attribute {a}", dir.display()))
                })
                .collect::<std::result::Result<_, _>>()?
        };
        if list.is_empty() {
            return Err(format!("{}: no fanN_input attributes", dir.display()));
        }
        let rotors = list
            .into_iter()
            .map(|(input, input_path)| {
                let label = sysfs::attr(dir, &format!("{input}_label"))
                    .and_then(|p| sysfs::read_trim(&p).ok())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| input.clone());
                let fault_paths = if self.fault_attrs {
                    ["fault", "alarm"]
                        .iter()
                        .filter_map(|k| sysfs::attr(dir, &format!("{input}_{k}")))
                        .collect()
                } else {
                    Vec::new()
                };
                let model = self.model_for(&label, &input);
                Rotor {
                    input,
                    label,
                    src: RotorSrc::Path(input_path),
                    fault_paths,
                    model,
                    rpm: None,
                    fault_flag: false,
                    error: None,
                    expected: None,
                }
            })
            .collect();
        self.rotors = rotors;
        self.discovered_for = Some(dir.to_path_buf());
        Ok(())
    }

    fn model_for(&self, label: &str, input: &str) -> Option<RotorModel> {
        if let Some(rc) = self
            .cal
            .as_ref()
            .and_then(|c| c.rotors.get(label).or_else(|| c.rotors.get(input)))
        {
            if let Some(m) = RotorModel::from_cal(rc) {
                return Some(m);
            }
        }
        let by_label = self
            .by_label
            .iter()
            .filter(|(k, _)| label.contains(k.as_str()))
            .max_by_key(|(k, _)| k.len())
            .map(|(_, v)| *v);
        by_label.or(self.default_max).map(RotorModel::linear)
    }

    /// Read every rotor and check it against the commanded duty.
    fn check(&mut self, now: f64, root: &Path, commanded: Option<f64>, since_change: f64) -> Vec<Fault> {
        if self.i2c.is_none() {
            let sel = &self.sel;
            let dir = match self.res.get(now, || sysfs::find_hwmon(root, sel)) {
                Ok(d) => d,
                Err(e) => {
                    self.rotors.clear();
                    self.discovered_for = None;
                    return vec![Fault::new("tach", format!("tach: {e}"))];
                }
            };
            if self.discovered_for.as_deref() != Some(dir.as_path()) {
                if let Err(e) = self.discover(&dir) {
                    self.res.invalidate();
                    return vec![Fault::new("tach", format!("tach: {e}"))];
                }
            }
        }
        let mut faults = Vec::new();
        let mut gone = false;
        let i2c = &self.i2c;
        for r in &mut self.rotors {
            r.expected = None;
            let reading = match (&r.src, i2c) {
                (RotorSrc::I2c(reg), Some((bus, scale))) => {
                    reg.read(bus.as_ref()).map(|v| v * scale).map_err(ReadError::Other)
                }
                (RotorSrc::Path(p), _) => sysfs::read_number(p),
                (RotorSrc::I2c(_), None) => Err(ReadError::Other("no i2c backend".into())),
            };
            match reading {
                Ok(v) => {
                    r.rpm = Some(v);
                    r.error = None;
                }
                Err(e) => {
                    gone |= matches!(e, ReadError::NotFound(_));
                    let m = e.message();
                    faults.push(Fault::new(format!("{}/read", r.label), format!("{}: {m}", r.label)));
                    r.rpm = None;
                    r.error = Some(m);
                    continue;
                }
            }
            r.fault_flag = r
                .fault_paths
                .iter()
                .any(|p| sysfs::read_number(p).is_ok_and(|v| v != 0.0));
            if r.fault_flag {
                faults.push(Fault::new(
                    format!("{}/fault", r.label),
                    format!("{}: fault flag set", r.label),
                ));
            }
            let rpm = r.rpm.unwrap_or(0.0);
            let Some(d) = commanded.filter(|d| *d > 0.0) else {
                continue;
            };
            r.expected = r.model.as_ref().map(|m| m.expected(d));
            if let Some(min) = self.min_rpm {
                if d >= self.check_above && rpm < min {
                    faults.push(Fault::new(
                        format!("{}/min_rpm", r.label),
                        format!("{}: {rpm:.0} rpm is below the minimum {min:.0} rpm at {d:.0}%", r.label),
                    ));
                }
            }
            if let (Some(tol), Some(exp)) = (self.follow_tol, r.expected) {
                if since_change >= self.grace && rpm < exp * (1.0 - tol) {
                    faults.push(Fault::new(
                        format!("{}/follow", r.label),
                        format!(
                            "{}: {rpm:.0} rpm, expected about {exp:.0} rpm at {d:.0}% (not following its command)",
                            r.label
                        ),
                    ));
                }
            }
        }
        if gone {
            self.res.invalidate();
            self.discovered_for = None;
        }
        faults
    }
}

pub struct OutputRt {
    pub name: String,
    pub kind: OutputKind,
    sink: Sink,
    /// i2c `map`: raw <-> duty table instead of linear raw_min..raw_max.
    map: Option<RawMap>,
    enable: Option<u32>,
    exit_enable: Option<u32>,
    raw_min: u32,
    raw_max: u32,
    pub min_duty: f64,
    pub max_duty: f64,
    pub stop: bool,
    readback: Readback,
    tol: u32,
    reassert: f64,
    pub tach: Option<Tach>,
    pub duty: Option<f64>,
    pub raw: Option<u32>,
    pub observed_raw: Option<u32>,
    last_write: Option<f64>,
    last_change: f64,
    pub write_error: Option<String>,
    pub faults: Vec<Fault>,
    pub warnings: Vec<Fault>,
    /// "50.2%, raw_min 128 of 255": the hardware minimum and its source.
    pub min_desc: String,
    /// Duty requested this tick before the clamp (max over its zones).
    pub demand: Option<f64>,
    /// The request is below the hardware minimum and was raised to it.
    pub clamped: bool,
}

impl OutputRt {
    fn new(name: &str, c: &OutputConfig, cal: &Calibration, bus: &Arc<dyn Bus>) -> Result<OutputRt> {
        let sel = c.hwmon.clone().map(|s| s.0);
        let sink = match c.kind {
            OutputKind::I2c => {
                let x = I2cSink::new(c, bus)
                    .and_then(|x| x.ok_or_else(|| "no i2c register".to_string()))
                    .map_err(|e| Error(format!("outputs.{name}.i2c: {e}")))?;
                Sink::I2c(Box::new(x))
            }
            OutputKind::Hwmon => Sink::Sysfs(
                OutLoc::Hwmon {
                    sel: sel.clone().unwrap_or_default(),
                    pwm: c.pwm.clone().unwrap_or_else(|| "pwm1".into()),
                },
                Resolver::default(),
            ),
            OutputKind::File => Sink::Sysfs(OutLoc::File(c.path.clone().unwrap_or_default()), Resolver::default()),
            OutputKind::Exec => Sink::Exec {
                argv: c.command.clone().unwrap_or_default(),
                timeout: c.timeout.map_or(5.0, |t| t.0),
            },
        };
        let tach = c.tach.as_ref().map(|t| Tach {
            sel: t.hwmon.clone().map(|s| s.0).or(sel).unwrap_or_default(),
            res: Resolver::default(),
            discovered_for: None,
            auto: t.auto,
            inputs: t.inputs.clone(),
            default_max: t.max_rpm,
            by_label: t.max_rpm_by_label.clone(),
            cal: cal.outputs.get(name).cloned(),
            min_rpm: t.min_rpm,
            check_above: t.check_above_duty,
            follow_tol: t.follow_tolerance,
            grace: t.follow_grace.0,
            fault_attrs: t.fault_attrs,
            on_fault: t.on_fault,
            rotors: Vec::new(),
            i2c: None,
        });
        let mut tach = tach;
        if let (Some(t), Some(tc)) = (&mut tach, c.tach.as_ref().filter(|t| !t.i2c.is_empty())) {
            let mut rotors = Vec::new();
            for (n, rc) in tc.i2c.iter().enumerate() {
                let reg = rc
                    .to_reg(c.i2c.as_ref())
                    .map_err(|e| Error(format!("outputs.{name}.tach.i2c[{n}]: {e}")))?;
                let label = rc.label.clone().unwrap_or_else(|| format!("fan{}", n + 1));
                rotors.push(Rotor {
                    input: reg.to_string(),
                    model: t.model_for(&label, &reg.to_string()),
                    label,
                    src: RotorSrc::I2c(reg),
                    fault_paths: Vec::new(),
                    rpm: None,
                    fault_flag: false,
                    error: None,
                    expected: None,
                });
            }
            t.rotors = rotors;
            t.i2c = Some((bus.clone(), tc.scale.unwrap_or(1.0)));
        }
        Ok(OutputRt {
            name: name.to_string(),
            kind: c.kind,
            map: if c.kind == OutputKind::I2c { c.raw_map() } else { None },
            sink,
            enable: c.enable,
            exit_enable: c.exit_enable,
            raw_min: c.raw_min,
            raw_max: c.raw_max,
            min_duty: c.min_duty(),
            max_duty: c.max_duty(),
            stop: c.stop,
            readback: c.readback,
            tol: c.readback_tolerance,
            reassert: c.reassert(),
            tach,
            duty: None,
            raw: None,
            observed_raw: None,
            last_write: None,
            last_change: 0.0,
            write_error: None,
            faults: Vec::new(),
            warnings: Vec::new(),
            min_desc: hwmin::describe(c),
            demand: None,
            clamped: false,
        })
    }

    /// Is a demand below the hardware minimum (so the clamp raises it)?
    pub fn below_min(&self, demand: f64) -> bool {
        if let Some(m) = &self.map {
            return hwmin::below_map(demand, self.min_duty, self.stop, m);
        }
        hwmin::below(demand, self.min_duty, self.stop, self.raw_max)
    }

    /// Record the tick's demand; log when it starts or stops being raised to
    /// the hardware minimum (zone targets already include the minimum, so
    /// this means an override, a failsafe/startup duty below it, or a value
    /// between 0 and the minimum on a stop-capable output). `who` names the
    /// requesting zones.
    fn track_min(&mut self, demand: f64, who: impl FnOnce() -> String) {
        self.demand = Some(demand);
        let below = self.below_min(demand);
        if below && !self.clamped {
            self.clamped = true;
            log_warn!(
                "output {}: requested {} ({}) is below its hardware minimum ({}); running at {}",
                self.name,
                hwmin::pct(demand),
                who(),
                self.min_desc,
                hwmin::pct(self.clamp(demand))
            );
        } else if !below && self.clamped {
            self.clamped = false;
            log_info!(
                "output {}: requested {} ({}) is at or above its hardware minimum ({}) again",
                self.name,
                hwmin::pct(demand),
                who(),
                hwmin::pct(self.min_duty)
            );
        }
    }

    /// Duty the output will actually run at for a demand: within
    /// [min_duty, max_duty], or 0 if stopping is allowed and asked for.
    pub fn clamp(&self, demand: f64) -> f64 {
        if self.stop && demand <= 0.0 {
            0.0
        } else {
            demand.clamp(self.min_duty, self.max_duty)
        }
    }

    /// Percent -> raw, rounding up (never less air than asked for) and
    /// respecting the hardware's raw range.
    pub fn to_raw(&self, duty: f64) -> u32 {
        if let Some(m) = &self.map {
            return if self.stop && duty <= 0.0 {
                m.off
            } else {
                m.raw_for(duty)
            };
        }
        if self.stop && duty <= 0.0 {
            return 0;
        }
        let r = (duty * f64::from(self.raw_max) / 100.0 - 1e-9).ceil().max(0.0);
        let r = if r >= f64::from(self.raw_max) {
            self.raw_max
        } else {
            r as u32
        };
        r.clamp(self.raw_min, self.raw_max)
    }

    pub fn raw_to_duty(&self, raw: u32) -> f64 {
        if let Some(m) = &self.map {
            let off = f64::from(raw) < m.points[0].0 || (self.stop && raw == m.off);
            return if off { 0.0 } else { m.duty(raw) };
        }
        f64::from(raw) * 100.0 / f64::from(self.raw_max.max(1))
    }

    /// Duty needed for every rotor to reach `rpm` (None without an RPM model).
    pub fn duty_for_rpm(&self, rpm: f64) -> Option<f64> {
        self.tach
            .as_ref()?
            .rotors
            .iter()
            .filter_map(|r| r.model.as_ref().map(|m| m.duty_for(rpm)))
            .reduce(f64::max)
    }

    pub fn path(&self) -> Option<&Path> {
        match &self.sink {
            Sink::Sysfs(_, r) => r.resolved.as_deref(),
            Sink::Exec { .. } | Sink::I2c(_) => None,
        }
    }

    pub fn command(&self) -> Option<&[String]> {
        match &self.sink {
            Sink::Exec { argv, .. } => Some(argv),
            Sink::Sysfs(..) | Sink::I2c(_) => None,
        }
    }

    /// Where duties go: the resolved path, or "i2c 4-0068:0x50".
    pub fn location(&self) -> Option<String> {
        match &self.sink {
            Sink::I2c(x) => Some(format!("i2c {}", x.reg)),
            _ => self.path().map(|p| p.display().to_string()),
        }
    }

    /// Read-only report of an i2c output for `fand check`: (ok, line) for the
    /// duty register, every `expect` and the `arm` register. None otherwise.
    pub fn i2c_report(&self) -> Option<Vec<(bool, String)>> {
        let Sink::I2c(x) = &self.sink else { return None };
        let bus = x.bus.as_ref();
        let mut out = Vec::new();
        out.push(match x.reg.read_raw(bus) {
            Ok(v) => (
                true,
                format!("i2c {} = {v} ({})", x.reg, hwmin::pct(self.raw_to_duty(v))),
            ),
            Err(e) => (false, format!("ERROR {e}")),
        });
        for ex in &x.expect {
            out.push(match ex.check(bus) {
                Ok(v) => (true, format!("expect {} = 0x{v:02x} (ok: {})", ex.reg, ex.desc())),
                Err(e) => (false, format!("ERROR {e}")),
            });
        }
        if let Some(a) = &x.arm {
            out.push(match a.reg.read_raw(bus) {
                Ok(v) if a.holds(v) => (true, format!("arm {} = 0x{v:02x} (armed)", a.reg)),
                Ok(v) => (
                    true,
                    format!(
                        "arm {} = 0x{v:02x} (not armed, want {}; fand run writes 0x{:02x})",
                        a.reg,
                        a.want(),
                        a.write
                    ),
                ),
                Err(e) => (false, format!("ERROR {e}")),
            });
        }
        Some(out)
    }

    /// Resolve the pwm path (sysfs outputs).
    pub fn resolve(&mut self, now: f64, root: &Path) -> std::result::Result<Option<PathBuf>, String> {
        match &mut self.sink {
            Sink::Sysfs(loc, res) => res.get(now, || loc.resolve(root)).map(Some),
            Sink::Exec { .. } | Sink::I2c(_) => Ok(None),
        }
    }

    fn enable_path(pwm: &Path) -> PathBuf {
        let mut s = pwm.as_os_str().to_os_string();
        s.push("_enable");
        PathBuf::from(s)
    }

    fn write_raw(&mut self, raw: u32, duty: f64, now: f64, root: &Path) -> std::result::Result<(), String> {
        match &mut self.sink {
            Sink::Sysfs(loc, res) => {
                let p = res.get(now, || loc.resolve(root))?;
                if let Some(ev) = self.enable {
                    let ep = Self::enable_path(&p);
                    let cur = sysfs::read_number(&ep);
                    if !matches!(cur, Ok(v) if v == f64::from(ev)) {
                        sysfs::write_value(&ep, &ev.to_string())
                            .map_err(|e| format!("writing {ev} to {}: {e}", ep.display()))?;
                        if let Ok(v) = cur {
                            log_notice!("output {}: {} was {v}, set to {ev}", self.name, ep.display());
                        }
                    }
                }
                if let Err(e) = sysfs::write_value(&p, &raw.to_string()) {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        res.invalidate();
                    }
                    return Err(format!("writing {raw} to {}: {e}", p.display()));
                }
                if self.readback == Readback::Off {
                    return Ok(());
                }
                let got =
                    sysfs::read_number(&p).map_err(|e| format!("readback of {}: {}", p.display(), e.message()))?;
                let (got_i, want, tol) = (got.round() as i64, i64::from(raw), i64::from(self.tol));
                self.observed_raw = u32::try_from(got_i).ok();
                let ok = match self.readback {
                    Readback::AtLeast => got_i + tol >= want,
                    Readback::Exact => (got_i - want).abs() <= tol,
                    Readback::Off => true,
                };
                if ok {
                    Ok(())
                } else {
                    Err(format!(
                        "readback mismatch on {}: wrote {raw}, read {got_i}",
                        p.display()
                    ))
                }
            }
            Sink::Exec { argv, timeout } => {
                let pct = format!("{:.0}", duty.ceil());
                let args: Vec<String> = argv
                    .iter()
                    .map(|a| a.replace("{percent}", &pct).replace("{raw}", &raw.to_string()))
                    .collect();
                exec::run(&args, Duration::from_secs_f64(*timeout)).map(|_| ())
            }
            Sink::I2c(x) => {
                let bus = x.bus.clone();
                let bus = bus.as_ref();
                // Identity first: on an unexpected device nothing is written,
                // not even the arm register (its failsafe stays in charge).
                for ex in &x.expect {
                    ex.check(bus)?;
                }
                // The duty is written even if arming failed: a failsafe duty
                // must always be attempted. The arm error is still returned.
                let arm_err = x.arm.as_mut().and_then(|a| a.assert(bus, &self.name).err());
                x.reg.write(bus, raw)?;
                if self.readback != Readback::Off {
                    let got = x.reg.read_raw(bus)?;
                    self.observed_raw = Some(got);
                    let (got_i, want, tol) = (i64::from(got), i64::from(raw), i64::from(self.tol));
                    let ok = match self.readback {
                        Readback::AtLeast => got_i + tol >= want,
                        Readback::Exact => (got_i - want).abs() <= tol,
                        Readback::Off => true,
                    };
                    if !ok {
                        return Err(format!("readback mismatch on i2c {}: wrote {raw}, read {got}", x.reg));
                    }
                }
                arm_err.map_or(Ok(()), Err)
            }
        }
    }

    /// Read feedback and collect faults for this tick.
    fn feedback(&mut self, now: f64, root: &Path, dry_run: bool) {
        let mut faults = Vec::new();
        if let Some(e) = &self.write_error {
            faults.push(Fault::new("write", e.clone()));
        }
        if matches!(self.sink, Sink::Sysfs(..)) {
            match self.resolve(now, root) {
                Ok(Some(p)) => {
                    let obs = sysfs::read_number(&p)
                        .ok()
                        .and_then(|v| u32::try_from(v.round() as i64).ok());
                    if dry_run && obs != self.observed_raw {
                        self.last_change = now;
                    }
                    self.observed_raw = obs;
                }
                Ok(None) => {}
                Err(e) => faults.push(Fault::new("resolve", e)),
            }
        }
        if let Sink::I2c(x) = &self.sink {
            match x.reg.read_raw(x.bus.as_ref()) {
                Ok(v) => {
                    if dry_run && Some(v) != self.observed_raw {
                        self.last_change = now;
                    }
                    self.observed_raw = Some(v);
                }
                Err(e) => faults.push(Fault::new("i2c", e)),
            }
        }
        let commanded = if dry_run {
            self.observed_raw.map(|r| self.raw_to_duty(r))
        } else {
            self.duty
        };
        let since = now - self.last_change;
        let mut warnings = Vec::new();
        if let Some(t) = &mut self.tach {
            let tf = t.check(now, root, commanded, since);
            match t.on_fault {
                FaultAction::Failsafe => faults.extend(tf),
                FaultAction::Warn => warnings.extend(tf),
            }
        }
        for f in faults.iter().chain(&warnings) {
            if !self.faults.iter().chain(&self.warnings).any(|o| o.key == f.key) && f.key != "write" {
                log_warn!("output {}: {}", self.name, f.msg);
            }
        }
        for o in self.faults.iter().chain(&self.warnings) {
            if !faults.iter().chain(&warnings).any(|f| f.key == o.key) && o.key != "write" {
                log_info!("output {}: cleared: {}", self.name, o.msg);
            }
        }
        self.faults = faults;
        self.warnings = warnings;
    }

    /// Command a demand (percent). Writes when the raw value changes or the
    /// reassert interval is due. On failure the failsafe duty is attempted
    /// at once and the error becomes a fault for the next tick.
    fn apply(&mut self, demand: f64, now: f64, root: &Path, dry_run: bool, failsafe_duty: f64) {
        let duty = self.clamp(demand);
        let raw = self.to_raw(duty);
        if self.raw != Some(raw) && !dry_run {
            self.last_change = now;
        }
        let due = self.raw != Some(raw)
            || self.write_error.is_some()
            || self.last_write.is_none_or(|t| now - t >= self.reassert);
        if dry_run || !due {
            self.duty = Some(duty);
            self.raw = Some(raw);
            return;
        }
        self.last_write = Some(now);
        match self.write_raw(raw, duty, now, root) {
            Ok(()) => {
                if self.write_error.take().is_some() {
                    log_info!("output {}: writes work again", self.name);
                }
                self.duty = Some(duty);
                self.raw = Some(raw);
            }
            Err(e) => {
                if self.write_error.as_ref() != Some(&e) {
                    log_error!("output {}: {e}", self.name);
                }
                self.write_error = Some(e);
                let fs_duty = self.clamp(failsafe_duty);
                let fs_raw = self.to_raw(fs_duty);
                if fs_raw != raw && self.write_raw(fs_raw, fs_duty, now, root).is_ok() {
                    self.duty = Some(fs_duty);
                    self.raw = Some(fs_raw);
                } else {
                    self.duty = Some(duty);
                    self.raw = Some(raw);
                }
            }
        }
    }

    /// Write a duty now, bypassing zones (exit, failsafe command, calibration).
    pub fn force(&mut self, duty: f64, now: f64, root: &Path) -> std::result::Result<u32, String> {
        let d = self.clamp(duty);
        let raw = self.to_raw(d);
        self.write_raw(raw, d, now, root)?;
        self.duty = Some(d);
        self.raw = Some(raw);
        self.last_write = Some(now);
        self.last_change = now;
        Ok(raw)
    }

    /// Write exit_enable (if configured), e.g. to hand control back to firmware.
    pub fn restore_enable(&mut self, now: f64, root: &Path) -> std::result::Result<(), String> {
        let Some(v) = self.exit_enable else { return Ok(()) };
        if let Sink::Sysfs(loc, res) = &mut self.sink {
            let p = res.get(now, || loc.resolve(root))?;
            let ep = Self::enable_path(&p);
            sysfs::write_value(&ep, &v.to_string()).map_err(|e| format!("writing {v} to {}: {e}", ep.display()))?;
        }
        Ok(())
    }

    /// Current rotor readings without fault evaluation (calibration).
    pub fn read_rotors(&mut self, now: f64, root: &Path) -> std::result::Result<Vec<(String, String, f64)>, String> {
        let Some(t) = &mut self.tach else {
            return Err("output has no tach".into());
        };
        let faults = t.check(now, root, None, 0.0);
        if t.rotors.is_empty() {
            return Err(faults.into_iter().map(|f| f.msg).collect::<Vec<_>>().join("; "));
        }
        Ok(t.rotors
            .iter()
            .filter_map(|r| r.rpm.map(|rpm| (r.label.clone(), r.input.clone(), rpm)))
            .collect())
    }
}

// ---------------------------------------------------------------- zones

enum PolicyRt {
    Curve(Curve, Hysteresis),
    Target(Pid),
    Fixed(DutySpec),
}

struct ZSensor {
    idx: usize,
    offset: f64,
    weight: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Override {
    pub duty: DutySpec,
    pub until: f64,
}

pub struct ZoneRt {
    pub name: String,
    sensors: Vec<ZSensor>,
    pub outputs: Vec<usize>,
    aggregate: Aggregate,
    policy: PolicyRt,
    pub policy_desc: String,
    pub profile: Profile,
    failsafe_duty: f64,
    critical: Option<f64>,
    min_sensors: usize,
    ema: Ema,
    ramp: Ramp,
    pub guard: FailGuard,
    pub temperature: Option<f64>,
    pub smoothed: Option<f64>,
    pub demand: Option<f64>,
    pub floor: f64,
    pub duty: f64,
    pub override_: Option<Override>,
    fault_keys: Vec<String>,
    warned_rpm: bool,
    started: bool,
}

/// "zones board 45%, asic 45% override": the zones driving output `i`.
fn requesters(zones: &[ZoneRt], i: usize) -> String {
    let list: Vec<String> = zones
        .iter()
        .filter(|z| z.outputs.contains(&i))
        .map(|z| {
            let tag = if z.guard.active() {
                " failsafe"
            } else if z.override_.is_some() {
                " override"
            } else {
                ""
            };
            format!("{} {}{tag}", z.name, hwmin::pct(z.duty))
        })
        .collect();
    format!("zone{} {}", if list.len() > 1 { "s" } else { "" }, list.join(", "))
}

fn spec_to_pct(d: DutySpec, zone_outputs: &[usize], outputs: &[OutputRt]) -> Option<f64> {
    match d {
        DutySpec::Percent(p) => Some(p),
        DutySpec::Rpm(r) => zone_outputs
            .iter()
            .filter_map(|&o| outputs[o].duty_for_rpm(r))
            .reduce(f64::max),
    }
}

impl ZoneRt {
    fn new(
        name: &str,
        z: &ZoneConfig,
        cfg: &Config,
        sensor_idx: &BTreeMap<String, usize>,
        output_idx: &BTreeMap<String, usize>,
    ) -> Result<ZoneRt> {
        let zone_target = z.policy.target;
        let mut sensors = Vec::new();
        for s in &z.sensors {
            let idx = *sensor_idx
                .get(&s.sensor)
                .ok_or_else(|| Error(format!("zones.{name}: unknown sensor {}", s.sensor)))?;
            let extra = match (zone_target, s.target) {
                (Some(zt), Some(st)) => zt - st,
                _ => 0.0,
            };
            sensors.push(ZSensor {
                idx,
                offset: s.offset + extra,
                weight: s.weight,
            });
        }
        let outputs = z
            .outputs
            .iter()
            .map(|o| {
                output_idx
                    .get(o)
                    .copied()
                    .ok_or_else(|| Error(format!("zones.{name}: unknown output {o}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let pol = &z.policy;
        let (policy, policy_desc) = if let Some(c) = &pol.curve {
            let curve = cfg
                .curve(c)
                .ok_or_else(|| Error(format!("zones.{name}: unknown curve")))?;
            let band = z.hysteresis.unwrap_or(curve.hysteresis);
            let desc = match c {
                crate::config::CurveRef::Named(n) => format!("curve {n}"),
                crate::config::CurveRef::Inline { .. } => "curve (inline)".to_string(),
            };
            (PolicyRt::Curve(curve, Hysteresis::new(band)), desc)
        } else if let Some(t) = pol.target {
            let (kp, ki, kd) = (
                pol.kp.unwrap_or(DEFAULT_KP),
                pol.ki.unwrap_or(DEFAULT_KI),
                pol.kd.unwrap_or(DEFAULT_KD),
            );
            let d_tau = pol.d_filter.map_or(5.0, |s| s.0);
            (
                PolicyRt::Target(Pid::new(t, kp, ki, kd, d_tau)),
                format!("target {t} C (kp {kp}, ki {ki}, kd {kd})"),
            )
        } else if let Some(d) = pol.fixed {
            (PolicyRt::Fixed(d), format!("fixed {d}"))
        } else {
            return Err(Error(format!("zones.{name}: no policy")));
        };
        Ok(ZoneRt {
            name: name.to_string(),
            min_sensors: z.min_sensors.unwrap_or(if sensors.is_empty() { 0 } else { 1 }),
            sensors,
            outputs,
            aggregate: z.aggregate,
            policy,
            policy_desc,
            profile: z.profile.unwrap_or(cfg.profile),
            failsafe_duty: z.failsafe_duty.unwrap_or(cfg.failsafe_duty).0,
            critical: z.critical,
            ema: Ema::new(z.smoothing.0),
            ramp: {
                let (up, down, dwell) = z.ramp();
                Ramp::new(up, down, dwell)
            },
            guard: FailGuard::new(z.calm_period.unwrap_or(cfg.calm_period).0),
            temperature: None,
            smoothed: None,
            demand: None,
            floor: 0.0,
            duty: cfg.startup_duty(),
            override_: None,
            fault_keys: Vec::new(),
            warned_rpm: false,
            started: false,
        })
    }

    fn pct(&mut self, d: DutySpec, outputs: &[OutputRt]) -> f64 {
        match spec_to_pct(d, &self.outputs, outputs) {
            Some(p) => p.clamp(0.0, 100.0),
            None => {
                if !self.warned_rpm {
                    log_warn!(
                        "zone {}: cannot convert {d} to a duty (no RPM model on its outputs); using 100%",
                        self.name
                    );
                    self.warned_rpm = true;
                }
                100.0
            }
        }
    }

    fn update(&mut self, now: f64, dt: f64, sensors: &[SensorRt], outputs: &[OutputRt], table: &ProfileDuties) {
        if !self.started {
            // The startup duty counts as an increase: hold it for the dwell
            // time while the first readings settle.
            self.started = true;
            self.ramp.mark_increase(now);
        }
        let mut faults: Vec<Fault> = Vec::new();
        let mut vals = Vec::new();
        for zs in &self.sensors {
            let s = &sensors[zs.idx];
            match s.value {
                Some(v) => {
                    if let Some(c) = s.critical {
                        if v >= c {
                            faults.push(Fault::new(
                                format!("sensor/{}/critical", s.name),
                                format!("sensor {} at {v:.1} C, critical is {c:.1} C", s.name),
                            ));
                        }
                    }
                    vals.push((v + zs.offset, zs.weight));
                }
                None if s.required => faults.push(Fault::new(
                    format!("sensor/{}", s.name),
                    format!("sensor {}: {}", s.name, s.error.as_deref().unwrap_or("no reading")),
                )),
                None => {}
            }
        }
        if vals.len() < self.min_sensors {
            faults.push(Fault::new(
                "sensors",
                format!(
                    "only {} of {} sensors usable (need {})",
                    vals.len(),
                    self.sensors.len(),
                    self.min_sensors
                ),
            ));
        }
        for &oi in &self.outputs {
            let o = &outputs[oi];
            for f in &o.faults {
                faults.push(Fault::new(
                    format!("output/{}/{}", o.name, f.key),
                    format!("output {}: {}", o.name, f.msg),
                ));
            }
        }
        self.temperature = exec::aggregate_weighted(&vals, self.aggregate);
        if let (Some(t), Some(c)) = (self.temperature, self.critical) {
            if t >= c {
                faults.push(Fault::new(
                    "critical",
                    format!("zone temperature {t:.1} C is at/above critical {c:.1} C"),
                ));
            }
        }
        self.smoothed = match self.temperature {
            Some(t) => Some(self.ema.update(t, dt)),
            None => {
                self.ema.reset();
                None
            }
        };

        let msgs: Vec<String> = faults.iter().map(|f| f.msg.clone()).collect();
        let keys: Vec<String> = faults.iter().map(|f| f.key.clone()).collect();
        let tr = self.guard.update(&msgs, now);
        match tr {
            Transition::Entered => log_error!(
                "zone {}: FAILSAFE ({}%): {}",
                self.name,
                self.failsafe_duty,
                msgs.join("; ")
            ),
            Transition::Recovering => log_notice!(
                "zone {}: faults cleared, holding failsafe for the {:.0}s calm period",
                self.name,
                self.guard.calm
            ),
            Transition::Left => log_notice!("zone {}: leaving failsafe, resuming {}", self.name, self.policy_desc),
            Transition::None => {
                if self.guard.active() && !keys.is_empty() && keys != self.fault_keys {
                    log_warn!("zone {}: still failsafe: {}", self.name, msgs.join("; "));
                }
            }
        }
        self.fault_keys = keys;

        let floor_spec = self.profile.floor(table);
        self.floor = self.pct(floor_spec, outputs);
        // The highest hardware minimum of the zone's outputs (stop-capable
        // ones may go to 0, so they do not count): the zone's target never
        // goes below it, so its duty is what the fans actually get and the
        // ramp does not spend time in a range where nothing changes.
        let out_min = self
            .outputs
            .iter()
            .map(|&o| if outputs[o].stop { 0.0 } else { outputs[o].min_duty })
            .fold(0.0, f64::max);
        let lo = self.floor.max(out_min).min(100.0);

        if self.guard.active() {
            self.demand = None;
            self.duty = self.failsafe_duty;
            self.ramp.mark_increase(now);
            return;
        }

        let pv = self.smoothed;
        let current = self.duty;
        let demand = match &mut self.policy {
            PolicyRt::Curve(c, h) => pv.map(|t| c.eval(h.apply(t))),
            PolicyRt::Target(pid) => pv.map(|t| {
                if !pid.initialized() || tr == Transition::Left {
                    pid.reset_to(current, Some(t));
                }
                pid.update(t, dt, lo, 100.0)
            }),
            PolicyRt::Fixed(_) => None,
        };
        let fixed = match &self.policy {
            PolicyRt::Fixed(spec) => Some(*spec),
            _ => None,
        };
        let demand = match (fixed, demand) {
            (Some(spec), _) => self.pct(spec, outputs),
            (None, Some(d)) => d,
            // No usable reading but no fault either: only optional sensors and
            // min_sensors allows that, so this zone asks for nothing beyond its floor.
            (None, None) => 0.0,
        };
        self.demand = Some(demand);

        if let Some(ov) = self.override_ {
            if now >= ov.until {
                log_info!("zone {}: manual override expired", self.name);
                self.override_ = None;
            } else {
                self.duty = self.pct(ov.duty, outputs);
                self.ramp.mark_increase(now);
                return;
            }
        }
        let target = demand.max(lo);
        self.duty = self.ramp.step(self.duty, target, now, dt);
    }

    /// Anti-windup for target zones: feed back the duty the zone's fans
    /// actually got this tick (after its ramp, the max over zones and the
    /// output clamp). Of several outputs the least driven one counts: there
    /// the zone has the most say, the others may be held up by other zones.
    fn track(&mut self, outputs: &[OutputRt]) {
        if let PolicyRt::Target(pid) = &mut self.policy {
            if let Some(a) = self.outputs.iter().filter_map(|&o| outputs[o].duty).reduce(f64::min) {
                pid.track(a);
            }
        }
    }
}

// ---------------------------------------------------------------- engine

pub struct Engine {
    pub cfg: Config,
    root: PathBuf,
    pub dry_run: bool,
    epoch: Instant,
    pub sensors: Vec<SensorRt>,
    pub outputs: Vec<OutputRt>,
    pub zones: Vec<ZoneRt>,
    last_tick: Option<f64>,
    pub ticks: u64,
}

impl Engine {
    pub fn new(cfg: Config, opts: Options) -> Result<Engine> {
        let bus: Arc<dyn Bus> = opts
            .i2c
            .clone()
            .unwrap_or_else(|| Arc::new(i2c::System::new(&opts.root)));
        let mut sensors = Vec::new();
        let mut sensor_idx = BTreeMap::new();
        for (name, sc) in &cfg.sensors {
            sensor_idx.insert(name.clone(), sensors.len());
            sensors.push(SensorRt::new(name, sc, cfg.sensor_grace(sc), opts.exec_mode, &bus)?);
        }
        let mut outputs = Vec::new();
        let mut output_idx = BTreeMap::new();
        for (name, oc) in &cfg.outputs {
            output_idx.insert(name.clone(), outputs.len());
            outputs.push(OutputRt::new(name, oc, &opts.calibration, &bus)?);
        }
        let zones = cfg
            .zones
            .iter()
            .map(|(n, z)| ZoneRt::new(n, z, &cfg, &sensor_idx, &output_idx))
            .collect::<Result<Vec<_>>>()?;
        // A held value must not mask a zone's critical either (the zone sees
        // value + offset).
        for z in &zones {
            if let Some(c) = z.critical {
                for zs in &z.sensors {
                    let s = &mut sensors[zs.idx];
                    let lim = c - zs.offset;
                    s.grace_ceiling = Some(s.grace_ceiling.map_or(lim, |x| x.min(lim)));
                }
            }
        }
        Ok(Engine {
            root: opts.root,
            dry_run: opts.dry_run,
            epoch: opts.epoch,
            sensors,
            outputs,
            zones,
            cfg,
            last_tick: None,
            ticks: 0,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Seconds since the engine's epoch (the daemon's `now`).
    pub fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    /// Start exec sensor workers (threaded mode only).
    pub fn start(&mut self) {
        for s in &mut self.sensors {
            if let Source::Exec(x) = &mut s.src {
                x.start(self.epoch, &s.name);
            }
        }
    }

    pub fn read_sensors(&mut self, now: f64) {
        for s in &mut self.sensors {
            s.update(now, &self.root);
        }
    }

    pub fn tick(&mut self, now: f64) {
        let dt = self.last_tick.map_or(self.cfg.interval.0, |t| (now - t).max(0.0));
        self.last_tick = Some(now);
        self.ticks += 1;
        self.read_sensors(now);
        for o in &mut self.outputs {
            o.feedback(now, &self.root, self.dry_run);
        }
        for z in &mut self.zones {
            z.update(now, dt, &self.sensors, &self.outputs, &self.cfg.profiles);
        }
        let failsafe = self.cfg.failsafe_duty.0;
        let zones = &self.zones;
        for (i, o) in self.outputs.iter_mut().enumerate() {
            let demand = zones
                .iter()
                .filter(|z| z.outputs.contains(&i))
                .map(|z| z.duty)
                .reduce(f64::max)
                .unwrap_or(failsafe);
            o.track_min(demand, || requesters(zones, i));
            o.apply(demand, now, &self.root, self.dry_run, failsafe);
        }
        for z in &mut self.zones {
            z.track(&self.outputs);
        }
    }

    /// Any zone in failsafe?
    pub fn failsafe(&self) -> bool {
        self.zones.iter().any(|z| z.guard.active())
    }

    /// Write `duty` to every output (then exit_enable). Returns the errors.
    pub fn shutdown(&mut self, duty: f64, now: f64) -> Vec<String> {
        let mut errs = Vec::new();
        for o in &mut self.outputs {
            if self.dry_run {
                log_info!("output {}: dry run, would set {duty}%", o.name);
                continue;
            }
            match o.force(duty, now, &self.root) {
                Ok(raw) if o.below_min(duty) => log_warn!(
                    "output {}: {} is below its hardware minimum ({}); set to {} (raw {raw})",
                    o.name,
                    hwmin::pct(duty),
                    o.min_desc,
                    hwmin::pct(o.clamp(duty))
                ),
                Ok(raw) => log_info!("output {}: set to {:.0}% (raw {raw})", o.name, o.clamp(duty)),
                Err(e) => {
                    log_error!("output {}: could not set exit duty: {e}", o.name);
                    errs.push(format!("{}: {e}", o.name));
                    continue;
                }
            }
            if let Err(e) = o.restore_enable(now, &self.root) {
                log_error!("output {}: {e}", o.name);
                errs.push(format!("{}: {e}", o.name));
            }
        }
        errs
    }

    fn zone_indices(&self, zone: Option<&str>) -> Result<Vec<usize>> {
        match zone {
            None => Ok((0..self.zones.len()).collect()),
            Some(z) => self
                .zones
                .iter()
                .position(|x| x.name == z)
                .map(|i| vec![i])
                .ok_or_else(|| Error(format!("unknown zone {z:?}"))),
        }
    }

    /// Manual override of one or all zones for `secs` (capped by max_override).
    /// Failsafe still wins over an override.
    pub fn set_override(&mut self, zone: Option<&str>, duty: DutySpec, secs: f64, now: f64) -> Result<String> {
        if secs.is_nan() || secs <= 0.0 {
            return Err(Error("override duration must be > 0".into()));
        }
        let secs = secs.min(self.cfg.max_override.0);
        let idx = self.zone_indices(zone)?;
        for i in &idx {
            self.zones[*i].override_ = Some(Override {
                duty,
                until: now + secs,
            });
        }
        let names: Vec<&str> = idx.iter().map(|i| self.zones[*i].name.as_str()).collect();
        let mut msg = format!("override {duty} for {secs:.0}s on {}", names.join(", "));
        // Still accepted; the outputs clamp it. Say so.
        let mut raised: Vec<String> = Vec::new();
        for i in &idx {
            let z = &self.zones[*i];
            let Some(p) = spec_to_pct(duty, &z.outputs, &self.outputs) else {
                continue;
            };
            for &oi in &z.outputs {
                let o = &self.outputs[oi];
                let d = format!("{} ({})", o.name, o.min_desc);
                if o.below_min(p.clamp(0.0, 100.0)) && !raised.contains(&d) {
                    raised.push(d);
                }
            }
        }
        if raised.is_empty() {
            log_notice!("{msg}");
        } else {
            msg.push_str(&format!(
                "; below the hardware minimum, raised to it on output{} {}",
                if raised.len() > 1 { "s" } else { "" },
                raised.join(", ")
            ));
            log_warn!("{msg}");
        }
        Ok(msg)
    }

    pub fn clear_override(&mut self, zone: Option<&str>) -> Result<String> {
        let idx = self.zone_indices(zone)?;
        for i in &idx {
            self.zones[*i].override_ = None;
        }
        let msg = format!(
            "override cleared on {}",
            idx.iter()
                .map(|i| self.zones[*i].name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        log_notice!("{msg}");
        Ok(msg)
    }

    pub fn zone(&self, name: &str) -> Option<&ZoneRt> {
        self.zones.iter().find(|z| z.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&OutputRt> {
        self.outputs.iter().find(|o| o.name == name)
    }

    pub fn status(&self, now: f64) -> Status {
        let zones = self
            .zones
            .iter()
            .map(|z| {
                let (state, reasons) = match z.guard.state() {
                    None => ("normal", Vec::new()),
                    Some(s) if s.calm_since.is_some() => ("recovering", s.reasons.clone()),
                    Some(s) => ("failsafe", s.reasons.clone()),
                };
                (
                    z.name.clone(),
                    ZoneStatus {
                        state,
                        reasons,
                        temperature: z.temperature.map(r1),
                        smoothed: z.smoothed.map(r1),
                        policy: z.policy_desc.clone(),
                        profile: z.profile.to_string(),
                        demand: z.demand.map(r1),
                        floor: r1(z.floor),
                        duty: r1(z.duty),
                        override_duty: z.override_.map(|o| o.duty.to_string()),
                        override_remaining: z.override_.map(|o| (o.until - now).max(0.0).round()),
                    },
                )
            })
            .collect();
        let sensors = self
            .sensors
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    SensorStatus {
                        value: s.value.map(r1),
                        error: s.error.clone(),
                        required: s.required,
                        path: s.location(),
                        age: s.age.map(r1),
                        in_grace: s.in_grace,
                    },
                )
            })
            .collect();
        let outputs = self
            .outputs
            .iter()
            .map(|o| {
                let rotors = o
                    .tach
                    .as_ref()
                    .map(|t| {
                        t.rotors
                            .iter()
                            .map(|r| RotorStatus {
                                input: r.input.clone(),
                                label: r.label.clone(),
                                rpm: r.rpm,
                                expected: r.expected.map(f64::round),
                                max_rpm: r.model.as_ref().map(|m| m.max_rpm().round()),
                                percent_of_max: match (r.rpm, r.model.as_ref()) {
                                    (Some(rpm), Some(m)) if m.max_rpm() > 0.0 => Some(r1(rpm * 100.0 / m.max_rpm())),
                                    _ => None,
                                },
                                calibrated: r.model.as_ref().is_some_and(|m| m.calibrated),
                                fault: r.fault_flag,
                                error: r.error.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (
                    o.name.clone(),
                    OutputStatus {
                        duty: o.duty.map(r1),
                        demand: o.demand.map(r1),
                        hardware_min: r1(o.min_duty),
                        clamped: o.clamped,
                        raw: o.raw,
                        observed_raw: o.observed_raw,
                        path: o.location(),
                        faults: o.faults.iter().map(|f| f.msg.clone()).collect(),
                        warnings: o.warnings.iter().map(|f| f.msg.clone()).collect(),
                        rotors,
                    },
                )
            })
            .collect();
        Status {
            version: env!("CARGO_PKG_VERSION"),
            uptime: r1(now),
            dry_run: self.dry_run,
            ticks: self.ticks,
            failsafe: self.failsafe(),
            zones,
            sensors,
            outputs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(yaml: &str) -> OutputRt {
        let c: OutputConfig = serde_saphyr::from_str(yaml).unwrap();
        let bus: Arc<dyn Bus> = Arc::new(NoBus);
        OutputRt::new("o", &c, &Calibration::default(), &bus).unwrap()
    }

    #[test]
    fn percent_to_raw_rounds_up_and_respects_hardware_range() {
        let o = output("{ type: file, path: /x, raw_min: 128 }");
        assert_eq!(o.to_raw(100.0), 255);
        assert_eq!(o.to_raw(80.0), 204);
        assert_eq!(o.to_raw(50.0), 128);
        assert_eq!(o.to_raw(10.0), 128, "never below raw_min");
        assert_eq!(o.to_raw(0.0), 128, "no stop capability");
        assert!((o.clamp(10.0) - 50.196).abs() < 0.01);
        assert_eq!(o.clamp(150.0), 100.0);
        let o = output("{ type: file, path: /x, stop: true, min_duty: 20, max_duty: 90 }");
        assert_eq!(o.to_raw(0.0), 0);
        assert_eq!(o.clamp(0.0), 0.0);
        assert_eq!(o.clamp(5.0), 20.0);
        assert_eq!(o.clamp(95.0), 90.0);
        assert_eq!(o.to_raw(20.0), 51);
        let o = output("{ type: exec, command: [x, '{raw}'], raw_max: 100 }");
        assert_eq!(o.to_raw(42.2), 43);
        assert_eq!(o.to_raw(100.0), 100);
    }

    #[test]
    fn i2c_map_converts_both_ways() {
        let o = output(
            "{ type: i2c, i2c: { bus: 4, address: 0x68, register: 0x50 }, map: { points: [[1, 4.6], [63, 100]], min: 9 } }",
        );
        assert_eq!(o.to_raw(0.0), 9, "hardware minimum, no stop");
        assert_eq!(o.to_raw(17.0), 10);
        assert_eq!(o.to_raw(100.0), 63);
        assert!((o.clamp(5.0) - 16.909).abs() < 0.01);
        assert!((o.raw_to_duty(42) - 67.69).abs() < 0.01, "u01's 0x2a = 68 %");
        assert_eq!(o.raw_to_duty(0), 0.0, "below the map = off");
        assert!(
            o.below_min(10.0) && !o.below_min(16.9),
            "16.9 % rounds up to step 9 anyway"
        );
        assert_eq!(o.location().as_deref(), Some("i2c 4-0068:0x50"));
        let o = output(
            "{ type: i2c, i2c: { bus: 4, address: 0x68, register: 0x50 }, stop: true, map: { points: [[1, 4.6], [63, 100]], min: 9 } }",
        );
        assert_eq!(o.to_raw(0.0), 0, "off");
        assert!(!o.below_min(0.0));
    }
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub version: &'static str,
    pub uptime: f64,
    pub dry_run: bool,
    pub ticks: u64,
    pub failsafe: bool,
    pub zones: BTreeMap<String, ZoneStatus>,
    pub sensors: BTreeMap<String, SensorStatus>,
    pub outputs: BTreeMap<String, OutputStatus>,
}

#[derive(Debug, Serialize)]
pub struct ZoneStatus {
    pub state: &'static str,
    pub reasons: Vec<String>,
    pub temperature: Option<f64>,
    pub smoothed: Option<f64>,
    pub policy: String,
    pub profile: String,
    pub demand: Option<f64>,
    pub floor: f64,
    pub duty: f64,
    pub override_duty: Option<String>,
    pub override_remaining: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct SensorStatus {
    pub value: Option<f64>,
    pub error: Option<String>,
    pub required: bool,
    pub path: Option<String>,
    pub age: Option<f64>,
    /// The reading failed and the last good value is being held (`grace`).
    pub in_grace: bool,
}

#[derive(Debug, Serialize)]
pub struct OutputStatus {
    pub duty: Option<f64>,
    /// Requested duty before the hardware limits (max over the zones).
    pub demand: Option<f64>,
    pub hardware_min: f64,
    /// The request is below the hardware minimum and was raised to it.
    pub clamped: bool,
    pub raw: Option<u32>,
    pub observed_raw: Option<u32>,
    pub path: Option<String>,
    pub faults: Vec<String>,
    pub warnings: Vec<String>,
    pub rotors: Vec<RotorStatus>,
}

#[derive(Debug, Serialize)]
pub struct RotorStatus {
    pub input: String,
    pub label: String,
    pub rpm: Option<f64>,
    pub expected: Option<f64>,
    pub max_rpm: Option<f64>,
    pub percent_of_max: Option<f64>,
    pub calibrated: bool,
    pub fault: bool,
    pub error: Option<String>,
}
