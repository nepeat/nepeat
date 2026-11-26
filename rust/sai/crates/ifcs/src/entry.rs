//! L2 entries and route entries: their keys as the vendor's accessor sequences build them
//! (`tl_l2.cpp` `l2_key`, `tl_l3.cpp` `route_key`), and the entry calls (create, delete, attribute
//! get / set, the table walk of `ifcs_l2_entry_get_all`).
//!
//! The keys, the IP prefix and the union members passed by value are kept as their bytes (the
//! structs' byte twins of `ifcs-sys`, `bytes.rs`): zeroed when made (the C++ `memset`s them), every
//! field written and read at its compiler-given offset, so no byte libifcs receives is undefined
//! and a copy (a walk's key, a member read out of its key) keeps every byte. libifcs copies the
//! by-value members into its keys and compares and hashes the raw key bytes, padding included: a
//! Rust struct value with undefined padding made the CS8210 answer NOTFOUND for deletes of entries
//! it had created (`ifcs-sys` docs, "By-value structs"). A member libifcs writes through
//! (`mac_l2vni`, `ip_dest_l3vni`) is passed as a pointer into its key's bytes, which the asserts
//! below show is large and aligned enough for it.
//!
//! One private wrapper per C signature shape, generic over the key type (one unsafe call site for
//! every signature of that shape: `(K*)`, `(K*, int)`, `(K*, handle)`, `(K*, V*)`, `(K*, V)`,
//! `(node, K*, count, attr*)`, `(node, K*)`, `(node, K*, count, attr*, u32*)`), plus the walk and its
//! callback. The contract with libifcs is `call.rs`'s: pointers are borrowed for the call, sized as
//! the types and counts say; libifcs keeps none after returning.
//!
//! The walk's callback gets no Rust memory: its `void* user` is a number keying this module's
//! table of walks (as `callback.rs`), so a stray call finds nothing.
#![allow(unsafe_code)]

use core::ffi::{c_int, c_void};
use core::mem::{align_of, offset_of, size_of};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use ifcs_sys as sys;

use crate::Ifcs;
use crate::attr::Attrs;
use crate::bytes::{CBytes, get_field, member_ptr, twin_at};
use crate::handle::Handle;
use crate::status::Status;

/// The status answered for a table entry that is `None` (never: `Table::resolve` fills every
/// entry or fails): `IFCS_UNINIT`.
const MISSING: Status = Status(sys::IFCS_UNINIT.cast_signed());

type L2 = sys::ifcs_l2_entry_key_t;
type L2Member = sys::ifcs_l2_entry_key_mac_l2vni_t;
type Route = sys::ifcs_route_entry_key_t;
type RouteMember = sys::ifcs_route_entry_key_ip_dest_l3vni_t;
type Prefix = sys::ifcs_ip_prefix_t;

/// Where the key's union (`key`) starts: its members all start there.
const L2_KEY: usize = offset_of!(L2, key);
const ROUTE_KEY: usize = offset_of!(Route, key);

// The union members libifcs writes through a pointer into the key's bytes fit the union and are
// aligned there (the offset a multiple of the member's alignment, the key at least as aligned).
const _: () = assert!(size_of::<sys::ifcs_l2_entry_key_map_t>() >= size_of::<L2Member>());
const _: () = assert!(L2_KEY.is_multiple_of(align_of::<L2Member>()) && align_of::<L2>() >= align_of::<L2Member>());
const _: () = assert!(size_of::<sys::ifcs_route_entry_key_map_t>() >= size_of::<RouteMember>());
const _: () =
    assert!(ROUTE_KEY.is_multiple_of(align_of::<RouteMember>()) && align_of::<Route>() >= align_of::<RouteMember>());

/// The parity harness's negative control, in a TEST build only (`IFCS_POISON_PADDING=1` at build
/// time, build.rs): the bytes after a by-value member's `dirty` (its tail padding) made 0xa5, as the
/// stack left them before the members were byte twins (evidence rust-h2/06: 0xff, 0xed, 0xbd there).
#[cfg(ifcs_poison_padding)]
fn poison<T: CBytes>(mut t: T, dirty: usize) -> T {
    for b in t.as_bytes_mut().iter_mut().skip(dirty + 1) {
        *b = 0xa5;
    }
    t
}

/// A product build: the member as it is.
#[cfg(not(ifcs_poison_padding))]
const fn poison<T>(t: T, _dirty: usize) -> T {
    t
}

/// An `ifcs_l2_entry_key_t`, as its bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct L2Key(sys::ifcs_l2_entry_key_t_bytes);

