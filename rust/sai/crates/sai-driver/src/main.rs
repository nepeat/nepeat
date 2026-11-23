//! `sai-rs-frontend-driver [--queries] [script|-]`: runs an op script against the Rust SAI core on
//! its virtual backend and prints what sai-openbcm's C++ `frontend_driver` prints (crate
//! documentation). Without arguments, or with `-`, the script is read from stdin.
#![forbid(unsafe_code)]

use sai_driver::{Driver, LogMode, OnHardware, Options};
use std::io::{BufRead as _, Write as _};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut options = Options::default();
    let mut path: Option<String> = None;
    let mut args = std::env::args();
    let me = args.next().unwrap_or_else(|| "sai-rs-frontend-driver".to_owned());
    for a in args {
        if a == "--queries" {
            options.log = LogMode::Full;
        } else if path.is_none() {
            path = Some(a);
        } else {
            eprintln!("usage: {me} [--queries] [script]");
            return ExitCode::from(2);
        }
    }
    if std::env::var("SAI_RS_VIRTUAL_ON_HARDWARE").is_ok_and(|v| v == "1") {
        options.on_hardware = OnHardware::Yes;
    }
    let input: Box<dyn std::io::BufRead> = match path.as_deref() {
        None | Some("-") => Box::new(std::io::stdin().lock()),
        Some(p) => {
            let Ok(f) = std::fs::File::open(p) else {
                eprintln!("{me}: cannot read {p}");
                return ExitCode::from(2);
            };
            Box::new(std::io::BufReader::new(f))
        }
    };
    let mut driver = Driver::new(options);
    let mut out = std::io::stdout().lock();
    for line in input.lines() {
        let Ok(line) = line else {
            eprintln!("{me}: read error");
            return ExitCode::from(2);
        };
        if let Some(text) = driver.run_line(&line) {
            // flushed per operation, as the C++ driver does
            if out.write_all(text.as_bytes()).and_then(|()| out.flush()).is_err() {
                return ExitCode::from(1);
            }
        }
    }
    ExitCode::SUCCESS
}
