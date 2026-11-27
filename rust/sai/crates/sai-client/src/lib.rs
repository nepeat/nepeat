//! A SAI user's side of the C ABI, as syncd (or a C program linked against `libsai.so`) has it:
//! a `libsai.so` loaded with `dlopen` and driven through the SAI C API only. Used by the Teralynx
//! smoke program (`sai-teralynx-smoke`) and the oracle's JSON script runner (`sai-script`), which
//! ran on EITHER Teralynx libsai (the C++ one, deleted in H5 on 2026-11-27, last in the tree at
//! commit 767d9b0, and the Rust one), so nothing here knows which library it talks to.
//!
//! - [`Sai`]: the loaded library: `sai_api_initialize` with this crate's service method table
//!   (the profile of [`set_profile`]), `sai_api_query` per API ([`Api`]), `sai_api_uninitialize`,
//!   `sai_object_type_query`, `sai_query_stats_capability`; create / remove / set / get / stats by
//!   object type over the method tables, the route / neighbour / FDB entry calls, `flush_fdb_entries`.
//! - [`Attr`] / [`Value`]: attributes as owned Rust values. A call encodes them into
//!   `sai_attribute_t` arrays that are zeroed first (as the C programs `memset` them) and whose list
//!   members point into storage the call owns; a get decodes what the library wrote back (also when
//!   it failed: the C programs print what is there).
//! - Notifications ([`Notify`] values for the `*_NOTIFY` attributes) arrive as [`Event`]s at the
//!   sink of [`set_event_sink`], on the library's threads.
//! - [`tee`]: the process's stderr through a pipe, kept and passed on (the smoke reads the loader's
//!   learn-ring guard lines, which both libsais write to stderr).
//!
//! # Unsafe
//!
//! Denied at the crate root; allowed in `dl` (`dlopen`, `dlsym`, `dlerror` and its text, a
//! symbol as its function type), `ffi` (one call site per C signature, the method table read),
//! `raw` (zeroed C structs, the bytes of a value union the library wrote, an FDB event's object id),
//! `callback` (the profile and notification callbacks' pointer reads and writes), `mock` (feature `mock`, off by default: the mock
//! libifcs's test hooks of the end-to-end test, one call each) and `tee` (`dup2`). Each block has one unsafe operation and its contract. Everything else is safe code.
#![deny(unsafe_code)]

mod callback;
mod dispatch;
mod dl;
mod ffi;
#[cfg(feature = "mock")]
pub mod mock;
mod raw;
pub mod tee;
mod value;

use std::path::{Path, PathBuf};

pub use callback::{Event, replace_profile, set_event_sink, set_profile};
#[cfg(feature = "mock")]
pub use mock::Mock;
pub use sai_sys as sys;
pub use value::{AclAction, AclData, AclField, Attr, Notify, Prefix, QosMapEntry, QosParams, Value};

/// A SAI status (`sai_status_t`).
pub type Status = sys::sai_status_t;

/// `SAI_STATUS_SUCCESS`.
pub const SUCCESS: Status = sys::SAI_STATUS_SUCCESS;
/// `SAI_STATUS_NOT_SUPPORTED`: the object type has no such call here (the C programs' `default:`).
pub const NOT_SUPPORTED: Status = sys::SAI_STATUS_NOT_SUPPORTED;
/// `SAI_STATUS_NOT_IMPLEMENTED`: the library's method table has a null entry, or the API was not
/// queried (a C program would call through the null pointer).
pub const NOT_IMPLEMENTED: Status = sys::SAI_STATUS_NOT_IMPLEMENTED;