impl core::fmt::Debug for L2Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("L2Key")
            .field("key_type", &self.key_type())
            .field("mac", &self.mac())
            .field("l2vni", &self.l2vni())
            .finish_non_exhaustive()
    }
}

impl Default for L2Key {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl L2Key {
    /// All zero (the C++ `memset`).
    pub const fn zeroed() -> Self {
        Self(sys::ifcs_l2_entry_key_t_bytes::ZERO)
    }

    /// `key_type`.
    pub fn key_type(&self) -> i32 {
        get_field!(self.0.as_bytes(), 0, L2, key_type)
    }

    /// `key.mac_l2vni` as it is now, every byte (the by-value argument of
    /// `ifcs_l2_entry_key_t_mac_l2vni_set`: the C++ passes `k.key.mac_l2vni`).
    pub fn mac_l2vni(&self) -> sys::ifcs_l2_entry_key_mac_l2vni_t_bytes {
        poison(twin_at(self.0.as_bytes(), L2_KEY), offset_of!(L2Member, dirty))
    }

    /// `key.mac_l2vni.mac_addr`.
    pub fn mac(&self) -> [u8; 6] {
        get_field!(self.0.as_bytes(), L2_KEY, L2Member, mac_addr)
    }

    /// `key.mac_l2vni.l2vni`.
    pub fn l2vni(&self) -> Handle {
        Handle(get_field!(self.0.as_bytes(), L2_KEY, L2Member, l2vni))
    }

    /// The key's bytes as libifcs sees them.
    pub fn bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    fn ptr(&mut self) -> *mut L2 {
        self.0.c_ptr()
    }

    /// `&k.key.mac_l2vni`: the union member at the union's start.
    fn mac_l2vni_ptr(&mut self) -> *mut L2Member {
        member_ptr(&mut self.0, L2_KEY)
    }
}

/// An `ifcs_ip_prefix_t`, as its bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct IpPrefix(sys::ifcs_ip_prefix_t_bytes);

impl core::fmt::Debug for IpPrefix {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IpPrefix")
            .field("addr_family", &self.addr_family())
            .field("addr", &self.addr())
            .field("mask", &self.mask())
            .finish_non_exhaustive()
    }
}

impl Default for IpPrefix {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl IpPrefix {
    /// All zero (the C++ `memset`).
    pub const fn zeroed() -> Self {
        Self(sys::ifcs_ip_prefix_t_bytes::ZERO)
    }

    /// `addr_family`.
    pub fn addr_family(&self) -> i32 {
        get_field!(self.0.as_bytes(), 0, Prefix, addr_family)
    }

    /// `addr`'s bytes.
    pub fn addr(&self) -> [u8; 16] {
        get_field!(self.0.as_bytes(), 0, Prefix, addr.blob)
    }

    /// `mask`'s bytes.
    pub fn mask(&self) -> [u8; 16] {
        get_field!(self.0.as_bytes(), 0, Prefix, mask.blob)
    }

    /// The prefix's bytes as libifcs left them.
    pub fn bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    fn ptr(&mut self) -> *mut Prefix {
        self.0.c_ptr()
    }
}

/// An `ifcs_route_entry_key_t`, as its bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RouteKey(sys::ifcs_route_entry_key_t_bytes);

impl core::fmt::Debug for RouteKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RouteKey")
            .field("key_type", &self.key_type())
            .field("bytes", &self.0.as_bytes())
            .finish_non_exhaustive()
    }
}

impl Default for RouteKey {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl RouteKey {
    /// All zero (the C++ `memset`).
    pub const fn zeroed() -> Self {
        Self(sys::ifcs_route_entry_key_t_bytes::ZERO)
    }

    /// `key_type`.
    pub fn key_type(&self) -> i32 {
        get_field!(self.0.as_bytes(), 0, Route, key_type)
    }

    /// `key.ip_dest_l3vni` as it is now, every byte (the by-value argument of
    /// `ifcs_route_entry_key_t_ip_dest_l3vni_set`: the C++ passes `k.key.ip_dest_l3vni`).
    pub fn ip_dest_l3vni(&self) -> sys::ifcs_route_entry_key_ip_dest_l3vni_t_bytes {
        poison(twin_at(self.0.as_bytes(), ROUTE_KEY), offset_of!(RouteMember, dirty))
    }

    /// `key.ip_dest_l3vni.l3vni`.
    pub fn l3vni(&self) -> Handle {
        Handle(get_field!(self.0.as_bytes(), ROUTE_KEY, RouteMember, l3vni))
    }

