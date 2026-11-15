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
}

impl SensorConfig {
    pub fn scale(&self) -> f64 {
        self.scale.unwrap_or(match self.kind {
            SensorKind::Hwmon | SensorKind::ThermalZone => 0.001,
            SensorKind::File | SensorKind::Exec => 1.0,
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
}

fn d_raw_max() -> u32 {
    255
}

impl OutputConfig {
    /// The output's hardware minimum: raw_min as a percentage, or min_duty
    /// if higher. Every command is clamped to it (see `crate::hwmin`).
    pub fn min_duty(&self) -> f64 {
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
    pub fn reassert(&self) -> f64 {
        self.reassert
            .map_or(if self.kind == OutputKind::Exec { 60.0 } else { 0.0 }, |s| s.0)
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
            if let Some(t) = &o.tach {
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

        if errs.is_empty() {
            Ok(())
        } else {
            Err(Error(format!("invalid configuration:\n  - {}", errs.join("\n  - "))))
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
}