/// The APIs a program can query, in the order `sai_script.cpp` queries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    /// `SAI_API_SWITCH`
    Switch,
    /// `SAI_API_PORT`
    Port,
    /// `SAI_API_HOSTIF`
    Hostif,
    /// `SAI_API_POLICER`
    Policer,
    /// `SAI_API_VLAN`
    Vlan,
    /// `SAI_API_ROUTER_INTERFACE`
    RouterInterface,
    /// `SAI_API_ROUTE`
    Route,
    /// `SAI_API_NEIGHBOR`
    Neighbor,
    /// `SAI_API_NEXT_HOP`
    NextHop,
    /// `SAI_API_BRIDGE`
    Bridge,
    /// `SAI_API_VIRTUAL_ROUTER`
    VirtualRouter,
    /// `SAI_API_ACL`
    Acl,
    /// `SAI_API_QOS_MAP`
    QosMap,
    /// `SAI_API_SCHEDULER`
    Scheduler,
    /// `SAI_API_SCHEDULER_GROUP`
    SchedulerGroup,
    /// `SAI_API_WRED`
    Wred,
    /// `SAI_API_BUFFER`
    Buffer,
    /// `SAI_API_QUEUE`
    Queue,
    /// `SAI_API_MIRROR`
    Mirror,
    /// `SAI_API_SAMPLEPACKET`
    Samplepacket,
    /// `SAI_API_NEXT_HOP_GROUP`
    NextHopGroup,
    /// `SAI_API_LAG`
    Lag,
    /// `SAI_API_FDB`
    Fdb,
    /// `SAI_API_HASH`
    Hash,
}

impl Api {
    /// Every API, in `sai_script.cpp`'s query order.
    pub const ALL: [Self; 24] = [
        Self::Switch,
        Self::Port,
        Self::Hostif,
        Self::Policer,
        Self::Vlan,
        Self::RouterInterface,
        Self::Route,
        Self::Neighbor,
        Self::NextHop,
        Self::Bridge,
        Self::VirtualRouter,
        Self::Acl,
        Self::QosMap,
        Self::Scheduler,
        Self::SchedulerGroup,
        Self::Wred,
        Self::Buffer,
        Self::Queue,
        Self::Mirror,
        Self::Samplepacket,
        Self::NextHopGroup,
        Self::Lag,
        Self::Fdb,
        Self::Hash,
    ];
}

/// The kind of an entry call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryOp {
    /// `create_*_entry`
    Create,
    /// `remove_*_entry`
    Remove,
    /// `set_*_entry_attribute` (the first attribute)
    Set,
}

/// An entry key (`sai_route_entry_t`, `sai_neighbor_entry_t`, `sai_fdb_entry_t`); the switch id is
/// the call's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// a route: virtual router, destination prefix
    Route {
        /// `vr_id`
        vr: sys::sai_object_id_t,
        /// `destination`
        prefix: Prefix,
    },
    /// a neighbour: router interface, address
    Neighbor {
        /// `rif_id`
        rif: sys::sai_object_id_t,
        /// `ip_address`
        ip: std::net::IpAddr,
    },
    /// an FDB entry: bridge or VLAN, MAC
    Fdb {
        /// `bv_id`
        bv: sys::sai_object_id_t,
        /// `mac_address`
        mac: [u8; 6],
    },
}

/// A loaded `libsai.so` (never closed: it starts threads).
pub struct Sai {
    lib: ffi::Lib,
    tables: dispatch::Tables,
}

impl std::fmt::Debug for Sai {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sai").finish_non_exhaustive()
    }
}

/// The library a tool uses when it is given none: `$SAI_LIBSAI`, else `libsai.so.1` in the
/// `lib` directory beside the program's `bin` (the package layout), else the bare name
/// `libsai.so.1` (the loader's search: `LD_LIBRARY_PATH`, ...).
pub fn default_libsai() -> String {
    if let Some(p) = std::env::var_os("SAI_LIBSAI").filter(|p| !p.is_empty()) {
        return p.to_string_lossy().into_owned();
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(Path::parent).map(|d| d.join("lib/libsai.so.1")))
        .filter(|p: &PathBuf| p.exists());
    beside.map_or_else(|| "libsai.so.1".to_owned(), |p| p.to_string_lossy().into_owned())
}

impl Sai {
    /// `dlopen(path, RTLD_NOW | RTLD_GLOBAL)` (a linked program has the library in the global
    /// scope) and the five global functions this crate calls.
    pub fn load(path: &str) -> Result<Self, String> {
        Ok(Self {
            lib: ffi::Lib::open(path)?,
            tables: dispatch::Tables::default(),
        })
    }

