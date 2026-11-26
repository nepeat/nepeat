//! The calls into the library: one call site per C signature, and the method table read after
//! `sai_api_query`. Every function pointer here comes from the library (`dlsym` of a header
//! function, or a method table the library handed out), with the header's prototype.
#![allow(unsafe_code)]

use core::ffi::c_void;

use crate::dl::Library;
use crate::raw;
use crate::sys;
use crate::value::{self, Encoded};
use crate::{Entry, Status, callback};

type InitFn = unsafe extern "C" fn(u64, *const sys::sai_service_method_table_t) -> Status;
type QueryFn = unsafe extern "C" fn(sys::sai_api_t, *mut *mut c_void) -> Status;
type UninitFn = unsafe extern "C" fn() -> Status;
type TypeQueryFn = unsafe extern "C" fn(sys::sai_object_id_t) -> sys::sai_object_type_t;
type StatsCapFn =
    unsafe extern "C" fn(sys::sai_object_id_t, sys::sai_object_type_t, *mut sys::sai_stat_capability_list_t) -> Status;

/// `create_<type>`
pub(crate) type CreateFn =
    unsafe extern "C" fn(*mut sys::sai_object_id_t, sys::sai_object_id_t, u32, *const sys::sai_attribute_t) -> Status;
/// `create_switch`
pub(crate) type CreateSwitchFn =
    unsafe extern "C" fn(*mut sys::sai_object_id_t, u32, *const sys::sai_attribute_t) -> Status;
/// `remove_<type>`
pub(crate) type RemoveFn = unsafe extern "C" fn(sys::sai_object_id_t) -> Status;
/// `set_<type>_attribute`
pub(crate) type SetFn = unsafe extern "C" fn(sys::sai_object_id_t, *const sys::sai_attribute_t) -> Status;
/// `get_<type>_attribute`
pub(crate) type GetFn = unsafe extern "C" fn(sys::sai_object_id_t, u32, *mut sys::sai_attribute_t) -> Status;
/// `get_<type>_stats`
pub(crate) type StatsFn =
    unsafe extern "C" fn(sys::sai_object_id_t, u32, *const sys::sai_stat_id_t, *mut u64) -> Status;
/// `flush_fdb_entries`
pub(crate) type FlushFn = unsafe extern "C" fn(sys::sai_object_id_t, u32, *const sys::sai_attribute_t) -> Status;
/// `create_<entry>`
pub(crate) type EntryCreateFn<K> = unsafe extern "C" fn(*const K, u32, *const sys::sai_attribute_t) -> Status;
/// `remove_<entry>`
pub(crate) type EntryRemoveFn<K> = unsafe extern "C" fn(*const K) -> Status;
/// `set_<entry>_attribute`
pub(crate) type EntrySetFn<K> = unsafe extern "C" fn(*const K, *const sys::sai_attribute_t) -> Status;

/// The loaded library's global functions.
pub(crate) struct Lib {
    _handle: Library,
    init: InitFn,
    query: QueryFn,
    uninit: UninitFn,
    type_query: TypeQueryFn,
    stats_cap: Option<StatsCapFn>,
}

/// The method table types `sai_api_query` may be read as (the header's `sai_<api>_api_t`).
pub(crate) trait Table: Copy + sealed::Sealed {}
mod sealed {
    pub(crate) trait Sealed {}
}
macro_rules! tables {
    ($($t:ty),*) => { $(impl sealed::Sealed for $t {} impl Table for $t {})* };
}
tables!(
    sys::sai_switch_api_t,
    sys::sai_port_api_t,
    sys::sai_hostif_api_t,
    sys::sai_policer_api_t,
    sys::sai_vlan_api_t,
    sys::sai_router_interface_api_t,
    sys::sai_route_api_t,
    sys::sai_neighbor_api_t,
    sys::sai_next_hop_api_t,
    sys::sai_bridge_api_t,
    sys::sai_virtual_router_api_t,
    sys::sai_acl_api_t,
    sys::sai_qos_map_api_t,
    sys::sai_scheduler_api_t,
    sys::sai_scheduler_group_api_t,
    sys::sai_wred_api_t,
    sys::sai_buffer_api_t,
    sys::sai_queue_api_t,
    sys::sai_mirror_api_t,
    sys::sai_samplepacket_api_t,
    sys::sai_next_hop_group_api_t,
    sys::sai_lag_api_t,
    sys::sai_fdb_api_t,
    sys::sai_hash_api_t
);

