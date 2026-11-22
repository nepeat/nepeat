//! `sai_attribute_value_t` (a C union) to and from [`Value`], by the attribute's
//! [`ValueType`] from the metadata: the one place that knows which union member an
//! attribute travels in.
use crate::Status;
use crate::ptr;
use core::ffi::c_char;
use sai_core::error::{AttrError, Error};
use sai_core::oid::RawOid;
use sai_core::value::{Pointer, Value};
use sai_meta::ValueType;
use sai_sys as sys;

/// Reads one member of the union.
macro_rules! member {
    ($value:ident . $field:ident) => {
        // SAFETY: the contract of `decode` / `encode`: the C caller initialised this member (the
        // one the metadata names for the attribute). Every member read through this macro is
        // plain data (integers, byte arrays, a count with a raw pointer) for which each bit
        // pattern is a value.
        unsafe { $value.$field }
    };
}

/// Copies a list member of an input value into a `Vec`.
macro_rules! list_in {
    ($value:ident . $field:ident) => {{
        let list = member!($value.$field);
        // SAFETY: the contract of `decode`: a list member's pointer names `count` initialised
        // elements unless the count is zero, which is `in_slice`'s contract.
        unsafe { ptr::in_slice(list.list.cast_const(), list.count) }
            .map(<[_]>::to_vec)
            .map_err(|_| Error::attr(AttrError::InvalidValue, 0))?
    }};
}

/// Writes a result list into the buffer a list member of an output value names.
macro_rules! list_out {
    ($slot:ident . $field:ident, $items:expr) => {{
        let mut list = member!($slot.$field);
        // SAFETY: the contract of `encode`: the caller set the member's count to the capacity of
        // the buffer its pointer names (or to zero), which is `write_list`'s contract.
        let outcome = unsafe { ptr::write_list(&mut list.count, list.list, $items) };
        $slot.$field = list;
        outcome
    }};
}

/// Decodes an attribute value the C caller passed in (create, set).
///
/// An error is for attribute index 0; the caller re-indexes it.
///
/// # Safety
/// The caller of the SAI function initialised the member of `value` that `value_type` names;
/// when that member is a list, its pointer names `count` initialised elements that are not
/// written to during the call, or its count is zero.
pub(crate) unsafe fn decode(value_type: ValueType, value: &sys::sai_attribute_value_t) -> Result<Value, Error> {
    Ok(match value_type {
        // Read as a byte: a C caller may store any non-zero value, which is not a Rust bool.
        ValueType::Bool => Value::Bool(member!(value.u8_) != 0),
        ValueType::CharData => {
            let raw = member!(value.chardata);
            let bytes: Vec<u8> = raw
                .iter()
                .map(|c| u8::from_ne_bytes(c.to_ne_bytes()))
                .take_while(|b| *b != 0)
                .collect();
            Value::CharData(String::from_utf8_lossy(&bytes).into_owned())
        }
        ValueType::U8 => Value::U8(member!(value.u8_)),
        ValueType::I8 => Value::I8(member!(value.s8)),
        ValueType::U16 => Value::U16(member!(value.u16_)),
        ValueType::I16 => Value::I16(member!(value.s16)),
        ValueType::U32 => Value::U32(member!(value.u32_)),
        ValueType::I32 => Value::I32(member!(value.s32)),
        ValueType::U64 => Value::U64(member!(value.u64_)),
        ValueType::I64 => Value::I64(member!(value.s64)),
        ValueType::Enum => Value::Enum(member!(value.s32)),
        ValueType::Mac => Value::Mac(member!(value.mac)),
        ValueType::Oid => Value::Oid(RawOid(member!(value.oid))),
        ValueType::OidList => Value::OidList(list_in!(value.objlist).into_iter().map(RawOid).collect()),
        ValueType::U8List => Value::U8List(list_in!(value.u8list)),
        ValueType::I8List => Value::I8List(list_in!(value.s8list)),
        ValueType::U16List => Value::U16List(list_in!(value.u16list)),
        ValueType::U32List => Value::U32List(list_in!(value.u32list)),
        ValueType::I32List => Value::I32List(list_in!(value.s32list)),
        ValueType::VlanList => Value::VlanList(list_in!(value.vlanlist)),
        ValueType::U32Range => {
            let range = member!(value.u32range);
            Value::U32Range {
                min: range.min,
                max: range.max,
            }
        }
        // The address is kept as a number; `notify` turns it back into a pointer.
        ValueType::Pointer => Value::Pointer(Pointer::from_addr(member!(value.ptr).expose_provenance())),
        ValueType::Other => return Err(Error::attr(AttrError::NotSupported, 0)),
    })
}

