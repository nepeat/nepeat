//! Panic containment at the C boundary.
use crate::Status;
use crate::log;
use sai_sys as sys;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs the body of an exported function. A panic does not unwind into C: it becomes
/// `SAI_STATUS_FAILURE`.
///
/// Why the body may be treated as unwind-safe: the only state shared between calls is behind
/// the API mutex, and a panic while it is held poisons it, after which every call fails
/// until `sai_api_uninitialize` throws the state away ([`crate::state`]). So a half-finished
/// change is never reported as success, neither by this call nor by a later one. Output
/// parameters are written as the last step of a call, after the core returned.
///
/// This needs `panic = "unwind"` (the workspace profiles set it). With `panic = "abort"` a
/// panic ends the process instead, which is still not undefined behaviour.
pub(crate) fn guard(api: sys::sai_api_t, body: impl FnOnce() -> Result<(), Status>) -> Status {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => sys::SAI_STATUS_SUCCESS,
        Ok(Err(status)) => status,
        Err(_) => {
            log::emit(
                api,
                sys::SAI_LOG_LEVEL_CRITICAL,
                format_args!("panic inside a SAI call; the API fails until it is uninitialized"),
            );
            sys::SAI_STATUS_FAILURE
        }
    }
}

/// The same for the two entry points that return a value instead of a status.
pub(crate) fn guard_value<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_failure() {
        assert_eq!(guard(sys::SAI_API_SWITCH, || Ok(())), sys::SAI_STATUS_SUCCESS);
        assert_eq!(
            guard(sys::SAI_API_SWITCH, || Err(sys::SAI_STATUS_TABLE_FULL)),
            sys::SAI_STATUS_TABLE_FULL
        );
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let status = guard(sys::SAI_API_SWITCH, || panic!("test"));
        let value = guard_value(7, || panic!("test"));
        std::panic::set_hook(quiet);
        assert_eq!((status, value), (sys::SAI_STATUS_FAILURE, 7));
    }
}
