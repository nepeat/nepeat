//! fand command line.

use fand::calibrate::{self, CalOptions};
use fand::config::{self, Config, DutySpec};
use fand::engine::{Engine, ExecMode, Options};
use fand::error::{Error, Result};
use fand::{check, daemon, log, log_error};
use std::collections::BTreeMap;
use std::path::PathBuf;

const USAGE: &str = "\
fand - fan control daemon

usage: fand <command> [options]

commands:
  run         run the control loop (as a systemd Type=notify service)
                --dry-run            compute and log, never write
  check       validate the config and show resolved devices (read-only)
                --syntax-only        only parse and validate, touch nothing
                --exec               also run exec sensors once
  status      print the running daemon's state as JSON
                --socket PATH        (default: from the config)
  set DUTY    manual override, e.g. 80%, 60 or 12000rpm; \"set clear\" ends it
                --zone NAME          (default: all zones)
                --for DURATION       (default 10m, capped by max_override)
  calibrate   step duties and record per-rotor RPM into calibration_file
                --output NAME        (repeatable; default: every output with a tach)
                --steps 100,80,60    duties to visit
                --settle 8s          wait after each change
                --samples 3          readings averaged per step
                --max-temp 70        abort if any sensor reaches this (C)
                --write PATH | -     (default: calibration_file; - prints)
                --force              run even if the daemon socket answers
  failsafe    set every output to the exit duty now (systemd ExecStopPost)
                --duty PCT           (default: exit_duty)
  curves      print the built-in curves

common options:
  --config PATH   (default /etc/fand/config.yaml, or $FAND_CONFIG)
  --debug         debug logging
  --help, --version
";

const VALUE_FLAGS: &[&str] = &[
    "config", "socket", "zone", "for", "output", "steps", "settle", "samples", "max-temp", "write", "duty",
];
const BOOL_FLAGS: &[&str] = &["dry-run", "syntax-only", "exec", "force", "debug", "help", "version"];

struct Args {
    positional: Vec<String>,
    values: BTreeMap<String, Vec<String>>,
    flags: Vec<String>,
}

impl Args {
    fn parse(raw: impl Iterator<Item = String>) -> Result<Args> {
        let mut a = Args {
            positional: Vec::new(),
            values: BTreeMap::new(),
            flags: Vec::new(),
        };
        let mut it = raw.peekable();
        while let Some(arg) = it.next() {
            if arg == "-h" {
                a.flags.push("help".into());
            } else if let Some(opt) = arg.strip_prefix("--") {
                let (name, inline) = match opt.split_once('=') {
                    Some((n, v)) => (n.to_string(), Some(v.to_string())),
                    None => (opt.to_string(), None),
                };
                if VALUE_FLAGS.contains(&name.as_str()) {
                    let v = match inline {
                        Some(v) => v,
                        None => it.next().ok_or_else(|| Error(format!("--{name} needs a value")))?,
                    };
                    a.values.entry(name).or_default().push(v);
                } else if BOOL_FLAGS.contains(&name.as_str()) && inline.is_none() {
                    a.flags.push(name);
                } else {
                    return Err(Error(format!("unknown option --{name} (see fand --help)")));
                }
            } else {
                a.positional.push(arg);
            }
        }
        Ok(a)
    }

    fn flag(&self, n: &str) -> bool {
        self.flags.iter().any(|f| f == n)
    }

    fn value(&self, n: &str) -> Option<&str> {
        self.values.get(n).and_then(|v| v.last()).map(String::as_str)
    }

    fn config_path(&self) -> PathBuf {
        self.value("config")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("FAND_CONFIG").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(config::DEFAULT_CONFIG))
    }

    fn config(&self) -> Result<Config> {
        Config::load(&self.config_path())
    }

    fn socket(&self) -> Result<(PathBuf, Option<PathBuf>)> {
        if let Some(s) = self.value("socket") {
            return Ok((PathBuf::from(s), None));
        }
        let cfg = self.config()?;
        let sock = cfg
            .runtime
            .socket
            .clone()
            .ok_or_else(|| Error("the config disables the control socket (runtime.socket: null)".into()))?;
        Ok((sock, cfg.runtime.state_file.clone()))
    }
}

fn secs(s: &str) -> Result<f64> {
    config::parse_duration(s).map_err(Error)
}

