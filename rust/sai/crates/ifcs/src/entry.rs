//! L2 entries and route entries: their keys as the vendor's accessor sequences build them
//! (`tl_l2.cpp` `l2_key`, `tl_l3.cpp` `route_key`), and the entry calls (create, delete, attribute
//! get / set, the table walk of `ifcs_l2_entry_get_all`).
//!
//! The keys are the header's structs, owned here and zeroed when made (the C++ `memset`s them);
//! their unions are byte blobs in `ifcs-sys`, so a member is read and written as bytes at its
//! (compiler-checked) offsets, and a member libifcs writes through (`mac_l2vni`, `ip_dest_l3vni`)
//! is passed as a pointer into the blob, which the asserts below show is large and aligned
//! enough for it.
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
use crate::handle::Handle;
use crate::status::Status;

/// The status answered for a table entry that is `None` (never: `Table::resolve` fills every
/// entry or fails): `IFCS_UNINIT`.
const MISSING: Status = Status(sys::IFCS_UNINIT.cast_signed());

// The union members libifcs writes through a pointer into the key's blob fit it.
const _: () = assert!(size_of::<sys::ifcs_l2_entry_key_map_t>() >= size_of::<sys::ifcs_l2_entry_key_mac_l2vni_t>());
const _: () = assert!(align_of::<sys::ifcs_l2_entry_key_map_t>() >= align_of::<sys::ifcs_l2_entry_key_mac_l2vni_t>());
const _: () =
    assert!(size_of::<sys::ifcs_route_entry_key_map_t>() >= size_of::<sys::ifcs_route_entry_key_ip_dest_l3vni_t>());
const _: () =
    assert!(align_of::<sys::ifcs_route_entry_key_map_t>() >= align_of::<sys::ifcs_route_entry_key_ip_dest_l3vni_t>());

const L2_BLOB: usize = size_of::<sys::ifcs_l2_entry_key_map_t>();
const ROUTE_BLOB: usize = size_of::<sys::ifcs_route_entry_key_map_t>();
const MAC_OFF: usize = offset_of!(sys::ifcs_l2_entry_key_mac_l2vni_t, mac_addr);
const L2VNI_OFF: usize = offset_of!(sys::ifcs_l2_entry_key_mac_l2vni_t, l2vni);
const L2_DIRTY_OFF: usize = offset_of!(sys::ifcs_l2_entry_key_mac_l2vni_t, dirty);

fn bytes<const N: usize>(blob: &[u8], off: usize) -> [u8; N] {
    let mut out = [0u8; N];
    if let Some(src) = blob.get(off..off + N) {
        out.copy_from_slice(src);
    }
    out
}

fn u32_at(blob: &[u8], off: usize) -> u32 {
    u32::from_ne_bytes(bytes(blob, off))
}

fn prefix_from(blob: &[u8], off: usize) -> sys::ifcs_ip_prefix_t {
    sys::ifcs_ip_prefix_t {
        addr_family: c_int::from_ne_bytes(bytes(blob, off + offset_of!(sys::ifcs_ip_prefix_t, addr_family))),
        addr: sys::ifcs_ip_addr_t {
            blob: bytes(blob, off + offset_of!(sys::ifcs_ip_prefix_t, addr)),
        },
        mask: sys::ifcs_ip_addr_t {
            blob: bytes(blob, off + offset_of!(sys::ifcs_ip_prefix_t, mask)),
        },
        dirty: bytes::<1>(blob, off + offset_of!(sys::ifcs_ip_prefix_t, dirty))[0],
    }
}

/// An `ifcs_l2_entry_key_t`.
#[derive(Clone, Copy)]
pub struct L2Key(sys::ifcs_l2_entry_key_t);

