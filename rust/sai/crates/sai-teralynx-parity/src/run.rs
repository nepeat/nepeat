//! The runs and the verdict: the same programs (built by `build-fixtures.sh`) on the C++ libsai
//! (the reference) and on the Rust libsai, each on the same mock libifcs, in the same directories
//! (so every path in the output is the same), then compared part by part:
//!
//! - **checks**: `test_ifcs` (unmodified, `c/sections.cpp` beside it) on both; the gate value "Rust
//!   passes N of M", every Rust FAIL line with its area, the checks not reached;
//! - **sections**: the same runs' IFCS call streams (`TEST_IFCS_DUMP=1`, the tap), section by section;
//! - **driver**: the frozen frontend's op scripts through `frontend_driver.cpp` (unmodified, `c/tl_port.cpp`
//!   beside it) on both, op block by op block (status, values, the op's IFCS lines, notifications);
//! - **oracle**: the oracle's JSON scripts (their SAI steps: `c/oracle_prep.cpp`) through its runner
//!   `sai-script` (unmodified) on both, step by step.
//!
//! **Argument bytes** (`bytes`, on unless `--no-bytes`): every run has the byte tap
//! (`bin/libparity-bytes.so`, `c/bytes_rt.c`) as `SAI_TERALYNX_IFCS_TRACE`, so the stream holds an
//! `@B` line per IFCS call with ALL the bytes of every struct argument (padding and union bytes
//! included) and of every attribute: the sections and oracle comparisons compare them with the mock's
//! lines, the driver part compares each script's `@B` lines besides its op blocks. The mock compares
//! fields, libifcs compares bytes (evidence rust-h2/06-ifcs-key-padding.txt): this is what catches
//! a padding byte the C++ has zero and the Rust has not.
//!
//! A difference is **allowed** (`tests/parity-allow.txt`, deliberate), **pending** (its area, or the
//! area a `tests/parity-pending.txt` entry names, is still pending: [`crate::areas`]),
//! **downstream** (after a pending difference in the same run: state the unported area left
//! behind), or **red**. `strict` (the G2 gate): only allowed differences pass. The C++ reference must
//! pass every check and give the same output twice (`repeat_reference`).
#![forbid(unsafe_code)]

use crate::areas::Areas;
use crate::checks::{self, Gate};
use crate::golden::{self, Golden, Norm, RefLines, RefUnit};
use crate::lists::{self, Pending};
use crate::oracle;
use crate::sha256;
use crate::stream::{self, LineDiff, Unit};
use sai_driver::compare::{self, Entry};
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
}

/// What the run does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The C++ libsai and the Rust libsai, compared (the live parity).
    Live,
    /// The Rust libsai against the golden C++ side (`Config::golden`; no C++ needed).
    Golden,
    /// The live C++ side rendered again and required equal to the golden (the proof that the golden
    /// is what the C++ gives; no Rust needed).
    CheckGolden,
    /// The live C++ side captured into `Config::golden` (no header: the capture's header is written
    /// by hand over it, README "Parity").
    Capture,
}

/// What to run.
#[derive(Clone, Debug)]
pub struct Config {
    /// What the run does.
    pub mode: Mode,
    /// The golden directory ([`Mode::Golden`], [`Mode::CheckGolden`]: read; [`Mode::Capture`]: written).
    pub golden: Option<PathBuf>,
    /// `build-fixtures.sh`'s output: `bin/`, `mock/libifcs.so`, `cfg/`.
    pub fixtures: PathBuf,
    /// Scratch directory (short: the diag socket's path lives under it).
    pub work: PathBuf,
    /// The C++ libsai (`libsai.so.1.0.0` of ai_re/nix/packages/sai-teralynx; empty in [`Mode::Golden`]).
    pub cpp_lib: PathBuf,
    /// The Rust libsai (sai-ffi with the teralynx feature; empty in the modes without Rust).
    pub rust_lib: PathBuf,
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
    /// Run the C++ reference twice and require equal outputs.
    pub repeat_reference: bool,
    /// The byte tap in every run (`bin/libparity-bytes.so` as `SAI_TERALYNX_IFCS_TRACE`).
    pub bytes: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Cpp,
    Rust,
}

