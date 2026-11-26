//! `ifcs_attr_t` lists, built and read in safe code.
//!
//! An attribute is an id, a 120-byte value union (opaque in `ifcs-sys`: a blob; every member is
//! at offset 0) and a bitmap. The C++ builds them zeroed with the member assigned directly, bitmap
//! 0 (`tl_core.h` `attr_of`, `attr_u32`, ...: the vendor CLI's form); so does [`Attrs`]. Members
//! are written as their bytes at the offsets of their (checked) structs; list members point at
//! buffers [`Attrs`] owns, filled in by [`Attrs::args`] right before a call, so no pointer
//! outlives the borrow it came from.
//!
//! The three forms of an attribute argument the C++ passes, kept apart because libifcs sees them:
//! [`Attrs::null`] (count 0, `NULL`: `ifcs_node_create(0, 0, NULL)`), [`Attrs::none`] (count 0, a
//! pointer to one zeroed attribute: `ifcs_stp_create(.., 0, &none)`), and a list of n.
#![forbid(unsafe_code)]

use core::mem::{offset_of, size_of};
use ifcs_sys as sys;

use crate::handle::Handle;

const VALUE: usize = size_of::<sys::ifcs_attr_value_t>();

/// What an attribute's value is (the union member the C++ assigns).
#[derive(Clone, Debug, PartialEq)]
enum Value {
    /// Nothing assigned (`attr_of(id)`): all zero.
    Zero,
    U64(u64),
    U32(u32),
    U16(u16),
    U8(u8),
    /// `value.data` (`ifcs_bool_t`, an int).
    Data(bool),
    Handle(Handle),
    Mac([u8; 6]),
    /// `value.fwd_policy.fwd_action`.
    Fwd(i32),
    /// `value.ctc_policy`: action and trap handle.
    Ctc(i32, Handle),
    /// `ifcs_flex_stats_attributes_t` copied into the value.
    Flex {
        mode: u32,
        start: u32,
        count: u32,
        dirty: u8,
    },
    I8List(Vec<i8>),
    U8List(Vec<u8>),
    I32List(Vec<i32>),
    U32List(Vec<u32>),
    /// `value.string`: chars, no NUL.
    Str(Vec<u8>),
    /// A struct's bytes copied into the value at offset 0 (`std::memcpy(&a.value, &s, sizeof s)`).
    Bytes(Vec<u8>),
    HandleList(Vec<Handle>),
    /// `value` = an `ifcs_qos_map_info_t`: its `type` and `dirty`, its list = these words (whole
    /// `ifcs_qos_map_t`s, `qos.rs`).
    QosMapInfo {
        kind: i32,
        dirty: u8,
        words: Vec<u32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Null,
    None,
    List,
}

/// An attribute list for one IFCS call: what to send, then what came back.
#[derive(Clone)]
pub struct Attrs {
    form: Form,
    items: Vec<(u32, Value)>,
    raw: Vec<sys::ifcs_attr_t>,
}

/// A zeroed attribute with `id` (C++ `attr_of`).
pub const fn zeroed(id: u32) -> sys::ifcs_attr_t {
    sys::ifcs_attr_t {
        id,
        value: sys::ifcs_attr_value_t { blob: [0; VALUE] },
        bitmap: 0,
    }
}

fn put(blob: &mut [u8; VALUE], off: usize, bytes: &[u8]) {
    if let Some(dst) = blob.get_mut(off..off + bytes.len()) {
        dst.copy_from_slice(bytes);
    }
}

fn put_list(blob: &mut [u8; VALUE], count: usize, count_off: usize, ptr_off: usize, ptr: usize) {
    put(blob, count_off, &u32::try_from(count).unwrap_or(u32::MAX).to_ne_bytes());
    put(blob, ptr_off, &ptr.to_ne_bytes());
}

fn get<const N: usize>(blob: &[u8; VALUE], off: usize) -> [u8; N] {
    let mut out = [0u8; N];
    if let Some(src) = blob.get(off..off + N) {
        out.copy_from_slice(src);
    }
    out
}

impl core::fmt::Debug for Attrs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Attrs")
            .field("form", &self.form)
            .field("items", &self.items)
            .field("raw_ids", &self.raw.iter().map(|a| a.id).collect::<Vec<_>>())
            .finish()
    }
}

impl Default for Attrs {
    fn default() -> Self {
        Self::new()
    }
}

