//! Checking the attributes of a call against the SAI metadata before any object code runs:
//! unknown ids, values of the wrong kind, attributes a call may not carry (read-only on
//! create, create-only on set), duplicates, missing mandatory ones, object ids that are
//! null or of a type the attribute does not accept.
use crate::error::{AttrError, Error, Result};
use crate::model::Kept;
use crate::oid::{RawOid, TypedId};
use crate::value::{Attr, FromValue, Pointer, Value};
use sai_meta::{AttrMeta, DefaultValue, Flags, ValueType};
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr};

/// The metadata of `id`, or the "unknown attribute" error for position `index`.
pub fn meta(object_type: sys::sai_object_type_t, id: sys::sai_attr_id_t, index: usize) -> Result<&'static AttrMeta> {
    sai_meta::attribute(object_type, id).ok_or(Error::attr(AttrError::Unknown, index))
}

/// What a get of the attribute answers while nobody set it: the header's `@default` as a
/// value of the attribute's kind. `None` when the header gives none, or one that depends on
/// the object ("internal", "vendor", another attribute's value).
pub fn default_value(meta: &AttrMeta) -> Option<Value> {
    Some(match (meta.default, meta.value_type) {
        (DefaultValue::Bool(b), ValueType::Bool) => Value::Bool(b),
        (DefaultValue::Int(n), ValueType::U8) => Value::U8(u8::try_from(n).ok()?),
        (DefaultValue::Int(n), ValueType::I8) => Value::I8(i8::try_from(n).ok()?),
        (DefaultValue::Int(n), ValueType::U16) => Value::U16(u16::try_from(n).ok()?),
        (DefaultValue::Int(n), ValueType::I16) => Value::I16(i16::try_from(n).ok()?),
        (DefaultValue::Int(n), ValueType::U32) => Value::U32(u32::try_from(n).ok()?),
        (DefaultValue::Int(n), ValueType::I32) => Value::I32(i32::try_from(n).ok()?),
        (DefaultValue::Int(n), ValueType::U64) => Value::U64(u64::try_from(n).ok()?),
        (DefaultValue::Int(n), ValueType::I64) => Value::I64(n),
        (DefaultValue::Int(n), ValueType::Enum) => Value::Enum(i32::try_from(n).ok()?),
        (DefaultValue::NullOid, ValueType::Oid) => Value::Oid(RawOid::NULL),
        (DefaultValue::NullPointer, ValueType::Pointer) => Value::Pointer(Pointer::NULL),
        (DefaultValue::EmptyText, ValueType::CharData) => Value::CharData(String::new()),
        (DefaultValue::ZeroMac, ValueType::Mac) => Value::Mac([0; 6]),
        (DefaultValue::ZeroIp, ValueType::Ip4) => Value::Ip4(Ipv4Addr::UNSPECIFIED),
        (DefaultValue::ZeroIp, ValueType::IpAddress) => Value::IpAddress(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        (DefaultValue::Empty, ValueType::OidList) => Value::OidList(Vec::new()),
        (DefaultValue::Empty, ValueType::U8List) => Value::U8List(Vec::new()),
        (DefaultValue::Empty, ValueType::I8List) => Value::I8List(Vec::new()),
        (DefaultValue::Empty, ValueType::U16List) => Value::U16List(Vec::new()),
        (DefaultValue::Empty, ValueType::I16List) => Value::I16List(Vec::new()),
        (DefaultValue::Empty, ValueType::U32List) => Value::U32List(Vec::new()),
        (DefaultValue::Empty, ValueType::I32List) => Value::I32List(Vec::new()),
        (DefaultValue::Empty, ValueType::VlanList) => Value::VlanList(Vec::new()),
        (DefaultValue::Empty, ValueType::QosMapList) => Value::QosMapList(Vec::new()),
        (DefaultValue::Empty, ValueType::MapList) => Value::MapList(Vec::new()),
        _ => return None,
    })
}

/// Object-id attributes whose header has no `@allownull` but whose `@default` names a switch
/// object, and which the C++ answers for a null id with that default (`sai_hostif.cpp:344` /
/// `:396`: a trap with a null `TRAP_GROUP` is in the default trap group).
fn null_is_default(meta: &AttrMeta) -> bool {
    matches!(
        (meta.object_type, meta.id),
        (sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP)
    )
}

/// Checks one attribute's value against its metadata.
fn check_value(meta: &AttrMeta, value: &Value, index: usize) -> Result<()> {
    let bad = Error::attr(AttrError::InvalidValue, index);
    if value.value_type() != meta.value_type {
        return Err(bad);
    }
    let check_oid = |oid: RawOid| {
        if oid.is_null() {
            return if meta.allow_null || null_is_default(meta) {
                Ok(())
            } else {
                Err(bad)
            };
        }
        if meta.allowed_objects.contains(&oid.sai_object_type()) {
            Ok(())
        } else {
            Err(bad)
        }
    };
    // An enumerator the headers do not have is not a value of the attribute.
    let check_enum = |v: &i32| {
        if meta.enum_values.is_empty() || meta.enum_values.contains(v) {
            Ok(())
        } else {
            Err(bad)
        }
    };
    match value {
        Value::Oid(oid) => check_oid(*oid),
        Value::OidList(list) => list.iter().try_for_each(|oid| check_oid(*oid)),
        Value::Enum(v) => check_enum(v),
        Value::I32List(list) => list.iter().try_for_each(check_enum),
        _ => Ok(()),
    }
}

/// Checks the attribute of a set call and returns its metadata.
pub fn check_set(object_type: sys::sai_object_type_t, attr: &Attr) -> Result<&'static AttrMeta> {
    let meta = meta(object_type, attr.id, 0)?;
    if !meta.flags.settable() {
        return Err(Error::attr(AttrError::Invalid, 0));
    }
    check_value(meta, &attr.value, 0)?;
    Ok(meta)
}

