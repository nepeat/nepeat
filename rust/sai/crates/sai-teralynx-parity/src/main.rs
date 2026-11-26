//! `sai-teralynx-parity`: the harness's command line (crate documentation; README "Parity").
//!
//! ```text
//! sai-teralynx-parity --fixtures DIR --work DIR --cpp-lib FILE --rust-lib FILE
//!     [--parts checks,sections,driver,oracle] [--only NAME,...] [--strict] [--mutate TEXT]
//!     [--no-repeat] [--timeout SECONDS] [--driver-scripts DIR] [--oracle-scripts DIR|none]
//!     [--areas FILE] [--expected FILE] [--allow FILE] [--pending FILE]
//! ```
//!
//! Exit 0: green; 1: red (the report says why); 2: the harness could not run. `--mutate` is the
//! negative control: the Rust side's IFCS lines containing TEXT are marked, the run must be red.
//! Paths default to this checkout (the crate's and sai-backend-teralynx's `tests/`, the
//! frontend's and the oracle's scripts in ai_re/nix/packages/sai-teralynx).
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
        "usage: sai-teralynx-parity --fixtures DIR --work DIR --cpp-lib FILE --rust-lib FILE [--parts LIST] [--only LIST] [--strict] [--mutate TEXT] [--no-repeat] [--timeout S] [--driver-scripts DIR] [--oracle-scripts DIR|none] [--areas F] [--expected F] [--allow F] [--pending F]"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let tl = here("../../../../ai_re/nix/packages/sai-teralynx");
    let mut cfg = Config {
        fixtures: PathBuf::new(),
        work: PathBuf::new(),
        cpp_lib: PathBuf::new(),
        rust_lib: PathBuf::new(),
        areas: here("../sai-backend-teralynx/tests/areas.txt"),
        expected: here("../sai-backend-teralynx/tests/test_ifcs-expected.txt"),
        allow: here("tests/parity-allow.txt"),
        pending: here("tests/parity-pending.txt"),
        driver_scripts: tl.join("frontend/tests/driver-scripts"),
        oracle_scripts: Some(tl.join("oracle/scripts")),
        strict: false,
        parts: Part::ALL.into_iter().collect(),
        only: Vec::new(),
        mutate: None,
        repeat_reference: true,
        timeout: Duration::from_secs(300),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().ok_or_else(|| format!("{a} needs a value"));
        let r: Result<(), String> = (|| {
            match a.as_str() {
                "--fixtures" => cfg.fixtures = val()?.into(),
                "--work" => cfg.work = val()?.into(),
                "--cpp-lib" => cfg.cpp_lib = val()?.into(),
                "--rust-lib" => cfg.rust_lib = val()?.into(),
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
                "--no-repeat" => cfg.repeat_reference = false,
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
    for (what, p) in [
        ("--fixtures", &cfg.fixtures),
        ("--work", &cfg.work),
        ("--cpp-lib", &cfg.cpp_lib),
        ("--rust-lib", &cfg.rust_lib),
    ] {
        if p.as_os_str().is_empty() {
            return usage(&format!("{what} is required"));
        }
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
