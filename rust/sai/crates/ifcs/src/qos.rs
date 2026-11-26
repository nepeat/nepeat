//! Queues and BAM queues, the `QoS` map list and the traffic-monitor policy (`tl_qos.cpp`,
//! `tl_mirror.cpp`): the keys and value structs as the C++ builds them (zeroed, the vendor's `_init`,
//! then the members), the queue attribute and counter calls. Each struct is kept as its bytes
//! (`ifcs-sys`'s byte twin, `bytes.rs`): zeroed, then its fields, so no padding byte libifcs
//! receives (a queue key it looks up, a policy copied into an attribute) is undefined.
//!
//! The key initialisers and setters and the queue attribute calls have the signature shapes of
//! `entry.rs` and go through its wrappers (`(K*)`, `(K*, u32)`, `(node, K*, count, attr*)`,
//! `(node, K*, count, attr*, u32*)`); the counter reads and clears are this module's two shapes, one
//! unsafe call site each: `(node, K*, count, int* ids, u64* values, u32* got)` and `(node, K*, count,
//! int* ids)` (`ifcs_bam_queue_t` is `ifcs_queue_t` in the header, so the queue and BAM queue
//! functions share them). The contract with libifcs is `call.rs`'s: pointers borrowed for the call,
//! sized as the counts say, nothing kept after it returns.
#![allow(unsafe_code)]

use core::ffi::c_int;
use core::mem::{align_of, offset_of, size_of};

use ifcs_sys as sys;

use crate::Ifcs;
use crate::attr::Attrs;
use crate::bytes::{CBytes, get_field, put_field};
use crate::entry::{entry_attrs, entry_attrs_get, key_handle, key_only};
use crate::handle::Handle;
use crate::status::Status;

/// The status answered for a table entry that is `None` (never: `Table::resolve` fills every
/// entry or fails): `IFCS_UNINIT`.
const MISSING: Status = Status(sys::IFCS_UNINIT.cast_signed());

type Queue = sys::ifcs_queue_t;
type MapInfo = sys::ifcs_qos_map_info_t;
type Policy = sys::ifcs_traffic_monitor_policy_t;

/// An `ifcs_queue_t` (also an `ifcs_bam_queue_t`: the header's typedef), as its bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct QueueKey(sys::ifcs_queue_t_bytes);

impl core::fmt::Debug for QueueKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("QueueKey")
            .field("devport", &self.devport())
            .field("queue_id", &self.queue_id())
            .field("dirty", &self.dirty())
            .finish_non_exhaustive()
    }
}

impl Default for QueueKey {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl QueueKey {
    /// All zero (the C++ `memset`).
    pub const fn zeroed() -> Self {
        Self(sys::ifcs_queue_t_bytes::ZERO)
    }

    /// `k.devport = devport` (the C++ sets the BAM queue key's members directly).
    pub fn set_devport(&mut self, devport: u32) {
        put_field!(self.0.as_bytes_mut(), 0, Queue, devport, devport);
    }

    /// `k.queue_id = id`.
    pub fn set_queue_id(&mut self, id: u32) {
        put_field!(self.0.as_bytes_mut(), 0, Queue, queue_id, id);
    }

    /// `k.dirty = bits`.
    pub fn set_dirty(&mut self, bits: u8) {
        put_field!(self.0.as_bytes_mut(), 0, Queue, dirty, bits);
    }

    /// `devport`.
    pub fn devport(&self) -> u32 {
        get_field!(self.0.as_bytes(), 0, Queue, devport)
    }

    /// `queue_id`.
    pub fn queue_id(&self) -> u32 {
        get_field!(self.0.as_bytes(), 0, Queue, queue_id)
    }

    /// `dirty`.
    pub fn dirty(&self) -> u8 {
        get_field!(self.0.as_bytes(), 0, Queue, dirty)
    }

    /// The key's bytes as libifcs sees them.
    pub fn bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    fn ptr(&mut self) -> *mut Queue {
        self.0.c_ptr()
    }
}

/// An `ifcs_qos_map_info_t` (`tl_qos.cpp` `map_info`), as its bytes: zeroed,
/// `ifcs_qos_map_info_t_init`, its `type`; its list is given to [`Attrs::qos_map_info`] (which owns
/// the entries for the call).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct QosMapInfo(sys::ifcs_qos_map_info_t_bytes);

impl core::fmt::Debug for QosMapInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("QosMapInfo")
            .field("type", &self.kind())
            .field("dirty", &self.dirty())
            .finish_non_exhaustive()
    }
}

impl Default for QosMapInfo {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl QosMapInfo {
    /// All zero (the C++ `memset`).
    pub const fn zeroed() -> Self {
        Self(sys::ifcs_qos_map_info_t_bytes::ZERO)
    }

