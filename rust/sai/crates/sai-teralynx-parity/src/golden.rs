//! The golden C++ side of the parity harness (README "Parity", kind "golden"): what the C++ libsai
//! of the CS8210 (the C++ sai-teralynx package, deleted in H5 on 2026-11-27, last in the tree at commit
//! 767d9b0) gave on the mock libifcs, captured once (H5-0) and stored in `tests/golden/`, so the Rust side
//! is judged against it (`--golden DIR`). The goldens are frozen: never regenerated, never edited.
//!
//! The streams are big (the oracle's alone are about 98 MB), so a golden keeps per UNIT (a
//! `test_ifcs` section, a driver op block, an oracle step): the line count, the SHA-256 of its lines
//! (normalised: [`Norm`]) and its first [`HEAD`] lines, each cut to [`CAP`] characters, so a
//! difference can be named and located. The digest decides; the head only describes. Small things
//! are stored whole: the per-section check counts, the oracle runner's answers (its stdout without
//! times). Every golden file begins with a `#` header (the C / C++ files with their sha256, the
//! capture command, by design naming the deleted sources); the records below it are what
//! [`IfcsGolden::render`] and friends write back out (`tests/golden.rs` proves the parse is lossless).
#![forbid(unsafe_code)]

use crate::checks::Count;
use crate::sha256::Sha256;
use crate::stream::{self, LineDiff};
use regex::Regex;
use sai_driver::compare;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

/// Lines of a unit kept as text.
pub const HEAD: usize = 4;
/// Characters of a kept line (the digest covers the whole line).
pub const CAP: usize = 160;

/// The file of the `test_ifcs` part (checks and sections).
pub const IFCS_FILE: &str = "test_ifcs-cpp.txt";
/// The file of the driver part.
pub const DRIVER_FILE: &str = "driver-cpp.txt";
/// The file of the oracle part.
pub const ORACLE_FILE: &str = "oracle-cpp.txt";

static STORE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"/nix/store/[a-z0-9]{32}-[^/ ]*").ok());

