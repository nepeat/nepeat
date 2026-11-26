//! Zeroed C structs, the bytes of a value union, an FDB event's object id.
#![allow(unsafe_code)]

use crate::sys;

mod sealed {
    /// Only this module implements it (the list below).
    pub(crate) trait Sealed {}
}

/// The C structs this crate zeroes before it fills them (as the C programs `memset` theirs): only
/// the bindgen `repr(C)` types listed below, whose members are integers, `bool`, arrays, raw
/// pointers, `Option` of function pointers and unions or structs of those, so all-zero bytes are a
/// valid value of each (`false`, 0, null, `None`). Sealed: no other type can implement it.
pub(crate) trait Zeroable: Copy + sealed::Sealed {}

macro_rules! zeroable {
    ($($t:ty),*) => { $(impl sealed::Sealed for $t {} impl Zeroable for $t {})* };
}
// sai_attribute_t: id + the value union (integers, bools, arrays, pointers, unions of them);
// the QoS map entry and its params (integers); the entry keys (object ids, IP prefix / address
// unions of byte arrays, a MAC)
zeroable!(
    sys::sai_attribute_t,
    sys::sai_qos_map_t,
    sys::sai_qos_map_params_t,
    sys::sai_route_entry_t,
    sys::sai_neighbor_entry_t,
    sys::sai_fdb_entry_t
);

/// An all-zero `T` (padding included).
pub(crate) fn zeroed<T: Zeroable>() -> T {
    // SAFETY: `T` is one of the sealed `Zeroable` list above: all-zero bytes are a valid `T`.
    unsafe { core::mem::zeroed() }
}

const VALUE_SIZE: usize = core::mem::size_of::<sys::sai_attribute_value_t>();

/// The bytes of an attribute's value union. Only for attributes of this crate's `Encoded`: zeroed
/// in full before the call (`zeroed`), then written by safe code and by the library.
pub(crate) fn value_bytes(a: &sys::sai_attribute_t) -> [u8; VALUE_SIZE] {
    let p = (&raw const a.value).cast::<[u8; VALUE_SIZE]>();
    // SAFETY: `p` points to the whole union (same size, u8 arrays have alignment 1); every byte is
    // initialised: the attribute was zeroed before anything wrote a member (see above).
    unsafe { p.read() }
}

/// The object id member of an attribute the library built (an FDB event's `BRIDGE_PORT_ID`).
pub(crate) fn value_oid(a: &sys::sai_attribute_t) -> sys::sai_object_id_t {
    // SAFETY: the header gives `SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID` the `oid` member, which the
    // library wrote (callers check the id); a u64 read of 8 initialised bytes.
    unsafe { a.value.oid }
}
