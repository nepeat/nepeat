//! Logging: per-API levels set by `sai_log_set` (default WARN), lines to stderr.
use sai_sys as sys;
use std::io::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};

const APIS: usize = sys::SAI_API_MAX as usize + 1;
static LEVELS: [AtomicU32; APIS] = [const { AtomicU32::new(sys::SAI_LOG_LEVEL_WARN) }; APIS];

/// Sets the level of one API; `false` when either number is out of range.
pub(crate) fn set_level(api: sys::sai_api_t, level: sys::sai_log_level_t) -> bool {
    let slot = usize::try_from(api).ok().and_then(|i| LEVELS.get(i));
    match slot {
        Some(slot) if level <= sys::SAI_LOG_LEVEL_CRITICAL => {
            slot.store(level, Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

/// Writes a line when `level` is at or above the API's level.
pub(crate) fn emit(api: sys::sai_api_t, level: sys::sai_log_level_t, message: core::fmt::Arguments<'_>) {
    let threshold = usize::try_from(api)
        .ok()
        .and_then(|i| LEVELS.get(i))
        .map_or(sys::SAI_LOG_LEVEL_WARN, |l| l.load(Ordering::Relaxed));
    if level < threshold {
        return;
    }
    let name = match level {
        sys::SAI_LOG_LEVEL_DEBUG => "DEBUG",
        sys::SAI_LOG_LEVEL_INFO => "INFO",
        sys::SAI_LOG_LEVEL_NOTICE => "NOTICE",
        sys::SAI_LOG_LEVEL_WARN => "WARN",
        sys::SAI_LOG_LEVEL_ERROR => "ERROR",
        _ => "CRITICAL",
    };
    // A closed stderr must not take the process down (eprintln! would panic).
    let _ = writeln!(std::io::stderr().lock(), "sai-rs {name} api={api}: {message}");
}