fn n(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The array's address, NULL when empty (a C program's empty `std::vector`'s `data()`).
fn arr(e: &Encoded) -> *const sys::sai_attribute_t {
    if e.raw.is_empty() {
        core::ptr::null()
    } else {
        e.raw.as_ptr()
    }
}

impl Lib {
    pub(crate) fn open(path: &str) -> Result<Self, String> {
        let lib = Library::open(path)?;
        let need = |name: &str| format!("{path}: no {name}");
        Ok(Self {
            init: lib
                .function::<InitFn>(c"sai_api_initialize")
                .ok_or_else(|| need("sai_api_initialize"))?,
            query: lib
                .function::<QueryFn>(c"sai_api_query")
                .ok_or_else(|| need("sai_api_query"))?,
            uninit: lib
                .function::<UninitFn>(c"sai_api_uninitialize")
                .ok_or_else(|| need("sai_api_uninitialize"))?,
            type_query: lib
                .function::<TypeQueryFn>(c"sai_object_type_query")
                .ok_or_else(|| need("sai_object_type_query"))?,
            stats_cap: lib.function::<StatsCapFn>(c"sai_query_stats_capability"),
            _handle: lib,
        })
    }

    pub(crate) fn initialize(&self) -> Status {
        // SAFETY: sai_api_initialize(flags, services): the services table is a static that lives for
        // the process, its callbacks are this crate's (`callback.rs`).
        unsafe { (self.init)(0, &raw const callback::SERVICES) }
    }

    pub(crate) fn uninitialize(&self) -> Status {
        // SAFETY: no arguments.
        unsafe { (self.uninit)() }
    }

    pub(crate) fn object_type(&self, oid: sys::sai_object_id_t) -> sys::sai_object_type_t {
        // SAFETY: an integer argument.
        unsafe { (self.type_query)(oid) }
    }

    pub(crate) fn stats_capability(
        &self,
        switch: sys::sai_object_id_t,
        object_type: sys::sai_object_type_t,
        room: usize,
    ) -> (Status, Vec<(u32, u32)>) {
        let Some(f) = self.stats_cap else {
            return (crate::NOT_IMPLEMENTED, Vec::new());
        };
        let mut caps = vec![
            sys::sai_stat_capability_t {
                stat_enum: 0,
                stat_modes: 0
            };
            room
        ];
        let mut l = sys::sai_stat_capability_list_t {
            count: n(caps.len()),
            list: caps.as_mut_ptr(),
        };
        // SAFETY: the list names `room` writable entries (caps, alive for the call).
        let st = unsafe { f(switch, object_type, &raw mut l) };
        if st != crate::SUCCESS {
            return (st, Vec::new());
        }
        let got = usize::try_from(l.count).unwrap_or(usize::MAX);
        (st, caps.iter().take(got).map(|c| (c.stat_enum, c.stat_modes)).collect())
    }

    /// `sai_api_query(api)` and its method table, copied (the library's tables are static).
    pub(crate) fn table<T: Table>(&self, api: sys::sai_api_t) -> (Status, Option<T>) {
        let mut p: *mut c_void = core::ptr::null_mut();
        // SAFETY: an API number and a writable pointer slot.
        let st = unsafe { (self.query)(api, &raw mut p) };
        if st != crate::SUCCESS || p.is_null() {
            return (st, None);
        }
        // SAFETY: for `api` the library hands out its `sai_<api>_api_t` (the caller's `T`, see
        // dispatch.rs), a static table of function pointers; a read of one copy.
        (st, Some(unsafe { p.cast::<T>().read() }))
    }
}

// The calls below take the library's function and an `Encoded` (whose array and lists are alive
// and sized as its counts say for the whole call).

pub(crate) fn create_switch(f: CreateSwitchFn, id: &mut sys::sai_object_id_t, e: &Encoded) -> Status {
    // SAFETY: a writable id; `e.raw` holds `len` initialised attributes (lists alive in `e`).
    unsafe { f(id, n(e.raw.len()), arr(e)) }
}

pub(crate) fn create(f: CreateFn, switch: sys::sai_object_id_t, e: &Encoded) -> (Status, sys::sai_object_id_t) {
    let mut id = 0;
    // SAFETY: as create_switch.
    let st = unsafe { f(&raw mut id, switch, n(e.raw.len()), arr(e)) };
    (st, id)
}

pub(crate) fn remove(f: RemoveFn, oid: sys::sai_object_id_t) -> Status {
    // SAFETY: an integer argument.
    unsafe { f(oid) }
}

pub(crate) fn set(f: SetFn, oid: sys::sai_object_id_t, e: &Encoded) -> Status {
    // SAFETY: one initialised attribute (its lists alive in `e`), or NULL (the library checks).
    unsafe { f(oid, arr(e)) }
}

pub(crate) fn get(f: GetFn, oid: sys::sai_object_id_t, e: &mut Encoded) -> Status {
    let len = n(e.raw.len());
    let p = if e.raw.is_empty() {
        core::ptr::null_mut()
    } else {
        e.raw.as_mut_ptr()
    };
    // SAFETY: `len` writable attributes (or NULL for none); their list members point into `e`'s
    // storage with room for the counts they carry.
    unsafe { f(oid, len, p) }
}

pub(crate) fn stats(f: StatsFn, oid: sys::sai_object_id_t, ids: &[sys::sai_stat_id_t], out: &mut [u64]) -> Status {
    let len = n(ids.len().min(out.len()));
    // SAFETY: `len` ids to read and `len` writable counters.
    unsafe { f(oid, len, ids.as_ptr(), out.as_mut_ptr()) }
}

pub(crate) fn flush(f: FlushFn, switch: sys::sai_object_id_t, e: &Encoded) -> Status {
    // SAFETY: as create_switch.
    unsafe { f(switch, n(e.raw.len()), arr(e)) }
}

pub(crate) fn entry_create<K>(f: EntryCreateFn<K>, key: &K, e: &Encoded) -> Status {
    // SAFETY: a key the caller built in full (zeroed first), and `e`'s attributes.
    unsafe { f(key, n(e.raw.len()), arr(e)) }
}

pub(crate) fn entry_remove<K>(f: EntryRemoveFn<K>, key: &K) -> Status {
    // SAFETY: a key the caller built in full.
    unsafe { f(key) }
}

pub(crate) fn entry_set<K>(f: EntrySetFn<K>, key: &K, e: &Encoded) -> Status {
    // SAFETY: a key the caller built in full and one initialised attribute (or NULL).
    unsafe { f(key, arr(e)) }
}

/// The C key of an entry.
pub(crate) enum Key {
    Route(sys::sai_route_entry_t),
    Neighbor(sys::sai_neighbor_entry_t),
    Fdb(sys::sai_fdb_entry_t),
}

pub(crate) fn key(switch: sys::sai_object_id_t, k: &Entry) -> Key {
    match k {
        Entry::Route { vr, prefix } => {
            let mut e: sys::sai_route_entry_t = raw::zeroed();
            e.switch_id = switch;
            e.vr_id = *vr;
            e.destination = value::ip_prefix(prefix);
            Key::Route(e)
        }
        Entry::Neighbor { rif, ip } => {
            let mut e: sys::sai_neighbor_entry_t = raw::zeroed();
            e.switch_id = switch;
            e.rif_id = *rif;
            e.ip_address = value::ip_address(*ip);
            Key::Neighbor(e)
        }
        Entry::Fdb { bv, mac } => {
            let mut e: sys::sai_fdb_entry_t = raw::zeroed();
            e.switch_id = switch;
            e.bv_id = *bv;
            e.mac_address = *mac;
            Key::Fdb(e)
        }
    }
}
