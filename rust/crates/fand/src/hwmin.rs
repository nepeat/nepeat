//! Hardware minimums: settings that ask an output for less than it can do.
//!
//! An output's *hardware minimum* is [`OutputConfig::min_duty`]: `raw_min`
//! as a percentage of `raw_max`, or `min_duty` when that is higher. Zone
//! targets never go below the highest minimum of their outputs, and every
//! command is clamped to it (0 stays 0 on a `stop: true` output). This module
//! only makes explicit settings below it visible ([`static_warnings`], logged
//! at start and printed by `fand check`) and holds the predicate [`below`].

use crate::config::{Config, CurveRef, DutySpec, OutputConfig, OutputKind, Profile, RawMap};
use crate::curves::Curve;
use std::fmt;

/// A setting below an output's hardware minimum.
#[derive(Debug, Clone, PartialEq)]
pub struct MinWarning {
    /// Config path of the setting (`profile`, `zones.board.policy`, ...).
    pub path: String,
    pub msg: String,
    /// false: by design (a built-in curve's low end), informational only.
    pub warn: bool,
}

impl fmt::Display for MinWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.msg)
    }
}

/// "warning: <path>: <msg>" or "info: ..." as `fand check` prints it.
pub fn line(w: &MinWarning) -> String {
    format!("{}: {w}", if w.warn { "warning" } else { "info" })
}

/// A percentage with at most one decimal: 45 -> "45%", 50.196 -> "50.2%".
pub fn pct(x: f64) -> String {
    format!("{}%", num(x))
}