/// The checked attributes of a create call. Object code takes the attributes it implements;
/// [`CreateArgs::finish`] then answers for the rest, so an attribute nobody looked at is
/// refused instead of being silently dropped.
#[derive(Debug)]
pub struct CreateArgs<'a> {
    attrs: &'a [Attr],
    taken: Vec<bool>,
}

impl<'a> CreateArgs<'a> {
    /// Checks a create call's attribute list.
    pub fn check(object_type: sys::sai_object_type_t, attrs: &'a [Attr]) -> Result<Self> {
        for (index, attr) in attrs.iter().enumerate() {
            // not an attribute of the type, or read-only: the C++ answer (as the rim's)
            let Some(meta) = sai_meta::attribute(object_type, attr.id).filter(|m| m.flags.creatable()) else {
                return Err(crate::dispatch::refused_create(object_type, attr.id, index));
            };
            check_value(meta, &attr.value, index)?;
            if attrs.iter().take(index).any(|earlier| earlier.id == attr.id) {
                return Err(Error::InvalidParameter);
            }
        }
        // Conditional attributes are mandatory only under their condition, which the tables do
        // not carry: those are left to the object code.
        let missing = sai_meta::attributes(object_type)
            .iter()
            .filter(|m| m.flags.contains(Flags::MANDATORY_ON_CREATE) && !m.conditional)
            .any(|m| !attrs.iter().any(|a| a.id == m.id));
        if missing {
            return Err(Error::MandatoryAttributeMissing);
        }
        Ok(Self {
            attrs,
            taken: vec![false; attrs.len()],
        })
    }

