//! libifcs' callbacks (link scan events, L2 events, the log function) into Rust closures,
//! without handing libifcs any Rust memory: as `sdk-glue`'s `link.rs`.
//!
//! * The closures live in one process-wide table behind a mutex ([`REGISTRY`]), one entry per
//!   [`Callbacks`] (one per switch), keyed by a number that is never reused.
//! * The `void* user` libifcs hands back is that key as an address-sized number: compared, never
//!   dereferenced.
//! * The C callbacks are this module's `extern "C"` trampolines: each looks its entry up, takes a
//!   clone of the closure's `Arc`, **releases the table**, copies the event out of libifcs' struct
//!   (checking its pointers), and calls the closure under `catch_unwind` (a panic never unwinds
//!   into libifcs; it is counted).
//!
//! So a late or stray call (after [`Callbacks`] is dropped, with any `user`) finds no entry and
//! does nothing; the C++ `on_log` keeps `this` registered in libifcs after its backend is gone.
//! The closures run on libifcs' threads: they must not block and never take a lock of the SAI
//! layer (the backend's queue to the `EventSink`, which only sends on a channel).
#![allow(unsafe_code)]

use core::ffi::{CStr, c_char, c_int, c_void};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use ifcs_sys as sys;

use crate::Ifcs;
use crate::handle::Handle;
use crate::status::Status;

/// A link scan event (`ifcs_linkscan_event_info_t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkEvent {
    /// `devport_id`.
    pub devport: u32,
    /// `port_state == IFCS_DEVPORT_LINK_STATUS_UP`.
    pub up: bool,
}

/// An L2 event (`ifcs_l2_event_info_t`), copied out of libifcs' struct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct L2Event {
    /// `event_type` (`IFCS_L2_EVENT_TYPE_*`).
    pub event_type: i32,
    /// `l2_entry.key_type` (`IFCS_L2_ENTRY_KEY_TYPE_*`).
    pub key_type: i32,
    /// `l2_entry.key.mac_l2vni.mac_addr` (meaningful for `MAC_L2VNI` keys).
    pub mac: [u8; 6],
    /// `l2_entry.key.mac_l2vni.l2vni`.
    pub l2vni: Handle,
    /// `attr_p[0..attr_count]`: each attribute's id and the first 32 bits of its value
    /// (`value.handle` / `value.u32`), at most [`L2Event::MAX_ATTRS`].
    pub attrs: Vec<(u32, u32)>,
}

impl L2Event {
    /// The most attributes copied from one event (libifcs sends a handful).
    pub const MAX_ATTRS: u32 = 64;
}

type LinkFn = dyn Fn(LinkEvent) + Send + Sync;
type L2Fn = dyn Fn(L2Event) + Send + Sync;
type LogFn = dyn Fn(i32, &str) + Send + Sync;

/// What a switch wants to hear from libifcs.
#[derive(Clone, Default)]
pub struct Handlers {
    /// Link scan events.
    pub link: Option<Arc<LinkFn>>,
    /// L2 learn / age / move events.
    pub l2: Option<Arc<L2Fn>>,
    /// libifcs' log messages (level, text).
    pub log: Option<Arc<LogFn>>,
}

impl core::fmt::Debug for Handlers {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Handlers")
            .field("link", &self.link.is_some())
            .field("l2", &self.l2.is_some())
            .field("log", &self.log.is_some())
            .finish()
    }
}

static REGISTRY: Mutex<BTreeMap<usize, Handlers>> = Mutex::new(BTreeMap::new());
static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);
static PANICS: AtomicU64 = AtomicU64::new(0);

/// Closures that panicked in a trampoline (the event is lost), since the process started.
pub fn callback_panics() -> u64 {
    PANICS.load(Ordering::Relaxed)
}

/// A switch's entry in the callback table; dropping it removes the entry (no IFCS call: the
/// backend unregisters from libifcs first, as the C++ does).
#[derive(Debug)]
pub struct Callbacks {
    key: usize,
}

impl Callbacks {
    /// An entry with `handlers`.
    pub fn new(handlers: Handlers) -> Self {
        let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
        REGISTRY
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, handlers);
        Self { key }
    }

    fn user(&self) -> *mut c_void {
        core::ptr::without_provenance_mut(self.key)
    }
}

impl Drop for Callbacks {
    fn drop(&mut self) {
        REGISTRY
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

fn handlers(user: *mut c_void) -> Option<Handlers> {
    REGISTRY
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&user.addr())
        .cloned()
}

fn run(f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        PANICS.fetch_add(1, Ordering::Relaxed);
    }
}

extern "C" fn on_link(_node: sys::ifcs_node_id_t, user: *mut c_void, info: *mut sys::ifcs_linkscan_event_info_t) {
    let Some(f) = handlers(user).and_then(|h| h.link) else {
        return;
    };
    if info.is_null() {
        return;
    }
    // SAFETY: libifcs passes a valid event struct (checked non-null) for the duration of the call;
    // it is copied out at once.
    let info = unsafe { *info };
    let event = LinkEvent {
        devport: info.devport_id,
        up: info.port_state == sys::IFCS_DEVPORT_LINK_STATUS_UP.cast_signed(),
    };
    run(|| f(event));
}

