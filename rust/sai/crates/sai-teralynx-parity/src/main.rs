//! `sai-teralynx-parity`: the harness's command line (crate documentation; README "Parity").
//!
//! ```text
//! sai-teralynx-parity --fixtures DIR --work DIR --rust-lib FILE --golden DIR --test-ifcs FILE --sai-script FILE
//!     [--parts checks,sections,oracle,driver] [--only NAME,...] [--strict] [--mutate TEXT]
//!     [--timeout SECONDS] [--driver-scripts DIR] [--oracle-scripts DIR|none]
//!     [--areas FILE] [--expected FILE] [--allow FILE] [--pending FILE]
//! ```
//!
//! The Rust libsai is judged against the golden C++ side (`tests/golden`, module `golden`; the C++
//! libsai it was captured from is deleted, H5). `--test-ifcs` is the Rust test_ifcs the checks and sections parts
//! run (the ifcs_e2e test binary of crates/sai-backend-teralynx), `--sai-script` the Rust `sai-script` program
//! the oracle part runs (`--cpp-names`, the C++ runner's own mode). The default parts are checks, sections and
//! oracle; the driver part needs `bin/tl-frontend-driver` in the fixtures, which nothing builds since H5 (the
//! C++ frontend driver was deleted): asking for it stops the run with exit 2, a part is never skipped.
//!
//! Exit 0: green; 1: red (the report says why); 2: the harness could not run. `--mutate` is the negative
//! control: the Rust side's IFCS lines containing TEXT are marked, the run must be red.
//! Paths default to this checkout (the crate's and sai-backend-teralynx's `tests/`, sai-driver's scripts, the oracle's scripts in ai_re/nix/packages/ifcs-oracle).
#![forbid(unsafe_code)]

use sai_teralynx_parity::run::{self, Config, Part};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

fn here(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("sai-teralynx-parity: {msg}");
    eprintln!(
        "usage: sai-teralynx-parity --fixtures DIR --work DIR --rust-lib FILE --golden DIR --test-ifcs FILE --sai-script FILE [--parts LIST] [--only LIST] [--strict] [--mutate TEXT] [--timeout S] [--driver-scripts DIR] [--oracle-scripts DIR|none] [--areas F] [--expected F] [--allow F] [--pending F]"
    );
    ExitCode::from(2)
}

/// The first path the run needs and the command line did not give.
fn missing(cfg: &Config) -> Option<&'static str> {
    [
        ("--fixtures", &cfg.fixtures),
        ("--work", &cfg.work),
        ("--rust-lib", &cfg.rust_lib),
        ("--golden", &cfg.golden),
    ]
    .into_iter()
    .find(|(_, p)| p.as_os_str().is_empty())
    .map(|(what, _)| what)
}

fn main() -> ExitCode {
    let mut cfg = Config {
        golden: PathBuf::new(),
        fixtures: PathBuf::new(),
        work: PathBuf::new(),
        rust_lib: PathBuf::new(),
        sai_script: PathBuf::new(),
        test_ifcs: PathBuf::new(),
        areas: here("../sai-backend-teralynx/tests/areas.txt"),
        expected: here("../sai-backend-teralynx/tests/test_ifcs-expected.txt"),
        allow: here("tests/parity-allow.txt"),
        pending: here("tests/parity-pending.txt"),
        driver_scripts: here("../sai-driver/tests/driver-scripts"),
        oracle_scripts: Some(here("../../../../ai_re/nix/packages/ifcs-oracle/scripts")),
        strict: false,
        parts: Part::DEFAULT.into_iter().collect(),
        only: Vec::new(),
        mutate: None,
        timeout: Duration::from_secs(300),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().ok_or_else(|| format!("{a} needs a value"));
        let r: Result<(), String> = (|| {
            match a.as_str() {
                "--fixtures" => cfg.fixtures = val()?.into(),
                "--work" => cfg.work = val()?.into(),
                "--rust-lib" => cfg.rust_lib = val()?.into(),
                "--golden" => cfg.golden = val()?.into(),
                "--sai-script" => cfg.sai_script = val()?.into(),
                "--test-ifcs" => cfg.test_ifcs = val()?.into(),
                "--areas" => cfg.areas = val()?.into(),
                "--expected" => cfg.expected = val()?.into(),
                "--allow" => cfg.allow = val()?.into(),
                "--pending" => cfg.pending = val()?.into(),
                "--driver-scripts" => cfg.driver_scripts = val()?.into(),
                "--oracle-scripts" => {
                    let v = val()?;
                    cfg.oracle_scripts = (v != "none").then(|| v.into());
                }
                "--strict" => cfg.strict = true,
                "--mutate" => cfg.mutate = Some(val()?),
                "--timeout" => {
                    cfg.timeout = Duration::from_secs(val()?.parse().map_err(|_| "--timeout: seconds".to_owned())?);
                }
                "--only" => cfg.only = val()?.split(',').map(str::to_owned).collect(),
                "--parts" => {
                    let v = val()?;
                    cfg.parts = v
                        .split(',')
                        .map(|p| Part::parse(p).ok_or_else(|| format!("unknown part {p}")))
                        .collect::<Result<BTreeSet<_>, _>>()?;
                }
                other => return Err(format!("unknown argument {other}")),
            }
            Ok(())
        })();
        if let Err(e) = r {
            return usage(&e);
        }
    }
    if let Some(what) = missing(&cfg) {
        return usage(&format!("{what} is required"));
    }
    match run::run(&cfg) {
        Ok(o) => {
            print!("{}", o.report);
            if o.red.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("sai-teralynx-parity: {e}");
            ExitCode::from(2)
        }
    }
}
