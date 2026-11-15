//! Piecewise-linear temperature -> duty curves, including the built-in set.

/// A monotone curve: points are (temperature C, duty %), temperatures strictly
/// increasing and duties non-decreasing (hotter never means slower).
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub points: Vec<(f64, f64)>,
    /// Degrees C the temperature must fall below a level before the duty is
    /// allowed to follow it down (see [`crate::control::Hysteresis`]).
    pub hysteresis: f64,
}

/// A built-in curve definition.
pub struct Builtin {
    pub name: &'static str,
    pub about: &'static str,
    pub points: &'static [(f64, f64)],
    pub hysteresis: f64,
}

/// Built-in curves, tuned for high-flow, high-static-pressure server/switch
/// fans (Delta-style). Such fans move a lot of air even at 40-50 %, are loud
/// above ~70 % and often have a high minimum stable duty, so the curves start
/// higher than desktop curves and reach 100 % well before components are at
/// their limits. They are meant to be fed a board/inlet-class temperature,
/// except `delta-asic`, which expects a switch-chip die temperature.
pub const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "delta-quiet",
        about: "board/inlet sensors; lowest noise, relies on good airflow",
        points: &[(35.0, 30.0), (45.0, 40.0), (55.0, 60.0), (65.0, 85.0), (70.0, 100.0)],
        hysteresis: 3.0,
    },
    Builtin {
        name: "delta-balanced",
        about: "board/inlet sensors; default trade-off",
        points: &[(30.0, 35.0), (40.0, 45.0), (50.0, 65.0), (58.0, 85.0), (65.0, 100.0)],
        hysteresis: 2.0,
    },
    Builtin {
        name: "delta-cool",
        about: "board/inlet sensors; keeps components cold, loud",
        points: &[(25.0, 50.0), (35.0, 60.0), (45.0, 80.0), (52.0, 100.0)],
        hysteresis: 2.0,
    },
    Builtin {
        name: "delta-asic",
        about: "switch ASIC / CPU die temperature",
        points: &[(60.0, 35.0), (75.0, 55.0), (85.0, 80.0), (95.0, 100.0)],
        hysteresis: 3.0,
    },
];

pub fn builtin(name: &str) -> Option<Curve> {
    BUILTINS.iter().find(|b| b.name == name).map(|b| Curve {
        points: b.points.to_vec(),
        hysteresis: b.hysteresis,
    })
}

impl Curve {
    pub fn validate(&self) -> Result<(), String> {
        if self.points.len() < 2 {
            return Err("a curve needs at least two points".into());
        }
        if !(self.hysteresis.is_finite() && self.hysteresis >= 0.0) {
            return Err("hysteresis must be >= 0".into());
        }
        for (t, d) in &self.points {
            if !t.is_finite() || !(0.0..=100.0).contains(d) {
                return Err(format!("point ({t}, {d}): duty must be within 0..100"));
            }
        }
        for w in self.points.windows(2) {
            if w[1].0 <= w[0].0 {
                return Err(format!("temperatures must increase ({} then {})", w[0].0, w[1].0));
            }
            if w[1].1 < w[0].1 {
                return Err(format!(
                    "duties must not decrease with temperature ({}% at {} C, then {}% at {} C)",
                    w[0].1, w[0].0, w[1].1, w[1].0
                ));
            }
        }
        Ok(())
    }

    /// Duty for a temperature; flat beyond the first and last points.
    pub fn eval(&self, t: f64) -> f64 {
        interp(&self.points, t)
    }
}

/// Linear interpolation over points sorted by x, clamped at both ends.
/// Empty input gives 100 (the safe answer for a duty).
pub fn interp(points: &[(f64, f64)], x: f64) -> f64 {
    let Some(first) = points.first() else {
        return 100.0;
    };
    if x.is_nan() {
        return points.last().map_or(100.0, |p| p.1);
    }
    if x <= first.0 {
        return first.1;
    }
    for w in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            if x1 == x0 {
                return y1;
            }
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    points.last().map_or(100.0, |p| p.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_and_end_at_full_speed() {
        for b in BUILTINS {
            let c = builtin(b.name).unwrap();
            c.validate().unwrap_or_else(|e| panic!("{}: {e}", b.name));
            assert_eq!(c.points.last().unwrap().1, 100.0, "{}", b.name);
        }
    }

    #[test]
    fn interpolates_and_clamps() {
        let c = Curve {
            points: vec![(30.0, 40.0), (50.0, 80.0), (60.0, 100.0)],
            hysteresis: 0.0,
        };
        assert_eq!(c.eval(0.0), 40.0);
        assert_eq!(c.eval(30.0), 40.0);
        assert_eq!(c.eval(40.0), 60.0);
        assert_eq!(c.eval(55.0), 90.0);
        assert_eq!(c.eval(99.0), 100.0);
        assert_eq!(c.eval(f64::NAN), 100.0);
    }

    #[test]
    fn rejects_bad_curves() {
        let mk = |p: Vec<(f64, f64)>| {
            Curve {
                points: p,
                hysteresis: 1.0,
            }
            .validate()
        };
        assert!(mk(vec![(30.0, 40.0)]).is_err());
        assert!(mk(vec![(30.0, 40.0), (30.0, 50.0)]).is_err());
        assert!(mk(vec![(30.0, 60.0), (40.0, 50.0)]).is_err());
        assert!(mk(vec![(30.0, 60.0), (40.0, 150.0)]).is_err());
        assert!(mk(vec![(30.0, 60.0), (40.0, 70.0)]).is_ok());
    }
}
