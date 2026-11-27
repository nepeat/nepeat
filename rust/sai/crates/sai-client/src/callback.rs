//! The C side's callbacks: the profile (`sai_service_method_table_t`, no user pointer: process
//! state) and the notifications (`*_NOTIFY` attributes), delivered as [`Event`]s to one sink.
#![allow(unsafe_code)]

use core::ffi::{CStr, c_char, c_int, c_void};
use std::ffi::CString;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, PoisonError};

use crate::raw;
use crate::sys;
use crate::value::Notify;

/// A notification from the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// `PORT_STATE_CHANGE_NOTIFY`, one per entry
    PortState {
        /// `port_id`
        port: sys::sai_object_id_t,
        /// `port_state` (`sai_port_oper_status_t`)
        state: i32,
    },
    /// `FDB_EVENT_NOTIFY`, one per entry
    Fdb {
        /// `event_type`
        event: i32,
        /// the entry's MAC
        mac: [u8; 6],
        /// the entry's `bv_id`
        bv: sys::sai_object_id_t,
        /// its `BRIDGE_PORT_ID` attribute (0 if none)
        bridge_port: sys::sai_object_id_t,
    },
    /// `SWITCH_SHUTDOWN_REQUEST_NOTIFY`
    Shutdown,
}

/// The profile: `(key, value)` in the order `profile_get_next_value` serves them. Every profile
/// ever set is leaked (a library may keep a value's address), so the references live for the process.
static PROFILE: Mutex<Option<&'static [(CString, CString)]>> = Mutex::new(None);
static NEXT: Mutex<usize> = Mutex::new(0);

type Sink = Arc<dyn Fn(&Event) + Send + Sync>;
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// The profile the library will be served (once per process; a later call is refused). Keys or
/// values with a NUL byte are dropped.
pub fn set_profile(pairs: Vec<(String, String)>) -> Result<(), String> {
    if current_profile().is_some() {
        return Err("the profile is set already".to_owned());
    }
    replace_profile(pairs);
    Ok(())
}

/// The profile the library is served from now on (the end-to-end test changes it between its
/// refusals and the switch it creates). The previous one stays alive: a library may keep its values.
pub fn replace_profile(pairs: Vec<(String, String)>) {
    let c: Vec<(CString, CString)> = pairs
        .into_iter()
        .filter_map(|(k, v)| Some((CString::new(k).ok()?, CString::new(v).ok()?)))
        .collect();
    let leaked: &'static [(CString, CString)] = Box::leak(c.into_boxed_slice());
    *PROFILE.lock().unwrap_or_else(PoisonError::into_inner) = Some(leaked);
}

fn current_profile() -> Option<&'static [(CString, CString)]> {
    *PROFILE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where notifications go (replaces the previous sink). The sink runs on the library's threads.
pub fn set_event_sink(f: impl Fn(&Event) + Send + Sync + 'static) {
    *SINK.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(f));
}

fn deliver(e: &Event) {
    let sink = SINK.lock().unwrap_or_else(PoisonError::into_inner).clone();
    if let Some(s) = sink {
        // a panic in the sink must not unwind into the library
        let _ = catch_unwind(AssertUnwindSafe(|| s(e)));
    }
}

/// # Safety
///
/// Called by the library with a NUL-terminated key or NULL.
unsafe extern "C" fn get_value(_profile: sys::sai_switch_profile_id_t, variable: *const c_char) -> *const c_char {
    if variable.is_null() {
        return core::ptr::null();
    }
    // SAFETY: non-null: the library passes a NUL-terminated key (the header's contract).
    let key = unsafe { CStr::from_ptr(variable) };
    current_profile()
        .and_then(|p| p.iter().rev().find(|(k, _)| k.as_c_str() == key))
        .map_or(core::ptr::null(), |(_, v)| v.as_ptr())
}

/// # Safety
///
/// Called by the library with two writable pointer slots, or NULL to restart.
unsafe extern "C" fn get_next_value(
    _profile: sys::sai_switch_profile_id_t,
    variable: *mut *const c_char,
    value: *mut *const c_char,
) -> c_int {
    let mut next = NEXT.lock().unwrap_or_else(PoisonError::into_inner);
    if variable.is_null() || value.is_null() {
        *next = 0;
        return 0;
    }
    let Some((k, v)) = current_profile().and_then(|p| p.get(*next)) else {
        return -1;
    };
    *next += 1;
    // SAFETY: a non-null writable slot (the header's contract); the string lives for the process.
    unsafe { variable.write(k.as_ptr()) };
    // SAFETY: as above.
    unsafe { value.write(v.as_ptr()) };
    0
}

/// The service method table handed to `sai_api_initialize`.
pub(crate) static SERVICES: sys::sai_service_method_table_t = sys::sai_service_method_table_t {
    profile_get_value: Some(get_value),
    profile_get_next_value: Some(get_next_value),
};

/// # Safety
///
/// Called by the library with `count` entries at `data` (or NULL).
unsafe extern "C" fn on_port_state(count: u32, data: *const sys::sai_port_oper_status_notification_t) {
    if data.is_null() {
        return;
    }
    let n = usize::try_from(count).unwrap_or(0);
    // SAFETY: the library passes `count` initialised entries at `data`, valid during the call.
    let d = unsafe { core::slice::from_raw_parts(data, n) };
    for x in d {
        deliver(&Event::PortState {
            port: x.port_id,
            state: i32::try_from(x.port_state).unwrap_or(i32::MAX),
        });
    }
}

/// # Safety
///
/// Called by the library with `count` entries at `data` (or NULL), each with `attr_count`
/// attributes at `attr`.
unsafe extern "C" fn on_fdb(count: u32, data: *const sys::sai_fdb_event_notification_data_t) {
    if data.is_null() {
        return;
    }
    let n = usize::try_from(count).unwrap_or(0);
    // SAFETY: `count` initialised entries at `data`, valid during the call.
    let d = unsafe { core::slice::from_raw_parts(data, n) };
    for x in d {
        let mut bridge_port = 0;
        if !x.attr.is_null() {
            let m = usize::try_from(x.attr_count).unwrap_or(0);
            // SAFETY: the entry's `attr_count` initialised attributes at `attr`, valid during the call.
            let attrs = unsafe { core::slice::from_raw_parts(x.attr, m) };
            for a in attrs {
                if a.id == sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID {
                    bridge_port = raw::value_oid(a);
                }
            }
        }
        deliver(&Event::Fdb {
            event: i32::try_from(x.event_type).unwrap_or(i32::MAX),
            mac: x.fdb_entry.mac_address,
            bv: x.fdb_entry.bv_id,
            bridge_port,
        });
    }
}

extern "C" fn on_shutdown(_switch: sys::sai_object_id_t) {
    deliver(&Event::Shutdown);
}

/// The address a `ptr` attribute carries for a callback.
pub(crate) fn address(n: Notify) -> *mut c_void {
    match n {
        Notify::PortState => {
            let f: unsafe extern "C" fn(u32, *const sys::sai_port_oper_status_notification_t) = on_port_state;
            f as *mut c_void
        }
        Notify::Fdb => {
            let f: unsafe extern "C" fn(u32, *const sys::sai_fdb_event_notification_data_t) = on_fdb;
            f as *mut c_void
        }
        Notify::Shutdown => {
            let f: extern "C" fn(sys::sai_object_id_t) = on_shutdown;
            f as *mut c_void
        }
        Notify::Null => core::ptr::null_mut(),
    }
}