    /// The attributes of the call.
    pub fn attrs(&self) -> &'a [Attr] {
        self.attrs
    }

    /// The position of attribute `id` in the call (0 when absent), for error codes.
    pub fn index_of(&self, id: sys::sai_attr_id_t) -> usize {
        self.attrs.iter().position(|a| a.id == id).unwrap_or(0)
    }

    /// Takes attribute `id` as a `T`, if the call carries it.
    pub fn take<T: FromValue>(&mut self, id: sys::sai_attr_id_t) -> Result<Option<T>> {
        let Some(index) = self.attrs.iter().position(|a| a.id == id) else {
            return Ok(None);
        };
        let value = self.attrs.get(index).and_then(|a| T::from_value(&a.value));
        if let Some(slot) = self.taken.get_mut(index) {
            *slot = true;
        }
        // The value's kind was checked against the metadata; a mismatch here is this
        // library asking for the wrong Rust type.
        value.map(Some).ok_or(Error::Failure)
    }

    /// Takes a mandatory attribute.
    pub fn require<T: FromValue>(&mut self, id: sys::sai_attr_id_t) -> Result<T> {
        self.take(id)?.ok_or(Error::MandatoryAttributeMissing)
    }

    /// Takes an object id attribute as a typed id.
    pub fn take_id<I: TypedId>(&mut self, id: sys::sai_attr_id_t) -> Result<Option<I>> {
        let index = self.index_of(id);
        match self.take::<RawOid>(id)? {
            None => Ok(None),
            Some(raw) => I::try_from(raw)
                .map(Some)
                .map_err(|_| Error::attr(AttrError::InvalidValue, index)),
        }
    }

    /// Ends the call's argument handling: attributes nobody took are kept as they are when
    /// their id is in `keep` (or, with `keep_pointers`, when they are callbacks) and are
    /// otherwise answered "not supported" with their index.
    pub fn finish(self, keep: &[sys::sai_attr_id_t], keep_pointers: bool) -> Result<Kept> {
        let mut kept = Kept::new();
        for (index, (attr, taken)) in self.attrs.iter().zip(&self.taken).enumerate() {
            if *taken {
                continue;
            }
            if keep.contains(&attr.id) || (keep_pointers && matches!(attr.value, Value::Pointer(_))) {
                kept.insert(attr.id, attr.value.clone());
            } else {
                return Err(Error::attr(AttrError::NotSupported, index));
            }
        }
        Ok(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oid::{PortId, VlanId};

    #[test]
    fn create_checks() {
        let vlan = |attrs: &[Attr]| CreateArgs::check(sys::SAI_OBJECT_TYPE_VLAN, attrs).map(|_| ());
        assert_eq!(vlan(&[]), Err(Error::MandatoryAttributeMissing));
        assert_eq!(vlan(&[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5))]), Ok(()));
        // wrong kind of value for the attribute
        assert_eq!(
            vlan(&[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U32(5))]),
            Err(Error::attr(AttrError::InvalidValue, 0))
        );
        // read-only on create: the C++ create_vlan's answer (stub_set, ATTR_NOT_SUPPORTED_1)
        assert_eq!(
            vlan(&[
                Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5)),
                Attr::new(sys::SAI_VLAN_ATTR_MEMBER_LIST, Value::OidList(vec![]))
            ]),
            Err(Error::attr(AttrError::NotSupported, 1))
        );
        // not an attribute at all
        assert_eq!(
            vlan(&[
                Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5)),
                Attr::new(0x0fff_ffff, Value::Bool(true))
            ]),
            Err(Error::attr(AttrError::NotSupported, 1))
        );
        // twice
        let id = Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5));
        assert_eq!(vlan(&[id.clone(), id]), Err(Error::InvalidParameter));
    }

    #[test]
    fn enumerators_and_defaults_come_from_the_headers() {
        let set = |id, value| check_set(sys::SAI_OBJECT_TYPE_PORT, &Attr::new(id, value)).map(|_| ());
        let rs = sys::SAI_PORT_FEC_MODE_RS.cast_signed();
        assert_eq!(set(sys::SAI_PORT_ATTR_FEC_MODE, Value::Enum(rs)), Ok(()));
        assert_eq!(
            set(sys::SAI_PORT_ATTR_FEC_MODE, Value::Enum(77)),
            Err(Error::attr(AttrError::InvalidValue, 0))
        );
        let default = |t, id| default_value(sai_meta::attribute(t, id).unwrap());
        let port = sys::SAI_OBJECT_TYPE_PORT;
        assert_eq!(default(port, sys::SAI_PORT_ATTR_MTU), Some(Value::U32(1514)));
        assert_eq!(default(port, sys::SAI_PORT_ATTR_ADMIN_STATE), Some(Value::Bool(false)));
        assert_eq!(default(port, sys::SAI_PORT_ATTR_PORT_VLAN_ID), Some(Value::U16(1)));
        assert_eq!(
            default(port, sys::SAI_PORT_ATTR_FEC_MODE),
            Some(Value::Enum(sys::SAI_PORT_FEC_MODE_NONE.cast_signed()))
        );
        assert_eq!(
            default(port, sys::SAI_PORT_ATTR_INGRESS_ACL),
            Some(Value::Oid(RawOid::NULL))
        );
        assert_eq!(default(port, sys::SAI_PORT_ATTR_HW_LANE_LIST), None);
        // a default that depends on another object is nobody's constant
        assert_eq!(
            default(
                sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER,
                sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS
            ),
            None
        );
        // every default the tables carry is a value of its attribute's kind
        for (t, _) in sai_meta::OBJECT_TYPES {
            for meta in sai_meta::attributes(t) {
                if let Some(value) = default_value(meta) {
                    assert_eq!(value.value_type(), meta.value_type, "{}", meta.name);
                }
            }
        }
    }

    #[test]
    fn object_ids_must_be_of_an_accepted_type() {
        let member = |vlan: RawOid, bp: RawOid| {
            CreateArgs::check(
                sys::SAI_OBJECT_TYPE_VLAN_MEMBER,
                &[
                    Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(vlan)),
                    Attr::new(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(bp)),
                ],
            )
            .map(|_| ())
        };
        let vlan = VlanId::from_index(1).raw();
        let bp = crate::oid::BridgePortId::from_index(1).raw();
        assert_eq!(member(vlan, bp), Ok(()));
        assert_eq!(
            member(PortId::from_index(1).raw(), bp),
            Err(Error::attr(AttrError::InvalidValue, 0))
        );
        assert_eq!(member(vlan, vlan), Err(Error::attr(AttrError::InvalidValue, 1)));
        assert_eq!(member(vlan, RawOid::NULL), Err(Error::attr(AttrError::InvalidValue, 1)));
    }

    #[test]
    fn set_checks_and_unclaimed_attributes() {
        let set = |id, value| check_set(sys::SAI_OBJECT_TYPE_PORT, &Attr::new(id, value)).map(|_| ());
        assert_eq!(set(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)), Ok(()));
        assert_eq!(
            set(sys::SAI_PORT_ATTR_HW_LANE_LIST, Value::U32List(vec![1])),
            Err(Error::attr(AttrError::Invalid, 0))
        );
        assert_eq!(
            set(sys::SAI_PORT_ATTR_OPER_STATUS, Value::Enum(1)),
            Err(Error::attr(AttrError::Invalid, 0))
        );

        let attrs = [
            Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5)),
            Attr::new(sys::SAI_VLAN_ATTR_LEARN_DISABLE, Value::Bool(true)),
        ];
        let mut args = CreateArgs::check(sys::SAI_OBJECT_TYPE_VLAN, &attrs).unwrap();
        assert_eq!(args.require::<u16>(sys::SAI_VLAN_ATTR_VLAN_ID), Ok(5));
        assert_eq!(
            args.finish(&[], false).unwrap_err(),
            Error::attr(AttrError::NotSupported, 1)
        );
        let mut args = CreateArgs::check(sys::SAI_OBJECT_TYPE_VLAN, &attrs).unwrap();
        args.require::<u16>(sys::SAI_VLAN_ATTR_VLAN_ID).unwrap();
        assert_eq!(
            args.finish(&[sys::SAI_VLAN_ATTR_LEARN_DISABLE], false).unwrap().len(),
            1
        );
    }
}