impl Attrs {
    /// An empty list (count 0, pointer to an empty array: use [`Attrs::null`] or [`Attrs::none`]
    /// for the C++'s two empty forms; this one passes `NULL` too).
    pub const fn new() -> Self {
        Self {
            form: Form::List,
            items: Vec::new(),
            raw: Vec::new(),
        }
    }

    /// Count 0 and a `NULL` pointer.
    pub const fn null() -> Self {
        Self {
            form: Form::Null,
            items: Vec::new(),
            raw: Vec::new(),
        }
    }

    /// Count 0 and a pointer to one zeroed attribute with id 0 (the C++
    /// `ifcs_attr_t none = attr_of(0)` passed as `&none`).
    pub fn none() -> Self {
        Self {
            form: Form::None,
            items: vec![(0, Value::Zero)],
            raw: Vec::new(),
        }
    }

    /// One attribute as the vendor accessors built it (`ifcs_attr_t_init` / `_id_set` /
    /// `_value_data_set`): sent as it is.
    pub fn from_raw(attr: sys::ifcs_attr_t) -> Self {
        Self {
            form: Form::List,
            items: Vec::new(),
            raw: vec![attr],
        }
    }

    fn with(mut self, id: u32, v: Value) -> Self {
        self.form = Form::List;
        self.items.push((id, v));
        self
    }

    /// `attr_of(id)`: nothing assigned (a get's request).
    #[must_use]
    pub fn of(self, id: u32) -> Self {
        self.with(id, Value::Zero)
    }
    /// `value.u64`.
    #[must_use]
    pub fn u64(self, id: u32, v: u64) -> Self {
        self.with(id, Value::U64(v))
    }
    /// `value.u32` (`attr_u32`).
    #[must_use]
    pub fn u32(self, id: u32, v: u32) -> Self {
        self.with(id, Value::U32(v))
    }
    /// `value.u16` (`attr_u16`).
    #[must_use]
    pub fn u16(self, id: u32, v: u16) -> Self {
        self.with(id, Value::U16(v))
    }
    /// `value.u8`.
    #[must_use]
    pub fn u8(self, id: u32, v: u8) -> Self {
        self.with(id, Value::U8(v))
    }
    /// `value.data` = `IFCS_BOOL_TRUE` / `_FALSE` (`attr_data`).
    #[must_use]
    pub fn data(self, id: u32, v: bool) -> Self {
        self.with(id, Value::Data(v))
    }
    /// `value.handle` (`attr_handle`).
    #[must_use]
    pub fn handle(self, id: u32, h: Handle) -> Self {
        self.with(id, Value::Handle(h))
    }
    /// `value.devport`.
    #[must_use]
    pub fn devport(self, id: u32, dp: u32) -> Self {
        self.with(id, Value::U32(dp))
    }
    /// `value.mac` (`attr_mac`).
    #[must_use]
    pub fn mac(self, id: u32, mac: [u8; 6]) -> Self {
        self.with(id, Value::Mac(mac))
    }
    /// `value.fwd_policy.fwd_action` (`attr_fwd`).
    #[must_use]
    pub fn fwd(self, id: u32, action: i32) -> Self {
        self.with(id, Value::Fwd(action))
    }
    /// `value.ctc_policy` (`attr_ctc`): copy to the CPU on / off, the trap.
    #[must_use]
    pub fn ctc(self, id: u32, enable: bool, trap: Handle) -> Self {
        let action = if enable {
            sys::IFCS_COPY_TO_CPU_ENABLE
        } else {
            sys::IFCS_COPY_TO_CPU_DISABLE
        };
        self.with(id, Value::Ctc(action.cast_signed(), trap))
    }
    /// An `ifcs_flex_stats_attributes_t` copied into the value (`tl_l3.cpp` `pre_enable`).
    #[must_use]
    pub fn flex_stats(self, id: u32, counter_mode: u32, start_offset: u32, number_of_counters: u32, dirty: u8) -> Self {
        self.with(
            id,
            Value::Flex {
                mode: counter_mode,
                start: start_offset,
                count: number_of_counters,
                dirty,
            },
        )
    }
    /// `value.i8_list`.
    #[must_use]
    pub fn i8_list(self, id: u32, v: Vec<i8>) -> Self {
        self.with(id, Value::I8List(v))
    }
    /// `value.u8_list`.
    #[must_use]
    pub fn u8_list(self, id: u32, v: Vec<u8>) -> Self {
        self.with(id, Value::U8List(v))
    }
    /// `value.i32_list`.
    #[must_use]
    pub fn i32_list(self, id: u32, v: Vec<i32>) -> Self {
        self.with(id, Value::I32List(v))
    }
    /// `value.u32_list`.
    #[must_use]
    pub fn u32_list(self, id: u32, v: Vec<u32>) -> Self {
        self.with(id, Value::U32List(v))
    }
    /// `value.string` (count = bytes, no NUL).
    #[must_use]
    pub fn string(self, id: u32, s: &str) -> Self {
        self.with(id, Value::Str(s.as_bytes().to_vec()))
    }
    /// A struct copied into the value (its bytes at offset 0, the rest zero; at most the value's
    /// 120 bytes): the C++'s `std::memcpy(&a.value, &action, sizeof action)` of the ACL action
    /// values (`tl_acl.cpp`).
    #[must_use]
    pub fn bytes(self, id: u32, v: Vec<u8>) -> Self {
        self.with(id, Value::Bytes(v))
    }
    /// `value.handle_list`.
    #[must_use]
    pub fn handle_list(self, id: u32, v: Vec<Handle>) -> Self {
        self.with(id, Value::HandleList(v))
    }
    /// An `ifcs_qos_map_info_t` copied into the value (`tl_qos.cpp` `map_info`): `info` as
    /// `ifcs_qos_map_info_t_init` left it with its `type`, `list` = `entries` (owned by this list, so
    /// the pointer is valid for the call).
    #[must_use]
    pub fn qos_map_info(self, id: u32, info: &crate::qos::QosMapInfo, entries: &crate::qos::QosMapList) -> Self {
        self.with(
            id,
            Value::QosMapInfo {
                kind: info.kind(),
                dirty: info.dirty(),
                words: entries.words(),
            },
        )
    }

