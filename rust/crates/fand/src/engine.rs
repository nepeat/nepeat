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
    Aggregate, Config, DEFAULT_DEGRADED_MAX_FAILED, DEFAULT_KD, DEFAULT_KI, DEFAULT_KP, DutySpec, FaultAction,
    HwmonSelector, LedConfig, OutputConfig, OutputKind, Profile, ProfileDuties, RawMap, Readback, RotorFaultPolicy,
    SensorConfig, SensorKind, ZoneConfig,
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
    /// Seconds after the first update during which "never had a reading"
    /// is "not online yet" instead of a fault.
    startup_grace: f64,
    first_update: Option<f64>,
    /// No reading yet and still within `startup_grace`: not a fault.
    pub waiting: bool,
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
            startup_grace: c.startup_grace.map_or(0.0, |s| s.0),
            first_update: None,
            waiting: false,
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
        let t0 = *self.first_update.get_or_insert(now);
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
                if self.waiting {
                    self.waiting = false;
                    self.error = None;
                    log_info!("sensor {}: online after {:.0}s, {v:.1} C", self.name, now - t0);
                } else if self.in_grace {
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
                // Not online yet: no good reading since the start and still
                // within startup_grace. Its zones do not fault on it (they
                // run on their other sensors, their floor or waiting_duty).
                // A reading that is there but implausible is a fault at once.
                if self.last_good.is_none() && self.startup_grace > 0.0 && self.error_kind.is_none() {
                    if kind != "implausible" && now >= t0 && now - t0 <= self.startup_grace {
                        if !self.waiting {
                            self.waiting = true;
                            log_info!(
                                "sensor {}: not online yet ({msg}); not a fault for up to {}s after the start",
                                self.name,
                                self.startup_grace
                            );
                        }
                        self.error = Some(msg);
                        self.value = None;
                        return;
                    }
                    if self.waiting {
                        self.waiting = false;
                        log_info!(
                            "sensor {}: still no reading {:.0}s after the start (startup_grace {}s)",
                            self.name,
                            now - t0,
                            self.startup_grace
                        );
                    }
                }
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
    present: Option<Presence>,
    led: Option<Led>,
}

/// `present`: the module is present iff (register & mask) == value.
struct Presence {
    reg: i2c::Reg,
    mask: u32,
    value: u32,
    /// The last reading: None before the first one and while it fails.
    state: Option<bool>,
    /// The last successful reading (edges are between known states).
    known: Option<bool>,
    error: Option<String>,
}

/// What a module's LED shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedState {
    /// Present, rotors healthy (green).
    Ok,
    /// fand is handling the module (a recovery, a rotor that should come up
    /// after an insertion, the start or a release): `fault` blinking.
    Blink,
    /// A rotor fault fand is not (or no longer) handling: solid `fault`.
    Fault,
    Absent,
}

impl LedState {
    pub fn name(self) -> &'static str {
        match self {
            LedState::Ok => "ok",
            LedState::Blink => "blink",
            LedState::Fault => "fault",
            LedState::Absent => "absent",
        }
    }
}

/// Ticks between readbacks of an LED register that should not have changed.
pub const LED_READBACK_TICKS: u32 = 12;

/// `led`: written when the wanted state changes, and again when a periodic
/// readback differs (read-modify-write under a partial mask).
struct Led {
    reg: i2c::Reg,
    mask: u32,
    ok: u32,
    fault: u32,
    absent: u32,
    exit: Option<u32>,
    /// `blink`: (off value, period s).
    blink: Option<(u32, f64)>,
    want: Option<LedState>,
    /// The state and value last written successfully.
    written: Option<(LedState, u32)>,
    ticks_since_check: u32,
    error: Option<String>,
}

impl Led {
    /// The value of a state (blink: its `fault` phase).
    fn value(&self, s: LedState) -> u32 {
        match s {
            LedState::Ok => self.ok,
            LedState::Blink | LedState::Fault => self.fault,
            LedState::Absent => self.absent,
        }
    }

    /// What the register should hold at `now`: blink alternates `fault` and
    /// `off` every half period (one clock for every LED, so they blink together).
    fn shown(&self, s: LedState, now: f64) -> u32 {
        match (s, self.blink) {
            (LedState::Blink, Some((off, period))) => {
                if ((now / (period / 2.0)).floor() as i64).rem_euclid(2) == 0 {
                    self.fault
                } else {
                    off
                }
            }
            _ => self.value(s),
        }
    }

    fn describe(&self, s: LedState) -> String {
        match (s, self.blink) {
            (LedState::Blink, Some((off, p))) => format!("0x{:02x}/0x{off:02x} every {}s", self.fault, p / 2.0),
            _ => format!("0x{:02x}", self.value(s)),
        }
    }

    fn holds(&self, cur: u32, v: u32) -> bool {
        cur & self.mask == v & self.mask
    }

    /// Write `v` under the mask (read-modify-write unless it covers every bit).
    fn put(&self, bus: &dyn Bus, v: u32) -> std::result::Result<(), String> {
        let new = if self.mask == self.reg.width.max() {
            v
        } else {
            (self.reg.read_raw(bus)? & !self.mask) | (v & self.mask)
        };
        self.reg.write(bus, new)
    }
}

/// `isolate_on_rotor_fault`: a latched "this module is switched off".
struct Isolation {
    raw: u32,
    since: Option<f64>,
    reason: String,
    /// Released, but its rotors have not been checked at a running duty yet
    /// (it was at the isolation value): its LED stays "fault" until then.
    proving: bool,
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
        let present = match &c.present {
            None => None,
            Some(p) => Some(Presence {
                reg: p.to_reg(Some(base))?,
                mask: p.mask_or_all(),
                value: p.value.map_or(0, |v| v.0 as u32),
                state: None,
                known: None,
                error: None,
            }),
        };
        let led = match &c.led {
            None => None,
            Some(l) => Some(Self::led(l, base)?),
        };
        Ok(Some(I2cSink {
            reg,
            bus: bus.clone(),
            arm,
            expect,
            present,
            led,
        }))
    }

    fn led(l: &LedConfig, base: &crate::config::I2cRegConfig) -> std::result::Result<Led, String> {
        let rc = l.reg_config();
        Ok(Led {
            reg: rc.to_reg(Some(base))?,
            mask: rc.mask_or_all(),
            ok: l.ok.0 as u32,
            fault: l.fault.0 as u32,
            absent: l.absent.unwrap_or(l.fault).0 as u32,
            exit: l.exit.map(|v| v.0 as u32),
            blink: l.blink.as_ref().map(|b| (b.off.0 as u32, b.period.0)),
            want: None,
            written: None,
            ticks_since_check: 0,
            error: None,
        })
    }

    /// Every `expect` holds (nothing is written to the device otherwise).
    fn identity_ok(&self) -> bool {
        self.expect.iter().all(|x| x.check(self.bus.as_ref()).is_ok())
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
    /// The module's presence (`present`): None without it or while unreadable.
    pub present: Option<bool>,
    isolation: Option<Isolation>,
    /// `isolation_max_duty`.
    isolation_cap: Option<f64>,
    /// The cap is applied this tick (for its log lines).
    pub isolation_capped: bool,
    /// `isolate_grace`: after the start, a reinsertion or a release, a rotor
    /// fault neither isolates nor starts a recovery before `grace_until`.
    grace_s: f64,
    grace_until: Option<f64>,
    /// This tick's rotor faults and warnings (none while absent or in a dip).
    tach_faults: Vec<Fault>,
    /// Set by the engine during a recovery's dip: rotor checks are void.
    suppress_rotors: bool,
    /// Set by the engine while a recovery works on this output.
    pub recovering: bool,
    /// `stall_recovery` bookkeeping, for outputs of its group.
    rec: Option<RecState>,
    /// The last absent -> present edge (cleared once an attempt starts).
    inserted_at: Option<f64>,
    /// Its rotor faults are being handled (grace, a pending or running
    /// recovery): they do not count as failed rotors.
    pub pending: bool,
}