    /// `sai_api_initialize(0, services)` with this crate's profile callbacks ([`set_profile`]).
    pub fn initialize(&self) -> Status {
        self.lib.initialize()
    }

    /// `sai_api_query` of one API; its method table is kept for the calls below.
    pub fn query(&mut self, api: Api) -> Status {
        self.tables.query(&self.lib, api)
    }

    /// `sai_api_uninitialize()`.
    pub fn uninitialize(&self) -> Status {
        self.lib.uninitialize()
    }

    /// `sai_object_type_query(oid)`.
    pub fn object_type(&self, oid: sys::sai_object_id_t) -> sys::sai_object_type_t {
        self.lib.object_type(oid)
    }

    /// `sai_query_stats_capability(switch, type, list)` with room for `room` entries: the status and
    /// `(stat_enum, stat_modes)` of the entries the library reported (none unless it succeeded).
    pub fn stats_capability(
        &self,
        switch: sys::sai_object_id_t,
        object_type: sys::sai_object_type_t,
        room: usize,
    ) -> (Status, Vec<(u32, u32)>) {
        self.lib.stats_capability(switch, object_type, room)
    }

    /// `create_switch(&id, attrs)`: `id` keeps its value unless the library writes one (as a C
    /// program's variable).
    pub fn create_switch(&self, id: &mut sys::sai_object_id_t, attrs: &[Attr]) -> Status {
        let Some(f) = self.tables.switch.and_then(|t| t.create_switch) else {
            return NOT_IMPLEMENTED;
        };
        let enc = value::Encoded::new(attrs);
        ffi::create_switch(f, id, &enc)
    }

    /// `remove_switch`.
    pub fn remove_switch(&self, switch: sys::sai_object_id_t) -> Status {
        self.tables
            .switch
            .and_then(|t| t.remove_switch)
            .map_or(NOT_IMPLEMENTED, |f| ffi::remove(f, switch))
    }

    /// `create_<type>(&oid, switch, attrs)`: the status and the id the library wrote (0 if none).
    pub fn create(
        &self,
        object_type: sys::sai_object_type_t,
        switch: sys::sai_object_id_t,
        attrs: &[Attr],
    ) -> (Status, sys::sai_object_id_t) {
        match self.tables.create(object_type) {
            dispatch::Slot::Missing => (NOT_SUPPORTED, 0),
            dispatch::Slot::Null => (NOT_IMPLEMENTED, 0),
            dispatch::Slot::Fn(f) => ffi::create(f, switch, &value::Encoded::new(attrs)),
        }
    }

    /// `remove_<type>(oid)`.
    pub fn remove(&self, object_type: sys::sai_object_type_t, oid: sys::sai_object_id_t) -> Status {
        match self.tables.remove(object_type) {
            dispatch::Slot::Missing => NOT_SUPPORTED,
            dispatch::Slot::Null => NOT_IMPLEMENTED,
            dispatch::Slot::Fn(f) => ffi::remove(f, oid),
        }
    }

    /// `set_<type>_attribute(oid, attr)` (`None`: a NULL attribute, as a C program's empty list).
    pub fn set(&self, object_type: sys::sai_object_type_t, oid: sys::sai_object_id_t, attr: Option<&Attr>) -> Status {
        match self.tables.set(object_type) {
            dispatch::Slot::Missing => NOT_SUPPORTED,
            dispatch::Slot::Null => NOT_IMPLEMENTED,
            dispatch::Slot::Fn(f) => ffi::set(f, oid, &value::Encoded::new(attr.map_or(&[], core::slice::from_ref))),
        }
    }