    /// `key.ip_dest_l3vni.ip_dest`, copied (every byte).
    pub fn ip_dest(&self) -> IpPrefix {
        IpPrefix(twin_at(self.0.as_bytes(), ROUTE_KEY + offset_of!(RouteMember, ip_dest)))
    }

    /// The key's bytes as libifcs sees them.
    pub fn bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    fn ptr(&mut self) -> *mut Route {
        self.0.c_ptr()
    }

    /// `&k.key.ip_dest_l3vni`: the union member at the union's start.
    fn ip_dest_l3vni_ptr(&mut self) -> *mut RouteMember {
        member_ptr(&mut self.0, ROUTE_KEY)
    }
}

/// `(K*)`: key / prefix initialisers (also the queue keys and value structs of `qos.rs`).
pub(crate) fn key_only<K>(f: Option<unsafe extern "C" fn(*mut K) -> sys::ifcs_status_t>, k: *mut K) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: the module's contract; `k` points at a key or prefix of type `K` the caller borrows
    // exclusively for the call (made from a `&mut` right before it).
    Status(unsafe { f(k) })
}

/// `(K*, int)`: key type and address family setters.
fn key_int<K>(f: Option<unsafe extern "C" fn(*mut K, c_int) -> sys::ifcs_status_t>, k: *mut K, v: c_int) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: as `key_only`.
    Status(unsafe { f(k, v) })
}

/// `(K*, ifcs_handle_t)`: the L2VNI / L3VNI of a key member (also `qos.rs`'s queue key setters: their
/// `ifcs_devport_t` / `uint32_t` argument is `ifcs_handle_t`'s type, so the signature is this one).
pub(crate) fn key_handle<K>(
    f: Option<unsafe extern "C" fn(*mut K, sys::ifcs_handle_t) -> sys::ifcs_status_t>,
    k: *mut K,
    h: Handle,
) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: as `key_only`; `k` may point into a key's union blob (sized and aligned for `K`:
    // the module's asserts).
    Status(unsafe { f(k, h.0) })
}

/// `(K*, V*)`: MAC, address, mask and prefix setters (libifcs copies `*v`).
fn key_ptr<K, V>(
    f: Option<unsafe extern "C" fn(*mut K, *mut V) -> sys::ifcs_status_t>,
    k: *mut K,
    v: *mut V,
) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: as `key_handle`; `v` points at a value of the type the function reads (every byte
    // defined: a byte array, or a byte twin), made from a `&mut` right before the call.
    Status(unsafe { f(k, v) })
}

/// `(K*, V)`: a key's union member set by value; `V` is the member's byte twin, which the
/// `<name>_fn` typedef declares in the struct's place (`ifcs-sys` docs, "By-value structs").
fn key_value<K, V: CBytes>(
    f: Option<unsafe extern "C" fn(*mut K, V) -> sys::ifcs_status_t>,
    k: *mut K,
    v: V,
) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: as `key_only`; `v` is the byte twin of the C parameter's struct (`V::C`), passed by
    // value as the target's C passes that struct (same size and alignment, integer members only:
    // `ifcs-sys` "By-value structs", proven by tests/abi.rs); every byte of it is defined.
    Status(unsafe { f(k, v) })
}

/// `(node, K*, u32 count, ifcs_attr_t*)`: entry create and attribute set (also `qos.rs`'s queue
/// attribute set).
pub(crate) fn entry_attrs<K>(
    f: Option<unsafe extern "C" fn(sys::ifcs_node_id_t, *mut K, u32, *mut sys::ifcs_attr_t) -> sys::ifcs_status_t>,
    node: u32,
    k: *mut K,
    attrs: &mut Attrs,
) -> Status {
    let Some(f) = f else { return MISSING };
    let (n, p) = attrs.args();
    // SAFETY: the module's contract: the key as `key_only`, `p` points at `n` attributes of
    // `attrs` (or is NULL), alive for the call.
    Status(unsafe { f(node, k, n, p) })
}

/// `(node, K*)`: entry delete.
fn entry_key<K>(
    f: Option<unsafe extern "C" fn(sys::ifcs_node_id_t, *mut K) -> sys::ifcs_status_t>,
    node: u32,
    k: *mut K,
) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: as `key_only`.
    Status(unsafe { f(node, k) })
}