/// Per output of the `stall_recovery` group.
struct RecState {
    attempts: u32,
    max: u32,
    /// Attempts exhausted: the reason.
    failed: Option<String>,
    /// No further attempt for this stall before this (`interval`).
    next_try: Option<f64>,
}

/// Rotor fault keys that isolate (`isolate_on_rotor_fault`): a measured
/// problem of the rotor, not a read error (a dead bus is not a dead fan).
fn isolating(f: &Fault) -> bool {
    ["/min_rpm", "/follow", "/fault"].iter().any(|s| f.key.ends_with(s))
}

/// Output fault keys a `rotor_fault_policy: degraded` zone rides out: a
/// rotor problem or a module that is out (absent, isolated). Read and write
/// errors are not among them.
fn rotor_class(key: &str) -> bool {
    matches!(key, "absent" | "isolated") || ["/min_rpm", "/follow", "/fault"].iter().any(|s| key.ends_with(s))
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
            present: None,
            grace_s: c.isolate_grace(),
            grace_until: None,
            tach_faults: Vec::new(),
            suppress_rotors: false,
            recovering: false,
            rec: None,
            inserted_at: None,
            pending: false,
            isolation: c.isolate_on_rotor_fault.then(|| Isolation {
                raw: c.isolate_raw.unwrap_or(0),
                since: None,
                reason: String::new(),
                proving: false,
            }),
            isolation_cap: c.isolation_max_duty.map(|p| p.0),
            isolation_capped: false,
        })
    }

    /// `present`: Some(the state, None while unreadable), None without it.
    pub fn presence(&self) -> Option<Option<bool>> {
        match &self.sink {
            Sink::I2c(x) if x.present.is_some() => Some(self.present),
            _ => None,
        }
    }

    /// Isolated (`isolate_on_rotor_fault`): since when and why.
    pub fn isolated(&self) -> Option<(f64, &str)> {
        let i = self.isolation.as_ref()?;
        i.since.map(|s| (s, i.reason.as_str()))
    }

    /// The LED: (wanted state, the value it shows now, written, last error).
    pub fn led(&self) -> Option<(LedState, u32, bool, Option<&str>)> {
        let Sink::I2c(x) = &self.sink else { return None };
        let l = x.led.as_ref()?;
        let w = l.want?;
        let (applied, v) = match l.written {
            Some((s, v)) if s == w => (true, v),
            _ => (false, l.value(w)),
        };
        Some((w, v, applied, l.error.as_deref()))
    }

    /// The LED blinks (the daemon's LED timer needs to run).
    pub fn led_blinking(&self) -> bool {
        matches!(&self.sink, Sink::I2c(x) if x.led.as_ref().is_some_and(|l| l.want == Some(LedState::Blink) && l.blink.is_some()))
    }

    /// Stall recovery: (failed attempts so far, given up: why).
    pub fn recovery(&self) -> Option<(u32, Option<&str>)> {
        self.rec.as_ref().map(|r| (r.attempts, r.failed.as_deref()))
    }

    /// Rotors below \`min_rpm\` this tick (a stall), with the messages.
    fn stall_msgs(&self) -> Vec<&str> {
        self.tach_faults
            .iter()
            .filter(|f| f.key.ends_with("/min_rpm"))
            .map(|f| f.msg.as_str())
            .collect()
    }

    /// Labels and readings of rotors below \`min_rpm\` now ("rear 0 rpm").
    fn stalled_rotors(&self) -> Vec<String> {
        let Some(t) = &self.tach else { return Vec::new() };
        let min = t.min_rpm.unwrap_or(0.0);
        t.rotors
            .iter()
            .filter(|r| r.rpm.is_none_or(|v| v < min))
            .map(|r| match r.rpm {
                Some(v) => format!("{} {v:.0} rpm", r.label),
                None => format!("{} unreadable", r.label),
            })
            .collect()
    }

    fn rotor_readings(&self) -> String {
        self.tach
            .as_ref()
            .map(|t| {
                t.rotors
                    .iter()
                    .map(|r| format!("{} {}", r.label, r.rpm.map_or("?".into(), |v| format!("{v:.0} rpm"))))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    }

    fn in_grace(&self, now: f64) -> bool {
        self.grace_until.is_some_and(|u| now < u)
    }

    /// Rotors out of service for \`degraded_max_failed_rotors\`: every rotor of
    /// an absent or isolated module, else those with a rotor fault.
    pub fn failed_rotors(&self) -> usize {
        if self.present == Some(false) || self.isolated().is_some() {
            return self.tach.as_ref().map_or(1, |t| t.rotors.len().max(1));
        }
        if self.pending {
            return 0;
        }
        let mut labels: Vec<&str> = self
            .faults
            .iter()
            .chain(&self.warnings)
            .filter(|f| isolating(f))
            .filter_map(|f| f.key.split('/').next())
            .collect();
        labels.sort_unstable();
        labels.dedup();
        labels.len()
    }

    /// Isolate now (recovery attempts exhausted).
    fn isolate(&mut self, now: f64, reason: String) -> bool {
        let name = &self.name;
        let Some(i) = self.isolation.as_mut() else { return false };
        if i.since.is_some() {
            return true;
        }
        i.since = Some(now);
        i.reason = reason;
        log_error!(
            "output {name}: ISOLATED: {}; writing raw {} to it instead of any zone or failsafe duty until it is reinserted, released (fand release {name}) or fand restarts",
            i.reason,
            i.raw
        );
        true
    }

    /// End an isolation, or a given-up recovery, by hand (\`fand release\`);
    /// the module gets the grace again to prove itself.
    pub fn release(&mut self, now: f64, why: &str) -> std::result::Result<(), String> {
        let name = self.name.clone();
        let failed = self.rec.as_ref().is_some_and(|r| r.failed.is_some());
        if self.isolation.is_none() && self.rec.is_none() {
            return Err(format!(
                "output {name} has no isolate_on_rotor_fault and is not in stall_recovery"
            ));
        }
        let was_isolated = self.isolation.as_mut().is_some_and(|i| i.since.take().is_some());
        if !was_isolated && !failed {
            return Err(format!("output {name} is not isolated"));
        }
        if let Some(r) = self.rec.as_mut() {
            r.attempts = 0;
            r.failed = None;
        }
        if let Some(i) = self.isolation.as_mut() {
            i.proving = was_isolated;
        }
        self.grace_until = Some(now + self.grace_s);
        log_notice!(
            "output {name}: {} released ({why}); back under zone control, {:.0}s grace before a rotor fault is acted on again",
            if was_isolated { "isolation" } else { "given-up recovery" },
            self.grace_s
        );
        Ok(())
    }

    /// Read \`present\` (i2c outputs with one). Logs transitions; an
    /// absent -> present edge is a reinsertion: it releases an isolation,
    /// resets recovery attempts and restarts the grace and the follow check.
    /// Returns the state (None = no \`present\` or unreadable).
    fn update_presence(&mut self, now: f64) -> Option<bool> {
        let name = &self.name;
        let Sink::I2c(x) = &mut self.sink else { return None };
        let p = x.present.as_mut()?;
        match p.reg.read_raw(x.bus.as_ref()) {
            Ok(v) => {
                let present = v & p.mask == p.value & p.mask;
                if p.error.take().is_some() {
                    log_info!(
                        "output {name}: presence readable again: {}",
                        if present { "present" } else { "absent" }
                    );
                }
                let prev = p.known;
                p.state = Some(present);
                p.known = Some(present);
                match (prev, present) {
                    (Some(true) | None, false) => log_warn!("output {name}: module absent ({} = 0x{v:02x})", p.reg),
                    (Some(false), true) => {
                        log_notice!("output {name}: module present again ({} = 0x{v:02x})", p.reg);
                        self.last_change = now;
                        self.grace_until = Some(now + self.grace_s);
                        self.inserted_at = Some(now);
                        if let Some(r) = self.rec.as_mut() {
                            r.attempts = 0;
                            r.failed = None;
                            r.next_try = None;
                        }
                        if let Some(i) = self.isolation.as_mut() {
                            if i.since.take().is_some() {
                                i.proving = true;
                                log_notice!(
                                    "output {name}: isolation released (module reinserted); {:.0}s grace before a rotor fault is acted on again",
                                    self.grace_s
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(e) => {
                if p.error.as_ref() != Some(&e) {
                    log_warn!("output {name}: presence unknown (rotors are checked as if present): {e}");
                }
                p.error = Some(e);
                p.state = None;
            }
        }
        p.state
    }

    /// Isolate on a rotor fault of this output (see \`isolating\`), unless
    /// absent, within the grace, in a recovery's hands, or a stall that
    /// \`stall_recovery\` will try first.
    fn update_isolation(&mut self, now: f64, rotor_faults: &[Fault], absent: bool) {
        if self.isolated().is_some() || absent || self.in_grace(now) || self.recovering || self.suppress_rotors {
            return;
        }
        if self.rec.is_some() && rotor_faults.iter().any(|f| f.key.ends_with("/min_rpm")) {
            return;
        }
        let hits: Vec<&str> = rotor_faults
            .iter()
            .filter(|f| isolating(f))
            .map(|f| f.msg.as_str())
            .collect();
        if hits.is_empty() {
            return;
        }
        let reason = hits.join("; ");
        self.isolate(now, reason);
    }

    /// Work out the LED state for this tick (logged on a change):
    /// absent -> absent; isolated or given up -> solid fault; being handled
    /// (a recovery on it, a stall a recovery will try, a rotor fault within
    /// the grace, released and not yet checked) -> blink; any other rotor
    /// fault -> solid fault; else ok.
    fn want_led(&mut self, now: f64) {
        let absent = self.present == Some(false);
        let failed = self.rec.as_ref().is_some_and(|r| r.failed.is_some());
        let isolated = self.isolated().is_some();
        let proving = self.isolation.as_ref().is_some_and(|i| i.proving);
        let trouble = !self.tach_faults.is_empty();
        let pending = self
            .rec
            .as_ref()
            .is_some_and(|r| r.failed.is_none() && r.attempts < r.max && !self.stall_msgs().is_empty());
        let want = if absent {
            LedState::Absent
        } else if isolated || failed {
            LedState::Fault
        } else if self.recovering || pending || proving || (trouble && self.in_grace(now)) {
            LedState::Blink
        } else if trouble {
            LedState::Fault
        } else {
            LedState::Ok
        };
        let name = &self.name;
        let Sink::I2c(x) = &mut self.sink else { return };
        let Some(l) = x.led.as_mut() else { return };
        if l.want != Some(want) {
            // The first state is logged only if it is not "ok" (a quiet start).
            if l.want.is_some() || want != LedState::Ok {
                log_info!(
                    "output {name}: LED {} -> {} ({})",
                    l.want.map_or("unset", LedState::name),
                    want.name(),
                    l.describe(want)
                );
            }
            l.want = Some(want);
        }
    }

    /// Drive the LED (never in a dry run, never past a failing \`expect\`).
    /// Writes when the value it should show changes (a new state, a blink
    /// phase); from a tick, a steady LED is also read back every
    /// LED_READBACK_TICKS and rewritten if it differs. A failing LED write is
    /// logged, retried, and not a fault.
    fn drive_led(&mut self, now: f64, dry_run: bool, from_tick: bool) {
        let name = &self.name;
        let Sink::I2c(x) = &mut self.sink else { return };
        let Some((want, v, steady)) = x.led.as_ref().and_then(|l| {
            l.want
                .map(|w| (w, l.shown(w, now), l.written == Some((w, l.shown(w, now)))))
        }) else {
            return;
        };
        // Between ticks (the blink timer) nothing touches the bus unless the
        // blink phase changed.
        if dry_run || (steady && !from_tick) || !x.identity_ok() {
            return;
        }
        let bus = x.bus.clone();
        let Some(l) = x.led.as_mut() else { return };
        let bus = bus.as_ref();
        if l.written == Some((want, v)) {
            if !from_tick || want == LedState::Blink {
                return;
            }
            l.ticks_since_check += 1;
            if l.ticks_since_check < LED_READBACK_TICKS {
                return;
            }
            l.ticks_since_check = 0;
            match l.reg.read_raw(bus) {
                Ok(cur) if l.holds(cur, v) => return,
                Ok(cur) => log_notice!(
                    "output {name}: LED register {} reads 0x{cur:02x}, want 0x{v:02x} ({}) under mask 0x{:02x}: writing it again",
                    l.reg,
                    want.name(),
                    l.mask
                ),
                Err(e) => {
                    if l.error.as_ref() != Some(&e) {
                        log_warn!("output {name}: LED: {e}");
                    }
                    l.error = Some(e);
                    return;
                }
            }
        }
        match l.put(bus, v) {
            Ok(()) => {
                l.written = Some((want, v));
                l.ticks_since_check = 0;
                if l.error.take().is_some() {
                    log_info!("output {name}: LED writes work again");
                }
            }
            Err(e) => {
                l.written = None;
                if l.error.as_ref() != Some(&e) {
                    log_warn!("output {name}: LED: {e}");
                }
                l.error = Some(e);
            }
        }
    }

    /// On exit (not in a dry run): the LED's \`exit\` value if configured, else
    /// a blinking LED goes solid \`fault\` (nobody handles the module any
    /// more); the others stay as they are.
    fn exit_led(&mut self) -> std::result::Result<(), String> {
        let Sink::I2c(x) = &self.sink else { return Ok(()) };
        let Some(l) = x.led.as_ref() else { return Ok(()) };
        let v = match (l.exit, l.want) {
            (Some(v), _) => v,
            (None, Some(LedState::Blink)) => l.fault,
            _ => return Ok(()),
        };
        if !x.identity_ok() {
            return Err("LED exit value not written: an expect check fails".into());
        }
        l.put(x.bus.as_ref(), v)
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
        if let Some(p) = &x.present {
            out.push(match p.reg.read_raw(bus) {
                Ok(v) if v & p.mask == p.value & p.mask => (true, format!("present {} = 0x{v:02x} (present)", p.reg)),
                Ok(v) => (false, format!("present {} = 0x{v:02x} (ABSENT: a fault)", p.reg)),
                Err(e) => (true, format!("present: unreadable ({e}), checked as present")),
            });
        }
        if let Some(l) = &x.led {
            out.push(match l.reg.read_raw(bus) {
                Ok(v) => {
                    let shows = [LedState::Ok, LedState::Fault, LedState::Absent]
                        .into_iter()
                        .filter(|s| l.holds(v, l.value(*s)))
                        .map(LedState::name)
                        .collect::<Vec<_>>();
                    let shows = if shows.is_empty() {
                        "none of ok/fault/absent".to_string()
                    } else {
                        shows.join("/")
                    };
                    (true, format!("led {} = 0x{v:02x} ({shows})", l.reg))
                }
                Err(e) => (true, format!("led: unreadable ({e})")),
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
        self.grace_until.get_or_insert(now + self.grace_s);
        self.present = self.update_presence(now);
        let absent = self.present == Some(false);
        let commanded = if dry_run {
            self.observed_raw.map(|r| self.raw_to_duty(r))
        } else {
            self.duty
        };
        let since = now - self.last_change;
        let mut warnings = Vec::new();
        let mut rotor_faults = Vec::new();
        let suppress = self.suppress_rotors;
        if let Some(t) = &mut self.tach {
            let tf = t.check(now, root, commanded, since);
            if absent {
                // Nothing to check in an empty slot.
                for r in &mut t.rotors {
                    r.expected = None;
                }
            } else {
                // In a recovery's dip the duty is not the zones': only read
                // errors count.
                let tf: Vec<Fault> = if suppress {
                    tf.into_iter().filter(|f| !isolating(f)).collect()
                } else {
                    tf
                };
                rotor_faults.clone_from(&tf);
                match t.on_fault {
                    FaultAction::Failsafe => faults.extend(tf),
                    FaultAction::Warn => warnings.extend(tf),
                }
            }
        }
        if absent {
            faults.push(Fault::new("absent", "module absent"));
        }
        self.update_isolation(now, &rotor_faults, absent);
        if let Some(i) = self.isolation.as_mut() {
            if i.proving && commanded.is_some_and(|d| d > 0.0) {
                i.proving = false;
            }
        }
        let isolated = self.isolated().map(|(_, r)| r.to_string());
        if let Some(r) = &isolated {
            faults.push(Fault::new("isolated", format!("isolated: {r}")));
        }
        // A module whose rotors run again on their own (or after a manual
        // fix) starts from zero attempts.
        let healthy = !absent
            && !suppress
            && !self.recovering
            && isolated.is_none()
            && rotor_faults.is_empty()
            && commanded.is_some_and(|d| d > 0.0);
        if healthy {
            self.inserted_at = None;
        }
        if let Some(r) = self.rec.as_mut() {
            if healthy && (r.attempts > 0 || r.failed.is_some()) {
                r.attempts = 0;
                r.failed = None;
                r.next_try = None;
                log_notice!(
                    "output {}: rotors running again, stall recovery attempts reset",
                    self.name
                );
            }
        }
        // Rotors fand is still working on (just inserted or started, a
        // recovery pending or running) are not "failed" yet; they are once
        // the recovery gives up.
        let stalled = rotor_faults.iter().any(|f| f.key.ends_with("/min_rpm"));
        let in_grace = self.in_grace(now);
        self.pending = !absent
            && isolated.is_none()
            && self.rec.as_ref().is_some_and(|r| {
                r.failed.is_none() && (self.recovering || (r.attempts < r.max && (stalled || in_grace)))
            });
        self.tach_faults = rotor_faults;
        // "write" is logged where it happens; absent/isolated by their own lines.
        let quiet = |k: &str| matches!(k, "write" | "absent" | "isolated");
        for f in faults.iter().chain(&warnings) {
            if !self.faults.iter().chain(&self.warnings).any(|o| o.key == f.key) && !quiet(&f.key) {
                log_warn!("output {}: {}", self.name, f.msg);
            }
        }
        for o in self.faults.iter().chain(&self.warnings) {
            if !faults.iter().chain(&warnings).any(|f| f.key == o.key) && !quiet(&o.key) {
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
        if let Some(raw) = self.isolation.as_ref().filter(|i| i.since.is_some()).map(|i| i.raw) {
            self.apply_fixed_raw(raw, now, root, dry_run);
            return;
        }
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

    /// A fixed raw value (isolated, or a recovery's dip), whatever the zones
    /// ask; a failed write is retried but never replaced by the failsafe duty.
    fn apply_fixed_raw(&mut self, raw: u32, now: f64, root: &Path, dry_run: bool) {
        let duty = self.raw_to_duty(raw);
        if self.raw != Some(raw) && !dry_run {
            self.last_change = now;
        }
        let due = self.raw != Some(raw)
            || self.write_error.is_some()
            || self.last_write.is_none_or(|t| now - t >= self.reassert);
        if !dry_run && due {
            self.last_write = Some(now);
            match self.write_raw(raw, duty, now, root) {
                Ok(()) => {
                    if self.write_error.take().is_some() {
                        log_info!("output {}: writes work again", self.name);
                    }
                }
                Err(e) => {
                    if self.write_error.as_ref() != Some(&e) {
                        log_error!("output {}: {e}", self.name);
                    }
                    self.write_error = Some(e);
                }
            }
        }
        self.duty = Some(duty);
        self.raw = Some(raw);
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
    /// Duty asked for at least while a sensor is not online yet.
    waiting_duty: Option<f64>,
    /// Sensors of this zone that are not online yet (`startup_grace`).
    pub waiting: Vec<String>,
    pub rotor_policy: RotorFaultPolicy,
    degraded_min: Option<f64>,
    per_rotor: f64,
    max_failed: usize,
    /// degraded: the rotor faults ridden out this tick.
    pub degraded: Vec<String>,
    degraded_keys: Vec<String>,
    /// Failed rotors on the zone's outputs (see `OutputRt::failed_rotors`).
    pub failed_rotors: usize,
    /// Hottest sensor value (with its zone offset) this tick.
    pub hottest: Option<f64>,
    /// This tick's sensor and critical faults (a stall recovery waits for none).
    hard_faults: Vec<String>,
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
            waiting_duty: z.waiting_duty.map(|p| p.0),
            waiting: Vec::new(),
            rotor_policy: cfg.rotor_fault_policy(z),
            degraded_min: z.degraded_min_duty.map(|p| p.0),
            per_rotor: z.degraded_duty_per_rotor.unwrap_or(0.0),
            max_failed: z.degraded_max_failed_rotors.unwrap_or(DEFAULT_DEGRADED_MAX_FAILED),
            degraded: Vec::new(),
            degraded_keys: Vec::new(),
            failed_rotors: 0,
            hottest: None,
            hard_faults: Vec::new(),
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
        let mut waiting: Vec<String> = Vec::new();
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
                // Not online yet: neither a fault nor a value.
                None if s.waiting => waiting.push(s.name.clone()),
                None if s.required => faults.push(Fault::new(
                    format!("sensor/{}", s.name),
                    format!("sensor {}: {}", s.name, s.error.as_deref().unwrap_or("no reading")),
                )),
                None => {}
            }
        }
        if waiting != self.waiting {
            match (waiting.is_empty(), self.waiting_duty) {
                // (when the wait ended in a fault instead, the sensor and the failsafe lines say so)
                (true, _) if faults.is_empty() => {
                    log_info!("zone {}: all sensors online, resuming {}", self.name, self.policy_desc)
                }
                (true, _) => {}
                (false, Some(d)) => log_info!(
                    "zone {}: waiting for {} (not online yet): at least {} meanwhile",
                    self.name,
                    waiting.join(", "),
                    hwmin::pct(d)
                ),
                (false, None) => log_info!(
                    "zone {}: waiting for {} (not online yet): only its floor meanwhile",
                    self.name,
                    waiting.join(", ")
                ),
            }
            self.waiting = waiting;
        }
        // Sensors that are not online yet count as present here: the zone is
        // short of sensors only once their startup_grace has run out.
        if vals.len() + self.waiting.len() < self.min_sensors {
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
        self.hottest = vals.iter().map(|(v, _)| *v).reduce(f64::max);
        let degraded_policy = self.rotor_policy == RotorFaultPolicy::Degraded;
        let mut degraded: Vec<Fault> = Vec::new();
        for &oi in &self.outputs {
            let o = &outputs[oi];
            for f in &o.faults {
                let zf = Fault::new(
                    format!("output/{}/{}", o.name, f.key),
                    format!("output {}: {}", o.name, f.msg),
                );
                if degraded_policy && rotor_class(&f.key) {
                    degraded.push(zf);
                } else {
                    faults.push(zf);
                }
            }
        }
        self.failed_rotors = self.outputs.iter().map(|&o| outputs[o].failed_rotors()).sum();
        if degraded_policy && self.failed_rotors > self.max_failed {
            faults.push(Fault::new(
                "rotors",
                format!(
                    "{} failed rotors, more than degraded_max_failed_rotors {}",
                    self.failed_rotors, self.max_failed
                ),
            ));
            faults.append(&mut degraded);
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
        // Only these hold off a stall recovery; a failsafe for other reasons
        // (rotors, write errors) does not.
        self.hard_faults = faults
            .iter()
            .filter(|f| f.key.starts_with("sensor") || f.key == "critical")
            .map(|f| f.msg.clone())
            .collect();
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
        let dkeys: Vec<String> = degraded.iter().map(|f| f.key.clone()).collect();
        let dmsgs: Vec<String> = degraded.iter().map(|f| f.msg.clone()).collect();
        if !self.guard.active() && dkeys != self.degraded_keys {
            let n = self.failed_rotors;
            if self.degraded_keys.is_empty() {
                log_warn!(
                    "zone {}: DEGRADED ({n} failed rotor{}): {}; keeping {} with the remaining fans",
                    self.name,
                    if n == 1 { "" } else { "s" },
                    dmsgs.join("; "),
                    self.policy_desc
                );
            } else if dkeys.is_empty() {
                log_notice!("zone {}: no longer degraded", self.name);
            } else {
                log_warn!("zone {}: still degraded ({n} failed): {}", self.name, dmsgs.join("; "));
            }
        }
        self.degraded_keys = if self.guard.active() { Vec::new() } else { dkeys };
        self.degraded = if self.guard.active() { Vec::new() } else { dmsgs };

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
        let demand = match self.waiting_duty {
            Some(d) if !self.waiting.is_empty() => demand.max(d),
            _ => demand,
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
        let mut target = demand.max(lo);
        if !self.degraded.is_empty() {
            target = (target + self.per_rotor * self.failed_rotors as f64)
                .max(self.degraded_min.unwrap_or(0.0))
                .min(100.0);
        }
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

// ---------------------------------------------------------------- stall recovery

/// The fixed duties of a recovery's dip, for \`apply_outputs\`.
struct Dip {
    raw: u32,
    group: Vec<usize>,
    release: Vec<usize>,
}

struct Run {
    targets: Vec<usize>,
    attempt: u32,
    /// 0 = hold, 1..=ramp.len() = ramp steps, then settle (normal control).
    step: usize,
    until: f64,
    started: f64,
}

/// \`stall_recovery\` at run time: one recovery at a time.
struct Recovery {
    group: Vec<usize>,
    release: Vec<usize>,
    hold_raw: u32,
    hold: f64,
    ramp: Vec<(u32, f64)>,
    settle: f64,
    max_attempts: u32,
    interval: f64,
    min_gap: f64,
    insert_delay: f64,
    /// Zone name -> limit for its hottest sensor.
    limits: BTreeMap<String, f64>,
    run: Option<Run>,
    last_end: Option<f64>,
    last_result: Option<String>,
    /// The last "postponed" reason logged (one line per new reason).
    postponed: Option<String>,
    file: Option<PathBuf>,
    /// The state last written to \`file\`.
    saved: Option<String>,
}

impl Recovery {
    fn dipping(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.step <= self.ramp.len())
    }

    fn dip(&self) -> Option<Dip> {
        let r = self.run.as_ref()?;
        let raw = match r.step {
            0 => self.hold_raw,
            s if s <= self.ramp.len() => self.ramp[s - 1].0,
            _ => return None,
        };
        Some(Dip {
            raw,
            group: self.group.clone(),
            release: self.release.clone(),
        })
    }

    fn phase(&self) -> Option<&'static str> {
        let r = self.run.as_ref()?;
        Some(match r.step {
            0 => "hold",
            s if s <= self.ramp.len() => "ramp",
            _ => "settle",
        })
    }
}

/// Persistent recovery state of one output (\`runtime.recovery_file\`).
#[derive(Debug, Serialize, serde::Deserialize, PartialEq)]
struct SavedRec {
    attempts: u32,
    #[serde(default)]
    failed: Option<String>,
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
    recovery: Option<Recovery>,
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
        let recovery = match &cfg.stall_recovery {
            None => None,
            Some(r) => {
                let idx = |names: &[String]| -> Result<Vec<usize>> {
                    names
                        .iter()
                        .map(|n| {
                            output_idx
                                .get(n)
                                .copied()
                                .ok_or_else(|| Error(format!("stall_recovery: unknown output {n}")))
                        })
                        .collect()
                };
                let group = idx(&cfg.recovery_outputs())?;
                let release = idx(&r.release)?;
                let limits = cfg
                    .recovery_limits()
                    .map_err(|z| Error(format!("stall_recovery.max_temp: no limit for {}", z.join(", "))))?;
                let file = cfg.recovery_file();
                let saved: BTreeMap<String, SavedRec> = file
                    .as_ref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
                for &i in &group {
                    let o = &mut outputs[i];
                    let s = saved.get(&o.name);
                    o.rec = Some(RecState {
                        attempts: s.map_or(0, |s| s.attempts),
                        max: r.max_attempts,
                        failed: s.and_then(|s| s.failed.clone()),
                        next_try: None,
                    });
                    if let Some(s) = s {
                        log_notice!(
                            "output {}: stall recovery state from before the restart: {} attempt(s){}",
                            o.name,
                            s.attempts,
                            s.failed.as_ref().map_or(String::new(), |f| format!(", given up ({f})"))
                        );
                    }
                }
                Some(Recovery {
                    group,
                    release,
                    hold_raw: r.hold_raw.0 as u32,
                    hold: r.hold.0,
                    ramp: r.ramp.iter().map(|s| (s.raw.0 as u32, s.duration.0)).collect(),
                    settle: r.settle.0,
                    max_attempts: r.max_attempts,
                    interval: r.interval.0,
                    min_gap: r.min_gap.0,
                    insert_delay: r.insert_delay.0,
                    limits,
                    run: None,
                    last_end: None,
                    last_result: None,
                    postponed: None,
                    saved: file.as_ref().and_then(|p| std::fs::read_to_string(p).ok()),
                    file,
                })
            }
        };
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
            recovery,
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
        // Recovery steps that fell due since the last call (the daemon's
        // timer usually does them on time).
        self.recovery_timer(now);
        self.read_sensors(now);
        self.sync_recovery_flags();
        for o in &mut self.outputs {
            o.feedback(now, &self.root, self.dry_run);
        }
        for z in &mut self.zones {
            z.update(now, dt, &self.sensors, &self.outputs, &self.cfg.profiles);
        }
        if let Some(mut rec) = self.recovery.take() {
            self.recovery_tick(&mut rec, now);
            self.recovery = Some(rec);
        }
        self.sync_recovery_flags();
        let dip = self.recovery.as_ref().and_then(Recovery::dip);
        self.apply_outputs(now, dip.as_ref());
        for o in &mut self.outputs {
            o.want_led(now);
            o.drive_led(now, self.dry_run, true);
        }
        for z in &mut self.zones {
            z.track(&self.outputs);
        }
        self.save_recovery();
    }

    /// Between ticks (the daemon calls this every few tens of ms): recovery
    /// steps that are due, and blinking LEDs. Does nothing when neither runs.
    pub fn timers(&mut self, now: f64) {
        self.recovery_timer(now);
        if self.dry_run {
            return;
        }
        for o in &mut self.outputs {
            if o.led_blinking() {
                o.drive_led(now, false, false);
            }
        }
    }

    /// Every output's duty from the zones (with the isolation cap), or the
    /// dip's raw value for a recovery's group and 0 % for its \`release\`.
    fn apply_outputs(&mut self, now: f64, dip: Option<&Dip>) {
        let failsafe = self.cfg.failsafe_duty.0;
        let zones = &self.zones;
        let isolated: Vec<String> = self
            .outputs
            .iter()
            .filter(|o| o.isolated().is_some())
            .map(|o| o.name.clone())
            .collect();
        for (i, o) in self.outputs.iter_mut().enumerate() {
            let mut demand = zones
                .iter()
                .filter(|z| z.outputs.contains(&i))
                .map(|z| z.duty)
                .reduce(f64::max)
                .unwrap_or(failsafe);
            let cap = o.isolation_cap.filter(|_| !isolated.is_empty());
            if cap.is_some() != o.isolation_capped {
                o.isolation_capped = cap.is_some();
                match cap {
                    Some(c) => log_warn!(
                        "output {}: at most {} while {} {} isolated (isolation_max_duty)",
                        o.name,
                        hwmin::pct(c),
                        isolated.join(", "),
                        if isolated.len() > 1 { "are" } else { "is" }
                    ),
                    None => log_notice!(
                        "output {}: no output isolated, isolation_max_duty no longer applies",
                        o.name
                    ),
                }
            }
            if let Some(c) = cap {
                demand = demand.min(c);
            }
            let released = dip.is_some_and(|d| d.release.contains(&i));
            if released {
                demand = 0.0;
            }
            o.track_min(demand, || requesters(zones, i));
            match dip.filter(|d| d.group.contains(&i) && o.isolated().is_none()) {
                Some(d) => o.apply_fixed_raw(d.raw, now, &self.root, self.dry_run),
                None => o.apply(demand, now, &self.root, self.dry_run, failsafe),
            }
        }
    }

    fn sync_recovery_flags(&mut self) {
        let (dipping, group, targets) = match &self.recovery {
            Some(r) => (
                r.dipping(),
                r.group.clone(),
                r.run.as_ref().map(|x| x.targets.clone()).unwrap_or_default(),
            ),
            None => (false, Vec::new(), Vec::new()),
        };
        for (i, o) in self.outputs.iter_mut().enumerate() {
            o.suppress_rotors = dipping && group.contains(&i);
            o.recovering = targets.contains(&i);
        }
    }

    fn names(&self, idx: &[usize]) -> String {
        idx.iter()
            .map(|&i| self.outputs[i].name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Why a recovery may not run now (None: it may).
    fn recovery_guard(&self, rec: &Recovery) -> Option<String> {
        if let Some(s) = self.sensors.iter().find(|s| s.required && s.value.is_none()) {
            return Some(format!("sensor {} has no reading", s.name));
        }
        for z in &self.zones {
            if let Some(f) = z.hard_faults.first() {
                return Some(format!("zone {} has a fault: {f}", z.name));
            }
            if let (Some(h), Some(lim)) = (z.hottest, rec.limits.get(&z.name)) {
                if h >= *lim {
                    return Some(format!(
                        "zone {}: hottest sensor at {h:.1} C, at or above the limit {lim} C",
                        z.name
                    ));
                }
            }
        }
        None
    }

    /// A tick's part: isolate given-up modules, abort a dip whose guard
    /// fails, judge the result after the settle time, or start a recovery.
    fn recovery_tick(&mut self, rec: &mut Recovery, now: f64) {
        for &i in &rec.group {
            let o = &mut self.outputs[i];
            if o.present == Some(false) || o.isolated().is_some() {
                continue;
            }
            if let Some(f) = o.rec.as_ref().and_then(|r| r.failed.clone()) {
                o.isolate(now, format!("stall recovery gave up: {f}"));
            }
        }
        if let Some(run) = &rec.run {
            if rec.dipping() {
                if let Some(why) = self.recovery_guard(rec) {
                    self.recovery_abort(rec, now, &why, false);
                }
            } else if now >= run.until {
                self.recovery_finish(rec, now);
            }
            return;
        }
        if rec.last_end.is_some_and(|t| now - t < rec.min_gap) {
            return;
        }
        // Eligible: present, not given up or isolated, a rotor below
        // min_rpm, and either just reinserted (after insert_delay, whatever
        // the grace and interval) or past its grace and its interval.
        let cands: Vec<usize> = rec
            .group
            .iter()
            .copied()
            .filter(|&i| {
                let o = &self.outputs[i];
                let fresh = o.inserted_at.is_some_and(|t| now - t >= rec.insert_delay);
                o.present != Some(false)
                    && o.isolated().is_none()
                    && o.rec.as_ref().is_some_and(|r| {
                        r.failed.is_none()
                            && r.attempts < r.max
                            && (fresh || (!o.in_grace(now) && r.next_try.is_none_or(|t| now >= t)))
                    })
                    && !o.stall_msgs().is_empty()
            })
            .collect();
        if cands.is_empty() {
            rec.postponed = None;
            return;
        }
        let names = self.names(&cands);
        if self.dry_run {
            log_notice!("dry run: would start a stall recovery for {names}");
            rec.last_end = Some(now);
            return;
        }
        if let Some(why) = self.recovery_guard(rec) {
            if rec.postponed.as_ref() != Some(&why) {
                log_warn!("stall recovery for {names} postponed: {why}");
                rec.postponed = Some(why);
            }
            return;
        }
        rec.postponed = None;
        let mut attempt = 0;
        let mut why = Vec::new();
        for &i in &cands {
            let o = &mut self.outputs[i];
            why.push(format!("{}: {}", o.name, o.stall_msgs().join("; ")));
            o.inserted_at = None;
            if let Some(r) = o.rec.as_mut() {
                r.attempts += 1;
                attempt = attempt.max(r.attempts);
            }
        }
        let ramp: Vec<String> = rec
            .ramp
            .iter()
            .map(|(raw, d)| format!("0x{raw:02x} for {d}s"))
            .collect();
        log_warn!(
            "stall recovery attempt {attempt}/{} for {names} ({}): {}{} fan outputs to raw 0x{:02x} for {}s{}, then normal control and a check after {}s",
            rec.max_attempts,
            why.join("; "),
            if rec.release.is_empty() {
                String::new()
            } else {
                format!("releasing {} first, ", self.names(&rec.release))
            },
            rec.group.len(),
            rec.hold_raw,
            rec.hold,
            if ramp.is_empty() {
                String::new()
            } else {
                format!(", then {}", ramp.join(", "))
            },
            rec.settle
        );
        rec.run = Some(Run {
            targets: cands,
            attempt,
            step: 0,
            until: now + rec.hold,
            started: now,
        });
        self.recovery_write(rec, now);
    }

    /// Write the dip: the \`release\` outputs first (a force-to-max would void
    /// it), then the group; a duty register that does not read back what was
    /// written (still forced) aborts.
    fn recovery_write(&mut self, rec: &mut Recovery, now: f64) {
        self.sync_recovery_flags_with(rec);
        let Some(dip) = rec.dip() else { return };
        let failsafe = self.cfg.failsafe_duty.0;
        for &i in &dip.release {
            let o = &mut self.outputs[i];
            o.track_min(0.0, String::new);
            o.apply(0.0, now, &self.root, self.dry_run, failsafe);
            if let Some(e) = &o.write_error {
                let why = format!("releasing output {} failed: {e}", o.name);
                self.recovery_abort(rec, now, &why, false);
                return;
            }
        }
        for &i in &dip.group {
            let o = &mut self.outputs[i];
            if o.isolated().is_some() {
                continue;
            }
            o.apply_fixed_raw(dip.raw, now, &self.root, self.dry_run);
            // A write error is the bus, not the module: not counted. A duty
            // register that reads back otherwise is a dip that cannot be
            // made (a force still on): counted, so it ends.
            let bad = match (&o.write_error, o.observed_raw) {
                (Some(e), _) => Some((format!("output {}: {e}", o.name), false)),
                (None, Some(got)) if o.readback != Readback::Off && got != dip.raw => Some((
                    format!(
                        "output {} reads raw {got} after writing {} (is a force-to-max still on?)",
                        o.name, dip.raw
                    ),
                    true,
                )),
                _ => None,
            };
            if let Some((why, counted)) = bad {
                self.recovery_abort(rec, now, &why, counted);
                return;
            }
        }
    }

    fn sync_recovery_flags_with(&mut self, rec: &Recovery) {
        let targets = rec.run.as_ref().map(|x| x.targets.clone()).unwrap_or_default();
        for (i, o) in self.outputs.iter_mut().enumerate() {
            o.suppress_rotors = rec.dipping() && rec.group.contains(&i);
            o.recovering = targets.contains(&i);
        }
    }

    /// Stop a recovery and go back to normal control at once. \`counted\`:
    /// the attempt counts (the dip could not be made); a guard abort does not.
    fn recovery_abort(&mut self, rec: &mut Recovery, now: f64, why: &str, counted: bool) {
        let Some(run) = rec.run.take() else { return };
        for &i in &run.targets {
            if let Some(r) = self.outputs[i].rec.as_mut() {
                if counted {
                    r.next_try = Some(now + rec.interval);
                } else {
                    r.attempts = r.attempts.saturating_sub(1);
                }
            }
        }
        log_warn!(
            "stall recovery attempt {} for {} aborted after {:.0}s: {why}; back to normal control{}",
            run.attempt,
            self.names(&run.targets),
            now - run.started,
            if counted { "" } else { " (the attempt does not count)" }
        );
        rec.last_result = Some(format!("aborted: {why}"));
        rec.last_end = Some(now);
        self.sync_recovery_flags_with(rec);
        self.apply_outputs(now, None);
    }

    /// After the settle time: each target recovered, still stalled (another
    /// attempt after \`interval\`), or given up after \`max_attempts\`.
    fn recovery_finish(&mut self, rec: &mut Recovery, now: f64) {
        let Some(run) = rec.run.take() else { return };
        let mut results = Vec::new();
        for &i in &run.targets {
            let o = &mut self.outputs[i];
            let name = o.name.clone();
            if o.present == Some(false) {
                results.push(format!("{name} absent"));
                continue;
            }
            let stalled = o.stalled_rotors();
            let readings = o.rotor_readings();
            let Some(r) = o.rec.as_mut() else { continue };
            if stalled.is_empty() {
                r.attempts = 0;
                r.next_try = None;
                log_notice!(
                    "stall recovery: output {name} recovered after attempt {} ({readings})",
                    run.attempt
                );
                results.push(format!("{name} recovered"));
            } else if r.attempts >= r.max {
                let f = format!("{} after {} attempts", stalled.join(", "), r.attempts);
                r.failed = Some(f.clone());
                let iso = o.isolation.is_some();
                log_error!(
                    "stall recovery: output {name} GIVEN UP ({f}); {}",
                    if iso {
                        "isolating it"
                    } else {
                        "its LED stays solid fault"
                    }
                );
                o.isolate(now, format!("stall recovery gave up: {f}"));
                results.push(format!("{name} given up"));
            } else {
                r.next_try = Some(now + rec.interval);
                log_warn!(
                    "stall recovery: output {name} still stalled after attempt {}/{} ({}); next attempt in {}s at the earliest",
                    r.attempts,
                    r.max,
                    stalled.join(", "),
                    rec.interval
                );
                results.push(format!("{name} still stalled"));
            }
        }
        rec.last_result = Some(results.join(", "));
        rec.last_end = Some(now);
    }

    /// Advance a running recovery's dip by the clock.
    fn recovery_timer(&mut self, now: f64) {
        let Some(mut rec) = self.recovery.take() else { return };
        while let Some(run) = rec.run.as_mut() {
            if run.step > rec.ramp.len() || now < run.until {
                break;
            }
            run.step += 1;
            if run.step <= rec.ramp.len() {
                let (raw, d) = rec.ramp[run.step - 1];
                run.until += d;
                log_info!("stall recovery: raw 0x{raw:02x} for {d}s");
                self.recovery_write(&mut rec, now);
            } else {
                run.until = now + rec.settle;
                let t = self.names(&run.targets);
                log_info!(
                    "stall recovery: dip over, back to normal control; checking {t} in {}s",
                    rec.settle
                );
                self.sync_recovery_flags_with(&rec);
                self.apply_outputs(now, None);
            }
        }
        self.recovery = Some(rec);
    }

    /// Keep attempts and given-up modules in \`runtime.recovery_file\` (so a
    /// restart does not start over with modules that are known dead).
    fn save_recovery(&mut self) {
        let Some(rec) = self.recovery.as_mut() else { return };
        let Some(p) = rec.file.clone() else { return };
        let map: BTreeMap<&str, SavedRec> = rec
            .group
            .iter()
            .filter_map(|&i| {
                let o = &self.outputs[i];
                let r = o.rec.as_ref()?;
                (r.attempts > 0 || r.failed.is_some()).then(|| {
                    (
                        o.name.as_str(),
                        SavedRec {
                            attempts: r.attempts,
                            failed: r.failed.clone(),
                        },
                    )
                })
            })
            .collect();
        let text = serde_json::to_string_pretty(&map).unwrap_or_default() + "\n";
        if rec.saved.as_deref() == Some(text.as_str()) || self.dry_run {
            return;
        }
        if let Err(e) = crate::daemon::write_atomic(&p, text.as_bytes()) {
            // Logged once per change of the state; a restart then starts over.
            log_warn!("stall recovery state not saved: {e}");
        }
        rec.saved = Some(text);
    }

    /// Any zone in failsafe?
    pub fn failsafe(&self) -> bool {
        self.zones.iter().any(|z| z.guard.active())
    }

    /// Write `duty` to every output (then exit_enable and the LED's `exit`
    /// value); an isolated output keeps its isolation raw value. Returns the errors.
    pub fn shutdown(&mut self, duty: f64, now: f64) -> Vec<String> {
        let mut errs = Vec::new();
        for o in &mut self.outputs {
            if self.dry_run {
                log_info!("output {}: dry run, would set {duty}%", o.name);
                continue;
            }
            if let Err(e) = o.exit_led() {
                log_error!("output {}: {e}", o.name);
                errs.push(format!("{}: {e}", o.name));
            }
            if let Some(raw) = o.isolation.as_ref().filter(|i| i.since.is_some()).map(|i| i.raw) {
                match o.write_raw(raw, o.raw_to_duty(raw), now, &self.root) {
                    Ok(()) => log_warn!(
                        "output {}: isolated, left at raw {raw} instead of the exit duty",
                        o.name
                    ),
                    Err(e) => {
                        log_error!("output {}: could not write its isolation value: {e}", o.name);
                        errs.push(format!("{}: {e}", o.name));
                    }
                }
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

    /// `fand release OUTPUT`: end an isolation by hand.
    pub fn release(&mut self, output: &str, now: f64) -> Result<String> {
        let o = self
            .outputs
            .iter_mut()
            .find(|o| o.name == output)
            .ok_or_else(|| Error(format!("unknown output {output:?}")))?;
        o.release(now, "fand release").map_err(Error)?;
        Ok(format!("output {output}: isolation released"))
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
                let (state, reasons) = if state == "normal" && !z.degraded.is_empty() {
                    ("degraded", z.degraded.clone())
                } else {
                    (state, reasons)
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
                        waiting: z.waiting.clone(),
                        failed_rotors: (z.rotor_policy == RotorFaultPolicy::Degraded).then_some(z.failed_rotors),
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
                        waiting: s.waiting,
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
                        present: o.presence(),
                        led: o.led().map(|(s, raw, applied, err)| LedStatus {
                            state: s.name(),
                            raw,
                            applied,
                            error: err.map(str::to_string),
                        }),
                        isolated: o.isolation.as_ref().map(|i| i.since.is_some()),
                        isolated_reason: o.isolated().map(|(_, r)| r.to_string()),
                        isolated_since: o.isolated().map(|(s, _)| r1(s)),
                        recovering: o.recovery().map(|_| o.recovering),
                        recovery_attempts: o.recovery().map(|(a, _)| a),
                        recovery_given_up: o.recovery().and_then(|(_, f)| f.map(str::to_string)),
                    },
                )
            })
            .collect();
        let recovery = self.recovery.as_ref().map(|r| RecoveryStatus {
            active: r.run.is_some(),
            phase: r.phase(),
            attempt: r.run.as_ref().map(|x| x.attempt),
            targets: r
                .run
                .as_ref()
                .map(|x| x.targets.iter().map(|&i| self.outputs[i].name.clone()).collect())
                .unwrap_or_default(),
            since: r.run.as_ref().map(|x| r1(x.started)),
            next_allowed: r.last_end.map(|t| r1(t + r.interval)),
            last_result: r.last_result.clone(),
            postponed: r.postponed.clone(),
        });
        Status {
            version: env!("CARGO_PKG_VERSION"),
            recovery,
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
    /// `stall_recovery` (left out without it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryStatus>,
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
    /// Sensors of the zone that are not online yet (`startup_grace`); the
    /// zone is "normal" meanwhile and asks for its floor or `waiting_duty`.
    pub waiting: Vec<String>,
    /// `rotor_fault_policy: degraded`: failed rotors on the zone's outputs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_rotors: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct RecoveryStatus {
    pub active: bool,
    /// "hold", "ramp" or "settle" while active.
    pub phase: Option<&'static str>,
    pub attempt: Option<u32>,
    pub targets: Vec<String>,
    /// Uptime (s) at which it started.
    pub since: Option<f64>,
    /// No new recovery before this uptime (s).
    pub next_allowed: Option<f64>,
    pub last_result: Option<String>,
    /// Why a wanted recovery waits (a guard).
    pub postponed: Option<String>,
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
    /// No reading since the start, within `startup_grace`: not a fault yet.
    pub waiting: bool,
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
    /// `present`: the module is present (null: unreadable). Left out without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub present: Option<Option<bool>>,
    /// `led`: the state fand shows. Left out without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub led: Option<LedStatus>,
    /// `isolate_on_rotor_fault`: isolated now. Left out without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolated_reason: Option<String>,
    /// Uptime (s) at which the isolation began.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolated_since: Option<f64>,
    /// `stall_recovery` (outputs of its group only): a recovery works on it,
    /// failed attempts so far, given up (why).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovering: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_attempts: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_given_up: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LedStatus {
    /// "ok", "blink", "fault" or "absent".
    pub state: &'static str,
    /// The value for that state (under the LED's mask).
    pub raw: u32,
    /// The register holds it (written successfully).
    pub applied: bool,
    pub error: Option<String>,
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
