//! The `openbcm-asic-temp` binary: [`openbcm_asic_temp::run`] on the process's arguments,
//! standard streams, signals and clock, attached through the glue this build links. A build on
//! the fake chip (feature `sdk-fake`) attaches only to the scenario file named by
//! `OPENBCM_ASIC_TEMP_FAKE_SCENARIO`: without it, it refuses (exit 2), so it cannot pass for the
//! tool by accident.
#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use openbcm_asic_temp::glue::GlueAttacher;
use openbcm_asic_temp::{AttachError, Attacher, Clock, Env, ME, Sensor};

/// The process's clock; the stop signals set `stop` and write to `wake`'s other end.
struct Process {
    stop: Arc<AtomicBool>,
    wake: Option<UnixStream>,
}

impl Clock for Process {
    fn settle(&mut self) {
        std::thread::sleep(Duration::from_secs(1));
    }

    fn arm(&mut self, err: &mut dyn Write) {
        let pair = UnixStream::pair();
        for signal in [
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGHUP,
        ] {
            // the flag first: a woken sleep then sees it set
            if let Err(e) = signal_hook::flag::register(signal, Arc::clone(&self.stop)) {
                let _ = writeln!(err, "{ME}: cannot handle signal {signal}: {e}");
            }
            if let Ok((_, writer)) = &pair {
                if let Err(e) = writer
                    .try_clone()
                    .and_then(|w| signal_hook::low_level::pipe::register(signal, w))
                {
                    let _ = writeln!(err, "{ME}: cannot wake on signal {signal}: {e}");
                }
            }
        }
        self.wake = pair.ok().map(|(reader, _)| reader);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn wait(&mut self, seconds: u32) {
        let deadline = Instant::now() + Duration::from_secs(u64::from(seconds));
        while !self.stopped() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            match &self.wake {
                // a signal's byte (or the timeout) ends the read; spurious wake-ups loop
                Some(reader) if reader.set_read_timeout(Some(left)).is_ok() => {
                    let _ = (&mut &*reader).read(&mut [0u8; 16]);
                }
                _ => std::thread::sleep(left.min(Duration::from_millis(100))),
            }
        }
    }

    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }
}

/// The glue's attacher: on the fake chip, the scenario file's.
#[cfg(feature = "sdk-fake")]
fn attacher() -> Result<GlueAttacher, AttachError> {
    let refuse = |message: String, code_text: &str| AttachError {
        busy: false,
        message,
        code_text: code_text.to_owned(),
    };
    let Some(path) = std::env::var_os("OPENBCM_ASIC_TEMP_FAKE_SCENARIO") else {
        return Err(refuse(
            "this build drives a fake chip (feature sdk-fake); OPENBCM_ASIC_TEMP_FAKE_SCENARIO names its scenario"
                .to_owned(),
            "not attempted",
        ));
    };
    let text = std::fs::read_to_string(&path).map_err(|e| {
        refuse(
            format!("fake scenario {}: {e}", path.to_string_lossy()),
            "not attempted",
        )
    })?;
    let chip = sdk_glue::rust_fake::Chip::new();
    chip.load(&text).map_err(|n| {
        refuse(
            format!("fake glue: {}:{n}: not a scenario line", path.to_string_lossy()),
            "Invalid configuration",
        )
    })?;
    Ok(GlueAttacher::on(chip))
}

/// The glue's attacher: the glue this program links.
#[cfg(not(feature = "sdk-fake"))]
#[allow(clippy::unnecessary_wraps, reason = "the signature of the fake build's")]
fn attacher() -> Result<GlueAttacher, AttachError> {
    Ok(GlueAttacher::process())
}

/// An attacher that only refuses (the fake build without its scenario).
struct Refuse(Option<AttachError>);

impl Attacher for Refuse {
    fn attach(&mut self, _unit: i32) -> Result<(Box<dyn Sensor>, String), AttachError> {
        Err(self.0.take().unwrap_or(AttachError {
            busy: false,
            message: "attached twice".to_owned(),
            code_text: "not attempted".to_owned(),
        }))
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_else(|| ME.into());
    let args: Vec<_> = args.collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let (mut out, mut err) = (stdout.lock(), stderr.lock());
    let mut clock = Process {
        stop: Arc::new(AtomicBool::new(false)),
        wake: None,
    };
    let (mut glue, mut refuse);
    let attacher: &mut dyn Attacher = match attacher() {
        Ok(g) => {
            glue = g;
            &mut glue
        }
        Err(e) => {
            refuse = Refuse(Some(e));
            &mut refuse
        }
    };
    let mut env = Env {
        out: &mut out,
        err: &mut err,
        attacher,
        clock: &mut clock,
    };
    ExitCode::from(openbcm_asic_temp::run(&program, &args, &mut env))
}
