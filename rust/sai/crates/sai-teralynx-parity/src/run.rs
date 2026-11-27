//! The runs and the verdict: the Rust libsai on the mock libifcs, judged against the golden C++ side
//! (`tests/golden`, [`crate::golden`]: what the C++ libsai of the CS8210 gave on the same mock, captured at H5-0
//! from the C++ sai-teralynx package, deleted at H5 on 2026-11-27, last in the tree at commit 767d9b0). There
//! is no C++ side to run: nothing here compiles or starts C or C++ of that package. The programs are
//! the fixtures' (`build-fixtures.sh`), each run in the same directories (so every path in the output is the
//! same), then compared part by part:
//!
//! - **checks**: the Rust `test_ifcs` (`Config::test_ifcs`: the test binary of
//!   crates/sai-backend-teralynx/tests/ifcs_e2e, the port of the C++ `test_ifcs`, which marks its sections in the
//!   stream and writes their counts as the C++ test's section wrappers did): its per-section counts (the gate
//!   value "Rust passes N of M"), every Rust FAIL line with its area, the checks not reached;
//! - **sections**: the same run's IFCS call streams (the tap), section by section;
//! - **driver**: `bin/tl-frontend-driver`: the op scripts (sai-driver's `tests/driver-scripts`) through the
//!   libsai, op block by op block (status, values, the op's IFCS lines, notifications);
//! - **oracle**: the oracle's JSON scripts (their SAI steps: `c/oracle_prep.cpp`) through the Rust
//!   `sai-script --cpp-names` (crate sai-script: in that mode equal to the C++ runner the golden was captured
//!   with, its own differential), step by step.
//!
//! Nothing builds `tl-frontend-driver` since H5: it was the C++ frontend driver (`frontend_driver.cpp` of the
//! deleted frozen frontend) linked against the Rust libsai, and no Rust program drives the op scripts through a
//! libsai's C ABI. A run that asks for the driver part without it stops with exit 2 saying so (README
//! "Parity"); it never skips the part. Its golden stays (tests/golden/driver-cpp.txt, checked by tests/golden.rs).
//!
//! **Argument bytes**: every run has the byte tap (`bin/libparity-bytes.so`, `c/bytes_rt.c`) as
//! `SAI_TERALYNX_IFCS_TRACE`, so the stream holds an `@B` line per IFCS call with ALL the bytes of every struct
//! argument (padding and union bytes included) and of every attribute: the sections and oracle
//! comparisons compare them with the mock's lines, the driver part compares each script's `@B` lines
//! besides its op blocks. The mock compares fields, libifcs compares bytes (evidence
//! rust-h2/06-ifcs-key-padding.txt): this is what catches a padding byte the C++ had zero and the Rust has not.
//!
//! A difference is **allowed** (`tests/parity-allow.txt`, deliberate), **pending** (its area, or the
//! area a `tests/parity-pending.txt` entry names, is still pending: [`crate::areas`]),
//! **downstream** (after a pending difference in the same run: state the unported area left
//! behind), or **red**. `strict` (the G2 gate): only allowed differences pass. The golden's own integrity
//! (digests, inputs, the changed-golden negative controls) is checked by `tests/golden.rs`.
#![forbid(unsafe_code)]

use crate::areas::Areas;
use crate::checks::{self, Gate};
use crate::golden::{self, Golden, Norm, RefLines, RefUnit};
use crate::lists::{self, Pending};
use crate::oracle;
use crate::sha256;
use crate::stream::{self, LineDiff, Unit};
use sai_driver::compare::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The parts of a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    /// `test_ifcs`'s checks.
    Checks,
    /// `test_ifcs`'s IFCS calls per section.
    Sections,
    /// The frontend's op scripts.
    Driver,
    /// The oracle's JSON scripts.
    Oracle,
}

impl Part {
    /// Every part.
    pub const ALL: [Self; 4] = [Self::Checks, Self::Sections, Self::Driver, Self::Oracle];
    /// The parts a run has by default: those with a program since H5 (not the driver's: `Part::program`).
    pub const DEFAULT: [Self; 3] = [Self::Checks, Self::Sections, Self::Oracle];

    /// Its name on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Checks => "checks",
            Self::Sections => "sections",
            Self::Driver => "driver",
            Self::Oracle => "oracle",
        }
    }

    /// The part of that name.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }

    /// The fixtures program the part runs on the libsai, if it is one: the driver's (the `test_ifcs` parts run
    /// `Config::test_ifcs`, the oracle `Config::sai_script`).
    pub fn program(self) -> &'static str {
        match self {
            Self::Driver => "bin/tl-frontend-driver",
            Self::Checks | Self::Sections | Self::Oracle => "",
        }
    }
}

/// What to run.
#[derive(Clone, Debug)]
pub struct Config {
    /// The golden directory (`tests/golden`).
    pub golden: PathBuf,
    /// `build-fixtures.sh`'s output: `bin/`, `mock/libifcs.so`, `cfg/`.
    pub fixtures: PathBuf,
    /// Scratch directory (short: the diag socket's path lives under it).
    pub work: PathBuf,
    /// The Rust libsai (sai-ffi with the teralynx feature).
    pub rust_lib: PathBuf,
    /// The Rust `sai-script` program (crate sai-script, feature cli) the oracle part runs.
    pub sai_script: PathBuf,
    /// The Rust `test_ifcs` the checks and sections parts run: the test binary of
    /// crates/sai-backend-teralynx/tests/ifcs_e2e (beside it `golden/` and `CHECK-MAP.txt`, or none: then its
    /// own source tree's).
    pub test_ifcs: PathBuf,
    /// `crates/sai-backend-teralynx/tests/areas.txt`.
    pub areas: PathBuf,
    /// `crates/sai-backend-teralynx/tests/test_ifcs-expected.txt` (the pending areas).
    pub expected: PathBuf,
    /// `tests/parity-allow.txt`.
    pub allow: PathBuf,
    /// `tests/parity-pending.txt`.
    pub pending: PathBuf,
    /// sai-driver's `tests/driver-scripts` (the Teralynx ones are 01-09, moved from the frontend's tests).
    pub driver_scripts: PathBuf,
    /// The oracle's `scripts` (`None`: the part is skipped).
    pub oracle_scripts: Option<PathBuf>,
    /// The G2 gate: only allow-listed differences pass.
    pub strict: bool,
    /// The parts to run.
    pub parts: BTreeSet<Part>,
    /// Only these units (script stems, section names) in the detail (all are still run).
    pub only: Vec<String>,
    /// Negative control: the Rust side's tap marks stream lines containing this text.
    pub mutate: Option<String>,
    /// Per program run.
    pub timeout: Duration,
}

