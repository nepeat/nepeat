//! `sai_attribute_t` to and from the model's typed values.
//!
//! Which union member an attribute travels in is the header's (sai-meta's tables); the members
//! sai-meta does not decode are taken by attribute id: SAK, salt and hash key of an SA, the
//! ACL field / action data of an ACL entry. An attribute no table knows is handed on as a
//! placeholder the model never looks at (it answers "not supported" for it at its index, as
//! the C++ does).
#![allow(
    unsafe_code,
    reason = "reading union members of the caller's sai_attribute_t: see the crate documentation"
)]

use crate::ptr::{fill_list, in_slice};
use sai_core::Error;
use sai_core::error::AttrError;
use sai_core::oid::RawOid;
use sai_macsec::{AclField, AclParameter, Attr, Secret, Value};
use sai_meta::ValueType;
use sai_sys as sys;

fn invalid(index: usize) -> Error {
    Error::attr(AttrError::InvalidValue, index)
}

const ENTRY_FIELDS: core::ops::RangeInclusive<sys::sai_attr_id_t> =
    sys::SAI_ACL_ENTRY_ATTR_FIELD_START..=sys::SAI_ACL_ENTRY_ATTR_FIELD_END;
const ENTRY_ACTIONS: core::ops::RangeInclusive<sys::sai_attr_id_t> =
    sys::SAI_ACL_ENTRY_ATTR_ACTION_START..=sys::SAI_ACL_ENTRY_ATTR_ACTION_END;
const TABLE_FIELDS: core::ops::RangeInclusive<sys::sai_attr_id_t> =
    sys::SAI_ACL_TABLE_ATTR_FIELD_START..=sys::SAI_ACL_TABLE_ATTR_FIELD_END;

/// A `bool` member read as the byte it is: C may have put any value there, and a Rust `bool`
/// that is not 0 or 1 is undefined behaviour. `booldata` and `u8` share offset 0.
fn bool_of(v: &sys::sai_attribute_value_t) -> bool {
    // SAFETY: every byte value is a valid u8; the caller initialised the member (booldata) that
    // starts at the same offset (the header's contract).
    unsafe { v.u8_ != 0 }
}

/// `attrs[index]` of a call on an object of type `object_type`, as the model takes it.
pub(crate) fn decode(
    object_type: sys::sai_object_type_t,
    a: &sys::sai_attribute_t,
    index: usize,
) -> Result<Attr, Error> {
    let v = &a.value;
    let value = match (object_type, a.id) {
        (sys::SAI_OBJECT_TYPE_MACSEC_SA, sys::SAI_MACSEC_SA_ATTR_SAK) => {
            // SAFETY: SAK travels in macsecsak (the header's @type sai_macsec_sak_t); a byte array has no invalid values.
            let bytes = unsafe { &v.macsecsak };
            Value::Sak(Secret::from_slice(bytes).ok_or(invalid(index))?)
        }
        (sys::SAI_OBJECT_TYPE_MACSEC_SA, sys::SAI_MACSEC_SA_ATTR_SALT) => {
            // SAFETY: SALT travels in macsecsalt; a byte array has no invalid values.
            let bytes = unsafe { &v.macsecsalt };
            Value::Salt(Secret::from_slice(bytes).ok_or(invalid(index))?)
        }
        (sys::SAI_OBJECT_TYPE_MACSEC_SA, sys::SAI_MACSEC_SA_ATTR_AUTH_KEY) => {
            // SAFETY: AUTH_KEY travels in macsecauthkey; a byte array has no invalid values.
            let bytes = unsafe { &v.macsecauthkey };
            Value::AuthKey(Secret::from_slice(bytes).ok_or(invalid(index))?)
        }
        (sys::SAI_OBJECT_TYPE_ACL_ENTRY, id) if ENTRY_FIELDS.contains(&id) => Value::AclField(field(v, id)),
        (sys::SAI_OBJECT_TYPE_ACL_ENTRY, id) if ENTRY_ACTIONS.contains(&id) => action(v, id),
        // the C++ reads every field of a MACsec-stage table as booldata, whatever its type
        (sys::SAI_OBJECT_TYPE_ACL_TABLE, id) if TABLE_FIELDS.contains(&id) => Value::Bool(bool_of(v)),
        (ot, id) => match sai_meta::attribute(ot, id).map(|m| m.value_type) {
            Some(ValueType::Bool) => Value::Bool(bool_of(v)),
            // SAFETY: the attribute's member is u8 (sai-meta, from the header's @type).
            Some(ValueType::U8) => Value::U8(unsafe { v.u8_ }),
            // SAFETY: the attribute's member is u32.
            Some(ValueType::U32) => Value::U32(unsafe { v.u32_ }),
            // SAFETY: the attribute's member is u64.
            Some(ValueType::U64) => Value::U64(unsafe { v.u64_ }),
            // SAFETY: the attribute's member is s32 (an enumerator or a number).
            Some(ValueType::Enum | ValueType::I32) => Value::Enum(unsafe { v.s32 }),
            // SAFETY: the attribute's member is oid.
            Some(ValueType::Oid) => Value::Oid(RawOid(unsafe { v.oid })),
            Some(ValueType::OidList) => {
                // SAFETY: the attribute's member is objlist (a count and a pointer: no invalid values).
                let l = unsafe { v.objlist };
                // SAFETY: count elements at list (the caller's contract for an input list).
                let items = unsafe { in_slice(l.list.cast_const(), usize::try_from(l.count).unwrap_or(usize::MAX)) };
                Value::OidList(items.ok_or(invalid(index))?.iter().map(|o| RawOid(*o)).collect())
            }
            Some(ValueType::I32List) => {
                // SAFETY: the attribute's member is s32list (a count and a pointer).
                let l = unsafe { v.s32list };
                // SAFETY: count elements at list (the caller's contract for an input list).
                let items = unsafe { in_slice(l.list.cast_const(), usize::try_from(l.count).unwrap_or(usize::MAX)) };
                Value::I32List(items.ok_or(invalid(index))?.to_vec())
            }
            // not an attribute the model handles: it refuses it by its id
            _ => Value::U32(0),
        },
    };
    Ok(Attr::new(a.id, value))
}

