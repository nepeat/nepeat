//! `lab-power`: read and switch the lab's power feeds.
//!
//! Targets (one switchable feed each) and devices (a list of feeds) come from `targets.json`
//! (built in; `LAB_POWER_CONFIG` names another file). Backends: Home Assistant switch entities
//! over its REST API (`HA_BASE`/`HA_KEY` from the environment) and the `NetBotz` 250 switched
//! outlet over its SSH CLI (system `ssh`, password from a private file through askpass).
//! Every call names its target: there is no default, so `lab-power off` is refused.
#![forbid(unsafe_code)]

mod cli;
mod config;
mod ha;
mod http;
mod netbotz;
mod ops;
mod secure;

use std::process::ExitCode;

fn main() -> ExitCode {
    // ssh runs this same binary as its SSH_ASKPASS helper: answer the password prompt and stop.
    if let Some(code) = secure::askpass_mode() {
        return code;
    }
    let mut argv = Vec::new();
    for arg in std::env::args_os().skip(1) {
        let Ok(arg) = arg.into_string() else {
            eprintln!("lab-power: arguments must be UTF-8");
            return ExitCode::from(2);
        };
        argv.push(arg);
    }
    ops::main(&argv)
}
