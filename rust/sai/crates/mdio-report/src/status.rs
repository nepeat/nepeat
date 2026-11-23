//! The status files: what happened, for programs (JSON) and for people (text).

use std::ffi::OsStr;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use mdio_script::{E_ABORT, E_FILE, E_IO, E_POLL, E_SYNTAX, E_VERIFY, Failure, OpLetter, Stats, Until};

use crate::class::FailClass;
use crate::names::Names;
use crate::report::{PhyState, Report};
use crate::text::{CBuf, base, cut, put};

const SUMMARY_LEN: usize = 700;
const HINT_LEN: usize = 400;
const SDK_LEN: usize = 320;

/// Where a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// Not finished.
    Running,
    /// Finished, everything as the scripts expect.
    Ok,
    /// Finished, not as expected.
    Failed,
    /// Stopped by a signal.
    Interrupted,
}

impl State {
    /// `running`, `ok`, `failed`, `interrupted`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// What the tool was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Run the scripts.
    Run,
    /// Read the verification sections only.
    Verify,
}

impl Mode {
    /// `run` or `verify`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Verify => "verify",
        }
    }
}

/// The JSON's word for a script's result code (`mdio_script::E_*`, 0 = ok).
pub const fn result_name(code: i32) -> &'static str {
    match code {
        0 => "ok",
        E_FILE => "file",
        E_SYNTAX => "syntax",
        E_POLL => "poll",
        E_IO => "io",
        E_VERIFY => "verify",
        E_ABORT => "aborted",
        _ => "?",
    }
}

/// One script of a run.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptStatus {
    /// The path as given on the command line.
    pub path: Vec<u8>,
    /// What the parse pass counted (`ops`: operations in the file).
    pub parsed: Stats,
    /// The run's counters, when `started`.
    pub run: Stats,
    /// `run` and `result` hold numbers.
    pub started: bool,
    /// The run's result code (`mdio_script::E_*`, 0 = ok).
    pub result: i32,
    /// How long the run took.
    pub seconds: f64,
}

impl ScriptStatus {
    /// A script that was parsed and not run yet.
    pub fn new(path: &OsStr, parsed: Stats) -> Self {
        Self {
            path: path.as_bytes().to_vec(),
            parsed,
            run: Stats::default(),
            started: false,
            result: 0,
            seconds: 0.0,
        }
    }
}

/// The ASIC temperature published while the script ran.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TempStatus {
    /// Good readings.
    pub reads: u64,
    /// Failed or implausible readings.
    pub failed: u64,
    /// The hottest monitor of the last good reading, in degrees C.
    pub last_c: f64,
    /// The longest a reading took, in milliseconds.
    pub max_ms: f64,
}

/// Everything the status files say about a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// Where the run is.
    pub state: State,
    /// What was asked.
    pub mode: Mode,
    /// Unix time of the start.
    pub started: i64,
    /// Unix time of the end, 0 while running.
    pub finished: i64,
    /// Run time so far.
    pub seconds: f64,
    /// The tool's process id.
    pub pid: u32,
    sdk: Vec<u8>,
    /// The scripts, in command-line order.
    pub scripts: Vec<ScriptStatus>,
    /// The failure's class, when there is one.
    pub fail_class: Option<FailClass>,
    /// The script that failed.
    pub fail_script: Option<Vec<u8>>,
    /// Where and why it stopped (all zero when it is not about an operation).
    pub fail: Failure,
    summary: Vec<u8>,
    hint: Vec<u8>,
    /// The verification reads, classified; `None`: none were made.
    pub report: Option<Report>,
    /// When they were made: "the end of the script", "after the failure (reads only)", "now".
    pub report_when: Option<&'static str>,
    /// Names for PHYs and registers.
    pub names: Option<Names>,
    /// `Some` when the ASIC temperature is published while the script runs.
    pub temp: Option<TempStatus>,
}

fn phy_label(names: Option<&Names>, phy: u16) -> Vec<u8> {
    match names.and_then(|n| n.phy(u32::from(phy))) {
        Some(name) => {
            let mut b = name.to_vec();
            put!(b, " (phy 0x{phy:x})");
            b
        }
        None => format!("phy 0x{phy:x}").into_bytes(),
    }
}