/// Encodes a result into the value slot of a get call. Scalars overwrite the slot; lists go
/// to the buffer the slot names, with SAI's buffer-overflow convention ([`ptr::write_list`]).
///
/// # Safety
/// When `result` is a list, the caller of the SAI function initialised the list member of
/// `slot` that carries it: the count is the capacity of the buffer the pointer names (valid
/// for that many writes, not accessed otherwise during the call), or zero.
pub(crate) unsafe fn encode(result: &Value, slot: &mut sys::sai_attribute_value_t) -> Result<(), Status> {
    match result {
        Value::Bool(v) => slot.booldata = *v,
        Value::CharData(text) => {
            let mut chars: [c_char; 32] = [0; 32];
            // At most 31 bytes: the last stays NUL.
            for (dst, src) in chars.iter_mut().zip(text.bytes().take(31)) {
                *dst = c_char::from_ne_bytes(src.to_ne_bytes());
            }
            slot.chardata = chars;
        }
        Value::U8(v) => slot.u8_ = *v,
        Value::I8(v) => slot.s8 = *v,
        Value::U16(v) => slot.u16_ = *v,
        Value::I16(v) => slot.s16 = *v,
        Value::U32(v) => slot.u32_ = *v,
        Value::I32(v) | Value::Enum(v) => slot.s32 = *v,
        Value::U64(v) => slot.u64_ = *v,
        Value::I64(v) => slot.s64 = *v,
        Value::Mac(v) => slot.mac = *v,
        Value::Oid(v) => slot.oid = v.0,
        Value::U32Range { min, max } => slot.u32range = sys::sai_u32_range_t { min: *min, max: *max },
        Value::Pointer(v) => slot.ptr = core::ptr::with_exposed_provenance_mut(v.addr()),
        Value::OidList(items) => {
            let raw: Vec<sys::sai_object_id_t> = items.iter().map(|oid| oid.0).collect();
            return list_out!(slot.objlist, &raw);
        }
        Value::U8List(items) => return list_out!(slot.u8list, items),
        Value::I8List(items) => return list_out!(slot.s8list, items),
        Value::U16List(items) => return list_out!(slot.u16list, items),
        Value::U32List(items) => return list_out!(slot.u32list, items),
        Value::I32List(items) => return list_out!(slot.s32list, items),
        Value::VlanList(items) => return list_out!(slot.vlanlist, items),
    }
    Ok(())
}

#[cfg(test)]
// The unsafe blocks of the tests pass null, zero counts or pointers to live locals of the
// stated length; one comment here instead of one per assertion.
#[allow(clippy::undocumented_unsafe_blocks, clippy::cast_ptr_alignment)]
mod tests {
    use super::*;

    fn zeroed() -> sys::sai_attribute_value_t {
        sys::sai_attribute_value_t { u64_: 0 }
    }