/// A pointer to the member at `offset` of the struct at `base` (no access, no unsafe).
fn member<T, U>(base: *const T, offset: usize) -> *const U {
    base.cast::<u8>().wrapping_add(offset).cast()
}

/// The data of an ACL entry field: `None` when it is disabled (or not one a MACsec table has).
fn field(v: &sys::sai_attribute_value_t, id: sys::sai_attr_id_t) -> Option<AclField> {
    // an ACL entry field travels in aclfield; its parts are read through pointers, so that
    // the C bool "enable" is read as the byte it is (bool_of's reason)
    let f = &raw const v.aclfield;
    let enable = member::<_, u8>(f, core::mem::offset_of!(sys::sai_acl_field_data_t, enable));
    // SAFETY: points into *v (live for this call, initialised: the header's contract), aligned
    // (a u8); any byte value is a valid u8.
    if unsafe { enable.read() } == 0 {
        return None;
    }
    let data = member::<_, sys::sai_acl_field_data_data_t>(f, core::mem::offset_of!(sys::sai_acl_field_data_t, data));
    let mask = member::<_, sys::sai_acl_field_data_mask_t>(f, core::mem::offset_of!(sys::sai_acl_field_data_t, mask));
    // SAFETY: as above, aligned as the struct's member; a union value has no invalid bit patterns.
    let data = unsafe { data.read() };
    // SAFETY: as above.
    let mask = unsafe { mask.read() };
    // the members by the field's type (its @type): mac (DST_MAC), u16 (ETHER_TYPE), u64
    // (MACSEC_SCI); none has invalid values
    match id {
        sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_MAC => Some(AclField::Mac {
            // SAFETY: DST_MAC's data is mac.
            data: unsafe { data.mac },
            // SAFETY: DST_MAC's mask is mac.
            mask: unsafe { mask.mac },
        }),
        sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE => Some(AclField::U16 {
            // SAFETY: ETHER_TYPE's data is u16.
            data: unsafe { data.u16_ },
            // SAFETY: ETHER_TYPE's mask is u16.
            mask: unsafe { mask.u16_ },
        }),
        sys::SAI_ACL_ENTRY_ATTR_FIELD_MACSEC_SCI => Some(AclField::U64 {
            // SAFETY: MACSEC_SCI's data is u64.
            data: unsafe { data.u64_ },
            // SAFETY: MACSEC_SCI's mask is u64.
            mask: unsafe { mask.u64_ },
        }),
        _ => None,
    }
}

