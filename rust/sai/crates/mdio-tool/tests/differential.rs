//! Differential test against the C tool: the same command line, scripts, names and (fake) chip
//! for `openbcm-mdio-script` built from sai-openbcm's C sources and for this crate's binary;
//! exit code, stdout, stderr, `status.json`, `status.txt` and the chip's access trace must be
//! the same bytes once run times, the pid and the directory are masked. The chip: for the C tool
//! `tests/c/mdio_stub.c` (the MDIO side of the glue in C), for the Rust tool sdk-glue's Rust fake;
//! both read the same scenario, so this also holds the stub and the Rust fake to each other.
//!
//! The C tool is built by `build.rs` when `MDIO_SCRIPT_C_DIR` is set at build time (never at
//! test time). Where the build tree is not (a copied test executable), `SAI_RS_TEST_BIN` names
//! a directory with `openbcm-mdio-script-c`, `openbcm-mdio-script-c-fake` (from
//! `$OUT_DIR/target-tests`) and `openbcm-mdio-script-rs` (the binary cargo builds for this
//! test: with the fake glue). Without the C tool the tests say so and pass.
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

struct Tools {
    c: PathBuf,
    c_fake: PathBuf,
    rust: PathBuf,
}

fn tools() -> Option<Tools> {
    let bundle = std::env::var_os("SAI_RS_TEST_BIN").map(PathBuf::from);
    let built = Some(env!("MDIO_TOOL_C_HELPERS"))
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let helpers = bundle
        .clone()
        .filter(|d| d.join("openbcm-mdio-script-c-fake").is_file())
        .or(built);
    let Some(helpers) = helpers else {
        eprintln!(
            "differential: the C tool was not built (MDIO_SCRIPT_C_DIR at build time, or SAI_RS_TEST_BIN), \
             the comparison with mdio_script_main.c is SKIPPED"
        );
        return None;
    };
    let rust = bundle
        .map(|d| d.join("openbcm-mdio-script-rs"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openbcm-mdio-script-rs")));
    Some(Tools {
        c: helpers.join("openbcm-mdio-script-c"),
        c_fake: helpers.join("openbcm-mdio-script-c-fake"),
        rust,
    })
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

/// What differs between two runs of one tool: times, the pid, the directory.
fn mask(bytes: &[u8], dir: &Path) -> String {
    let mut text = String::from_utf8_lossy(bytes).replace(dir.to_str().unwrap(), "<DIR>");
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
    /// `{d}` is replaced by the tool's directory.
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
    for (name, text) in case.files {
        put(dir, name, &text.replace("{d}", d));
    }
    let mut cmd = Command::new(exe);
    cmd.arg0("openbcm-mdio-script")
        .args(case.args.iter().map(|a| a.replace("{d}", d)))
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

fn compare(tools: &Tools, base: &Path, case: &Case<'_>) -> Result_ {
    let c_exe = if case.no_sdk { &tools.c } else { &tools.c_fake };
    let c = run_one(c_exe, &base.join("c"), case, false);
    let rust = run_one(&tools.rust, &base.join("r"), case, !case.no_sdk);
    assert!(
        c == rust,
        "C and Rust disagree in case '{}' ({:?})\n--- C\n{c:#?}\n--- Rust\n{rust:#?}",
        case.name,
        case.args
    );
    c
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
    let Some(tools) = tools() else { return };
    let base = scratch("diff-run");
    let ok = script_text(&["0", "1"]);
    let healthy = "reg 0 1 8b00 2391\nreg 0 1 8b01 80a0\nreg 0 1 8215 d00b\nreg 0 1 8251 2d\n\
                   reg 1 1 8b00 2391\nreg 1 1 8b01 80a0\nreg 1 1 8215 d00b\nreg 1 1 8251 2d\n";
    let chip = |extra: &str| format!("{TRACE}name BCM56960_B1\n{healthy}{extra}");
    let ready = "reg 1 1 8221 303\n";
    let mut n = 0;
    let mut go = |name: &str, args: &[&str], files: &[(&str, &str)], code: i32| {
        let r = compare(
            &tools,
            &base,
            &Case {
                name,
                args,
                files,
                no_sdk: false,
            },
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
    eprintln!(
        "differential: {n} runs on the fake chip agree with the C tool (exit code, stdout, stderr, status files, bus trace)"
    );
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn everything_before_the_chip() {
    let Some(tools) = tools() else { return };
    let base = scratch("diff-cli");
    let ok: &'static str = Box::leak(script_text(&["0", "1"]).into_boxed_str());
    let mut n = 0;
    for no_sdk in [false, true] {
        let mut go = |name: &str, args: &[&str], files: &[(&str, &str)], code: i32| {
            let r = compare(
                &tools,
                &base,
                &Case {
                    name,
                    args,
                    files,
                    no_sdk,
                },
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
        &tools,
        &base,
        &Case {
            name: "without the SDK",
            args: &["--config", "{d}/chip.cfg", "{d}/init.mdio"],
            files: &[("init.mdio", ok), ("chip.cfg", "")],
            no_sdk: true,
        },
    );
    assert_eq!(r.code, Some(1));
    assert!(r.stderr.contains("built without the SDK"));
    eprintln!(
        "differential: {} command lines that never reach a chip agree with the C tool",
        n + 1
    );
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn verification_through_a_diag_socket() {
    let Some(tools) = tools() else { return };
    let base = scratch("diff-diag");
    // one server for both tools; its path is outside the masked directories
    let sock = base.join("diag.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let commands = std::sync::Mutex::new(Vec::new());
    let sock_arg: &'static str = Box::leak(sock.to_str().unwrap().to_owned().into_boxed_str());
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
                    let args: Vec<&str> =
                        [&["--verify-only", "--diag-socket", sock_arg], extra, &["{d}/init.mdio"]].concat();
                    let before = commands.lock().unwrap().len();
                    let r = compare(
                        &tools,
                        &base,
                        &Case {
                            name: "diag",
                            args: &args,
                            files: f,
                            no_sdk,
                        },
                    );
                    assert_eq!(r.code, Some(code), "{r:#?}");
                    // both tools sent the same commands, reads only
                    let sent = commands.lock().unwrap()[before..].to_vec();
                    let (c, rust) = sent.split_at(sent.len() / 2);
                    assert_eq!(c, rust);
                    assert!(sent.iter().all(|c| c.starts_with("phy raw c45 0x")), "{sent:?}");
                    n += 1;
                }
            }
        }
    });
    eprintln!("differential: {n} read-only verifications through a diag socket agree with the C tool");
    std::fs::remove_dir_all(base).unwrap();
}
