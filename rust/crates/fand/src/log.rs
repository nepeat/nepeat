//! Minimal leveled logging to stderr.
//!
//! Under systemd (JOURNAL_STREAM set) every line gets a `<N>` syslog priority
//! prefix, which journald turns into the message priority. Otherwise lines are
//! prefixed with the level name. Rate limiting is done by the callers, which
//! log on state changes and on a periodic summary rather than every tick.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 3,
    Warn = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

static JOURNAL: OnceLock<bool> = OnceLock::new();
static DEBUG: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);

pub fn set_debug(on: bool) {
    DEBUG.store(on, Ordering::Relaxed);
}

/// Suppress everything below Warn (used by machine-readable subcommands).
pub fn set_quiet(on: bool) {
    QUIET.store(on, Ordering::Relaxed);
}

pub fn log(level: Level, msg: &str) {
    if level == Level::Debug && !DEBUG.load(Ordering::Relaxed) {
        return;
    }
    if level > Level::Warn && QUIET.load(Ordering::Relaxed) {
        return;
    }
    let journal = *JOURNAL.get_or_init(|| std::env::var_os("JOURNAL_STREAM").is_some());
    if journal {
        eprintln!("<{}>{msg}", level as u8);
    } else {
        let name = match level {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Notice => "notice",
            Level::Info => "info",
            Level::Debug => "debug",
        };
        eprintln!("fand {name}: {msg}");
    }
}

#[macro_export]
macro_rules! log_error { ($($t:tt)*) => { $crate::log::log($crate::log::Level::Error, &format!($($t)*)) } }
#[macro_export]
macro_rules! log_warn { ($($t:tt)*) => { $crate::log::log($crate::log::Level::Warn, &format!($($t)*)) } }
#[macro_export]
macro_rules! log_notice { ($($t:tt)*) => { $crate::log::log($crate::log::Level::Notice, &format!($($t)*)) } }
#[macro_export]
macro_rules! log_info { ($($t:tt)*) => { $crate::log::log($crate::log::Level::Info, &format!($($t)*)) } }
#[macro_export]
macro_rules! log_debug { ($($t:tt)*) => { $crate::log::log($crate::log::Level::Debug, &format!($($t)*)) } }