    /// `info.type = t` (`IFCS_QOS_MAP_TYPE_*`).
    pub fn set_type(&mut self, t: u32) {
        put_field!(self.0.as_bytes_mut(), 0, MapInfo, type_, t.cast_signed());
    }

    /// `type`.
    pub fn kind(&self) -> i32 {
        get_field!(self.0.as_bytes(), 0, MapInfo, type_)
    }

    /// `dirty` (as `_init` left it).
    pub fn dirty(&self) -> u8 {
        get_field!(self.0.as_bytes(), 0, MapInfo, dirty)
    }

    fn ptr(&mut self) -> *mut MapInfo {
        self.0.c_ptr()
    }
}

/// The 32-bit words of one `ifcs_qos_map_t` (28 bytes, 4-aligned: a list of them is sent as words).
pub(crate) const QOS_MAP_WORDS: usize = size_of::<sys::ifcs_qos_map_t>() / 4;
const _: () = assert!(size_of::<sys::ifcs_qos_map_t>() == QOS_MAP_WORDS * 4);
const _: () = assert!(align_of::<sys::ifcs_qos_map_t>() <= align_of::<u32>());

const SRC_OFF: usize = offset_of!(sys::ifcs_qos_map_t, map_src);
const VALUE_OFF: usize = offset_of!(sys::ifcs_qos_map_t, map_value);
const TC_OFF: usize = offset_of!(sys::ifcs_qos_tc_dp_t, tc);
const DP_OFF: usize = offset_of!(sys::ifcs_qos_tc_dp_t, dp);
const DOT1P_OFF: usize = offset_of!(sys::ifcs_qos_dot1p_cfi_t, dot1p);
const CFI_OFF: usize = offset_of!(sys::ifcs_qos_dot1p_cfi_t, cfi);
// Every union member starts at the union's start (`map_src.dscp` there), and the members fit it.
const _: () = assert!(size_of::<sys::ifcs_qos_map_fields_t>() >= size_of::<sys::ifcs_qos_tc_dp_t>());
const _: () = assert!(size_of::<sys::ifcs_qos_map_fields_t>() >= size_of::<sys::ifcs_qos_dot1p_cfi_t>());

/// The source of a `QoS` map entry (the union member of `map_src`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QosMapSrc {
    /// `map_src.dscp`.
    Dscp(u32),
    /// `map_src.dot1p_cfi`.
    Dot1pCfi {
        /// `dot1p`.
        dot1p: u32,
        /// `cfi`.
        cfi: u8,
    },
}

/// An `ifcs_qos_map_t` list: each entry zeroed (the C++ `memset`), then `map_src` and
/// `map_value.tc_dp`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QosMapList {
    bytes: Vec<u8>,
}

fn put(b: &mut [u8], off: usize, v: &[u8]) {
    if let Some(d) = b.get_mut(off..off + v.len()) {
        d.copy_from_slice(v);
    }
}

impl QosMapList {
    /// An empty list.
    pub const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    /// One entry `src -> (tc, dp)`.
    pub fn push(&mut self, src: QosMapSrc, tc: u32, dp: u32) {
        let mut e = [0u8; QOS_MAP_WORDS * 4];
        match src {
            QosMapSrc::Dscp(d) => put(&mut e, SRC_OFF, &d.to_ne_bytes()),
            QosMapSrc::Dot1pCfi { dot1p, cfi } => {
                put(&mut e, SRC_OFF + DOT1P_OFF, &dot1p.to_ne_bytes());
                put(&mut e, SRC_OFF + CFI_OFF, &[cfi]);
            }
        }
        put(&mut e, VALUE_OFF + TC_OFF, &tc.to_ne_bytes());
        put(&mut e, VALUE_OFF + DP_OFF, &dp.to_ne_bytes());
        self.bytes.extend_from_slice(&e);
    }

    /// The number of entries.
    pub const fn len(&self) -> usize {
        self.bytes.len() / (QOS_MAP_WORDS * 4)
    }

    /// Whether it has none.
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The entries as the words libifcs reads (each entry `QOS_MAP_WORDS` words, native order).
    pub(crate) fn words(&self) -> Vec<u32> {
        self.bytes
            .chunks_exact(4)
            .map(|c| <[u8; 4]>::try_from(c).map_or(0, u32::from_ne_bytes))
            .collect()
    }
}

/// An `ifcs_traffic_monitor_policy_t` (`tl_mirror.cpp` `policy_set`), as its bytes: zeroed,
/// `ifcs_traffic_monitor_policy_t_init`, then its members; sent copied into the attribute's value
/// ([`MonitorPolicy::bytes`] with [`Attrs::bytes`]).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MonitorPolicy(sys::ifcs_traffic_monitor_policy_t_bytes);