/// Takes what differs between machines out of a text: the work directory, the fixtures directory and
/// nix store paths. Applied to the C++ side at capture (H5-0) and to the Rust side against the golden.
#[derive(Clone, Debug, Default)]
pub struct Norm {
    first: (String, &'static str),
    second: (String, &'static str),
}

impl Norm {
    /// The normaliser of a run.
    pub fn new(work: &Path, fixtures: &Path) -> Self {
        let (w, f) = (
            work.to_string_lossy().into_owned(),
            fixtures.to_string_lossy().into_owned(),
        );
        let (w, f) = ((w, "<work>"), (f, "<fixtures>"));
        // the longer path first, so a path inside the other is not cut in two
        if w.0.len() >= f.0.len() {
            Self { first: w, second: f }
        } else {
            Self { first: f, second: w }
        }
    }

    /// The text normalised.
    pub fn text(&self, s: &str) -> String {
        let mut t = s.to_owned();
        for (from, to) in [&self.first, &self.second] {
            if !from.is_empty() {
                t = t.replace(from.as_str(), to);
            }
        }
        match STORE.as_ref() {
            Some(re) => re.replace_all(&t, "<store>").into_owned(),
            None => t,
        }
    }
}

/// A line cut to [`CAP`] characters.
pub fn cap(line: &str) -> String {
    line.chars().take(CAP).collect()
}

/// What a golden keeps of a run of lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest {
    /// The number of lines.
    pub lines: usize,
    /// SHA-256 of the lines, each followed by a newline, in hex.
    pub sha: String,
    /// The first [`HEAD`] lines, cut by [`cap`].
    pub head: Vec<String>,
}

/// The digest of a run of lines.
pub fn digest(lines: &[String]) -> Digest {
    let mut h = Sha256::new();
    for l in lines {
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    Digest {
        lines: lines.len(),
        sha: h.finish_hex(),
        head: lines.iter().take(HEAD).map(|l| cap(l)).collect(),
    }
}

/// The reference side of one unit: its lines themselves (a unit without lines, a test's small expectation) or a
/// golden's digest.
#[derive(Clone, Debug)]
pub enum RefLines {
    /// The lines themselves.
    Full(Vec<String>),
    /// What a golden file keeps.
    Digest(Digest),
}

impl RefLines {
    /// The number of lines.
    pub fn len(&self) -> usize {
        match self {
            Self::Full(v) => v.len(),
            Self::Digest(d) => d.lines,
        }
    }

    /// No lines.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether `actual` is the same run of lines.
    pub fn matches(&self, actual: &[String]) -> bool {
        match self {
            Self::Full(v) => v == actual,
            Self::Digest(d) => {
                let a = digest(actual);
                a.lines == d.lines && a.sha == d.sha
            }
        }
    }
}

/// Compares the reference's lines with the Rust side's. Whole lines: [`stream::diff_lines`]. A
/// digest: the digests decide; the first difference is looked for in the stored head only.
pub fn diff_ref(r: &RefLines, actual: &[String]) -> LineDiff {
    match r {
        RefLines::Full(v) => stream::diff_lines(v, actual),
        RefLines::Digest(d) => {
            let raw_equal = r.matches(actual);
            let first = if raw_equal { None } else { first_in_head(d, actual) };
            LineDiff {
                counts: (d.lines, actual.len()),
                raw_equal,
                canonical_equal: raw_equal,
                first,
                only: (0, 0),
                reduced: true,
            }
        }
    }
}

type First = (usize, Option<String>, Option<String>);

fn first_in_head(d: &Digest, actual: &[String]) -> Option<First> {
    let w = actual.len().min(HEAD);
    (0..d.head.len().max(w)).find_map(|i| {
        let (g, a) = (d.head.get(i), actual.get(i).map(|l| cap(l)));
        (g.cloned() != a).then(|| (i, g.cloned(), a))
    })
}

/// One unit of a golden: a `test_ifcs` section, an oracle step.
#[derive(Clone, Debug)]
pub struct RefUnit {
    /// Its name.
    pub name: String,
    /// The program died inside it.
    pub died: bool,
    /// Its lines.
    pub lines: RefLines,
}

impl RefUnit {
    /// Whether the Rust unit is this one.
    pub fn same(&self, r: &stream::Unit) -> bool {
        self.name == r.name && self.lines.matches(&r.lines)
    }

    fn digest(&self) -> Digest {
        match &self.lines {
            RefLines::Digest(d) => d.clone(),
            RefLines::Full(v) => digest(v),
        }
    }
}

fn put_digest(s: &mut String, tag: &str, d: &Digest, died: Option<bool>, name: Option<&str>) {
    let _ = write!(s, "{tag}\t{}\t{}", d.lines, d.sha);
    if let Some(x) = died {
        let _ = write!(s, "\t{}", u8::from(x));
    }
    if let Some(n) = name {
        let _ = write!(s, "\t{n}");
    }
    s.push('\n');
    for h in &d.head {
        let _ = writeln!(s, "|{h}");
    }
}

fn put_unit(s: &mut String, tag: &str, u: &RefUnit, with_died: bool) {
    put_digest(s, tag, &u.digest(), with_died.then_some(u.died), Some(&u.name));
}

// ------------------------------------------------------------------ parsing

struct Rec {
    tag: String,
    rest: String,
    head: Vec<String>,
    line: usize,
}

fn recs(text: &str) -> Result<Vec<Rec>, String> {
    let mut v: Vec<Rec> = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(h) = l.strip_prefix('|') {
            v.last_mut()
                .ok_or_else(|| format!("line {}: a head line before any record", i + 1))?
                .head
                .push(h.to_owned());
            continue;
        }
        let (tag, rest) = l.split_once('\t').unwrap_or((l, ""));
        v.push(Rec {
            tag: tag.to_owned(),
            rest: rest.to_owned(),
            head: Vec::new(),
            line: i + 1,
        });
    }
    Ok(v)
}

fn fields(r: &Rec, n: usize) -> Result<Vec<&str>, String> {
    let f: Vec<&str> = r.rest.splitn(n, '\t').collect();
    if f.len() == n {
        Ok(f)
    } else {
        Err(format!("line {}: '{}' needs {n} tab-separated fields", r.line, r.tag))
    }
}

fn num(r: &Rec, s: &str) -> Result<usize, String> {
    s.parse().map_err(|_| format!("line {}: '{s}' is not a number", r.line))
}

fn sha(r: &Rec, s: &str) -> Result<String, String> {
    if s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        Ok(s.to_owned())
    } else {
        Err(format!("line {}: '{s}' is not a sha256", r.line))
    }
}