    /// `get_<type>_attribute(oid, n, attrs)`; afterwards every attribute holds what the library
    /// wrote (decoded as the kind it was given as), whatever the status.
    pub fn get(&self, object_type: sys::sai_object_type_t, oid: sys::sai_object_id_t, attrs: &mut [Attr]) -> Status {
        match self.tables.get(object_type) {
            dispatch::Slot::Missing => NOT_SUPPORTED,
            dispatch::Slot::Null => NOT_IMPLEMENTED,
            dispatch::Slot::Fn(f) => {
                let mut enc = value::Encoded::new(attrs);
                let st = ffi::get(f, oid, &mut enc);
                enc.decode(attrs);
                st
            }
        }
    }

    /// `get_<type>_stats(oid, n, ids, values)` (port, router interface, queue, priority group,
    /// buffer pool): the status and the values (what the library wrote, zero-initialised).
    pub fn stats(
        &self,
        object_type: sys::sai_object_type_t,
        oid: sys::sai_object_id_t,
        ids: &[sys::sai_stat_id_t],
    ) -> (Status, Vec<u64>) {
        let mut out = vec![0u64; ids.len()];
        let st = match self.tables.stats(object_type) {
            dispatch::Slot::Missing => NOT_SUPPORTED,
            dispatch::Slot::Null => NOT_IMPLEMENTED,
            dispatch::Slot::Fn(f) => ffi::stats(f, oid, ids, &mut out),
        };
        (st, out)
    }

    /// `get_<type>_stats_ext(oid, n, ids, mode, values)` (port, router interface, queue): the
    /// status and the values.
    pub fn stats_ext(
        &self,
        object_type: sys::sai_object_type_t,
        oid: sys::sai_object_id_t,
        ids: &[sys::sai_stat_id_t],
        mode: sys::sai_stats_mode_t,
    ) -> (Status, Vec<u64>) {
        let mut out = vec![0u64; ids.len()];
        let st = match self.tables.stats_ext(object_type) {
            dispatch::Slot::Missing => NOT_SUPPORTED,
            dispatch::Slot::Null => NOT_IMPLEMENTED,
            dispatch::Slot::Fn(f) => ffi::stats_ext(f, oid, ids, mode, &mut out),
        };
        (st, out)
    }

    /// `clear_<type>_stats(oid, n, ids)` (port, router interface, queue).
    pub fn clear_stats(
        &self,
        object_type: sys::sai_object_type_t,
        oid: sys::sai_object_id_t,
        ids: &[sys::sai_stat_id_t],
    ) -> Status {
        match self.tables.clear_stats(object_type) {
            dispatch::Slot::Missing => NOT_SUPPORTED,
            dispatch::Slot::Null => NOT_IMPLEMENTED,
            dispatch::Slot::Fn(f) => ffi::clear_stats(f, oid, ids),
        }
    }

    /// `clear_port_all_stats(port)`.
    pub fn clear_port_all_stats(&self, port: sys::sai_object_id_t) -> Status {
        match self.tables.clear_port_all_stats() {
            dispatch::Slot::Fn(f) => ffi::remove(f, port),
            _ => NOT_IMPLEMENTED,
        }
    }

    /// `get_<entry>_attribute` of a route / neighbour / FDB entry; afterwards every attribute holds
    /// what the library wrote (see [`Sai::get`]).
    pub fn get_entry(&self, switch: sys::sai_object_id_t, key: &Entry, attrs: &mut [Attr]) -> Status {
        let mut enc = value::Encoded::new(attrs);
        let st = self.tables.entry_get(switch, key, &mut enc);
        enc.decode(attrs);
        st
    }

    /// A route / neighbour / FDB entry call (`Set`: the first attribute).
    pub fn entry(&self, op: EntryOp, switch: sys::sai_object_id_t, key: &Entry, attrs: &[Attr]) -> Status {
        let enc = value::Encoded::new(attrs);
        self.tables.entry(op, switch, key, &enc)
    }

    /// `flush_fdb_entries(switch, attrs)`.
    pub fn flush_fdb(&self, switch: sys::sai_object_id_t, attrs: &[Attr]) -> Status {
        self.tables
            .fdb
            .and_then(|t| t.flush_fdb_entries)
            .map_or(NOT_IMPLEMENTED, |f| ffi::flush(f, switch, &value::Encoded::new(attrs)))
    }
}
