//! SAI attribute metadata as Rust tables, generated at build time from the annotations in
//! the SAI headers (see `build.rs`). For every attribute of every object type: the member
//! of `sai_attribute_value_t` it travels in ([`ValueType`]), its create / set / read flags,
//! and for object ids the object types it may name. The C-ABI rim decodes and encodes values
//! by this table; the core validates calls by it. No C metadata library is linked.
//!
//! Also carried: the header's `@default` ([`DefaultValue`]), the enumerators of enum-typed
//! attributes ([`AttrMeta::enum_values`]) and the counter ids of every object type
//! ([`stat_ids`]). Not carried: conditions (only the fact that an attribute is conditional).
//! Attributes whose type is not one of [`ValueType`]'s members are listed as
//! [`ValueType::Other`] and answered "not supported" by the rim.
#![forbid(unsafe_code)]

use sai_sys as sys;

/// The member of `sai_attribute_value_t` an attribute's value travels in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueType {
    /// `booldata`
    Bool,
    /// `chardata` (32 bytes, NUL-terminated text)
    CharData,
    /// `u8`
    U8,
    /// `s8`
    I8,
    /// `u16`
    U16,
    /// `s16`
    I16,
    /// `u32`
    U32,
    /// `s32`
    I32,
    /// `u64`
    U64,
    /// `s64`
    I64,
    /// `s32` holding an enumerator of the type named by [`AttrMeta::type_name`]
    Enum,
    /// `mac`
    Mac,
    /// `oid`
    Oid,
    /// `objlist`
    OidList,
    /// `u8list`
    U8List,
    /// `s8list`
    I8List,
    /// `u16list`
    U16List,
    /// `u32list`
    U32List,
    /// `s32list` (plain or enumerators)
    I32List,
    /// `vlanlist`
    VlanList,
    /// `u32range`
    U32Range,
    /// `ptr` (notification callbacks)
    Pointer,
    /// `s16list`
    I16List,
    /// `u16range`
    U16Range,
    /// `s32range`
    I32Range,
    /// `ip4` (four bytes in network order)
    Ip4,
    /// `ip6`
    Ip6,
    /// `ipaddr`
    IpAddress,
    /// `ipprefix`
    IpPrefix,
    /// `qosmap`
    QosMapList,
    /// `maplist`
    MapList,
    /// `aclcapability`
    AclCapability,
    /// `aclresource`
    AclResourceList,
    /// `timespec`
    Timespec,
    /// Any other member: not decoded by this implementation yet.
    Other,
}

/// Create / set / read rules of an attribute (the header's `@flags`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flags(u8);

impl Flags {
    /// No flag (the header gave none).
    pub const NONE: Self = Self(0);
    /// Must be passed to create (unless the attribute is conditional).
    pub const MANDATORY_ON_CREATE: Self = Self(1);
    /// May be passed to create, never to set.
    pub const CREATE_ONLY: Self = Self(2);
    /// May be passed to create and to set.
    pub const CREATE_AND_SET: Self = Self(4);
    /// Get only.
    pub const READ_ONLY: Self = Self(8);
    /// Part of the object's key.
    pub const KEY: Self = Self(16);
    /// The value changes by itself (status, counters).
    pub const DYNAMIC: Self = Self(32);
    /// Special handling in SAI's own metadata.
    pub const SPECIAL: Self = Self(64);

    /// The raw bit set.
    pub const fn bits(self) -> u8 {
        self.0
    }
    /// A flag set from raw bits (used by the generated tables).
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }
    /// Whether every flag of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    /// May appear in a create call.
    pub const fn creatable(self) -> bool {
        self.0 & (Self::MANDATORY_ON_CREATE.0 | Self::CREATE_ONLY.0 | Self::CREATE_AND_SET.0) != 0
    }
    /// May appear in a set call.
    pub const fn settable(self) -> bool {
        self.contains(Self::CREATE_AND_SET)
    }
}

/// The header's `@default` of an attribute: what a get answers while nobody set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultValue {
    /// The header gives none (mandatory and read-only attributes).
    None,
    /// `true` / `false`
    Bool(bool),
    /// A number or an enumerator.
    Int(i64),
    /// `SAI_NULL_OBJECT_ID`
    NullOid,
    /// The empty list.
    Empty,
    /// `NULL` (pointers)
    NullPointer,
    /// The empty string.
    EmptyText,
    /// `0.0.0.0`
    ZeroIp,
    /// `00:00:00:00:00:00`
    ZeroMac,
    /// Depends on the object or the vendor ("internal", "vendor", "attrvalue ...").
    Special,
}

