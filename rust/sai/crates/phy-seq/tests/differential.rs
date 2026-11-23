//! Golden test of this crate's table tools (`phy_seq::cli`, run in process; the binaries
//! `openbcm-ext-macsec-rs` / `openbcm-ext-phy-rs`) against what the C++ tools of sai-openbcm
//! answered: `openbcm-ext-macsec` / `openbcm-ext-phy` (`src/ext_macsec_check.cpp` /
//! `src/ext_phy_check.cpp` over `sai_extmacsec.cpp` / `sai_extphy.cpp`). Until that C++ was
//! deleted this file ran both on the same inputs and compared them live; the files under
//! `tests/golden/` are the C++ tools' outputs, captured at commit 50a6aa7 (2026-11-23), so a
//! test fails whenever the Rust output changes. A deliberate change of behaviour edits the golden
//! by hand, with a reason, like an allow-list entry; a golden is never regenerated from the Rust
//! output.
//!
//! What is compared: exit code, standard output and standard error of `--check` and, for a
//! table that parses, of `--render` of its sequences without parameters, with parameters, and
//! with parameters and recorded reads.
//! - Corpus (`golden/ext-*-corpus.txt`, in full): the hand-written tables below, usage errors
//!   and files that cannot be opened.
//! - Generated (`golden/ext-*-generated.txt`): tables from `common`'s generators with a fixed
//!   proptest RNG seed; per case a 64-bit FNV-1a digest of the input and one of each
//!   comparison. A different input digest means the generator changed, not the tool; a
//!   different output digest prints the case.
//! - Board tables (`golden/as7772-ext-*.txt`): the files in `PHY_SEQ_TABLES` (colon-separated,
//!   e.g. the AS7772's tables that Nix generates). A file is compared when its length and FNV-1a
//!   digest are the golden's (its sha256 is in the golden's header), otherwise it is SKIPPED
//!   (stderr and `$SAI_RS_TEST_SUMMARY`); every listed table must still parse.
//!
//! Nothing is compiled or run but this crate. Inputs are written to a scratch directory (under
//! `$SAI_RS_TEST_TMPDIR` when set, else cargo's directory for test files), which outputs show
//! as `$D`.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use phy_seq::{ExtPhyTable, MacsecTable, ParamName};
use proptest::prelude::*;
use proptest::strategy::ValueTree as _;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

const MACSEC_CORPUS_GOLDEN: &str = include_str!("golden/ext-macsec-corpus.txt");
const EXTPHY_CORPUS_GOLDEN: &str = include_str!("golden/ext-phy-corpus.txt");
const MACSEC_GENERATED_GOLDEN: &str = include_str!("golden/ext-macsec-generated.txt");
const EXTPHY_GENERATED_GOLDEN: &str = include_str!("golden/ext-phy-generated.txt");
const MACSEC_BOARD_GOLDEN: &str = include_str!("golden/as7772-ext-macsec.txt");
const EXTPHY_BOARD_GOLDEN: &str = include_str!("golden/as7772-ext-phy.txt");

/// The generated cases: how many, and the seeds of the proptest RNG (`ChaCha`) that makes them.
const MACSEC_CASES: usize = 4000;
const EXTPHY_CASES: usize = 16000;
const MACSEC_SEED: [u8; 32] = *b"phy-seq ext-macsec golden 50a6aa";
const EXTPHY_SEED: [u8; 32] = *b"phy-seq ext-phy golden 50a6aa7..";

/// A line for whoever runs the suite: on stderr, and appended to the file `$SAI_RS_TEST_SUMMARY`
/// names (the target-test bundle collects these; captured stderr of a passing test is not shown).
fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// 64-bit FNV-1a: identifies inputs and outputs in the goldens (not a security measure).
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Bytes as a quoted ASCII string: printable characters as they are, `\\`, `\"`, `\n`, `\r`,
/// `\t`, the rest `\xNN`.
fn esc(bytes: &[u8]) -> String {
    let mut s = String::from("\"");
    for &b in bytes {
        match b {
            b'\\' => s.push_str("\\\\"),
            b'"' => s.push_str("\\\""),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\t' => s.push_str("\\t"),
            0x20..=0x7e => s.push(char::from(b)),
            _ => write!(s, "\\x{b:02x}").unwrap(),
        }
    }
    s.push('"');
    s
}

/// A prebuilt helper by name from `$SAI_RS_TEST_BIN` (the bundle on a machine without cargo).
fn prebuilt(name: &str) -> Option<PathBuf> {
    let in_bin = PathBuf::from(std::env::var_os("SAI_RS_TEST_BIN")?).join(name);
    in_bin.is_file().then_some(in_bin)
}