/// The outcome.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// The report text.
    pub report: String,
    /// The red items (empty: green).
    pub red: Vec<String>,
    /// The gate value (passed, total) of `test_ifcs` on Rust, if run.
    pub gate: Option<(u32, u32)>,
}

/// How one differing unit is judged.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Judged {
    Allowed(usize),
    Pending(String),
    Downstream,
    Red,
}

impl Judged {
    fn text(&self, allow: &[Entry]) -> String {
        match self {
            Self::Allowed(i) => format!("allowed: {}", allow.get(*i).map_or("", |e| e.reason.as_str())),
            Self::Pending(a) => format!("pending {a}"),
            Self::Downstream => "downstream (after a pending difference)".to_owned(),
            Self::Red => "RED".to_owned(),
        }
    }
}

struct Ctx<'a> {
    cfg: &'a Config,
    areas: Areas,
    pending_areas: BTreeSet<String>,
    allow: Vec<Entry>,
    allow_used: Vec<usize>,
    pend: Vec<Pending>,
    pend_used: Vec<usize>,
    /// The golden C++ side: the reference.
    golden: Golden,
    /// What differs between machines in a text (the golden's streams are normalised).
    norm: Norm,
    out: String,
    detail: String,
    red: Vec<String>,
}

fn read(p: &Path) -> Result<String, String> {
    fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn fresh(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

struct Ran {
    code: Option<i32>,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

impl Ran {
    fn status(&self) -> String {
        if self.timed_out {
            "timed out".to_owned()
        } else {
            self.code
                .map_or_else(|| "killed by a signal".to_owned(), |c| format!("exit {c}"))
        }
    }
}

impl Ctx<'_> {
    fn line(&mut self, s: impl AsRef<str>) {
        self.out.push_str(s.as_ref());
        self.out.push('\n');
    }

    fn redden(&mut self, s: String) {
        self.line(format!("   RED: {s}"));
        self.red.push(s);
    }

    /// A line of `details.txt` (every differing unit).
    fn detail(&mut self, s: impl AsRef<str>) {
        self.detail.push_str(s.as_ref());
        self.detail.push('\n');
    }

    /// The unit's details also go into the report (`--only`).
    fn shown(&self, unit: &str) -> bool {
        self.cfg.only.iter().any(|o| o == unit)
    }

    fn fixture(&self, rel: &str) -> PathBuf {
        self.cfg.fixtures.join(rel)
    }

    /// The byte tap of the runs.
    fn bytes_tap(&self) -> PathBuf {
        self.fixture("bin/libparity-bytes.so")
    }

    fn out_dir(&self, rel: &str) -> Result<PathBuf, String> {
        let d = self.cfg.work.join("out").join(rel);
        fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        Ok(d)
    }

    /// The libsai directory: the library, its sonames, the shim next to it.
    fn lib_dir(&self) -> Result<PathBuf, String> {
        let d = self.cfg.work.join("lib-rust");
        fresh(&d)?;
        copy(&self.cfg.rust_lib, &d.join("libsai.so.1.0.0"))?;
        std::os::unix::fs::symlink("libsai.so.1.0.0", d.join("libsai.so.1")).map_err(|e| e.to_string())?;
        std::os::unix::fs::symlink("libsai.so.1", d.join("libsai.so")).map_err(|e| e.to_string())?;
        copy(
            &self.fixture("bin/libsai-teralynx-shim.so"),
            &d.join("libsai-teralynx-shim.so"),
        )?;
        Ok(d)
    }

    /// Runs a program on the libsai with the tap.
    fn run(&self, prog: &Path, args: &[&str], cwd: &Path, tap: &Path, env: &[(&str, String)]) -> Result<Ran, String> {
        let lib = self.cfg.work.join("lib-rust");
        let (so, se) = (cwd.join(".stdout"), cwd.join(".stderr"));
        let file = |p: &Path| fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()));
        let mut c = Command::new(prog);
        c.args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(file(&so)?)
            .stderr(file(&se)?)
            .env("LD_LIBRARY_PATH", &lib)
            .env("LD_PRELOAD", self.fixture("bin/libparity-tap.so"))
            .env("PARITY_TAP_FILE", tap)
            .env("PARITY_MOCK", self.fixture("mock/libifcs.so"))
            .env_remove("PARITY_TAP_MUTATE")
            .env_remove("SAI_SCRIPT_TOOLS")
            .env_remove("SAI_LIBSAI")
            // a program whose profile is fixed (test_ifcs's) gets the byte tap's key through this
            .env(
                "PARITY_PROFILE_ADD",
                format!("SAI_TERALYNX_IFCS_TRACE={}", self.bytes_tap().display()),
            );
        if let Some(m) = &self.cfg.mutate {
            c.env("PARITY_TAP_MUTATE", m);
        }
        for (k, v) in env {
            c.env(k, v);
        }
        let mut child = c.spawn().map_err(|e| format!("{}: {e}", prog.display()))?;
        let deadline = Instant::now() + self.cfg.timeout;
        let mut timed_out = false;
        let status = loop {
            if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
                break Some(st);
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let text = |p: &Path| {
            fs::read(p)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default()
        };
        Ok(Ran {
            code: status.and_then(|s| s.code()),
            timed_out,
            stdout: text(&so),
            stderr: text(&se),
        })
    }

    /// The program of a part, or the error that says it is not there (a part is never skipped).
    fn program(&self, part: Part) -> Result<PathBuf, String> {
        let (p, what) = match part {
            Part::Checks | Part::Sections => (self.cfg.test_ifcs.clone(), "--test-ifcs (the ifcs_e2e test binary)"),
            Part::Oracle => (self.cfg.sai_script.clone(), "--sai-script"),
            Part::Driver => (
                self.fixture(part.program()),
                "a frontend driver in the fixtures: nothing builds it since H5 (it was the C++ frontend_driver.cpp, deleted; README \"Parity\"); run --parts without driver",
            ),
        };
        if p.is_file() {
            Ok(p)
        } else {
            Err(format!(
                "the {} part needs {} ({}), which is not there",
                part.name(),
                what,
                p.display()
            ))
        }
    }

    /// Judges a differing unit (`own`: the unit's areas).
    fn judge(&mut self, scope: &str, unit: &str, own: &[String], after_pending: bool) -> Judged {
        if let Some(i) = self.allow.iter().position(|e| e.covers(scope, unit)) {
            self.allow_used.push(i);
            return Judged::Allowed(i);
        }
        if self.cfg.strict {
            return Judged::Red;
        }
        if let Some(i) = self
            .pend
            .iter()
            .position(|p| p.entry.covers(scope, unit) && self.pending_areas.contains(&p.area))
        {
            self.pend_used.push(i);
            return Judged::Pending(self.pend.get(i).map(|p| p.area.clone()).unwrap_or_default());
        }
        if let Some(a) = own.iter().find(|a| self.pending_areas.contains(*a)) {
            return Judged::Pending(a.clone());
        }
        if after_pending {
            return Judged::Downstream;
        }
        Judged::Red
    }
}

/// The context of a run: the lists, the golden.
fn context(cfg: &Config) -> Result<Ctx<'_>, String> {
    let areas = Areas::parse(&read(&cfg.areas)?)?;
    let expected = read(&cfg.expected)?;
    let pending_areas = areas.pending(&expected);
    let allow = lists::parse_allow(&read(&cfg.allow)?).map_err(|e| format!("{}: {e}", cfg.allow.display()))?;
    let pend = lists::parse_pending(&read(&cfg.pending)?).map_err(|e| format!("{}: {e}", cfg.pending.display()))?;
    for p in &pend {
        if areas.get(&p.area).is_none() {
            return Err(format!(
                "{}: line {}: unknown area {}",
                cfg.pending.display(),
                p.entry.line,
                p.area
            ));
        }
    }
    fs::create_dir_all(&cfg.work).map_err(|e| format!("{}: {e}", cfg.work.display()))?;
    let golden = Golden::load(&cfg.golden)?;
    Ok(Ctx {
        cfg,
        areas,
        pending_areas,
        allow,
        allow_used: Vec::new(),
        pend,
        pend_used: Vec::new(),
        golden,
        norm: Norm::new(&cfg.work, &cfg.fixtures),
        out: String::new(),
        detail: String::new(),
        red: Vec::new(),
    })
}

