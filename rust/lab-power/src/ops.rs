//! The verbs on targets and devices, and their output.

use std::io::Write;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::cli::{self, Args};
use crate::config::{Backend, Config, Device, Named, Target};
use crate::ha;
use crate::netbotz::{self, Session};

const NO_METER: &str = "no power meter on this target";

enum Fail {
    Usage(String),
    Failed(String),
}

/// Runs the command line; 0 ok, 1 failed, 2 usage.
pub(crate) fn main(argv: &[String]) -> ExitCode {
    match run(argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fail::Usage(msg)) => {
            eprintln!("lab-power: {msg}\n{}", cli::USAGE);
            ExitCode::from(2)
        }
        Err(Fail::Failed(msg)) => {
            eprintln!("lab-power: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(words: &[String]) -> Result<(), Fail> {
    let args = cli::parse(words).map_err(Fail::Usage)?;
    if args.help {
        say(cli::USAGE);
        return Ok(());
    }
    let config = Config::load().map_err(Fail::Failed)?;
    if args.list {
        list(&config, args.json);
        return Ok(());
    }
    match config.lookup(&args.target) {
        None => Err(Fail::Usage(format!(
            "unknown target {:?} (targets: {})",
            args.target,
            config.names()
        ))),
        Some(Named::Target(t)) => on_target(t, &args).map_err(Fail::Failed),
        Some(Named::Device(d)) => on_device(&config, d, &args).map_err(Fail::Failed),
    }
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}").and_then(|()| out.flush());
}

/// A power reading and where it came from.
struct Power {
    watts: f64,
    source: String,
}

/// One feed's state after a read or a switch.
struct Reading {
    state: String,
    line: String,
    json: Map<String, Value>,
    power: Option<Power>,
    power_note: Option<String>,
}

impl Reading {
    fn power_line(&self) -> String {
        match (&self.power, &self.power_note) {
            (Some(p), _) => format!("  power: {:.1} W ({})", p.watts, p.source),
            (None, Some(note)) => format!("  power: {note}"),
            (None, None) => format!("  power: {NO_METER}"),
        }
    }

    fn to_json(&self) -> Value {
        let mut m = self.json.clone();
        m.insert("state".into(), json!(self.state));
        m.insert(
            "power".into(),
            match &self.power {
                Some(p) => json!({ "watts": p.watts, "source": p.source }),
                None => Value::Null,
            },
        );
        if self.power.is_none() {
            m.insert(
                "power_note".into(),
                json!(self.power_note.as_deref().unwrap_or(NO_METER)),
            );
        }
        Value::Object(m)
    }
}

fn base_json(t: &Target) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("target".into(), json!(t.name));
    m.insert("feeds".into(), json!(t.feeds));
    m
}

// --- backends --------------------------------------------------------------

fn read(t: &Target, args: &Args) -> Result<Reading, String> {
    match &t.backend {
        Backend::HomeAssistant { entity, power_sensor } => {
            let client = ha::Client::from_env(args.timeout)?;
            let state = client.state(entity)?;
            ha_reading(t, entity, power_sensor.as_deref(), &client, state)
        }
        Backend::NetBotz(cfg) => {
            let mut session = Session::open(cfg, args.timeout)?;
            let outlet = session.outlet()?;
            session.close();
            Ok(nb_reading(t, &cfg.host, &outlet))
        }
    }
}

fn switch(t: &Target, on: bool, args: &Args) -> Result<Reading, String> {
    let word = netbotz::on_off(on);
    match &t.backend {
        Backend::HomeAssistant { entity, power_sensor } => {
            let client = ha::Client::from_env(args.timeout)?;
            let before = client.state(entity)?;
            let before = before
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if before == "unavailable" || before == "unknown" {
                return Err(format!("{entity} is {before}; cannot confirm control"));
            }
            client.switch(entity, on)?;
            let deadline = Instant::now() + args.wait;
            loop {
                let state = client.state(entity)?;
                let now = state
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if now == word {
                    return ha_reading(t, entity, power_sensor.as_deref(), &client, state);
                }
                if Instant::now() >= deadline {
                    return Err(format!(
                        "service accepted but {entity} did not report {word}; last state: {now}"
                    ));
                }
                thread::sleep(Duration::from_millis(250).min(deadline.saturating_duration_since(Instant::now())));
            }
        }
        Backend::NetBotz(cfg) => {
            let mut session = Session::open(cfg, args.timeout)?;
            let before = session.outlet()?;
            let after = if before.on == on {
                before
            } else {
                session.set_outlet(on, args.wait)?
            };
            session.close();
            Ok(nb_reading(t, &cfg.host, &after))
        }
    }
}