fn kept_digest(r: &Rec, lines: &str, sha_s: &str) -> Result<Digest, String> {
    let lines = num(r, lines)?;
    if r.head.len() != lines.min(HEAD) {
        return Err(format!(
            "line {}: {} head lines for {lines} lines (expected {})",
            r.line,
            r.head.len(),
            lines.min(HEAD)
        ));
    }
    Ok(Digest {
        lines,
        sha: sha(r, sha_s)?,
        head: r.head.clone(),
    })
}

fn flag(r: &Rec, s: &str) -> Result<bool, String> {
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(format!("line {}: '{s}' is not 0 or 1", r.line)),
    }
}

/// The records of a golden file as written: without comments and blank lines.
pub fn data_lines(text: &str) -> String {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .fold(String::new(), |mut s, l| {
            s.push_str(l);
            s.push('\n');
            s
        })
}

// ------------------------------------------------------------------ test_ifcs

/// The `test_ifcs` part: checks per section, the sections' IFCS calls.
#[derive(Clone, Debug)]
pub struct IfcsGolden {
    /// How the program ended (`exit 0`).
    pub status: String,
    /// The last line of its stdout.
    pub stdout_last: String,
    /// `@B` lines in the stream.
    pub bytes: usize,
    /// Checks per section.
    pub counts: Vec<Count>,
    /// The stream's sections.
    pub sections: Vec<RefUnit>,
}

impl IfcsGolden {
    /// The records.
    pub fn render(&self) -> String {
        let mut s = format!(
            "status\t{}\nstdout-last\t{}\nbytes\t{}\n",
            self.status, self.stdout_last, self.bytes
        );
        for c in &self.counts {
            let _ = writeln!(
                s,
                "count\t{}\t{}\t{}\t{}",
                c.name,
                c.checks,
                c.failed,
                u8::from(c.incomplete)
            );
        }
        for u in &self.sections {
            put_unit(&mut s, "section", u, true);
        }
        s
    }

    /// Reads the records.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (mut status, mut stdout_last, mut bytes) = (None, None, None);
        let (mut counts, mut sections) = (Vec::new(), Vec::new());
        for r in recs(text)? {
            match r.tag.as_str() {
                "status" => status = Some(r.rest.clone()),
                "stdout-last" => stdout_last = Some(r.rest.clone()),
                "bytes" => bytes = Some(num(&r, &r.rest)?),
                "count" => {
                    let f = fields(&r, 4)?;
                    let (name, checks, failed, inc) = (f.first(), f.get(1), f.get(2), f.get(3));
                    let (Some(name), Some(checks), Some(failed), Some(inc)) = (name, checks, failed, inc) else {
                        return Err(format!("line {}: count", r.line));
                    };
                    counts.push(Count {
                        name: (*name).to_owned(),
                        checks: u32::try_from(num(&r, checks)?).map_err(|e| e.to_string())?,
                        failed: u32::try_from(num(&r, failed)?).map_err(|e| e.to_string())?,
                        incomplete: flag(&r, inc)?,
                    });
                }
                "section" => {
                    let fs = fields(&r, 4)?;
                    let (Some(l), Some(h), Some(d), Some(n)) = (fs.first(), fs.get(1), fs.get(2), fs.get(3)) else {
                        return Err(format!("line {}: section", r.line));
                    };
                    sections.push(RefUnit {
                        name: (*n).to_owned(),
                        died: flag(&r, d)?,
                        lines: RefLines::Digest(kept_digest(&r, l, h)?),
                    });
                }
                other => return Err(format!("line {}: unknown record '{other}'", r.line)),
            }
        }
        Ok(Self {
            status: status.ok_or("no status record")?,
            stdout_last: stdout_last.ok_or("no stdout-last record")?,
            bytes: bytes.ok_or("no bytes record")?,
            counts,
            sections,
        })
    }
}

