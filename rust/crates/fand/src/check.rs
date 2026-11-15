//! `fand check`: validate a config and show what it resolves to, read-only.

use crate::config::Config;
use crate::curves;
use crate::engine::{Engine, ExecMode, Options};
use crate::error::Result;
use std::fmt::Write as _;

/// Returns (report, all_resolved).
pub fn report(cfg: Config, run_exec: bool) -> Result<(String, bool)> {
    let mut opts = Options::new(&cfg);
    opts.exec_mode = ExecMode::Inline;
    opts.dry_run = true;
    opts.calibration = crate::daemon::load_calibration(&cfg);
    let mut out = String::new();
    let mut ok = true;
    let _ = writeln!(
        out,
        "interval {}s, profile {} ({}), failsafe {}%, exit {}%, calm period {}s{}",
        cfg.interval.0,
        cfg.profile,
        cfg.profile.floor(&cfg.profiles),
        cfg.failsafe_duty.0,
        cfg.exit_duty.0,
        cfg.calm_period.0,
        if cfg.dry_run { ", DRY RUN" } else { "" }
    );
    let min_warnings = crate::hwmin::static_warnings(&cfg);
    let mut engine = Engine::new(cfg, opts)?;
    let root = engine.root().to_path_buf();
    let _ = writeln!(out, "\nsensors:");
    for s in &mut engine.sensors {
        let kind = format!("{:?}", s.kind).to_lowercase();
        if let Some(cmd) = s.command() {
            let cmd = cmd.join(" ");
            if !run_exec {
                let _ = writeln!(
                    out,
                    "  {:<14} exec   {}  (not run; use --exec)",
                    s.name,
                    crate::sysfs::truncate(&cmd, 60)
                );
                continue;
            }
            s.update(0.0, &root);
            let val = match (&s.value, &s.error) {
                (Some(v), _) => format!("{v:.1} C"),
                (_, Some(e)) => {
                    ok &= !s.required;
                    format!("ERROR {e}")
                }
                _ => "?".into(),
            };
            let _ = writeln!(
                out,
                "  {:<14} exec   {}  = {val}",
                s.name,
                crate::sysfs::truncate(&cmd, 60)
            );
            continue;
        }
        match s.resolve(0.0, &root) {
            Ok(p) => {
                s.update(0.0, &root);
                let val = match (&s.value, &s.error) {
                    (Some(v), _) => format!("{v:.1} C"),
                    (_, Some(e)) => {
                        ok &= !s.required;
                        format!("ERROR {e}")
                    }
                    _ => "?".into(),
                };
                let p = p
                    .map(|p| p.display().to_string())
                    .or_else(|| s.location())
                    .unwrap_or_default();
                let _ = writeln!(out, "  {:<14} {kind:<6} {p}  = {val}", s.name);
            }
            Err(e) => {
                ok &= !s.required;
                let _ = writeln!(
                    out,
                    "  {:<14} {kind:<6} UNRESOLVED: {e}{}",
                    s.name,
                    if s.required { "" } else { " (optional)" }
                );
            }
        }
    }
    let _ = writeln!(out, "\noutputs:");
    for o in &mut engine.outputs {
        let range = format!(
            "hardware minimum {}, max {}{}",
            o.min_desc,
            crate::hwmin::pct(o.max_duty),
            if o.stop { ", may stop" } else { "" }
        );
        if let Some(lines) = o.i2c_report() {
            // Read-only: the duty register, identity checks, arm state.
            for (i, (good, line)) in lines.iter().enumerate() {
                ok &= good;
                if i == 0 {
                    let _ = writeln!(out, "  {:<14} i2c    {line}  ({range})", o.name);
                } else {
                    let _ = writeln!(out, "      {line}");
                }
            }
        } else if let Some(cmd) = o.command() {
            let _ = writeln!(out, "  {:<14} exec   {}  ({range})", o.name, cmd.join(" "));
        } else {
            match o.resolve(0.0, &root) {
                Ok(p) => {
                    let p = p.map(|p| p.display().to_string()).unwrap_or_default();
                    let cur = std::fs::read_to_string(&p)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|e| format!("unreadable: {e}"));
                    let _ = writeln!(
                        out,
                        "  {:<14} {:<6} {p} = {cur}  ({range})",
                        o.name,
                        format!("{:?}", o.kind).to_lowercase()
                    );
                }
                Err(e) => {
                    ok = false;
                    let _ = writeln!(out, "  {:<14} UNRESOLVED: {e}", o.name);
                }
            }
        }
        if o.tach.is_some() {
            if let Err(e) = o.read_rotors(0.0, &root) {
                ok = false;
                let _ = writeln!(out, "      UNRESOLVED {e}");
            }
            if let Some(t) = &o.tach {
                for r in &t.rotors {
                    let model = match &r.model {
                        Some(m) => format!(
                            "max {:.0} rpm{}",
                            m.max_rpm(),
                            if m.calibrated { ", calibrated" } else { "" }
                        ),
                        None => "no rpm model".into(),
                    };
                    let rpm = r
                        .rpm
                        .map_or_else(|| r.error.clone().unwrap_or_default(), |v| format!("{v:.0} rpm"));
                    let _ = writeln!(
                        out,
                        "      {:<12} {:<6} {rpm:<10} {model}{}",
                        r.label,
                        r.input,
                        if r.fault_flag { "  FAULT" } else { "" }
                    );
                }
            }
        }
    }
    let _ = writeln!(out, "\nzones:");
    let names: Vec<String> = engine.outputs.iter().map(|o| o.name.clone()).collect();
    let sensor_names: Vec<String> = engine.sensors.iter().map(|s| s.name.clone()).collect();
    for (zname, zc) in &engine.cfg.zones {
        let z = engine.zone(zname);
        let _ = writeln!(
            out,
            "  {zname}: {} -> {}; {}; profile {}",
            if zc.sensors.is_empty() {
                "(no sensors)".to_string()
            } else {
                format!(
                    "{:?} of [{}]",
                    zc.aggregate,
                    zc.sensors
                        .iter()
                        .map(|s| s.sensor.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .to_lowercase()
            },
            zc.outputs.join(", "),
            z.map(|z| z.policy_desc.clone()).unwrap_or_default(),
            z.map(|z| z.profile.to_string()).unwrap_or_default(),
        );
        if let Some(c) = zc.policy.curve.as_ref().and_then(|c| engine.cfg.curve(c)) {
            let pts: Vec<String> = c.points.iter().map(|(t, d)| format!("{t}C {d}%")).collect();
            let _ = writeln!(
                out,
                "      {} (hysteresis {} C)",
                pts.join(" | "),
                zc.hysteresis.unwrap_or(c.hysteresis)
            );
        }
    }
    let _ = (names, sensor_names);
    // Settings below a hardware minimum; they never change `ok`.
    let _ = writeln!(out, "\nhardware minimum:");
    if min_warnings.is_empty() {
        let _ = writeln!(out, "  no setting asks an output for less than its hardware minimum");
    }
    for w in &min_warnings {
        let _ = writeln!(out, "  {}", crate::hwmin::line(w));
    }
    Ok((out, ok))
}

/// The built-in curve table.
pub fn curves_table() -> String {
    let mut out = String::new();
    for b in curves::BUILTINS {
        let pts: Vec<String> = b.points.iter().map(|(t, d)| format!("{t}C {d}%")).collect();
        let _ = writeln!(
            out,
            "{:<15} {}  (hysteresis {} C)\n{:<15} {}",
            b.name,
            pts.join(" | "),
            b.hysteresis,
            "",
            b.about
        );
    }
    out
}