impl core::fmt::Debug for L2Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("L2Key")
            .field("key_type", &self.0.key_type)
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
        Self(sys::ifcs_l2_entry_key_t {
            _base_key: 0,
            key_type: 0,
            key: sys::ifcs_l2_entry_key_map_t { blob: [0; L2_BLOB] },
            dirty: 0,
        })
    }

    /// `key_type`.
    pub const fn key_type(&self) -> i32 {
        self.0.key_type
    }

    /// `key.mac_l2vni` as it is now (the by-value argument of `ifcs_l2_entry_key_t_mac_l2vni_set`).
    pub fn mac_l2vni(&self) -> sys::ifcs_l2_entry_key_mac_l2vni_t {
        let b = &self.0.key.blob;
        sys::ifcs_l2_entry_key_mac_l2vni_t {
            mac_addr: bytes(b, MAC_OFF),
            l2vni: u32_at(b, L2VNI_OFF),
            dirty: bytes::<1>(b, L2_DIRTY_OFF)[0],
        }
    }

    /// `key.mac_l2vni.mac_addr`.
    pub fn mac(&self) -> [u8; 6] {
        bytes(&self.0.key.blob, MAC_OFF)
    }

    /// `key.mac_l2vni.l2vni`.
    pub fn l2vni(&self) -> Handle {
        Handle(u32_at(&self.0.key.blob, L2VNI_OFF))
    }

    /// The key as libifcs sees it.
    pub const fn raw(&self) -> &sys::ifcs_l2_entry_key_t {
        &self.0
    }

    fn ptr(&mut self) -> *mut sys::ifcs_l2_entry_key_t {
        &raw mut self.0
    }

    /// `&k.key.mac_l2vni`: the union member at the blob's start.
    fn mac_l2vni_ptr(&mut self) -> *mut sys::ifcs_l2_entry_key_mac_l2vni_t {
        (&raw mut self.0.key).cast()
    }
}

/// An `ifcs_ip_prefix_t`.
#[derive(Clone, Copy)]
pub struct IpPrefix(sys::ifcs_ip_prefix_t);

impl core::fmt::Debug for IpPrefix {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IpPrefix")
            .field("addr_family", &self.0.addr_family)
            .field("addr", &self.0.addr.blob)
            .field("mask", &self.0.mask.blob)
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
        Self(sys::ifcs_ip_prefix_t {
            addr_family: 0,
            addr: sys::ifcs_ip_addr_t { blob: [0; 16] },
            mask: sys::ifcs_ip_addr_t { blob: [0; 16] },
            dirty: 0,
        })
    }

    /// The prefix as libifcs left it.
    pub const fn raw(&self) -> &sys::ifcs_ip_prefix_t {
        &self.0
    }

    fn ptr(&mut self) -> *mut sys::ifcs_ip_prefix_t {
        &raw mut self.0
    }
}

/// An `ifcs_route_entry_key_t`.
#[derive(Clone, Copy)]
pub struct RouteKey(sys::ifcs_route_entry_key_t);

impl core::fmt::Debug for RouteKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RouteKey")
            .field("key_type", &self.0.key_type)
            .field("key", &self.0.key.blob)
            .finish_non_exhaustive()
    }
}

impl Default for RouteKey {
    fn default() -> Self {
        Self::zeroed()
    }
}

const ROUTE_L3VNI_OFF: usize = offset_of!(sys::ifcs_route_entry_key_ip_dest_l3vni_t, l3vni);
const ROUTE_DEST_OFF: usize = offset_of!(sys::ifcs_route_entry_key_ip_dest_l3vni_t, ip_dest);
const ROUTE_DIRTY_OFF: usize = offset_of!(sys::ifcs_route_entry_key_ip_dest_l3vni_t, dirty);

impl RouteKey {
    /// All zero (the C++ `memset`).
    pub const fn zeroed() -> Self {
        Self(sys::ifcs_route_entry_key_t {
            _base_key: 0,
            key_type: 0,
            key: sys::ifcs_route_entry_key_map_t { blob: [0; ROUTE_BLOB] },
            dirty: 0,
        })
    }

    /// `key.ip_dest_l3vni` as it is now (the by-value argument of
    /// `ifcs_route_entry_key_t_ip_dest_l3vni_set`).
    pub fn ip_dest_l3vni(&self) -> sys::ifcs_route_entry_key_ip_dest_l3vni_t {
        let b = &self.0.key.blob;
        sys::ifcs_route_entry_key_ip_dest_l3vni_t {
            ip_dest: prefix_from(b, ROUTE_DEST_OFF),
            l3vni: u32_at(b, ROUTE_L3VNI_OFF),
            dirty: bytes::<1>(b, ROUTE_DIRTY_OFF)[0],
        }
    }

