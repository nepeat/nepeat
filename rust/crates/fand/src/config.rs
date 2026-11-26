//! Configuration schema (YAML), defaults and validation.
//!
//! Parsing is strict (unknown keys are errors) so typos cannot silently fall
//! back to defaults. `Config::parse` both deserializes and validates; code
//! after that may rely on every invariant checked in `validate`.

use crate::curves::{self, Curve};
use crate::error::{Context, Error, Result};
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG: &str = "/etc/fand/config.yaml";

// ---------------------------------------------------------------- scalars

/// A duration in seconds; YAML accepts a number (seconds) or "500ms", "2s",
/// "5m", "1h".
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Secs(pub f64);

pub fn parse_duration(s: &str) -> std::result::Result<f64, String> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix("ms") {
        (n, 0.001)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60.0)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3600.0)
    } else {
        (s, 1.0)
    };
    let v: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("invalid duration {s:?} (use e.g. 2s, 500ms, 5m)"))?;
    if !v.is_finite() || v < 0.0 {
        return Err(format!("invalid duration {s:?}: must be >= 0"));
    }
    Ok(v * mult)
}

/// A duty: a percentage ("80", "80%") or a fan speed ("9000rpm") that is
/// converted per output through its RPM model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DutySpec {
    Percent(f64),
    Rpm(f64),
}

impl DutySpec {
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        let t = s.trim().to_ascii_lowercase();
        if let Some(n) = t.strip_suffix("rpm") {
            let v: f64 = n.trim().parse().map_err(|_| format!("invalid rpm value {s:?}"))?;
            if !v.is_finite() || v <= 0.0 {
                return Err(format!("invalid rpm value {s:?}: must be > 0"));
            }
            return Ok(DutySpec::Rpm(v));
        }
        let n = t.strip_suffix('%').unwrap_or(&t);
        let v: f64 = n
            .trim()
            .parse()
            .map_err(|_| format!("invalid duty {s:?} (use e.g. 60, 60% or 9000rpm)"))?;
        Self::percent(v)
    }

    fn percent(v: f64) -> std::result::Result<Self, String> {
        if !(0.0..=100.0).contains(&v) {
            return Err(format!("duty {v} is outside 0..100"));
        }
        Ok(DutySpec::Percent(v))
    }
}

impl fmt::Display for DutySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DutySpec::Percent(p) => write!(f, "{p}%"),
            DutySpec::Rpm(r) => write!(f, "{r}rpm"),
        }
    }
}

/// A non-negative integer, decimal or "0x" hex (register numbers, addresses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Int(pub u64);

/// A percentage only (no rpm), "80" or "80%".
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Pct(pub f64);

/// Minimum-speed profile applied after the policy.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Profile {
    None,
    Slow,
    #[default]
    Medium,
    Fast,
    Custom(DutySpec),
}

impl Profile {
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        Ok(match s.trim() {
            "none" | "off" => Profile::None,
            "slow" => Profile::Slow,
            "medium" => Profile::Medium,
            "fast" => Profile::Fast,
            other => Profile::Custom(
                DutySpec::parse(other).map_err(|e| format!("{e}; a profile is none, slow, medium, fast or a duty"))?,
            ),
        })
    }

    pub fn floor(&self, table: &ProfileDuties) -> DutySpec {
        match self {
            Profile::None => DutySpec::Percent(0.0),
            Profile::Slow => table.slow,
            Profile::Medium => table.medium,
            Profile::Fast => table.fast,
            Profile::Custom(d) => *d,
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Profile::None => f.write_str("none"),
            Profile::Slow => f.write_str("slow"),
            Profile::Medium => f.write_str("medium"),
            Profile::Fast => f.write_str("fast"),
            Profile::Custom(d) => write!(f, "{d}"),
        }
    }
}

/// Visitor accepting YAML scalars (numbers or strings) and turning them into
/// a value via a string parser.
struct ScalarVisitor<T>(&'static str, fn(&str) -> std::result::Result<T, String>);

impl<'de, T> Visitor<'de> for ScalarVisitor<T> {
    type Value = T;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<T, E> {
        (self.1)(v).map_err(E::custom)
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<T, E> {
        (self.1)(&v.to_string()).map_err(E::custom)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<T, E> {
        (self.1)(&v.to_string()).map_err(E::custom)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<T, E> {
        (self.1)(&v.to_string()).map_err(E::custom)
    }
}

macro_rules! scalar_deserialize {
    ($ty:ty, $what:expr, $parse:expr) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                d.deserialize_any(ScalarVisitor($what, $parse))
            }
        }
    };
}

scalar_deserialize!(Secs, "a duration (seconds, or e.g. 2s, 500ms, 5m)", |s| parse_duration(
    s
)
.map(Secs));
scalar_deserialize!(DutySpec, "a duty (e.g. 60, 60% or 9000rpm)", DutySpec::parse);
scalar_deserialize!(
    Profile,
    "a profile (none, slow, medium, fast or a duty)",
    Profile::parse
);
scalar_deserialize!(Int, "an integer (e.g. 104 or \"0x68\")", |s| crate::i2c::parse_int(s)
    .map(Int));
scalar_deserialize!(Pct, "a percentage 0..100", |s| match DutySpec::parse(s)? {
    DutySpec::Percent(p) => Ok(Pct(p)),
    DutySpec::Rpm(_) => Err("an rpm value is not allowed here, use a percentage".into()),
});

/// Types that can be written either as a plain string or as a mapping.
trait StrOrMap: Sized {
    type Map: for<'de> Deserialize<'de>;
    const EXPECTING: &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, String>;
    fn from_map(m: Self::Map) -> Self;
}

struct StrOrMapVisitor<T>(PhantomData<T>);

impl<'de, T: StrOrMap> Visitor<'de> for StrOrMapVisitor<T> {
    type Value = T;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(T::EXPECTING)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<T, E> {
        T::from_str(v).map_err(E::custom)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<T, A::Error> {
        T::Map::deserialize(de::value::MapAccessDeserializer::new(map)).map(T::from_map)
    }
}

macro_rules! str_or_map_deserialize {
    ($ty:ty) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                d.deserialize_any(StrOrMapVisitor::<$ty>(PhantomData))
            }
        }
    };
}

// ---------------------------------------------------------------- top level

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Control loop period.
    #[serde(default = "d_interval")]
    pub interval: Secs,
    /// Global minimum-speed profile (zones may override).
    #[serde(default)]
    pub profile: Profile,
    /// Duties behind the slow/medium/fast profile names.
    #[serde(default)]
    pub profiles: ProfileDuties,
    /// Duty used while a zone is in failsafe.
    #[serde(default = "d_full")]
    pub failsafe_duty: Pct,
    /// Duty written on exit (signal, panic, normal return) and by `fand failsafe`.
    #[serde(default = "d_full")]
    pub exit_duty: Pct,
    /// Duty every zone starts from (ramping down from there). Default: failsafe_duty.
    #[serde(default)]
    pub startup_duty: Option<Pct>,
    /// Fault-free time required before leaving failsafe.
    #[serde(default = "d_calm")]
    pub calm_period: Secs,
    /// Default `grace` of every sensor (0 = none).
    #[serde(default)]
    pub sensor_grace: Secs,
    /// Compute everything, write nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// Prefix for every device path (testing / simulation). Default "/".
    #[serde(default)]
    pub sysfs_root: Option<PathBuf>,
    /// Longest manual override `fand set` may request.
    #[serde(default = "d_max_override")]
    pub max_override: Secs,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    /// Named user curves, usable like the built-in ones.
    #[serde(default)]
    pub curves: BTreeMap<String, CurveDef>,
    /// Default `rotor_fault_policy` of every zone.
    #[serde(default)]
    pub rotor_fault_policy: RotorFaultPolicy,
    /// Stalled-rotor recovery (off without it).
    pub stall_recovery: Option<StallRecoveryConfig>,
    pub sensors: BTreeMap<String, SensorConfig>,
    pub outputs: BTreeMap<String, OutputConfig>,
    pub zones: BTreeMap<String, ZoneConfig>,
}

fn d_interval() -> Secs {
    Secs(2.0)
}
fn d_full() -> Pct {
    Pct(100.0)
}
fn d_calm() -> Secs {
    Secs(60.0)
}
fn d_max_override() -> Secs {
    Secs(3600.0)
}
fn d_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDuties {
    #[serde(default = "d_slow")]
    pub slow: DutySpec,
    #[serde(default = "d_medium")]
    pub medium: DutySpec,
    #[serde(default = "d_fast")]
    pub fast: DutySpec,
}

fn d_slow() -> DutySpec {
    DutySpec::Percent(30.0)
}
fn d_medium() -> DutySpec {
    DutySpec::Percent(45.0)
}
fn d_fast() -> DutySpec {
    DutySpec::Percent(65.0)
}