/// A test's own directory for the files it gives the tools.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Self {
        let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
            .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
        let dir = base
            .join(format!("phy-seq-differential-{}", std::process::id()))
            .join(test);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn put(&self, name: &str, content: &[u8]) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    /// `bytes` with this directory written `$D`, escaped.
    fn norm(&self, bytes: &[u8]) -> String {
        let dir = self.dir.as_os_str().as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut rest = bytes;
        while let Some((&b, tail)) = rest.split_first() {
            if let Some(after) = rest.strip_prefix(dir) {
                out.extend_from_slice(b"$D");
                rest = after;
            } else {
                out.push(b);
                rest = tail;
            }
        }
        esc(&out)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Exit code (`None`: killed by a signal), standard output, standard error.
type Output = (Option<i32>, Vec<u8>, Vec<u8>);
type Tool = fn(&[OsString], &mut dyn std::io::Write, &mut dyn std::io::Write) -> u8;
type Runner<'a> = dyn FnMut(&[OsString]) -> Output + 'a;

fn run_exe(exe: &Path, args: &[OsString]) -> Output {
    let out = Command::new(exe)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
    (out.status.code(), out.stdout, out.stderr)
}

/// One of the tools in process, as if run with `args`.
fn in_process(tool: Tool) -> impl FnMut(&[OsString]) -> Output {
    move |args| {
        let mut full = vec![OsString::from("rs")];
        full.extend_from_slice(args);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = tool(&full, &mut out, &mut err);
        (Some(i32::from(code)), out, err)
    }
}

fn os(args: &[&OsStr]) -> Vec<OsString> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

/// What a golden keeps of a run: all of it, the exit code (the usage text names the program,
/// which differs), or a digest.
#[derive(Clone, Copy)]
enum Keep {
    Full,
    ExitCode,
    Digest,
}

/// One entry of a transcript: a section line (`== what`, the input) or a run (`-- args`, then
/// what was kept). `golden` is the text the golden holds, `shown` the whole run.
struct Entry {
    golden: String,
    shown: String,
    digest: Option<u64>,
}

/// The runs of a test in order, as text the goldens are compared with.
#[derive(Default)]
struct Transcript {
    entries: Vec<Entry>,
    runs: u64,
}

impl Transcript {
    fn section(&mut self, what: &str) {
        let line = format!("== {what}\n");
        self.entries.push(Entry {
            golden: line.clone(),
            shown: line,
            digest: None,
        });
    }

    fn run(&mut self, sc: &Scratch, run: &mut Runner<'_>, args: &[OsString], keep: Keep) -> Output {
        let o = run(args);
        let label: Vec<String> = args.iter().map(|a| sc.norm(a.as_bytes())).collect();
        let label = if label.is_empty() {
            vec!["(no arguments)".to_owned()]
        } else {
            label
        };
        let exit = o.0.map_or_else(|| "signal".to_owned(), |c| c.to_string());
        let shown = format!(
            "-- {}\nexit {exit}\nout {}\nerr {}\n",
            label.join(" "),
            sc.norm(&o.1),
            sc.norm(&o.2)
        );
        let digest = fnv1a64(shown.as_bytes());
        let golden = match keep {
            Keep::Full => shown.clone(),
            Keep::ExitCode => format!("-- {}\nexit {exit}\n", label.join(" ")),
            Keep::Digest => format!("-- {} = {digest:016x}\n", label.join(" ")),
        };
        self.entries.push(Entry {
            golden,
            shown,
            digest: Some(digest),
        });
        self.runs += 1;
        o
    }

    /// The case line of a generated golden: index, input digest, the digest of each run.
    fn case_line(&self, i: usize, input: u64) -> String {
        let mut line = format!("{i} {input:016x}");
        for d in self.entries.iter().filter_map(|e| e.digest) {
            write!(line, " {d:016x}").unwrap();
        }
        line
    }
}

/// A golden's lines without its header (`#` lines).
fn body(golden: &str) -> impl Iterator<Item = &str> {
    golden.lines().filter(|l| !l.starts_with('#'))
}

/// Compares a transcript with a golden in the transcript format; the number of runs.
fn check_transcript(name: &str, golden: &str, t: &Transcript) -> u64 {
    let mut want: Vec<String> = Vec::new();
    for line in body(golden) {
        match want.last_mut() {
            Some(entry) if !line.starts_with("== ") && !line.starts_with("-- ") => {
                entry.push_str(line);
                entry.push('\n');
            }
            _ => want.push(format!("{line}\n")),
        }
    }
    let mut section = "(none)";
    for (i, e) in t.entries.iter().enumerate() {
        if e.golden.starts_with("== ") {
            section = &e.golden;
        }
        let Some(w) = want.get(i) else {
            panic!(
                "{name}: the golden ends before entry {i}; this test now also gives\n{}",
                e.shown
            );
        };
        if *w != e.golden {
            let why = if e.golden.starts_with("== ") || w.starts_with("== ") {
                "the inputs differ from the golden's (this test's corpus or code changed, not the tool)"
            } else {
                "the Rust tool's output differs from the C++ tool's (the golden)"
            };
            panic!(
                "{name}: entry {i}: {why}\nsection: {section}=== golden (C++ at 50a6aa7):\n{w}=== Rust now:\n{}",
                e.shown
            );
        }
    }
    assert_eq!(
        want.len(),
        t.entries.len(),
        "{name}: the golden has more entries than this test gives (next: {:?})",
        want.get(t.entries.len())
    );
    t.runs
}

/// Parameter files: every parameter in full, with bytes that tell the parameters apart; and a
/// short key (16 bytes) with odd spellings the C++ reader accepts.
fn param_files() -> [Vec<u8>; 2] {
    let mut full = String::new();
    for (n, name) in (1u8..).zip(ParamName::ALL) {
        write!(full, "{name} ").unwrap();
        for i in 0..name.max_len() {
            write!(full, "{:02x}", 0x10 * n + i).unwrap();
        }
        full.push('\n');
    }
    let short = b"key 000102030405060708090A0b0c0d0e0f\n  sci 1z+2-1 0x 3\nunknown 00\nhkey 0f0e0d0c0b0a09080706050403020100ffee pn 0102\nkey 0f0e0d0c0b0a0908070605040302010\n".to_vec();
    [full.into_bytes(), short]
}

/// `--check` of one ext-macsec table, and `--render` of up to `max_seqs` of its sequences
/// (kept as `renders` says).
fn macsec_runs(
    sc: &Scratch,
    t: &mut Transcript,
    run: &mut Runner<'_>,
    text: &[u8],
    max_seqs: usize,
    seed: u64,
    renders: Keep,
) {
    let file = sc.put("table", text);
    let f = file.as_os_str();
    t.run(sc, run, &os(&["--check".as_ref(), f]), Keep::Full);
    let Ok(table) = MacsecTable::parse(text) else {
        return;
    };
    let [full, short] = param_files();
    let (full, short) = (sc.put("params-full", &full), sc.put("params-short", &short));
    // recorded reads: a few that make comparisons pass now and then, written the ways strtoul reads
    let mut state = seed | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let reads: String = (0..64)
        .map(|i| match i % 7 {
            0 => "0 ".to_owned(),
            1 => format!("0x{:x} ", next() & 0xffff),
            2 => "ffff ".to_owned(),
            _ => format!("{:x}\n", next() & 0xffff),
        })
        .collect();
    let reads = sc.put("reads", reads.as_bytes());
    for (name, _) in table.sequences().take(max_seqs) {
        let phy = format!("{:x}", next() & 0xffff);
        let variants: [Vec<&OsStr>; 5] = [
            vec![],
            vec!["-".as_ref()],
            vec![full.as_os_str()],
            vec![short.as_os_str()],
            vec![full.as_os_str(), reads.as_os_str()],
        ];
        for extra in variants {
            let mut args = os(&["--render".as_ref(), f, name.as_ref(), phy.as_ref()]);
            args.extend(extra.iter().map(|a| (*a).to_owned()));
            t.run(sc, run, &args, renders);
        }
    }
}

fn extphy_runs(sc: &Scratch, t: &mut Transcript, run: &mut Runner<'_>, text: &[u8]) {
    let file = sc.put("table", text);
    t.run(sc, run, &os(&["--check".as_ref(), file.as_os_str()]), Keep::Full);
}

const MACSEC_CORPUS: &[&[u8]] = &[
    b"",
    b"\n",
    b"ext-phy 1\n",
    b"ext-macsec 2\n",
    b"ext-macsec 1",
    b"ext-macsec 1\n",
    b"\n# c\n\text-macsec\x0b1\x0c\r\ncounters absolute\r\nifg-bits 2040\r\nport 100000 ffff\r\nseq a\r\nD 100000\r\n",
    b"ext-macsec 1\nW p 1 a000 6\n",
    b"ext-macsec 1\nseq a\nW p 0 a000 6\n",
    b"ext-macsec 1\nseq a\nW p 20 a000 6\n",
    b"ext-macsec 1\nseq a\nW q 1 a000 6\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 10000\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 key[32].00\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 hkey[16].00\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 secret[0].00\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 0.key[0]\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 $16\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 $0\n",
    b"ext-macsec 1\nseq a\nX p 1 a000 $16\n",
    b"ext-macsec 1\nseq a\nX p 1 a000 pn[8]\n",
    b"ext-macsec 1\nseq a\nX p 1 a000 pn\n",
    b"ext-macsec 1\nseq a\nE p 1 a000 3 1\n",
    b"ext-macsec 1\nseq a\nP p 1 a000 1 1 0 0\n",
    b"ext-macsec 1\nseq a\nD 200000\n",
    b"ext-macsec 1\nseq a\nC p 1 a000 1 1\n",
    b"ext-macsec 1\nseq a\nseq a\n",
    b"ext-macsec 1\nseq A\n",
    b"ext-macsec 1\nport 1 10\nport 1 11\n",
    b"ext-macsec 1\nport 1\n",
    b"ext-macsec 1\ncounters sometimes\n",
    b"ext-macsec 1\nifg-bits 9999\n",
    b"ext-macsec 1\ncounters absolute\nseq a\nW 10 1 a000 $0|1\n",
    // the corners of strtoul and of std::string
    b"ext-macsec 1\nport 007 0x1F\nport 7 2\n",
    b"ext-macsec 1\nport 1\x00junk 10\nport 1 11\n",
    b"ext-macsec 1\nport \x00 \x00\nseq s\nW \x00 1\x00 \x00 \x00\nW p 1 0 \x00\x00.1\x00\nX p 1 0 $\x00\nW p 1 1 $\x00&\x00|\x00\nD \x00\nP p 1 0 \x00 \x00 1\x00 \x00\n",
    b"ext-macsec 1\nseq s\nW 0x10 0x1 0xa000 0xffff\nW p 0X1f 0XFFFF 0X0\nE p 1 0 0x1 0x1\nR 0x 1 0\n",
    b"ext-macsec 1\nseq s\nR p 1 0x\n",
    b"ext-macsec 1\nseq s\nD 0x10\n",
    b"ext-macsec 1\n\xff\xfe\x00 1\n",
    b"ext-macsec 1\nseq s\nW p 1 0 00.0x\n",
    b"ext-macsec 1\nseq s\nW p 1 0 $0|1&2\n",
    b"ext-macsec 1\nseq s\nX p 1 0 $1\nW p 1 0 $1&&1\n",
    b"ext-macsec 1\nseq s\nX p 1 0 $1\nW p 1 0 $1.2\nW p 1 0 $1&ff|1|2\n",
    b"ext-macsec 1\nseq s\nX p 1 0 $01\nW p 1 0 $1\nW p 1 0 $001&0ff|0x1\n",
    b"ext-macsec 1\nseq s\nX p 1 0 a[0]]\n",
    b"ext-macsec 1\nseq s\nX p 1 0 [0]\n",
    b"ext-macsec 1\nseq s\nX p 1 0 a[07]\nX p 1 0 _[7]\nX p 1 0 $3[1]\n",
    b"ext-macsec 1\nseq b\nW p 1 0 $1\nseq a\nX p 1 0 $2\nW p 1 0 $3|1\n",
    b"ext-macsec 1\nseq s\nP p 1 8221 12e3 ffff 3 5\nE p 1 a100 2 f\nX p 1 1 r[2]\nX 5 1f ffff $15\nW 1e0 1 2 $15&ff00|1\nD 0\nP 2 1 3 0 0 2 0\n",
    b"ext-macsec 1\nseq s\nW p 1 1 key[1].key[0]\nW p 1 2 key[31].key[30]\nW p 1 3 00.sci[7]\nW p 1 4 iv[11].window[3]\n",
];

const EXTPHY_CORPUS: &[&[u8]] = &[
    b"",
    b"mdio-script 1\n",
    b"ext-phy 2\n",
    b"ext-phy 1\n",
    b"ext-phy 1\n# comment\npoll-ms 0\nup-polls 2\nholdoff-ms 50\nport 1\nlink\n  R 10 1 8221\n  W 10 1 a002 c161\n  W 10 1 8222 1111\n  P 10 1 8221 11e3 ffff 3 0\n  E 10 1 a100 2\n  C 10 1 a101 3 3\nadmin-down\n  W 10 1 d139 351\nadmin-up\n  E 10 1 d0e1 a008 ff7f\n  W 10 1 d139 350\nport 5\nlink\n  C 11 1 a101 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 0 1 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 20 1 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 10000 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 1\n",
    b"ext-phy 1\nport 1\nlink\nW 10 1 a000 6\n",
    b"ext-phy 1\nport 1\nadmin-down\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport 1\n",
    b"ext-phy 1\nW 10 1 a000 6\n",
    b"ext-phy 1\nport 1\nW 10 1 a000 6\n",
    b"ext-phy 1\nlink\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\nport 1\nlink\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\nlink\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport x\n",
    b"ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 0 0\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 5 200000\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport 1\nlink\nX 10 1 8221\n",
    b"ext-phy 1\nport 1\nlink\nW 10 1 a000 zz\n",
    b"ext-phy 1\nport 1\nlink\nW 10 1 a000 -6\n",
    b"ext-phy 1\npoll-ms 70000\n",
    b"ext-phy 1\nup-polls 0\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\npoll-ms 100\n",
    b"ext-phy 1\nport 1\nlink\nP 1 1 1 2 1 0 0\n",
    b"ext-phy 1\nport 1\nlink\nP 1 1 1 2 1 1 0\n",
    b"ext-phy 1\nport 3\nadmin-up\nR 1 1 1\n\nport 03\n",
    b"ext-phy 1\nport 3\x00x\nadmin-up\nR 1 1 1\n\nport 3\x00y\n",
    b"ext-phy 1\nport 1\nport 2\nlink\nR 1 1 1\n",
    b"ext-phy 1\nport 1\nlink\nlink\nC 0x1 0x1 0x1 0x1 0x1\nadmin-up\nadmin-up\nW 1 1 1 1\nadmin-up\n",
    b"ext-phy 1\nport 1\nadmin-down now\n",
    b"ext-phy 1\nholdoff-ms 600000\npoll-ms 60000\nup-polls 100\nholdoff-ms 0\nport 100000\nadmin-down\nR ffff 1f ffff\n",
    b"ext-phy 1\nport 1\nlink\nC 1 1 1 1 1\nport 2\nlink\nR 1 1 1\n",
];

/// The ext-macsec corpus, the test table of `common`, usage errors and unopenable files.
fn macsec_corpus(sc: &Scratch, run: &mut Runner<'_>) -> Transcript {
    let mut t = Transcript::default();
    for (i, text) in MACSEC_CORPUS.iter().enumerate() {
        t.section(&format!("table {i} {}", esc(text)));
        macsec_runs(sc, &mut t, run, text, usize::MAX, i as u64, Keep::Full);
    }
    let ms = common::ms_table(true, true);
    t.section(&format!("test table {}", esc(ms.as_bytes())));
    macsec_runs(sc, &mut t, run, ms.as_bytes(), usize::MAX, 99, Keep::Full);
    // usage: the exit code only (the usage text names the program)
    let missing = sc.dir.join("no-such-file");
    let table = sc.put("usage-table", b"ext-macsec 1\nseq s\nW p 1 1 key[0].00\n");
    t.section(&format!(
        "usage, {} = {}",
        sc.norm(table.as_os_str().as_bytes()),
        esc(b"ext-macsec 1\nseq s\nW p 1 1 key[0].00\n")
    ));
    let usage: Vec<Vec<OsString>> = vec![
        vec![],
        os(&["--check".as_ref()]),
        os(&["--frob".as_ref(), table.as_os_str()]),
        os(&["--render".as_ref(), table.as_os_str(), "s".as_ref()]),
        os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "1".as_ref(),
            "-".as_ref(),
            "-".as_ref(),
            "-".as_ref(),
        ]),
    ];
    for args in &usage {
        t.run(sc, run, args, Keep::ExitCode);
    }
    t.section("files that cannot be opened, bad arguments");
    let files: Vec<Vec<OsString>> = vec![
        os(&["--check".as_ref(), missing.as_os_str()]),
        os(&["--check".as_ref(), sc.dir.as_os_str()]),
        os(&["--render".as_ref(), table.as_os_str(), "nope".as_ref(), "1".as_ref()]),
        os(&["--render".as_ref(), table.as_os_str(), "s".as_ref(), "1e0".as_ref()]),
        os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "1e0".as_ref(),
            missing.as_os_str(),
        ]),
        os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "1e0".as_ref(),
            "-".as_ref(),
            missing.as_os_str(),
        ]),
        os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "0x1E0".as_ref(),
            table.as_os_str(),
            table.as_os_str(),
        ]),
    ];
    for args in &files {
        t.run(sc, run, args, Keep::Full);
    }
    t
}

