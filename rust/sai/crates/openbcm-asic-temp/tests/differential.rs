//! The binary against the C tool's golden: the unmodified `asic_temp.c` + `temp_files.c` on the
//! stub glue `tests/c/stub_glue.c` (a C model of the glue functions the tool calls, answering as
//! `sdk-glue`'s Rust fake chip) and `openbcm-asic-temp` on that Rust fake chip, the same scenario
//! file and command line, each in its own directory with the same relative paths: exit code,
//! stdout, stderr, the files while it runs (the Prometheus read timestamp masked) and after it
//! ended must be what the C tool gave. Runs that do not end by themselves get SIGTERM / SIGINT /
//! SIGHUP (`kill`) once the first reading is on disk. The tool sleeps for real (1 s after the
//! discarded read, then the interval): the cases run in parallel.
//!
//! Until 2026-11-23 `build.rs` built the C tool and this test ran it next to the binary (30
//! cases, all equal). Its results, captured at commit 50a6aa7 (sha256 of the C files in the
//! header; the stub glue was deleted afterwards and is in git there), are
//! `golden/differential.txt`, one record per case with its arguments and a digest of its scenario
//! and stale files, so an edited case shows as "golden inputs differ". The golden is not
//! regenerated from this crate's output: a deliberate behaviour change edits the record by hand
//! with a reason.
//!
//! Also the binary alone: the guard of the fake build, the real signals.
//!
//! Where the build tree is not (a copied test executable), `SAI_RS_TEST_BIN` may name a directory
//! with `openbcm-asic-temp`.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    missing_docs,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn rust_tool() -> PathBuf {
    std::env::var_os("SAI_RS_TEST_BIN")
        .map(|d| PathBuf::from(d).join("openbcm-asic-temp"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openbcm-asic-temp")))
}

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("asic-temp-d-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// How a run ends.
#[derive(Clone, Copy, Debug)]
enum End {
    /// by itself
    Exits,
    /// this signal once FILE (the first word after --out) has a reading
    Signal(&'static str),
}

struct Case {
    name: &'static str,
    scenario: &'static str,
    args: &'static [&'static str],
    end: End,
    /// files made before the run (relative path, content): stale readings
    stale: &'static [(&'static str, &'static str)],
}

/// What one run left: exit code, streams, the files on disk before the signal and after the end.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    during: Vec<(String, Option<String>)>,
    after: Vec<(String, Option<String>)>,
}

const WATCH: &[&str] = &[
    "asic/temperature",
    "asic/temperature.tmp",
    "prom/asic.prom",
    "prom/asic.prom.tmp",
    "a/b/t",
];

fn files(dir: &Path) -> Vec<(String, Option<String>)> {
    WATCH
        .iter()
        .map(|f| {
            let text = std::fs::read_to_string(dir.join(f)).ok().map(|t| {
                // the read timestamp differs between two runs
                t.lines()
                    .map(|l| {
                        if l.starts_with("switch_asic_temperature_read_timestamp_seconds{") {
                            let (head, _) = l.rsplit_once(' ').unwrap();
                            format!("{head} T\n")
                        } else {
                            format!("{l}\n")
                        }
                    })
                    .collect()
            });
            ((*f).to_owned(), text)
        })
        .collect()
}

fn spawn(tool: &Path, dir: &Path, case: &Case) -> Child {
    std::fs::write(dir.join("scenario"), case.scenario).unwrap();
    for (f, text) in case.stale {
        let p = dir.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    Command::new(tool)
        .arg0("openbcm-asic-temp")
        .args(case.args)
        .current_dir(dir)
        .env("OPENBCM_ASIC_TEMP_FAKE_SCENARIO", "scenario")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn signal(child: &Child, sig: &str) {
    let ok = Command::new("kill")
        .args([format!("-{sig}"), child.id().to_string()])
        .status()
        .unwrap()
        .success();
    assert!(ok, "kill -{sig}");
}

fn run(tool: &Path, dir: &Path, case: &Case) -> Seen {
    let child = spawn(tool, dir, case);
    let mut during = Vec::new();
    if let End::Signal(sig) = case.end {
        let out = case.args.iter().skip_while(|a| **a != "--out").nth(1).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while std::fs::metadata(dir.join(out)).map_or(true, |m| m.len() == 0) {
            assert!(Instant::now() < deadline, "{}: no reading after 30 s", case.name);
            std::thread::sleep(Duration::from_millis(20));
        }
        // the prom file is written after FILE: give it its moment
        std::thread::sleep(Duration::from_millis(200));
        during = files(dir);
        signal(&child, sig);
    }
    let Output { status, stdout, stderr } = child.wait_with_output().unwrap();
    Seen {
        code: status.code(),
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
        during,
        after: files(dir),
    }
}

const GOLDEN: &str = include_str!("golden/differential.txt");

/// 64-bit FNV-1a.
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The digest of a case's input files: the scenario, then each stale file (path NUL content NUL).
fn inputs_digest(case: &Case) -> u64 {
    let mut bytes = case.scenario.as_bytes().to_vec();
    bytes.push(0);
    for (path, text) in case.stale {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
    }
    fnv1a64(&bytes)
}

/// What identifies a case in the golden.
fn identity(i: usize, case: &Case) -> String {
    format!(
        "case {i} {}\nargs {:?}\nends {:?}\ninputs {:016x}",
        case.name,
        case.args,
        case.end,
        inputs_digest(case)
    )
}

/// The lines of a `|`-prefixed block, up to the next line that is not one; a following
/// `\ no newline at end` takes the last newline away.
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

/// `golden/differential.txt`: per case `case <i> <name>`, `args` (Rust `{:?}`), `ends`, `inputs`
/// (FNV-1a), `code`, the blocks `stdout` and `stderr`, per watched file `during <path>` /
/// `after <path>` with its block or `during-absent` / `after-absent <path>`, `end`; `#` lines
/// between records are comments. The identity of each record and what the C tool did.
fn golden() -> Vec<(String, Seen)> {
    let mut records = Vec::new();
    let mut lines = GOLDEN.lines().peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        assert!(line.starts_with("case "), "golden: expected a case line, not {line:?}");
        let mut id = vec![line.to_owned()];
        for key in ["args ", "ends ", "inputs "] {
            let l = lines.next().unwrap_or_default();
            assert!(l.starts_with(key), "golden {line}: expected {key}, not {l:?}");
            id.push(l.to_owned());
        }
        let code = lines
            .next()
            .and_then(|l| l.strip_prefix("code "))
            .unwrap_or_else(|| panic!("golden {line}: no code"));
        let mut seen = Seen {
            code: (code != "none").then(|| code.parse().unwrap()),
            stdout: String::new(),
            stderr: String::new(),
            during: Vec::new(),
            after: Vec::new(),
        };
        loop {
            let l = lines.next().unwrap_or_else(|| panic!("golden {line}: no end"));
            let (word, path) = l.split_once(' ').unwrap_or((l, ""));
            match word {
                "end" => break,
                "stdout" => seen.stdout = block(&mut lines),
                "stderr" => seen.stderr = block(&mut lines),
                "during" => seen.during.push((path.to_owned(), Some(block(&mut lines)))),
                "after" => seen.after.push((path.to_owned(), Some(block(&mut lines)))),
                "during-absent" => seen.during.push((path.to_owned(), None)),
                "after-absent" => seen.after.push((path.to_owned(), None)),
                _ => panic!("golden {line}: unexpected {l:?}"),
            }
        }
        records.push((id.join("\n"), seen));
    }
    records
}

const FILE: &str = "asic/temperature";

const CASES: &[Case] = &[
    Case {
        name: "usage-none",
        scenario: "",
        args: &[],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-help",
        scenario: "",
        args: &["--help"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-invalid",
        scenario: "",
        args: &["-x"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-ambiguous",
        scenario: "",
        args: &["--o", "f"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-missing",
        scenario: "",
        args: &["--once", "--unit"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-noarg",
        scenario: "",
        args: &["--once=1"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-unknown",
        scenario: "",
        args: &["--bogus"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-interval",
        scenario: "",
        args: &["--out", FILE, "-i", "0"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-limit",
        scenario: "",
        args: &["--once", "-s", "151"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "usage-prom-only",
        scenario: "",
        args: &["--prom", "p"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "busy",
        scenario: "init busy",
        args: &["--once"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "unit",
        scenario: "temp 400",
        args: &["--once", "-u", "1"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "init-unit",
        scenario: "init unit",
        args: &["--once"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "init-attach",
        scenario: "init attach",
        args: &["--once"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "bad-scenario",
        scenario: "temp 400\nbogus\n",
        args: &["--once"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "once",
        scenario: "temp 413 418 -55 0 7",
        args: &["--once"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "once-ties",
        scenario: "temp 5 0",
        args: &["-1"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "once-unavail",
        scenario: "",
        args: &["--once", "--out", FILE],
        end: End::Exits,
        stale: &[(FILE, "1.0\n")],
    },
    Case {
        name: "once-implausible",
        scenario: "temp 1501",
        args: &["--once"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "run-term",
        scenario: "temp 413 418 -55",
        args: &["--interval", "1", "--out", FILE, "--prom", "prom/asic.prom"],
        end: End::Signal("TERM"),
        stale: &[],
    },
    Case {
        name: "run-int",
        scenario: "temp 400",
        args: &["--out", FILE, "-i", "3600"],
        end: End::Signal("INT"),
        stale: &[],
    },
    Case {
        name: "run-hup",
        scenario: "temp 1490",
        args: &["-s", "0", "--out", FILE],
        end: End::Signal("HUP"),
        stale: &[],
    },
    Case {
        name: "fails",
        scenario: "temp 400\ntemp-fail 2",
        args: &["-i", "1", "--out", FILE, "--prom", "prom/asic.prom"],
        end: End::Exits,
        stale: &[(FILE, "50.0\n"), ("prom/asic.prom", "old\n")],
    },
    Case {
        name: "fails-after-one",
        scenario: "temp 400\ntemp-fail 3",
        args: &["-i", "1", "--out", FILE],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "implausible",
        scenario: "temp 400 -401",
        args: &["-i", "1", "--out", FILE],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "unavail",
        scenario: "",
        args: &["-i", "1", "--out", FILE],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "overtemp",
        scenario: "temp 400 1250",
        args: &["-i", "1", "--out", FILE, "--prom", "prom/asic.prom"],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "overtemp-fail",
        scenario: "temp 1300\nfail overtemp 1 -16",
        args: &["-i", "1", "--out", FILE],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "overtemp-at",
        scenario: "temp 1000",
        args: &["-i", "1", "-s", "100", "--out", FILE],
        end: End::Exits,
        stale: &[],
    },
    Case {
        name: "unwritable",
        scenario: "temp 400",
        args: &[
            "-i",
            "1",
            "--out",
            "a/b/t",
            "--prom",
            "plain/asic.prom",
            "--",
            "ignored",
        ],
        end: End::Exits,
        stale: &[("plain", "")],
    },
];

#[test]
fn the_rust_tool_behaves_like_asic_temp_c() {
    let golden = golden();
    assert_eq!(
        golden.len(),
        CASES.len(),
        "the golden has {} cases, the test {}: golden inputs differ (the cases were edited)",
        golden.len(),
        CASES.len()
    );
    for (i, (case, (id, _))) in CASES.iter().zip(&golden).enumerate() {
        let this = identity(i, case);
        assert!(
            *id == this,
            "golden case {i} was made for another case: golden inputs differ (the cases were edited)\n--- golden\n{id}\n--- this case\n{this}"
        );
    }
    let rust = rust_tool();
    let base = scratch("all");
    // "unwritable" would run for ever (no reading ever lands): it is stopped from here after 3 s
    let results: Vec<(&Case, Seen)> = std::thread::scope(|s| {
        let jobs: Vec<_> = CASES
            .iter()
            .map(|case| {
                let (rust, base) = (&rust, &base);
                s.spawn(move || {
                    let dir = base.join(format!("{}-rust", case.name));
                    std::fs::create_dir_all(&dir).unwrap();
                    if case.name == "unwritable" {
                        let child = spawn(rust, &dir, case);
                        std::thread::sleep(Duration::from_secs(3));
                        signal(&child, "TERM");
                        let Output { status, stdout, stderr } = child.wait_with_output().unwrap();
                        let seen = Seen {
                            code: status.code(),
                            stdout: String::from_utf8(stdout).unwrap(),
                            // the number of reads in 3 s varies: compare the set
                            stderr: {
                                let mut l: Vec<&str> = std::str::from_utf8(&stderr).unwrap().lines().collect();
                                l.dedup();
                                l.sort_unstable();
                                l.dedup();
                                l.join("\n")
                            },
                            during: Vec::new(),
                            after: files(&dir),
                        };
                        return (case, seen);
                    }
                    (case, run(rust, &dir, case))
                })
            })
            .collect();
        jobs.into_iter().map(|j| j.join().unwrap()).collect()
    });
    let mut differ = Vec::new();
    for ((case, rust), (_, c)) in results.iter().zip(&golden) {
        if c != rust {
            differ.push(format!("{}:\n  C (golden): {c:?}\n  Rust:       {rust:?}", case.name));
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        differ.is_empty(),
        "{} of {} cases differ from the C tool's golden:\n{}",
        differ.len(),
        results.len(),
        differ.join("\n")
    );
    // the cases are not vacuous: what C did, per kind
    let by = |name: &str| &golden[CASES.iter().position(|c| c.name == name).unwrap()].1;
    assert_eq!(by("overtemp").code, Some(5));
    assert_eq!(by("fails").code, Some(4));
    assert_eq!(by("busy").code, Some(3));
    assert_eq!(by("run-term").code, Some(0));
    assert!(
        by("run-term")
            .during
            .iter()
            .any(|(f, t)| f == FILE && t.as_deref() == Some("41.8\n"))
    );
    assert!(by("run-term").stderr.ends_with("openbcm-asic-temp: stopped\n"));
    summary(&format!(
        "openbcm-asic-temp differential: {} cases (Rust on the Rust fake vs the golden of C asic_temp.c on the stub glue, commit 50a6aa7): exit code, stdout, stderr, files equal",
        results.len()
    ));
}

#[test]
fn the_fake_build_needs_its_scenario() {
    let o = Command::new(rust_tool())
        .arg("--once")
        .env_remove("OPENBCM_ASIC_TEMP_FAKE_SCENARIO")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(o.stderr).unwrap(),
        "openbcm-asic-temp: this build drives a fake chip (feature sdk-fake); OPENBCM_ASIC_TEMP_FAKE_SCENARIO names its scenario (not attempted)\n"
    );
}

#[test]
fn a_signal_ends_the_sleep_at_once() {
    let dir = scratch("signal");
    let case = Case {
        name: "signal",
        scenario: "temp 400",
        args: &["--out", FILE, "--interval", "3600"],
        end: End::Signal("TERM"),
        stale: &[],
    };
    let t = Instant::now();
    let seen = run(&rust_tool(), &dir, &case);
    assert_eq!(seen.code, Some(0), "{}", seen.stderr);
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    assert!(seen.after.iter().all(|(_, t)| t.is_none()));
    let _ = std::fs::remove_dir_all(&dir);
}