    /// The number of attributes sent.
    pub fn len(&self) -> usize {
        match self.form {
            Form::Null | Form::None => 0,
            Form::List if self.items.is_empty() => self.raw.len(),
            Form::List => self.items.len(),
        }
    }

    /// Whether none is sent.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The count and pointer for a call (the attributes encoded now, list members pointing at
    /// this list's buffers, valid while `self` is borrowed). After the call the attributes the
    /// callee wrote are read with [`Attrs::u32_at`] etc.
    pub(crate) fn args(&mut self) -> (u32, *mut sys::ifcs_attr_t) {
        if !self.items.is_empty() {
            let mut raw = Vec::with_capacity(self.items.len());
            for (id, v) in &mut self.items {
                let mut a = zeroed(*id);
                encode(&mut a.value.blob, v);
                raw.push(a);
            }
            self.raw = raw;
        }
        match self.form {
            Form::Null => (0, core::ptr::null_mut()),
            Form::None => (0, self.raw.as_mut_ptr()),
            Form::List if self.raw.is_empty() => (0, core::ptr::null_mut()),
            Form::List => (u32::try_from(self.raw.len()).unwrap_or(u32::MAX), self.raw.as_mut_ptr()),
        }
    }

    /// The attributes as sent / as the callee left them.
    pub fn raw(&self) -> &[sys::ifcs_attr_t] {
        &self.raw
    }

    fn blob(&self, i: usize) -> Option<&[u8; VALUE]> {
        self.raw.get(i).map(|a| &a.value.blob)
    }

    /// `value.u32` of attribute `i` after a call (0 if there is none).
    pub fn u32_at(&self, i: usize) -> u32 {
        self.blob(i).map_or(0, |b| u32::from_ne_bytes(get(b, 0)))
    }
    /// `value.u16` of attribute `i`.
    pub fn u16_at(&self, i: usize) -> u16 {
        self.blob(i).map_or(0, |b| u16::from_ne_bytes(get(b, 0)))
    }
    /// `value.u64` of attribute `i`.
    pub fn u64_at(&self, i: usize) -> u64 {
        self.blob(i).map_or(0, |b| u64::from_ne_bytes(get(b, 0)))
    }
    /// `value.handle` of attribute `i`.
    pub fn handle_at(&self, i: usize) -> Handle {
        Handle(self.u32_at(i))
    }
    /// `value.data` of attribute `i` (an `ifcs_bool_t`).
    pub fn data_at(&self, i: usize) -> i32 {
        self.blob(i).map_or(0, |b| i32::from_ne_bytes(get(b, 0)))
    }
    /// `value.flt` of attribute `i`.
    pub fn flt_at(&self, i: usize) -> f32 {
        self.blob(i).map_or(0.0, |b| f32::from_ne_bytes(get(b, 0)))
    }
}