/// Runs everything the configuration asks for.
pub fn run(cfg: &Config) -> Result<Outcome, String> {
    let mut x = context(cfg)?;
    // every part asked for has its program, before anything runs
    for p in &cfg.parts {
        x.program(*p)?;
    }
    header(&mut x);
    x.lib_dir()?;
    let mut gate = None;
    if cfg.parts.contains(&Part::Checks) || cfg.parts.contains(&Part::Sections) {
        gate = test_ifcs(&mut x)?;
    }
    if cfg.parts.contains(&Part::Driver) {
        driver(&mut x)?;
    }
    if cfg.parts.contains(&Part::Oracle) {
        oracle_part(&mut x)?;
    }
    let report = finish(&mut x)?;
    Ok(Outcome {
        report,
        red: x.red,
        gate,
    })
}

/// The report's first lines: the libraries, the pending areas, the negative control.
fn header(x: &mut Ctx<'_>) {
    let cfg = x.cfg;
    x.line(format!(
        "sai-teralynx parity ({} mode): reference the golden C++ side {}, candidate Rust {}",
        if cfg.strict { "strict: the G2 gate" } else { "progress" },
        cfg.golden.display(),
        cfg.rust_lib.display()
    ));
    x.line(format!(
        "parts: {}",
        cfg.parts.iter().map(|p| p.name()).collect::<Vec<_>>().join(", ")
    ));
    let pa: Vec<String> = x
        .pending_areas
        .iter()
        .map(|a| format!("{a} ({})", x.areas.get(a).map_or("?", |r| r.group.as_str())))
        .collect();
    x.line(format!(
        "pending areas (FAIL lines left in {}): {}",
        cfg.expected
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        if pa.is_empty() {
            "none".to_owned()
        } else {
            pa.join(", ")
        }
    ));
    if let Some(m) = &cfg.mutate {
        x.line(format!(
            "NEGATIVE CONTROL: the Rust side's stream lines containing '{m}' are marked [mutated]"
        ));
    }
}

/// Stale list entries, the verdict; writes report.txt and details.txt; the report.
fn finish(x: &mut Ctx<'_>) -> Result<String, String> {
    let cfg = x.cfg;
    // stale list entries
    for (i, e) in x.allow.clone().iter().enumerate() {
        if !x.allow_used.contains(&i) {
            x.redden(format!(
                "stale parity-allow.txt entry (line {}): {}\t{}\t{}",
                e.line,
                e.script,
                e.pattern.as_str(),
                e.reason
            ));
        }
    }
    let stale: Vec<String> = x
        .pend
        .iter()
        .enumerate()
        .filter(|(i, p)| !x.pend_used.contains(i) && !cfg.strict && !x.pending_areas.contains(&p.area))
        .map(|(_, p)| {
            format!(
                "line {} ({}, area {} no longer pending)",
                p.entry.line, p.entry.script, p.area
            )
        })
        .collect();
    if !stale.is_empty() {
        x.line(format!("== parity-pending.txt entries to drop: {}", stale.join("; ")));
    }
    let verdict = if x.red.is_empty() {
        "GREEN".to_owned()
    } else {
        format!("RED ({} items)", x.red.len())
    };
    x.line(format!("== verdict: {verdict}"));
    for r in x.red.clone().iter().take(40) {
        x.line(format!("   {r}"));
    }
    if x.red.len() > 40 {
        x.line(format!("   ... {} more (details.txt)", x.red.len() - 40));
    }
    for r in x.red.clone() {
        x.detail(format!("RED {r}"));
    }
    let dir = x.out_dir("")?;
    x.line(format!(
        "(every differing unit: {}; the runs' outputs beside it)",
        dir.join("details.txt").display()
    ));
    let report = std::mem::take(&mut x.out);
    fs::write(dir.join("report.txt"), &report).map_err(|e| e.to_string())?;
    fs::write(dir.join("details.txt"), &x.detail).map_err(|e| e.to_string())?;
    Ok(report)
}

// ============================================================== test_ifcs: checks + sections

struct IfcsRun {
    tap: String,
    counts: Vec<checks::Count>,
    stdout: String,
    stderr: String,
    status: String,
}