extern "C" fn on_l2(_node: sys::ifcs_node_id_t, user: *mut c_void, info: *mut sys::ifcs_l2_event_info_t) {
    let Some(f) = handlers(user).and_then(|h| h.l2) else {
        return;
    };
    if info.is_null() {
        return;
    }
    // SAFETY: as `on_link`: a valid event struct for the call, copied out at once.
    let info = unsafe { *info };
    let key = info.l2_entry.key.blob;
    let mut mac = [0u8; 6];
    let mac_off = core::mem::offset_of!(sys::ifcs_l2_entry_key_mac_l2vni_t, mac_addr);
    let vni_off = core::mem::offset_of!(sys::ifcs_l2_entry_key_mac_l2vni_t, l2vni);
    if let Some(m) = key.get(mac_off..mac_off + 6) {
        mac.copy_from_slice(m);
    }
    let l2vni = key
        .get(vni_off..vni_off + 4)
        .and_then(|b| b.try_into().ok())
        .map_or(0, u32::from_ne_bytes);
    let mut attrs = Vec::new();
    if !info.attr_p.is_null() && info.attr_count > 0 {
        let n = usize::try_from(info.attr_count.min(L2Event::MAX_ATTRS)).unwrap_or(0);
        // SAFETY: libifcs passes attr_count attributes at attr_p (checked non-null) for the call;
        // at most MAX_ATTRS of them are read, and copied out at once.
        let list = unsafe { core::slice::from_raw_parts(info.attr_p, n) };
        attrs = list
            .iter()
            .map(|a| {
                (
                    a.id,
                    a.value
                        .blob
                        .get(..4)
                        .and_then(|b| b.try_into().ok())
                        .map_or(0, u32::from_ne_bytes),
                )
            })
            .collect();
    }
    let event = L2Event {
        event_type: info.event_type,
        key_type: info.l2_entry.key_type,
        mac,
        l2vni: Handle(l2vni),
        attrs,
    };
    run(|| f(event));
}

extern "C" fn on_log(level: c_int, user: *mut c_void, msg: *const c_char) {
    let Some(f) = handlers(user).and_then(|h| h.log) else {
        return;
    };
    if msg.is_null() {
        return; // tl_switch.cpp on_log: no message, nothing logged
    }
    // SAFETY: libifcs passes a NUL-terminated message (checked non-null) for the call; copied.
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy().into_owned();
    run(|| f(level, &text));
}

impl Ifcs {
    /// `ifcs_linkscan_event_register_notify(node, user, on_link)`.
    pub fn ifcs_linkscan_event_register_notify(&self, node: u32, cb: &Callbacks) -> Status {
        let Some(f) = self.t().ifcs_linkscan_event_register_notify else {
            return Status(sys::IFCS_UNINIT.cast_signed());
        };
        // SAFETY: `user` is a number (never dereferenced: the module's rules), the trampoline has
        // the header's ifcs_linkscan_event_notify_fn_t signature and lives for the process.
        Status(unsafe { f(node, cb.user(), Some(on_link)) })
    }

    /// `ifcs_l2_event_register_notify(node, user, on_l2)`.
    pub fn ifcs_l2_event_register_notify(&self, node: u32, cb: &Callbacks) -> Status {
        let Some(f) = self.t().ifcs_l2_event_register_notify else {
            return Status(sys::IFCS_UNINIT.cast_signed());
        };
        // SAFETY: as ifcs_linkscan_event_register_notify.
        Status(unsafe { f(node, cb.user(), Some(on_l2)) })
    }

    /// `ifcs_log_function_register(on_log, user)`.
    pub fn ifcs_log_function_register(&self, cb: &Callbacks) -> Status {
        let Some(f) = self.t().ifcs_log_function_register else {
            return Status(sys::IFCS_UNINIT.cast_signed());
        };
        // SAFETY: as ifcs_linkscan_event_register_notify (ifcs_log_notify_t).
        Status(unsafe { f(Some(on_log), cb.user()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn trampolines_find_their_entry_and_nothing_else() {
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let cb = Callbacks::new(Handlers {
            link: Some(Arc::new(move |e| tx.send(format!("{e:?}")).unwrap())),
            log: Some(Arc::new(move |l, m| tx2.send(format!("{l} {m}")).unwrap())),
            l2: Some(Arc::new(|_| panic!("caught"))),
        });
        let mut info = sys::ifcs_linkscan_event_info_t {
            node_id: 0,
            devport_id: 3,
            port_state: 1,
            fault_state: 0,
            rx_aligned_sts: 0,
            rx_hi_ber_sts: 0,
            rx_hi_ser_sts: 0,
        };
        on_link(0, cb.user(), &raw mut info);
        assert_eq!(rx.try_recv().unwrap(), "LinkEvent { devport: 3, up: true }");
        on_link(0, cb.user(), core::ptr::null_mut());
        on_log(3, cb.user(), c"hello".as_ptr());
        assert_eq!(rx.try_recv().unwrap(), "3 hello");
        // another key: nothing
        on_link(0, core::ptr::without_provenance_mut(usize::MAX), &raw mut info);
        assert!(rx.try_recv().is_err());
        // a panicking closure is caught and counted
        let before = callback_panics();
        let mut l2 = sys::ifcs_l2_event_info_t {
            event_type: 0,
            count: 1,
            l2_entry: sys::ifcs_l2_entry_key_t {
                _base_key: 0,
                key_type: 0,
                key: sys::ifcs_l2_entry_key_map_t { blob: [0; 88] },
                dirty: 0,
            },
            attr_count: 0,
            attr_p: core::ptr::null_mut(),
        };
        on_l2(0, cb.user(), &raw mut l2);
        assert_eq!(callback_panics(), before + 1);
        // after the entry is gone: nothing
        let user = cb.user();
        drop(cb);
        on_link(0, user, &raw mut info);
        assert!(rx.try_recv().is_err());
    }
}