// ------------------------------------------------------------------ driver

/// One op block of a driver script's output.
#[derive(Clone, Debug)]
pub struct RefBlock {
    /// The op line without `> `.
    pub op: String,
    /// The block's lines, the op line included.
    pub d: Digest,
}

/// One driver script's output.
#[derive(Clone, Debug)]
pub struct DriverScript {
    /// The script stem.
    pub name: String,
    /// How the driver ended (`exit 0`).
    pub status: String,
    /// SHA-256 of the script file the driver was given (the input).
    pub input: String,
    /// The op blocks of its stdout.
    pub blocks: Vec<RefBlock>,
    /// The `@B` lines of the byte tap, the whole script.
    pub bytes: Digest,
}

/// The driver part.
#[derive(Clone, Debug, Default)]
pub struct DriverGolden {
    /// Its scripts, in order.
    pub scripts: Vec<DriverScript>,
}

impl DriverGolden {
    /// The records.
    pub fn render(&self) -> String {
        let mut s = String::new();
        for sc in &self.scripts {
            let _ = writeln!(
                s,
                "script\t{}\t{}\t{}\t{}",
                sc.name,
                sc.status,
                sc.input,
                sc.blocks.len()
            );
            for b in &sc.blocks {
                put_digest(&mut s, "block", &b.d, None, Some(&b.op));
            }
            put_digest(&mut s, "bytes", &sc.bytes, None, None);
        }
        s
    }

    /// Reads the records.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut out: Vec<DriverScript> = Vec::new();
        let mut want: Vec<usize> = Vec::new();
        for r in recs(text)? {
            match r.tag.as_str() {
                "script" => {
                    let f = fields(&r, 4)?;
                    let (Some(n), Some(st), Some(i), Some(b)) = (f.first(), f.get(1), f.get(2), f.get(3)) else {
                        return Err(format!("line {}: script", r.line));
                    };
                    want.push(num(&r, b)?);
                    out.push(DriverScript {
                        name: (*n).to_owned(),
                        status: (*st).to_owned(),
                        input: sha(&r, i)?,
                        blocks: Vec::new(),
                        bytes: Digest {
                            lines: 0,
                            sha: String::new(),
                            head: Vec::new(),
                        },
                    });
                }
                "block" => {
                    let f = fields(&r, 3)?;
                    let (Some(l), Some(h), Some(op)) = (f.first(), f.get(1), f.get(2)) else {
                        return Err(format!("line {}: block", r.line));
                    };
                    let d = kept_digest(&r, l, h)?;
                    out.last_mut()
                        .ok_or_else(|| format!("line {}: block before a script", r.line))?
                        .blocks
                        .push(RefBlock {
                            op: (*op).to_owned(),
                            d,
                        });
                }
                "bytes" => {
                    let f = fields(&r, 2)?;
                    let (Some(l), Some(h)) = (f.first(), f.get(1)) else {
                        return Err(format!("line {}: bytes", r.line));
                    };
                    out.last_mut()
                        .ok_or_else(|| format!("line {}: bytes before a script", r.line))?
                        .bytes = kept_digest(&r, l, h)?;
                }
                other => return Err(format!("line {}: unknown record '{other}'", r.line)),
            }
        }
        for (s, n) in out.iter().zip(want) {
            if s.blocks.len() != n {
                return Err(format!("script {}: {} blocks, header says {n}", s.name, s.blocks.len()));
            }
        }
        Ok(Self { scripts: out })
    }

    /// The script of that stem.
    pub fn script(&self, name: &str) -> Option<&DriverScript> {
        self.scripts.iter().find(|s| s.name == name)
    }
}