/// `(node, K*, u32 count, ifcs_attr_t*, u32* got)`: entry attribute get (also `qos.rs`'s queue
/// attribute get).
pub(crate) fn entry_attrs_get<K>(
    f: Option<
        unsafe extern "C" fn(sys::ifcs_node_id_t, *mut K, u32, *mut sys::ifcs_attr_t, *mut u32) -> sys::ifcs_status_t,
    >,
    node: u32,
    k: *mut K,
    attrs: &mut Attrs,
) -> (Status, u32) {
    let Some(f) = f else { return (MISSING, 0) };
    let (n, p) = attrs.args();
    let mut got = 0u32;
    // SAFETY: as `entry_attrs`; libifcs writes the values of the `n` attributes and `got`.
    let rc = unsafe { f(node, k, n, p, &raw mut got) };
    (Status(rc), got)
}

/// The walks in progress: their number (the callback's `user`) -> the keys seen so far.
static WALKS: Mutex<BTreeMap<usize, Vec<L2Key>>> = Mutex::new(BTreeMap::new());
static NEXT_WALK: AtomicUsize = AtomicUsize::new(1);

/// `ifcs_l2_entry_user_cb_t` of the walk: copies the key out (`tl_l2.cpp` `on_l2_entry`) into the
/// walk `user` names; a stray `user` finds no walk and nothing happens.
extern "C" fn collect_l2(
    _node: sys::ifcs_node_id_t,
    key: *mut sys::ifcs_l2_entry_key_t,
    _count: u32,
    _attrs: *mut sys::ifcs_attr_t,
    user: *mut c_void,
) {
    if key.is_null() {
        return;
    }
    // SAFETY: libifcs passes a valid key (checked non-null, aligned for ifcs_l2_entry_key_t, whose
    // byte twin has its size and alignment) for the duration of the call; its bytes are copied out
    // at once, every one (a later delete with this key must match libifcs' bytes, padding included).
    // Memory libifcs wrote holds the bytes it wrote (no undefined bytes cross the C boundary).
    let k = unsafe { key.cast::<sys::ifcs_l2_entry_key_t_bytes>().read() };
    if let Some(keys) = WALKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_mut(&user.addr())
    {
        keys.push(L2Key(k));
    }
}

/// `ifcs_l2_entry_get_all(node, NULL, 0, NULL, cb, user, &count)`: the whole table.
fn walk(f: sys::ifcs_l2_entry_get_all_fn, node: u32) -> (Status, Vec<L2Key>, u32) {
    let Some(f) = f else {
        return (MISSING, Vec::new(), 0);
    };
    let id = NEXT_WALK.fetch_add(1, Ordering::Relaxed);
    WALKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, Vec::new());
    let mut count = 0u32;
    // SAFETY: the module's contract: a NULL key and attribute list with count 0 (the whole table,
    // the C++ form); the callback is `collect_l2`, which reads only its key argument; `user` is the
    // walk's number (never dereferenced); libifcs writes `count`. The table lock is not held.
    let rc = unsafe {
        f(
            node,
            core::ptr::null_mut(),
            0,
            core::ptr::null_mut(),
            Some(collect_l2),
            core::ptr::without_provenance_mut(id),
            &raw mut count,
        )
    };
    let keys = WALKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id)
        .unwrap_or_default();
    (Status(rc), keys, count)
}

impl Ifcs {
    /// `ifcs_l2_entry_key_t_init(&k)`.
    pub fn ifcs_l2_entry_key_t_init(&self, k: &mut L2Key) -> Status {
        key_only(self.t().ifcs_l2_entry_key_t_init, k.ptr())
    }

    /// `ifcs_l2_entry_key_t_key_type_set(&k, type)`.
    pub fn ifcs_l2_entry_key_t_key_type_set(&self, k: &mut L2Key, key_type: i32) -> Status {
        key_int(self.t().ifcs_l2_entry_key_t_key_type_set, k.ptr(), key_type)
    }

    /// `ifcs_l2_entry_key_t_mac_l2vni_set(&k, v)`.
    pub fn ifcs_l2_entry_key_t_mac_l2vni_set(
        &self,
        k: &mut L2Key,
        v: sys::ifcs_l2_entry_key_mac_l2vni_t_bytes,
    ) -> Status {
        key_value(self.t().ifcs_l2_entry_key_t_mac_l2vni_set, k.ptr(), v)
    }

    /// `ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(&k.key.mac_l2vni, &mac)`.
    pub fn ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(&self, k: &mut L2Key, mac: [u8; 6]) -> Status {
        let mut m: sys::ifcs_mac_t = mac;
        key_ptr(
            self.t().ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set,
            k.mac_l2vni_ptr(),
            &raw mut m,
        )
    }