fn ha_reading(
    t: &Target,
    entity: &str,
    power_sensor: Option<&str>,
    client: &ha::Client,
    state: Value,
) -> Result<Reading, String> {
    let s = state
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut m = base_json(t);
    m.insert("backend".into(), json!("home-assistant"));
    m.insert("entity_id".into(), json!(entity));
    m.insert("home_assistant".into(), state);
    let (power, power_note) = match power_sensor {
        None => (None, None),
        Some(sensor) => ha_power(client, sensor)?,
    };
    Ok(Reading {
        line: format!("plug {} ({entity}): {s}", t.name),
        state: s,
        json: m,
        power,
        power_note,
    })
}

/// A W (or kW) sensor of Home Assistant; unavailable readings are a note, not an error.
fn ha_power(client: &ha::Client, sensor: &str) -> Result<(Option<Power>, Option<String>), String> {
    let v = client.state(sensor)?;
    let raw = v.get("state").and_then(Value::as_str).unwrap_or_default();
    let unit = v
        .pointer("/attributes/unit_of_measurement")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let scale = match unit {
        "W" => 1.0,
        "kW" => 1000.0,
        _ => return Err(format!("{sensor} reports {unit:?}, not W or kW")),
    };
    match raw.parse::<f64>() {
        Ok(x) if x.is_finite() => Ok((
            Some(Power {
                watts: x * scale,
                source: format!("Home Assistant {sensor}"),
            }),
            None,
        )),
        _ => Ok((None, Some(format!("meter {sensor} is {raw}")))),
    }
}

fn nb_reading(t: &Target, host: &str, outlet: &netbotz::Outlet) -> Reading {
    let state = netbotz::on_off(outlet.on).to_owned();
    let normal = netbotz::on_off(outlet.normal_on);
    let mut m = base_json(t);
    m.insert("backend".into(), json!("netbotz"));
    m.insert("host".into(), json!(host));
    m.insert("outlet".into(), json!(outlet.name));
    m.insert("normal_state".into(), json!(normal));
    Reading {
        line: format!(
            "{} (NetBotz {host} switched outlet \"{}\"): {state}, normal state {normal}",
            t.name, outlet.name
        ),
        state,
        json: m,
        power: None,
        power_note: Some("no power meter on this target (the NetBotz 250 outlet does not meter)".to_owned()),
    }
}

/// Only reads a meter (no outlet session when there is none).
fn power_only(t: &Target, args: &Args) -> Result<Reading, String> {
    let Backend::HomeAssistant {
        entity,
        power_sensor: Some(sensor),
    } = &t.backend
    else {
        let mut m = base_json(t);
        m.insert("meter".into(), json!(false));
        return Ok(Reading {
            state: String::new(),
            line: t.name.clone(),
            json: m,
            power: None,
            power_note: None,
        });
    };
    let client = ha::Client::from_env(args.timeout)?;
    let (power, power_note) = ha_power(&client, sensor)?;
    let mut m = base_json(t);
    m.insert("backend".into(), json!("home-assistant"));
    m.insert("entity_id".into(), json!(entity));
    let line = format!("plug {} ({entity})", t.name);
    Ok(Reading {
        state: String::new(),
        line,
        json: m,
        power,
        power_note,
    })
}

fn has_meter(t: &Target) -> bool {
    matches!(
        t.backend,
        Backend::HomeAssistant {
            power_sensor: Some(_),
            ..
        }
    )
}

// --- single targets ----------------------------------------------------------

fn feed_notice(t: &Target, action: &str) {
    if let Some(feeds) = &t.feeds {
        eprintln!("lab-power: {action} {}: it feeds {feeds}", t.name);
    }
}

fn emit(r: &Reading, args: &Args, extra: &[(&str, Value)]) {
    if args.json {
        let mut v = r.to_json();
        if let Some(m) = v.as_object_mut() {
            for (k, x) in extra {
                m.insert((*k).to_owned(), x.clone());
            }
        }
        say(&v.to_string());
    } else {
        say(&r.line);
        say(&r.power_line());
    }
}

