//! The library's log, as the C++ sai-openbcm writes it (`sai_api.cpp` `log_msg`): one line per
//! message to syslog (`/dev/log`, facility user, the program's name as the tag) and, when
//! `SAI_OPENBCM_LOG_STDERR=1` ([`configure_stderr`]), to stderr as well. Per-API levels
//! (`sai_log_set`, default WARN): a message below its API's level is not formatted.
//!
//! Line text: `sai-openbcm <LEVEL> api=<n>: <message>` (the C++ text, so log greps of the VM
//! tests match either library); the message is cut to 1023 bytes as by the C++ buffer.
//!
//! Differences from glibc `syslog(3)`: no timestamp in the datagram (journald and rsyslog stamp
//! it on receipt; a local-time stamp would need the C library's time zone code), and the
//! socket is reopened once when a send fails (glibc reconnects the same way).
use sai_sys as sys;
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

/// Writes one message when its level is enabled for its API.
pub fn emit(api: sys::sai_api_t, level: sys::sai_log_level_t, message: core::fmt::Arguments<'_>) {
    if !enabled(api, level) {
        return;
    }
    let text = line(api, level, message);
    syslog(level_name(level).1, &text);
    if to_stderr() {
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
