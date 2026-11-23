//! Golden test against the C tool: the same command line, scripts, names and (fake) chip as
//! `openbcm-mdio-script` built from sai-openbcm's C sources had (`mdio_script_main.c`,
//! `mdio_script.c`, `mdio_report.c`; with the SDK on `tests/c/mdio_stub.c`, the MDIO side of the
//! glue in C) for this crate's binary on sdk-glue's Rust fake; exit code, stdout, stderr,
//! `status.json`, `status.txt` and the chip's access trace must be the bytes the C tool gave once
//! run times, the pid and the directories are masked, and through the diag socket the commands
//! sent must be the C tool's.
//!
//! Until 2026-11-23 the C tool was built by `build.rs` and run next to this binary (159 runs, all
//! equal). Its results, captured at commit 50a6aa7 (sha256 of the C files in the header), are
//! `golden/differential.txt`: one record per run, with the case's name, arguments and a digest of
//! its input files, so an edited case shows as "golden inputs differ". The golden is not
//! regenerated from this crate's output: a deliberate behaviour change edits the record by hand
//! with a reason. Where the build tree is not (a copied test executable), `SAI_RS_TEST_BIN` may
//! name a directory with `openbcm-mdio-script-rs` (the binary cargo builds for this test: with
//! the fake glue).
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{NAMES, put, scratch, script_text};

const GOLDEN: &str = include_str!("golden/differential.txt");

/// This crate's binary: the one cargo built, or `openbcm-mdio-script-rs` in `SAI_RS_TEST_BIN`.
fn rust_tool() -> PathBuf {
    std::env::var_os("SAI_RS_TEST_BIN")
        .map(|d| PathBuf::from(d).join("openbcm-mdio-script-rs"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openbcm-mdio-script-rs")))
}

/// 64-bit FNV-1a.
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The digest of a case's input files (`{d}` not replaced).
fn inputs_digest(files: &[(&str, &str)]) -> u64 {
    let mut bytes = Vec::new();
    for (name, text) in files {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
    }
    fnv1a64(&bytes)
}

/// One run of the C tool, from the golden.
struct Record {
    /// `<test> <i>`
    id: String,
    /// name, arguments, without the SDK, input digest: what identifies the case
    case: String,
    result: Result_,
    /// the commands sent to the diag socket (`verification_through_a_diag_socket`)
    commands: Vec<String>,
}

/// The lines of a `|`-prefixed block from `lines`, up to the next line that is not one; a
/// following `\ no newline at end` takes the last newline away.
fn block<'a>(lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>) -> String {
    let mut text = String::new();
    while let Some(l) = lines.next_if(|l| l.starts_with('|')) {
        text.push_str(&l[1..]);
        text.push('\n');
    }
    if lines.next_if(|l| *l == "\\ no newline at end").is_some() {
        text.pop();
    }
    text
}

/// `golden/differential.txt`: records of `case <test> <i>`, `name`, `args` (Rust's `{:?}`),
/// `no_sdk`, `inputs` (FNV-1a), `code`, then the blocks `stdout`, `stderr`, `file <path>` and
/// `commands` (one per line), `end`; `#` lines between records are comments.
fn golden(test: &str) -> Vec<Record> {
    let mut records = Vec::new();
    let mut lines = GOLDEN.lines().peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let id = line
            .strip_prefix("case ")
            .unwrap_or_else(|| panic!("golden: expected a case line, not {line:?}"))
            .to_owned();
        let mut field = |key: &str| {
            let l = lines.next().unwrap_or_default();
            l.strip_prefix(key)
                .and_then(|v| v.strip_prefix(' '))
                .unwrap_or_else(|| panic!("golden {id}: expected {key}, not {l:?}"))
                .to_owned()
        };
        let case = [field("name"), field("args"), field("no_sdk"), field("inputs")].join("\n");
        let code = field("code");
        let mut result = Result_ {
            code: (code != "none").then(|| code.parse().unwrap()),
            stdout: String::new(),
            stderr: String::new(),
            files: Vec::new(),
        };
        let mut commands = Vec::new();
        loop {
            let l = lines.next().unwrap_or_else(|| panic!("golden {id}: no end"));
            match l {
                "end" => break,
                "stdout" => result.stdout = block(&mut lines),
                "stderr" => result.stderr = block(&mut lines),
                "commands" => commands = block(&mut lines).split_inclusive('\n').map(str::to_owned).collect(),
                _ => {
                    let path = l
                        .strip_prefix("file ")
                        .unwrap_or_else(|| panic!("golden {id}: unexpected {l:?}"));
                    result.files.push((path.to_owned(), block(&mut lines)));
                }
            }
        }
        if id.split(' ').next() == Some(test) {
            records.push(Record {
                id,
                case,
                result,
                commands,
            });
        }
    }
    records
}