fn on_target(t: &Target, args: &Args) -> Result<(), String> {
    match args.action.as_str() {
        "status" => emit(&read(t, args)?, args, &[]),
        "on" => emit(&switch(t, true, args)?, args, &[("action", json!("on"))]),
        "off" => {
            feed_notice(t, "off");
            emit(&switch(t, false, args)?, args, &[("action", json!("off"))]);
        }
        "cycle" => {
            feed_notice(t, "cycle");
            let off = args.off_time.unwrap_or(t.cycle_off);
            switch(t, false, args)?;
            eprintln!("lab-power: {} is off; on again in {:.1} s", t.name, off.as_secs_f64());
            thread::sleep(off);
            let r = switch(t, true, args)?;
            emit(
                &r,
                args,
                &[("action", json!("cycle")), ("off_seconds", json!(off.as_secs_f64()))],
            );
        }
        _ => power(std::slice::from_ref(t), args)?,
    }
    Ok(())
}

/// `power` for one target or all feeds of a device; `--watch` repeats it.
fn power(targets: &[Target], args: &Args) -> Result<(), String> {
    if args.watch.is_some() && !targets.iter().any(has_meter) {
        return Err(format!("{NO_METER}: nothing to watch"));
    }
    let mut samples = 0u64;
    loop {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let readings = targets
            .iter()
            .map(|t| power_only(t, args))
            .collect::<Result<Vec<_>, _>>()?;
        let metered: Vec<f64> = readings
            .iter()
            .filter_map(|r| r.power.as_ref().map(|p| p.watts))
            .collect();
        let total = (metered.len() == readings.len()).then(|| metered.iter().sum::<f64>());
        if args.json {
            let feeds: Vec<Value> = readings.iter().map(Reading::to_json).collect();
            let mut v = json!({ "time": stamp, "feeds": feeds, "total_watts": total });
            if let ([one], Some(m)) = (feeds.as_slice(), v.as_object_mut()) {
                m.insert("power".into(), one.get("power").cloned().unwrap_or(Value::Null));
            }
            say(&v.to_string());
        } else {
            for r in &readings {
                let what = match (&r.power, &r.power_note) {
                    (Some(p), _) => format!("{:.1} W ({})", p.watts, p.source),
                    (None, Some(n)) => n.clone(),
                    (None, None) => NO_METER.to_owned(),
                };
                let prefix = if args.watch.is_some() {
                    format!("{stamp} ")
                } else {
                    String::new()
                };
                say(&format!("{prefix}{}: {what}", r.line));
            }
            if let (Some(total), true) = (total, readings.len() > 1) {
                say(&format!("total: {total:.1} W"));
            }
        }
        samples += 1;
        match args.watch {
            Some(every) if args.count.is_none_or(|n| samples < n) => thread::sleep(every),
            _ => return Ok(()),
        }
    }
}

// --- devices -------------------------------------------------------------------

fn on_device(config: &Config, d: &Device, args: &Args) -> Result<(), String> {
    let feeds: Vec<Target> = d.feeds.iter().filter_map(|f| config.targets.get(f).cloned()).collect();
    if args.action == "power" {
        return power(&feeds, args);
    }
    if args.action == "status" {
        let results: Vec<(String, Result<Reading, String>)> =
            feeds.iter().map(|t| (t.name.clone(), read(t, args))).collect();
        report_device(d, args, "status", &results);
        let bad: Vec<String> = results
            .iter()
            .filter(|(_, r)| r.is_err())
            .map(|(n, _)| n.clone())
            .collect();
        return if bad.is_empty() {
            Ok(())
        } else {
            Err(format!("{}: could not read feed(s) {}", d.name, bad.join(", ")))
        };
    }
    let list = feeds
        .iter()
        .map(|t| t.feeds.as_ref().map_or(t.name.clone(), |f| format!("{} ({f})", t.name)));
    eprintln!(
        "lab-power: {} {}: all feeds of {}: {}",
        args.action,
        d.name,
        d.description,
        list.collect::<Vec<_>>().join(", ")
    );
    // Pre-flight: every feed must be readable before anything is switched.
    for t in &feeds {
        read(t, args).map_err(|e| format!("{}: feed {} cannot be read, nothing switched: {e}", d.name, t.name))?;
    }
    match args.action.as_str() {
        "on" | "off" => {
            let on = args.action == "on";
            let results: Vec<(String, Result<Reading, String>)> =
                feeds.iter().map(|t| (t.name.clone(), switch(t, on, args))).collect();
            report_device(d, args, &args.action, &results);
            failed(d, &results, &format!("did not turn {}", args.action))
        }
        _ => {
            let off = args
                .off_time
                .unwrap_or_else(|| feeds.iter().map(|t| t.cycle_off).max().unwrap_or_default());
            let offs: Vec<(String, Result<Reading, String>)> =
                feeds.iter().map(|t| (t.name.clone(), switch(t, false, args))).collect();
            if offs.iter().any(|(_, r)| r.is_err()) {
                // Not a cold cycle: put every feed back on and say which one did not follow.
                let restore: Vec<(String, Result<Reading, String>)> =
                    feeds.iter().map(|t| (t.name.clone(), switch(t, true, args))).collect();
                report_device(d, args, "cycle-aborted", &restore);
                let mut msg = failed(d, &offs, "did not turn off; cycle aborted, all feeds switched back on")
                    .err()
                    .unwrap_or_default();
                if let Err(e) = failed(d, &restore, "did not come back on") {
                    msg = format!("{msg}; {e}");
                }
                return Err(msg);
            }
            eprintln!(
                "lab-power: {}: all feeds off; on again in {:.1} s",
                d.name,
                off.as_secs_f64()
            );
            thread::sleep(off);
            let ons: Vec<(String, Result<Reading, String>)> =
                feeds.iter().map(|t| (t.name.clone(), switch(t, true, args))).collect();
            report_device(d, args, "cycle", &ons);
            failed(d, &ons, "did not come back on")
        }
    }
}

