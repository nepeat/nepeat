//! The library's state: one mutex, taken by every SAI call for the time the safe core runs.
//!
//! Stages: `sai_api_initialize` puts an [`Api`] (core without a switch) into the mutex;
//! creating the switch brings the backend up and starts the notifier thread; removing it
//! stops both; `sai_api_uninitialize` empties the mutex.
//!
//! A panic while the mutex is held poisons it. From then on every call fails
//! (`SAI_STATUS_FAILURE`) until `sai_api_uninitialize`, which discards the state without
//! looking at it: a half-applied change is never built upon.
//!
//! The mutex is never held while foreign code runs (profile callbacks, notification
//! callbacks), so the SAI user may call back into the library from its callbacks.
use crate::Status;
use core::ffi::{CStr, c_char};
use core::ptr;
use sai_core::backend::Backend;
use sai_core::backend::Profile;
use sai_core::sai::Sai;
use sai_sys as sys;
use std::collections::BTreeMap;
use std::ffi::CString;
use std::sync::Mutex;
use std::thread::JoinHandle;

/// The profile callbacks of the service method table the SAI user passed to
/// `sai_api_initialize`.
#[derive(Clone, Copy)]
pub(crate) struct Services {
    get_value: sys::sai_profile_get_value_fn,
    get_next_value: sys::sai_profile_get_next_value_fn,
}

/// The most pairs read from `profile_get_next_value` (the C++ `collect_profile` guard).
const ENUMERATION_MAX: usize = 4096;

impl Services {
    /// Keeps the table's callbacks.
    ///
    /// # Safety
    /// The table's function pointers are null or stay callable, from any thread, with the
    /// signatures of the SAI headers until `sai_api_uninitialize` returns;
    /// `profile_get_value` returns null or a NUL-terminated string; `profile_get_next_value`
    /// accepts null `variable` and `value` (restart the enumeration) and, when it returns 0,
    /// has stored null or a NUL-terminated string in each; every such string stays valid at
    /// least until the next call of a table function.
    pub(crate) unsafe fn new(table: &sys::sai_service_method_table_t) -> Self {
        Self {
            get_value: table.profile_get_value,
            get_next_value: table.profile_get_next_value,
        }
    }

    /// One value of the SAI user's profile (`profile_get_value`, the C++ `profile_get`);
    /// `None` when absent. Runs foreign code: call it without holding the API lock.
    pub(crate) fn value(self, profile_id: sys::sai_switch_profile_id_t, key: &str) -> Option<String> {
        let get_value = self.get_value?;
        let c_key = CString::new(key).ok()?;
        // SAFETY: `get_value` is callable with this signature (contract of `Services::new`,
        // the only constructor); `c_key` is a NUL-terminated string alive across the call.
        let value = unsafe { get_value(profile_id, c_key.as_ptr()) };
        if value.is_null() {
            return None;
        }
        // SAFETY: non-null (checked) and NUL-terminated, valid until the next table call
        // (contract of `Services::new`); it is copied before this function returns.
        let text = unsafe { CStr::from_ptr(value) };
        Some(text.to_string_lossy().into_owned())
    }

    /// The pairs `profile_get_next_value` enumerates: one restart call, then at most
    /// [`ENUMERATION_MAX`] calls while it returns 0; a pair with a null key or value is
    /// skipped. Runs foreign code: call it without holding the API lock.
    fn enumerated(self, profile_id: sys::sai_switch_profile_id_t) -> Vec<(String, String)> {
        let Some(next) = self.get_next_value else {
            return Vec::new();
        };
        let copy = |text: *const c_char| -> Option<String> {
            if text.is_null() {
                return None;
            }
            // SAFETY: only called below on what `next` just stored after returning 0: non-null
            // (checked) and NUL-terminated, valid until the next table call (contract of
            // `Services::new`); both strings of a pair are copied before that call.
            Some(unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned())
        };
        let call = |variable: *mut *const c_char, value: *mut *const c_char| {
            // SAFETY: `next` is callable with this signature (contract of `Services::new`);
            // `variable` and `value` are both null (restart) or both point to live slots.
            unsafe { next(profile_id, variable, value) }
        };
        // Restart the enumeration; the C++ does not look at the answer either.
        call(ptr::null_mut(), ptr::null_mut());
        let mut pairs = Vec::new();
        for _ in 0..ENUMERATION_MAX {
            let mut variable: *const c_char = ptr::null();
            let mut value: *const c_char = ptr::null();
            if call(&raw mut variable, &raw mut value) != 0 {
                break;
            }
            if let (Some(variable), Some(value)) = (copy(variable), copy(value)) {
                pairs.push((variable, value));
            }
        }
        pairs
    }

    /// The profile a switch is created with (the C++ `collect_profile`): every pair
    /// `profile_get_next_value` enumerates, then each of `keys` asked with
    /// `profile_get_value` (absent when it answers null); a later value of a key replaces an
    /// earlier one. Runs foreign code: call it without holding the API lock.
    pub(crate) fn profile(self, profile_id: sys::sai_switch_profile_id_t, keys: &[&str]) -> Profile {
        let mut values: BTreeMap<String, String> = self.enumerated(profile_id).into_iter().collect();
        for key in keys {
            if let Some(value) = self.value(profile_id, key) {
                values.insert((*key).to_owned(), value);
            }
        }
        Profile::from_pairs(values)
    }
}