/// The golden records of one test, in the order the test runs its cases.
struct Golden {
    test: &'static str,
    records: std::vec::IntoIter<Record>,
    count: usize,
}

impl Golden {
    fn new(test: &'static str) -> Self {
        let records = golden(test);
        Self {
            test,
            count: records.len(),
            records: records.into_iter(),
        }
    }

    /// The next record, after checking that it was made for this case.
    fn next(&mut self, case: &Case<'_>) -> Record {
        let this = format!(
            "{}\n{:?}\n{}\n{:016x}",
            case.name,
            case.args,
            u8::from(case.no_sdk),
            inputs_digest(case.files)
        );
        let record = self.records.next().unwrap_or_else(|| {
            panic!(
                "{}: case '{}' is not in the golden ({} records): golden inputs differ\n{this}",
                self.test, case.name, self.count
            )
        });
        assert!(
            record.case == this,
            "golden {}: made for another case, golden inputs differ (the test was edited)\n--- golden\n{}\n--- this case\n{this}",
            record.id,
            record.case
        );
        record
    }

    /// Every record was used.
    fn finish(mut self) {
        let rest = self.records.len();
        assert!(
            self.records.next().is_none(),
            "{}: {rest} golden records not run: golden inputs differ",
            self.test
        );
    }
}

/// Replaces the run of digits and dots after every `marker` by `N`.
fn mask_number(text: &str, marker: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        let (head, tail) = rest.split_at(at + marker.len());
        out.push_str(head);
        let digits = tail.chars().take_while(|c| c.is_ascii_digit() || *c == '.').count();
        if digits > 0 {
            out.push('N');
        }
        rest = &tail[digits..];
    }
    out.push_str(rest);
    out
}