fn run_test_ifcs(x: &Ctx<'_>) -> Result<IfcsRun, String> {
    let dir = x.cfg.work.join("t");
    fresh(&dir)?;
    let ct = dir.join("ct");
    if ct.join("diag.sock").as_os_str().len() >= 100 {
        return Err(format!(
            "the work directory {} is too long: test_ifcs's diag socket {} needs < 100 bytes (use a shorter --work)",
            x.cfg.work.display(),
            ct.join("diag.sock").display()
        ));
    }
    let tap = dir.join("tap.txt");
    let counts = dir.join("counts.txt");
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let prog = x.program(Part::Checks)?;
    // the test binary's golden and check map: beside it when installed (a Nix build), else its source tree's
    let mut env = vec![
        ("IFCS_E2E_REQUIRE", "1".to_owned()),
        ("IFCS_E2E_LIBSAI", s(&x.cfg.work.join("lib-rust/libsai.so.1.0.0"))),
        ("IFCS_E2E_MOCK", s(&x.fixture("mock/libifcs.so"))),
        // the C++ test's scratch directory (its second argument): the same paths in the stream
        ("IFCS_E2E_DIR", s(&ct)),
        ("TEST_IFCS_DUMP", "1".to_owned()),
        ("PARITY_COUNTS_FILE", s(&counts)),
    ];
    if let Some(d) = prog.parent().filter(|d| d.join("CHECK-MAP.txt").is_file()) {
        env.push(("IFCS_E2E_FIXTURES", s(d)));
    }
    let r = x.run(
        &prog,
        &["--exact", "ifcs_e2e", "--nocapture", "--test-threads=1"],
        &dir,
        &tap,
        &env,
    )?;
    let keep = x.out_dir("test_ifcs/rust")?;
    let tap_text = fs::read_to_string(&tap).unwrap_or_default();
    let counts_text = fs::read_to_string(&counts).unwrap_or_default();
    for (name, text) in [
        ("tap.txt", &tap_text),
        ("counts.txt", &counts_text),
        ("stdout.txt", &r.stdout),
        ("stderr.txt", &r.stderr),
    ] {
        fs::write(keep.join(name), text).map_err(|e| e.to_string())?;
    }
    let status = r.status();
    Ok(IfcsRun {
        tap: tap_text,
        counts: checks::parse_counts(&counts_text)?,
        status,
        stdout: r.stdout,
        stderr: r.stderr,
    })
}

/// The C++ side of `test_ifcs`: what the golden holds.
struct IfcsRef {
    status: String,
    stdout_last: String,
    counts: Vec<checks::Count>,
    bytes: usize,
    sections: Vec<RefUnit>,
}

/// The byte tap's line prefix (`c/bytes_rt.c`).
const BYTES: &str = "@B ";

/// The byte tap's lines of a stream.
fn bytes_lines(stream: &str) -> Vec<String> {
    stream
        .lines()
        .filter(|l| l.starts_with(BYTES))
        .map(str::to_owned)
        .collect()
}

