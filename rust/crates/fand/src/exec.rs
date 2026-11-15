//! Running external commands with a timeout, and parsing their output.

use crate::config::{Aggregate, ParseSpec};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const MAX_OUTPUT: u64 = 1 << 20;

/// Run argv (no shell) with a timeout; returns stdout. On timeout the whole
/// process group is killed (while the child is still unreaped, so the group
/// id cannot have been reused) so helper processes do not pile up.
pub fn run(argv: &[String], timeout: Duration) -> Result<String, String> {
    use std::os::unix::process::CommandExt;
    let (prog, args) = argv.split_first().ok_or("empty command")?;
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("cannot run {prog}: {e}"))?;
    let pipe = |r: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(r) = r {
                let _ = r.take(MAX_OUTPUT).read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let out_rx = pipe(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err_rx = pipe(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() >= deadline => {
                kill_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{prog} timed out after {:.1}s", timeout.as_secs_f64()));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(format!("waiting for {prog}: {e}")),
        }
    };
    // A backgrounded grandchild may hold the pipes open; do not wait forever.
    let left = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(100));
    let stdout = out_rx.recv_timeout(left).unwrap_or_default();
    let stderr = err_rx.recv_timeout(Duration::from_millis(100)).unwrap_or_default();
    if !status.success() {
        let msg = String::from_utf8_lossy(&stderr);
        let first = msg.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        return Err(format!(
            "{prog} failed ({status}){}",
            if first.is_empty() {
                String::new()
            } else {
                format!(": {}", crate::sysfs::truncate(first, 120))
            }
        ));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

#[allow(unsafe_code)]
fn kill_group(pid: u32) {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return };
    if pid <= 1 {
        return;
    }
    // SAFETY: kill(2) has no memory-safety preconditions. The group id is our
    // own child's pid (it was started with process_group(0)), so this can only
    // signal that command's process group.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

/// A tiny JSON path: dot-separated keys, `[]`/`[*]`/`*` for every element
/// (array items or object values) and `[N]` for an index.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonPath(Vec<Seg>);

#[derive(Debug, Clone, PartialEq)]
enum Seg {
    Key(String),
    All,
    Index(usize),
}

impl JsonPath {
    pub fn parse(s: &str) -> Result<JsonPath, String> {
        let mut segs = Vec::new();
        let s = s.trim().trim_start_matches('$');
        for part in s.split('.').filter(|p| !p.is_empty()) {
            let (key, mut rest) = match part.find('[') {
                Some(i) => (&part[..i], &part[i..]),
                None => (part, ""),
            };
            match key {
                "" => {}
                "*" => segs.push(Seg::All),
                k => segs.push(Seg::Key(k.to_string())),
            }
            while !rest.is_empty() {
                let end = rest.find(']').ok_or_else(|| format!("unclosed '[' in {s:?}"))?;
                let inner = &rest[1..end];
                segs.push(match inner {
                    "" | "*" => Seg::All,
                    n => Seg::Index(n.parse().map_err(|_| format!("bad index [{n}] in {s:?}"))?),
                });
                rest = &rest[end + 1..];
                if !rest.is_empty() && !rest.starts_with('[') {
                    return Err(format!("unexpected {rest:?} in {s:?}"));
                }
            }
        }
        Ok(JsonPath(segs))
    }

    /// Every number reached by the path; numeric strings count, null and
    /// other types are skipped (e.g. optics without a sensor).
    pub fn numbers(&self, v: &serde_json::Value) -> Vec<f64> {
        let mut cur = vec![v];
        for seg in &self.0 {
            let mut next = Vec::new();
            for v in cur {
                match (seg, v) {
                    (Seg::Key(k), serde_json::Value::Object(m)) => next.extend(m.get(k)),
                    (Seg::Index(i), serde_json::Value::Array(a)) => next.extend(a.get(*i)),
                    (Seg::All, serde_json::Value::Array(a)) => next.extend(a.iter()),
                    (Seg::All, serde_json::Value::Object(m)) => next.extend(m.values()),
                    _ => {}
                }
            }
            cur = next;
        }
        cur.into_iter()
            .filter_map(|v| match v {
                serde_json::Value::Number(n) => n.as_f64(),
                serde_json::Value::String(s) => crate::sysfs::parse_number(s).ok(),
                _ => None,
            })
            .filter(|f| f.is_finite())
            .collect()
    }
}

/// Exec output parser.
#[derive(Debug, Clone)]
pub enum Parser {
    Number,
    Regex(regex_lite::Regex, Aggregate),
    Json(JsonPath, Aggregate),
}

impl Parser {
    pub fn from_spec(spec: Option<&ParseSpec>) -> Result<Parser, String> {
        let Some(s) = spec else { return Ok(Parser::Number) };
        match (&s.regex, &s.json) {
            (Some(r), None) => Ok(Parser::Regex(
                regex_lite::Regex::new(r).map_err(|e| e.to_string())?,
                s.aggregate,
            )),
            (None, Some(j)) => Ok(Parser::Json(JsonPath::parse(j)?, s.aggregate)),
            (None, None) => Ok(Parser::Number),
            (Some(_), Some(_)) => Err("parse takes regex or json, not both".into()),
        }
    }

    pub fn parse(&self, out: &str) -> Result<f64, String> {
        match self {
            Parser::Number => crate::sysfs::parse_number(out),
            Parser::Regex(re, agg) => {
                let vals: Vec<f64> = re
                    .captures_iter(out)
                    .filter_map(|c| c.get(1).or_else(|| c.get(0)))
                    .filter_map(|m| crate::sysfs::parse_number(m.as_str()).ok())
                    .collect();
                aggregate(&vals, *agg).ok_or_else(|| {
                    format!(
                        "regex {:?} matched no number in {:?}",
                        re.as_str(),
                        crate::sysfs::truncate(out.trim(), 60)
                    )
                })
            }
            Parser::Json(path, agg) => {
                let v: serde_json::Value = serde_json::from_str(out).map_err(|e| format!("output is not JSON: {e}"))?;
                aggregate(&path.numbers(&v), *agg).ok_or_else(|| "JSON path matched no number".to_string())
            }
        }
    }
}

/// Combine values (weights are all 1 here).
pub fn aggregate(vals: &[f64], agg: Aggregate) -> Option<f64> {
    let pairs: Vec<(f64, f64)> = vals.iter().map(|v| (*v, 1.0)).collect();
    aggregate_weighted(&pairs, agg)
}

pub fn aggregate_weighted(vals: &[(f64, f64)], agg: Aggregate) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    let it = vals.iter().map(|(v, _)| *v);
    Some(match agg {
        Aggregate::Max => it.fold(f64::NEG_INFINITY, f64::max),
        Aggregate::Min => it.fold(f64::INFINITY, f64::min),
        Aggregate::Mean => it.sum::<f64>() / vals.len() as f64,
        Aggregate::WeightedMean => {
            let w: f64 = vals.iter().map(|(_, w)| w).sum();
            if w <= 0.0 {
                return None;
            }
            vals.iter().map(|(v, w)| v * w).sum::<f64>() / w
        }
        Aggregate::First => vals[0].0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Vec<String> {
        vec!["/bin/sh".into(), "-c".into(), script.into()]
    }

    #[test]
    fn runs_commands_and_reports_failures() {
        assert_eq!(run(&sh("echo 41.5"), Duration::from_secs(5)).unwrap().trim(), "41.5");
        let e = run(&sh("echo broken >&2; exit 3"), Duration::from_secs(5)).unwrap_err();
        assert!(e.contains("failed") && e.contains("broken"), "{e}");
        // "cannot run ..." natively; under qemu-user (cross tests) the spawn
        // succeeds and the emulator exits non-zero. Either way: an error.
        let e = run(&["/nonexistent/tool".to_string()], Duration::from_secs(1)).unwrap_err();
        assert!(e.contains("/nonexistent/tool"), "{e}");
        assert!(run(&[], Duration::from_secs(1)).is_err());
    }

    #[test]
    fn times_out_and_kills() {
        let t = Instant::now();
        let e = run(&sh("while :; do :; done"), Duration::from_millis(200)).unwrap_err();
        assert!(e.contains("timed out"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn json_paths() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"ports":[{"name":"Ethernet0","temp":41.5},{"name":"Ethernet4","temp":null},{"temp":"47.25"}],
               "by_port":{"e0":{"t":30},"e4":{"t":33}},"asic":{"temp":58}}"#,
        )
        .unwrap();
        let n = |p: &str| JsonPath::parse(p).unwrap().numbers(&v);
        assert_eq!(n("ports[].temp"), [41.5, 47.25]);
        assert_eq!(n("ports[*].temp"), [41.5, 47.25]);
        assert_eq!(n("ports[0].temp"), [41.5]);
        assert_eq!(n("by_port.*.t"), [30.0, 33.0]);
        assert_eq!(n("$.asic.temp"), [58.0]);
        assert!(n("missing.key").is_empty());
        assert!(JsonPath::parse("a[1").is_err());
        assert!(JsonPath::parse("a[x]").is_err());
        let arr: serde_json::Value = serde_json::from_str("[1, 5, 3]").unwrap();
        assert_eq!(JsonPath::parse("[]").unwrap().numbers(&arr), [1.0, 5.0, 3.0]);
    }

    #[test]
    fn parsers_and_aggregation() {
        let spec = |regex: Option<&str>, json: Option<&str>, aggregate| ParseSpec {
            regex: regex.map(String::from),
            json: json.map(String::from),
            aggregate,
        };
        let p = Parser::from_spec(Some(&spec(Some(r"temperature\s*=\s*([0-9.]+)"), None, Aggregate::Max))).unwrap();
        assert_eq!(
            p.parse("unit 0: temperature = 51.2\nunit 1: temperature = 57.9\n")
                .unwrap(),
            57.9
        );
        assert!(p.parse("no data").unwrap_err().contains("matched no number"));
        let p = Parser::from_spec(Some(&spec(None, Some("[].temp"), Aggregate::Mean))).unwrap();
        assert_eq!(p.parse(r#"[{"temp": 40}, {"temp": 50}]"#).unwrap(), 45.0);
        assert!(p.parse("not json").is_err());
        assert_eq!(Parser::Number.parse("62\n").unwrap(), 62.0);
        assert_eq!(aggregate(&[3.0, 9.0, 1.0], Aggregate::Min), Some(1.0));
        assert_eq!(aggregate(&[3.0, 9.0, 1.0], Aggregate::First), Some(3.0));
        assert_eq!(aggregate(&[], Aggregate::Max), None);
        assert_eq!(
            aggregate_weighted(&[(40.0, 1.0), (60.0, 3.0)], Aggregate::WeightedMean),
            Some(55.0)
        );
    }
}