fn reg_label(names: Option<&Names>, dev: u8, reg: u16) -> Vec<u8> {
    match names.and_then(|n| n.reg(u32::from(dev), u32::from(reg))) {
        Some(name) => {
            let mut b = name.to_vec();
            put!(b, " ({dev:x}.{reg:04x})");
            b
        }
        None => format!("{dev:x}.{reg:04x}").into_bytes(),
    }
}

/// A JSON string: `"` and `\` escaped, control bytes as `\u00XX`, every other byte as it is.
fn jstr(out: &mut Vec<u8>, s: &[u8]) {
    out.push(b'"');
    for &ch in s {
        match ch {
            b'"' | b'\\' => out.extend_from_slice(&[b'\\', ch]),
            0..0x20 => put!(out, "\\u{ch:04x}"),
            _ => out.push(ch),
        }
    }
    out.push(b'"');
}

fn jstr_or_null(out: &mut Vec<u8>, s: Option<&[u8]>) {
    match s {
        Some(s) => jstr(out, s),
        None => out.extend_from_slice(b"null"),
    }
}

fn jstats(out: &mut Vec<u8>, s: &Stats) {
    put!(
        out,
        "{{\"operations\": {}, \"writes\": {}, \"reads\": {}, \"polls\": {}, \"poll_rereads\": {}, \
         \"delays\": {}, \"labels\": {}, \"expected_values_differed\": {}, \"verify_checks\": {}, \
         \"verify_failed\": {}}}",
        s.ops,
        s.writes,
        s.reads,
        s.polls,
        s.poll_rereads,
        s.delays,
        s.labels,
        s.differs,
        s.verify_checks,
        s.verify_failed
    );
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
#[allow(
    clippy::many_single_char_names,
    reason = "the names of the published algorithm (days_from_civil)"
)]
const fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if month <= 2 { 1 } else { 0 }, month, day)
}

