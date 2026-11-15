//! Per-rotor RPM models and the calibration file written by `fand calibrate`.

use crate::curves::interp;
use crate::error::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub outputs: BTreeMap<String, OutputCal>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputCal {
    /// Keyed by rotor label (fanN_label, or "fanN" without labels).
    pub rotors: BTreeMap<String, RotorCal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RotorCal {
    pub max_rpm: f64,
    /// (duty %, measured rpm), ascending duty.
    pub points: Vec<(f64, f64)>,
}

impl Calibration {
    /// Missing file = empty calibration.
    pub fn load(path: &Path) -> Result<Calibration> {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).ctx(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Calibration::default()),
            Err(e) => Err(e).ctx(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self).ctx(|| "serializing calibration")?;
        text.push('\n');
        crate::daemon::write_atomic(path, text.as_bytes())
    }
}

/// Expected RPM as a function of duty for one rotor.
#[derive(Debug, Clone, PartialEq)]
pub struct RotorModel {
    /// (duty %, rpm), ascending duty, ending at 100 %.
    points: Vec<(f64, f64)>,
    pub calibrated: bool,
}

impl RotorModel {
    /// Linear 0..max_rpm. PWM fans run above this line at mid duty, so it is
    /// a conservative lower expectation.
    pub fn linear(max_rpm: f64) -> RotorModel {
        RotorModel {
            points: vec![(0.0, 0.0), (100.0, max_rpm)],
            calibrated: false,
        }
    }

    pub fn from_cal(c: &RotorCal) -> Option<RotorModel> {
        let mut pts: Vec<(f64, f64)> = c
            .points
            .iter()
            .copied()
            .filter(|(d, r)| d.is_finite() && r.is_finite() && (0.0..=100.0).contains(d) && *r >= 0.0)
            .collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts.dedup_by(|a, b| a.0 == b.0);
        if !(c.max_rpm.is_finite() && c.max_rpm > 0.0) {
            return None;
        }
        if pts.last().is_none_or(|p| p.0 < 100.0) {
            pts.push((100.0, c.max_rpm));
        }
        if pts.len() < 2 {
            pts.insert(0, (0.0, 0.0));
        }
        Some(RotorModel {
            points: pts,
            calibrated: true,
        })
    }

    pub fn max_rpm(&self) -> f64 {
        self.points.last().map_or(0.0, |p| p.1)
    }

    pub fn expected(&self, duty: f64) -> f64 {
        interp(&self.points, duty)
    }

    /// Smallest duty expected to reach `rpm` (100 if unreachable).
    pub fn duty_for(&self, rpm: f64) -> f64 {
        if rpm <= self.points[0].1 {
            return self.points[0].0;
        }
        for w in self.points.windows(2) {
            let ((d0, r0), (d1, r1)) = (w[0], w[1]);
            if rpm <= r1 && r1 > r0 {
                return d0 + (d1 - d0) * (rpm - r0) / (r1 - r0);
            }
        }
        100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_model_round_trips() {
        let m = RotorModel::linear(20000.0);
        assert_eq!(m.expected(50.0), 10000.0);
        assert_eq!(m.duty_for(10000.0), 50.0);
        assert_eq!(m.duty_for(25000.0), 100.0);
        assert_eq!(m.max_rpm(), 20000.0);
    }

    #[test]
    fn calibrated_model_interpolates_measured_points() {
        let c = RotorCal {
            max_rpm: 21000.0,
            points: vec![(100.0, 21000.0), (50.0, 12600.0), (70.0, 16400.0), (f64::NAN, 1.0)],
        };
        let m = RotorModel::from_cal(&c).unwrap();
        assert!(m.calibrated);
        assert_eq!(m.expected(50.0), 12600.0);
        assert_eq!(m.expected(60.0), 14500.0);
        assert_eq!(m.expected(30.0), 12600.0); // flat below the lowest measured duty
        assert!((m.duty_for(14500.0) - 60.0).abs() < 1e-9);
        assert_eq!(m.duty_for(1000.0), 50.0);
        assert!(
            RotorModel::from_cal(&RotorCal {
                max_rpm: 0.0,
                points: vec![]
            })
            .is_none()
        );
    }

    #[test]
    fn calibration_file_round_trip() {
        let fs = crate::fakesys::FakeSys::new("cal");
        let p = fs.root.join("var/lib/fand/calibration.json");
        assert_eq!(Calibration::load(&p).unwrap(), Calibration::default());
        let mut c = Calibration {
            version: 1,
            ..Default::default()
        };
        c.outputs.entry("fans".into()).or_default().rotors.insert(
            "fan1_front".into(),
            RotorCal {
                max_rpm: 21000.0,
                points: vec![(50.0, 12000.0), (100.0, 21000.0)],
            },
        );
        c.save(&p).unwrap();
        assert_eq!(Calibration::load(&p).unwrap(), c);
    }
}
