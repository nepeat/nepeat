//! Logging: a thin forward to [`sai_core::log`] (syslog, and stderr with
//! `SAI_OPENBCM_LOG_STDERR=1`; per-API levels set by `sai_log_set`, default WARN).
use sai_sys as sys;

/// Sets the level of one API; `false` when either number is out of range.
pub(crate) fn set_level(api: sys::sai_api_t, level: sys::sai_log_level_t) -> bool {
    sai_core::log::set_level(api, level)
}

/// Writes a line when `level` is at or above the API's level.
pub(crate) fn emit(api: sys::sai_api_t, level: sys::sai_log_level_t, message: core::fmt::Arguments<'_>) {
    sai_core::log::emit(api, level, message);
}
