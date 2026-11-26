//! Command-line parsing (no dependency: the grammar is two words and a few options).

use std::time::Duration;

/// The verbs; a target or device name may not be one of them.
pub(crate) const ACTIONS: &[&str] = &["status", "on", "off", "cycle", "power", "normal-state"];

/// The verbs as the error messages list them.
const VERBS: &str = "status, on, off, cycle, power or normal-state on|off";

/// What the command line asked for.
#[derive(Debug)]
pub(crate) struct Args {
    pub(crate) json: bool,
    pub(crate) list: bool,
    pub(crate) help: bool,
    /// Per HTTP request / per SSH prompt.
    pub(crate) timeout: Duration,
    /// How long to wait for a switched feed to report its new state.
    pub(crate) wait: Duration,
    /// `cycle`: how long the feed stays off (else the target's `cycle_off_seconds`).
    pub(crate) off_time: Option<Duration>,
    /// `power`: sample every this often.
    pub(crate) watch: Option<Duration>,
    /// `power --watch`: stop after this many samples.
    pub(crate) count: Option<u64>,
    pub(crate) target: String,
    pub(crate) action: String,
    /// `normal-state on|off`: the outlet's state after the `NetBotz` itself restarts.
    pub(crate) normal: Option<bool>,
}

pub(crate) const USAGE: &str = "usage: lab-power <target|device> status|on|off|cycle|power [options]
       lab-power <netbotz-target> normal-state on|off [options]
       lab-power --list