/// The ext-phy corpus, usage errors and an unopenable file.
fn extphy_corpus(sc: &Scratch, run: &mut Runner<'_>) -> Transcript {
    let mut t = Transcript::default();
    for (i, text) in EXTPHY_CORPUS.iter().enumerate() {
        t.section(&format!("table {i} {}", esc(text)));
        extphy_runs(sc, &mut t, run, text);
    }
    let missing = sc.dir.join("no-such-file");
    t.section("usage");
    for args in [
        vec![],
        os(&["--render".as_ref(), missing.as_os_str()]),
        os(&["--check".as_ref(), missing.as_os_str(), "x".as_ref()]),
    ] {
        t.run(sc, run, &args, Keep::ExitCode);
    }
    t.section("missing file");
    t.run(sc, run, &os(&["--check".as_ref(), missing.as_os_str()]), Keep::Full);
    t
}

/// A board table's transcript: `--check` in full, the renders as digests.
fn macsec_board(sc: &Scratch, run: &mut Runner<'_>, text: &[u8]) -> Transcript {
    let mut t = Transcript::default();
    t.section("board table");
    macsec_runs(sc, &mut t, run, text, usize::MAX, 7, Keep::Digest);
    t
}

fn extphy_board(sc: &Scratch, run: &mut Runner<'_>, text: &[u8]) -> Transcript {
    let mut t = Transcript::default();
    t.section("board table");
    extphy_runs(sc, &mut t, run, text);
    t
}

