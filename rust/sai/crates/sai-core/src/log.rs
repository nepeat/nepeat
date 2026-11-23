//! The library's log, as the C++ sai-openbcm writes it (`sai_api.cpp` `log_msg`): one line per
//! message to syslog (`/dev/log`, facility user, the program's name as the tag) and, when
//! `SAI_OPENBCM_LOG_STDERR=1` ([`configure_stderr`]), to stderr as well; errors go to stderr also
//! without it unless stderr is the journal ([`on_stderr`]: a failure stays visible to whoever runs a
//! program on this library by hand, as the Rust backend's stderr lines were). Per-API levels
//! (`sai_log_set`, default WARN): a message below its API's level is not formatted.
//!
//! Line text: `sai-openbcm <LEVEL> api=<n>: <message>` (the C++ text, so log greps of the VM
//! tests match either library); the message is cut to 1023 bytes as by the C++ buffer.
//!
//! Differences from glibc `syslog(3)`: no timestamp in the datagram (journald and rsyslog stamp
//! it on receipt; a local-time stamp would need the C library's time zone code), and the
//! socket is reopened once when a send fails (glibc reconnects the same way).
//!
//! Every part of the library writes here: the areas, the virtual backend, the hardware backend
//! (`sai-backend-openbcm`, which also passes on what `sdk-glue`'s callback trampolines report),
//! the external-PHY runtime, the `MACsec` model and the diag socket (through their loggers).
//! Nothing writes to stderr on its own.
//!
//! [`capture`]: a sink for tests: the lines written while a closure runs (those the levels let
//! through, from any thread) are handed back instead of going to syslog / stderr.
use sai_sys as sys;
use std::cell::Cell;
use std::io::Write as _;
use std::os::unix::net::UnixDatagram;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

const APIS: usize = sys::SAI_API_MAX as usize + 1;
static LEVELS: [AtomicU32; APIS] = [const { AtomicU32::new(sys::SAI_LOG_LEVEL_WARN) }; APIS];
static STDERR: AtomicBool = AtomicBool::new(false);
static SYSLOG: Mutex<Option<UnixDatagram>> = Mutex::new(None);

/// The syslog socket.
const DEV_LOG: &str = "/dev/log";
/// `LOG_USER`
const FACILITY_USER: u32 = 1 << 3;
/// The C++ message buffer (1024 bytes with the terminating NUL).
const MESSAGE_MAX: usize = 1023;

/// The lines of a running [`capture`] (`None`: none runs).
static CAPTURED: Mutex<Option<Vec<Line>>> = Mutex::new(None);
/// Held while a [`capture`] runs: one at a time in the process.
static CAPTURE_TURN: Mutex<()> = Mutex::new(());

thread_local! {
    /// This thread runs a [`capture`] (a nested one adds to it).
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
}

/// One line as [`capture`] hands it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The API it was written for.
    pub api: sys::sai_api_t,
    /// Its level.
    pub level: sys::sai_log_level_t,
    /// The whole line ([`line`]: `sai-openbcm <LEVEL> api=<n>: <message>`).
    pub text: String,
}

impl Line {
    /// The message: the line without `sai-openbcm <LEVEL> api=<n>: `.
    pub fn message(&self) -> &str {
        self.text.split_once(": ").map_or(self.text.as_str(), |(_, m)| m)
    }
}

/// Runs `f` and returns every line the process wrote to the log meanwhile, on any thread (lines
/// below their API's level are not written, as always); those lines go nowhere else (not to
/// syslog, not to stderr). For tests: captures take turns (a second one waits for the first), so
/// the lines of a test whose every log-writing part runs inside its capture are its own. A
/// capture inside a capture on the same thread adds to the outer one and returns no lines.
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<Line>) {
    /// Ends the capture, also when `f` panics.
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            CAPTURED.lock().unwrap_or_else(PoisonError::into_inner).take();
            CAPTURING.with(|c| c.set(false));
        }
    }
    if CAPTURING.with(Cell::get) {
        return (f(), Vec::new());
    }
    let _turn = CAPTURE_TURN.lock().unwrap_or_else(PoisonError::into_inner);
    *CAPTURED.lock().unwrap_or_else(PoisonError::into_inner) = Some(Vec::new());
    CAPTURING.with(|c| c.set(true));
    let running = Running;
    let result = f();
    let lines = CAPTURED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .unwrap_or_default();
    drop(running);
    (result, lines)
}