/// What differs between two runs of one tool: times, the pid, the directory (`<DIR>`) and the
/// directory of the test (`<BASE>`: the diag socket's).
fn mask(bytes: &[u8], dir: &Path) -> String {
    let mut text = String::from_utf8_lossy(bytes)
        .replace(dir.to_str().unwrap(), "<DIR>")
        .replace(dir.parent().unwrap().to_str().unwrap(), "<BASE>");
    for marker in [
        "\"pid\": ",
        "\"started\": ",
        "\"finished\": ",
        "\"seconds\": ",
        "\"longest_read_ms\": ",
        "operations in ",
        "done in ",
        "FAILED in ",
        ", took ",
        "longest read ",
        ", pid ",
        "read_timestamp_seconds{unit=\"0\"} ",
    ] {
        text = mask_number(&text, marker);
    }
    // " started 2026-11-22 10:11:12 +0000"
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(" started 2") {
        let (head, tail) = rest.split_at(at + " started ".len());
        out.push_str(head);
        out.push_str("DATE");
        rest = tail.get(25..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

struct Case<'a> {
    name: &'a str,
    /// `{d}` is replaced by the tool's directory, `{b}` by the test's (that directory's parent).
    args: &'a [&'a str],
    /// Files in the tool's directory (`{d}` replaced in the content too).
    files: &'a [(&'a str, &'a str)],
    /// Run the builds without the SDK (C) / without `MDIO_TOOL_FAKE_SDK` (Rust).
    no_sdk: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Result_ {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    files: Vec<(String, String)>,
}

fn run_one(exe: &Path, dir: &Path, case: &Case<'_>, fake_env: bool) -> Result_ {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let d = dir.to_str().unwrap();
    let b = dir.parent().unwrap().to_str().unwrap();
    for (name, text) in case.files {
        put(dir, name, &text.replace("{d}", d));
    }
    let mut cmd = Command::new(exe);
    cmd.arg0("openbcm-mdio-script")
        .args(case.args.iter().map(|a| a.replace("{d}", d).replace("{b}", b)))
        .env("TZ", "UTC")
        .env_remove("NOTIFY_SOCKET")
        .env_remove("POSIXLY_CORRECT")
        .env_remove("MDIO_TOOL_FAKE_SDK");
    if fake_env {
        cmd.env("MDIO_TOOL_FAKE_SDK", "1");
    }
    let o = cmd.output().unwrap();
    // what the run left behind, besides its inputs
    let mut files = Vec::new();
    let mut names: Vec<_> = walk(dir)
        .into_iter()
        .filter(|p| !case.files.iter().any(|(n, _)| dir.join(n) == *p))
        .collect();
    names.sort();
    for path in names {
        let rel = path.strip_prefix(dir).unwrap().to_str().unwrap().to_owned();
        files.push((rel, mask(&std::fs::read(&path).unwrap(), dir)));
    }
    Result_ {
        code: o.status.code(),
        stdout: mask(&o.stdout, dir),
        stderr: mask(&o.stderr, dir),
        files,
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else if p.file_name().is_some_and(|n| n != "diag.sock") {
            out.push(p);
        }
    }
    out
}

/// Runs this crate's tool on `case` and compares with the C tool's run in the golden; with
/// `commands` (what the diag server received) also the commands this run sent.
fn compare(
    golden: &mut Golden,
    base: &Path,
    case: &Case<'_>,
    commands: Option<&std::sync::Mutex<Vec<String>>>,
) -> Result_ {
    let want = golden.next(case);
    let before = commands.map(|c| c.lock().unwrap().len());
    let rust = run_one(&rust_tool(), &base.join("r"), case, !case.no_sdk);
    assert!(
        want.result == rust,
        "Rust differs from the C tool's golden {} in case '{}' ({:?})\n--- C (golden)\n{:#?}\n--- Rust\n{rust:#?}",
        want.id,
        case.name,
        case.args,
        want.result
    );
    if let (Some(commands), Some(before)) = (commands, before) {
        let sent = commands.lock().unwrap()[before..].to_vec();
        assert_eq!(
            sent, want.commands,
            "golden {} '{}': the diag socket commands differ from the C tool's",
            want.id, case.name
        );
    }
    rust
}

const RUN: &[&str] = &[
    "--config",
    "{d}/chip.cfg",
    "--status-file",
    "{d}/st/status.json",
    "--names",
    "{d}/names",
    "{d}/init.mdio",
];
const TRACE: &str = "trace {d}/trace.txt\n";

#[test]
fn runs_on_the_fake_chip() {
    let mut golden = Golden::new("runs_on_the_fake_chip");
    let base = scratch("diff-run");
    let ok = script_text(&["0", "1"]);
    let healthy = "reg 0 1 8b00 2391\nreg 0 1 8b01 80a0\nreg 0 1 8215 d00b\nreg 0 1 8251 2d\n\
                   reg 1 1 8b00 2391\nreg 1 1 8b01 80a0\nreg 1 1 8215 d00b\nreg 1 1 8251 2d\n";
    let chip = |extra: &str| format!("{TRACE}name BCM56960_B1\n{healthy}{extra}");
    let ready = "reg 1 1 8221 303\n";
    let mut n = 0;
    let mut go = |name: &str, args: &[&str], files: &[(&str, &str)], code: i32| {
        let r = compare(
            &mut golden,
            &base,
            &Case {
                name,
                args,
                files,
                no_sdk: false,
            },
            None,
        );
        assert_eq!(r.code, Some(code), "case '{name}': {r:#?}");
        n += 1;
        r
    };
    let files =
        |chip: &'static str| -> Vec<(&'static str, &'static str)> { vec![("chip.cfg", chip), ("names", NAMES)] };
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let ok: &'static str = leak(ok);

    // success: status files, names, verification table
    let mut f = files(leak(chip(ready)));
    f.push(("init.mdio", ok));
    let r = go("ok", RUN, &f, 0);
    assert!(
        r.files
            .iter()
            .any(|(n, t)| n == "st/status.json" && t.contains("\"state\": \"ok\"")),
        "{r:#?}"
    );
    assert!(
        r.files
            .iter()
            .any(|(n, t)| n == "trace.txt" && t.contains("W 1 1 1 1\n"))
    );
    // the poll needs re-reads
    let mut f = files(leak(chip("after 3 1 1 8221 303\n")));
    f.push(("init.mdio", ok));
    go("poll re-reads", RUN, &f, 0);
    // temperature published while it runs, and removed
    let with_temp: Vec<&str> = [
        RUN,
        &[
            "--temp-out",
            "{d}/asic/temperature",
            "--temp-prom",
            "{d}/asic/t.prom",
            "--temp-interval",
            "1",
        ],
    ]
    .concat();
    let mut f = files(leak(chip(&format!("{ready}temp 413 418 395\n"))));
    f.push(("init.mdio", ok));
    go("temperature", &with_temp, &f, 0);
    let mut f = files(leak(chip(&format!("{ready}temp 413 418 395\ntemp-fail 2\n"))));
    f.push(("init.mdio", ok));
    go("temperature read fails", &with_temp, &f, 0);
    let mut f = files(leak(chip(&format!("{ready}temp 413 1600\n"))));
    f.push(("init.mdio", ok));
    go("temperature implausible", &with_temp, &f, 0);
    let mut f = files(leak(chip(ready)));
    f.push(("init.mdio", ok));
    go("no monitors", &with_temp, &f, 0);
    // the poll never matches: diverged, post-mortem reads
    let mut f = files(leak(chip("")));
    f.push(("init.mdio", ok));
    let r = go("poll times out", RUN, &f, 2);
    assert!(r.stderr.contains("FAILED (sequence-diverged)"), "{r:#?}");
    // ... and the PHY reads a constant afterwards
    let mut f = files(leak(chip("const 1 1900\n")));
    f.push(("init.mdio", ok));
    let r = go("poll times out, hung", RUN, &f, 2);
    assert!(r.stderr.contains("FAILED (phy-not-answering)"), "{r:#?}");
    // a PHY hangs after the last write: verification finds it; a board hint
    let hints: Vec<&str> = [
        &[
            "--hint",
            "phy-not-answering=cold power cycle (lab plug 1)",
            "--hint",
            "x=y",
        ],
        RUN,
    ]
    .concat();
    let mut f = files(leak(chip(&format!("{ready}hang 3 1 1900\n"))));
    f.push(("init.mdio", ok));
    go("hangs before verification", &hints, &f, 2);
    // verification: mismatch, MDIO error, both, masked and negated assertions
    let mut f = files(leak(chip(&format!("{ready}reg 1 1 8215 1\n"))));
    f.push(("init.mdio", ok));
    let r = go("verify mismatch", RUN, &f, 2);
    assert!(r.stderr.contains("FAILED (verify-mismatch)"), "{r:#?}");
    let three: &'static str = leak(format!(
        "{}{}",
        script_text(&["0", "1", "24", "26"]),
        "N 0 1 8b00 2391 ffff 1 0\nP 0 1 8251 d f 1 0\n"
    ));
    let mut f = files(leak(chip(&format!("{ready}const 24 1900\ndead 26\n"))));
    f.push(("init.mdio", three));
    go("verify constant + dead", RUN, &f, 2);
    let dead: &'static str = leak(script_text(&["0", "26", "1"]));
    let mut f = files(leak(chip(&format!("{ready}dead 26\n"))));
    f.push(("init.mdio", dead));
    let r = go("verify dead", RUN, &f, 2);
    assert!(r.stderr.contains("FAILED (mdio-io)"), "{r:#?}");
    // MDIO master errors in the body
    for (what, rule) in [
        ("write fails", "fail-write 2\n"),
        ("poll read fails", "fail-read 1\n"),
        ("write to a dead PHY", "dead 1\n"),
    ] {
        let mut f = files(leak(chip(&format!("{ready}{rule}"))));
        f.push(("init.mdio", ok));
        let r = go(what, RUN, &f, 2);
        assert!(r.stderr.contains("FAILED (mdio-io)"), "{r:#?}");
    }
    // SIGTERM between two operations, at several places
    for at in 1..=3 {
        let mut f = files(leak(chip(&format!("{ready}sigterm {at}\n"))));
        f.push(("init.mdio", ok));
        let r = go("sigterm", RUN, &f, 7);
        assert!(r.stderr.contains("FAILED (interrupted)"), "{r:#?}");
    }
    // a long delay is interrupted at a tick
    let slow: &'static str = "mdio-script 1\nL slow\nW 0 1 1 1\nD 1200000\nW 0 1 2 2\n";
    let mut f = files(leak(chip("sigterm 1\n")));
    f.push(("init.mdio", slow));
    go("sigterm before a delay", RUN, &f, 7);
    // E mismatches are logged and counted; a script without a verification section; labels
    let plain: &'static str =
        "mdio-script 1\nL one \"quoted\" \\ step\nW 0 1 1 5\nE 0 1 1 6\nE 0 1 1 5\nR 0 1 2\nN 0 1 1 6 ffff 2 10\n";
    let mut f = files(leak(chip("")));
    f.push(("init.mdio", plain));
    go("no verification section", RUN, &f, 0);
    // several scripts: the second fails, the third is not run
    let many: Vec<&str> = [RUN, &["{d}/b.mdio", "{d}/c.mdio"]].concat();
    let mut f = files(leak(chip(&format!("{ready}fail-write 4\n"))));
    f.extend([("init.mdio", ok), ("b.mdio", plain), ("c.mdio", plain)]);
    go("second script fails", &many, &f, 2);
    let mut f = files(leak(chip(ready)));
    f.extend([("init.mdio", ok), ("b.mdio", plain), ("c.mdio", ok)]);
    go("three scripts", &many, &f, 0);
    // the SDK does not come up
    for (how, code) in [
        ("busy", 3),
        ("unit", 4),
        ("soc", 4),
        ("misc", 4),
        ("delay", 4),
        ("divisor", 4),
    ] {
        let mut f = files(leak(format!("init {how}\n")));
        f.push(("init.mdio", ok));
        go(how, RUN, &f, code);
        go(how, &[&["--verify-only"], RUN].concat(), &f, code);
    }
    go("config missing", RUN, &[("init.mdio", ok), ("names", NAMES)], 4);
    go(
        "unit 1",
        &[&["--unit", "1"], RUN].concat(),
        &{
            let mut f = files(leak(chip(ready)));
            f.push(("init.mdio", ok));
            f
        },
        4,
    );
    // read-only with the SDK itself: table and JSON, good and bad
    for extra in ["", "const 1 1900\n", "dead 1\n", "reg 0 1 8b00 1\n"] {
        let mut f = files(leak(chip(&format!("{ready}{extra}"))));
        f.push(("init.mdio", ok));
        let code = if extra.is_empty() { 0 } else { 2 };
        let r = go("verify-only", &[&["--verify-only"], RUN].concat(), &f, code);
        assert!(!r.files.iter().any(|(n, _)| n.starts_with("st/")), "{r:#?}");
        assert!(
            !r.files.iter().any(|(n, t)| n == "trace.txt" && t.contains("\nW ")),
            "{r:#?}"
        );
        go(
            "verify-only json",
            &[&["--verify-only", "--json"], RUN].concat(),
            &f,
            code,
        );
    }
    golden.finish();
    eprintln!(
        "differential: {n} runs on the fake chip agree with the C tool's golden (exit code, stdout, stderr, status files, bus trace)"
    );
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn everything_before_the_chip() {
    let mut golden = Golden::new("everything_before_the_chip");
    let base = scratch("diff-cli");
    let ok: &'static str = Box::leak(script_text(&["0", "1"]).into_boxed_str());
    let mut n = 0;
    for no_sdk in [false, true] {
        let mut go = |name: &str, args: &[&str], files: &[(&str, &str)], code: i32| {
            let r = compare(
                &mut golden,
                &base,
                &Case {
                    name,
                    args,
                    files,
                    no_sdk,
                },
                None,
            );
            assert_eq!(r.code, Some(code), "case '{name}': {r:#?}");
            n += 1;
        };
        let f: &[(&str, &str)] = &[("init.mdio", ok), ("names", NAMES), ("chip.cfg", "")];
        go("check", &["--check", "{d}/init.mdio"], f, 0);
        go("check two", &["-c", "{d}/init.mdio", "{d}/init.mdio"], f, 0);
        go("help", &["--help"], f, 0);
        go("no arguments", &[], f, 1);
        go("no script", &["--check"], f, 1);
        go("check and verify", &["--check", "--verify-only", "{d}/init.mdio"], f, 1);
        go("verify without a bus", &["--verify-only", "{d}/init.mdio"], f, 1);
        go(
            "socket without verify",
            &["--config", "{d}/chip.cfg", "--diag-socket", "/x", "{d}/init.mdio"],
            f,
            1,
        );
        go(
            "prom without out",
            &["--config", "{d}/chip.cfg", "--temp-prom", "/x", "{d}/init.mdio"],
            f,
            1,
        );
        go(
            "interval",
            &["--config", "{d}/chip.cfg", "--temp-interval", "0", "{d}/init.mdio"],
            f,
            1,
        );
        go("hint without =", &["--check", "--hint", "x", "{d}/init.mdio"], f, 1);
        go("unknown option", &["--check", "--bogus", "{d}/init.mdio"], f, 1);
        go("unknown letter", &["-cx", "{d}/init.mdio"], f, 1);
        go("missing argument", &["{d}/init.mdio", "--config"], f, 1);
        go("ambiguous", &["--te", "x", "{d}/init.mdio"], f, 1);
        go("argument not allowed", &["--check=1", "{d}/init.mdio"], f, 1);
        go(
            "nine scripts",
            &["--check", "a", "b", "c", "d", "e", "f", "g", "h", "i"],
            f,
            1,
        );
        // scripts that do not parse: --check, and a run with status files
        for (what, text) in [
            ("empty", ""),
            ("not a script", "mdio-script 2\n"),
            ("truncated", "mdio-script 1\nW 0 1 1 1\nW 0 1 2\n"),
            ("clause 22", "mdio-script 1\nL x\nW 0 0 1 1\n"),
            ("unknown word", "mdio-script 1\nbogus\n"),
            ("unknown letter", "mdio-script 1\nX 1 1 1 1\n"),
            ("bad delay", "mdio-script 1\nD 60000001\n"),
        ] {
            let f: &[(&str, &str)] = &[("init.mdio", ok), ("bad.mdio", text), ("chip.cfg", "")];
            go(what, &["--check", "{d}/init.mdio", "{d}/bad.mdio"], f, 5);
            go(
                what,
                &[
                    "--config",
                    "{d}/chip.cfg",
                    "--status-file",
                    "{d}/st/status.json",
                    "--hint",
                    "script-malformed=install it again, by hand",
                    "{d}/init.mdio",
                    "{d}/bad.mdio",
                ],
                f,
                5,
            );
        }
        go("missing", &["--check", "{d}/missing.mdio"], f, 5);
        go(
            "missing, run",
            &[
                "--config",
                "{d}/chip.cfg",
                "--status-file",
                "{d}/status",
                "{d}/missing.mdio",
            ],
            f,
            5,
        );
        go(
            "names missing",
            &["--check", "--names", "{d}/nonames", "{d}/init.mdio"],
            f,
            0,
        );
        go(
            "names bad",
            &["--check", "--names", "{d}/init.mdio", "{d}/init.mdio"],
            f,
            0,
        );
        go(
            "no verification section",
            &["--verify-only", "--diag-socket", "{d}/diag.sock", "{d}/plain.mdio"],
            &[("plain.mdio", "mdio-script 1\nW 0 1 1 1\n")],
            5,
        );
        go(
            "nobody listens",
            &["--verify-only", "--diag-socket", "{d}/diag.sock", "{d}/init.mdio"],
            f,
            2,
        );
    }
    // a build without the SDK asked to run
    let r = compare(
        &mut golden,
        &base,
        &Case {
            name: "without the SDK",
            args: &["--config", "{d}/chip.cfg", "{d}/init.mdio"],
            files: &[("init.mdio", ok), ("chip.cfg", "")],
            no_sdk: true,
        },
        None,
    );
    assert_eq!(r.code, Some(1));
    assert!(r.stderr.contains("built without the SDK"));
    golden.finish();
    eprintln!(
        "differential: {} command lines that never reach a chip agree with the C tool's golden",
        n + 1
    );
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn verification_through_a_diag_socket() {
    let mut golden = Golden::new("verification_through_a_diag_socket");
    let base = scratch("diff-diag");
    // one server for both tools; its path is outside the masked directories
    let sock = base.join("diag.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let commands = std::sync::Mutex::new(Vec::new());
    let mut n = 0;
    std::thread::scope(|scope| {
        scope.spawn(|| common::diag_server(&listener, &commands));
        let _stop = common::StopOnDrop(&sock);
        for (phys, code) in [
            (&["0", "1"][..], 0),
            (&["0", "24"], 2),
            (&["0", "25"], 2),
            (&["26", "1"], 2),
            (&["24", "25", "26", "0"], 2),
        ] {
            let text: &'static str = Box::leak(script_text(phys).into_boxed_str());
            let f: &[(&str, &str)] = &[("init.mdio", text), ("names", NAMES)];
            for extra in [
                &[][..],
                &["--json"],
                &["--names", "{d}/names"],
                &[
                    "--names",
                    "{d}/names",
                    "--json",
                    "--hint",
                    "phy-not-answering=cold power cycle",
                ],
            ] {
                for no_sdk in [false, true] {
                    let args: Vec<&str> = [
                        &["--verify-only", "--diag-socket", "{b}/diag.sock"],
                        extra,
                        &["{d}/init.mdio"],
                    ]
                    .concat();
                    let before = commands.lock().unwrap().len();
                    let r = compare(
                        &mut golden,
                        &base,
                        &Case {
                            name: "diag",
                            args: &args,
                            files: f,
                            no_sdk,
                        },
                        Some(&commands),
                    );
                    assert_eq!(r.code, Some(code), "{r:#?}");
                    // the C tool's commands (compare checked them), reads only
                    let sent = commands.lock().unwrap()[before..].to_vec();
                    assert!(sent.iter().all(|c| c.starts_with("phy raw c45 0x")), "{sent:?}");
                    n += 1;
                }
            }
        }
    });
    golden.finish();
    eprintln!("differential: {n} read-only verifications through a diag socket agree with the C tool's golden");
    std::fs::remove_dir_all(base).unwrap();
}