#[allow(clippy::too_many_lines, reason = "one part's report, in order")]
fn test_ifcs(x: &mut Ctx<'_>) -> Result<Option<(u32, u32)>, String> {
    let mut rust = run_test_ifcs(x)?;
    // the reference: the golden's record of the C++ run (tests/golden)
    let reference = IfcsRef {
        status: x.golden.ifcs.status.clone(),
        stdout_last: x.golden.ifcs.stdout_last.clone(),
        counts: x.golden.ifcs.counts.clone(),
        bytes: x.golden.ifcs.bytes,
        sections: x.golden.ifcs.sections.clone(),
    };
    // the golden's streams are normalised (work and fixtures directories, store paths)
    rust.tap = x.norm.text(&rust.tap);
    let cpp_total: u32 = reference.counts.iter().map(|c| c.checks).sum();
    let cpp_failed: u32 = reference.counts.iter().map(|c| c.failed).sum();
    x.line(format!(
        "== test_ifcs on Rust, the C++ side as the golden captured: C++ reference {cpp_total} checks, {cpp_failed} failed ({}; '{}'); Rust: {}",
        reference.status, reference.stdout_last, rust.status
    ));
    if cpp_failed > 0 || reference.counts.iter().any(|c| c.incomplete) || reference.status != "exit 0" {
        x.redden(format!(
            "the golden's C++ reference does not pass test_ifcs ({} failed, {})",
            cpp_failed, reference.status
        ));
    }
    x.line("   tap check: made at capture (the capture refused a stream that did not end as the mock's own log)");
    let (cb, rb) = (reference.bytes, bytes_lines(&rust.tap).len());
    x.line(format!(
        "   byte tap: {cb} C++ / {rb} Rust @B lines (every IFCS call's argument bytes, compared in the sections)"
    ));
    if cb == 0 || rb == 0 {
        x.redden("byte tap: a side's stream has no @B lines (SAI_TERALYNX_IFCS_TRACE not taken)".to_owned());
    }
    // how the Rust test_ifcs ended: its summary line as the C++ test's last stdout line (the test binary's
    // own lines are around it, its "test ifcs_e2e ... " may begin the same line)
    let rust_last = rust
        .stdout
        .lines()
        .rev()
        .find_map(|l| l.find("test_ifcs: ").and_then(|i| l.get(i..)))
        .unwrap_or("(none)");
    if rust.status != reference.status || rust_last != reference.stdout_last {
        x.redden(format!(
            "test_ifcs on Rust ended {} '{rust_last}', the golden's C++ run {} '{}'",
            rust.status, reference.status, reference.stdout_last
        ));
    }
    let gate = Gate::new(&reference.counts, &rust.counts);
    let sec_areas = |x: &Ctx<'_>, name: &str| x.areas.of_section(name).to_vec();
    if x.cfg.parts.contains(&Part::Checks) {
        x.line(format!(
            "== checks: Rust passes {} of {} (C++ reference); {} not reached{}",
            gate.passed(),
            gate.total(),
            gate.rows.iter().map(|r| r.not_run).sum::<u32>(),
            gate.stopped_at()
                .map_or_else(String::new, |s| format!(" (the run stopped in {s})"))
        ));
        x.line("   section          ref  pass  fail  not-run  areas");
        let mut after_pending = false;
        for r in gate.rows.clone() {
            let own = sec_areas(x, &r.name);
            let pending = own.iter().any(|a| x.pending_areas.contains(a));
            x.line(format!(
                "   {:<15} {:>4} {:>5} {:>5} {:>8}  {}{}{}",
                r.name,
                r.reference,
                r.passed,
                r.failed,
                r.not_run,
                own.join(","),
                if pending { " (pending)" } else { "" },
                if r.died {
                    "  died here"
                } else if r.extra > 0 {
                    "  more checks than the reference"
                } else {
                    ""
                }
            ));
            if r.not_run > 0 && !pending && !after_pending && !x.cfg.strict {
                x.redden(format!(
                    "test_ifcs section {}: {} checks not reached on Rust",
                    r.name, r.not_run
                ));
            }
            if x.cfg.strict && r.not_run > 0 {
                x.redden(format!(
                    "test_ifcs section {}: {} checks not reached on Rust",
                    r.name, r.not_run
                ));
            }
            after_pending |= pending && (r.failed > 0 || r.not_run > 0 || r.died);
        }
        // every Rust FAIL line with its area
        let fails = checks::fails(&rust.stderr);
        let mut per_area: BTreeMap<String, (usize, &'static str)> = BTreeMap::new();
        let mut red_fails: Vec<String> = Vec::new();
        for f in &fails {
            let area = x.areas.of_fail(f).unwrap_or_else(|| "unmapped".to_owned());
            let judged = x.judge("test_ifcs-check", &f.text, std::slice::from_ref(&area), false);
            let kind = match judged {
                Judged::Allowed(_) => "allowed",
                Judged::Pending(_) => "pending",
                Judged::Downstream | Judged::Red => {
                    red_fails.push(format!("{} [area {area}]", f.text));
                    "RED"
                }
            };
            per_area.entry(area).or_insert((0, kind)).0 += 1;
        }
        let list: Vec<String> = per_area.iter().map(|(a, (n, k))| format!("{a} {n} ({k})")).collect();
        x.line(format!(
            "   Rust FAIL lines: {} by area: {}",
            fails.len(),
            if list.is_empty() {
                "none".to_owned()
            } else {
                list.join(", ")
            }
        ));
        for (i, f) in red_fails.iter().enumerate() {
            let line = format!("test_ifcs check failed on Rust in an area that is not pending: {f}");
            x.detail(&line);
            if i < 10 {
                x.line(format!("   RED: {line}"));
            }
            x.red.push(line);
        }
        if red_fails.len() > 10 {
            x.line(format!(
                "   ... {} more such FAIL lines (details.txt)",
                red_fails.len() - 10
            ));
        }
    }
    if x.cfg.parts.contains(&Part::Sections) {
        let (cs, rs) = (reference.sections.clone(), stream::sections(&rust.tap));
        let mut stats: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut rows = Vec::new();
        let mut after_pending = false;
        let keep = x.out_dir("sections")?;
        for c in &cs {
            let r = rs.iter().find(|r| r.name == c.name);
            let empty = Unit {
                name: c.name.clone(),
                lines: Vec::new(),
                died: false,
            };
            let r = r.unwrap_or(&empty);
            let d = golden::diff_ref(&c.lines, &r.lines);
            let own = sec_areas(x, &c.name);
            let (kind, judged) = if d.raw_equal {
                ("equal", None)
            } else {
                let j = x.judge("test_ifcs", &c.name, &own, after_pending);
                after_pending |= matches!(j, Judged::Pending(_));
                fs::write(keep.join(format!("{}.rust.txt", c.name)), r.lines.join("\n") + "\n")
                    .map_err(|e| e.to_string())?;
                (
                    match j {
                        Judged::Allowed(_) => "allowed",
                        Judged::Pending(_) => "pending",
                        Judged::Downstream => "downstream",
                        Judged::Red => "RED",
                    },
                    Some(j),
                )
            };
            *stats.entry(kind).or_insert(0) += 1;
            rows.push((c.name.clone(), own, d, judged, rs.iter().all(|u| u.name != c.name)));
        }
        let summary: Vec<String> = stats.iter().map(|(k, n)| format!("{n} {k}")).collect();
        x.line(format!(
            "== sections (test_ifcs IFCS calls per area, the tap): {} sections: {}",
            cs.len(),
            summary.join(", ")
        ));
        for (name, own, d, judged, missing) in rows {
            let verdict = judged
                .as_ref()
                .map_or_else(String::new, |j| format!("  [{}]", j.text(&x.allow)));
            x.line(format!(
                "   {:<15} {}{}{}",
                name,
                if missing {
                    "NOT REACHED on Rust; ".to_owned()
                } else {
                    String::new()
                },
                stream::describe(&d),
                verdict
            ));
            if judged == Some(Judged::Red) {
                let hint = format!(
                    "the Rust side's lines: {}/{name}.rust.txt; the golden keeps the digest and the head",
                    keep.display()
                );
                x.red.push(format!(
                    "test_ifcs section {name} ({}) differs: {} ({hint})",
                    own.join(","),
                    stream::describe(&d)
                ));
            }
        }
    }
    Ok(Some((gate.passed(), gate.total())))
}

// ============================================================== driver scripts

fn ops_files(dir: &Path, ext: &str) -> Result<Vec<PathBuf>, String> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    v.sort();
    Ok(v)
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The profile lines the driver scripts get first: the Teralynx keys on the mock (8 ports, the
/// frontend's fake default).
fn driver_profile(x: &Ctx<'_>, dir: &Path) -> String {
    let key = |k: &str, v: &Path| format!("profile {k}={}\n", v.display());
    [
        key("SAI_TERALYNX_LIBIFCS", &x.fixture("mock/libifcs.so")),
        key("SAI_TERALYNX_PORT_MAP", &x.fixture("cfg/ports8.map")),
        key("SAI_INIT_CONFIG_FILE", &x.fixture("cfg/init.yaml")),
        key("SAI_TERALYNX_RUN_DIR", &dir.join("run")),
        // the host interfaces as netdevs the library creates (test_ifcs's mode): the build host has no
        // Ethernet<n> to adopt (the box's default), so adopt would only compare the refusal
        "profile SAI_TERALYNX_NETDEV=create\n".to_owned(),
        key("SAI_TERALYNX_IFCS_TRACE", &x.bytes_tap()),
    ]
    .concat()
}

fn run_driver(x: &Ctx<'_>, script: &Path) -> Result<(String, String, String), String> {
    let dir = x.cfg.work.join("d");
    fresh(&dir)?;
    let text = driver_profile(x, &dir) + &read(script)?;
    fs::write(dir.join("script.ops"), text).map_err(|e| e.to_string())?;
    let r = x.run(
        &x.program(Part::Driver)?,
        &["script.ops"],
        &dir,
        &dir.join("tap.txt"),
        &[],
    )?;
    let keep = x.out_dir("driver")?;
    let name = stem(script);
    fs::write(keep.join(format!("{name}.rust.txt")), &r.stdout).map_err(|e| e.to_string())?;
    fs::write(keep.join(format!("{name}.rust.stderr.txt")), &r.stderr).map_err(|e| e.to_string())?;
    let tap = fs::read_to_string(dir.join("tap.txt")).unwrap_or_default();
    fs::write(keep.join(format!("{name}.rust.tap.txt")), &tap).map_err(|e| e.to_string())?;
    let status = r.status();
    Ok((r.stdout, status, tap))
}

/// The driver part's byte check of one script: the `@B` lines of the two taps, whole script.
/// Answers (C++ lines, differ, red); a difference goes into `lines` (and, red, into the red list).
fn driver_bytes(
    x: &mut Ctx<'_>,
    name: &str,
    cb: &RefLines,
    rust_tap: &str,
    lines: &mut Vec<(String, bool)>,
) -> (usize, bool, bool) {
    let rb = bytes_lines(rust_tap);
    if cb.is_empty() {
        x.redden(format!("driver {name}: the byte tap wrote no @B lines"));
        return (0, false, true);
    }
    let d = golden::diff_ref(cb, &rb);
    if d.raw_equal {
        return (cb.len(), false, false);
    }
    let j = x.judge(name, "@B", &[], false);
    let text = format!("argument bytes ({} @B lines): {}", cb.len(), stream::describe(&d));
    let red = j == Judged::Red;
    if red {
        x.red.push(format!("driver {name}: {text}"));
    }
    lines.push((format!("      {text}  [{}]", j.text(&x.allow)), red));
    (cb.len(), true, red)
}

#[allow(clippy::too_many_lines, reason = "one part's report, in order")]
fn driver(x: &mut Ctx<'_>) -> Result<(), String> {
    // the Teralynx scripts are 01-09; the directory (sai-driver's tests/driver-scripts) also holds the
    // AS7772's external-PHY and MACsec scripts (10, 11), which the Teralynx backend does not run
    let scripts = teralynx_driver_scripts(&x.cfg.driver_scripts)?;
    if scripts.is_empty() {
        return Err(format!("no *.ops in {}", x.cfg.driver_scripts.display()));
    }
    let mut total = 0;
    let mut differing = 0;
    let mut rows = Vec::new();
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    let (mut bytes_total, mut bytes_differ) = (0usize, 0usize);
    // its scripts are the directory's, byte for byte (the inputs are digested)
    let (have, want): (Vec<String>, Vec<String>) = (
        x.golden.driver.scripts.iter().map(|s| s.name.clone()).collect(),
        scripts.iter().map(|p| stem(p)).collect(),
    );
    if have != want {
        return Err(format!(
            "the golden's driver scripts {have:?} are not the directory's {want:?}"
        ));
    }
    for script in &scripts {
        let name = stem(script);
        let gold = x
            .golden
            .driver
            .script(&name)
            .cloned()
            .ok_or_else(|| format!("driver {name}: not in the golden"))?;
        let (rust, rust_st, rust_tap) = run_driver(x, script)?;
        // the reference: its status, its answers compared op by op, its byte-tap lines
        let cb = RefLines::Digest(gold.bytes.clone());
        if gold.status != "exit 0" {
            x.redden(format!(
                "driver {name}: the golden's C++ reference run ended {}",
                gold.status
            ));
        }
        let input = sha256::hex(read(script)?.as_bytes());
        if input != gold.input {
            x.redden(format!(
                "driver {name}: the script's sha256 {input} is not the golden's input digest {}",
                gold.input
            ));
        }
        // the golden's streams are normalised (work and fixtures directories, store paths)
        let (rust, rust_tap) = (x.norm.text(&rust), x.norm.text(&rust_tap));
        let c = golden::compare_blocks(&name, &gold.blocks, &rust)
            .map_err(|e| format!("driver {name}: {e} (Rust run: {rust_st})"))?;
        total += c.blocks;
        differing += c.diffs.len();
        let mut after_pending = false;
        let mut lines = Vec::new();
        let mut red_here = 0;
        for d in &c.diffs {
            let own: Vec<String> = x.areas.of_op(&d.op).map(str::to_owned).into_iter().collect();
            let j = x.judge(&name, &d.op, &own, after_pending);
            after_pending |= matches!(j, Judged::Pending(_));
            *kinds
                .entry(match j {
                    Judged::Allowed(_) => "allowed",
                    Judged::Pending(_) => "pending",
                    Judged::Downstream => "downstream",
                    Judged::Red => "RED",
                })
                .or_insert(0) += 1;
            if j == Judged::Red {
                red_here += 1;
                x.red.push(format!(
                    "driver {name} block {}: > {} [area {}]: {}",
                    d.index,
                    d.op,
                    own.join(","),
                    d.first
                ));
            }
            lines.push((
                format!(
                    "      block {:>3} > {}  [{}; {}]: {}",
                    d.index,
                    d.op,
                    own.join(","),
                    j.text(&x.allow),
                    d.first
                ),
                j == Judged::Red,
            ));
        }
        let (n, differs, red) = driver_bytes(x, &name, &cb, &rust_tap, &mut lines);
        bytes_total += n;
        bytes_differ += usize::from(differs);
        red_here += usize::from(red);
        rows.push((name, c.blocks, c.diffs.len(), red_here, rust_st, lines));
    }
    let k: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
    x.line(format!(
        "== driver scripts (driver-scripts through {} on the Rust libsai, the C++ side as the golden captured): {} scripts, {total} ops, {} identical, {differing} differ{}",
        x.program(Part::Driver)?.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        scripts.len(),
        total - differing,
        if k.is_empty() { String::new() } else { format!(" ({})", k.join(", ")) }
    ));
    x.line(format!(
        "   argument bytes (@B lines of the byte tap, whole script): {bytes_total} lines, {} scripts equal, {bytes_differ} differ",
        scripts.len() - bytes_differ
    ));
    for (name, blocks, diffs, red_here, rust_st, lines) in rows {
        x.line(format!(
            "   {name}: {blocks} ops, {} identical, {diffs} differ{} (Rust run: {rust_st})",
            blocks - diffs,
            if red_here > 0 {
                format!(", {red_here} RED")
            } else {
                String::new()
            }
        ));
        x.detail(format!("driver {name}:"));
        for (l, red) in lines {
            if red || x.shown(&name) {
                x.line(&l);
            }
            x.detail(l);
        }
    }
    Ok(())
}

// ============================================================== oracle scripts

fn oracle_profile(x: &Ctx<'_>, dir: &Path) -> String {
    let bytes = format!("SAI_TERALYNX_IFCS_TRACE={}\n", x.bytes_tap().display());
    bytes
        + &format!(
            "SAI_OPENBCM_BACKEND=openbcm\nSAI_TERALYNX_NETDEV=create\nSAI_TERALYNX_LIBIFCS={}\nSAI_TERALYNX_PORT_MAP={}\nSAI_INIT_CONFIG_FILE={}\nSAI_TERALYNX_RUN_DIR={}\nSAI_OPENBCM_DIAG_SOCKET={}\n",
            x.fixture("mock/libifcs.so").display(),
            x.fixture("cfg/ports32.map").display(),
            x.fixture("cfg/init.yaml").display(),
            dir.join("run").display(),
            dir.join("diag.sock").display()
        )
}

struct OracleRun {
    stream: String,
    stdout: String,
    status: String,
}

fn run_oracle(x: &Ctx<'_>, prepped: &Path, name: &str) -> Result<OracleRun, String> {
    let dir = x.cfg.work.join("o");
    fresh(&dir)?;
    fs::write(dir.join("profile"), oracle_profile(x, &dir)).map_err(|e| e.to_string())?;
    let stream_file = dir.join("stream.txt");
    let script = prepped.to_string_lossy().into_owned();
    // the Rust sai-script in the C++ runner's own mode (crate sai-script: --cpp-names is equal to sai_script.cpp)
    let lib = x.cfg.work.join("lib-rust").join("libsai.so.1");
    let lib = lib.to_string_lossy().into_owned();
    let sai_script = x.cfg.sai_script.clone();
    let r = x.run(
        &sai_script,
        &["--cpp-names", "--libsai", &lib, "profile", &script],
        &dir,
        &stream_file,
        &[("IFCS_TRACE_FILE", stream_file.to_string_lossy().into_owned())],
    )?;
    let stream = fs::read_to_string(&stream_file).unwrap_or_default();
    let keep = x.out_dir("oracle")?;
    fs::write(keep.join(format!("{name}.rust.stream.txt")), &stream).map_err(|e| e.to_string())?;
    fs::write(keep.join(format!("{name}.rust.stdout.txt")), &r.stdout).map_err(|e| e.to_string())?;
    fs::write(keep.join(format!("{name}.rust.stderr.txt")), &r.stderr).map_err(|e| e.to_string())?;
    let status = r.status();
    Ok(OracleRun {
        stream,
        status,
        stdout: r.stdout,
    })
}

/// The script without its frame-sender steps (`oracle-prep`, a fixture), and what it dropped.
fn oracle_prep(x: &Ctx<'_>, script: &Path, name: &str) -> Result<(PathBuf, String), String> {
    let prepped = x.out_dir("oracle")?.join(format!("{name}.json"));
    let prep = Command::new(x.fixture("bin/oracle-prep"))
        .arg(script)
        .arg(&prepped)
        .output()
        .map_err(|e| format!("oracle-prep: {e}"))?;
    let prep_text = String::from_utf8_lossy(&prep.stdout).into_owned();
    if !prep.status.success() {
        return Err(format!("oracle-prep {name}: {}", String::from_utf8_lossy(&prep.stderr)));
    }
    Ok((prepped, prep_text.lines().last().unwrap_or("").to_owned()))
}

#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "one part's report, in order; c / r: the C++ and the Rust side, as everywhere here"
)]
fn oracle_part(x: &mut Ctx<'_>) -> Result<(), String> {
    let Some(dir) = x.cfg.oracle_scripts.clone() else {
        x.line("== oracle scripts: SKIPPED (no --oracle-scripts)");
        return Ok(());
    };
    let scripts = ops_files(&dir, "json")?;
    let mut equal = Vec::new();
    let mut rows = Vec::new();
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    // its scripts are the directory's
    let (have, want): (Vec<String>, Vec<String>) = (
        x.golden.oracle.scripts.iter().map(|s| s.name.clone()).collect(),
        scripts.iter().map(|p| stem(p)).collect(),
    );
    if have != want {
        return Err(format!(
            "the golden's oracle scripts {have:?} are not the directory's {want:?}"
        ));
    }
    for script in &scripts {
        let name = stem(script);
        let (prepped, skipped) = oracle_prep(x, script, &name)?;
        let g = x
            .golden
            .oracle
            .script(&name)
            .cloned()
            .ok_or_else(|| format!("oracle {name}: not in the golden"))?;
        let mut rust = run_oracle(x, &prepped, &name)?;
        // the reference: its steps, its answers, how it ended
        let input = sha256::hex(read(script)?.as_bytes());
        if input != g.input {
            x.redden(format!(
                "oracle {name}: the script's sha256 {input} is not the golden's input digest {}",
                g.input
            ));
        }
        if skipped != g.skipped {
            x.redden(format!(
                "oracle {name}: oracle-prep says '{skipped}', the golden was captured with '{}'",
                g.skipped
            ));
        }
        // the golden's streams are normalised (work and fixtures directories, store paths)
        rust.stream = x.norm.text(&rust.stream);
        rust.stdout = x.norm.text(&rust.stdout);
        let (cs, co, ce, cpp_status): (Vec<RefUnit>, Vec<String>, Vec<String>, String) =
            (g.steps.clone(), g.out.clone(), g.events.clone(), g.status.clone());
        let rs = oracle::steps(&rust.stream);
        let (ro, re) = oracle::stdout(&rust.stdout);
        let n = cs.len().max(rs.len());
        let own = x.areas.of_oracle(&name).to_vec();
        let scope = format!("oracle/{name}");
        let empty = RefLines::Full(Vec::new());
        let mut after_pending = false;
        let mut judged: Vec<(String, Judged, LineDiff)> = Vec::new();
        for i in 0..n {
            let (c, r) = (cs.get(i), rs.get(i));
            if matches!((c, r), (Some(c), Some(r)) if c.same(r)) {
                continue;
            }
            let unit = c.map(|u| &u.name).or(r.map(|u| &u.name)).cloned().unwrap_or_default();
            let step_own: Vec<String> = x
                .areas
                .of_step_op(oracle::step_op(&unit))
                .map_or_else(|| own.clone(), |a| vec![a.to_owned()]);
            let d = golden::diff_ref(c.map_or(&empty, |u| &u.lines), r.map_or(&[], |u| u.lines.as_slice()));
            let j = x.judge(&scope, &unit, &step_own, after_pending);
            after_pending |= matches!(j, Judged::Pending(_));
            judged.push((unit, j, d));
        }
        let out_d: LineDiff = stream::diff_lines(&co, &ro);
        if judged.is_empty() && (!out_d.raw_equal || ce != re) {
            // the IFCS calls agree, the answers do not
            let j = x.judge(&scope, "stdout", &own, false);
            judged.push(("stdout".to_owned(), j, out_d.clone()));
        }
        let (kind, detail) = if judged.is_empty() {
            equal.push(name.clone());
            ("equal", String::new())
        } else {
            let count = |k: fn(&Judged) -> bool| judged.iter().filter(|(_, j, _)| k(j)).count();
            let reds = count(|j| *j == Judged::Red);
            let kind = if reds > 0 {
                "RED"
            } else if count(|j| matches!(j, Judged::Pending(_))) > 0 {
                "pending"
            } else if count(|j| matches!(j, Judged::Allowed(_))) == judged.len() {
                "allowed"
            } else {
                "downstream"
            };
            let first = judged
                .first()
                .map(|(u, j, d)| format!("first: '{u}' [{}]: {}", j.text(&x.allow), stream::describe(d)))
                .unwrap_or_default();
            for (u, j, d) in &judged {
                x.detail(format!(
                    "oracle {name} step '{u}' [{}]: {}",
                    j.text(&x.allow),
                    stream::describe(d)
                ));
                if *j == Judged::Red {
                    x.red.push(format!(
                        "oracle {name} step '{u}' [{}]: {}",
                        own.join(","),
                        stream::describe(d)
                    ));
                }
            }
            let detail = format!(
                "{} of {n} steps differ ({reds} RED); {first}; stdout {}; events C++ {} / Rust {}",
                judged.len(),
                if out_d.raw_equal {
                    "equal".to_owned()
                } else {
                    stream::describe(&out_d)
                },
                ce.len(),
                re.len()
            );
            (kind, detail)
        };
        *kinds.entry(kind).or_insert(0) += 1;
        rows.push((name, own, kind, detail, skipped, cpp_status, rust.status, cs.len()));
    }
    let k: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
    x.line(format!(
        "== oracle scripts (oracle/scripts through the Rust sai-script --cpp-names on the Rust libsai, the C++ side as the golden captured, SAI steps only): {} scripts: {}",
        scripts.len(),
        k.join(", ")
    ));
    x.line(format!(
        "   equal today: {}",
        if equal.is_empty() {
            "none".to_owned()
        } else {
            equal.join(", ")
        }
    ));
    for (name, own, kind, detail, skipped, cs, rs, steps) in rows {
        let _ = kind;
        let mut l = format!(
            "   {name} [{}] {steps} steps; {skipped}; C++ {cs}, Rust {rs}",
            own.join(",")
        );
        if !detail.is_empty() {
            let _ = write!(l, ": {detail}");
        }
        x.line(l);
    }
    Ok(())
}