fn real_main() -> Result<i32> {
    let args = Args::parse(std::env::args().skip(1))?;
    log::set_debug(args.flag("debug"));
    if args.flag("version") {
        println!("fand {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    let Some(cmd) = args.positional.first().cloned() else {
        print!("{USAGE}");
        return Ok(if args.flag("help") { 0 } else { 2 });
    };
    if args.flag("help") {
        print!("{USAGE}");
        return Ok(0);
    }
    let extra = &args.positional[1..];
    let no_extra = || {
        if extra.is_empty() {
            Ok(())
        } else {
            Err(Error(format!("unexpected argument {:?}", extra[0])))
        }
    };
    match cmd.as_str() {
        "run" => {
            no_extra()?;
            daemon::run(args.config()?, args.flag("dry-run"))
        }
        "check" => {
            no_extra()?;
            let path = args.config_path();
            let cfg = args.config()?;
            if args.flag("syntax-only") {
                println!(
                    "{}: ok ({} zones, {} sensors, {} outputs)",
                    path.display(),
                    cfg.zones.len(),
                    cfg.sensors.len(),
                    cfg.outputs.len()
                );
                // Warnings never change the exit code (this runs at NixOS build time).
                for w in fand::hwmin::static_warnings(&cfg) {
                    println!("{}", fand::hwmin::line(&w));
                }
                return Ok(0);
            }
            log::set_quiet(true);
            let (text, ok) = check::report(cfg, args.flag("exec"))?;
            println!(
                "{}: {}\n{text}",
                path.display(),
                if ok {
                    "ok"
                } else {
                    "PROBLEMS FOUND (the daemon would run these zones in failsafe)"
                }
            );
            Ok(if ok { 0 } else { 2 })
        }
        "status" => {
            no_extra()?;
            let (sock, state) = args.socket()?;
            match daemon::request(&sock, &serde_json::json!({"cmd": "status"})) {
                Ok(v) if v["ok"] == true => {
                    println!("{}", serde_json::to_string_pretty(&v["status"]).unwrap_or_default());
                    Ok(0)
                }
                Ok(v) => Err(Error(format!("daemon: {}", v["error"]))),
                Err(e) => match state.and_then(|p| std::fs::read_to_string(&p).ok().map(|s| (p, s))) {
                    Some((p, s)) => {
                        eprintln!("fand: {e}; showing {} instead", p.display());
                        print!("{s}");
                        Ok(1)
                    }
                    None => Err(e),
                },
            }
        }
        "set" => {
            let what = extra
                .first()
                .ok_or_else(|| Error("usage: fand set <duty>|clear [--zone Z] [--for 10m]".into()))?;
            if extra.len() > 1 {
                return Err(Error(format!("unexpected argument {:?}", extra[1])));
            }
            let (sock, _) = args.socket()?;
            let zone = args.value("zone");
            let req = if what == "clear" {
                serde_json::json!({"cmd": "clear", "zone": zone})
            } else {
                DutySpec::parse(what).map_err(Error)?;
                let dur = secs(args.value("for").unwrap_or("10m"))?;
                serde_json::json!({"cmd": "set", "zone": zone, "duty": what, "seconds": dur})
            };
            let v = daemon::request(&sock, &req)?;
            if v["ok"] == true {
                println!("{}", v["message"].as_str().unwrap_or("ok"));
                Ok(0)
            } else {
                Err(Error(format!("daemon: {}", v["error"].as_str().unwrap_or("error"))))
            }
        }
        "calibrate" => {
            no_extra()?;
            let cfg = args.config()?;
            if !args.flag("force") {
                if let Some(s) = &cfg.runtime.socket {
                    if std::os::unix::net::UnixStream::connect(s).is_ok() {
                        return Err(Error("the daemon is running; stop it first (or pass --force)".into()));
                    }
                }
            }
            let steps = match args.value("steps") {
                None => None,
                Some(s) => Some(
                    s.split(',')
                        .map(|x| match DutySpec::parse(x) {
                            Ok(DutySpec::Percent(p)) => Ok(p),
                            _ => Err(Error(format!("bad step {x:?}"))),
                        })
                        .collect::<Result<Vec<f64>>>()?,
                ),
            };
            let opts = CalOptions {
                outputs: args.values.get("output").cloned().unwrap_or_default(),
                steps,
                settle: secs(args.value("settle").unwrap_or("8s"))?,
                samples: args
                    .value("samples")
                    .unwrap_or("3")
                    .parse()
                    .map_err(|_| Error("--samples needs a number".into()))?,
                max_temp: args
                    .value("max-temp")
                    .unwrap_or("70")
                    .parse()
                    .map_err(|_| Error("--max-temp needs a number".into()))?,
            };
            let target = match args.value("write") {
                Some("-") => None,
                Some(p) => Some(PathBuf::from(p)),
                None => Some(
                    cfg.runtime
                        .calibration_file
                        .clone()
                        .ok_or_else(|| Error("no calibration_file configured; pass --write".into()))?,
                ),
            };
            let (term, _) = daemon::install_signals()?;
            let cal = calibrate::calibrate(cfg, opts, term)?;
            match target {
                None => println!("{}", serde_json::to_string_pretty(&cal).unwrap_or_default()),
                Some(p) => {
                    cal.save(&p)?;
                    println!("wrote {}", p.display());
                }
            }
            Ok(0)
        }
        "failsafe" => {
            no_extra()?;
            let cfg = args.config()?;
            let duty = match args.value("duty") {
                Some(d) => match DutySpec::parse(d).map_err(Error)? {
                    DutySpec::Percent(p) => p,
                    DutySpec::Rpm(_) => return Err(Error("--duty takes a percentage".into())),
                },
                None => cfg.exit_duty.0,
            };
            let mut opts = Options::new(&cfg);
            opts.exec_mode = ExecMode::Inline;
            let mut engine = Engine::new(cfg, opts)?;
            let now = engine.now();
            let errs = engine.shutdown(duty, now);
            Ok(if errs.is_empty() { 0 } else { 1 })
        }
        "curves" => {
            no_extra()?;
            print!("{}", check::curves_table());
            Ok(0)
        }
        other => Err(Error(format!("unknown command {other:?} (see fand --help)"))),
    }
}

fn main() {
    let code = match real_main() {
        Ok(c) => c,
        Err(e) => {
            log_error!("{e}");
            1
        }
    };
    std::process::exit(code);
}