impl core::fmt::Debug for MonitorPolicy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let b = self.0.as_bytes();
        let action: i32 = get_field!(b, 0, Policy, traffic_monitor_action);
        let set: u32 = get_field!(b, 0, Policy, collector_set);
        let container: i32 = get_field!(b, 0, Policy, traffic_monitor_container);
        let rate: u32 = get_field!(b, 0, Policy, sampling_rate);
        let dirty: u8 = get_field!(b, 0, Policy, dirty);
        f.debug_struct("MonitorPolicy")
            .field("action", &action)
            .field("collector_set", &set)
            .field("container", &container)
            .field("sampling_rate", &rate)
            .field("dirty", &dirty)
            .finish()
    }
}

impl Default for MonitorPolicy {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl MonitorPolicy {
    /// All zero (the C++ `memset`).
    pub const fn zeroed() -> Self {
        Self(sys::ifcs_traffic_monitor_policy_t_bytes::ZERO)
    }

    /// The members the C++ assigns after `_init`.
    pub fn set(&mut self, action: u32, set: Handle, container: u32, rate: u32) {
        let b = self.0.as_bytes_mut();
        put_field!(b, 0, Policy, traffic_monitor_action, action.cast_signed());
        put_field!(b, 0, Policy, collector_set, set.0);
        put_field!(b, 0, Policy, traffic_monitor_container, container.cast_signed());
        put_field!(b, 0, Policy, sampling_rate, rate);
    }

    /// The struct's bytes (`std::memcpy(&a.value, &p, sizeof p)`), every one: the policy as
    /// `_init` and [`MonitorPolicy::set`] left it, padding zero.
    pub fn bytes(&self) -> Vec<u8> {
        self.0.as_bytes().to_vec()
    }

    fn ptr(&mut self) -> *mut Policy {
        self.0.c_ptr()
    }
}
const _: () = assert!(size_of::<sys::ifcs_traffic_monitor_policy_t>() <= size_of::<sys::ifcs_attr_value_t>());

/// `(node, K*, u32 count, int* ids, u64* values, u32* got)`: queue / BAM queue counter reads.
fn key_stats_get(
    f: sys::ifcs_queue_stats_get_fn,
    node: u32,
    k: &mut QueueKey,
    ids: &mut [c_int],
    values: &mut [u64],
) -> (Status, u32) {
    let Some(f) = f else { return (MISSING, 0) };
    let Ok(count) = u32::try_from(ids.len()) else {
        return (Status(sys::IFCS_PARAM.cast_signed()), 0);
    };
    if values.len() != ids.len() {
        return (Status(sys::IFCS_PARAM.cast_signed()), 0);
    }
    let mut got = 0u32;
    // SAFETY: the module's contract: the key borrowed exclusively for the call; `ids` and `values`
    // hold `count` elements each (checked above), libifcs writes at most `count` values and `got`.
    let rc = unsafe {
        f(
            node,
            k.ptr(),
            count,
            ids.as_mut_ptr(),
            values.as_mut_ptr(),
            &raw mut got,
        )
    };
    (Status(rc), got)
}

/// `(node, K*, u32 count, int* ids)`: queue / BAM queue counter clears.
fn key_stats_clear(f: sys::ifcs_queue_stats_clear_fn, node: u32, k: &mut QueueKey, ids: &mut [c_int]) -> Status {
    let Some(f) = f else { return MISSING };
    let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
    // SAFETY: the module's contract; `ids` holds `count` ids.
    Status(unsafe { f(node, k.ptr(), count, ids.as_mut_ptr()) })
}

impl Ifcs {
    /// `ifcs_queue_t_init(&k)`.
    pub fn ifcs_queue_t_init(&self, k: &mut QueueKey) -> Status {
        key_only(self.t().ifcs_queue_t_init, k.ptr())
    }

    /// `ifcs_queue_t_devport_set(&k, devport)`.
    pub fn ifcs_queue_t_devport_set(&self, k: &mut QueueKey, devport: u32) -> Status {
        key_handle(self.t().ifcs_queue_t_devport_set, k.ptr(), Handle(devport))
    }

    /// `ifcs_queue_t_queue_id_set(&k, id)`.
    pub fn ifcs_queue_t_queue_id_set(&self, k: &mut QueueKey, id: u32) -> Status {
        key_handle(self.t().ifcs_queue_t_queue_id_set, k.ptr(), Handle(id))
    }

    /// `ifcs_queue_attr_set(node, &k, count, attrs)`.
    pub fn ifcs_queue_attr_set(&self, node: u32, k: &mut QueueKey, attrs: &mut Attrs) -> Status {
        entry_attrs(self.t().ifcs_queue_attr_set, node, k.ptr(), attrs)
    }