/// One differing op block against a golden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefDiff {
    /// The block's position (0-based).
    pub index: usize,
    /// The op line without `> `.
    pub op: String,
    /// The first line pair that differs ([`compare::Diff::first_difference`]'s form).
    pub first: String,
}

/// The result of [`compare_blocks`].
#[derive(Clone, Debug, Default)]
pub struct RefComparison {
    /// Blocks compared.
    pub blocks: usize,
    /// The differing ones.
    pub diffs: Vec<RefDiff>,
}

fn short(s: Option<&String>) -> String {
    let s = s.map_or("<none>", String::as_str);
    let cut: String = s.chars().take(70).collect();
    if cut.len() < s.len() { format!("{cut}...") } else { cut }
}

/// [`compare::compare`] against a golden's blocks: the echo lines and the count are a hard failure;
/// a block differs when its digest does.
pub fn compare_blocks(script: &str, golden: &[RefBlock], rust: &str) -> Result<RefComparison, String> {
    let b = compare::blocks(rust);
    for (i, (x, y)) in golden.iter().zip(&b).enumerate() {
        if x.op != y.op {
            return Err(format!(
                "{script}: block {i}: the echo lines differ: golden '> {}', rust '> {}'",
                x.op, y.op
            ));
        }
    }
    if golden.len() != b.len() {
        return Err(format!(
            "{script}: golden has {} op blocks, rust {}",
            golden.len(),
            b.len()
        ));
    }
    let diffs = golden
        .iter()
        .zip(&b)
        .enumerate()
        .filter(|(_, (x, y))| !RefLines::Digest(x.d.clone()).matches(&y.lines))
        .map(|(index, (x, y))| RefDiff {
            index,
            op: x.op.clone(),
            first: match first_in_head(&x.d, &y.lines) {
                Some((_, g, r)) => format!("cpp {} | rust {}", short(g.as_ref()), short(r.as_ref())).replace('\t', " "),
                None => format!(
                    "differs beyond the first {} lines (golden {} lines, sha256 {}...; Rust {} lines)",
                    HEAD,
                    x.d.lines,
                    x.d.sha.chars().take(12).collect::<String>(),
                    y.lines.len()
                ),
            },
        })
        .collect();
    Ok(RefComparison { blocks: b.len(), diffs })
}

// ------------------------------------------------------------------ oracle

/// One oracle script's run.
#[derive(Clone, Debug)]
pub struct OracleScript {
    /// The script stem.
    pub name: String,
    /// How the runner ended (`exit 0`).
    pub status: String,
    /// SHA-256 of the script file (the input).
    pub input: String,
    /// What oracle-prep said (the steps it dropped).
    pub skipped: String,
    /// The steps of the IFCS stream.
    pub steps: Vec<RefUnit>,
    /// The runner's stdout without times, events apart ([`crate::oracle::stdout`]).
    pub out: Vec<String>,
    /// Its `event` lines, sorted.
    pub events: Vec<String>,
}

/// The oracle part.
#[derive(Clone, Debug, Default)]
pub struct OracleGolden {
    /// Its scripts, in order.
    pub scripts: Vec<OracleScript>,
}

impl OracleGolden {
    /// The records.
    pub fn render(&self) -> String {
        let mut s = String::new();
        for sc in &self.scripts {
            let _ = writeln!(
                s,
                "script\t{}\t{}\t{}\t{}\t{}",
                sc.name,
                sc.status,
                sc.input,
                sc.steps.len(),
                sc.skipped
            );
            for u in &sc.steps {
                put_unit(&mut s, "step", u, false);
            }
            for l in &sc.out {
                let _ = writeln!(s, "O\t{l}");
            }
            for l in &sc.events {
                let _ = writeln!(s, "E\t{l}");
            }
        }
        s
    }

