//! `openbcm-ext-phy-rs`: see [`phy_seq::cli`].
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let code = phy_seq::cli::ext_phy(&args, &mut std::io::stdout().lock(), &mut std::io::stderr().lock());
    ExitCode::from(code)
}