    /// `ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(&k.key.mac_l2vni, l2vni)`.
    pub fn ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(&self, k: &mut L2Key, l2vni: Handle) -> Status {
        key_handle(
            self.t().ifcs_l2_entry_key_mac_l2vni_t_l2vni_set,
            k.mac_l2vni_ptr(),
            l2vni,
        )
    }

    /// `ifcs_l2_entry_create(node, &k, count, attrs)`.
    pub fn ifcs_l2_entry_create(&self, node: u32, k: &mut L2Key, attrs: &mut Attrs) -> Status {
        entry_attrs(self.t().ifcs_l2_entry_create, node, k.ptr(), attrs)
    }

    /// `ifcs_l2_entry_delete(node, &k)`.
    pub fn ifcs_l2_entry_delete(&self, node: u32, k: &mut L2Key) -> Status {
        entry_key(self.t().ifcs_l2_entry_delete, node, k.ptr())
    }

    /// `ifcs_l2_entry_attr_get(node, &k, count, attrs, &got)`.
    pub fn ifcs_l2_entry_attr_get(&self, node: u32, k: &mut L2Key, attrs: &mut Attrs) -> (Status, u32) {
        entry_attrs_get(self.t().ifcs_l2_entry_attr_get, node, k.ptr(), attrs)
    }

    /// `ifcs_l2_entry_get_all(node, NULL, 0, NULL, cb, user, &count)`: every key of the table, in the
    /// order libifcs reported them, and its count.
    pub fn ifcs_l2_entry_get_all(&self, node: u32) -> (Status, Vec<L2Key>, u32) {
        walk(self.t().ifcs_l2_entry_get_all, node)
    }

    /// `ifcs_ip_prefix_t_init(&p)`.
    pub fn ifcs_ip_prefix_t_init(&self, p: &mut IpPrefix) -> Status {
        key_only(self.t().ifcs_ip_prefix_t_init, p.ptr())
    }

    /// `ifcs_ip_prefix_t_addr_family_set(&p, family)`.
    pub fn ifcs_ip_prefix_t_addr_family_set(&self, p: &mut IpPrefix, family: i32) -> Status {
        key_int(self.t().ifcs_ip_prefix_t_addr_family_set, p.ptr(), family)
    }

    /// `ifcs_ip_prefix_t_addr_set(&p, &addr)`: `addr` is the `ifcs_ip_addr_t`'s bytes.
    pub fn ifcs_ip_prefix_t_addr_set(&self, p: &mut IpPrefix, addr: [u8; 16]) -> Status {
        let mut a = sys::ifcs_ip_addr_t { blob: addr }; // a byte array: no padding
        key_ptr(self.t().ifcs_ip_prefix_t_addr_set, p.ptr(), &raw mut a)
    }

    /// `ifcs_ip_prefix_t_mask_set(&p, &mask)`.
    pub fn ifcs_ip_prefix_t_mask_set(&self, p: &mut IpPrefix, mask: [u8; 16]) -> Status {
        let mut m = sys::ifcs_ip_addr_t { blob: mask };
        key_ptr(self.t().ifcs_ip_prefix_t_mask_set, p.ptr(), &raw mut m)
    }

    /// `ifcs_route_entry_key_t_init(&k)`.
    pub fn ifcs_route_entry_key_t_init(&self, k: &mut RouteKey) -> Status {
        key_only(self.t().ifcs_route_entry_key_t_init, k.ptr())
    }

    /// `ifcs_route_entry_key_t_key_type_set(&k, type)`.
    pub fn ifcs_route_entry_key_t_key_type_set(&self, k: &mut RouteKey, key_type: i32) -> Status {
        key_int(self.t().ifcs_route_entry_key_t_key_type_set, k.ptr(), key_type)
    }

    /// `ifcs_route_entry_key_t_ip_dest_l3vni_set(&k, v)`.
    pub fn ifcs_route_entry_key_t_ip_dest_l3vni_set(
        &self,
        k: &mut RouteKey,
        v: sys::ifcs_route_entry_key_ip_dest_l3vni_t_bytes,
    ) -> Status {
        key_value(self.t().ifcs_route_entry_key_t_ip_dest_l3vni_set, k.ptr(), v)
    }

    /// `ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(&k.key.ip_dest_l3vni, l3vni)`.
    pub fn ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(&self, k: &mut RouteKey, l3vni: Handle) -> Status {
        key_handle(
            self.t().ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set,
            k.ip_dest_l3vni_ptr(),
            l3vni,
        )
    }

