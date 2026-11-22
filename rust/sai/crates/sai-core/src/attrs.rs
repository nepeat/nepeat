//! Checking the attributes of a call against the SAI metadata before any object code runs:
//! unknown ids, values of the wrong kind, attributes a call may not carry (read-only on
//! create, create-only on set), duplicates, missing mandatory ones, object ids that are
//! null or of a type the attribute does not accept.
use crate::error::{AttrError, Error, Result};
use crate::model::Kept;
use crate::oid::{ObjectType, RawOid, TypedId};
use crate::value::{Attr, FromValue, Value};
use sai_meta::{AttrMeta, Flags};
use sai_sys as sys;

/// The metadata of `id`, or the "unknown attribute" error for position `index`.
pub fn meta(object_type: ObjectType, id: sys::sai_attr_id_t, index: usize) -> Result<&'static AttrMeta> {
    sai_meta::attribute(object_type.sai(), id).ok_or(Error::attr(AttrError::Unknown, index))
}

/// Checks one attribute's value against its metadata.
fn check_value(meta: &AttrMeta, value: &Value, index: usize) -> Result<()> {
    let bad = Error::attr(AttrError::InvalidValue, index);
    if value.value_type() != meta.value_type {
        return Err(bad);
    }
    let check_oid = |oid: RawOid| {
        if oid.is_null() {
            return if meta.allow_null { Ok(()) } else { Err(bad) };
        }
        if meta.allowed_objects.contains(&oid.sai_object_type()) {
            Ok(())
        } else {
            Err(bad)
        }
    };
    match value {
        Value::Oid(oid) => check_oid(*oid),
        Value::OidList(list) => list.iter().try_for_each(|oid| check_oid(*oid)),
        _ => Ok(()),
    }
}

/// Checks the attribute of a set call and returns its metadata.
pub fn check_set(object_type: ObjectType, attr: &Attr) -> Result<&'static AttrMeta> {
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
    pub fn check(object_type: ObjectType, attrs: &'a [Attr]) -> Result<Self> {
        for (index, attr) in attrs.iter().enumerate() {
            let meta = meta(object_type, attr.id, index)?;
            if !meta.flags.creatable() {
                return Err(Error::attr(AttrError::Invalid, index));
            }
            check_value(meta, &attr.value, index)?;
            if attrs.iter().take(index).any(|earlier| earlier.id == attr.id) {
                return Err(Error::InvalidParameter);
            }
        }
        // Conditional attributes are mandatory only under their condition, which the tables do
        // not carry: those are left to the object code.
        let missing = sai_meta::attributes(object_type.sai())
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
        let vlan = |attrs: &[Attr]| CreateArgs::check(ObjectType::Vlan, attrs).map(|_| ());
        assert_eq!(vlan(&[]), Err(Error::MandatoryAttributeMissing));
        assert_eq!(vlan(&[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5))]), Ok(()));
        // wrong kind of value for the attribute
        assert_eq!(
            vlan(&[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U32(5))]),
            Err(Error::attr(AttrError::InvalidValue, 0))
        );
        // read-only on create
        assert_eq!(
            vlan(&[
                Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5)),
                Attr::new(sys::SAI_VLAN_ATTR_MEMBER_LIST, Value::OidList(vec![]))
            ]),
            Err(Error::attr(AttrError::Invalid, 1))
        );
        // not an attribute at all
        assert_eq!(
            vlan(&[
                Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5)),
                Attr::new(0x0fff_ffff, Value::Bool(true))
            ]),
            Err(Error::attr(AttrError::Unknown, 1))
        );
        // twice
        let id = Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(5));
        assert_eq!(vlan(&[id.clone(), id]), Err(Error::InvalidParameter));
    }

    #[test]
    fn object_ids_must_be_of_an_accepted_type() {
        let member = |vlan: RawOid, bp: RawOid| {
            CreateArgs::check(
                ObjectType::VlanMember,
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
        let set = |id, value| check_set(ObjectType::Port, &Attr::new(id, value)).map(|_| ());
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
        let mut args = CreateArgs::check(ObjectType::Vlan, &attrs).unwrap();
        assert_eq!(args.require::<u16>(sys::SAI_VLAN_ATTR_VLAN_ID), Ok(5));
        assert_eq!(
            args.finish(&[], false).unwrap_err(),
            Error::attr(AttrError::NotSupported, 1)
        );
        let mut args = CreateArgs::check(ObjectType::Vlan, &attrs).unwrap();
        args.require::<u16>(sys::SAI_VLAN_ATTR_VLAN_ID).unwrap();
        assert_eq!(
            args.finish(&[sys::SAI_VLAN_ATTR_LEARN_DISABLE], false).unwrap().len(),
            1
        );
    }
}
