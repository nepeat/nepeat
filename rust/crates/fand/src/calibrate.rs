//! `fand calibrate`: step each output's duty, record rotor RPM, write the
//! calibration file. Other outputs are held at 100 % meanwhile, sensors are
//! watched throughout, and every exit path restores the exit duty.

use crate::calibration::{Calibration, RotorCal};
use crate::config::Config;
use crate::daemon::ExitGuard;
use crate::engine::{Engine, ExecMode, Options};
use crate::error::{Error, Result};
use crate::{log_error, log_info, log_notice};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct CalOptions {
    /// Outputs to calibrate (empty: every output with a tach).
    pub outputs: Vec<String>,
    /// Duties to visit, descending (default 100, 90, ... down to min_duty).
    pub steps: Option<Vec<f64>>,
    /// Seconds to wait after each change before sampling.
    pub settle: f64,
    /// Samples (1 s apart) averaged per step.
    pub samples: usize,
    /// Abort if any sensor reaches this temperature.
    pub max_temp: f64,
}

pub fn default_steps(min_duty: f64) -> Vec<f64> {
    let mut steps: Vec<f64> = (0..=10)
        .rev()
        .map(|i| f64::from(i) * 10.0)
        .filter(|d| *d >= min_duty && *d > 0.0)
        .collect();
    let min = min_duty.max(1.0).ceil();
    if steps.last().is_none_or(|l| *l > min) {
        steps.push(min);
    }
    steps
}

fn safety(engine: &mut Engine, max_temp: f64) -> Result<()> {
    let now = engine.now();
    engine.read_sensors(now);
    for s in &engine.sensors {
        match (s.value, &s.error) {
            (Some(v), _) if v >= max_temp || s.critical.is_some_and(|c| v >= c) => {
                return Err(Error(format!("sensor {} at {v:.1} C: aborting calibration", s.name)));
            }
            (None, Some(e)) if s.required => {
                return Err(Error(format!("sensor {}: {e}: aborting calibration", s.name)));
            }
            _ => {}
        }
    }
    Ok(())
}

fn wait(engine: &mut Engine, secs: f64, term: &AtomicBool, max_temp: f64) -> Result<()> {
    let end = engine.now() + secs;
    loop {
        if term.load(Ordering::Relaxed) {
            return Err(Error("interrupted".into()));
        }
        safety(engine, max_temp)?;
        let left = end - engine.now();
        if left <= 0.0 {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs_f64(left.min(1.0)));
    }
}

pub fn calibrate(cfg: Config, opts: CalOptions, term: Arc<AtomicBool>) -> Result<Calibration> {
    let mut eopts = Options::new(&cfg);
    eopts.exec_mode = ExecMode::Inline;
    if eopts.dry_run {
        return Err(Error(
            "calibration writes duties; it cannot run with dry_run set".into(),
        ));
    }
    let exit = cfg.exit_duty.0;
    let mut cal = crate::daemon::load_calibration(&cfg);
    cal.version = 1;
    let mut guard = ExitGuard::new(Engine::new(cfg, eopts)?, exit);
    let engine = &mut guard.engine;
    let root = engine.root().to_path_buf();

    let targets: Vec<usize> = if opts.outputs.is_empty() {
        (0..engine.outputs.len())
            .filter(|i| engine.outputs[*i].tach.is_some())
            .collect()
    } else {
        opts.outputs
            .iter()
            .map(|n| {
                engine
                    .outputs
                    .iter()
                    .position(|o| &o.name == n)
                    .ok_or_else(|| Error(format!("unknown output {n:?}")))
            })
            .collect::<Result<_>>()?
    };
    if targets.is_empty() {
        return Err(Error("no output with a tach to calibrate".into()));
    }
    for &i in &targets {
        if engine.outputs[i].tach.is_none() {
            return Err(Error(format!("output {} has no tach", engine.outputs[i].name)));
        }
    }

    let now = engine.now();
    for o in &mut engine.outputs {
        o.force(100.0, now, &root)
            .map_err(|e| Error(format!("output {}: {e}", o.name)))?;
    }
    safety(engine, opts.max_temp)?;

    for &i in &targets {
        let name = engine.outputs[i].name.clone();
        let steps = opts
            .steps
            .clone()
            .unwrap_or_else(|| default_steps(engine.outputs[i].min_duty));
        let mut points: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
        log_notice!("calibrating {name}: duties {steps:?}, {}s settle", opts.settle);
        for &duty in &steps {
            let d = engine.outputs[i].clamp(duty);
            let now = engine.now();
            engine.outputs[i]
                .force(d, now, &root)
                .map_err(|e| Error(format!("output {name}: {e}")))?;
            wait(engine, opts.settle, &term, opts.max_temp)?;
            let mut sums: BTreeMap<String, (f64, usize)> = BTreeMap::new();
            for n in 0..opts.samples.max(1) {
                if n > 0 {
                    wait(engine, 1.0, &term, opts.max_temp)?;
                }
                let now = engine.now();
                for (label, _input, rpm) in engine.outputs[i]
                    .read_rotors(now, &root)
                    .map_err(|e| Error(format!("output {name}: {e}")))?
                {
                    let e = sums.entry(label).or_insert((0.0, 0));
                    e.0 += rpm;
                    e.1 += 1;
                }
            }
            let line: Vec<String> = sums
                .iter()
                .map(|(l, (s, n))| format!("{l} {:.0}", s / *n as f64))
                .collect();
            log_info!("{name} at {d:.0}%: {}", line.join(", "));
            for (label, (s, n)) in sums {
                points.entry(label).or_default().push((d, (s / n as f64).round()));
            }
        }
        let entry = cal.outputs.entry(name.clone()).or_default();
        for (label, mut pts) in points {
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let max_rpm = pts.last().map_or(0.0, |p| p.1);
            if max_rpm <= 0.0 {
                log_error!("{name}/{label}: no rotation measured, not recording it");
                continue;
            }
            entry.rotors.insert(label, RotorCal { max_rpm, points: pts });
        }
        let now = engine.now();
        engine.outputs[i]
            .force(100.0, now, &root)
            .map_err(|e| Error(format!("output {name}: {e}")))?;
    }
    let errs = guard.finish();
    if !errs.is_empty() {
        return Err(Error(format!(
            "calibration done but exit duty failed: {}",
            errs.join("; ")
        )));
    }
    Ok(cal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_cover_the_range() {
        assert_eq!(default_steps(50.196), vec![100.0, 90.0, 80.0, 70.0, 60.0, 51.0]);
        assert_eq!(
            default_steps(30.0),
            vec![100.0, 90.0, 80.0, 70.0, 60.0, 50.0, 40.0, 30.0]
        );
        assert_eq!(default_steps(0.0).last(), Some(&1.0));
    }
}