    #[test]
    fn scalars_round_trip_through_the_union() {
        let values = [
            Value::Bool(true),
            Value::U8(200),
            Value::I8(-3),
            Value::U16(4094),
            Value::I16(-2),
            Value::U32(100_000),
            Value::I32(-7),
            Value::U64(u64::MAX - 1),
            Value::I64(i64::MIN),
            Value::Enum(3),
            Value::Mac([1, 2, 3, 4, 5, 6]),
            Value::Oid(RawOid(0x2100_0000_0000_0001)),
            Value::U32Range { min: 5, max: 9 },
            Value::CharData("Ethernet0".into()),
        ];
        for value in values {
            let mut slot = zeroed();
            // SAFETY: not a list.
            unsafe { encode(&value, &mut slot) }.unwrap();
            // SAFETY: the member for this value type was written by encode just above.
            assert_eq!(unsafe { decode(value.value_type(), &slot) }, Ok(value));
        }
    }

    #[test]
    fn bool_is_read_as_a_byte_and_chardata_is_cut() {
        let slot = sys::sai_attribute_value_t { u8_: 2 };
        // SAFETY: u8_ (the byte booldata occupies) is initialised.
        assert_eq!(unsafe { decode(ValueType::Bool, &slot) }, Ok(Value::Bool(true)));
        let mut slot = zeroed();
        let long = "x".repeat(40);
        // SAFETY: not a list.
        unsafe { encode(&Value::CharData(long), &mut slot) }.unwrap();
        // SAFETY: chardata was written by encode.
        assert_eq!(
            unsafe { decode(ValueType::CharData, &slot) },
            Ok(Value::CharData("x".repeat(31)))
        );
        // SAFETY: nothing is read for a type that is not decoded.
        assert_eq!(
            unsafe { decode(ValueType::Other, &slot) },
            Err(Error::attr(AttrError::NotSupported, 0))
        );
    }

    #[test]
    fn lists_in_and_out() {
        let mut ids = [0x10u64, 0x20, 0x30];
        let slot = sys::sai_attribute_value_t {
            objlist: sys::sai_object_list_t {
                count: 3,
                list: ids.as_mut_ptr(),
            },
        };
        // SAFETY: objlist names the 3 elements of `ids`.
        assert_eq!(
            unsafe { decode(ValueType::OidList, &slot) },
            Ok(Value::OidList(vec![RawOid(0x10), RawOid(0x20), RawOid(0x30)]))
        );
        // zero count: the pointer is not looked at
        let slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 0,
                list: core::ptr::null_mut(),
            },
        };
        // SAFETY: a zero count.
        assert_eq!(unsafe { decode(ValueType::U32List, &slot) }, Ok(Value::U32List(vec![])));
        // a count with a null pointer is the attribute's fault
        let slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 2,
                list: core::ptr::null_mut(),
            },
        };
        // SAFETY: null is refused before anything is read.
        assert_eq!(
            unsafe { decode(ValueType::U32List, &slot) },
            Err(Error::attr(AttrError::InvalidValue, 0))
        );

        let result = Value::U32List(vec![1, 2, 3, 4]);
        let mut buf = [0u32; 8];
        let mut slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 8,
                list: buf.as_mut_ptr(),
            },
        };
        // SAFETY (the three encodes below): the list member names a live buffer of `count` elements, or count is 0.
        assert_eq!(unsafe { encode(&result, &mut slot) }, Ok(()));
        // SAFETY: u32list was initialised above and updated by encode.
        assert_eq!(unsafe { slot.u32list.count }, 4);
        assert_eq!(buf, [1, 2, 3, 4, 0, 0, 0, 0]);

        let mut small = [9u32; 2];
        let mut slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 2,
                list: small.as_mut_ptr(),
            },
        };
        assert_eq!(
            unsafe { encode(&result, &mut slot) },
            Err(sys::SAI_STATUS_BUFFER_OVERFLOW)
        );
        // SAFETY: as above.
        assert_eq!(unsafe { slot.u32list.count }, 4);
        assert_eq!(small, [9, 9]);

        let mut slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 0,
                list: core::ptr::null_mut(),
            },
        };
        assert_eq!(
            unsafe { encode(&result, &mut slot) },
            Err(sys::SAI_STATUS_BUFFER_OVERFLOW)
        );
        // SAFETY: as above.
        assert_eq!(unsafe { slot.u32list.count }, 4);
    }
}