    /// `ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(&k.key.ip_dest_l3vni, &p)`.
    pub fn ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(&self, k: &mut RouteKey, p: &mut IpPrefix) -> Status {
        key_ptr(
            self.t().ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set,
            k.ip_dest_l3vni_ptr(),
            p.ptr(),
        )
    }

    /// `ifcs_route_entry_create(node, &k, count, attrs)`.
    pub fn ifcs_route_entry_create(&self, node: u32, k: &mut RouteKey, attrs: &mut Attrs) -> Status {
        entry_attrs(self.t().ifcs_route_entry_create, node, k.ptr(), attrs)
    }

    /// `ifcs_route_entry_attr_set(node, &k, count, attrs)`.
    pub fn ifcs_route_entry_attr_set(&self, node: u32, k: &mut RouteKey, attrs: &mut Attrs) -> Status {
        entry_attrs(self.t().ifcs_route_entry_attr_set, node, k.ptr(), attrs)
    }

    /// `ifcs_route_entry_delete(node, &k)`.
    pub fn ifcs_route_entry_delete(&self, node: u32, k: &mut RouteKey) -> Status {
        entry_key(self.t().ifcs_route_entry_delete, node, k.ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_their_union_members_at_the_header_offsets() {
        let mut k = L2Key::zeroed();
        assert_eq!(k.key_type(), 0);
        assert_eq!((k.mac(), k.l2vni()), ([0; 6], Handle::NULL));
        let mac_off = L2_KEY + offset_of!(L2Member, mac_addr);
        let vni_off = L2_KEY + offset_of!(L2Member, l2vni);
        k.0.0[mac_off..mac_off + 6].copy_from_slice(&[2, 0x11, 0x22, 0x33, 0x44, 0x55]);
        k.0.0[vni_off..vni_off + 4].copy_from_slice(&Handle::l2vni(100).0.to_ne_bytes());
        assert_eq!(k.mac(), [2, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(k.l2vni(), Handle::l2vni(100));
        let m = k.mac_l2vni();
        assert_eq!(m.0[..], k.bytes()[L2_KEY..L2_KEY + size_of::<L2Member>()]);
        assert_eq!(k.mac_l2vni_ptr().cast::<u8>(), k.0.0[L2_KEY..].as_mut_ptr());
        assert_eq!(k.bytes().len(), size_of::<L2>());

        let mut r = RouteKey::zeroed();
        let off = ROUTE_KEY + offset_of!(RouteMember, ip_dest) + offset_of!(Prefix, addr_family);
        r.0.0[off..off + 4].copy_from_slice(&1i32.to_ne_bytes());
        let l3 = ROUTE_KEY + offset_of!(RouteMember, l3vni);
        r.0.0[l3..l3 + 4].copy_from_slice(&Handle::l3vni(1).0.to_ne_bytes());
        assert_eq!((r.ip_dest().addr_family(), r.l3vni()), (1, Handle::l3vni(1)));
        assert_eq!(
            r.ip_dest_l3vni().0[..],
            r.bytes()[ROUTE_KEY..ROUTE_KEY + size_of::<RouteMember>()]
        );
        assert_eq!(IpPrefix::zeroed().addr(), [0; 16]);
    }

    #[test]
    fn every_byte_is_zero_when_made_and_kept_by_copies() {
        assert!(L2Key::zeroed().bytes().iter().all(|b| *b == 0));
        assert!(RouteKey::zeroed().bytes().iter().all(|b| *b == 0));
        assert!(IpPrefix::zeroed().bytes().iter().all(|b| *b == 0));
        // a key with a byte in what is padding of the member: a copy and the member read keep it
        let mut k = RouteKey::zeroed();
        let pad = ROUTE_KEY + size_of::<RouteMember>() - 1;
        k.0.0[pad] = 0xed;
        let c = k;
        assert_eq!(c.bytes()[pad], 0xed);
        assert_eq!(c.ip_dest_l3vni().0[size_of::<RouteMember>() - 1], 0xed);
    }

    #[test]
    fn a_stray_walk_callback_does_nothing() {
        let mut k = L2Key::zeroed();
        collect_l2(
            0,
            k.ptr(),
            0,
            core::ptr::null_mut(),
            core::ptr::without_provenance_mut(usize::MAX),
        );
        collect_l2(
            0,
            core::ptr::null_mut(),
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        assert!(!WALKS.lock().unwrap().contains_key(&usize::MAX));
    }
}