    /// Reads the records.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut out: Vec<OracleScript> = Vec::new();
        let mut want: Vec<usize> = Vec::new();
        for r in recs(text)? {
            let cur = |out: &mut Vec<OracleScript>| -> Result<usize, String> {
                out.len()
                    .checked_sub(1)
                    .ok_or_else(|| format!("line {}: '{}' before a script", r.line, r.tag))
            };
            match r.tag.as_str() {
                "script" => {
                    let f = fields(&r, 5)?;
                    let (Some(n), Some(st), Some(i), Some(k), Some(sk)) =
                        (f.first(), f.get(1), f.get(2), f.get(3), f.get(4))
                    else {
                        return Err(format!("line {}: script", r.line));
                    };
                    want.push(num(&r, k)?);
                    out.push(OracleScript {
                        name: (*n).to_owned(),
                        status: (*st).to_owned(),
                        input: sha(&r, i)?,
                        skipped: (*sk).to_owned(),
                        steps: Vec::new(),
                        out: Vec::new(),
                        events: Vec::new(),
                    });
                }
                "step" => {
                    let fs = fields(&r, 3)?;
                    let (Some(l), Some(h), Some(n)) = (fs.first(), fs.get(1), fs.get(2)) else {
                        return Err(format!("line {}: step", r.line));
                    };
                    let unit = RefUnit {
                        name: (*n).to_owned(),
                        died: false,
                        lines: RefLines::Digest(kept_digest(&r, l, h)?),
                    };
                    let at = cur(&mut out)?;
                    out.get_mut(at).ok_or("no script")?.steps.push(unit);
                }
                "O" | "E" => {
                    let i = cur(&mut out)?;
                    let sc = out.get_mut(i).ok_or("no script")?;
                    if r.tag == "O" {
                        sc.out.push(r.rest.clone());
                    } else {
                        sc.events.push(r.rest.clone());
                    }
                }
                other => return Err(format!("line {}: unknown record '{other}'", r.line)),
            }
        }
        for (s, n) in out.iter().zip(want) {
            if s.steps.len() != n {
                return Err(format!("script {}: {} steps, header says {n}", s.name, s.steps.len()));
            }
        }
        Ok(Self { scripts: out })
    }

    /// The script of that stem.
    pub fn script(&self, name: &str) -> Option<&OracleScript> {
        self.scripts.iter().find(|s| s.name == name)
    }
}

/// The three golden files of a directory.
#[derive(Clone, Debug)]
pub struct Golden {
    /// `test_ifcs`.
    pub ifcs: IfcsGolden,
    /// The driver scripts.
    pub driver: DriverGolden,
    /// The oracle scripts.
    pub oracle: OracleGolden,
}