/// Hands a line to the running capture; `false`: none runs.
fn captured(api: sys::sai_api_t, level: sys::sai_log_level_t, text: &str) -> bool {
    match CAPTURED.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        Some(lines) => {
            lines.push(Line {
                api,
                level,
                text: text.to_owned(),
            });
            true
        }
        None => false,
    }
}

/// Sets the level of one API (`sai_log_set`); `false` when either number is out of range
/// (`SAI_API_UNSPECIFIED..=SAI_API_MAX`, `DEBUG..=CRITICAL`).
pub fn set_level(api: sys::sai_api_t, level: sys::sai_log_level_t) -> bool {
    let slot = usize::try_from(api).ok().and_then(|i| LEVELS.get(i));
    match slot {
        Some(slot) if level <= sys::SAI_LOG_LEVEL_CRITICAL => {
            slot.store(level, Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

/// Whether a message of `level` for `api` is written (an API out of range counts as
/// `SAI_API_UNSPECIFIED`).
pub fn enabled(api: sys::sai_api_t, level: sys::sai_log_level_t) -> bool {
    let threshold = usize::try_from(api)
        .ok()
        .and_then(|i| LEVELS.get(i))
        .or(LEVELS.first())
        .map_or(sys::SAI_LOG_LEVEL_WARN, |l| l.load(Ordering::Relaxed));
    level >= threshold
}

/// Also write every line to stderr (`SAI_OPENBCM_LOG_STDERR=1`, read at `sai_api_initialize`).
pub fn configure_stderr(on: bool) {
    STDERR.store(on, Ordering::Relaxed);
}

/// Whether lines go to stderr too.
pub fn to_stderr() -> bool {
    STDERR.load(Ordering::Relaxed)
}

/// The level's name and syslog severity (a level out of range is ERROR, as in the C++).
const fn level_name(level: sys::sai_log_level_t) -> (&'static str, u32) {
    match level {
        sys::SAI_LOG_LEVEL_DEBUG => ("DEBUG", 7),
        sys::SAI_LOG_LEVEL_INFO => ("INFO", 6),
        sys::SAI_LOG_LEVEL_NOTICE => ("NOTICE", 5),
        sys::SAI_LOG_LEVEL_WARN => ("WARN", 4),
        sys::SAI_LOG_LEVEL_CRITICAL => ("CRITICAL", 2),
        _ => ("ERROR", 3),
    }
}

/// `text` cut to at most `max` bytes at a character boundary.
fn truncated(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// The line without the syslog header: `sai-openbcm <LEVEL> api=<n>: <message>`.
pub fn line(api: sys::sai_api_t, level: sys::sai_log_level_t, message: core::fmt::Arguments<'_>) -> String {
    let text = message.to_string();
    format!(
        "sai-openbcm {} api={api}: {}",
        level_name(level).0,
        truncated(&text, MESSAGE_MAX)
    )
}

/// The program's name, as glibc's syslog tags a message without `openlog`.
fn tag() -> String {
    std::env::args_os()
        .next()
        .and_then(|arg0| {
            std::path::Path::new(&arg0)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "sai".to_owned())
}

/// One datagram to `/dev/log`; failures are dropped (a library must not fail a call because
/// nobody listens).
fn syslog(severity: u32, line: &str) {
    // Miri has no AF_UNIX sockets: under it the lines go nowhere (or to stderr, as asked).
    if cfg!(miri) {
        return;
    }
    let datagram = format!("<{}>{}: {line}", FACILITY_USER | severity, tag());
    let mut socket = SYSLOG.lock().unwrap_or_else(PoisonError::into_inner);
    for _ in 0..2 {
        if socket.is_none() {
            *socket = UnixDatagram::unbound().ok().filter(|s| s.connect(DEV_LOG).is_ok());
        }
        match socket.as_ref().map(|s| s.send(datagram.as_bytes())) {
            Some(Ok(_)) | None => return,
            Some(Err(_)) => *socket = None,
        }
    }
}

/// Whether stderr is the journal (systemd's `JOURNAL_STREAM` names fd 2's device and inode):
/// there an error line would be in the journal twice, once from syslog.
fn stderr_is_journal() -> bool {
    use std::os::fd::AsFd as _;
    use std::os::unix::fs::MetadataExt as _;
    static ANSWER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ANSWER.get_or_init(|| {
        let Some(stream) = std::env::var_os("JOURNAL_STREAM") else {
            return false;
        };
        let Some((dev, ino)) = stream.to_str().and_then(|s| s.split_once(':')) else {
            return false;
        };
        let fd2 = std::io::stderr().as_fd().try_clone_to_owned().ok();
        let meta = fd2.map(std::fs::File::from).and_then(|f| f.metadata().ok());
        meta.is_some_and(|m| dev.parse() == Ok(m.dev()) && ino.parse() == Ok(m.ino()))
    })
}

/// Whether a line of `level` goes to stderr: every line with [`configure_stderr`]; errors (and
/// critical ones) also without it, unless stderr is the journal (syncd under systemd: the line is
/// there from syslog). So a failure is on the terminal of whoever runs a SAI program by hand (the
/// smoke program, a CLI); the C++ wrote errors to syslog only.
fn on_stderr(level: sys::sai_log_level_t) -> bool {
    to_stderr() || (level >= sys::SAI_LOG_LEVEL_ERROR && !cfg!(miri) && !stderr_is_journal())
}

/// Writes one message when its level is enabled for its API.
pub fn emit(api: sys::sai_api_t, level: sys::sai_log_level_t, message: core::fmt::Arguments<'_>) {
    if !enabled(api, level) {
        return;
    }
    let text = line(api, level, message);
    if captured(api, level, &text) {
        return;
    }
    syslog(level_name(level).1, &text);
    if on_stderr(level) {
        // A closed stderr must not take the process down (eprintln! would panic).
        let _ = writeln!(std::io::stderr().lock(), "{text}");
    }
}

/// `OB_ERR`
pub fn error(api: sys::sai_api_t, message: core::fmt::Arguments<'_>) {
    emit(api, sys::SAI_LOG_LEVEL_ERROR, message);
}

/// `OB_WARN`
pub fn warn(api: sys::sai_api_t, message: core::fmt::Arguments<'_>) {
    emit(api, sys::SAI_LOG_LEVEL_WARN, message);
}

/// `OB_NOTICE`
pub fn notice(api: sys::sai_api_t, message: core::fmt::Arguments<'_>) {
    emit(api, sys::SAI_LOG_LEVEL_NOTICE, message);
}

/// `OB_INFO`
pub fn info(api: sys::sai_api_t, message: core::fmt::Arguments<'_>) {
    emit(api, sys::SAI_LOG_LEVEL_INFO, message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_as_the_cpp_ones_and_levels_gate() {
        assert_eq!(
            line(
                sys::SAI_API_PORT,
                sys::SAI_LOG_LEVEL_NOTICE,
                format_args!("port {} admin up", "ce0")
            ),
            "sai-openbcm NOTICE api=2: port ce0 admin up"
        );
        assert_eq!(
            line(sys::SAI_API_SWITCH, 99, format_args!("x")),
            "sai-openbcm ERROR api=1: x"
        );
        let long = "\u{e9}".repeat(600);
        assert!(line(0, sys::SAI_LOG_LEVEL_WARN, format_args!("{long}")).len() <= 1023 + 25);
        assert!(!set_level(9999, sys::SAI_LOG_LEVEL_DEBUG));
        assert!(!set_level(sys::SAI_API_PORT, 99));
        assert!(enabled(sys::SAI_API_LAG, sys::SAI_LOG_LEVEL_WARN));
        assert!(!enabled(sys::SAI_API_LAG, sys::SAI_LOG_LEVEL_NOTICE));
        assert!(set_level(sys::SAI_API_LAG, sys::SAI_LOG_LEVEL_DEBUG));
        assert!(enabled(sys::SAI_API_LAG, sys::SAI_LOG_LEVEL_DEBUG));
        assert!(set_level(sys::SAI_API_LAG, sys::SAI_LOG_LEVEL_WARN));
        assert!(enabled(9999, sys::SAI_LOG_LEVEL_ERROR));
    }
}
