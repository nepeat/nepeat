//! The `openbcm-mdio-script-rs` binary: [`mdio_tool::run`] on the process's arguments, standard
//! streams and signals, with the SDK backend when it is built in.
#![forbid(unsafe_code)]

use std::os::unix::ffi::OsStrExt;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[cfg(feature = "sdk")]
fn opener() -> Option<mdio_tool::glue::GlueOpener> {
    // A build on the fake glue touches no hardware: it must not pass for the tool by accident.
    if cfg!(feature = "sdk-fake") && std::env::var_os("MDIO_TOOL_FAKE_SDK").is_none_or(|v| v != "1") {
        return None;
    }
    Some(mdio_tool::glue::GlueOpener)
}

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_else(|| mdio_tool::cli::ME.into());
    let args: Vec<_> = args.collect();
    let stop = Arc::new(AtomicBool::new(false));
    // SIGTERM and SIGINT set the flag; the run looks at it between two script operations.
    let mut arm = || {
        for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
            if let Err(e) = signal_hook::flag::register(signal, Arc::clone(&stop)) {
                eprintln!("{}: cannot handle signal {signal}: {e}", mdio_tool::cli::ME);
            }
        }
    };
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let (mut out, mut err) = (stdout.lock(), stderr.lock());
    #[cfg(feature = "sdk")]
    let mut glue = opener();
    let mut env = mdio_tool::Env {
        out: &mut out,
        err: &mut err,
        stop: &stop,
        arm_signals: &mut arm,
        #[cfg(feature = "sdk")]
        opener: glue.as_mut().map(|g| g as &mut dyn mdio_tool::ChipOpener),
        #[cfg(not(feature = "sdk"))]
        opener: None,
        notify_socket: std::env::var_os("NOTIFY_SOCKET").filter(|s| !s.as_bytes().is_empty()),
        lock_dir: "/run".into(),
    };
    ExitCode::from(mdio_tool::run(&program, &args, &mut env))
}