/// Writes a value into a zeroed blob (member offsets from the bindings, checked by the compiler).
fn encode(blob: &mut [u8; VALUE], v: &mut Value) {
    match v {
        Value::Zero => {}
        Value::U64(x) => put(blob, 0, &x.to_ne_bytes()),
        Value::U32(x) => put(blob, 0, &x.to_ne_bytes()),
        Value::U16(x) => put(blob, 0, &x.to_ne_bytes()),
        Value::U8(x) => put(blob, 0, &[*x]),
        Value::Data(b) => {
            let d = if *b { sys::IFCS_BOOL_TRUE } else { sys::IFCS_BOOL_FALSE };
            put(blob, 0, &d.cast_signed().to_ne_bytes());
        }
        Value::Handle(h) => put(blob, 0, &h.0.to_ne_bytes()),
        Value::Mac(m) => put(blob, 0, m),
        Value::Bytes(b) => put(blob, 0, b.get(..VALUE).unwrap_or(b)),
        Value::Fwd(a) => put(blob, offset_of!(sys::ifcs_fwd_policy_t, fwd_action), &a.to_ne_bytes()),
        Value::Ctc(a, h) => {
            put(blob, offset_of!(sys::ifcs_ctc_policy_t, ctc_action), &a.to_ne_bytes());
            put(
                blob,
                offset_of!(sys::ifcs_ctc_policy_t, trap_handle),
                &h.0.to_ne_bytes(),
            );
        }
        Value::Flex {
            mode,
            start,
            count,
            dirty,
        } => {
            put(
                blob,
                offset_of!(sys::ifcs_flex_stats_attributes_t, counter_mode),
                &mode.to_ne_bytes(),
            );
            put(
                blob,
                offset_of!(sys::ifcs_flex_stats_attributes_t, start_offset),
                &start.to_ne_bytes(),
            );
            put(
                blob,
                offset_of!(sys::ifcs_flex_stats_attributes_t, number_of_counters),
                &count.to_ne_bytes(),
            );
            put(blob, offset_of!(sys::ifcs_flex_stats_attributes_t, dirty), &[*dirty]);
        }
        Value::I8List(l) => put_list(
            blob,
            l.len(),
            offset_of!(sys::ifcs_i8_list_t, count),
            offset_of!(sys::ifcs_i8_list_t, arr),
            list_ptr(l),
        ),
        Value::U8List(l) | Value::Str(l) => put_list(
            blob,
            l.len(),
            offset_of!(sys::ifcs_u8_list_t, count),
            offset_of!(sys::ifcs_u8_list_t, arr),
            list_ptr(l),
        ),
        Value::I32List(l) => put_list(
            blob,
            l.len(),
            offset_of!(sys::ifcs_i32_list_t, count),
            offset_of!(sys::ifcs_i32_list_t, arr),
            list_ptr(l),
        ),
        Value::U32List(l) => put_list(
            blob,
            l.len(),
            offset_of!(sys::ifcs_u32_list_t, count),
            offset_of!(sys::ifcs_u32_list_t, arr),
            list_ptr(l),
        ),
        Value::HandleList(l) => put_list(
            blob,
            l.len(),
            offset_of!(sys::ifcs_handle_list_t, count),
            offset_of!(sys::ifcs_handle_list_t, list),
            list_ptr(l),
        ),
        Value::QosMapInfo { kind, dirty, words } => {
            type I = sys::ifcs_qos_map_info_t;
            put(blob, offset_of!(I, type_), &kind.to_ne_bytes());
            put_list(
                blob,
                words.len() / crate::qos::QOS_MAP_WORDS,
                offset_of!(I, list) + offset_of!(sys::ifcs_qos_map_list_t, count),
                offset_of!(I, list) + offset_of!(sys::ifcs_qos_map_list_t, arr),
                list_ptr(words),
            );
            put(blob, offset_of!(I, dirty), &[*dirty]);
        }
    }
}

/// The address of a list's first element (exposed: the C callee reads through it during the
/// call); `NULL` for an empty list.
fn list_ptr<T>(l: &mut [T]) -> usize {
    if l.is_empty() {
        0
    } else {
        l.as_mut_ptr().expose_provenance()
    }
}