    /// `ifcs_queue_attr_get(node, &k, count, attrs, &got)`.
    pub fn ifcs_queue_attr_get(&self, node: u32, k: &mut QueueKey, attrs: &mut Attrs) -> (Status, u32) {
        entry_attrs_get(self.t().ifcs_queue_attr_get, node, k.ptr(), attrs)
    }

    /// `ifcs_queue_stats_get(node, &k, count, ids, values, &got)`: `values` as long as `ids` (else
    /// nothing is called and `IFCS_PARAM` is answered).
    pub fn ifcs_queue_stats_get(
        &self,
        node: u32,
        k: &mut QueueKey,
        ids: &mut [c_int],
        values: &mut [u64],
    ) -> (Status, u32) {
        key_stats_get(self.t().ifcs_queue_stats_get, node, k, ids, values)
    }

    /// `ifcs_queue_stats_clear(node, &k, count, ids)`.
    pub fn ifcs_queue_stats_clear(&self, node: u32, k: &mut QueueKey, ids: &mut [c_int]) -> Status {
        key_stats_clear(self.t().ifcs_queue_stats_clear, node, k, ids)
    }

    /// `ifcs_bam_queue_t_init(&k)`.
    pub fn ifcs_bam_queue_t_init(&self, k: &mut QueueKey) -> Status {
        key_only(self.t().ifcs_bam_queue_t_init, k.ptr())
    }

    /// `ifcs_bam_queue_stats_get(node, &k, count, ids, values, &got)`, as `ifcs_queue_stats_get`.
    pub fn ifcs_bam_queue_stats_get(
        &self,
        node: u32,
        k: &mut QueueKey,
        ids: &mut [c_int],
        values: &mut [u64],
    ) -> (Status, u32) {
        key_stats_get(self.t().ifcs_bam_queue_stats_get, node, k, ids, values)
    }

    /// `ifcs_bam_queue_stats_clear(node, &k, count, ids)`.
    pub fn ifcs_bam_queue_stats_clear(&self, node: u32, k: &mut QueueKey, ids: &mut [c_int]) -> Status {
        key_stats_clear(self.t().ifcs_bam_queue_stats_clear, node, k, ids)
    }

    /// `ifcs_qos_map_info_t_init(&info)`.
    pub fn ifcs_qos_map_info_t_init(&self, info: &mut QosMapInfo) -> Status {
        key_only(self.t().ifcs_qos_map_info_t_init, info.ptr())
    }

    /// `ifcs_traffic_monitor_policy_t_init(&p)`.
    pub fn ifcs_traffic_monitor_policy_t_init(&self, p: &mut MonitorPolicy) -> Status {
        key_only(self.t().ifcs_traffic_monitor_policy_t_init, p.ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_entries_sit_at_the_header_offsets() {
        let mut l = QosMapList::new();
        assert!(l.is_empty());
        l.push(QosMapSrc::Dscp(46), 5, 0);
        l.push(QosMapSrc::Dot1pCfi { dot1p: 3, cfi: 1 }, 4, 2);
        assert_eq!(l.len(), 2);
        let w = l.words();
        assert_eq!(w.len(), 2 * QOS_MAP_WORDS);
        assert_eq!(QOS_MAP_WORDS, 7);
        // entry 0: map_src.dscp = 46, map_value.tc_dp = {5, 0} (map_value at 12: word 3)
        assert_eq!(&w[..6], &[46, 0, 0, 5, 0, 0]);
        // entry 1: dot1p 3, cfi 1 (byte 4 of the source), tc 4, dp 2
        assert_eq!(w[7], 3);
        assert_eq!(w[8].to_ne_bytes()[0], 1);
        assert_eq!((w[10], w[11]), (4, 2));
    }

    #[test]
    fn a_policy_is_copied_member_by_member() {
        let mut p = MonitorPolicy::zeroed();
        p.set(1, Handle(0x55), 1, 4096);
        let b = p.bytes();
        assert_eq!(b.len(), 20);
        assert_eq!(b[0..4], 1i32.to_ne_bytes());
        assert_eq!(b[4..8], 0x55u32.to_ne_bytes());
        assert_eq!(b[8..12], 1i32.to_ne_bytes());
        assert_eq!(b[12..16], 4096u32.to_ne_bytes());
        assert_eq!(b[16..], [0; 4]);
        let mut k = QueueKey::zeroed();
        k.set_devport(3);
        k.set_queue_id(7);
        k.set_dirty(3);
        assert_eq!((k.devport(), k.queue_id(), k.dirty()), (3, 7, 3));
        assert_eq!(k.bytes()[13..], [0; 3]); // the padding after dirty
        let mut i = QosMapInfo::zeroed();
        i.set_type(sys::IFCS_QOS_MAP_TYPE_DSCP_TO_TC_DP);
        assert_eq!((i.kind(), i.dirty()), (1, 0));
    }
}
