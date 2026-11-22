//! Differential test against the C implementation.
//!
//! With `MDIO_SCRIPT_C_DIR` set to the directory of `mdio_script.c` and `mdio_script.h`, the
//! driver `tests/differential_driver.c` is compiled with them (`cc`, or `$CC`) and run on a corpus
//! of scripts; its canonical output must equal what this crate produces on the same fake bus
//! (tests/common/mod.rs). Without the variable the tests say so and pass.
//!
//! Scripts with a syntax error are compared on the parse pass only: the C interpreter would run
//! the lines before the error, this crate runs nothing (see the crate documentation).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use common::{Job, Mode, rust_output};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

struct Driver {
    exe: PathBuf,
    dir: PathBuf,
}

/// The compiled C driver, or `None` (said once on stderr) when `MDIO_SCRIPT_C_DIR` is not set.
fn driver() -> Option<&'static Driver> {
    static DRIVER: OnceLock<Option<Result<Driver, String>>> = OnceLock::new();
    let built = DRIVER.get_or_init(|| {
        let Some(c_dir) = std::env::var_os("MDIO_SCRIPT_C_DIR") else {
            eprintln!("differential: MDIO_SCRIPT_C_DIR is not set, the comparison with mdio_script.c is SKIPPED");
            return None;
        };
        Some(build(Path::new(&c_dir)))
    });
    built
        .as_ref()
        .map(|built| built.as_ref().unwrap_or_else(|e| panic!("{e}")))
}