impl Side {
    fn name(self) -> &'static str {
        match self {
            Self::Cpp => "cpp",
            Self::Rust => "rust",
        }
    }
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
    /// The golden C++ side, when it is the reference ([`Mode::Golden`]).
    golden: Option<Golden>,
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

    /// The byte tap of the runs, if on.
    fn bytes_tap(&self) -> Option<PathBuf> {
        self.cfg.bytes.then(|| self.fixture("bin/libparity-bytes.so"))
    }

    fn out_dir(&self, rel: &str) -> Result<PathBuf, String> {
        let d = self.cfg.work.join("out").join(rel);
        fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        Ok(d)
    }

    /// The libsai directory of a side: the library, its sonames, the C shim next to it.
    fn lib_dir(&self, side: Side) -> Result<PathBuf, String> {
        let d = self.cfg.work.join(format!("lib-{}", side.name()));
        fresh(&d)?;
        let lib = match side {
            Side::Cpp => &self.cfg.cpp_lib,
            Side::Rust => &self.cfg.rust_lib,
        };
        copy(lib, &d.join("libsai.so.1.0.0"))?;
        std::os::unix::fs::symlink("libsai.so.1.0.0", d.join("libsai.so.1")).map_err(|e| e.to_string())?;
        std::os::unix::fs::symlink("libsai.so.1", d.join("libsai.so")).map_err(|e| e.to_string())?;
        copy(
            &self.fixture("bin/libsai-teralynx-shim.so"),
            &d.join("libsai-teralynx-shim.so"),
        )?;
        Ok(d)
    }

    /// Runs a fixture program on a side's libsai with the tap.
    fn run(
        &self,
        side: Side,
        prog: &str,
        args: &[&str],
        cwd: &Path,
        tap: &Path,
        env: &[(&str, String)],
    ) -> Result<Ran, String> {
        let lib = self.cfg.work.join(format!("lib-{}", side.name()));
        let (so, se) = (cwd.join(".stdout"), cwd.join(".stderr"));
        let file = |p: &Path| fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()));
        let mut c = Command::new(self.fixture(&format!("bin/{prog}")));
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
            .env_remove("PARITY_PROFILE_ADD");
        if let Some(t) = self.bytes_tap() {
            // test_ifcs's fixed profile gets the key through sections.cpp's sai_api_initialize wrap
            c.env("PARITY_PROFILE_ADD", format!("SAI_TERALYNX_IFCS_TRACE={}", t.display()));
        }
        if let (Side::Rust, Some(m)) = (side, &self.cfg.mutate) {
            c.env("PARITY_TAP_MUTATE", m);
        }
        for (k, v) in env {
            c.env(k, v);
        }
        let mut child = c.spawn().map_err(|e| format!("{prog}: {e}"))?;
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
    let golden = match (cfg.mode, &cfg.golden) {
        (Mode::Golden, Some(dir)) => {
            if !cfg.bytes {
                return Err(
                    "--golden needs the byte tap (the golden's streams hold the @B lines): no --no-bytes".to_owned(),
                );
            }
            Some(Golden::load(dir)?)
        }
        (Mode::Golden, None) => return Err("--golden DIR is required".to_owned()),
        _ => None,
    };
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
    match cfg.mode {
        Mode::Capture | Mode::CheckGolden => return golden_cpp(&mut x),
        Mode::Live | Mode::Golden => {}
    }
    header(&mut x);
    if cfg.mode == Mode::Live {
        x.lib_dir(Side::Cpp)?;
    }
    x.lib_dir(Side::Rust)?;
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
        "sai-teralynx parity ({} mode): reference {}, candidate Rust {}",
        if cfg.strict { "strict: the G2 gate" } else { "progress" },
        match &cfg.golden {
            Some(g) if cfg.mode == Mode::Golden => format!("the golden C++ side {}", g.display()),
            _ => format!("C++ {}", cfg.cpp_lib.display()),
        },
        cfg.rust_lib.display()
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

