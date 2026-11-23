//! The binary against the C tool: the unmodified `asic_temp.c` + `temp_files.c` on
//! `tests/c/stub_glue.c` (built by `build.rs`, never at test time) and `openbcm-asic-temp` on
//! `sdk-glue`'s Rust fake chip, the same scenario file and command line for both, each in its
//! own directory with the same relative paths: exit code, stdout, stderr, the files while it runs
//! (the Prometheus read timestamp masked) and after it ended must be equal. Runs that do not end
//! by themselves get SIGTERM / SIGINT / SIGHUP (`kill`) once the first reading is on disk.
//! Both sleep for real (1 s after the discarded read, then the interval): the cases run in
//! parallel.
//!
//! Also the binary alone: the guard of the fake build, the real signals.
//!
//! Where the build tree is not (a copied test executable), `SAI_RS_TEST_BIN` names a directory
//! with `openbcm-asic-temp-c` (from `$OUT_DIR/target-tests`) and `openbcm-asic-temp`. Without
//! the C tool the comparison says so and passes.
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

fn c_tool() -> Option<PathBuf> {
    let bundle = std::env::var_os("SAI_RS_TEST_BIN")
        .map(|d| PathBuf::from(d).join("openbcm-asic-temp-c"))
        .filter(|p| p.is_file());
    let built = Some(env!("ASIC_TEMP_C_HELPERS"))
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join("openbcm-asic-temp-c"));
    bundle.or(built)
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
    let Some(c) = c_tool() else {
        summary(
            "openbcm-asic-temp differential: the C tool was not built (SAI_OPENBCM_SRC at build time, or SAI_RS_TEST_BIN): SKIPPED",
        );
        return;
    };
    let rust = rust_tool();
    let base = scratch("all");
    // "unwritable" would run for ever (no reading ever lands): it is stopped from here after 3 s
    let results: Vec<(&Case, Seen, Seen)> = std::thread::scope(|s| {
        let jobs: Vec<_> = CASES
            .iter()
            .map(|case| {
                let (c, rust, base) = (&c, &rust, &base);
                s.spawn(move || {
                    let one = |tool: &Path, which: &str| {
                        let dir = base.join(format!("{}-{which}", case.name));
                        std::fs::create_dir_all(&dir).unwrap();
                        if case.name == "unwritable" {
                            let child = spawn(tool, &dir, case);
                            std::thread::sleep(Duration::from_secs(3));
                            signal(&child, "TERM");
                            let Output { status, stdout, stderr } = child.wait_with_output().unwrap();
                            return Seen {
                                code: status.code(),
                                stdout: String::from_utf8(stdout).unwrap(),
                                // the number of reads in 3 s may differ by one: compare the set
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
                        }
                        run(tool, &dir, case)
                    };
                    let (a, b) = std::thread::scope(|t| {
                        let a = t.spawn(|| one(c, "c"));
                        let b = t.spawn(|| one(rust, "rust"));
                        (a.join().unwrap(), b.join().unwrap())
                    });
                    (case, a, b)
                })
            })
            .collect();
        jobs.into_iter().map(|j| j.join().unwrap()).collect()
    });
    let mut differ = Vec::new();
    for (case, c, rust) in &results {
        if c != rust {
            differ.push(format!("{}:\n  C:    {c:?}\n  Rust: {rust:?}", case.name));
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        differ.is_empty(),
        "{} of {} cases differ:\n{}",
        differ.len(),
        results.len(),
        differ.join("\n")
    );
    // the cases are not vacuous: what C did, per kind
    let by = |name: &str| &results.iter().find(|r| r.0.name == name).unwrap().1;
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
        "openbcm-asic-temp differential: {} cases (C asic_temp.c on the stub glue vs Rust on the Rust fake): exit code, stdout, stderr, files equal",
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