fn build(c_dir: &Path) -> Result<Driver, String> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mdio-script-differential-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let exe = dir.join("driver");
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let out = Command::new(&cc)
        .args(["-O1", "-Wall", "-I"])
        .arg(c_dir)
        .arg("-o")
        .arg(&exe)
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/differential_driver.c"))
        .arg(c_dir.join("mdio_script.c"))
        .output()
        .map_err(|e| format!("cannot run the C compiler {}: {e}", cc.display()))?;
    if !out.status.success() {
        return Err(format!(
            "the C driver does not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(Driver { exe, dir })
}

impl Driver {
    fn c_output(&self, script: &Path, job: Job) -> String {
        let out = Command::new(&self.exe)
            .arg(script)
            .args([
                job.mode.arg(),
                &job.seed.to_string(),
                if job.tick { "1" } else { "0" },
                &job.abort_k.to_string(),
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "driver failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// Runs both implementations on `data` for every job; the number of comparisons made.
    fn compare(&self, data: &[u8], jobs: &[Job]) -> Result<u64, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = self.dir.join(format!("s{}.mdio", NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, data).unwrap();
        let mut done = 0;
        for &job in jobs {
            let (c, rust) = (self.c_output(&path, job), rust_output(data, job));
            if c != rust {
                return Err(format!(
                    "C and Rust disagree for {job:?} (script kept: {})\n--- script\n{}\n--- C\n{c}--- Rust\n{rust}",
                    path.display(),
                    String::from_utf8_lossy(data)
                ));
            }
            done += 1;
        }
        std::fs::remove_file(&path).unwrap();
        Ok(done)
    }
}

fn verify(phy: &str) -> String {
    format!(
        "L verify phy 0x{phy}\nP {phy} 1 8b00 0 3 1 0\nN {phy} 1 2 1 ffff 1 0\n# note\nP {phy} 1f ffff 0303 0303 1 0\n"
    )
}

/// Hand-written scripts: valid ones, each error class, verification sections.
fn corpus() -> Vec<String> {
    let body = "mdio-script 1\nL init\nW 0 1 8201 7e\nW 1 1 8201 7e\nD 10\nP 1 1 8221 0303 ffff 9 10\nW 1 1 1 1\n";
    let mut scripts: Vec<String> = [
        // valid, no verification section
        "mdio-script 1\n",
        "mdio-script 1",
        "mdio-script 1\r\n# c\r\nL step one\r\nW 17c 1 8201 7e\r\nD 30\r\nR 17c 1 900a\r\n",
        "mdio-script 1\n# c\nL step one\nW 17c 1 8201 7e\nD 30\nR 17c 1 900a\nE 17c 1 8b00 2391\nE 17c 1 8b00 0391 0fff\n\
         E 17c 1 8b00 1111\n\nW 0 1f ffff ffff\nE 0 1f ffff ffff\nE 0 1f ffff 00ff 00ff\n",
        "mdio-script 1\nL\nL \nL  spaced  \nW 0x1 0X1f 0002 3\n \t W\t1  1 2\t4 \nR 1 1 2\nE 1 1 2 4\nE 1 1 2 5\n",
        "mdio-script 1\nD 0\nD 499999\nD 500000\nD 500001\nD 1700000\nD 60000000\nW 1 1 2 3\n",
        "mdio-script 1\nP 24 1 8221 0303 ffff 10 1000\nN 24 1 8221 0303 ffff 3 500\nP 1 1 8221 0303 0303 1 0\nW 1 1 2 3\n",
        "mdio-script 1\nL slow poll\nP 1 1 8221 0303 ffff 9 1200000\nN 1 1 8221 0303 ffff 2 0\nW 1 1 2 3\n",
        // E_POLL, E_IO (read, write, inside a poll), mismatch counting beyond the 20 logged
        "mdio-script 1\nL download bus 1\nP 24 1 8b00 0303 ffff 5 10\nW 24 1 1 1\n",
        "mdio-script 1\nL never\nN 24 1 2 1900 ffff 4 600000\nW 24 1 1 1\n",
        "mdio-script 1\nL assert\nW 1 1 2 3\nP 1 1 2 4 ffff 1 0\nW 1 1 1 1\n",
        "mdio-script 1\nW 1 1 2 3\nR 24 1 dead\nW 24 1 1 1\n",
        "mdio-script 1\nL e\nE 26 1 2 3 ff\nW 24 1 1 1\n",
        "mdio-script 1\nL w\nW 1 1 2 3\nW 1 3 bad0 77\nW 24 1 1 1\n",
        "mdio-script 1\nL p\nP 26 1 2 3 ffff 5 10\nW 24 1 1 1\n",
        // syntax errors (parse pass only)
        "",
        "\n",
        "W 1 1 2 3\n",
        "mdio-script 2\n",
        "mdio-script 1\nW 1 0 2 3\n",
        "mdio-script 1\nW 1 20 2 3\n",
        "mdio-script 1\nW 1 1 2\n",
        "mdio-script 1\nW 1 1 2 10000\n",
        "mdio-script 1\nW 1 1 2 3 4\n",
        "mdio-script 1\nX 1 1 2 3\n",
        "mdio-script 1\nP 1 1 2 3 ffff 0 5\n",
        "mdio-script 1\nP 1 1 2 3 ffff 1000001 5\n",
        "mdio-script 1\nP 1 1 2 3 ffff 5 60000001\n",
        "mdio-script 1\nP 1 1 2 3 ffff 5 6 7 8 9 10\n",
        "mdio-script 1\nD -5\n",
        "mdio-script 1\nD +5\n",
        "mdio-script 1\nD 60000001\n",
        "mdio-script 1\nD 0x10\n",
        "mdio-script 1\nW 0x 1 2 3\n",
        "mdio-script 1\nW 1 1 2 3\nW 1 1 zz 3\n",
        "mdio-script 1\nL a\nW 1 1 2 3\nD 5\nbogus\nW 1 1 4 5\n",
        "mdio-script 1\nE 1 1 2\n",
        "mdio-script 1\nE 1 1 2 3 4 5\n",
        "mdio-script 1\nL\tx\n",
        "mdio-script 1\n # c\n",
        "mdio-script 1\nW 1 1 2 3\0\nW 1 1 2 4\n",
        "mdio-script 1\n\0W 1 1 2 3\nW 1 1 2 4\n",
        "mdio-script 1\nW 1 1 2 3\0",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    // 25 mismatches: 20 logged
    scripts.push(format!("mdio-script 1\nL many\n{}", "E 24 1 2 5 ff\n".repeat(25)));
    // long label (cut to 150 bytes; 60 in the verification message), long lines
    scripts.push(format!(
        "mdio-script 1\nL {}\nP 24 1 2 5 ffff 2 0\n",
        "label ".repeat(40)
    ));
    scripts.push(format!(
        "mdio-script 1\nL {}\nP 24 1 2 5 ffff 1 0\nP 26 1 2 5 ffff 1 0\n",
        "label ".repeat(40)
    ));
    for n in [509, 510, 511, 512, 1021, 1022] {
        scripts.push(format!("mdio-script 1\nW 1 1 2 3\n#{}\nW 1 1 2 4\n", "c".repeat(n - 1)));
        scripts.push(format!("mdio-script 1\nW 1 1 2 3\n#{}", "c".repeat(n - 1)));
        scripts.push(format!("mdio-script 1\nW 1 1 2 3\n#{}\r\n", "c".repeat(n - 1)));
    }
    // verification sections: all good depends on the seed; hung PHY 24; failing PHY 26; mixed
    for phys in [
        &["0"][..],
        &["0", "1"],
        &["24"],
        &["26"],
        &["0", "24", "25", "26"],
        &["26", "24", "1"],
    ] {
        let section: String = phys.iter().map(|p| verify(p)).collect();
        scripts.push(format!("{body}{section}"));
        scripts.push(format!("mdio-script 1\n{section}"));
        scripts.push(format!("{body}{section}L trailing label\n# and a comment\n"));
    }
    // assertions in the middle are not the section; a multi-try poll ends it
    scripts.push(format!("{body}{}W 1 1 2 3\n{}", verify("0"), verify("1")));
    scripts.push(format!("{body}{}P 1 1 2 3 ffff 2 0\n", verify("0")));
    scripts.push(format!("{body}{}D 5\n", verify("24")));
    scripts
}

fn all_jobs(parse_only: bool) -> Vec<Job> {
    let mut jobs = vec![Job {
        mode: Mode::Parse,
        seed: 0,
        tick: false,
        abort_k: 0,
    }];
    if parse_only {
        return jobs;
    }
    for mode in Mode::RUNS {
        for seed in [0, 1, 3, 6, 12, 99] {
            jobs.push(Job {
                mode,
                seed,
                tick: false,
                abort_k: 0,
            });
            jobs.push(Job {
                mode,
                seed,
                tick: true,
                abort_k: 0,
            });
        }
        for abort_k in 1..=24 {
            jobs.push(Job {
                mode,
                seed: 5,
                tick: true,
                abort_k,
            });
        }
    }
    jobs
}

#[test]
fn hand_written_corpus() {
    let Some(driver) = driver() else { return };
    let (mut scripts, mut compared) = (0u64, 0u64);
    for text in corpus() {
        let parse_only = mdio_script::Script::parse(&text).is_err();
        compared += driver
            .compare(text.as_bytes(), &all_jobs(parse_only))
            .unwrap_or_else(|e| panic!("{e}"));
        scripts += 1;
    }
    eprintln!("differential: hand-written corpus: {scripts} scripts, {compared} comparisons with the C code agree");
}

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// Runs `check` on generated values; `None` (and nothing run) without the C driver.
fn generated<S: Strategy>(
    cases: u32,
    strategy: &S,
    check: impl Fn(&Driver, S::Value) -> Result<u64, String>,
) -> Option<u64> {
    let driver = driver()?;
    let compared = AtomicU64::new(0);
    let mut runner = proptest::test_runner::TestRunner::new(config(cases));
    let run = runner.run(strategy, |value| {
        compared.fetch_add(check(driver, value).map_err(TestCaseError::fail)?, Ordering::Relaxed);
        Ok(())
    });
    if let Err(e) = run {
        panic!("{e}");
    }
    Some(compared.into_inner())
}

#[test]
fn generated_valid_scripts() {
    let compared = generated(
        400,
        &(common::valid_script(), common::jobs()),
        |driver, (text, (seed, tick, abort_k))| {
            if let Err(e) = mdio_script::Script::parse(&text) {
                return Err(format!("the generator made an invalid script: {e}\n{text}"));
            }
            let mut jobs = vec![Job {
                mode: Mode::Parse,
                seed,
                tick,
                abort_k,
            }];
            jobs.extend(Mode::RUNS.map(|mode| Job {
                mode,
                seed,
                tick,
                abort_k,
            }));
            driver.compare(text.as_bytes(), &jobs)
        },
    );
    if let Some(n) = compared {
        eprintln!("differential: 400 generated valid scripts, {n} comparisons (parse, run, verify, verify-only) agree");
    }
}

#[test]
fn generated_damaged_scripts() {
    let strategy = prop_oneof![4 => common::corrupted_script(), 1 => common::noise()];
    // The parse pass only: a damaged script that the C code would half run is never run here.
    let compared = generated(600, &strategy, |driver, data| driver.compare(&data, &all_jobs(true)));
    if let Some(n) = compared {
        eprintln!("differential: 600 generated damaged scripts, {n} parse-pass comparisons agree");
    }
}