/// `%Y-%m-%d %H:%M:%S %z` of a unix time, in UTC.
fn utc(time: i64) -> String {
    let (year, month, day) = civil(time.div_euclid(86400));
    let secs = time.rem_euclid(86400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} +0000",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

impl Status {
    /// A run that is starting now.
    pub fn new(mode: Mode, started: i64, pid: u32) -> Self {
        Self {
            state: State::Running,
            mode,
            started,
            finished: 0,
            seconds: 0.0,
            pid,
            sdk: Vec::new(),
            scripts: Vec::new(),
            fail_class: None,
            fail_script: None,
            fail: Failure::default(),
            summary: Vec::new(),
            hint: Vec::new(),
            report: None,
            report_when: None,
            names: None,
            temp: None,
        }
    }

    /// The chip / MDIO clock line (empty before the SDK is up).
    pub fn sdk(&self) -> &[u8] {
        &self.sdk
    }

    /// Sets the chip line (at most 319 bytes are kept).
    pub fn set_sdk(&mut self, line: &[u8]) {
        self.sdk = cut(line, SDK_LEN);
    }

    /// One line: what happened.
    pub fn summary(&self) -> &[u8] {
        &self.summary
    }

    /// Sets the summary (at most 699 bytes are kept). [`Status::summarize`] leaves a summary that
    /// is not empty alone: for a failure before anything ran.
    pub fn set_summary(&mut self, text: &[u8]) {
        self.summary = cut(text, SUMMARY_LEN);
    }

    /// One line: what to do.
    pub fn hint(&self) -> &[u8] {
        &self.hint
    }

    /// Sets the hint (at most 399 bytes are kept): a board's own text for the class.
    pub fn set_hint(&mut self, text: &[u8]) {
        self.hint = cut(text, HINT_LEN);
    }

    /// Operations done and in total, over all scripts.
    fn totals(&self) -> (u64, u64) {
        self.scripts.iter().fold((0u64, 0u64), |(done, all), s| {
            (
                done.saturating_add(if s.started { s.run.ops } else { 0 }),
                all.saturating_add(s.parsed.ops),
            )
        })
    }

    /// The one-line summary of the run into the status (and the class's hint, unless one is
    /// set), from the other fields: call before writing.
    pub fn summarize(&mut self) {
        let (done, all) = self.totals();
        if let Some(class) = self.fail_class {
            if self.hint.is_empty() {
                self.hint = cut(class.hint().as_bytes(), HINT_LEN);
            }
        }
        if !self.summary.is_empty() {
            return; // set by the caller (nothing ran)
        }
        let nphy = self.report.as_ref().map_or(0, |r| r.phys().len());
        let text = match self.state {
            State::Ok => match self.mode {
                Mode::Verify => format!("{nphy} PHYs verified, all as the script expects").into_bytes(),
                Mode::Run if nphy > 0 => {
                    format!("{nphy} PHYs verified after {done} operations in {:.1} s", self.seconds).into_bytes()
                }
                Mode::Run => format!(
                    "{done} operations in {:.1} s; the script has no verification section",
                    self.seconds
                )
                .into_bytes(),
            },
            State::Running => format!("{done} of {all} operations done").into_bytes(),
            State::Failed | State::Interrupted => self.failure_summary(done, all),
        };
        self.summary = cut(&text, SUMMARY_LEN);
    }

    /// Failed or interrupted while a script ran.
    #[allow(clippy::too_many_lines, reason = "one C function, kept in its order")]
    fn failure_summary(&self, done: u64, all: u64) -> Vec<u8> {
        let f = &self.fail;
        let r = self.report.as_ref();
        let names = self.names.as_ref();
        let rv = self
            .scripts
            .iter()
            .rfind(|s| s.started && s.result != 0)
            .map_or(0, |s| s.result);
        let but = |until: bool| if until { "anything but " } else { "" };
        let mut s = CBuf::new(SUMMARY_LEN);
        if f.line != 0 {
            s.app(base(self.fail_script.as_deref().unwrap_or_default()));
            s.app(format!(" line {} [", f.line).as_bytes());
            s.app(f.label.as_bytes());
            s.app(b"]: ");
        }
        match (self.fail_class, f.op, r) {
            (Some(FailClass::Interrupted), _, _) => s.app(b"stopped on request (signal)"),
            (Some(FailClass::MdioIo), Some(op), _) => {
                s.app(format!("MDIO {} of ", if op == OpLetter::W { "write" } else { "read" }).as_bytes());
                s.app(&phy_label(names, f.phy));
                s.app(b" ");
                s.app(&reg_label(names, f.dev, f.reg));
                s.app(b" failed in the switch chip's MDIO master");
            }
            (_, _, Some(r)) if rv == E_VERIFY && !r.phys().is_empty() => {
                let differ = r.phys().len().saturating_sub(r.count(PhyState::Ok));
                s.app(format!("verification: {differ} of {} PHYs differ: ", r.phys().len()).as_bytes());
                let mut shown = 0usize;
                for p in r.phys().iter().filter(|p| p.state != PhyState::Ok).take(3) {
                    s.app(if shown > 0 { b"; " } else { b"" });
                    shown = shown.saturating_add(1);
                    s.app(&phy_label(names, p.phy));
                    s.app(b" ");
                    if p.state == PhyState::Constant {
                        s.app(format!("reads 0x{:04x} on every verified register", p.constant).as_bytes());
                        continue;
                    }
                    match p.checks().iter().find(|c| !c.ok) {
                        None => s.app(b"differs"),
                        Some(c) if c.io_error => {
                            s.app(&reg_label(names, c.dev, c.reg));
                            s.app(b": read failed");
                        }
                        Some(c) => {
                            s.app(&reg_label(names, c.dev, c.reg));
                            s.app(
                                format!(
                                    " read 0x{:04x}, expected {}0x{:04x}{}",
                                    c.got,
                                    but(c.until == Until::NotEqual),
                                    c.want,
                                    if c.mask == 0xffff { "" } else { " (masked)" }
                                )
                                .as_bytes(),
                            );
                        }
                    }
                }
                if differ > shown {
                    s.app(b"; ...");
                }
            }
            (_, Some(op), _) => {
                s.app(&phy_label(names, f.phy));
                s.app(b" ");
                s.app(&reg_label(names, f.dev, f.reg));
                s.app(
                    format!(
                        " read 0x{:04x}, the script expects {}0x{:04x} (mask 0x{:04x}) after {} read{}",
                        f.got,
                        but(op == OpLetter::N),
                        f.want,
                        f.mask,
                        f.tries,
                        if f.tries == 1 { "" } else { "s" }
                    )
                    .as_bytes(),
                );
            }
            _ => s.app(b"failed"),
        }
        s.app(format!("; {done} of {all} operations had run").as_bytes());
        match self.fail_class {
            Some(FailClass::PhyConstant) => {
                let constant = |p: &&crate::report::PhyReport| p.state == PhyState::Constant;
                let p = r
                    .and_then(|r| r.find(f.phy))
                    .filter(constant)
                    .or_else(|| r.and_then(|r| r.phys().iter().find(constant)));
                if let Some(p) = p {
                    s.app(b"; ");
                    s.app(&phy_label(names, p.phy));
                    s.app(
                        format!(
                            " reads 0x{:04x} on every verified register: not answering or hung",
                            p.constant
                        )
                        .as_bytes(),
                    );
                }
            }
            Some(FailClass::Diverged) => {
                s.app(b"; ");
                if r.and_then(|r| r.find(f.phy)).is_some() {
                    s.app(b"the PHY still answers (its registers are not one constant): ");
                }
                s.app(b"it left the recorded sequence");
            }
            Some(FailClass::Verify) => {
                s.app(b"; the PHYs answer, but firmware or configuration is not as recorded");
            }
            _ => {}
        }
        s.into_bytes()
    }

    /// The machine-readable record (`status.json`).
    #[allow(clippy::too_many_lines, reason = "one C function, kept in its order")]
    pub fn json(&self, out: &mut Vec<u8>) {
        let (done, all) = self.totals();
        let names = self.names.as_ref();
        out.extend_from_slice(b"{\n \"version\": 1,\n \"state\": ");
        jstr(out, self.state.as_str().as_bytes());
        out.extend_from_slice(b",\n \"mode\": ");
        jstr(out, self.mode.as_str().as_bytes());
        out.extend_from_slice(b",\n \"summary\": ");
        jstr(out, &self.summary);
        put!(
            out,
            ",\n \"pid\": {},\n \"started\": {},\n \"finished\": ",
            self.pid,
            self.started
        );
        if self.finished == 0 {
            out.extend_from_slice(b"null");
        } else {
            put!(out, "{}", self.finished);
        }
        put!(
            out,
            ",\n \"seconds\": {:.1},\n \"operations_done\": {done},\n \"operations_total\": {all},\n \"sdk\": ",
            self.seconds
        );
        jstr(out, &self.sdk);
        out.extend_from_slice(b",\n \"scripts\": [");
        for (i, s) in self.scripts.iter().enumerate() {
            put!(out, "{}\n  {{\"path\": ", if i > 0 { "," } else { "" });
            jstr(out, &s.path);
            out.extend_from_slice(b", \"result\": ");
            jstr_or_null(out, s.started.then(|| result_name(s.result).as_bytes()));
            put!(
                out,
                ", \"seconds\": {:.1}, \"verify_line\": {},\n   \"file\": ",
                s.seconds,
                s.parsed.verify_line
            );
            jstats(out, &s.parsed);
            out.extend_from_slice(b",\n   \"run\": ");
            if s.started {
                jstats(out, &s.run);
            } else {
                out.extend_from_slice(b"null");
            }
            out.push(b'}');
        }
        out.extend_from_slice(b"\n ],\n \"failure\": ");
        match self.fail_class {
            Some(class) => self.json_failure(out, class),
            None => out.extend_from_slice(b"null"),
        }
        out.extend_from_slice(b",\n \"asic_temperature\": ");
        match &self.temp {
            Some(t) => put!(
                out,
                "{{\"readings\": {}, \"failed\": {}, \"last_celsius\": {:.1}, \"longest_read_ms\": {:.2}}}",
                t.reads,
                t.failed,
                t.last_c,
                t.max_ms
            ),
            None => out.extend_from_slice(b"null"),
        }
        out.extend_from_slice(b",\n \"verified\": ");
        match &self.report {
            Some(r) => {
                out.extend_from_slice(b"{\"when\": ");
                jstr(out, self.report_when.unwrap_or("").as_bytes());
                put!(
                    out,
                    ", \"phys_ok\": {}, \"phys_total\": {},\n  \"phys\": [",
                    r.count(PhyState::Ok),
                    r.phys().len()
                );
                for (i, p) in r.phys().iter().enumerate() {
                    put!(
                        out,
                        "{}\n   {{\"phy\": \"0x{:x}\", \"name\": ",
                        if i > 0 { "," } else { "" },
                        p.phy
                    );
                    jstr_or_null(out, names.and_then(|n| n.phy(u32::from(p.phy))));
                    put!(out, ", \"state\": \"{}\", \"registers\": [", p.state.name());
                    for (k, c) in p.checks().iter().enumerate() {
                        put!(
                            out,
                            "{}{{\"reg\": \"{:x}.{:04x}\", \"name\": ",
                            if k > 0 { ", " } else { "" },
                            c.dev,
                            c.reg
                        );
                        jstr_or_null(out, names.and_then(|n| n.reg(u32::from(c.dev), u32::from(c.reg))));
                        if c.io_error {
                            out.extend_from_slice(b", \"read\": null");
                        } else {
                            put!(out, ", \"read\": \"0x{:04x}\"", c.got);
                        }
                        put!(
                            out,
                            ", \"expected\": \"{}0x{:04x}\", \"mask\": \"0x{:04x}\", \"ok\": {}}}",
                            if c.until == Until::NotEqual { "!" } else { "" },
                            c.want,
                            c.mask,
                            c.ok
                        );
                    }
                    out.extend_from_slice(b"]}");
                }
                out.extend_from_slice(b"\n  ]}");
            }
            None => out.extend_from_slice(b"null"),
        }
        out.extend_from_slice(b"\n}\n");
    }

    fn json_failure(&self, out: &mut Vec<u8>, class: FailClass) {
        let x = &self.fail;
        out.extend_from_slice(b"{\"class\": ");
        jstr(out, class.as_str().as_bytes());
        out.extend_from_slice(b", \"hint\": ");
        jstr(out, &self.hint);
        out.extend_from_slice(b",\n  \"script\": ");
        jstr_or_null(out, self.fail_script.as_deref());
        if x.line != 0 {
            put!(out, ", \"line\": {}, \"label\": ", x.line);
            jstr(out, x.label.as_bytes());
            out.extend_from_slice(b", \"op\": ");
            let op = x.op.map(|op| op.as_char().to_string()).unwrap_or_default();
            jstr(out, op.as_bytes());
            if x.op.is_some() {
                put!(out, ",\n  \"phy\": \"0x{:x}\", \"phy_name\": ", x.phy);
                jstr_or_null(out, self.names.as_ref().and_then(|n| n.phy(u32::from(x.phy))));
                put!(
                    out,
                    ", \"dev\": {}, \"reg\": \"0x{:04x}\", \"expected\": \"0x{:04x}\", \"mask\": \"0x{:04x}\", \"read\": ",
                    x.dev,
                    x.reg,
                    x.want,
                    x.mask
                );
                if x.have_value {
                    put!(out, "\"0x{:04x}\"", x.got);
                } else {
                    out.extend_from_slice(b"null");
                }
                put!(out, ", \"reads\": {}", x.tries);
            }
        }
        out.push(b'}');
    }

    /// The record for people (`status.txt`). Line 1 is `openbcm-mdio-script: ok: ...`,
    /// `...: FAILED (class): ...` or `...: running: ...`; then the details and the PHY table.
    pub fn text(&self, out: &mut Vec<u8>) {
        match self.state {
            State::Ok => out.extend_from_slice(b"openbcm-mdio-script: ok: "),
            State::Running => out.extend_from_slice(b"openbcm-mdio-script: running: "),
            State::Failed | State::Interrupted => put!(
                out,
                "openbcm-mdio-script: FAILED ({}): ",
                self.fail_class.map_or(self.state.as_str(), FailClass::as_str)
            ),
        }
        out.extend_from_slice(&self.summary);
        out.push(b'\n');
        if !self.hint.is_empty() {
            out.extend_from_slice(b"  what to do: ");
            out.extend_from_slice(&self.hint);
            out.push(b'\n');
        }
        put!(
            out,
            "  {}",
            if self.mode == Mode::Verify {
                "verification only"
            } else {
                "run"
            }
        );
        if self.started != 0 {
            put!(out, " started {}", utc(self.started));
        }
        if self.finished == 0 {
            put!(out, ", pid {}", self.pid);
        } else {
            put!(out, ", took {:.1} s", self.seconds);
        }
        out.push(b'\n');
        if !self.sdk.is_empty() {
            out.extend_from_slice(b"  switch chip: ");
            out.extend_from_slice(&self.sdk);
            out.push(b'\n');
        }
        for s in &self.scripts {
            out.extend_from_slice(b"  ");
            out.extend_from_slice(&s.path);
            if !s.started {
                put!(out, ": not run ({} operations)\n", s.parsed.ops);
                continue;
            }
            put!(
                out,
                ": {}, {} of {} operations in {:.1} s ({} writes, {} reads, {} re-reads; {} of the recorded \
                 values differed)\n",
                if s.result == 0 { "done" } else { "STOPPED" },
                s.run.ops,
                s.parsed.ops,
                s.seconds,
                s.run.writes,
                s.run.reads,
                s.run.poll_rereads,
                s.run.differs
            );
        }
        if let Some(t) = &self.temp {
            put!(
                out,
                "  ASIC temperature published meanwhile: {} readings, last {:.1} C, {} failed, longest read {:.2} ms\n",
                t.reads,
                t.last_c,
                t.failed,
                t.max_ms
            );
        }
        if let Some(r) = self.report.as_ref().filter(|r| !r.phys().is_empty()) {
            put!(
                out,
                "  PHY registers read at {}: {} of {} PHYs as expected\n",
                self.report_when.unwrap_or("the end"),
                r.count(PhyState::Ok),
                r.phys().len()
            );
            r.table(out, self.names.as_ref());
        }
    }

    /// Writes the JSON to `json_path` and the text next to it ([`text_path`]), each replaced
    /// atomically; the directory is created (one level). Both are attempted; the error is the
    /// last failure's.
    pub fn write(&self, json_path: &Path) -> std::io::Result<()> {
        let mut json = Vec::new();
        self.json(&mut json);
        let mut text = Vec::new();
        self.text(&mut text);
        let first = write_atomic(json_path, &json);
        write_atomic(&text_path(json_path), &text).and(first)
    }
}

/// The text file of a status file: `.json` replaced by `.txt`, else `.txt` appended.
pub fn text_path(json_path: &Path) -> PathBuf {
    let bytes = json_path.as_os_str().as_bytes();
    let stem = match bytes.strip_suffix(b".json") {
        Some(stem) if !stem.is_empty() => stem,
        _ => bytes,
    };
    let mut txt = stem.to_vec();
    txt.extend_from_slice(b".txt");
    PathBuf::from(OsStr::from_bytes(&txt))
}

/// `path` replaced atomically (`path.tmp` + rename), mode 0644; a missing directory is created
/// (one level, 0755).
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().as_bytes().to_vec();
    tmp.extend_from_slice(b".tmp");
    let tmp = PathBuf::from(OsStr::from_bytes(&tmp));
    let create = || std::fs::File::create(&tmp);
    let mut file = match create() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).ok_or(e)?;
            match std::fs::DirBuilder::new().mode(0o755).create(dir) {
                Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e),
                _ => create()?,
            }
        }
        other => other?,
    };
    let written = file
        .write_all(data)
        .and_then(|()| file.flush())
        .map(|()| {
            // C: fchmod's result is not looked at.
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o644));
        })
        .and_then(|()| {
            drop(file);
            std::fs::rename(&tmp, path)
        });
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_time_text() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 +0000");
        assert_eq!(utc(1_795_000_000), "2026-11-18 11:06:40 +0000");
        assert_eq!(utc(951_782_399), "2000-02-28 23:59:59 +0000");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00 +0000");
        assert_eq!(utc(4_107_542_399), "2100-02-28 23:59:59 +0000");
        assert_eq!(utc(4_107_542_400), "2100-03-01 00:00:00 +0000");
        assert_eq!(utc(-1), "1969-12-31 23:59:59 +0000");
    }

    #[test]
    fn json_strings() {
        let mut out = Vec::new();
        jstr(&mut out, b"a\"b\\c\n\x01\x7f\xff d");
        assert_eq!(out, b"\"a\\\"b\\\\c\\u000a\\u0001\x7f\xff d\"");
    }

    #[test]
    fn text_file_name() {
        assert_eq!(
            text_path(Path::new("/run/x/status.json")),
            Path::new("/run/x/status.txt")
        );
        assert_eq!(text_path(Path::new("/run/x/status")), Path::new("/run/x/status.txt"));
        assert_eq!(text_path(Path::new(".json")), Path::new(".json.txt"));
        assert_eq!(text_path(Path::new("a.json")), Path::new("a.txt"));
    }
}