    /// The key as libifcs sees it.
    pub const fn raw(&self) -> &sys::ifcs_route_entry_key_t {
        &self.0
    }

    fn ptr(&mut self) -> *mut sys::ifcs_route_entry_key_t {
        &raw mut self.0
    }

    /// `&k.key.ip_dest_l3vni`: the union member at the blob's start.
    fn ip_dest_l3vni_ptr(&mut self) -> *mut sys::ifcs_route_entry_key_ip_dest_l3vni_t {
        (&raw mut self.0.key).cast()
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
    v: &mut V,
) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: as `key_handle`; `v` is a value of the type the function reads, borrowed for the call.
    Status(unsafe { f(k, v) })
}

/// `(K*, V)`: a key's union member set by value.
fn key_value<K, V>(f: Option<unsafe extern "C" fn(*mut K, V) -> sys::ifcs_status_t>, k: *mut K, v: V) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: as `key_only`; `v` is passed by value (a plain C struct of the declared type).
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
    // SAFETY: libifcs passes a valid key (checked non-null) for the duration of the call; it is
    // copied out at once.
    let k = unsafe { *key };
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
    pub fn ifcs_l2_entry_key_t_mac_l2vni_set(&self, k: &mut L2Key, v: sys::ifcs_l2_entry_key_mac_l2vni_t) -> Status {
        key_value(self.t().ifcs_l2_entry_key_t_mac_l2vni_set, k.ptr(), v)
    }

    /// `ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(&k.key.mac_l2vni, &mac)`.
    pub fn ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(&self, k: &mut L2Key, mac: [u8; 6]) -> Status {
        let mut m: sys::ifcs_mac_t = mac;
        key_ptr(
            self.t().ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set,
            k.mac_l2vni_ptr(),
            &mut m,
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
        let mut a = sys::ifcs_ip_addr_t { blob: addr };
        key_ptr(self.t().ifcs_ip_prefix_t_addr_set, p.ptr(), &mut a)
    }

    /// `ifcs_ip_prefix_t_mask_set(&p, &mask)`.
    pub fn ifcs_ip_prefix_t_mask_set(&self, p: &mut IpPrefix, mask: [u8; 16]) -> Status {
        let mut m = sys::ifcs_ip_addr_t { blob: mask };
        key_ptr(self.t().ifcs_ip_prefix_t_mask_set, p.ptr(), &mut m)
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
        v: sys::ifcs_route_entry_key_ip_dest_l3vni_t,
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
            &mut p.0,
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
        k.0.key.blob[MAC_OFF..MAC_OFF + 6].copy_from_slice(&[2, 0x11, 0x22, 0x33, 0x44, 0x55]);
        k.0.key.blob[L2VNI_OFF..L2VNI_OFF + 4].copy_from_slice(&Handle::l2vni(100).0.to_ne_bytes());
        assert_eq!(k.mac(), [2, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(k.l2vni(), Handle::l2vni(100));
        let m = k.mac_l2vni();
        assert_eq!((m.mac_addr, m.l2vni, m.dirty), (k.mac(), Handle::l2vni(100).0, 0));
        assert_eq!(k.mac_l2vni_ptr().cast::<u8>(), (&raw mut k.0.key.blob).cast::<u8>());

        let mut r = RouteKey::zeroed();
        let off = ROUTE_DEST_OFF + offset_of!(sys::ifcs_ip_prefix_t, addr_family);
        r.0.key.blob[off..off + 4].copy_from_slice(&1i32.to_ne_bytes());
        r.0.key.blob[ROUTE_L3VNI_OFF..ROUTE_L3VNI_OFF + 4].copy_from_slice(&Handle::l3vni(1).0.to_ne_bytes());
        let v = r.ip_dest_l3vni();
        assert_eq!((v.ip_dest.addr_family, v.l3vni), (1, Handle::l3vni(1).0));
        assert_eq!(IpPrefix::zeroed().raw().addr.blob, [0; 16]);
    }

    #[test]
    fn a_stray_walk_callback_does_nothing() {
        let mut k = L2Key::zeroed().0;
        collect_l2(
            0,
            &raw mut k,
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