/// The Teralynx driver scripts (01-09) of a directory that also holds the AS7772's (10, 11).
fn teralynx_driver_scripts(dir: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(ops_files(dir, "ops")?
        .into_iter()
        .filter(|p| stem(p).starts_with('0'))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(strict: bool) -> Config {
        Config {
            golden: PathBuf::new(),
            fixtures: PathBuf::new(),
            work: PathBuf::new(),
            rust_lib: PathBuf::new(),
            sai_script: PathBuf::new(),
            test_ifcs: PathBuf::new(),
            areas: PathBuf::new(),
            expected: PathBuf::new(),
            allow: PathBuf::new(),
            pending: PathBuf::new(),
            driver_scripts: PathBuf::new(),
            oracle_scripts: None,
            strict,
            parts: BTreeSet::new(),
            only: Vec::new(),
            mutate: None,
            timeout: Duration::from_secs(1),
        }
    }

    fn ctx(cfg: &Config) -> Ctx<'_> {
        let areas =
            Areas::parse("area switch g1 a.cpp\narea l2 l2-lag b.cpp\narea qos q c.cpp\ncheck t_l2.cpp l2\n").unwrap();
        let pending_areas = areas.pending("FAIL t_l2.cpp:1: x\n");
        Ctx {
            cfg,
            areas,
            pending_areas,
            allow: lists::parse_allow("01-x\t^keep\tdeliberate: the reason\n").unwrap(),
            allow_used: Vec::new(),
            pend: lists::parse_pending("01-x\t^switch\tl2\tthe aging time\n01-x\t^get\tqos\tnot pending\n").unwrap(),
            pend_used: Vec::new(),
            golden: Golden {
                ifcs: golden::IfcsGolden {
                    status: String::new(),
                    stdout_last: String::new(),
                    bytes: 0,
                    counts: Vec::new(),
                    sections: Vec::new(),
                },
                driver: golden::DriverGolden::default(),
                oracle: golden::OracleGolden::default(),
            },
            norm: Norm::default(),
            out: String::new(),
            detail: String::new(),
            red: Vec::new(),
        }
    }

    #[test]
    fn a_difference_is_allowed_pending_downstream_or_red() {
        let c = cfg(false);
        let mut x = ctx(&c);
        let sw = ["switch".to_owned()];
        let l2 = ["l2".to_owned()];
        assert_eq!(x.judge("01-x", "keep this", &sw, false), Judged::Allowed(0));
        assert_eq!(
            x.judge("01-x", "switch A=1", &sw, false),
            Judged::Pending("l2".to_owned())
        );
        assert_eq!(x.pend_used, [0]);
        // an entry for an area that is not pending does not excuse anything
        assert_eq!(x.judge("01-x", "get switch", &sw, false), Judged::Red);
        assert_eq!(
            x.judge("02-y", "create vlan", &l2, false),
            Judged::Pending("l2".to_owned())
        );
        assert_eq!(x.judge("02-y", "get port", &sw, true), Judged::Downstream);
        assert_eq!(x.judge("02-y", "get port", &sw, false), Judged::Red);
    }

    #[test]
    fn strict_mode_lets_only_allowed_differences_pass() {
        let c = cfg(true);
        let mut x = ctx(&c);
        let l2 = ["l2".to_owned()];
        assert_eq!(x.judge("01-x", "keep", &l2, false), Judged::Allowed(0));
        assert_eq!(x.judge("01-x", "switch", &l2, true), Judged::Red);
        assert_eq!(x.judge("02-y", "create vlan", &l2, true), Judged::Red);
    }

    #[test]
    fn parts_by_name() {
        assert_eq!(Part::parse("oracle"), Some(Part::Oracle));
        assert_eq!(Part::parse("x"), None);
        assert!(Part::ALL.iter().all(|p| Part::parse(p.name()) == Some(*p)));
    }

    #[test]
    fn a_part_without_its_program_is_an_error_not_a_skip() {
        let dir = std::env::temp_dir().join(format!("parity-fx-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        let mut c = cfg(true);
        c.fixtures = dir.clone();
        c.test_ifcs = dir.join("ifcs_e2e");
        c.sai_script = dir.join("sai-script");
        let x = ctx(&c);
        for (part, needs) in [
            (Part::Driver, "tl-frontend-driver"),
            (Part::Checks, "--test-ifcs"),
            (Part::Sections, "--test-ifcs"),
            (Part::Oracle, "--sai-script"),
        ] {
            let e = x.program(part).unwrap_err();
            assert!(
                e.contains(&format!("the {} part needs", part.name())) && e.contains(needs),
                "{e}"
            );
        }
        for f in ["bin/tl-frontend-driver", "ifcs_e2e", "sai-script"] {
            fs::write(dir.join(f), "").unwrap();
        }
        assert!(Part::ALL.iter().all(|p| x.program(*p).is_ok()));
        assert!(!Part::DEFAULT.contains(&Part::Driver));
        let _ = fs::remove_dir_all(&dir);
    }
}
