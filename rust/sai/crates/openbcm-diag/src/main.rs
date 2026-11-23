//! `openbcm-diag-rs`: see the library.
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let socket = std::env::var_os(openbcm_diag::SOCKET_ENV);
    let code = openbcm_diag::run(
        &args,
        socket.as_deref(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