/// `n` values of `strategy` from the proptest RNG seeded with `seed` (no shrinking, no cases run).
fn generated<S: Strategy>(strategy: &S, seed: &[u8; 32], n: usize) -> Vec<S::Value> {
    let config = Config {
        failure_persistence: None,
        ..Config::default()
    };
    let mut runner = TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, seed));
    (0..n)
        .map(|_| strategy.new_tree(&mut runner).unwrap().current())
        .collect()
}

fn macsec_cases() -> Vec<(Vec<u8>, u64)> {
    generated(&(common::macsec_text(), any::<u64>()), &MACSEC_SEED, MACSEC_CASES)
}

fn extphy_cases() -> Vec<Vec<u8>> {
    generated(&common::extphy_text(), &EXTPHY_SEED, EXTPHY_CASES)
}

fn macsec_input_digest(text: &[u8], seed: u64) -> u64 {
    let mut bytes = seed.to_le_bytes().to_vec();
    bytes.extend_from_slice(text);
    fnv1a64(&bytes)
}

/// Compares generated cases with a generated golden: `case(i)` gives the input digest, the
/// input as text, and the transcript. Returns the number of runs.
fn check_generated(
    name: &str,
    golden: &str,
    n: usize,
    mut case: impl FnMut(usize) -> (u64, String, Transcript),
) -> u64 {
    let mut lines = body(golden);
    assert_eq!(
        lines.next(),
        Some(format!("cases {n}").as_str()),
        "{name}: the golden's case count"
    );
    let (mut runs, mut drift, mut changed) = (0, 0, 0);
    let mut first = Vec::new();
    for i in 0..n {
        let (input, shown_input, t) = case(i);
        runs += t.runs;
        let line = t.case_line(i, input);
        let want = lines
            .next()
            .unwrap_or_else(|| panic!("{name}: the golden ends before case {i}"));
        if want == line {
            continue;
        }
        let mut want_words = want.split(' ').skip(1);
        if want_words.next() != Some(format!("{input:016x}").as_str()) {
            drift += 1;
            if drift <= 3 {
                first.push(format!(
                    "case {i}: generator changed, golden inputs differ (input now {shown_input})"
                ));
            }
            continue;
        }
        changed += 1;
        if changed <= 3 {
            let runs: Vec<&Entry> = t.entries.iter().filter(|e| e.digest.is_some()).collect();
            let at = runs
                .iter()
                .zip(want_words.by_ref())
                .position(|(e, w)| e.digest.map(|d| format!("{d:016x}")).as_deref() != Some(w))
                .unwrap_or(runs.len());
            let what = runs.get(at).map_or_else(
                || "the golden has a different number of runs".to_owned(),
                |e| format!("Rust now gives\n{}", e.shown),
            );
            first.push(format!(
                "case {i}: run {at} differs from the C++ tool's output (the golden); input {shown_input}\n{what}"
            ));
        }
    }
    assert_eq!(lines.next(), None, "{name}: the golden has more than {n} cases");
    assert!(
        drift == 0 && changed == 0,
        "{name}: {changed} of {n} cases differ from the C++ tool's outputs, {drift} more because the generator changed (inputs differ from the golden's):\n{}",
        first.join("\n")
    );
    runs
}