/// An ACL entry action: the flow by oid, everything else by s32 (the packet action).
fn action(v: &sys::sai_attribute_value_t, id: sys::sai_attr_id_t) -> Value {
    let a = &raw const v.aclaction;
    let enable = member::<_, u8>(a, core::mem::offset_of!(sys::sai_acl_action_data_t, enable));
    let parameter =
        member::<_, sys::sai_acl_action_parameter_t>(a, core::mem::offset_of!(sys::sai_acl_action_data_t, parameter));
    // SAFETY: an ACL entry action travels in aclaction; the pointer is into *v, aligned (a u8),
    // and enable is read as the byte it is (bool_of's reason).
    let enable = unsafe { enable.read() } != 0;
    // SAFETY: as above, aligned as the struct's member; a union value has no invalid bit patterns.
    let parameter = unsafe { parameter.read() };
    let parameter = if id == sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW {
        // SAFETY: ACTION_MACSEC_FLOW's parameter is oid (no invalid values).
        AclParameter::Oid(RawOid(unsafe { parameter.oid }))
    } else {
        // SAFETY: ACTION_PACKET_ACTION's parameter is s32 (no invalid values); the model refuses
        // the other actions by their id.
        AclParameter::S32(unsafe { parameter.s32 })
    };
    Value::AclAction { enable, parameter }
}

/// Writes a value the model answered into the caller's attribute; the error is a SAI status
/// (`BUFFER_OVERFLOW` with the needed count written, as the C++ `fill_list`).
pub(crate) fn encode(value: &Value, a: &mut sys::sai_attribute_t) -> Result<(), i32> {
    let v = &mut a.value;
    match value {
        Value::Bool(b) => v.booldata = *b,
        Value::U8(x) => v.u8_ = *x,
        Value::U32(x) => v.u32_ = *x,
        Value::U64(x) => v.u64_ = *x,
        Value::Enum(x) => v.s32 = *x,
        Value::Oid(o) => v.oid = o.0,
        Value::OidList(list) => {
            // SAFETY: the attribute's member is objlist: the caller's capacity and buffer.
            let mut l = unsafe { v.objlist };
            let items: Vec<sys::sai_object_id_t> = list.iter().map(|o| o.0).collect();
            // SAFETY: list has room for count elements (the caller's contract for an output list).
            let filled = unsafe { fill_list(&mut l.count, l.list, &items) };
            v.objlist = l;
            filled?;
        }
        Value::AclField(None) => {
            v.aclfield = sys::sai_acl_field_data_t {
                enable: false,
                mask: sys::sai_acl_field_data_mask_t { u64_: 0 },
                data: sys::sai_acl_field_data_data_t { u64_: 0 },
            };
        }
        Value::AclField(Some(f)) => {
            let (mask, data) = match *f {
                AclField::Mac { data, mask } => (
                    sys::sai_acl_field_data_mask_t { mac: mask },
                    sys::sai_acl_field_data_data_t { mac: data },
                ),
                AclField::U16 { data, mask } => (
                    sys::sai_acl_field_data_mask_t { u16_: mask },
                    sys::sai_acl_field_data_data_t { u16_: data },
                ),
                AclField::U64 { data, mask } => (
                    sys::sai_acl_field_data_mask_t { u64_: mask },
                    sys::sai_acl_field_data_data_t { u64_: data },
                ),
            };
            v.aclfield = sys::sai_acl_field_data_t {
                enable: true,
                mask,
                data,
            };
        }
        Value::AclAction { enable, parameter } => {
            let parameter = match *parameter {
                AclParameter::S32(x) => sys::sai_acl_action_parameter_t { s32: x },
                AclParameter::Oid(o) => sys::sai_acl_action_parameter_t { oid: o.0 },
            };
            v.aclaction = sys::sai_acl_action_data_t {
                enable: *enable,
                parameter,
            };
        }
        // key material is write-only, and no get answers a list of numbers
        Value::Sak(_) | Value::Salt(_) | Value::AuthKey(_) | Value::I32List(_) => return Err(sys::SAI_STATUS_FAILURE),
    }
    Ok(())
}