impl Golden {
    /// Reads the directory.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |f: &str| {
            let p = dir.join(f);
            std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
        };
        let wrap = |f: &str, e: String| format!("{}: {e}", dir.join(f).display());
        Ok(Self {
            ifcs: IfcsGolden::parse(&read(IFCS_FILE)?).map_err(|e| wrap(IFCS_FILE, e))?,
            driver: DriverGolden::parse(&read(DRIVER_FILE)?).map_err(|e| wrap(DRIVER_FILE, e))?,
            oracle: OracleGolden::parse(&read(ORACLE_FILE)?).map_err(|e| wrap(ORACLE_FILE, e))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit as a golden keeps it (the digest of its lines).
    fn kept(u: &stream::Unit) -> RefUnit {
        RefUnit {
            name: u.name.clone(),
            died: u.died,
            lines: RefLines::Digest(digest(&u.lines)),
        }
    }

    fn kept_block(b: &compare::Block) -> RefBlock {
        RefBlock {
            op: b.op.clone(),
            d: digest(&b.lines),
        }
    }

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn normalising_takes_the_machine_out() {
        let n = Norm::new(
            Path::new("/tmp/w"),
            Path::new("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-fx"),
        );
        assert_eq!(
            n.text("a /tmp/w/t/ct b /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-fx/mock/libifcs.so /nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-x/lib"),
            "a <work>/t/ct b <fixtures>/mock/libifcs.so <store>/lib"
        );
    }

    #[test]
    fn digests_decide_and_the_head_describes() {
        let g = v(&["a", "b", "c", "d", "e", "f"]);
        let r = RefLines::Digest(digest(&g));
        assert!(diff_ref(&r, &g).raw_equal);
        let mut changed = g.clone();
        changed[1] = "B".to_owned();
        let d = diff_ref(&r, &changed);
        assert!(!d.raw_equal && d.reduced);
        assert_eq!(d.first, Some((1, Some("b".to_owned()), Some("B".to_owned()))));
        // a difference past the head: located by the counts and digest only
        let mut late = g.clone();
        late[5] = "F".to_owned();
        let d = diff_ref(&r, &late);
        assert!(!d.raw_equal && d.first.is_none());
        assert!(!RefLines::Digest(digest(&g)).matches(&g[..5]));
        assert!(RefLines::Full(g.clone()).matches(&g));
    }

    #[test]
    fn blocks_are_judged_by_digest_with_the_echo_lines_hard() {
        let cpp = "> switch\n= SUCCESS\n  be init()\n> get port\n= SUCCESS\n";
        let b: Vec<RefBlock> = compare::blocks(cpp).iter().map(kept_block).collect();
        assert_eq!(compare_blocks("x", &b, cpp).unwrap().diffs.len(), 0);
        let c = compare_blocks("x", &b, &cpp.replace("  be init()", "  be ports()")).unwrap();
        assert_eq!(c.blocks, 2);
        assert_eq!(c.diffs.len(), 1);
        assert_eq!(c.diffs[0].op, "switch");
        assert!(
            c.diffs[0].first.starts_with("cpp   be init() | rust   be ports()"),
            "{:?}",
            c.diffs[0]
        );
        assert!(compare_blocks("x", &b, &cpp.replace("get port", "get vlan")).is_err());
        assert!(compare_blocks("x", &b, "> switch\n= SUCCESS\n").is_err());
    }

    #[test]
    fn the_records_round_trip() {
        let sec = [stream::Unit {
            name: "l2".to_owned(),
            lines: v(&["x", "y"]),
            died: false,
        }];
        let counts = vec![Count {
            name: "l2".to_owned(),
            checks: 42,
            failed: 0,
            incomplete: false,
        }];
        let i = IfcsGolden {
            status: "exit 0".to_owned(),
            stdout_last: "test_ifcs: 42 checks, 0 failed".to_owned(),
            bytes: 1,
            counts,
            sections: sec.iter().map(kept).collect(),
        };
        let text = i.render();
        assert_eq!(IfcsGolden::parse(&text).unwrap().render(), text);
        let d = DriverGolden {
            scripts: vec![DriverScript {
                name: "01-x".to_owned(),
                status: "exit 0".to_owned(),
                input: "0".repeat(64),
                blocks: compare::blocks("> a\n= S\n> b\n").iter().map(kept_block).collect(),
                bytes: digest(&v(&["@B z"])),
            }],
        };
        let text = d.render();
        assert_eq!(DriverGolden::parse(&text).unwrap().render(), text);
        let o = OracleGolden {
            scripts: vec![OracleScript {
                name: "acl".to_owned(),
                status: "exit 2".to_owned(),
                input: "1".repeat(64),
                skipped: "0 exec dropped".to_owned(),
                steps: sec.iter().map(kept).collect(),
                out: v(&["STEP 0.0 x 0 ", ""]),
                events: v(&["event a"]),
            }],
        };
        let text = o.render();
        assert_eq!(OracleGolden::parse(&text).unwrap().render(), text);
        assert!(OracleGolden::parse("O\tx\n").is_err());
        assert!(IfcsGolden::parse("status\texit 0\n").is_err());
        assert_eq!(data_lines("# c\n\nx\ty\n"), "x\ty\n");
    }
}