impl Default for ProfileDuties {
    fn default() -> Self {
        ProfileDuties {
            slow: d_slow(),
            medium: d_medium(),
            fast: d_fast(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    /// JSON state written every tick (null disables).
    #[serde(default = "d_state_file")]
    pub state_file: Option<PathBuf>,
    /// Control socket for `fand status` / `fand set` (null disables).
    #[serde(default = "d_socket")]
    pub socket: Option<PathBuf>,
    /// Prometheus text-format file (node_exporter textfile collector).
    #[serde(default)]
    pub prometheus_file: Option<PathBuf>,
    /// RPM calibration written by `fand calibrate`.
    #[serde(default = "d_calibration")]
    pub calibration_file: Option<PathBuf>,
    /// Period of the summary log line.
    #[serde(default = "d_log_interval")]
    pub log_interval: Secs,
    /// stall_recovery: attempts and given-up modules, kept across restarts
    /// (default: recovery.json next to state_file; null without one).
    pub recovery_file: Option<PathBuf>,
}

fn d_state_file() -> Option<PathBuf> {
    Some("/run/fand/state.json".into())
}
fn d_socket() -> Option<PathBuf> {
    Some("/run/fand/fand.sock".into())
}
fn d_calibration() -> Option<PathBuf> {
    Some("/var/lib/fand/calibration.json".into())
}
fn d_log_interval() -> Secs {
    Secs(300.0)
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        RuntimeConfig {
            state_file: d_state_file(),
            socket: d_socket(),
            prometheus_file: None,
            calibration_file: d_calibration(),
            log_interval: d_log_interval(),
            recovery_file: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveDef {
    pub points: Vec<(f64, f64)>,
    #[serde(default)]
    pub hysteresis: f64,
}

// ---------------------------------------------------------------- hwmon selector

/// Which hwmon device: by driver name, device path substring, of_node path
/// substring, explicit path, and index among several matches. A plain string
/// means `{ name: <string> }`. hwmonN numbers are not stable, so prefer name
/// plus device/of_node over path.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HwmonSelector {
    pub name: Option<String>,
    pub device: Option<String>,
    pub of_node: Option<String>,
    pub path: Option<PathBuf>,
    pub index: Option<usize>,
}

impl StrOrMap for HwmonSelector {
    type Map = HwmonSelectorMap;
    const EXPECTING: &'static str = "a hwmon name or a {name, device, of_node, path, index} mapping";
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Ok(HwmonSelector {
            name: Some(s.to_string()),
            ..Default::default()
        })
    }
    fn from_map(m: Self::Map) -> Self {
        HwmonSelector {
            name: m.name,
            device: m.device,
            of_node: m.of_node,
            path: m.path,
            index: m.index,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HwmonSelectorMap {
    name: Option<String>,
    device: Option<String>,
    of_node: Option<String>,
    path: Option<PathBuf>,
    index: Option<usize>,
}

// HwmonSelector derives Deserialize for the map form used internally; the
// public field type is this wrapper so YAML may use a string too.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selector(pub HwmonSelector);

impl StrOrMap for Selector {
    type Map = HwmonSelectorMap;
    const EXPECTING: &'static str = HwmonSelector::EXPECTING;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        HwmonSelector::from_str(s).map(Selector)
    }
    fn from_map(m: Self::Map) -> Self {
        Selector(HwmonSelector::from_map(m))
    }
}
str_or_map_deserialize!(Selector);

impl fmt::Display for HwmonSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if let Some(n) = &self.name {
            parts.push(format!("name={n}"));
        }
        if let Some(d) = &self.device {
            parts.push(format!("device~{d}"));
        }
        if let Some(o) = &self.of_node {
            parts.push(format!("of_node~{o}"));
        }
        if let Some(p) = &self.path {
            parts.push(format!("path={}", p.display()));
        }
        if let Some(i) = self.index {
            parts.push(format!("index={i}"));
        }
        write!(f, "[{}]", parts.join(" "))
    }
}

// ---------------------------------------------------------------- sensors

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    Hwmon,
    File,
    Exec,
    ThermalZone,
    I2c,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregate {
    #[default]
    Max,
    Min,
    Mean,
    WeightedMean,
    First,
}

/// How to turn exec output into a number. Without it the first token of the
/// output is parsed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseSpec {
    /// Every match is a candidate; capture group 1 if present, else the match.
    pub regex: Option<String>,
    /// JSON path: "a.b", "ports[].temp", "items[*].t", "by_port.*.temp", "[0]".
    pub json: Option<String>,
    /// Combine several candidates (max by default).
    #[serde(default)]
    pub aggregate: Aggregate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensorConfig {
    #[serde(rename = "type")]
    pub kind: SensorKind,
    /// hwmon: device selector.
    pub hwmon: Option<Selector>,
    /// hwmon: attribute, "temp1" or "temp1_input" (default temp1).
    pub input: Option<String>,
    /// hwmon: pick the tempN whose tempN_label equals this.
    pub label: Option<String>,
    /// file: path to a file holding a number.
    pub path: Option<PathBuf>,
    /// exec: argv (no shell; use ["sh", "-c", "..."] if you need one).
    pub command: Option<Vec<String>>,
    /// exec: how often to run the command (default 10s).
    pub interval: Option<Secs>,
    /// exec: kill the command after this long (default 5s).
    pub timeout: Option<Secs>,
    /// exec: output parsing.
    pub parse: Option<ParseSpec>,
    /// thermal_zone: zone type (e.g. "x86_pkg_temp"), "thermal_zone3" or "3".
    pub zone: Option<String>,
    /// i2c: the register to read.
    pub i2c: Option<I2cRegConfig>,
    /// Multiply the raw number (default 0.001 for hwmon/thermal_zone, else 1).
    pub scale: Option<f64>,
    /// Add after scaling (C).
    #[serde(default)]
    pub offset: f64,
    /// Readings outside [lo, hi] C are errors (default [-20, 150]).
    pub plausible: Option<(f64, f64)>,
    /// A failing required sensor puts its zones into failsafe; an optional one
    /// is just left out of the aggregation.
    #[serde(default = "d_true")]
    pub required: bool,
    /// A reading older than this is stale (exec default: 3*interval + timeout).
    pub max_age: Option<Secs>,
    /// At or above this temperature every zone using the sensor goes failsafe.
    pub critical: Option<f64>,
    /// After a failed reading, keep using the last good value for up to this
    /// long since it was read (default: the global `sensor_grace`, 0).
    pub grace: Option<Secs>,
    /// "Not online yet": until the sensor's first good reading, and for at
    /// most this long after the daemon started, a missing reading is not a
    /// fault (default 0: it is one from the first tick).
    pub startup_grace: Option<Secs>,
}

impl SensorConfig {
    pub fn scale(&self) -> f64 {
        self.scale.unwrap_or(match self.kind {
            SensorKind::Hwmon | SensorKind::ThermalZone => 0.001,
            SensorKind::File | SensorKind::Exec | SensorKind::I2c => 1.0,
        })
    }
    pub fn plausible(&self) -> (f64, f64) {
        self.plausible.unwrap_or((-20.0, 150.0))
    }
    pub fn exec_interval(&self) -> f64 {
        self.interval.map_or(10.0, |s| s.0)
    }
    pub fn exec_timeout(&self) -> f64 {
        self.timeout.map_or(5.0, |s| s.0)
    }
    pub fn max_age(&self) -> Option<f64> {
        match (self.max_age, self.kind) {
            (Some(a), _) => Some(a.0),
            (None, SensorKind::Exec) => Some(3.0 * self.exec_interval() + self.exec_timeout()),
            (None, _) => None,
        }
    }
}

// ---------------------------------------------------------------- outputs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Hwmon,
    File,
    Exec,
    I2c,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readback {
    /// Read back must be >= written - tolerance (drivers that round up).
    #[default]
    AtLeast,
    /// |read back - written| <= tolerance.
    Exact,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultAction {
    #[default]
    Failsafe,
    Warn,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    #[serde(rename = "type")]
    pub kind: OutputKind,
    /// hwmon: device selector.
    pub hwmon: Option<Selector>,
    /// hwmon: pwm attribute (default pwm1).
    pub pwm: Option<String>,
    /// file: path of the raw duty file.
    pub path: Option<PathBuf>,
    /// exec: argv template; {percent} and {raw} are substituted.
    pub command: Option<Vec<String>>,
    /// exec: command timeout (default 5s).
    pub timeout: Option<Secs>,
    /// Value written to pwmN_enable (or <path>_enable) before writing duties,
    /// e.g. 1 = manual on most hwmon drivers. Re-asserted if it changes.
    pub enable: Option<u32>,
    /// Value written to the enable file after the exit duty (e.g. 2 = auto).
    pub exit_enable: Option<u32>,
    /// Raw value meaning 100 %.
    #[serde(default = "d_raw_max")]
    pub raw_max: u32,
    /// Lowest raw value the hardware accepts (e.g. 128 for "50..100 % only").
    /// Part of the hardware minimum, see [`OutputConfig::min_duty`].
    #[serde(default)]
    pub raw_min: u32,
    /// Never command less than this (default: raw_min as a percentage).
    pub min_duty: Option<Pct>,
    /// Never command more than this (default 100).
    pub max_duty: Option<Pct>,
    /// The fan may be stopped (duty 0) when the demand is 0.
    #[serde(default)]
    pub stop: bool,
    #[serde(default)]
    pub readback: Readback,
    /// Raw units of slack for the readback check.
    #[serde(default)]
    pub readback_tolerance: u32,
    /// Rewrite an unchanged value this often (default: every tick for sysfs,
    /// 60s for exec).
    pub reassert: Option<Secs>,
    pub tach: Option<TachConfig>,
    /// i2c: the duty register.
    pub i2c: Option<I2cRegConfig>,
    /// i2c: raw value <-> duty table, instead of the linear raw_min..raw_max.
    pub map: Option<MapConfig>,
    /// i2c: a register that must hold `value` (under `mask`) for the duty
    /// register to take effect; `write` is written when it does not, checked
    /// before every duty write.
    pub arm: Option<I2cRegConfig>,
    /// i2c: identity checks (e.g. a version register) before any write.
    #[serde(default)]
    pub expect: Vec<I2cRegConfig>,
    /// i2c: the fan module's presence: present iff (register & mask) ==
    /// value. An absent module is a fault of the output (zones go failsafe);
    /// its rotors are not checked.
    pub present: Option<I2cRegConfig>,
    /// i2c: the module's status LED, driven from presence and rotor faults.
    pub led: Option<LedConfig>,
    /// When one of this output's own rotors faults (below min_rpm, not
    /// following, fault flag), write `isolate_raw` to it instead of any zone
    /// or failsafe duty, until the module is reinserted (`present`), `fand
    /// release` or a restart. Only for an output that drives one fan module.
    #[serde(default)]
    pub isolate_on_rotor_fault: bool,
    /// Raw value written to an isolated output (default 0).
    pub isolate_raw: Option<u32>,
    /// After the start and after a reinsertion, a rotor fault does not
    /// isolate for this long (default 15s): a module may prove itself.
    pub isolate_grace: Option<Secs>,
    /// While any output is isolated, this output runs at most this duty
    /// (an output that would drive every fan at full speed, such as a CPLD
    /// force bit, must stay below that or it would run the isolated one too).
    pub isolation_max_duty: Option<Pct>,
}

/// `led` of an i2c output: a byte register (bus/address default to the
/// output's `i2c`); `ok`, `fault`, `absent` (default `fault`) and
/// `exit` are values under `mask` (default all bits).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedConfig {
    pub bus: Option<BusSpec>,
    pub address: Option<Int>,
    pub register: Int,
    pub force: Option<bool>,
    pub mask: Option<Int>,
    pub ok: Int,
    pub fault: Int,
    pub absent: Option<Int>,
    /// Written on exit (and by `fand failsafe`); default: a blinking LED
    /// goes solid `fault`, the others are left as they are.
    pub exit: Option<Int>,
    /// "fand is handling this module" blinks between `fault` and `off`
    /// (without it that state shows solid `fault`).
    pub blink: Option<BlinkConfig>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlinkConfig {
    /// The value between the `fault` flashes (e.g. the LED off).
    pub off: Int,
    /// One on + off cycle (default 1s).
    #[serde(default = "d_blink")]
    pub period: Secs,
}

fn d_blink() -> Secs {
    Secs(1.0)
}

// ---------------------------------------------------------------- stall recovery

/// `stall_recovery`: a "calm restart" for a rotor below `min_rpm`. A
/// stopped rotor in a running chassis is windmilled backwards by the others'
/// backdraft and cannot start; with every fan of the group slowed down
/// together it can.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StallRecoveryConfig {
    /// The outputs slowed down together (default: every output with a tach,
    /// except `release`).
    pub outputs: Option<Vec<String>>,
    /// Outputs driven to 0 % first and during the dip (e.g. a "force every fan
    /// to full speed" control that would void it).
    #[serde(default)]
    pub release: Vec<String>,
    /// Raw duty of the dip (default 0).
    #[serde(default)]
    pub hold_raw: Int,
    #[serde(default = "d_hold")]
    pub hold: Secs,
    /// Steps after the hold, each a raw duty held for a while.
    #[serde(default = "d_ramp_steps")]
    pub ramp: Vec<RecoveryStep>,
    /// Normal control for this long, then the stalled rotors are checked.
    #[serde(default = "d_settle")]
    pub settle: Secs,
    /// Failed attempts before the module is given up (isolated if it has
    /// `isolate_on_rotor_fault`).
    #[serde(default = "d_attempts")]
    pub max_attempts: u32,
    /// Least time between two attempts for the same stalled module.
    #[serde(default = "d_rec_interval")]
    pub interval: Secs,
    /// Least time between the end of one dip and the start of any next.
    #[serde(default = "d_min_gap")]
    pub min_gap: Secs,
    /// A reinserted module whose rotors read below min_rpm this long after
    /// the presence edge gets an attempt at once (no `interval`, no
    /// `isolate_grace`; `min_gap` and the guards still apply).
    #[serde(default = "d_insert_delay")]
    pub insert_delay: Secs,
    /// No recovery (and an abort of a running one) while the hottest sensor
    /// of a zone is at or above this: one number for every zone or `{zone:
    /// C}`. Default per zone: its curve's first point (a target zone: its
    /// target) plus `max_temp_margin`.
    pub max_temp: Option<MaxTemp>,
    #[serde(default = "d_margin")]
    pub max_temp_margin: f64,
    /// What a module whose recovery gave up gets: `isolate` (default: duty
    /// `isolate_raw` if it has `isolate_on_rotor_fault`, else its LED only)
    /// or `follow` (it keeps the zone duty, its healthy rotor still moves
    /// air; LED solid fault, no more dips, only its dead rotors count as
    /// failed).
    #[serde(default)]
    pub given_up: GivenUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GivenUp {
    #[default]
    Isolate,
    Follow,
}

fn d_margin() -> f64 {
    5.0
}

/// Longest allowed hold plus ramp of a stall recovery (s): the fans are
/// slow for that long.
pub const MAX_RECOVERY_DIP: f64 = 30.0;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryStep {
    pub raw: Int,
    #[serde(rename = "for")]
    pub duration: Secs,
}

/// `max_temp`: one limit for every zone, or one per zone.
#[derive(Debug, Clone, PartialEq)]
pub enum MaxTemp {
    All(f64),
    Zones(BTreeMap<String, f64>),
}

impl MaxTemp {
    pub fn for_zone(&self, z: &str) -> Option<f64> {
        match self {
            MaxTemp::All(t) => Some(*t),
            MaxTemp::Zones(m) => m.get(z).copied(),
        }
    }
}

struct MaxTempVisitor;

impl<'de> Visitor<'de> for MaxTempVisitor {
    type Value = MaxTemp;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a temperature (C) or {zone: C}")
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<MaxTemp, E> {
        Ok(MaxTemp::All(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<MaxTemp, E> {
        Ok(MaxTemp::All(v as f64))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<MaxTemp, E> {
        Ok(MaxTemp::All(v as f64))
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<MaxTemp, A::Error> {
        BTreeMap::<String, f64>::deserialize(de::value::MapAccessDeserializer::new(map)).map(MaxTemp::Zones)
    }
}

impl<'de> Deserialize<'de> for MaxTemp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        d.deserialize_any(MaxTempVisitor)
    }
}

fn d_hold() -> Secs {
    Secs(10.0)
}
fn d_ramp_steps() -> Vec<RecoveryStep> {
    vec![RecoveryStep {
        raw: Int(0x30),
        duration: Secs(5.0),
    }]
}
fn d_settle() -> Secs {
    Secs(15.0)
}
fn d_attempts() -> u32 {
    3
}
fn d_rec_interval() -> Secs {
    Secs(300.0)
}
fn d_min_gap() -> Secs {
    Secs(30.0)
}
fn d_insert_delay() -> Secs {
    Secs(5.0)
}

/// What a rotor fault (stalled, slow, flagged, isolated, an absent module)
/// does to a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotorFaultPolicy {
    /// The zone goes failsafe (as every other fault).
    #[default]
    Failsafe,
    /// The zone keeps its policy with the remaining fans (state "degraded"),
    /// until more than `degraded_max_failed_rotors` rotors are out.
    Degraded,
}

/// Default `degraded_max_failed_rotors`.
pub const DEFAULT_DEGRADED_MAX_FAILED: usize = 4;

impl LedConfig {
    /// The register as an [`I2cRegConfig`] (for `to_reg` and `check`).
    pub fn reg_config(&self) -> I2cRegConfig {
        I2cRegConfig {
            bus: self.bus.clone(),
            address: self.address,
            register: self.register,
            width: I2cWidth::Byte,
            byte_order: ByteOrder::Little,
            signed: false,
            force: self.force,
            label: None,
            mask: self.mask,
            value: None,
            write: None,
            min: None,
            max: None,
        }
    }
}

/// Default `isolate_grace` (s).
pub const DEFAULT_ISOLATE_GRACE: f64 = 15.0;
/// Longest allowed `isolate_grace` (s).
pub const MAX_ISOLATE_GRACE: f64 = 600.0;

fn d_raw_max() -> u32 {
    255
}

impl OutputConfig {
    /// The output's hardware minimum: raw_min as a percentage, or min_duty
    /// if higher. Every command is clamped to it (see `crate::hwmin`).
    pub fn min_duty(&self) -> f64 {
        if let Some(m) = self.raw_map() {
            let from_map = m.duty(m.min);
            return self.min_duty.map_or(from_map, |p| p.0.max(from_map));
        }
        let from_raw = if self.raw_max > 0 {
            f64::from(self.raw_min) * 100.0 / f64::from(self.raw_max)
        } else {
            0.0
        };
        self.min_duty.map_or(from_raw, |p| p.0.max(from_raw))
    }
    pub fn max_duty(&self) -> f64 {
        self.max_duty.map_or(100.0, |p| p.0)
    }
    /// The `map` of an i2c output, when it is valid (see [`RawMap`]).
    pub fn raw_map(&self) -> Option<RawMap> {
        self.map.as_ref().and_then(|m| RawMap::new(m).ok())
    }
    pub fn reassert(&self) -> f64 {
        self.reassert
            .map_or(if self.kind == OutputKind::Exec { 60.0 } else { 0.0 }, |s| s.0)
    }
    pub fn isolate_grace(&self) -> f64 {
        self.isolate_grace.map_or(DEFAULT_ISOLATE_GRACE, |s| s.0)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TachConfig {
    /// Device holding fanN_input (default: the output's hwmon device).
    pub hwmon: Option<Selector>,
    /// Use every fanN_input of the device.
    #[serde(default)]
    pub auto: bool,
    /// Explicit inputs, "fan1" or "fan1_input".
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Max RPM at 100 % for rotors without calibration or a label rule.
    pub max_rpm: Option<f64>,
    /// Max RPM by substring of fanN_label (longest match wins), e.g.
    /// { front: 21000, rear: 18000 } for counter-rotating pairs.
    #[serde(default)]
    pub max_rpm_by_label: BTreeMap<String, f64>,
    /// A rotor below this while commanded >= check_above_duty is a fault.
    pub min_rpm: Option<f64>,
    #[serde(default)]
    pub check_above_duty: f64,
    /// A rotor below expected * (1 - tolerance) is "not following" (null disables).
    #[serde(default = "d_follow")]
    pub follow_tolerance: Option<f64>,
    /// Time after a duty change before the follow check applies.
    #[serde(default = "d_grace")]
    pub follow_grace: Secs,
    /// Read fanN_fault / fanN_alarm when present.
    #[serde(default = "d_true")]
    pub fault_attrs: bool,
    /// failsafe (default) or warn.
    #[serde(default)]
    pub on_fault: FaultAction,
    /// I2C tach registers (bus/address default to the output's `i2c`).
    #[serde(default)]
    pub i2c: Vec<I2cRegConfig>,
    /// i2c tachs: RPM = register value * scale (default 1).
    pub scale: Option<f64>,
}

// ---------------------------------------------------------------- i2c

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum I2cWidth {
    #[default]
    Byte,
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ByteOrder {
    /// SMBus order (low byte first).
    #[default]
    Little,
    Big,
}

/// An I2C bus: a Linux bus number (`3`, `"0x3"`) or `{of_node: "..."}`, the
/// adapter whose devicetree node path ends with that (i2c-mux channels are
/// numbered in probe order, so their numbers are not stable).
#[derive(Debug, Clone, PartialEq)]
pub enum BusSpec {
    Number(Int),
    OfNode(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusSpecMap {
    of_node: String,
}

struct BusSpecVisitor;

impl<'de> Visitor<'de> for BusSpecVisitor {
    type Value = BusSpec;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a bus number (e.g. 3) or {of_node: \"<devicetree path suffix>\"}")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<BusSpec, E> {
        crate::i2c::parse_int(v)
            .map(|n| BusSpec::Number(Int(n)))
            .map_err(E::custom)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<BusSpec, E> {
        Ok(BusSpec::Number(Int(v)))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<BusSpec, E> {
        self.visit_str(&v.to_string())
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<BusSpec, E> {
        self.visit_str(&v.to_string())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<BusSpec, A::Error> {
        BusSpecMap::deserialize(de::value::MapAccessDeserializer::new(map)).map(|m| BusSpec::OfNode(m.of_node))
    }
}

impl<'de> Deserialize<'de> for BusSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        d.deserialize_any(BusSpecVisitor)
    }
}

/// An I2C/SMBus register. One shape for every use (sensor, output, `arm`,
/// `expect`, tach); `validate` rejects the keys a use does not take.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct I2cRegConfig {
    /// Linux bus number (/dev/i2c-N), or the adapter by devicetree node.
    pub bus: Option<BusSpec>,
    /// 7-bit address.
    pub address: Option<Int>,
    pub register: Int,
    #[serde(default)]
    pub width: I2cWidth,
    #[serde(default)]
    pub byte_order: ByteOrder,
    /// Two's complement (sensors).
    #[serde(default)]
    pub signed: bool,
    /// Use the address even if a kernel driver has claimed it.
    pub force: Option<bool>,
    /// tach: rotor label (default fan1, fan2, ...).
    pub label: Option<String>,
    /// arm/expect: bits compared (default all).
    pub mask: Option<Int>,
    /// arm: the armed value; expect: the exact expected value.
    pub value: Option<Int>,
    /// arm: written when not armed (default `value`).
    pub write: Option<Int>,
    /// expect: inclusive range.
    pub min: Option<Int>,
    pub max: Option<Int>,
}

impl I2cRegConfig {
    /// The runtime register; bus, address and force fall back to `base`.
    pub fn to_reg(&self, base: Option<&I2cRegConfig>) -> std::result::Result<crate::i2c::Reg, String> {
        let bus = self
            .bus
            .as_ref()
            .or(base.and_then(|b| b.bus.as_ref()))
            .ok_or("needs 'bus'")?;
        let address = self.address.or(base.and_then(|b| b.address)).ok_or("needs 'address'")?;
        let (bus, of_node) = match bus {
            BusSpec::Number(n) => (u32::try_from(n.0).map_err(|_| "bus is out of range".to_string())?, None),
            BusSpec::OfNode(s) if s.trim_matches('/').trim().is_empty() => {
                return Err("bus.of_node is empty".into());
            }
            BusSpec::OfNode(s) => (0, Some(crate::i2c::intern(s.trim()))),
        };
        if address.0 > 0x7f {
            return Err(format!("address 0x{:x} is not a 7-bit address", address.0));
        }
        if self.register.0 > 0xff {
            return Err(format!("register 0x{:x} is above 0xff", self.register.0));
        }
        Ok(crate::i2c::Reg {
            target: crate::i2c::Target {
                bus,
                address: address.0 as u16,
                force: self.force.or(base.and_then(|b| b.force)).unwrap_or(false),
                of_node,
            },
            reg: self.register.0 as u8,
            width: match self.width {
                I2cWidth::Byte => crate::i2c::Width::Byte,
                I2cWidth::Word => crate::i2c::Width::Word,
            },
            big_endian: self.byte_order == ByteOrder::Big,
            signed: self.signed,
        })
    }

    fn width_max(&self) -> u64 {
        match self.width {
            I2cWidth::Byte => 0xff,
            I2cWidth::Word => 0xffff,
        }
    }

    pub fn mask_or_all(&self) -> u32 {
        self.mask.map_or(self.width_max(), |m| m.0) as u32
    }

    /// Check a use: `allowed` lists the optional keys it takes (besides
    /// bus/address/register/width/byte_order/force). Values must fit the width.
    fn check(&self, p: &str, allowed: &[&str], base: Option<&I2cRegConfig>, errs: &mut Vec<String>) {
        if let Err(m) = self.to_reg(base) {
            errs.push(format!("{p}: {m}"));
        }
        let given = [
            ("signed", self.signed),
            ("label", self.label.is_some()),
            ("mask", self.mask.is_some()),
            ("value", self.value.is_some()),
            ("write", self.write.is_some()),
            ("min", self.min.is_some()),
            ("max", self.max.is_some()),
        ];
        for (k, set) in given {
            if set && !allowed.contains(&k) {
                errs.push(format!("{p}: '{k}' is not valid here"));
            }
        }
        for (k, v) in [
            ("mask", self.mask),
            ("value", self.value),
            ("write", self.write),
            ("min", self.min),
            ("max", self.max),
        ] {
            if v.is_some_and(|v| v.0 > self.width_max()) {
                errs.push(format!("{p}: {k} does not fit a {:?} register", self.width).to_lowercase());
            }
        }
    }
}

/// `map` of an i2c output: duty per raw value, piecewise linear.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapConfig {
    /// [[raw, percent], ...]: raw strictly increasing, percent
    /// non-decreasing, the last at 100.
    pub points: Vec<(f64, f64)>,
    /// Lowest raw value ever written while running: the hardware minimum
    /// (default: the first point).
    pub min: Option<Int>,
    /// Raw value written to stop (`stop: true` outputs only; default 0).
    pub off: Option<Int>,
}

/// Validated [`MapConfig`].
#[derive(Debug, Clone, PartialEq)]
pub struct RawMap {
    pub points: Vec<(f64, f64)>,
    pub min: u32,
    pub max: u32,
    pub off: u32,
}

impl RawMap {
    pub fn new(m: &MapConfig) -> std::result::Result<RawMap, String> {
        let p = &m.points;
        if p.len() < 2 {
            return Err("map needs at least two points".into());
        }
        for &(r, d) in p {
            if !(r.is_finite() && r >= 0.0 && r.fract() == 0.0 && r <= 65535.0) {
                return Err(format!("map: raw value {r} is not an integer 0..65535"));
            }
            if !(0.0..=100.0).contains(&d) {
                return Err(format!("map: duty {d} is outside 0..100"));
            }
        }
        for w in p.windows(2) {
            if w[1].0 <= w[0].0 {
                return Err("map: raw values must increase".into());
            }
            if w[1].1 < w[0].1 {
                return Err("map: duties must not decrease".into());
            }
        }
        let (lo, hi) = (p[0].0 as u32, p[p.len() - 1].0 as u32);
        if p[p.len() - 1].1 != 100.0 {
            return Err("map: the last point must be 100%".into());
        }
        let min = m.min.map_or(u64::from(lo), |v| v.0);
        if min < u64::from(lo) || min > u64::from(hi) {
            return Err(format!("map: min {min} is outside the map's raw range {lo}..{hi}"));
        }
        let off = m.off.map_or(0, |v| v.0);
        if off > 65535 {
            return Err(format!("map: off {off} is out of range"));
        }
        Ok(RawMap {
            points: p.clone(),
            min: min as u32,
            max: hi,
            off: off as u32,
        })
    }

    /// Duty of a raw value (flat beyond the ends).
    pub fn duty(&self, raw: u32) -> f64 {
        let r = f64::from(raw);
        let p = &self.points;
        if r <= p[0].0 {
            return p[0].1;
        }
        for w in p.windows(2) {
            let ((r0, d0), (r1, d1)) = (w[0], w[1]);
            if r <= r1 {
                return d0 + (r - r0) * (d1 - d0) / (r1 - r0);
            }
        }
        p[p.len() - 1].1
    }

    /// Smallest raw value in [min, max] giving at least `duty` (never less
    /// air than asked for).
    pub fn raw_for(&self, duty: f64) -> u32 {
        let p = &self.points;
        let mut r = p[p.len() - 1].0;
        if duty <= p[0].1 {
            r = p[0].0;
        } else {
            for w in p.windows(2) {
                let ((r0, d0), (r1, d1)) = (w[0], w[1]);
                if duty <= d1 {
                    r = r0 + (duty - d0) * (r1 - r0) / (d1 - d0);
                    break;
                }
            }
        }
        let mut raw = ((r - 1e-9).ceil().max(0.0) as u32).clamp(self.min, self.max);
        while raw < self.max && self.duty(raw) < duty - 1e-9 {
            raw += 1;
        }
        raw
    }
}

fn d_follow() -> Option<f64> {
    Some(0.35)
}
fn d_grace() -> Secs {
    Secs(20.0)
}

// ---------------------------------------------------------------- zones

#[derive(Debug, Clone, PartialEq)]
pub struct ZoneSensor {
    pub sensor: String,
    pub offset: f64,
    pub weight: f64,
    /// Per-sensor setpoint for a target policy (becomes an offset).
    pub target: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneSensorMap {
    sensor: String,
    #[serde(default)]
    offset: f64,
    #[serde(default = "d_weight")]
    weight: f64,
    target: Option<f64>,
}

fn d_weight() -> f64 {
    1.0
}

impl StrOrMap for ZoneSensor {
    type Map = ZoneSensorMap;
    const EXPECTING: &'static str = "a sensor name or a {sensor, offset, weight, target} mapping";
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Ok(ZoneSensor {
            sensor: s.to_string(),
            offset: 0.0,
            weight: 1.0,
            target: None,
        })
    }
    fn from_map(m: Self::Map) -> Self {
        ZoneSensor {
            sensor: m.sensor,
            offset: m.offset,
            weight: m.weight,
            target: m.target,
        }
    }
}
str_or_map_deserialize!(ZoneSensor);

#[derive(Debug, Clone, PartialEq)]
pub enum CurveRef {
    Named(String),
    Inline { points: Vec<(f64, f64)>, hysteresis: f64 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveRefMap {
    points: Vec<(f64, f64)>,
    #[serde(default)]
    hysteresis: f64,
}

impl StrOrMap for CurveRef {
    type Map = CurveRefMap;
    const EXPECTING: &'static str = "a curve name or a {points, hysteresis} mapping";
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Ok(CurveRef::Named(s.to_string()))
    }
    fn from_map(m: Self::Map) -> Self {
        CurveRef::Inline {
            points: m.points,
            hysteresis: m.hysteresis,
        }
    }
}
str_or_map_deserialize!(CurveRef);

/// `policy: delta-balanced` (a curve name), or a mapping with exactly one of
/// `curve`, `target` or `fixed`.
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyConfig {
    pub curve: Option<CurveRef>,
    pub target: Option<f64>,
    pub kp: Option<f64>,
    pub ki: Option<f64>,
    pub kd: Option<f64>,
    pub d_filter: Option<Secs>,
    pub fixed: Option<DutySpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyMap {
    curve: Option<CurveRef>,
    target: Option<f64>,
    kp: Option<f64>,
    ki: Option<f64>,
    kd: Option<f64>,
    d_filter: Option<Secs>,
    fixed: Option<DutySpec>,
}

impl StrOrMap for PolicyConfig {
    type Map = PolicyMap;
    const EXPECTING: &'static str = "a curve name or a policy mapping (curve / target / fixed)";
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Ok(PolicyConfig {
            curve: Some(CurveRef::Named(s.to_string())),
            target: None,
            kp: None,
            ki: None,
            kd: None,
            d_filter: None,
            fixed: None,
        })
    }
    fn from_map(m: Self::Map) -> Self {
        PolicyConfig {
            curve: m.curve,
            target: m.target,
            kp: m.kp,
            ki: m.ki,
            kd: m.kd,
            d_filter: m.d_filter,
            fixed: m.fixed,
        }
    }
}
str_or_map_deserialize!(PolicyConfig);

/// Default PI gains, chosen for high-flow fans on boards with seconds-scale
/// thermal response: 4 %/C proportional, 0.05 %/(C*s) integral, no D.
pub const DEFAULT_KP: f64 = 4.0;

/// Longest allowed sensor `grace` (s).
pub const MAX_SENSOR_GRACE: f64 = 60.0;
/// Longest allowed sensor `startup_grace` (s).
pub const MAX_STARTUP_GRACE: f64 = 3600.0;
pub const DEFAULT_KI: f64 = 0.05;
pub const DEFAULT_KD: f64 = 0.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneConfig {
    pub outputs: Vec<String>,
    #[serde(default)]
    pub sensors: Vec<ZoneSensor>,
    #[serde(default)]
    pub aggregate: Aggregate,
    pub policy: PolicyConfig,
    /// Overrides the global profile.
    pub profile: Option<Profile>,
    /// Max duty increase, %/s (0 = unlimited). See [`ZoneConfig::ramp`].
    pub ramp_up: Option<f64>,
    /// Max duty decrease, %/s (0 = unlimited).
    pub ramp_down: Option<f64>,
    /// Hold after an increase before decreasing.
    pub dwell: Option<Secs>,
    /// EMA time constant on the aggregated temperature (0 = off).
    #[serde(default)]
    pub smoothing: Secs,
    /// Overrides the curve's hysteresis.
    pub hysteresis: Option<f64>,
    /// Aggregated temperature at/above which the zone goes failsafe.
    pub critical: Option<f64>,
    pub failsafe_duty: Option<Pct>,
    pub calm_period: Option<Secs>,
    /// Fewer usable sensors than this is a fault (default 1 when the zone
    /// has sensors).
    pub min_sensors: Option<usize>,
    /// Lowest duty the zone asks for while one of its sensors is not online
    /// yet (`startup_grace`); default: only its floor.
    pub waiting_duty: Option<Pct>,
    /// Overrides the global `rotor_fault_policy`.
    pub rotor_fault_policy: Option<RotorFaultPolicy>,
    /// degraded: lowest duty while degraded.
    pub degraded_min_duty: Option<Pct>,
    /// degraded: added to the duty per failed rotor (percent).
    pub degraded_duty_per_rotor: Option<f64>,
    /// degraded: more failed rotors than this is failsafe (default 4).
    pub degraded_max_failed_rotors: Option<usize>,
}

impl ZoneConfig {
    /// (ramp_up %/s, ramp_down %/s, dwell s) with defaults filled in. Curve
    /// and fixed zones: 10, 1, 10 s (up fast, down slowly). Target zones: 0,
    /// 0, 0 (off): a PI loop wants a linear actuator, and rate limits or a
    /// hold inside the loop make it hunt slowly. Explicit settings win.
    pub fn ramp(&self) -> (f64, f64, f64) {
        let pi = self.policy.target.is_some();
        let (up, down, dwell) = if pi { (0.0, 0.0, 0.0) } else { (10.0, 1.0, 10.0) };
        (
            self.ramp_up.unwrap_or(up),
            self.ramp_down.unwrap_or(down),
            self.dwell.map_or(dwell, |s| s.0),
        )
    }
}

// ---------------------------------------------------------------- loading

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path).ctx(|| format!("reading {}", path.display()))?;
        Config::parse(&text).map_err(|e| Error(format!("{}: {e}", path.display())))
    }

    pub fn parse(text: &str) -> Result<Config> {
        let cfg: Config = serde_saphyr::from_str(text).map_err(|e| Error(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn sysfs_root(&self) -> PathBuf {
        self.sysfs_root.clone().unwrap_or_else(|| PathBuf::from("/"))
    }

    /// Grace of a sensor: its own, else the global default.
    pub fn sensor_grace(&self, s: &SensorConfig) -> f64 {
        s.grace.unwrap_or(self.sensor_grace).0
    }

    /// The outputs of `stall_recovery` (empty without it).
    pub fn recovery_outputs(&self) -> Vec<String> {
        let Some(r) = &self.stall_recovery else {
            return Vec::new();
        };
        match &r.outputs {
            Some(v) => v.clone(),
            None => self
                .outputs
                .iter()
                .filter(|(n, o)| o.tach.is_some() && !r.release.contains(n))
                .map(|(n, _)| n.clone())
                .collect(),
        }
    }

    /// stall_recovery: the temperature limit of every zone with sensors
    /// (explicit, else its curve's first point or its target plus the
    /// margin). Err: the zones without one.
    pub fn recovery_limits(&self) -> std::result::Result<BTreeMap<String, f64>, Vec<String>> {
        let Some(r) = &self.stall_recovery else {
            return Ok(BTreeMap::new());
        };
        let mut out = BTreeMap::new();
        let mut missing = Vec::new();
        for (name, z) in &self.zones {
            if z.sensors.is_empty() {
                continue;
            }
            let derived = || {
                if let Some(t) = z.policy.target {
                    return Some(t);
                }
                let c = z.policy.curve.as_ref().and_then(|c| self.curve(c))?;
                c.points.first().map(|p| p.0)
            };
            match r.max_temp.as_ref().and_then(|m| m.for_zone(name)) {
                Some(t) => {
                    out.insert(name.clone(), t);
                }
                None => match derived() {
                    Some(t) => {
                        out.insert(name.clone(), t + r.max_temp_margin);
                    }
                    None => missing.push(name.clone()),
                },
            }
        }
        if missing.is_empty() { Ok(out) } else { Err(missing) }
    }

    /// The default recovery file (next to `state_file`) when `recovery_file`
    /// is set elsewhere: read once if that one does not exist yet (migration).
    pub fn recovery_file_fallback(&self) -> Option<PathBuf> {
        self.stall_recovery.as_ref()?;
        let explicit = self.runtime.recovery_file.as_ref()?;
        let d = self.runtime.state_file.as_ref()?.parent()?.join("recovery.json");
        (&d != explicit).then_some(d)
    }

    /// Where stall recovery state is kept (see `RuntimeConfig::recovery_file`).
    pub fn recovery_file(&self) -> Option<PathBuf> {
        self.stall_recovery.as_ref()?;
        self.runtime.recovery_file.clone().or_else(|| {
            self.runtime
                .state_file
                .as_ref()
                .and_then(|p| p.parent())
                .map(|d| d.join("recovery.json"))
        })
    }

    /// A zone's `rotor_fault_policy` (its own, else the global one).
    pub fn rotor_fault_policy(&self, z: &ZoneConfig) -> RotorFaultPolicy {
        z.rotor_fault_policy.unwrap_or(self.rotor_fault_policy)
    }

    pub fn startup_duty(&self) -> f64 {
        self.startup_duty.unwrap_or(self.failsafe_duty).0
    }

    /// Resolve a policy curve reference (built-in or user-defined).
    pub fn curve(&self, r: &CurveRef) -> Option<Curve> {
        match r {
            CurveRef::Named(n) => self
                .curves
                .get(n)
                .map(|c| Curve {
                    points: c.points.clone(),
                    hysteresis: c.hysteresis,
                })
                .or_else(|| curves::builtin(n)),
            CurveRef::Inline { points, hysteresis } => Some(Curve {
                points: points.clone(),
                hysteresis: *hysteresis,
            }),
        }
    }

    /// Check every cross-reference and range. All problems are reported at once.
    pub fn validate(&self) -> Result<()> {
        let mut errs: Vec<String> = Vec::new();
        let mut e = |s: String| errs.push(s);

        if !(self.interval.0 >= 0.1 && self.interval.0 <= 60.0) {
            e(format!("interval: {}s is outside 0.1..60s", self.interval.0));
        }
        for (name, c) in &self.curves {
            if curves::builtin(name).is_some() {
                e(format!("curves.{name}: shadows a built-in curve; pick another name"));
            }
            if let Err(m) = (Curve {
                points: c.points.clone(),
                hysteresis: c.hysteresis,
            })
            .validate()
            {
                e(format!("curves.{name}: {m}"));
            }
        }
        if self.max_override.0 <= 0.0 {
            e("max_override must be > 0".into());
        }
        let long = |g: f64| g > MAX_SENSOR_GRACE;
        if long(self.sensor_grace.0) {
            e(format!(
                "sensor_grace: {}s is above {MAX_SENSOR_GRACE}s; a longer hold hides a dead sensor",
                self.sensor_grace.0
            ));
        }
        for (name, s) in &self.sensors {
            if let Some(g) = s.grace.filter(|g| long(g.0)) {
                e(format!(
                    "sensors.{name}: grace {}s is above {MAX_SENSOR_GRACE}s; a longer hold hides a dead sensor",
                    g.0
                ));
            }
            if let Some(g) = s.startup_grace.filter(|g| g.0 > MAX_STARTUP_GRACE) {
                e(format!(
                    "sensors.{name}: startup_grace {}s is above {MAX_STARTUP_GRACE}s; a sensor that never comes online must become a fault",
                    g.0
                ));
            }
        }

        for (name, s) in &self.sensors {
            let p = format!("sensors.{name}");
            let has = |b: bool, field: &str, errs: &mut Vec<String>| {
                if b {
                    errs.push(format!("{p}: '{field}' is not valid for type {:?}", s.kind));
                }
            };
            let mut local = Vec::new();
            match s.kind {
                SensorKind::Hwmon => {
                    if s.hwmon.is_none() {
                        local.push(format!("{p}: type hwmon needs 'hwmon' (a device name or selector)"));
                    }
                    if s.input.is_some() && s.label.is_some() {
                        local.push(format!("{p}: give either 'input' or 'label', not both"));
                    }
                    has(s.path.is_some(), "path", &mut local);
                    has(s.command.is_some(), "command", &mut local);
                    has(s.zone.is_some(), "zone", &mut local);
                }
                SensorKind::File => {
                    if s.path.is_none() {
                        local.push(format!("{p}: type file needs 'path'"));
                    }
                    has(s.hwmon.is_some(), "hwmon", &mut local);
                    has(s.command.is_some(), "command", &mut local);
                    has(s.zone.is_some(), "zone", &mut local);
                }
                SensorKind::Exec => {
                    match &s.command {
                        None => local.push(format!("{p}: type exec needs 'command'")),
                        Some(c) if c.is_empty() || c[0].is_empty() => local.push(format!("{p}: empty command")),
                        _ => {}
                    }
                    if s.exec_interval() < 0.5 {
                        local.push(format!("{p}: interval must be >= 0.5s"));
                    }
                    if s.exec_timeout() <= 0.0 {
                        local.push(format!("{p}: timeout must be > 0"));
                    }
                    if let Some(ps) = &s.parse {
                        match (&ps.regex, &ps.json) {
                            (Some(_), Some(_)) => local.push(format!("{p}: parse takes 'regex' or 'json', not both")),
                            (Some(r), None) => {
                                if let Err(err) = regex_lite::Regex::new(r) {
                                    local.push(format!("{p}: parse.regex: {err}"));
                                }
                            }
                            (None, Some(j)) => {
                                if let Err(err) = crate::exec::JsonPath::parse(j) {
                                    local.push(format!("{p}: parse.json: {err}"));
                                }
                            }
                            (None, None) => {}
                        }
                    }
                    has(s.hwmon.is_some(), "hwmon", &mut local);
                    has(s.path.is_some(), "path", &mut local);
                    has(s.zone.is_some(), "zone", &mut local);
                }
                SensorKind::ThermalZone => {
                    if s.zone.is_none() {
                        local.push(format!("{p}: type thermal_zone needs 'zone'"));
                    }
                    has(s.hwmon.is_some(), "hwmon", &mut local);
                    has(s.path.is_some(), "path", &mut local);
                    has(s.command.is_some(), "command", &mut local);
                }
                SensorKind::I2c => {
                    match &s.i2c {
                        None => local.push(format!("{p}: type i2c needs 'i2c' (bus, address, register)")),
                        Some(r) => r.check(&format!("{p}.i2c"), &["signed"], None, &mut local),
                    }
                    has(s.hwmon.is_some(), "hwmon", &mut local);
                    has(s.path.is_some(), "path", &mut local);
                    has(s.command.is_some(), "command", &mut local);
                    has(s.zone.is_some(), "zone", &mut local);
                }
            }
            if s.kind != SensorKind::I2c {
                has(s.i2c.is_some(), "i2c", &mut local);
            }
            if s.kind != SensorKind::Exec {
                has(s.interval.is_some(), "interval", &mut local);
                has(s.timeout.is_some(), "timeout", &mut local);
                has(s.parse.is_some(), "parse", &mut local);
            }
            if s.kind != SensorKind::Hwmon {
                has(s.input.is_some(), "input", &mut local);
                has(s.label.is_some(), "label", &mut local);
            }
            let (lo, hi) = s.plausible();
            if !(lo.is_finite() && hi.is_finite() && lo < hi) {
                local.push(format!("{p}: plausible must be [lo, hi] with lo < hi"));
            }
            if !s.scale().is_finite() || s.scale() == 0.0 {
                local.push(format!("{p}: scale must be a non-zero number"));
            }
            if !s.offset.is_finite() {
                local.push(format!("{p}: offset must be a number"));
            }
            if let Some(c) = s.critical {
                if !c.is_finite() {
                    local.push(format!("{p}: critical must be a number"));
                }
            }
            errs.extend(local);
        }

        let mut e = |s: String| errs.push(s);
        for (name, o) in &self.outputs {
            let p = format!("outputs.{name}");
            match o.kind {
                OutputKind::Hwmon => {
                    if o.hwmon.is_none() {
                        e(format!("{p}: type hwmon needs 'hwmon'"));
                    }
                    if o.path.is_some() || o.command.is_some() {
                        e(format!("{p}: 'path'/'command' are not valid for type hwmon"));
                    }
                }
                OutputKind::File => {
                    if o.path.is_none() {
                        e(format!("{p}: type file needs 'path'"));
                    }
                    if o.hwmon.is_some() || o.command.is_some() || o.pwm.is_some() {
                        e(format!("{p}: 'hwmon'/'pwm'/'command' are not valid for type file"));
                    }
                }
                OutputKind::Exec => {
                    match &o.command {
                        None => e(format!("{p}: type exec needs 'command'")),
                        Some(c) if c.is_empty() || c[0].is_empty() => e(format!("{p}: empty command")),
                        Some(c) if !c.iter().any(|a| a.contains("{percent}") || a.contains("{raw}")) => {
                            e(format!("{p}: command must contain {{percent}} or {{raw}}"))
                        }
                        _ => {}
                    }
                    if o.hwmon.is_some() || o.path.is_some() || o.pwm.is_some() {
                        e(format!("{p}: 'hwmon'/'path'/'pwm' are not valid for type exec"));
                    }
                    if o.enable.is_some() || o.exit_enable.is_some() {
                        e(format!("{p}: 'enable'/'exit_enable' need a sysfs output"));
                    }
                    if o.readback != Readback::AtLeast || o.readback_tolerance != 0 {
                        e(format!("{p}: exec outputs have no readback"));
                    }
                }
                // Checked below (validate_i2c_output).
                OutputKind::I2c => {}
            }
            if o.kind == OutputKind::I2c {
                let mut local = Vec::new();
                validate_i2c_output(&p, o, &mut local);
                for m in local {
                    e(m);
                }
            } else if o.i2c.is_some() || o.map.is_some() || o.arm.is_some() || !o.expect.is_empty() {
                e(format!("{p}: 'i2c'/'map'/'arm'/'expect' are only valid for type i2c"));
            }
            if o.kind != OutputKind::I2c && (o.present.is_some() || o.led.is_some()) {
                e(format!("{p}: 'present'/'led' are only valid for type i2c"));
            }
            {
                let mut local = Vec::new();
                validate_isolation(&p, o, self.recovery_outputs().contains(name), &mut local);
                for m in local {
                    e(m);
                }
            }
            if o.isolation_max_duty.is_some() && !self.outputs.values().any(|x| x.isolate_on_rotor_fault) {
                e(format!(
                    "{p}: isolation_max_duty has no effect: no output has isolate_on_rotor_fault"
                ));
            }
            if let Some(c) = o.isolation_max_duty {
                if c.0 < o.min_duty() {
                    e(format!(
                        "{p}: isolation_max_duty {}% is below the output's hardware minimum {:.1}%",
                        c.0,
                        o.min_duty()
                    ));
                }
            }
            if o.kind != OutputKind::Exec && o.timeout.is_some() {
                e(format!("{p}: 'timeout' is only valid for type exec"));
            }
            if o.raw_max == 0 || o.raw_min > o.raw_max {
                e(format!("{p}: need 0 <= raw_min <= raw_max and raw_max > 0"));
            }
            if o.min_duty() > o.max_duty() {
                e(format!(
                    "{p}: min_duty {} is above max_duty {}",
                    o.min_duty(),
                    o.max_duty()
                ));
            }
            if let Some(t) = o.tach.as_ref().filter(|t| !t.i2c.is_empty()) {
                let mut local = Vec::new();
                validate_i2c_tach(&p, o, t, &mut local);
                for m in local {
                    e(m);
                }
            }
            if let Some(t) = o.tach.as_ref().filter(|t| t.i2c.is_empty()) {
                if t.scale.is_some() {
                    e(format!("{p}: tach.scale only applies to i2c tach inputs"));
                }
                if t.hwmon.is_none() && o.hwmon.is_none() {
                    e(format!("{p}: tach needs 'hwmon' when the output is not a hwmon device"));
                }
                if !t.auto && t.inputs.is_empty() {
                    e(format!("{p}: tach needs 'auto: true' or a list of 'inputs'"));
                }
                if t.auto && !t.inputs.is_empty() {
                    e(format!("{p}: tach takes 'auto' or 'inputs', not both"));
                }
                if let Some(f) = t.follow_tolerance {
                    if !(0.0..1.0).contains(&f) {
                        e(format!("{p}: tach.follow_tolerance must be within 0..1"));
                    }
                }
                for (k, v) in t.max_rpm.iter().map(|v| ("max_rpm".to_string(), v)).chain(
                    t.max_rpm_by_label
                        .iter()
                        .map(|(k, v)| (format!("max_rpm_by_label.{k}"), v)),
                ) {
                    if !(v.is_finite() && *v > 0.0) {
                        e(format!("{p}: tach.{k} must be > 0"));
                    }
                }
            }
        }

        let mut used_outputs = std::collections::BTreeSet::new();
        for (name, z) in &self.zones {
            let p = format!("zones.{name}");
            if z.outputs.is_empty() {
                e(format!("{p}: needs at least one output"));
            }
            for o in &z.outputs {
                if !self.outputs.contains_key(o) {
                    e(format!("{p}: unknown output '{o}'"));
                }
                used_outputs.insert(o.clone());
            }
            for s in &z.sensors {
                if !self.sensors.contains_key(&s.sensor) {
                    e(format!("{p}: unknown sensor '{}'", s.sensor));
                }
                if !(s.weight.is_finite() && s.weight > 0.0) {
                    e(format!("{p}: sensor '{}': weight must be > 0", s.sensor));
                }
                if s.target.is_some() && z.policy.target.is_none() {
                    e(format!("{p}: sensor '{}': 'target' needs a target policy", s.sensor));
                }
            }
            let pol = &z.policy;
            let kinds = [pol.curve.is_some(), pol.target.is_some(), pol.fixed.is_some()]
                .iter()
                .filter(|b| **b)
                .count();
            if kinds != 1 {
                e(format!("{p}: policy needs exactly one of curve, target, fixed"));
            }
            if pol.target.is_none()
                && (pol.kp.is_some() || pol.ki.is_some() || pol.kd.is_some() || pol.d_filter.is_some())
            {
                e(format!("{p}: kp/ki/kd/d_filter only apply to a target policy"));
            }
            for (k, v) in [("kp", pol.kp), ("ki", pol.ki), ("kd", pol.kd)] {
                if v.is_some_and(|v| !(v.is_finite() && v >= 0.0)) {
                    e(format!("{p}: policy.{k} must be >= 0"));
                }
            }
            if let Some(c) = &pol.curve {
                match self.curve(c) {
                    None => e(format!(
                        "{p}: unknown curve {c:?} (built-in: {})",
                        curves::BUILTINS.iter().map(|b| b.name).collect::<Vec<_>>().join(", ")
                    )),
                    Some(curve) => {
                        if let Err(m) = curve.validate() {
                            e(format!("{p}: curve: {m}"));
                        }
                    }
                }
            }
            if (pol.curve.is_some() || pol.target.is_some()) && z.sensors.is_empty() {
                e(format!("{p}: curve and target policies need sensors"));
            }
            if z.sensors.is_empty() && z.min_sensors.is_some_and(|n| n > 0) {
                e(format!("{p}: min_sensors > 0 but the zone has no sensors"));
            }
            if let Some(n) = z.min_sensors {
                if n > z.sensors.len() {
                    e(format!(
                        "{p}: min_sensors {n} exceeds the {} sensors listed",
                        z.sensors.len()
                    ));
                }
            }
            if z.aggregate == Aggregate::First && z.sensors.len() > 1 {
                // Allowed, but only meaningful as "primary with fallbacks".
            }
            let (up, down, _) = z.ramp();
            if !(up.is_finite() && up >= 0.0 && down.is_finite() && down >= 0.0) {
                e(format!("{p}: ramp_up / ramp_down must be >= 0 (0 = unlimited)"));
            }
            if let Some(h) = z.hysteresis {
                if !(h.is_finite() && h >= 0.0) {
                    e(format!("{p}: hysteresis must be >= 0"));
                }
            }
        }
        // max_duty would silently cap these: a failsafe that is not what it says is an error.
        let mut capped = |key: &str, duty: f64, outs: &mut dyn Iterator<Item = &String>| {
            for on in outs {
                if let Some(o) = self.outputs.get(on) {
                    if duty > o.max_duty() + 1e-9 {
                        e(format!(
                            "{key}: {duty}% is above the max_duty {}% of output {on}, which would cap it there \
                             (lower {key} or raise outputs.{on}.max_duty)",
                            o.max_duty()
                        ));
                    }
                }
            }
        };
        // The global failsafe also covers write failures on every output.
        capped("failsafe_duty", self.failsafe_duty.0, &mut self.outputs.keys());
        capped("exit_duty", self.exit_duty.0, &mut self.outputs.keys());
        if let Some(s) = self.startup_duty {
            capped("startup_duty", s.0, &mut self.outputs.keys());
        }
        for (name, z) in &self.zones {
            if let Some(f) = z.failsafe_duty {
                capped(&format!("zones.{name}.failsafe_duty"), f.0, &mut z.outputs.iter());
            }
        }
        for o in self.outputs.keys() {
            if !used_outputs.contains(o) {
                e(format!(
                    "outputs.{o}: not used by any zone (it would never be controlled)"
                ));
            }
        }
        if self.zones.is_empty() {
            e("no zones configured".into());
        }
        for (name, z) in &self.zones {
            let p = format!("zones.{name}");
            if self.rotor_fault_policy(z) != RotorFaultPolicy::Degraded
                && (z.degraded_min_duty.is_some()
                    || z.degraded_duty_per_rotor.is_some()
                    || z.degraded_max_failed_rotors.is_some())
            {
                e(format!("{p}: degraded_* keys need rotor_fault_policy: degraded"));
            }
            if let Some(d) = z.degraded_duty_per_rotor {
                if !(d.is_finite() && (0.0..=100.0).contains(&d)) {
                    e(format!("{p}: degraded_duty_per_rotor must be within 0..100"));
                }
            }
        }
        if let Some(r) = &self.stall_recovery {
            let group = self.recovery_outputs();
            if group.is_empty() {
                e("stall_recovery: no outputs (none has a tach)".into());
            }
            for n in &group {
                match self.outputs.get(n) {
                    None => e(format!("stall_recovery.outputs: unknown output '{n}'")),
                    Some(o) => {
                        if o.tach.as_ref().and_then(|t| t.min_rpm).is_none() {
                            e(format!(
                                "stall_recovery: output {n} needs a tach with min_rpm (that is what a stall is)"
                            ));
                        }
                        let top = match (o.kind, &o.i2c) {
                            (OutputKind::I2c, Some(reg)) => reg.width_max(),
                            _ => u64::from(o.raw_max),
                        };
                        for (k, v) in std::iter::once(("hold_raw", r.hold_raw.0))
                            .chain(r.ramp.iter().map(|s| ("ramp raw", s.raw.0)))
                        {
                            if v > top {
                                e(format!("stall_recovery: {k} {v} does not fit output {n} (max {top})"));
                            }
                        }
                    }
                }
                if r.release.contains(n) {
                    e(format!("stall_recovery: output {n} is in both 'outputs' and 'release'"));
                }
            }
            for n in &r.release {
                if !self.outputs.contains_key(n) {
                    e(format!("stall_recovery.release: unknown output '{n}'"));
                }
            }
            if r.max_attempts == 0 {
                e("stall_recovery: max_attempts must be >= 1".into());
            }
            if r.hold.0 <= 0.0 || r.ramp.iter().any(|s| s.duration.0 <= 0.0) || r.settle.0 <= 0.0 {
                e("stall_recovery: hold, ramp durations and settle must be > 0".into());
            }
            let dip = r.hold.0 + r.ramp.iter().map(|s| s.duration.0).sum::<f64>();
            if dip > MAX_RECOVERY_DIP {
                e(format!(
                    "stall_recovery: hold plus ramp is {dip}s, above {MAX_RECOVERY_DIP}s (the fans are slow all that time)"
                ));
            }
            if r.insert_delay.0 > 60.0 {
                e(format!(
                    "stall_recovery: insert_delay {}s is above 60s",
                    r.insert_delay.0
                ));
            }
            if !(r.max_temp_margin.is_finite() && r.max_temp_margin >= 0.0) {
                e("stall_recovery.max_temp_margin must be >= 0".into());
            }
            match &r.max_temp {
                Some(MaxTemp::All(t)) if !t.is_finite() => e("stall_recovery.max_temp must be a number".into()),
                Some(MaxTemp::Zones(m)) => {
                    for k in m.keys() {
                        if !self.zones.contains_key(k) {
                            e(format!("stall_recovery.max_temp: unknown zone '{k}'"));
                        }
                    }
                }
                _ => {}
            }
            if let Err(zs) = self.recovery_limits() {
                for zn in zs {
                    e(format!(
                        "stall_recovery.max_temp: zone {zn} needs a limit (a fixed policy has no point to derive it from)"
                    ));
                }
            }
        }

        if errs.is_empty() {
            Ok(())
        } else {
            Err(Error(format!("invalid configuration:\n  - {}", errs.join("\n  - "))))
        }
    }
}

/// Checks of a `type: i2c` output beyond the common ones.
fn validate_i2c_output(p: &str, o: &OutputConfig, errs: &mut Vec<String>) {
    let Some(reg) = &o.i2c else {
        errs.push(format!("{p}: type i2c needs 'i2c' (bus, address, register)"));
        return;
    };
    reg.check(&format!("{p}.i2c"), &[], None, errs);
    if o.hwmon.is_some() || o.path.is_some() || o.command.is_some() || o.pwm.is_some() {
        errs.push(format!(
            "{p}: 'hwmon'/'path'/'command'/'pwm' are not valid for type i2c"
        ));
    }
    if o.enable.is_some() || o.exit_enable.is_some() {
        errs.push(format!(
            "{p}: 'enable'/'exit_enable' need a sysfs output; i2c outputs use 'arm'"
        ));
    }
    let top = match &o.map {
        Some(m) => {
            if o.raw_min != 0 || o.raw_max != d_raw_max() {
                errs.push(format!(
                    "{p}: with 'map' the raw range comes from the map (map.min for the hardware minimum); \
                     raw_min/raw_max are not used"
                ));
            }
            match RawMap::new(m) {
                Ok(rm) => u64::from(rm.max.max(rm.off)),
                Err(msg) => {
                    errs.push(format!("{p}: {msg}"));
                    0
                }
            }
        }
        None => u64::from(o.raw_max),
    };
    if top > reg.width_max() {
        errs.push(format!("{p}: raw value {top} does not fit the {:?} register", reg.width).to_lowercase());
    }
    if let Some(a) = &o.arm {
        a.check(&format!("{p}.arm"), &["mask", "value", "write"], Some(reg), errs);
        if a.value.is_none() {
            errs.push(format!("{p}.arm: needs 'value' (the armed state)"));
        }
    }
    if let Some(x) = &o.present {
        let q = format!("{p}.present");
        x.check(&q, &["mask", "value"], Some(reg), errs);
        if x.value.is_none() {
            errs.push(format!("{q}: needs 'value' (the present state under 'mask')"));
        }
    }
    if let Some(l) = &o.led {
        let q = format!("{p}.led");
        let r = l.reg_config();
        r.check(&q, &["mask"], Some(reg), errs);
        let mask = r.mask_or_all();
        for (k, v) in [
            ("ok", Some(l.ok)),
            ("fault", Some(l.fault)),
            ("absent", l.absent),
            ("exit", l.exit),
            ("blink.off", l.blink.as_ref().map(|b| b.off)),
        ] {
            if let Some(v) = v {
                if v.0 > u64::from(mask) || (v.0 as u32) & !mask != 0 {
                    errs.push(format!("{q}: {k} 0x{:02x} has bits outside the mask 0x{mask:02x}", v.0));
                }
            }
        }
        if l.ok == l.fault {
            errs.push(format!("{q}: 'ok' and 'fault' are the same value"));
        }
        if let Some(b) = &l.blink {
            if b.off == l.fault {
                errs.push(format!("{q}: blink.off is the 'fault' value (it would not blink)"));
            }
            if !(0.2..=10.0).contains(&b.period.0) {
                errs.push(format!("{q}: blink.period {}s is outside 0.2..10s", b.period.0));
            }
        }
    }
    for (i, x) in o.expect.iter().enumerate() {
        let q = format!("{p}.expect[{i}]");
        x.check(&q, &["mask", "value", "min", "max"], Some(reg), errs);
        if x.value.is_none() && x.min.is_none() && x.max.is_none() {
            errs.push(format!("{q}: needs 'value', 'min' or 'max'"));
        }
        if x.value.is_some() && (x.min.is_some() || x.max.is_some()) {
            errs.push(format!("{q}: give 'value' or 'min'/'max', not both"));
        }
    }
}

/// Checks of `isolate_on_rotor_fault` and its keys.
fn validate_isolation(p: &str, o: &OutputConfig, in_recovery: bool, errs: &mut Vec<String>) {
    if !o.isolate_on_rotor_fault {
        if o.isolate_raw.is_some() {
            errs.push(format!("{p}: 'isolate_raw' needs 'isolate_on_rotor_fault: true'"));
        }
        if o.isolate_grace.is_some() && !in_recovery {
            errs.push(format!(
                "{p}: 'isolate_grace' needs 'isolate_on_rotor_fault: true' or the output in stall_recovery"
            ));
        }
        return;
    }
    if o.tach.is_none() {
        errs.push(format!(
            "{p}: isolate_on_rotor_fault needs a 'tach' (it acts on this output's rotor faults)"
        ));
    }
    if let Some(r) = o.isolate_raw {
        let top = match (o.kind, &o.i2c) {
            (OutputKind::I2c, Some(reg)) => reg.width_max(),
            _ => u64::from(o.raw_max),
        };
        if u64::from(r) > top {
            errs.push(format!("{p}: isolate_raw {r} is above {top}"));
        }
    }
    if o.isolate_grace() > MAX_ISOLATE_GRACE {
        errs.push(format!(
            "{p}: isolate_grace {}s is above {MAX_ISOLATE_GRACE}s",
            o.isolate_grace()
        ));
    }
}

/// Checks of a tach with `i2c` inputs.
fn validate_i2c_tach(p: &str, o: &OutputConfig, t: &TachConfig, errs: &mut Vec<String>) {
    if t.auto || !t.inputs.is_empty() || t.hwmon.is_some() {
        errs.push(format!(
            "{p}: tach takes 'i2c' inputs or hwmon ('auto'/'inputs'/'hwmon'), not both"
        ));
    }
    let mut labels = std::collections::BTreeSet::new();
    for (i, r) in t.i2c.iter().enumerate() {
        r.check(&format!("{p}.tach.i2c[{i}]"), &["label"], o.i2c.as_ref(), errs);
        let l = r.label.clone().unwrap_or_else(|| format!("fan{}", i + 1));
        if !labels.insert(l.clone()) {
            errs.push(format!("{p}: tach label {l:?} is used twice"));
        }
    }
    if t.scale.is_some_and(|s| !(s.is_finite() && s > 0.0)) {
        errs.push(format!("{p}: tach.scale must be > 0"));
    }
    if let Some(f) = t.follow_tolerance {
        if !(0.0..1.0).contains(&f) {
            errs.push(format!("{p}: tach.follow_tolerance must be within 0..1"));
        }
    }
    for (k, v) in t.max_rpm.iter().map(|v| ("max_rpm".to_string(), v)).chain(
        t.max_rpm_by_label
            .iter()
            .map(|(k, v)| (format!("max_rpm_by_label.{k}"), v)),
    ) {
        if !(v.is_finite() && *v > 0.0) {
            errs.push(format!("{p}: tach.{k} must be > 0"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
sensors:
  board: { type: hwmon, hwmon: lm75 }
outputs:
  fans: { type: hwmon, hwmon: { name: as7760_fan }, raw_min: 128 }
zones:
  chassis:
    outputs: [fans]
    sensors: [board]
    policy: delta-balanced
"#;

    #[test]
    fn minimal_config_gets_defaults() {
        let c = Config::parse(MINIMAL).unwrap();
        assert_eq!(c.interval, Secs(2.0));
        assert_eq!(c.profile, Profile::Medium);
        assert_eq!(c.failsafe_duty, Pct(100.0));
        assert_eq!(c.exit_duty, Pct(100.0));
        assert_eq!(c.calm_period, Secs(60.0));
        let o = &c.outputs["fans"];
        assert!((o.min_duty() - 50.196).abs() < 0.01);
        assert_eq!(o.max_duty(), 100.0);
        assert_eq!(o.readback, Readback::AtLeast);
        let z = &c.zones["chassis"];
        assert_eq!(z.policy.curve, Some(CurveRef::Named("delta-balanced".into())));
        assert_eq!(
            z.sensors[0],
            ZoneSensor {
                sensor: "board".into(),
                offset: 0.0,
                weight: 1.0,
                target: None
            }
        );
        assert_eq!(c.sensors["board"].scale(), 0.001);
        assert_eq!(
            c.sensors["board"].hwmon.as_ref().unwrap().0.name.as_deref(),
            Some("lm75")
        );
    }

    #[test]
    fn scalars_parse_in_all_spellings() {
        assert_eq!(parse_duration("500ms").unwrap(), 0.5);
        assert_eq!(parse_duration("2s").unwrap(), 2.0);
        assert_eq!(parse_duration("5m").unwrap(), 300.0);
        assert_eq!(parse_duration("1h").unwrap(), 3600.0);
        assert_eq!(parse_duration("1.5").unwrap(), 1.5);
        assert!(parse_duration("-1s").is_err());
        assert!(parse_duration("soon").is_err());
        assert_eq!(DutySpec::parse("80").unwrap(), DutySpec::Percent(80.0));
        assert_eq!(DutySpec::parse("80 %").unwrap(), DutySpec::Percent(80.0));
        assert_eq!(DutySpec::parse("9000rpm").unwrap(), DutySpec::Rpm(9000.0));
        assert_eq!(DutySpec::parse("9000 RPM").unwrap(), DutySpec::Rpm(9000.0));
        assert!(DutySpec::parse("120").is_err());
        assert!(DutySpec::parse("-5%").is_err());
        assert_eq!(Profile::parse("fast").unwrap(), Profile::Fast);
        assert_eq!(Profile::parse("off").unwrap(), Profile::None);
        assert_eq!(Profile::parse("55").unwrap(), Profile::Custom(DutySpec::Percent(55.0)));
        assert!(Profile::parse("turbo").is_err());
    }

    #[test]
    fn full_policy_forms_parse() {
        let y = r#"
interval: 1s
profile: 9000rpm
profiles: { slow: 25%, medium: 40, fast: 12000rpm }
curves:
  mine: { points: [[20, 30], [60, 100]], hysteresis: 1.5 }
sensors:
  a: { type: file, path: /x }
  b: { type: exec, command: [sh, -c, "echo 1"], parse: { json: "ports[].temp", aggregate: max } }
  c: { type: thermal_zone, zone: x86_pkg_temp, required: false }
outputs:
  o1: { type: exec, command: [tool, "{percent}"], raw_max: 100 }
  o2: { type: file, path: /pwm, stop: true }
zones:
  z1: { outputs: [o1], sensors: [a, { sensor: b, offset: -20 }], policy: { curve: mine } }
  z2: { outputs: [o2], sensors: [{ sensor: a, target: 40 }, c], policy: { target: 45, kp: 2 } }
  z3: { outputs: [o1, o2], policy: { fixed: 30% }, profile: none }
  z4: { outputs: [o1], sensors: [a], policy: { curve: { points: [[30, 40], [50, 100]] } } }
"#;
        let c = Config::parse(y).unwrap();
        assert_eq!(c.profile, Profile::Custom(DutySpec::Rpm(9000.0)));
        assert_eq!(c.profiles.fast, DutySpec::Rpm(12000.0));
        assert_eq!(c.zones["z2"].policy.target, Some(45.0));
        assert_eq!(c.zones["z2"].sensors[0].target, Some(40.0));
        assert_eq!(c.zones["z3"].policy.fixed, Some(DutySpec::Percent(30.0)));
        assert_eq!(c.zones["z3"].profile, Some(Profile::None));
        assert!(matches!(c.zones["z4"].policy.curve, Some(CurveRef::Inline { .. })));
        assert_eq!(c.sensors["b"].max_age(), Some(35.0));
        assert_eq!(c.outputs["o1"].reassert(), 60.0);
        assert!(c.curve(&CurveRef::Named("mine".into())).is_some());
        // Ramp/dwell defaults: on for curve and fixed zones, off for target
        // zones; explicit settings win either way.
        assert_eq!(c.zones["z1"].ramp(), (10.0, 1.0, 10.0));
        assert_eq!(c.zones["z3"].ramp(), (10.0, 1.0, 10.0));
        assert_eq!(c.zones["z2"].ramp(), (0.0, 0.0, 0.0));
        let explicit = y.replace("kp: 2 } }", "kp: 2 }, ramp_down: 0.5, dwell: 5s }");
        let c = Config::parse(&explicit).unwrap();
        assert_eq!(c.zones["z2"].ramp(), (0.0, 0.5, 5.0));
    }

    #[test]
    fn sensor_grace_defaults_and_limit() {
        let c = Config::parse(MINIMAL).unwrap();
        assert_eq!(c.sensor_grace(&c.sensors["board"]), 0.0);
        let g = MINIMAL.replacen("sensors:", "sensor_grace: 5s\nsensors:", 1);
        let c = Config::parse(&g).unwrap();
        assert_eq!(c.sensor_grace(&c.sensors["board"]), 5.0);
        let long = MINIMAL.replacen("sensors:", "sensor_grace: 2m\nsensors:", 1);
        let err = Config::parse(&long).unwrap_err().0;
        assert!(err.contains("sensor_grace: 120s is above 60s"), "{err}");
    }

    #[test]
    fn errors_are_collected_and_specific() {
        let y = r#"
sensors:
  a: { type: hwmon, path: /x }
  b: { type: exec, command: [] }
outputs:
  o: { type: hwmon, hwmon: x, raw_min: 300 }
  unused: { type: file, path: /y }
zones:
  z: { outputs: [o, nope], sensors: [a, ghost], policy: { curve: delta-nonexistent, fixed: 50 } }
"#;
        let err = Config::parse(y).unwrap_err().0;
        for needle in [
            "sensors.a: type hwmon needs 'hwmon'",
            "sensors.a: 'path' is not valid",
            "sensors.b: empty command",
            "outputs.o: need 0 <= raw_min <= raw_max",
            "outputs.unused: not used by any zone",
            "unknown output 'nope'",
            "unknown sensor 'ghost'",
            "exactly one of curve, target, fixed",
            "unknown curve",
        ] {
            assert!(err.contains(needle), "missing {needle:?} in:\n{err}");
        }
    }

    #[test]
    fn duties_above_max_duty_are_errors() {
        let capped = MINIMAL.replace("raw_min: 128 }", "raw_min: 128, max_duty: 80 }");
        let err = Config::parse(&capped).unwrap_err().0;
        for needle in [
            "failsafe_duty: 100% is above the max_duty 80% of output fans, which would cap it there \
             (lower failsafe_duty or raise outputs.fans.max_duty)",
            "exit_duty: 100% is above the max_duty 80% of output fans",
        ] {
            assert!(err.contains(needle), "missing {needle:?} in:\n{err}");
        }
        let ok = format!("failsafe_duty: 80\nexit_duty: 75\n{capped}");
        assert!(Config::parse(&ok).is_ok());
        let bad = format!(
            "failsafe_duty: 80\nexit_duty: 80\nstartup_duty: 90\n{}",
            capped.replace(
                "policy: delta-balanced",
                "policy: delta-balanced\n    failsafe_duty: 85"
            )
        );
        let err = Config::parse(&bad).unwrap_err().0;
        assert!(err.contains("startup_duty: 90% is above the max_duty 80%"), "{err}");
        assert!(
            err.contains("zones.chassis.failsafe_duty: 85% is above the max_duty 80%"),
            "{err}"
        );
        assert!(!err.contains("exit_duty"), "{err}");
    }

    #[test]
    fn unknown_keys_and_bad_values_are_rejected() {
        let typo = MINIMAL.replace("policy: delta-balanced", "policy: delta-balanced\n    ramp_upp: 5");
        assert!(Config::parse(&typo).is_err());
        let bad = MINIMAL.replace("raw_min: 128", "raw_min: 128, min_duty: 150");
        assert!(Config::parse(&bad).unwrap_err().0.contains("outside 0..100"));
        let bad_curve = MINIMAL.replace(
            "policy: delta-balanced",
            "policy: { curve: { points: [[50, 90], [60, 40]] } }",
        );
        assert!(Config::parse(&bad_curve).unwrap_err().0.contains("must not decrease"));
        assert!(Config::parse("sensors: [").is_err());
    }

    const I2C: &str = r#"
sensors:
  t: { type: i2c, i2c: { bus: 7, address: 0x48, register: 0, width: word, byte_order: big, signed: true }, scale: 0.00390625 }
outputs:
  fans:
    type: i2c
    i2c: { bus: 4, address: "0x68", register: "0x50" }
    map: { points: [[1, 4.6], [63, 100]], min: 9 }
    expect: [{ register: "0x01", min: 135 }]
    arm: { register: "0x2a", mask: "0x02", value: 0 }
    tach: { i2c: [{ register: 0x60 }, { register: 0x61, label: rear }], scale: 100, max_rpm: 14400 }
zones:
  z: { outputs: [fans], sensors: [t], policy: { target: 45 } }
"#;

    #[test]
    fn i2c_sensor_and_output_parse() {
        let c = Config::parse(I2C).unwrap();
        let s = &c.sensors["t"];
        assert_eq!(s.scale(), 0.00390625);
        let r = s.i2c.as_ref().unwrap().to_reg(None).unwrap();
        assert_eq!(
            (r.target.bus, r.target.address, r.reg, r.big_endian, r.signed),
            (7, 0x48, 0, true, true)
        );
        let o = &c.outputs["fans"];
        let base = o.i2c.as_ref().unwrap();
        assert_eq!(base.to_reg(None).unwrap().to_string(), "4-0068:0x50");
        let arm = o.arm.as_ref().unwrap();
        assert_eq!(
            arm.to_reg(Some(base)).unwrap().to_string(),
            "4-0068:0x2a",
            "bus/address inherited"
        );
        assert_eq!(arm.mask_or_all(), 2);
        assert!(
            (o.min_duty() - 16.909).abs() < 0.01,
            "map.min 9 is the hardware minimum"
        );
        let t = o.tach.as_ref().unwrap();
        assert_eq!(t.i2c[1].label.as_deref(), Some("rear"));
        assert_eq!(t.scale, Some(100.0));
    }

    #[test]
    fn i2c_bus_by_devicetree_node() {
        let y = I2C.replace(
            r#"i2c: { bus: 4, address: "0x68", register: "0x50" }"#,
            r#"i2c: { bus: { of_node: "i2c-mux@74/i2c@2" }, address: "0x66", register: "0x22" }"#,
        );
        let c = Config::parse(&y).unwrap();
        let o = &c.outputs["fans"];
        let base = o.i2c.as_ref().unwrap();
        let r = base.to_reg(None).unwrap();
        assert_eq!(
            (r.target.of_node, r.to_string().as_str()),
            (Some("i2c-mux@74/i2c@2"), "[i2c-mux@74/i2c@2]-0066:0x22")
        );
        let arm = o.arm.as_ref().unwrap().to_reg(Some(base)).unwrap();
        assert_eq!(arm.target.of_node, Some("i2c-mux@74/i2c@2"), "inherited like a number");
        // A string is still a number, and a number may override a selector.
        let s: I2cRegConfig = serde_saphyr::from_str(r#"{ bus: "0x1f", address: 1, register: 2 }"#).unwrap();
        assert_eq!(s.to_reg(Some(base)).unwrap().to_string(), "31-0001:0x02");
        for (bad, msg) in [
            (r#"{ of_node: "/" }"#, "bus.of_node is empty"),
            (r#"{ of_node: "x", name: y }"#, "unknown field"),
        ] {
            let y = I2C.replace(r#"bus: 4, address: "0x68""#, &format!(r#"bus: {bad}, address: "0x68""#));
            let e = Config::parse(&y).unwrap_err().0;
            assert!(e.contains(msg), "{bad}: {e}");
        }
    }

    #[test]
    fn raw_map_rounds_up_within_its_range() {
        let m = RawMap::new(&MapConfig {
            points: vec![(1.0, 4.6), (63.0, 100.0)],
            min: Some(Int(9)),
            off: None,
        })
        .unwrap();
        assert_eq!((m.min, m.max, m.off), (9, 63, 0));
        assert!((m.duty(9) - 16.909).abs() < 0.001);
        assert_eq!(m.duty(63), 100.0);
        assert_eq!(m.duty(0), 4.6, "flat below the first point");
        assert_eq!(m.raw_for(0.0), 9, "never below min");
        assert_eq!(m.raw_for(16.9), 9);
        assert_eq!(m.raw_for(16.95), 10, "rounds up: never less air");
        assert_eq!(m.raw_for(m.duty(42)), 42, "an exact step stays");
        assert_eq!(m.raw_for(100.0), 63);
        // A table with a flat part: the first raw value reaching the duty.
        let t = RawMap::new(&MapConfig {
            points: vec![(0.0, 0.0), (10.0, 50.0), (20.0, 50.0), (30.0, 100.0)],
            min: None,
            off: None,
        })
        .unwrap();
        assert_eq!(t.raw_for(50.0), 10);
        assert_eq!(t.raw_for(51.0), 21);
        for bad in [
            vec![(1.0, 4.6)],
            vec![(1.0, 4.6), (1.0, 100.0)],
            vec![(1.0, 50.0), (63.0, 40.0)],
            vec![(1.0, 4.6), (63.0, 90.0)],
            vec![(1.5, 4.6), (63.0, 100.0)],
        ] {
            let m = MapConfig {
                points: bad.clone(),
                min: None,
                off: None,
            };
            assert!(RawMap::new(&m).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn i2c_misuse_is_reported() {
        let y = r#"
sensors:
  a: { type: i2c }
  b: { type: hwmon, hwmon: lm75, i2c: { bus: 1, address: 0x48, register: 0 } }
  c: { type: i2c, i2c: { bus: 1, address: 0x90, register: 0x100, label: x } }
outputs:
  o1: { type: hwmon, hwmon: x, map: { points: [[0, 0], [255, 100]] } }
  o2:
    type: i2c
    i2c: { bus: 4, address: 0x68, register: 0x50 }
    raw_min: 9
    map: { points: [[1, 4.6], [300, 100]], min: 0 }
    arm: { register: 0x2a, label: x }
    expect: [{ register: 1 }, { register: 1, value: 3, min: 1 }]
    enable: 1
    tach: { auto: true, i2c: [{ register: 0x60 }, { register: 0x61, label: fan1 }] }
  o3: { type: i2c, raw_max: 1000, i2c: { register: 0x50 } }
zones:
  z: { outputs: [o1, o2, o3], policy: { fixed: 50 } }
"#;
        let err = Config::parse(y).unwrap_err().0;
        for needle in [
            "sensors.a: type i2c needs 'i2c'",
            "sensors.b: 'i2c' is not valid for type Hwmon",
            "sensors.c.i2c: address 0x90 is not a 7-bit address",
            "sensors.c.i2c: 'label' is not valid here",
            "outputs.o1: 'i2c'/'map'/'arm'/'expect' are only valid for type i2c",
            "outputs.o2: with 'map' the raw range comes from the map",
            "outputs.o2: map: min 0 is outside the map's raw range 1..300",
            "outputs.o2.arm: 'label' is not valid here",
            "outputs.o2.arm: needs 'value'",
            "outputs.o2.expect[0]: needs 'value', 'min' or 'max'",
            "outputs.o2.expect[1]: give 'value' or 'min'/'max', not both",
            "outputs.o2: 'enable'/'exit_enable' need a sysfs output; i2c outputs use 'arm'",
            "outputs.o2: tach takes 'i2c' inputs or hwmon",
            "outputs.o2: tach label \"fan1\" is used twice",
            "outputs.o3.i2c: needs 'bus'",
            "outputs.o3: raw value 1000 does not fit the byte register",
        ] {
            assert!(err.contains(needle), "missing {needle:?} in:\n{err}");
        }
    }
    #[test]
    fn presence_led_and_isolation_parse() {
        let y = I2C.replace(
            "    tach: {",
            "    present: { register: 0x36, mask: 0x01, value: 0 }\n    led: { register: 0x34, mask: 0x03, ok: 1, fault: 2 }\n    isolate_on_rotor_fault: true\n    tach: {",
        );
        let c = Config::parse(&y).unwrap();
        let o = &c.outputs["fans"];
        let p = o.present.as_ref().unwrap();
        assert_eq!(
            p.to_reg(o.i2c.as_ref()).unwrap().to_string(),
            "4-0068:0x36",
            "bus/address inherited"
        );
        let l = o.led.as_ref().unwrap();
        assert_eq!((l.ok.0, l.fault.0, l.absent, l.exit), (1, 2, None, None));
        assert_eq!(l.reg_config().mask_or_all(), 3);
        assert!(o.isolate_on_rotor_fault);
        assert_eq!((o.isolate_raw, o.isolate_grace()), (None, DEFAULT_ISOLATE_GRACE));
        // Without the keys nothing changes.
        let c = Config::parse(I2C).unwrap();
        let o = &c.outputs["fans"];
        assert!(o.present.is_none() && o.led.is_none() && !o.isolate_on_rotor_fault);
    }

    #[test]
    fn presence_led_and_isolation_misuse_is_reported() {
        let y = r#"
sensors:
  t: { type: file, path: /t }
outputs:
  a:
    type: i2c
    i2c: { bus: 4, address: 0x66, register: 0x22 }
    present: { register: 0x26, mask: 1, label: x }
    led: { register: 0x24, mask: 0x03, ok: 0x05, fault: 2, absent: 0x100, exit: 2 }
    isolate_raw: 300
  b:
    type: i2c
    i2c: { bus: 4, address: 0x66, register: 0x32 }
    led: { register: 0x34, ok: 1, fault: 1 }
    isolate_on_rotor_fault: true
    isolate_grace: 20m
  c: { type: file, path: /c, present: { register: 1, value: 0 }, isolation_max_duty: 10, raw_min: 128 }
zones:
  z: { outputs: [a, b, c], sensors: [t], policy: { fixed: 50 } }
"#;
        let err = Config::parse(y).unwrap_err().0;
        for needle in [
            "outputs.a.present: 'label' is not valid here",
            "outputs.a.present: needs 'value'",
            "outputs.a.led: ok 0x05 has bits outside the mask 0x03",
            "outputs.a.led: absent 0x100 has bits outside the mask 0x03",
            "outputs.a: 'isolate_raw' needs 'isolate_on_rotor_fault: true'",
            "outputs.b.led: 'ok' and 'fault' are the same value",
            "outputs.b: isolate_on_rotor_fault needs a 'tach'",
            "outputs.b: isolate_grace 1200s is above 600s",
            "outputs.c: 'present'/'led' are only valid for type i2c",
            "outputs.c: isolation_max_duty 10% is below the output's hardware minimum 50.2%",
        ] {
            assert!(err.contains(needle), "missing {needle:?} in:\n{err}");
        }
        assert!(!err.contains("isolation_max_duty has no effect"), "b isolates:\n{err}");
        let lone = I2C.replace("    tach: {", "    isolation_max_duty: 99\n    tach: {");
        let err = Config::parse(&lone).unwrap_err().0;
        assert!(err.contains("outputs.fans: isolation_max_duty has no effect"), "{err}");
        let big = I2C.replace(
            "    tach: {",
            "    isolate_on_rotor_fault: true\n    isolate_raw: 256\n    tach: {",
        );
        let err = Config::parse(&big).unwrap_err().0;
        assert!(err.contains("outputs.fans: isolate_raw 256 is above 255"), "{err}");
    }
    #[test]
    fn stall_recovery_and_degraded_parse_and_misuse() {
        let base = format!("rotor_fault_policy: degraded\nstall_recovery: {{ max_temp_margin: 3 }}\n{MINIMAL}");
        let c = Config::parse(&base);
        // MINIMAL's output has no tach with min_rpm: an error that names it.
        let err = c.unwrap_err().0;
        assert!(err.contains("stall_recovery: no outputs (none has a tach)"), "{err}");
        let y = format!("stall_recovery: {{}}\n{I2C}").replace("max_rpm: 14400 }", "max_rpm: 14400, min_rpm: 1000 }");
        let c = Config::parse(&y).unwrap();
        assert_eq!(c.recovery_limits().unwrap()["z"], 50.0, "derived: target 45 + margin 5");
        let y = format!("stall_recovery: {{ max_temp: {{ z: 50 }} }}\n{I2C}")
            .replace("max_rpm: 14400 }", "max_rpm: 14400, min_rpm: 1000 }");
        let c = Config::parse(&y).unwrap();
        let r = c.stall_recovery.as_ref().unwrap();
        assert_eq!(
            (r.hold.0, r.ramp[0].raw.0, r.ramp[0].duration.0, r.settle.0),
            (10.0, 0x30, 5.0, 15.0)
        );
        assert_eq!((r.max_attempts, r.interval.0), (3, 300.0));
        assert_eq!(r.given_up, GivenUp::Isolate, "the default is unchanged");
        assert_eq!(
            c.recovery_file_fallback(),
            None,
            "no explicit recovery_file: nothing to migrate"
        );
        let f = format!(
            "runtime: {{ recovery_file: /var/lib/fand/recovery.json }}
{}",
            y.replace("max_temp: { z: 50 } }", "max_temp: { z: 50 }, given_up: follow }")
        );
        let c2 = Config::parse(&f).unwrap();
        assert_eq!(c2.stall_recovery.as_ref().unwrap().given_up, GivenUp::Follow);
        assert_eq!(c2.recovery_file(), Some(PathBuf::from("/var/lib/fand/recovery.json")));
        assert_eq!(
            c2.recovery_file_fallback(),
            Some(PathBuf::from("/run/fand/recovery.json"))
        );
        assert!(Config::parse(&y.replace("max_temp: { z: 50 } }", "max_temp: { z: 50 }, given_up: drop }")).is_err());
        assert_eq!(c.recovery_outputs(), vec!["fans".to_string()]);
        assert_eq!(c.recovery_limits().unwrap()["z"], 50.0);
        assert_eq!(c.recovery_file(), Some(PathBuf::from("/run/fand/recovery.json")));
        // Derived limit: a target zone's target plus the margin.
        let d = y.replace("max_temp: { z: 50 }", "max_temp_margin: 2");
        assert_eq!(Config::parse(&d).unwrap().recovery_limits().unwrap()["z"], 47.0);
        for (from, to, needle) in [
            (
                "max_temp: { z: 50 }",
                "max_temp: 50, hold: 20s, ramp: [{ raw: 1, for: 11s }]",
                "hold plus ramp is 31s, above 30s",
            ),
            (
                "max_temp: { z: 50 }",
                "max_temp: 50, outputs: [nope]",
                "stall_recovery.outputs: unknown output 'nope'",
            ),
            (
                "max_temp: { z: 50 }",
                "max_temp: 50, outputs: [fans], release: [fans]",
                "output fans is in both 'outputs' and 'release'",
            ),
            (
                "max_temp: { z: 50 }",
                "max_temp: { y: 1 }",
                "stall_recovery.max_temp: unknown zone 'y'",
            ),
            (
                "max_temp: { z: 50 }",
                "max_temp: 50, hold_raw: 300",
                "hold_raw 300 does not fit output fans",
            ),
            (
                "max_temp: { z: 50 }",
                "max_temp: 50, max_attempts: 0",
                "max_attempts must be >= 1",
            ),
        ] {
            let e = Config::parse(&y.replace(from, to)).unwrap_err().0;
            assert!(e.contains(needle), "{to}: {e}");
        }
        let fixed = y
            .replace("policy: { target: 45 }", "policy: { fixed: 50 }")
            .replace("max_temp: { z: 50 }", "hold: 5s");
        let e = Config::parse(&fixed).unwrap_err().0;
        assert!(e.contains("zone z needs a limit"), "{e}");
        let deg = I2C.replace(
            "policy: { target: 45 } }",
            "policy: { target: 45 }, degraded_max_failed_rotors: 2 }",
        );
        let e = Config::parse(&deg).unwrap_err().0;
        assert!(
            e.contains("zones.z: degraded_* keys need rotor_fault_policy: degraded"),
            "{e}"
        );
        let ok = deg.replace(
            "degraded_max_failed_rotors: 2",
            "rotor_fault_policy: degraded, degraded_max_failed_rotors: 2",
        );
        assert!(Config::parse(&ok).is_ok());
        let blink = I2C.replace(
            "    tach: {",
            "    led: { register: 0x34, mask: 3, ok: 1, fault: 2, blink: { off: 2 } }\n    tach: {",
        );
        let e = Config::parse(&blink).unwrap_err().0;
        assert!(e.contains("blink.off is the 'fault' value"), "{e}");
        let grace = I2C.replace("    tach: {", "    isolate_grace: 20s\n    tach: {");
        assert!(Config::parse(&grace).unwrap_err().0.contains("'isolate_grace' needs"));
        let grace_ok = format!("stall_recovery: {{ max_temp: 50 }}\n{grace}")
            .replace("max_rpm: 14400 }", "max_rpm: 14400, min_rpm: 1000 }");
        assert!(
            Config::parse(&grace_ok).is_ok(),
            "isolate_grace is the recovery grace too"
        );
    }
}