/// One attribute of one object type.
#[derive(Clone, Copy, Debug)]
pub struct AttrMeta {
    /// The object type the attribute belongs to.
    pub object_type: sys::sai_object_type_t,
    /// The attribute id (the header's enumerator value).
    pub id: sys::sai_attr_id_t,
    /// The enumerator's name.
    pub name: &'static str,
    /// How the value travels.
    pub value_type: ValueType,
    /// The header's `@type` text.
    pub type_name: &'static str,
    /// Create / set / read rules.
    pub flags: Flags,
    /// For object ids and lists of them: the object types the value may name.
    pub allowed_objects: &'static [sys::sai_object_type_t],
    /// For object ids: whether `SAI_NULL_OBJECT_ID` is a valid value.
    pub allow_null: bool,
    /// The header attaches a `@condition` or `@validonly` to the attribute (not evaluated here).
    pub conditional: bool,
    /// For an enum (or a list of enumerators): the enumerators of its type. Empty otherwise.
    pub enum_values: &'static [i32],
    /// The header's `@default`.
    pub default: DefaultValue,
}

// The generated tables name `sai-sys` constants and cast them to the width of the table
// they sit in (`as i32` for enumerators, which the C headers define as `int`).
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::unnecessary_cast,
    clippy::unreadable_literal
)]
mod generated {
    use super::{AttrMeta, DefaultValue, Flags, ValueType, sys};
    include!(concat!(env!("OUT_DIR"), "/meta.rs"));
}
pub use generated::{OBJECT_TYPES, UNMAPPED_ATTR_ENUMS, attributes, stat_ids};

/// The metadata of attribute `id` of `object_type`.
pub fn attribute(object_type: sys::sai_object_type_t, id: sys::sai_attr_id_t) -> Option<&'static AttrMeta> {
    // Linear: the tables are short and a few headers carry alias enumerators (same id twice).
    attributes(object_type).iter().find(|a| a.id == id)
}