/// A number with at most one decimal.
fn num(x: f64) -> String {
    let r = (x * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}

/// Would `duty` be raised to the hardware minimum `min`? 0 on a stop-capable
/// output is legal. A request that rounds up to the same raw value as the
/// minimum (50 % with raw_min 128 of 255) is not raised in practice and does
/// not count.
pub fn below(duty: f64, min: f64, stop: bool, raw_max: u32) -> bool {
    if (stop && duty <= 0.0) || duty >= min {
        return false;
    }
    let up = |d: f64| (d * f64::from(raw_max) / 100.0 - 1e-9).ceil();
    up(duty) < up(min)
}

/// [`below`] for an i2c output with a `map`: compared in map raw values.
pub fn below_map(duty: f64, min: f64, stop: bool, map: &RawMap) -> bool {
    if (stop && duty <= 0.0) || duty >= min {
        return false;
    }
    let free = RawMap { min: 0, ..map.clone() };
    free.raw_for(duty) < free.raw_for(min)
}

fn below_cfg(duty: f64, o: &OutputConfig) -> bool {
    if let Some(m) = o.raw_map().filter(|_| o.kind == OutputKind::I2c) {
        return below_map(duty, o.min_duty(), o.stop, &m);
    }
    below(duty, o.min_duty(), o.stop, o.raw_max)
}

/// An output's hardware minimum and where it comes from, for messages:
/// "50.2%, raw_min 128 of 255" or "20%, min_duty".
pub fn describe(o: &OutputConfig) -> String {
    if let Some(m) = o.raw_map().filter(|_| o.kind == OutputKind::I2c) {
        let src = match o.min_duty {
            Some(p) if p.0 >= m.duty(m.min) && p.0 > 0.0 => "min_duty".to_string(),
            _ => format!("map.min {} of {}..{}", m.min, m.points[0].0, m.max),
        };
        return format!("{}, {src}", pct(o.min_duty()));
    }
    let from_raw = if o.raw_max > 0 {
        f64::from(o.raw_min) * 100.0 / f64::from(o.raw_max)
    } else {
        0.0
    };
    let src = match o.min_duty {
        Some(p) if p.0 >= from_raw && p.0 > 0.0 => "min_duty".to_string(),
        _ if o.raw_min > 0 => format!("raw_min {} of {}", o.raw_min, o.raw_max),
        _ => "none set".to_string(),
    };
    format!("{}, {src}", pct(o.min_duty()))
}

/// "<what> is below the hardware minimum of output O (50.2%, raw_min 128 of
/// 255); it runs at 50.2% instead".
fn below_msg(what: &str, output: &str, o: &OutputConfig) -> String {
    format!(
        "{what} is below the hardware minimum of output {output} ({}); it runs at {} instead",
        describe(o),
        pct(o.min_duty())
    )
}

/// Messages grouped by (path, output), so a global setting shared by several
/// zones on one output is one line naming the zones.
#[derive(Default)]
struct Collect {
    items: Vec<(MinWarning, String, Vec<String>)>,
}

impl Collect {
    fn add(&mut self, warn: bool, path: &str, output: &str, msg: String, zone: Option<&str>) {
        let i = match self.items.iter().position(|(w, o, _)| w.path == path && o == output) {
            Some(i) => i,
            None => {
                let w = MinWarning {
                    path: path.to_string(),
                    msg,
                    warn,
                };
                self.items.push((w, output.to_string(), Vec::new()));
                self.items.len() - 1
            }
        };
        if let Some(z) = zone {
            let zones = &mut self.items[i].2;
            if !zones.iter().any(|x| x == z) {
                zones.push(z.to_string());
            }
        }
    }

    fn finish(self) -> Vec<MinWarning> {
        self.items
            .into_iter()
            .map(|(mut w, _, zones)| {
                match zones.len() {
                    0 => {}
                    1 => w.msg = format!("{} (zone {})", w.msg, zones[0]),
                    _ => w.msg = format!("{} (zones {})", w.msg, zones.join(", ")),
                }
                w
            })
            .collect()
    }
}

/// The lowest temperature at which a non-decreasing curve reaches `min`.
fn crossing(points: &[(f64, f64)], min: f64) -> Option<f64> {
    let first = points.first()?;
    if first.1 >= min {
        return Some(first.0);
    }
    points.windows(2).find_map(|w| {
        let ((t0, d0), (t1, d1)) = (w[0], w[1]);
        (d0 < min && d1 >= min).then(|| t0 + (min - d0) * (t1 - t0) / (d1 - d0))
    })
}

/// For a curve with points below the output's minimum: the WARN text for a
/// curve the user wrote ("curve (inline) asks <50.2% below 42.6 C ...") or
/// the INFO text for a built-in one ("curve delta-balanced is flat at the
/// hardware minimum ... below 42.6 C"). None when no point is below.
fn curve_msg(name: &str, curve: &Curve, builtin: bool, output: &str, o: &OutputConfig) -> Option<String> {
    let min = o.min_duty();
    let &(lt, ld) = curve.points.iter().find(|(_, d)| below_cfg(*d, o))?;
    let cross = crossing(&curve.points, min);
    let range = match cross {
        Some(t) => format!("below {} C", num(t)),
        None => "over its whole range".to_string(),
    };
    Some(if builtin {
        format!(
            "curve {name} is flat at the hardware minimum of output {output} ({}) {range}",
            describe(o)
        )
    } else {
        format!(
            "curve {name} asks <{} {range} (lowest {} at {} C), under the hardware minimum of output {output} ({}); \
             it runs at {} there",
            pct(min),
            pct(ld),
            num(lt),
            describe(o),
            pct(min)
        )
    })
}

/// Explicit settings that ask an output for less than its hardware minimum:
/// profile floors (named or numeric percentages), fixed percentages, the
/// points of inline and user-defined curves, failsafe_duty, startup_duty and
/// exit_duty (all WARN), plus the low end of built-in curves (INFO, by
/// design). Pure: reads nothing but the config. rpm values are not checked
/// (they need runtime models and are raised like everything else).
pub fn static_warnings(cfg: &Config) -> Vec<MinWarning> {
    let mut c = Collect::default();
    for (zn, z) in &cfg.zones {
        let outs: Vec<(&str, &OutputConfig)> = z
            .outputs
            .iter()
            .filter_map(|n| cfg.outputs.get(n).map(|o| (n.as_str(), o)))
            .collect();

        // Profile floor (0 = no floor).
        let (ppath, prof) = match z.profile {
            Some(p) => (format!("zones.{zn}.profile"), p),
            None => ("profile".to_string(), cfg.profile),
        };
        let floor = match prof.floor(&cfg.profiles) {
            DutySpec::Percent(p) => Some(p),
            DutySpec::Rpm(_) => None,
        };
        if let Some(f) = floor.filter(|f| *f > 0.0) {
            let what = match prof {
                Profile::Custom(_) => format!("profile {}", pct(f)),
                named => format!("profile {named} ({})", pct(f)),
            };
            for (on, o) in &outs {
                if below_cfg(f, o) {
                    c.add(true, &ppath, on, below_msg(&what, on, o), Some(zn));
                }
            }
        }
        // A floor at or above the minimum hides low policy values.
        let floor_covers = |o: &OutputConfig| floor.is_some_and(|f| f > 0.0 && !below_cfg(f, o));

        let ppol = format!("zones.{zn}.policy");
        if let Some(cr) = &z.policy.curve {
            if let Some(curve) = cfg.curve(cr) {
                let (name, builtin) = match cr {
                    CurveRef::Named(n) => (n.clone(), !cfg.curves.contains_key(n)),
                    CurveRef::Inline { .. } => ("(inline)".to_string(), false),
                };
                for (on, o) in &outs {
                    if floor_covers(o) {
                        continue;
                    }
                    if let Some(m) = curve_msg(&name, &curve, builtin, on, o) {
                        c.add(!builtin, &ppol, on, m, None);
                    }
                }
            }
        }
        if let Some(DutySpec::Percent(p)) = z.policy.fixed {
            for (on, o) in &outs {
                if !floor_covers(o) && below_cfg(p, o) {
                    c.add(true, &ppol, on, below_msg(&format!("fixed {}", pct(p)), on, o), None);
                }
            }
        }

        let (fpath, fs) = match z.failsafe_duty {
            Some(p) => (format!("zones.{zn}.failsafe_duty"), p.0),
            None => ("failsafe_duty".to_string(), cfg.failsafe_duty.0),
        };
        for (on, o) in &outs {
            if below_cfg(fs, o) {
                c.add(
                    true,
                    &fpath,
                    on,
                    below_msg(&format!("failsafe duty {}", pct(fs)), on, o),
                    Some(zn),
                );
            }
        }
        // Only when set explicitly; the default is failsafe_duty (above).
        if let Some(s) = cfg.startup_duty {
            for (on, o) in &outs {
                if below_cfg(s.0, o) {
                    let m = below_msg(&format!("startup duty {}", pct(s.0)), on, o);
                    c.add(true, "startup_duty", on, m, Some(zn));
                }
            }
        }
    }
    for (on, o) in &cfg.outputs {
        if below_cfg(cfg.exit_duty.0, o) {
            let m = below_msg(&format!("exit duty {}", pct(cfg.exit_duty.0)), on, o);
            c.add(true, "exit_duty", on, m, None);
        }
    }
    c.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(extra_top: &str, output: &str, zone: &str) -> Config {
        let y = format!(
            "{extra_top}\nsensors:\n  t: {{ type: file, path: /t }}\noutputs:\n  fans: {output}\nzones:\n  z: {zone}\n"
        );
        Config::parse(&y).unwrap_or_else(|e| panic!("{e}\n{y}"))
    }

    const FANS: &str = "{ type: hwmon, hwmon: as7760_fan, raw_min: 128 }";

    /// "warning: ..." / "info: ..." lines.
    fn msgs(c: &Config) -> Vec<String> {
        static_warnings(c).iter().map(line).collect()
    }

    #[test]
    fn predicate_respects_stop_and_raw_rounding() {
        let min = 128.0 * 100.0 / 255.0;
        assert!(below(45.0, min, false, 255));
        assert!(!below(50.0, min, false, 255), "50% rounds up to raw 128 anyway");
        assert!(!below(min, min, false, 255));
        assert!(!below(60.0, min, false, 255));
        assert!(below(0.0, 20.0, false, 255));
        assert!(!below(0.0, 20.0, true, 255), "0 is legal with stop: true");
        assert!(below(10.0, 20.0, true, 255), "but 10% is still raised");
        assert!(!below(10.0, 0.0, false, 255));
        assert_eq!(pct(min), "50.2%");
        assert_eq!(pct(45.0), "45%");
    }

    #[test]
    fn profile_floor_below_the_minimum() {
        let c = cfg("", FANS, "{ outputs: [fans], policy: { fixed: 60 } }");
        assert_eq!(
            msgs(&c),
            [
                "warning: profile: profile medium (45%) is below the hardware minimum of output fans \
              (50.2%, raw_min 128 of 255); it runs at 50.2% instead (zone z)"
            ]
        );
        // A zone's own profile has its own path; none and fast are fine.
        let c = cfg("", FANS, "{ outputs: [fans], policy: { fixed: 60 }, profile: 40% }");
        assert_eq!(msgs(&c).len(), 1);
        assert!(
            msgs(&c)[0].starts_with("warning: zones.z.profile: profile 40% is below"),
            "{:?}",
            msgs(&c)
        );
        let c = cfg("profile: none", FANS, "{ outputs: [fans], policy: { fixed: 60 } }");
        assert!(msgs(&c).is_empty(), "{:?}", msgs(&c));
        let c = cfg("profile: fast", FANS, "{ outputs: [fans], policy: { fixed: 60 } }");
        assert!(msgs(&c).is_empty());
        // The table behind the name counts; rpm values are not checked.
        let zone = "{ outputs: [fans], policy: { fixed: 60 } }";
        assert!(msgs(&cfg("profile: slow\nprofiles: { slow: 55 }", FANS, zone)).is_empty());
        assert!(msgs(&cfg("profile: 3000rpm", FANS, zone)).is_empty());
    }

    #[test]
    fn global_settings_shared_by_zones_are_one_line() {
        let y = format!(
            "sensors:\n  t: {{ type: file, path: /t }}\noutputs:\n  fans: {FANS}\nzones:\n  \
             a: {{ outputs: [fans], policy: {{ fixed: 60 }} }}\n  b: {{ outputs: [fans], policy: {{ fixed: 70 }} }}\n"
        );
        let w = msgs(&Config::parse(&y).unwrap());
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].ends_with("(zones a, b)"), "{w:?}");
    }

    #[test]
    fn curves_inline_points_warn_builtins_are_info() {
        // A built-in curve's low end under the minimum is by design: info only.
        let c = cfg(
            "profile: none",
            FANS,
            "{ outputs: [fans], sensors: [t], policy: delta-balanced }",
        );
        assert_eq!(
            msgs(&c),
            [
                "info: zones.z.policy: curve delta-balanced is flat at the hardware minimum of output fans \
              (50.2%, raw_min 128 of 255) below 42.6 C"
            ]
        );
        // Points the user wrote (inline or under curves:) are warnings.
        let c = cfg(
            "profile: none",
            FANS,
            "{ outputs: [fans], sensors: [t], policy: { curve: { points: [[30, 35], [50, 65]] } } }",
        );
        assert_eq!(
            msgs(&c),
            [
                "warning: zones.z.policy: curve (inline) asks <50.2% below 40.1 C (lowest 35% at 30 C), \
              under the hardware minimum of output fans (50.2%, raw_min 128 of 255); it runs at 50.2% there"
            ]
        );
        let c = cfg(
            "profile: none\ncurves: { mine: { points: [[30, 20], [60, 40]] } }",
            FANS,
            "{ outputs: [fans], sensors: [t], policy: mine }",
        );
        let w = msgs(&c);
        assert!(
            w[0].starts_with(
                "warning: zones.z.policy: curve mine asks <50.2% over its whole range (lowest 20% at 30 C)"
            ),
            "{w:?}"
        );
        // A floor at or above the minimum hides the low points.
        let c = cfg(
            "profile: fast",
            FANS,
            "{ outputs: [fans], sensors: [t], policy: delta-balanced }",
        );
        assert!(msgs(&c).is_empty());
        // stop: true: 0 points are legal, points between 0 and the minimum are not.
        let stop = "{ type: file, path: /p, stop: true, min_duty: 20 }";
        let ok = "{ outputs: [fans], sensors: [t], policy: { curve: { points: [[40, 0], [41, 30], [60, 100]] } } }";
        assert!(msgs(&cfg("profile: none", stop, ok)).is_empty());
        let low = "{ outputs: [fans], sensors: [t], policy: { curve: { points: [[40, 0], [41, 10], [60, 100]] } } }";
        let w = msgs(&cfg("profile: none", stop, low));
        assert!(w[0].contains("asks <20% below 43.1 C (lowest 10% at 41 C)"), "{w:?}");
        assert!(w[0].contains("(20%, min_duty)"), "{w:?}");
    }

    #[test]
    fn fixed_policies() {
        let c = cfg("profile: none", FANS, "{ outputs: [fans], policy: { fixed: 30 } }");
        assert_eq!(
            msgs(&c),
            [
                "warning: zones.z.policy: fixed 30% is below the hardware minimum of output fans \
              (50.2%, raw_min 128 of 255); it runs at 50.2% instead"
            ]
        );
        assert!(
            msgs(&cfg(
                "profile: none",
                FANS,
                "{ outputs: [fans], policy: { fixed: 8000rpm } }"
            ))
            .is_empty()
        );
    }

    #[test]
    fn failsafe_startup_and_exit_duties() {
        let zone = "{ outputs: [fans], policy: { fixed: 60 } }";
        let c = cfg(
            "profile: none\nfailsafe_duty: 40\nstartup_duty: 30\nexit_duty: 20",
            FANS,
            zone,
        );
        let w = msgs(&c);
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(w.iter().any(|m| m.starts_with("warning: failsafe_duty: failsafe duty 40% is below")
            && m.ends_with("(zone z)")));
        assert!(
            w.iter()
                .any(|m| m.starts_with("warning: startup_duty: startup duty 30% is below"))
        );
        assert!(
            w.iter()
                .any(|m| m.starts_with("warning: exit_duty: exit duty 20% is below")
                    && m.ends_with("it runs at 50.2% instead"))
        );
        let c = cfg(
            "profile: none",
            FANS,
            "{ outputs: [fans], policy: { fixed: 60 }, failsafe_duty: 45 }",
        );
        assert!(msgs(&c)[0].starts_with("warning: zones.z.failsafe_duty: failsafe duty 45% is below"));
        // Defaults (100 %) and 0 on a stop-capable output are fine.
        let stop = "{ type: file, path: /p, stop: true, min_duty: 20 }";
        assert!(msgs(&cfg("profile: none\nexit_duty: 0", stop, zone)).is_empty());
        assert_eq!(msgs(&cfg("profile: none\nexit_duty: 10", stop, zone)).len(), 1);
    }
}