normal-state sets the NetBotz outlet's normal state: the state it takes after the NetBotz itself
restarts (nboutlet -ns); the outlet's current state is left as it is.
options:
  --json             machine-readable output (one JSON object; one per sample with --watch)
  --timeout SECONDS  per HTTP request or SSH prompt (default 10)
  --wait SECONDS     how long a switched feed (or normal state) may take to read back (default 5)
  --off-time SECONDS cycle: time off (default: the target's cycle_off_seconds, 10)
  --watch SECONDS    power: sample repeatedly; --count N stops after N samples
There is no default target: every call names one.";

/// Parses the arguments after the program name.
pub(crate) fn parse(words: &[String]) -> Result<Args, String> {
    let mut args = Args {
        json: false,
        list: false,
        help: false,
        timeout: Duration::from_secs(10),
        wait: Duration::from_secs(5),
        off_time: None,
        watch: None,
        count: None,
        target: String::new(),
        action: String::new(),
        normal: None,
    };
    let mut positional = Vec::new();
    let mut it = words.iter();
    while let Some(arg) = it.next() {
        if arg == "--" {
            positional.extend(it.by_ref().cloned());
            break;
        }
        if arg == "-h" {
            args.help = true;
            continue;
        }
        let Some(flag) = arg.strip_prefix("--") else {
            if arg.len() > 1 && arg.starts_with('-') {
                return Err(format!("unknown option {arg}"));
            }
            positional.push(arg.clone());
            continue;
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (flag, None),
        };
        match name {
            "json" | "list" | "help" => {
                if inline.is_some() {
                    return Err(format!("--{name} takes no value"));
                }
                match name {
                    "json" => args.json = true,
                    "list" => args.list = true,
                    _ => args.help = true,
                }
            }
            "timeout" | "wait" | "off-time" | "watch" | "count" => {
                let value = match inline {
                    Some(value) => value,
                    None => it.next().cloned().ok_or_else(|| format!("--{name} needs a value"))?,
                };
                match name {
                    "timeout" => args.timeout = seconds(name, &value)?,
                    "wait" => args.wait = seconds(name, &value)?,
                    "off-time" => args.off_time = Some(seconds(name, &value)?),
                    "watch" => args.watch = Some(seconds(name, &value)?),
                    _ => args.count = Some(count(&value)?),
                }
            }
            _ => return Err(format!("unknown option --{name}")),
        }
    }
    if args.help || args.list {
        if positional.is_empty() {
            return Ok(args);
        }
        return Err("--list and --help take no target".to_owned());
    }
    let mut words = positional.into_iter();
    match (words.next(), words.next(), words.next(), words.next()) {
        (None, ..) => Err("name a target and an action".to_owned()),
        (Some(first), None, ..) if ACTIONS.contains(&first.as_str()) => Err(format!(
            "no default target: name one first, e.g. \"lab-power 2 {first}\" (lab-power --list)"
        )),
        (Some(_), None, ..) => Err(format!("missing the action ({VERBS})")),
        (Some(_), Some(_), _, Some(extra)) => Err(format!("unexpected argument {extra:?}")),
        (Some(target), Some(action), value, None) => {
            if !ACTIONS.contains(&action.as_str()) {
                return Err(format!("unknown action {action:?} ({VERBS})"));
            }
            args.normal = normal_value(&action, value.as_deref())?;
            if args.off_time.is_some() && action != "cycle" {
                return Err("--off-time is for cycle".to_owned());
            }
            if (args.watch.is_some() || args.count.is_some()) && action != "power" {
                return Err("--watch and --count are for power".to_owned());
            }
            if args.count.is_some() && args.watch.is_none() {
                return Err("--count needs --watch".to_owned());
            }
            args.target = target;
            args.action = action;
            Ok(args)
        }
    }
}

/// The third word: only `normal-state` takes one (on or off), and needs it.
fn normal_value(action: &str, value: Option<&str>) -> Result<Option<bool>, String> {
    match (action, value) {
        ("normal-state", Some("on")) => Ok(Some(true)),
        ("normal-state", Some("off")) => Ok(Some(false)),
        ("normal-state", Some(v)) => Err(format!("normal-state takes on or off, not {v:?}")),
        ("normal-state", None) => Err("normal-state needs on or off".to_owned()),
        (_, Some(extra)) => Err(format!("unexpected argument {extra:?}")),
        (_, None) => Ok(None),
    }
}

fn seconds(name: &str, value: &str) -> Result<Duration, String> {
    let bad = || format!("--{name} must be a finite positive number of seconds, up to a day");
    let secs: f64 = value.trim().parse().map_err(|_| bad())?;
    if !secs.is_finite() || secs <= 0.0 || secs > 86_400.0 {
        return Err(bad());
    }
    Duration::try_from_secs_f64(secs).map_err(|_| bad())
}

fn count(value: &str) -> Result<u64, String> {
    match value.trim().parse::<u64>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err("--count must be a positive integer".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Args, String> {
        parse(&args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn target_and_action_required() {
        for argv in [&["off"][..], &["on"], &["status"], &["cycle"], &[], &["off", "2", "x"]] {
            assert!(p(argv).is_err(), "{argv:?}");
        }
        assert!(p(&["off"]).unwrap_err().contains("no default target"));
        assert!(p(&["2", "toggle"]).is_err());
    }

    #[test]
    fn options_anywhere_and_validated() {
        let a = p(&["--json", "1", "status", "--timeout=2.5"]).unwrap();
        assert!(a.json);
        assert_eq!(a.timeout, Duration::from_millis(2500));
        assert_eq!((a.target.as_str(), a.action.as_str()), ("1", "status"));
        for bad in [
            &["1", "on", "--wait", "0"][..],
            &["1", "on", "--wait", "-1"],
            &["1", "on", "--wait", "nan"],
            &["1", "on", "--wait"],
            &["2", "off", "--entity", "switch.x"],
            &["1", "on", "-x"],
            &["1", "on", "--off-time", "3"],
            &["1", "status", "--watch", "1"],
            &["1", "power", "--count", "2"],
            &["--json=1", "1", "status"],
            &["--list", "1"],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
        let c = p(&["1", "cycle", "--off-time", "12"]).unwrap();
        assert_eq!(c.off_time, Some(Duration::from_secs(12)));
        let w = p(&["1", "power", "--watch", "2", "--count", "3"]).unwrap();
        assert_eq!((w.watch, w.count), (Some(Duration::from_secs(2)), Some(3)));
        assert!(p(&["--list"]).unwrap().list);
    }

    #[test]
    fn normal_state_takes_on_or_off() {
        let a = p(&["nb", "normal-state", "on", "--json"]).unwrap();
        assert_eq!(
            (a.action.as_str(), a.normal, a.json),
            ("normal-state", Some(true), true)
        );
        assert_eq!(p(&["nb", "normal-state", "off"]).unwrap().normal, Some(false));
        assert_eq!(p(&["nb", "status"]).unwrap().normal, None);
        for bad in [
            &["nb", "normal-state"][..],
            &["nb", "normal-state", "On"],
            &["nb", "normal-state", "toggle"],
            &["nb", "normal-state", "on", "x"],
            &["nb", "on", "on"],
            &["nb", "normal-state", "on", "--off-time", "3"],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }
}