/// The header's name of an object type.
pub fn object_type_name(object_type: sys::sai_object_type_t) -> Option<&'static str> {
    OBJECT_TYPES.iter().find(|(t, _)| *t == object_type).map(|(_, n)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_finds_every_entry_and_aliases_agree() {
        for (ot, _) in OBJECT_TYPES {
            for a in attributes(ot) {
                let found = attribute(ot, a.id).unwrap();
                assert_eq!((found.object_type, found.id), (ot, a.id));
                assert_eq!(found.value_type, a.value_type, "{} vs {}", found.name, a.name);
            }
        }
    }

    #[test]
    fn known_attributes() {
        let speed = attribute(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_SPEED).unwrap();
        assert_eq!(speed.name, "SAI_PORT_ATTR_SPEED");
        assert_eq!(speed.value_type, ValueType::U32);
        assert!(speed.flags.contains(Flags::MANDATORY_ON_CREATE) && speed.flags.settable());
        let lanes = attribute(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_HW_LANE_LIST).unwrap();
        assert_eq!(lanes.value_type, ValueType::U32List);
        assert!(lanes.flags.contains(Flags::CREATE_ONLY) && !lanes.flags.settable());
        let oper = attribute(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_OPER_STATUS).unwrap();
        assert_eq!(
            (oper.value_type, oper.type_name),
            (ValueType::Enum, "sai_port_oper_status_t")
        );
        assert!(oper.flags.contains(Flags::READ_ONLY) && !oper.flags.creatable());
        let bp = attribute(
            sys::SAI_OBJECT_TYPE_VLAN_MEMBER,
            sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID,
        )
        .unwrap();
        assert_eq!(bp.value_type, ValueType::Oid);
        assert_eq!(bp.allowed_objects, [sys::SAI_OBJECT_TYPE_BRIDGE_PORT]);
        let notify = attribute(
            sys::SAI_OBJECT_TYPE_SWITCH,
            sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
        )
        .unwrap();
        assert_eq!(notify.value_type, ValueType::Pointer);
        let list = attribute(sys::SAI_OBJECT_TYPE_SWITCH, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap();
        assert_eq!(list.value_type, ValueType::OidList);
        assert_eq!(
            object_type_name(sys::SAI_OBJECT_TYPE_VLAN),
            Some("SAI_OBJECT_TYPE_VLAN")
        );
        assert!(attribute(sys::SAI_OBJECT_TYPE_PORT, 0x7fff_fff0).is_none());
    }

    #[test]
    fn defaults_enumerators_and_counters() {
        let attr = |t, id| attribute(t, id).unwrap();
        let admin = attr(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_ADMIN_STATE);
        assert_eq!(admin.default, DefaultValue::Bool(false));
        let mtu = attr(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_MTU);
        assert_eq!(mtu.default, DefaultValue::Int(1514));
        let fec = attr(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_FEC_MODE);
        assert_eq!(fec.default, DefaultValue::Int(i64::from(sys::SAI_PORT_FEC_MODE_NONE)));
        assert!(fec.enum_values.contains(&sys::SAI_PORT_FEC_MODE_RS.cast_signed()));
        assert!(!fec.enum_values.contains(&1000));
        let pvid = attr(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_PORT_VLAN_ID);
        assert_eq!(pvid.default, DefaultValue::Int(1));
        let lanes = attr(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_HW_LANE_LIST);
        assert_eq!((lanes.default, lanes.enum_values.len()), (DefaultValue::None, 0));
        let acl = attr(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_INGRESS_ACL);
        assert_eq!(acl.default, DefaultValue::NullOid);
        let vr_mac = attr(
            sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER,
            sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS,
        );
        assert_eq!(vr_mac.default, DefaultValue::Special); // attrvalue SAI_SWITCH_ATTR_SRC_MAC_ADDRESS
        // a list of enumerators
        let fields = attr(sys::SAI_OBJECT_TYPE_HASH, sys::SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST);
        assert_eq!(fields.value_type, ValueType::I32List);
        assert!(
            fields
                .enum_values
                .contains(&sys::SAI_NATIVE_HASH_FIELD_SRC_IP.cast_signed())
        );
        // the new members
        let ip = attr(sys::SAI_OBJECT_TYPE_NEXT_HOP, sys::SAI_NEXT_HOP_ATTR_IP);
        assert_eq!(ip.value_type, ValueType::IpAddress);
        let map = attr(sys::SAI_OBJECT_TYPE_QOS_MAP, sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST);
        assert_eq!(map.value_type, ValueType::QosMapList);
        let stage = attr(sys::SAI_OBJECT_TYPE_SWITCH, sys::SAI_SWITCH_ATTR_ACL_STAGE_INGRESS);
        assert_eq!(stage.value_type, ValueType::AclCapability);
        // counters
        let port = stat_ids(sys::SAI_OBJECT_TYPE_PORT);
        assert!(port.contains(&sys::SAI_PORT_STAT_IF_IN_OCTETS.cast_signed()));
        assert!(stat_ids(sys::SAI_OBJECT_TYPE_QUEUE).contains(&sys::SAI_QUEUE_STAT_PACKETS.cast_signed()));
        assert_eq!(stat_ids(sys::SAI_OBJECT_TYPE_VLAN_MEMBER), [0i32; 0]);
    }

    #[test]
    fn coverage_of_the_headers() {
        let total: usize = OBJECT_TYPES.iter().map(|(t, _)| attributes(*t).len()).sum();
        let other: usize = OBJECT_TYPES
            .iter()
            .flat_map(|(t, _)| attributes(*t))
            .filter(|a| a.value_type == ValueType::Other)
            .count();
        // What the scan found, for the record (cargo test -- --nocapture).
        eprintln!(
            "sai-meta: {total} attributes of {} object types, {other} with a type not decoded yet",
            OBJECT_TYPES.len()
        );
        eprintln!("sai-meta: attribute enums without an object type: {UNMAPPED_ATTR_ENUMS:?}");
        assert!(total > 2000, "{total}");
        assert!(
            other * 5 < total,
            "{other} of {total} attributes have an undecoded type"
        );
        // Every object id attribute names the object types it accepts.
        for (t, _) in OBJECT_TYPES {
            for a in attributes(t) {
                if matches!(a.value_type, ValueType::Oid | ValueType::OidList) {
                    assert!(!a.allowed_objects.is_empty(), "{}", a.name);
                }
            }
        }
    }
}