fn board_tables() -> Option<Vec<(PathBuf, Vec<u8>)>> {
    let list = std::env::var_os("PHY_SEQ_TABLES")?;
    Some(
        std::env::split_paths(&list)
            .map(|path| {
                let text = std::fs::read(&path).unwrap_or_else(|e| panic!("PHY_SEQ_TABLES: {}: {e}", path.display()));
                (path, text)
            })
            .collect(),
    )
}

/// The board tables of one kind in `PHY_SEQ_TABLES`: each must parse; the one the golden was
/// captured from is compared with it, others are SKIPPED. Returns (runs, tables compared).
fn check_boards(
    kind: &str,
    golden: &str,
    parses: fn(&[u8]) -> bool,
    transcript: fn(&Scratch, &mut Runner<'_>, &[u8]) -> Transcript,
    run: &mut Runner<'_>,
) -> (u64, u32) {
    let Some(tables) = board_tables() else {
        summary(&format!(
            "phy-seq differential: PHY_SEQ_TABLES is not set, the {kind} board table golden is SKIPPED"
        ));
        return (0, 0);
    };
    let mut lines = body(golden);
    let id = lines.next().unwrap();
    let rest = lines.fold(String::new(), |mut rest, l| {
        rest.push_str(l);
        rest.push('\n');
        rest
    });
    let (mut runs, mut boards) = (0, 0);
    for (path, text) in tables.iter().filter(|(_, text)| text.starts_with(kind.as_bytes())) {
        assert!(parses(text), "{}: refused", path.display());
        let this = format!("table {} {:016x}", text.len(), fnv1a64(text));
        if this != id {
            summary(&format!(
                "phy-seq differential: {} ({this}) is not the table of the {kind} board golden ({id}), its comparison is SKIPPED",
                path.display()
            ));
            continue;
        }
        let sc = Scratch::new(&format!("{kind}-board"));
        runs += check_transcript(&path.display().to_string(), &rest, &transcript(&sc, run, text));
        boards += 1;
    }
    (runs, boards)
}

#[test]
fn ext_macsec_corpus_and_board_tables() {
    let mut rust = in_process(phy_seq::cli::ext_macsec);
    let sc = Scratch::new("ext-macsec-corpus");
    let t = macsec_corpus(&sc, &mut rust);
    let mut n = check_transcript("ext-macsec corpus", MACSEC_CORPUS_GOLDEN, &t);
    let (board_runs, boards) = check_boards(
        "ext-macsec",
        MACSEC_BOARD_GOLDEN,
        |text| MacsecTable::parse(text).is_ok(),
        macsec_board,
        &mut rust,
    );
    n += board_runs;
    summary(&format!(
        "phy-seq differential: ext-macsec corpus: {n} runs equal to the C++ tool's golden (50a6aa7), {boards} board table(s)"
    ));
}

#[test]
fn ext_phy_corpus_and_board_tables() {
    let mut rust = in_process(phy_seq::cli::ext_phy);
    let sc = Scratch::new("ext-phy-corpus");
    let mut n = check_transcript("ext-phy corpus", EXTPHY_CORPUS_GOLDEN, &extphy_corpus(&sc, &mut rust));
    let (board_runs, boards) = check_boards(
        "ext-phy",
        EXTPHY_BOARD_GOLDEN,
        |text| ExtPhyTable::parse(text).is_ok(),
        extphy_board,
        &mut rust,
    );
    n += board_runs;
    summary(&format!(
        "phy-seq differential: ext-phy corpus: {n} runs equal to the C++ tool's golden (50a6aa7), {boards} board table(s)"
    ));
}

#[test]
fn generated_ext_macsec_tables() {
    let mut rust = in_process(phy_seq::cli::ext_macsec);
    let sc = Scratch::new("ext-macsec-generated");
    let cases = macsec_cases();
    let n = check_generated(
        "generated ext-macsec tables",
        MACSEC_GENERATED_GOLDEN,
        cases.len(),
        |i| {
            let (text, seed) = &cases[i];
            let mut t = Transcript::default();
            macsec_runs(&sc, &mut t, &mut rust, text, 3, *seed, Keep::Full);
            (
                macsec_input_digest(text, *seed),
                format!("{} seed {seed}", esc(text)),
                t,
            )
        },
    );
    summary(&format!(
        "phy-seq differential: generated ext-macsec tables: {MACSEC_CASES} cases, {n} runs equal to the C++ tool's golden (50a6aa7)"
    ));
}

#[test]
fn generated_ext_phy_tables() {
    let mut rust = in_process(phy_seq::cli::ext_phy);
    let sc = Scratch::new("ext-phy-generated");
    let cases = extphy_cases();
    let n = check_generated("generated ext-phy tables", EXTPHY_GENERATED_GOLDEN, cases.len(), |i| {
        let text = &cases[i];
        let mut t = Transcript::default();
        extphy_runs(&sc, &mut t, &mut rust, text);
        (fnv1a64(text), esc(text), t)
    });
    summary(&format!(
        "phy-seq differential: generated ext-phy tables: {EXTPHY_CASES} cases, {n} runs equal to the C++ tool's golden (50a6aa7)"
    ));
}

/// The binaries themselves (not only the library functions behind them): arguments, streams and
/// exit codes arrive. Needs no C++.
#[test]
fn the_rust_binaries_run() {
    let sc = Scratch::new("binaries");
    let find = |name: &str, built: &str| prebuilt(name).unwrap_or_else(|| PathBuf::from(built));
    let macsec = find("openbcm-ext-macsec-rs", env!("CARGO_BIN_EXE_openbcm-ext-macsec-rs"));
    let phy = find("openbcm-ext-phy-rs", env!("CARGO_BIN_EXE_openbcm-ext-phy-rs"));
    if !macsec.is_file() || !phy.is_file() {
        eprintln!(
            "differential: the Rust tools are not at {} / {}: running them is SKIPPED",
            macsec.display(),
            phy.display()
        );
        return;
    }
    let table = sc.put(
        "table",
        b"ext-macsec 1\nport 1 1e0\nseq s\nW p 1 a000 key[1].key[0]\nE 5 1f 2 3 f\nD 9\n",
    );
    let params = sc.put("params", b"key 1122\n");
    let out = run_exe(&macsec, &os(&["--check".as_ref(), table.as_os_str()]));
    assert_eq!(out.0, Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.1),
        format!(
            "{}: 1 ports, 1 sequences, 3 operations, counters clear-on-read, ifg-bits 0\n",
            table.display()
        )
    );
    let out = run_exe(
        &macsec,
        &os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "1e0".as_ref(),
            params.as_os_str(),
        ]),
    );
    assert_eq!(
        (out.0, String::from_utf8_lossy(&out.1).as_ref()),
        (Some(0), "W 1e0 1 a000 2211\nR 005 1f 0002 0003 000f\nD 9\n")
    );
    let out = run_exe(
        &macsec,
        &os(&["--render".as_ref(), table.as_os_str(), "s".as_ref(), "1e0".as_ref()]),
    );
    assert_eq!(out.0, Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.2),
        format!("{}: s: line 4: parameter key[1] is not available\n", table.display())
    );
    assert_eq!(out.1, Vec::<u8>::new());
    let out = run_exe(&macsec, &[]);
    assert_eq!(out.0, Some(2));
    assert!(String::from_utf8_lossy(&out.2).ends_with(" --check FILE | --render FILE SEQ PHY [PARAMS|- [READS]]\n"));
    let out = run_exe(
        &macsec,
        &os(&["--render".as_ref(), table.as_os_str(), "s".as_ref(), "10000".as_ref()]),
    );
    assert_eq!(out.0, Some(2));
    let phy_table = sc.put(
        "phy-table",
        b"ext-phy 1\nport 1\nlink\nC 1 1 1 1 1\nadmin-up\nW 1 1 1 1\nW 1 1 1 2\n",
    );
    let out = run_exe(&phy, &os(&["--check".as_ref(), phy_table.as_os_str()]));
    assert_eq!(out.0, Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.1),
        format!(
            "{}: 1 ports (1 with a link poll), poll 500 ms, up after 2 good polls, hold-off 3000 ms; operations: link 1, admin-down 0, admin-up 2\n",
            phy_table.display()
        )
    );
    let out = run_exe(&phy, &os(&["--check".as_ref(), table.as_os_str()]));
    assert_eq!(out.0, Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.2),
        format!(
            "{}: line 1: not an ext-phy table (first line must be 'ext-phy 1')\n",
            table.display()
        )
    );
    assert_eq!(run_exe(&phy, &[]).0, Some(2));
}