fn failed(d: &Device, results: &[(String, Result<Reading, String>)], what: &str) -> Result<(), String> {
    let bad: Vec<String> = results
        .iter()
        .filter_map(|(n, r)| r.as_ref().err().map(|e| format!("feed {n} {what}: {e}")))
        .collect();
    if bad.is_empty() {
        Ok(())
    } else {
        Err(format!("{}: {}", d.name, bad.join("; ")))
    }
}

fn report_device(d: &Device, args: &Args, action: &str, results: &[(String, Result<Reading, String>)]) {
    if args.json {
        let feeds: Vec<Value> = results
            .iter()
            .map(|(n, r)| match r {
                Ok(r) => r.to_json(),
                Err(e) => json!({ "target": n, "error": e }),
            })
            .collect();
        let states: Vec<&str> = results
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok().map(|r| r.state.as_str()))
            .collect();
        say(&json!({
            "device": d.name,
            "description": d.description,
            "action": action,
            "state": summary(&states, results.len()),
            "feeds": feeds,
        })
        .to_string());
        return;
    }
    let states: Vec<&str> = results
        .iter()
        .filter_map(|(_, r)| r.as_ref().ok().map(|r| r.state.as_str()))
        .collect();
    say(&format!(
        "{} ({}): {}",
        d.name,
        d.description,
        summary(&states, results.len())
    ));
    for (name, r) in results {
        match r {
            Ok(r) => {
                say(&format!("  feed {name}: {}", r.line));
                say(&format!("  {}", r.power_line()));
            }
            Err(e) => say(&format!("  feed {name}: ERROR {e}")),
        }
    }
}

fn summary(states: &[&str], total: usize) -> String {
    if states.len() < total {
        return "unknown (a feed could not be read)".to_owned();
    }
    if states.iter().all(|s| *s == "on") {
        "all feeds on".to_owned()
    } else if states.iter().all(|s| *s == "off") {
        "all feeds off".to_owned()
    } else {
        format!("mixed ({})", states.join(", "))
    }
}

fn list(config: &Config, json_out: bool) {
    if json_out {
        let targets: Vec<Value> = config
            .targets
            .values()
            .map(|t| {
                let (backend, detail) = describe(t);
                json!({ "target": t.name, "backend": backend, "detail": detail, "feeds": t.feeds, "meter": has_meter(t) })
            })
            .collect();
        let devices: Vec<Value> = config
            .devices
            .values()
            .map(|d| json!({ "device": d.name, "description": d.description, "feeds": d.feeds }))
            .collect();
        say(&json!({ "targets": targets, "devices": devices }).to_string());
        return;
    }
    for t in config.targets.values() {
        let (backend, detail) = describe(t);
        say(&format!(
            "{:<14} {backend:<15} {detail:<36} feeds {}",
            t.name,
            t.feeds.as_deref().unwrap_or("-")
        ));
    }
    for d in config.devices.values() {
        say(&format!(
            "{:<14} {:<15} {} = {}",
            d.name,
            "device",
            d.description,
            d.feeds.join(" + ")
        ));
    }
}

fn describe(t: &Target) -> (&'static str, String) {
    match &t.backend {
        Backend::HomeAssistant { entity, .. } => ("home-assistant", entity.clone()),
        Backend::NetBotz(n) => ("netbotz", format!("{}:{} nboutlet", n.host, n.port)),
    }
}