fn run_test_ifcs(x: &Ctx<'_>, side: Side, tag: &str) -> Result<IfcsRun, String> {
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
    let mock = x.fixture("mock/libifcs.so");
    let (mock_s, ct_s) = (mock.to_string_lossy().into_owned(), ct.to_string_lossy().into_owned());
    let r = x.run(
        side,
        "test_ifcs-sections",
        &[&mock_s, &ct_s],
        &dir,
        &tap,
        &[
            ("TEST_IFCS_DUMP", "1".to_owned()),
            ("PARITY_COUNTS_FILE", counts.to_string_lossy().into_owned()),
        ],
    )?;
    let keep = x.out_dir(&format!("test_ifcs/{tag}"))?;
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

/// The C++ side of `test_ifcs`: from the live run or from the golden.
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

/// The tap's check: the final `TEST_IFCS_DUMP` (the mock's own buffer since its last clear) is the
/// tail of the stream before `switch_remove`: the lines in common at the end.
fn dump_tail(run: &IfcsRun) -> usize {
    let stream: Vec<&str> = run
        .tap
        .lines()
        .take_while(|l| *l != "@@ begin switch_remove")
        .filter(|l| !l.starts_with("@@ ") && !l.starts_with(BYTES))
        .collect();
    let err: Vec<&str> = run
        .stderr
        .lines()
        .take_while(|l| *l != "--- mock log at failure ---")
        .collect();
    stream
        .iter()
        .rev()
        .zip(err.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
}

#[allow(clippy::too_many_lines, reason = "one part's report, in order")]
fn test_ifcs(x: &mut Ctx<'_>) -> Result<Option<(u32, u32)>, String> {
    // the reference: the live C++ run, or the golden (captured from it, tests/golden)
    let cpp = if x.golden.is_some() {
        None
    } else {
        Some(run_test_ifcs(x, Side::Cpp, "cpp")?)
    };
    let mut rust = run_test_ifcs(x, Side::Rust, "rust")?;
    let reference = match (&cpp, &x.golden) {
        (Some(c), _) => IfcsRef {
            status: c.status.clone(),
            stdout_last: c.stdout.lines().last().unwrap_or("").to_owned(),
            counts: c.counts.clone(),
            bytes: bytes_lines(&c.tap).len(),
            sections: stream::sections(&c.tap).iter().map(RefUnit::live).collect(),
        },
        (None, Some(g)) => IfcsRef {
            status: g.ifcs.status.clone(),
            stdout_last: g.ifcs.stdout_last.clone(),
            counts: g.ifcs.counts.clone(),
            bytes: g.ifcs.bytes,
            sections: g.ifcs.sections.clone(),
        },
        (None, None) => return Err("test_ifcs: neither a C++ run nor a golden".to_owned()),
    };
    if cpp.is_none() {
        // the golden's streams are normalised (work and fixtures directories, store paths)
        rust.tap = x.norm.text(&rust.tap);
    }
    let cpp_total: u32 = reference.counts.iter().map(|c| c.checks).sum();
    let cpp_failed: u32 = reference.counts.iter().map(|c| c.failed).sum();
    x.line(format!(
        "== test_ifcs (unmodified) on {}: C++ reference {} checks, {} failed ({}; '{}'); Rust: {}",
        if cpp.is_some() {
            "both"
        } else {
            "Rust, the C++ side as the golden captured"
        },
        cpp_total,
        cpp_failed,
        reference.status,
        reference.stdout_last,
        rust.status
    ));
    if cpp_failed > 0 || reference.counts.iter().any(|c| c.incomplete) || reference.status != "exit 0" {
        x.redden(format!(
            "the C++ reference does not pass test_ifcs ({} failed, {})",
            cpp_failed, reference.status
        ));
    }
    if let Some(c) = &cpp {
        let tail = dump_tail(c);
        x.line(format!(
            "   tap check: the C++ run's final TEST_IFCS_DUMP equals the stream's tail ({tail} lines in common)"
        ));
        if tail == 0 {
            x.redden("tap check: the stream does not end as the mock's own log (the tap misses lines)".to_owned());
        }
    } else {
        x.line("   tap check: made at capture (--capture refuses a stream that does not end as the mock's own log)");
    }
    if x.cfg.bytes {
        let (cb, rb) = (reference.bytes, bytes_lines(&rust.tap).len());
        x.line(format!(
            "   byte tap: {cb} C++ / {rb} Rust @B lines (every IFCS call's argument bytes, compared in the sections)"
        ));
        if cb == 0 || rb == 0 {
            x.redden("byte tap: a side's stream has no @B lines (SAI_TERALYNX_IFCS_TRACE not taken)".to_owned());
        }
    }
    if let (true, Some(c)) = (x.cfg.repeat_reference, &cpp) {
        let again = run_test_ifcs(x, Side::Cpp, "cpp-again")?;
        if again.tap != c.tap || again.counts != c.counts {
            x.redden(
                "the C++ reference's test_ifcs run is not deterministic (stream or counts differ between two runs)"
                    .to_owned(),
            );
        } else {
            x.line("   reference repeatable: a second C++ run gave the same stream and counts");
        }
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
                // a golden keeps no lines to write (the head and the digest: tests/golden)
                if let RefLines::Full(lines) = &c.lines {
                    fs::write(keep.join(format!("{}.cpp.txt", c.name)), lines.join("\n") + "\n")
                        .map_err(|e| e.to_string())?;
                }
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
            "== sections (test_ifcs IFCS calls per area, TEST_IFCS_DUMP=1 + tap): {} sections: {}",
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
                let hint = if x.golden.is_some() {
                    format!(
                        "the Rust side's lines: {}/{name}.rust.txt; the golden keeps the digest and the head",
                        keep.display()
                    )
                } else {
                    format!(
                        "diff -u {}/{name}.cpp.txt {}/{name}.rust.txt",
                        keep.display(),
                        keep.display()
                    )
                };
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

impl IfcsRun {
    fn code_bad(&self) -> bool {
        self.status != "exit 0"
    }
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
        x.bytes_tap()
            .map_or_else(String::new, |t| key("SAI_TERALYNX_IFCS_TRACE", &t)),
    ]
    .concat()
}

fn run_driver(x: &Ctx<'_>, side: Side, script: &Path, tag: &str) -> Result<(String, String, String), String> {
    let dir = x.cfg.work.join("d");
    fresh(&dir)?;
    let text = driver_profile(x, &dir) + &read(script)?;
    fs::write(dir.join("script.ops"), text).map_err(|e| e.to_string())?;
    let r = x.run(
        side,
        "tl-frontend-driver",
        &["script.ops"],
        &dir,
        &dir.join("tap.txt"),
        &[],
    )?;
    let keep = x.out_dir("driver")?;
    let name = stem(script);
    fs::write(keep.join(format!("{name}.{tag}.txt")), &r.stdout).map_err(|e| e.to_string())?;
    fs::write(keep.join(format!("{name}.{tag}.stderr.txt")), &r.stderr).map_err(|e| e.to_string())?;
    let tap = fs::read_to_string(dir.join("tap.txt")).unwrap_or_default();
    fs::write(keep.join(format!("{name}.{tag}.tap.txt")), &tap).map_err(|e| e.to_string())?;
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
    // against a golden: its scripts are the directory's, byte for byte (the inputs are digested)
    if let Some(g) = &x.golden {
        let (have, want): (Vec<String>, Vec<String>) = (
            g.driver.scripts.iter().map(|s| s.name.clone()).collect(),
            scripts.iter().map(|p| stem(p)).collect(),
        );
        if have != want {
            return Err(format!(
                "the golden's driver scripts {have:?} are not the directory's {want:?}"
            ));
        }
    }
    for script in &scripts {
        let name = stem(script);
        let gold = x.golden.as_ref().and_then(|g| g.driver.script(&name)).cloned();
        let live = if gold.is_some() {
            None
        } else {
            Some(run_driver(x, Side::Cpp, script, "cpp")?)
        };
        let (rust, rust_st, rust_tap) = run_driver(x, Side::Rust, script, "rust")?;
        // the reference: its status, its answers compared op by op, its byte-tap lines
        let (cpp_st, cb) = match (&live, &gold) {
            (Some((_, st, tap)), _) => (st.clone(), RefLines::Full(bytes_lines(tap))),
            (None, Some(g)) => (g.status.clone(), RefLines::Digest(g.bytes.clone())),
            (None, None) => return Err(format!("driver {name}: neither a C++ run nor a golden")),
        };
        if cpp_st != "exit 0" {
            x.redden(format!("driver {name}: the C++ reference run ended {cpp_st}"));
        }
        if let Some(g) = &gold {
            let input = sha256::hex(read(script)?.as_bytes());
            if input != g.input {
                x.redden(format!(
                    "driver {name}: the script's sha256 {input} is not the golden's input digest {}",
                    g.input
                ));
            }
        }
        if let (true, Some((cpp, _, cpp_tap))) = (x.cfg.repeat_reference, &live) {
            let (again, _, again_tap) = run_driver(x, Side::Cpp, script, "cpp-again")?;
            if again != *cpp || bytes_lines(&again_tap) != bytes_lines(cpp_tap) {
                x.redden(format!(
                    "driver {name}: the C++ reference is not deterministic (two runs differ)"
                ));
            }
        }
        let (rust, rust_tap) = if gold.is_some() {
            (x.norm.text(&rust), x.norm.text(&rust_tap))
        } else {
            (rust, rust_tap)
        };
        let c = match (&live, &gold) {
            (Some((cpp, _, _)), _) => compare::compare(&name, cpp, &rust).map(|c| golden::RefComparison {
                blocks: c.blocks,
                diffs: c
                    .diffs
                    .iter()
                    .map(|d| golden::RefDiff {
                        index: d.index,
                        op: d.op.clone(),
                        first: d.first_difference(),
                    })
                    .collect(),
            }),
            (None, Some(g)) => golden::compare_blocks(&name, &g.blocks, &rust),
            (None, None) => Err("no reference".to_owned()),
        }
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
        if x.cfg.bytes {
            let (n, differs, red) = driver_bytes(x, &name, &cb, &rust_tap, &mut lines);
            bytes_total += n;
            bytes_differ += usize::from(differs);
            red_here += usize::from(red);
        }
        rows.push((name, c.blocks, c.diffs.len(), red_here, rust_st, lines));
    }
    let k: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
    x.line(format!(
        "== driver scripts (frontend driver-scripts through frontend_driver.cpp {}): {} scripts, {total} ops, {} identical, {differing} differ{}",
        if x.golden.is_some() { "on the Rust libsai, the C++ side as the golden captured" } else { "on both libsais" },
        scripts.len(),
        total - differing,
        if k.is_empty() { String::new() } else { format!(" ({})", k.join(", ")) }
    ));
    if x.cfg.bytes {
        x.line(format!(
            "   argument bytes (@B lines of the byte tap, whole script): {bytes_total} lines, {} scripts equal, {bytes_differ} differ",
            scripts.len() - bytes_differ
        ));
    }
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
    let bytes = x
        .bytes_tap()
        .map_or_else(String::new, |t| format!("SAI_TERALYNX_IFCS_TRACE={}\n", t.display()));
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

fn run_oracle(x: &Ctx<'_>, side: Side, prepped: &Path, name: &str, tag: &str) -> Result<OracleRun, String> {
    let dir = x.cfg.work.join("o");
    fresh(&dir)?;
    fs::write(dir.join("profile"), oracle_profile(x, &dir)).map_err(|e| e.to_string())?;
    let stream_file = dir.join("stream.txt");
    let script = prepped.to_string_lossy().into_owned();
    let r = x.run(
        side,
        "sai-script",
        &["profile", &script],
        &dir,
        &stream_file,
        &[("IFCS_TRACE_FILE", stream_file.to_string_lossy().into_owned())],
    )?;
    let stream = fs::read_to_string(&stream_file).unwrap_or_default();
    let keep = x.out_dir("oracle")?;
    fs::write(keep.join(format!("{name}.{tag}.stream.txt")), &stream).map_err(|e| e.to_string())?;
    fs::write(keep.join(format!("{name}.{tag}.stdout.txt")), &r.stdout).map_err(|e| e.to_string())?;
    fs::write(keep.join(format!("{name}.{tag}.stderr.txt")), &r.stderr).map_err(|e| e.to_string())?;
    let status = r.status();
    Ok(OracleRun {
        stream,
        status,
        stdout: r.stdout,
    })
}

/// The script without its frame-sender steps (`oracle-prep`, a fixture of both sides), and what it dropped.
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
    // against a golden: its scripts are the directory's
    if let Some(g) = &x.golden {
        let (have, want): (Vec<String>, Vec<String>) = (
            g.oracle.scripts.iter().map(|s| s.name.clone()).collect(),
            scripts.iter().map(|p| stem(p)).collect(),
        );
        if have != want {
            return Err(format!(
                "the golden's oracle scripts {have:?} are not the directory's {want:?}"
            ));
        }
    }
    for script in &scripts {
        let name = stem(script);
        let (prepped, skipped) = oracle_prep(x, script, &name)?;
        let gold = x.golden.as_ref().and_then(|g| g.oracle.script(&name)).cloned();
        let live = if gold.is_some() {
            None
        } else {
            Some(run_oracle(x, Side::Cpp, &prepped, &name, "cpp")?)
        };
        let mut rust = run_oracle(x, Side::Rust, &prepped, &name, "rust")?;
        if let (true, Some(cpp)) = (x.cfg.repeat_reference, &live) {
            let again = run_oracle(x, Side::Cpp, &prepped, &name, "cpp-again")?;
            if oracle::steps(&again.stream) != oracle::steps(&cpp.stream)
                || oracle::stdout(&again.stdout) != oracle::stdout(&cpp.stdout)
            {
                x.redden(format!(
                    "oracle {name}: the C++ reference is not deterministic (two runs differ)"
                ));
            }
        }
        // the reference: its steps, its answers, how it ended
        let (cs, co, ce, cpp_status): (Vec<RefUnit>, Vec<String>, Vec<String>, String) = match (&live, &gold) {
            (Some(cpp), _) => {
                let (co, ce) = oracle::stdout(&cpp.stdout);
                (
                    oracle::steps(&cpp.stream).iter().map(RefUnit::live).collect(),
                    co,
                    ce,
                    cpp.status.clone(),
                )
            }
            (None, Some(g)) => {
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
                (g.steps.clone(), g.out.clone(), g.events.clone(), g.status.clone())
            }
            (None, None) => return Err(format!("oracle {name}: neither a C++ run nor a golden")),
        };
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
        "== oracle scripts (oracle/scripts through sai-script {}, SAI steps only): {} scripts: {}",
        if x.golden.is_some() {
            "on the Rust libsai, the C++ side as the golden captured"
        } else {
            "on both libsais"
        },
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

// ============================================================== the golden: capture and check

/// The three golden files' records of one capture.
struct Rendered {
    ifcs: String,
    driver: String,
    oracle: String,
}

/// The Teralynx driver scripts (01-09) of a directory that also holds the AS7772's (10, 11).
fn teralynx_driver_scripts(dir: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(ops_files(dir, "ops")?
        .into_iter()
        .filter(|p| stem(p).starts_with('0'))
        .collect())
}

/// Runs the C++ side of every part once and renders what a golden keeps of it (normalised: [`Norm`]).
fn capture_once(x: &Ctx<'_>) -> Result<Rendered, String> {
    // test_ifcs: it must pass everything, and the tap must end as the mock's own log
    let r = run_test_ifcs(x, Side::Cpp, "cpp")?;
    let failed: u32 = r.counts.iter().map(|c| c.failed).sum();
    if failed > 0 || r.counts.iter().any(|c| c.incomplete) || r.code_bad() {
        return Err(format!(
            "the C++ reference does not pass test_ifcs ({failed} failed, {})",
            r.status
        ));
    }
    if dump_tail(&r) == 0 {
        return Err("tap check: the stream does not end as the mock's own log (the tap misses lines)".to_owned());
    }
    let tap = x.norm.text(&r.tap);
    let ifcs = golden::IfcsGolden::capture(
        &r.status,
        r.stdout.lines().last().unwrap_or(""),
        bytes_lines(&tap).len(),
        &r.counts,
        &stream::sections(&tap),
    )
    .render();
    // the driver scripts
    let mut drv = golden::DriverGolden::default();
    for script in teralynx_driver_scripts(&x.cfg.driver_scripts)? {
        let name = stem(&script);
        let (stdout, status, tap) = run_driver(x, Side::Cpp, &script, "cpp")?;
        if status != "exit 0" {
            return Err(format!("driver {name}: the C++ reference run ended {status}"));
        }
        drv.scripts.push(golden::DriverScript::capture(
            &name,
            &status,
            &sha256::hex(read(&script)?.as_bytes()),
            &x.norm.text(&stdout),
            &bytes_lines(&x.norm.text(&tap)),
        ));
    }
    // the oracle scripts
    let dir = x
        .cfg
        .oracle_scripts
        .clone()
        .ok_or_else(|| "the golden needs --oracle-scripts".to_owned())?;
    let mut orc = golden::OracleGolden::default();
    for script in ops_files(&dir, "json")? {
        let name = stem(&script);
        let (prepped, skipped) = oracle_prep(x, &script, &name)?;
        let run = run_oracle(x, Side::Cpp, &prepped, &name, "cpp")?;
        if !run.status.starts_with("exit ") {
            return Err(format!("oracle {name}: the C++ reference run ended {}", run.status));
        }
        let (out, events) = oracle::stdout(&x.norm.text(&run.stdout));
        orc.scripts.push(golden::OracleScript {
            name,
            status: run.status,
            input: sha256::hex(read(&script)?.as_bytes()),
            skipped,
            steps: oracle::steps(&x.norm.text(&run.stream))
                .iter()
                .map(RefUnit::kept)
                .collect(),
            out,
            events,
        });
    }
    Ok(Rendered {
        ifcs,
        driver: drv.render(),
        oracle: orc.render(),
    })
}

/// [`Mode::Capture`] and [`Mode::CheckGolden`]: the live C++ side, twice (it must be deterministic),
/// written as the golden files' records, or required equal to the golden's.
fn golden_cpp(x: &mut Ctx<'_>) -> Result<Outcome, String> {
    let cfg = x.cfg;
    let dir = cfg
        .golden
        .clone()
        .ok_or_else(|| "--capture / --check-golden DIR is required".to_owned())?;
    x.line(format!(
        "sai-teralynx parity golden ({}): C++ {}",
        if cfg.mode == Mode::Capture { "capture" } else { "check" },
        cfg.cpp_lib.display()
    ));
    x.lib_dir(Side::Cpp)?;
    let first = capture_once(x)?;
    if cfg.repeat_reference {
        let second = capture_once(x)?;
        for (part, a, b) in [
            ("test_ifcs", &first.ifcs, &second.ifcs),
            ("driver", &first.driver, &second.driver),
            ("oracle", &first.oracle, &second.oracle),
        ] {
            if let Some(d) = golden::first_line_difference(a, b) {
                return Err(format!("the C++ reference's {part} run is not deterministic: {d}"));
            }
        }
        x.line("   reference repeatable: a second C++ run rendered the same records");
    }
    let files = [
        (golden::IFCS_FILE, &first.ifcs),
        (golden::DRIVER_FILE, &first.driver),
        (golden::ORACLE_FILE, &first.oracle),
    ];
    if cfg.mode == Mode::Capture {
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for (f, text) in files {
            let p = dir.join(f);
            fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
            x.line(format!(
                "   wrote {} ({} records, {} bytes)",
                p.display(),
                text.lines().count(),
                text.len()
            ));
        }
    } else {
        for (f, text) in files {
            let want = golden::data_lines(&read(&dir.join(f))?);
            match golden::first_line_difference(&want, text) {
                None => x.line(format!(
                    "   {f}: the live C++ renders to the golden's {} records, byte for byte",
                    text.lines().count()
                )),
                Some(d) => x.redden(format!("{f}: the live C++ is not the golden: {d}")),
            }
        }
    }
    let verdict = if x.red.is_empty() {
        "GREEN".to_owned()
    } else {
        format!("RED ({} items)", x.red.len())
    };
    x.line(format!("== verdict: {verdict}"));
    Ok(Outcome {
        report: std::mem::take(&mut x.out),
        red: std::mem::take(&mut x.red),
        gate: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(strict: bool) -> Config {
        Config {
            mode: Mode::Live,
            golden: None,
            fixtures: PathBuf::new(),
            work: PathBuf::new(),
            cpp_lib: PathBuf::new(),
            rust_lib: PathBuf::new(),
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
            repeat_reference: false,
            bytes: false,
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
            golden: None,
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
    fn the_tap_check_counts_the_dump_at_the_streams_end() {
        let run = |tap: &str, stderr: &str| IfcsRun {
            tap: tap.to_owned(),
            counts: Vec::new(),
            stdout: String::new(),
            stderr: stderr.to_owned(),
            status: "exit 0".to_owned(),
        };
        let tap = "a\n@@ end main\nb\nc\n@@ begin switch_remove\nd\n";
        assert_eq!(dump_tail(&run(tap, "noise\nb\nc\n")), 2);
        assert_eq!(dump_tail(&run(tap, "c\nb\n")), 0);
        assert_eq!(dump_tail(&run(tap, "b\nc\n--- mock log at failure ---\nzzz\n")), 2);
    }

    #[test]
    fn parts_by_name() {
        assert_eq!(Part::parse("oracle"), Some(Part::Oracle));
        assert_eq!(Part::parse("x"), None);
        assert!(Part::ALL.iter().all(|p| Part::parse(p.name()) == Some(*p)));
    }
}