/// The initialised API.
pub(crate) struct Api {
    pub(crate) services: Services,
    /// The core over the backend the profile chose (`crate::backends`).
    pub(crate) sai: Sai<Box<dyn Backend>>,
    /// The notifier thread of the current switch.
    pub(crate) notifier: Option<JoinHandle<()>>,
    /// The diag socket server of the current switch; dropped (stopped) only outside the lock.
    pub(crate) diag: Option<sai_diag::DiagServer>,
    /// Counts switch creations: an event of an earlier switch is not applied to a later one.
    pub(crate) generation: u64,
}

static API: Mutex<Option<Api>> = Mutex::new(None);

/// `sai_api_initialize`.
pub(crate) fn initialize(services: Services) -> Result<(), Status> {
    // the MACsec model this library links (sai-macsec over phy-seq, natively)
    sai_core::macsec::register_loader(&sai_macsec_phy::native::LOADER);
    let mut guard = API.lock().map_err(|_| sys::SAI_STATUS_FAILURE)?;
    if guard.is_some() {
        return Err(sys::SAI_STATUS_FAILURE);
    }
    *guard = Some(Api {
        services,
        sai: Sai::new(crate::backends::placeholder()),
        notifier: None,
        diag: None,
        generation: 0,
    });
    Ok(())
}

/// `sai_api_uninitialize`: takes the state out (also out of a poisoned mutex), shuts the
/// backend down and waits for the notifier thread, outside the lock.
pub(crate) fn uninitialize() -> Result<(), Status> {
    let api = {
        let mut guard = API.lock().unwrap_or_else(|poisoned| {
            API.clear_poison();
            poisoned.into_inner()
        });
        guard.take()
    };
    let Some(Api {
        sai, notifier, diag, ..
    }) = api
    else {
        return Err(sys::SAI_STATUS_UNINITIALIZED);
    };
    drop(diag); // stops the diag server: its thread may be waiting for the lock, now free
    drop(sai); // shuts the backend down, which closes the event channel the notifier reads
    join(notifier);
    Ok(())
}

/// Runs `body` on the initialised API, under the lock.
pub(crate) fn with_api<T>(body: impl FnOnce(&mut Api) -> Result<T, Status>) -> Result<T, Status> {
    let mut guard = API.lock().map_err(|_| sys::SAI_STATUS_FAILURE)?;
    body(guard.as_mut().ok_or(sys::SAI_STATUS_UNINITIALIZED)?)
}

#[cfg(test)]
pub(crate) fn is_initialized() -> bool {
    API.lock().is_ok_and(|guard| guard.is_some())
}

/// Waits for a notifier thread to end, unless called from that thread itself (a callback
/// that removes the switch).
pub(crate) fn join(notifier: Option<JoinHandle<()>>) {
    if let Some(handle) = notifier {
        if handle.thread().id() != std::thread::current().id() {
            // A panic of the notifier already poisoned what it held; nothing to add here.
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::guard;

    /// The only unit test that uses the global state (the others would race with it).
    #[test]
    fn a_panic_under_the_lock_fails_every_call_until_uninitialize() {
        // SAFETY: a table without callbacks.
        let services = unsafe {
            Services::new(&sys::sai_service_method_table_t {
                profile_get_value: None,
                profile_get_next_value: None,
            })
        };
        assert!(services.profile(0, &["SAI_RS_BACKEND"]).get("SAI_RS_BACKEND").is_none());
        assert_eq!(with_api(|_| Ok(())), Err(sys::SAI_STATUS_UNINITIALIZED));
        assert_eq!(uninitialize(), Err(sys::SAI_STATUS_UNINITIALIZED));
        initialize(services).unwrap();
        assert_eq!(initialize(services), Err(sys::SAI_STATUS_FAILURE));
        assert_eq!(with_api(|api| Ok(api.generation)), Ok(0));

        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let status = guard(sys::SAI_API_SWITCH, || {
            with_api(|api| -> Result<(), Status> {
                api.generation = 99; // a change that is then abandoned half-way
                panic!("test panic inside a SAI call")
            })
        });
        std::panic::set_hook(quiet);
        assert_eq!(status, sys::SAI_STATUS_FAILURE);

        // Nothing is served from the abandoned state, not even reads.
        assert_eq!(with_api(|api| Ok(api.generation)), Err(sys::SAI_STATUS_FAILURE));
        assert_eq!(initialize(services), Err(sys::SAI_STATUS_FAILURE));
        assert!(!is_initialized());
        // Uninitialize discards it; the API then starts clean.
        assert_eq!(uninitialize(), Ok(()));
        assert_eq!(with_api(|_| Ok(())), Err(sys::SAI_STATUS_UNINITIALIZED));
        initialize(services).unwrap();
        assert_eq!(with_api(|api| Ok(api.generation)), Ok(0));
        uninitialize().unwrap();
    }
}