/// `Handle` is `#[repr(Rust)]` over a `u32`; a handle list is sent as the `u32`s.
const _: () = assert!(size_of::<Handle>() == size_of::<sys::ifcs_handle_t>());
/// An `ifcs_qos_map_info_t` fits the value.
const _: () = assert!(size_of::<sys::ifcs_qos_map_info_t>() <= VALUE);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_sit_where_the_union_members_are() {
        let mut a = Attrs::new()
            .u32(23, 7)
            .u16(51, 9122)
            .data(1, true)
            .handle(0, Handle(0x100))
            .mac(2, [1, 2, 3, 4, 5, 6])
            .flex_stats(46, 0, 0, 4096, 7)
            .ctc(2, true, Handle(0x55));
        let (n, p) = a.args();
        assert_eq!(n, 7);
        assert!(!p.is_null());
        assert_eq!(a.u32_at(0), 7);
        assert_eq!(a.raw()[0].id, 23);
        assert_eq!(a.u16_at(1), 9122);
        assert_eq!(a.data_at(2), 1);
        assert_eq!(a.handle_at(3), Handle(0x100));
        assert_eq!(a.raw()[4].value.blob[..6], [1, 2, 3, 4, 5, 6]);
        assert_eq!(a.u32_at(5), 0); // counter_mode
        assert_eq!(a.raw()[5].value.blob[8..12], 4096u32.to_ne_bytes());
        assert_eq!(a.raw()[5].value.blob[12], 7);
        assert_eq!(a.raw()[6].value.blob[4..8], 0x55u32.to_ne_bytes());
        assert!(a.raw().iter().all(|r| r.bitmap == 0));
    }

    #[test]
    fn bytes_are_copied_to_the_value_start() {
        let mut a = Attrs::new().bytes(2, vec![1, 2, 3]).bytes(3, vec![7; VALUE + 8]);
        a.args();
        assert_eq!(a.raw()[0].value.blob[..4], [1, 2, 3, 0]);
        assert!(a.raw()[0].value.blob[3..].iter().all(|b| *b == 0));
        assert!(a.raw()[1].value.blob.iter().all(|b| *b == 7)); // cut to the value's size
    }

    #[test]
    fn lists_point_at_their_buffers() {
        let mut a = Attrs::new()
            .i8_list(43, vec![2, -2])
            .u32_list(53, vec![5])
            .u8_list(50, Vec::new());
        a.args();
        let count = u32::from_ne_bytes(a.raw()[0].value.blob[0..4].try_into().unwrap());
        let ptr = usize::from_ne_bytes(a.raw()[0].value.blob[8..16].try_into().unwrap());
        assert_eq!(count, 2);
        assert_ne!(ptr, 0);
        assert_eq!(
            usize::from_ne_bytes(a.raw()[2].value.blob[8..16].try_into().unwrap()),
            0
        );
    }

    #[test]
    fn a_qos_map_info_points_at_its_entries() {
        let mut info = crate::qos::QosMapInfo::zeroed();
        info.set_type(sys::IFCS_QOS_MAP_TYPE_DSCP_TO_TC_DP);
        let mut l = crate::qos::QosMapList::new();
        l.push(crate::qos::QosMapSrc::Dscp(46), 5, 0);
        let mut a = Attrs::new().qos_map_info(0, &info, &l);
        a.args();
        let b = &a.raw()[0].value.blob;
        assert_eq!(b[0..4], 1i32.to_ne_bytes());
        let lo = offset_of!(sys::ifcs_qos_map_info_t, list);
        assert_eq!(b[lo..lo + 4], 1u32.to_ne_bytes());
        let p = usize::from_ne_bytes(b[lo + 8..lo + 16].try_into().unwrap());
        assert_ne!(p, 0);
        assert_eq!(p % 4, 0);
    }

    #[test]
    fn the_three_empty_forms() {
        let (n, p) = Attrs::null().args();
        assert_eq!((n, p.is_null()), (0, true));
        let mut none = Attrs::none();
        let (n, p) = none.args();
        assert_eq!((n, p.is_null()), (0, false));
        assert_eq!(none.raw().len(), 1);
        assert_eq!(none.raw()[0].id, 0);
        let mut raw = Attrs::from_raw(zeroed(5));
        assert_eq!(raw.args().0, 1);
        assert!(Attrs::new().is_empty() && Attrs::none().is_empty() && !raw.is_empty());
    }
}
