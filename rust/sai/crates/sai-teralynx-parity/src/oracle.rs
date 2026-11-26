//! The oracle runner's (`sai-script`, ai_re/nix/packages/sai-teralynx/oracle/sai_script.cpp) output on
//! the mock: its step markers (`M step <tag>`, `M end <tag> status=<n> ms=<t>`, written to
//! `IFCS_TRACE_FILE`, the same file as the tap's stream) cut the IFCS lines into steps; its stdout
//! (`STEP <tag> <status> <ms> ms <values>`) compared without the times, the asynchronous `event`
//! lines apart (as a multiset).
#![forbid(unsafe_code)]

use crate::stream::Unit;
use regex::Regex;
use std::sync::LazyLock;

/// Times: the end markers' `ms=`, `route_fill`'s `block_ms=` / `us_per_route=`, any `<x>_ms=`.
static TIMES: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)((?:[a-z_]*_)?ms|us_per_[a-z_]+)=[0-9.]+").ok());
static STEP: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^STEP (.*?)[ \t]+(-?[0-9]+)[ \t]+[0-9.]+ ms ?(.*)$").ok());

/// A line without its times (`ms=1.5` -> `ms=*`).
pub fn untimed(line: &str) -> String {
    TIMES
        .as_ref()
        .map_or_else(|| line.to_owned(), |re| re.replace_all(line, "${1}=*").into_owned())
}

/// The op of a step tag (`<depth>.<index> <op> <desc>`).
pub fn step_op(tag: &str) -> &str {
    tag.split_whitespace().nth(1).unwrap_or("")
}

/// The steps of a stream: `init` before the first step marker, then one unit per `M step`
/// (its marker, the IFCS lines, the end marker without its time, lines up to the next step).
pub fn steps(text: &str) -> Vec<Unit> {
    let mut out = vec![Unit {
        name: "init".to_owned(),
        lines: Vec::new(),
        died: false,
    }];
    for line in text.lines() {
        let line = untimed(line);
        if let Some(tag) = line.strip_prefix("M step ") {
            out.push(Unit {
                name: tag.to_owned(),
                lines: Vec::new(),
                died: false,
            });
        }
        if let Some(u) = out.last_mut() {
            u.lines.push(line);
        }
    }
    out
}

/// The stdout without times; the `event` lines (callbacks, asynchronous) sorted apart.
pub fn stdout(text: &str) -> (Vec<String>, Vec<String>) {
    let mut lines = Vec::new();
    let mut events = Vec::new();
    for l in text.lines() {
        if l.starts_with("event ") {
            events.push(l.to_owned());
            continue;
        }
        match STEP.as_ref().and_then(|re| re.captures(l)) {
            Some(c) => lines.push(format!(
                "STEP {} {} {}",
                c.get(1).map_or("", |m| m.as_str()),
                c.get(2).map_or("", |m| m.as_str()),
                c.get(3).map_or("", |m| m.as_str())
            )),
            None => lines.push(untimed(l)),
        }
    }
    events.sort();
    (lines, events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_cut_the_stream_and_lose_the_times() {
        let s = steps(
            "M script x\nM step 0.0 create_switch\nifcs_init\nM end 0.0 create_switch status=0 ms=1.599\nlate\nM step 0.1 get\nM end 0.1 get status=0 ms=0.013\n",
        );
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].lines, ["M script x"]);
        assert_eq!(s[1].name, "0.0 create_switch");
        assert_eq!(
            s[1].lines,
            [
                "M step 0.0 create_switch",
                "ifcs_init",
                "M end 0.0 create_switch status=0 ms=*",
                "late"
            ]
        );
        assert_eq!(step_op("0.12 qstats q0 before"), "qstats");
        assert_eq!(
            untimed("FILL create n=1000 block_ms=11.0 us_per_route=11.0"),
            "FILL create n=1000 block_ms=* us_per_route=*"
        );
    }

    #[test]
    fn stdout_without_times_and_events_apart() {
        let (l, e) = stdout(
            "STEP 0.0 create_switch                        0      1.599 ms \nevent port 0x1 oper 1\nSTEP 0.13 create drop dst   -131073      0.001 ms 0x0\nsai-script: 8 failed steps\n",
        );
        assert_eq!(
            l,
            [
                "STEP 0.0 create_switch 0 ",
                "STEP 0.13 create drop dst -131073 0x0",
                "sai-script: 8 failed steps"
            ]
        );
        assert_eq!(stdout("FILL done x ms=3.25\n").0, ["FILL done x ms=*"]);
        assert_eq!(untimed("got items=5 params=2"), "got items=5 params=2");
        assert_eq!(e, ["event port 0x1 oper 1"]);
    }
}
