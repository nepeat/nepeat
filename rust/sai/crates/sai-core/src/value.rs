//! Attribute values as Rust data: what the rim decodes a `sai_attribute_value_t` into and
//! encodes it from, by the attribute's [`sai_meta::ValueType`].
use crate::oid::RawOid;
pub use sai_meta::AclKind;
use sai_meta::ValueType;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// An IP prefix as SAI carries it: address and mask of one family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IpPrefix {
    addr: IpAddr,
    mask: IpAddr,
}

impl IpPrefix {
    /// A prefix; `None` when address and mask are of different families. The address is
    /// stored masked, so two spellings of one prefix are one key.
    pub fn new(addr: IpAddr, mask: IpAddr) -> Option<Self> {
        let addr = match (addr, mask) {
            (IpAddr::V4(a), IpAddr::V4(m)) => IpAddr::V4(Ipv4Addr::from_bits(a.to_bits() & m.to_bits())),
            (IpAddr::V6(a), IpAddr::V6(m)) => IpAddr::V6(Ipv6Addr::from_bits(a.to_bits() & m.to_bits())),
            _ => return None,
        };
        Some(Self { addr, mask })
    }
    /// The (masked) address.
    pub const fn addr(self) -> IpAddr {
        self.addr
    }
    /// The mask.
    pub const fn mask(self) -> IpAddr {
        self.mask
    }
}

/// One side of a traffic-class map entry (`sai_qos_map_params_t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct QosParams {
    /// traffic class
    pub tc: u8,
    /// DSCP
    pub dscp: u8,
    /// 802.1p
    pub dot1p: u8,
    /// PFC priority
    pub prio: u8,
    /// priority group
    pub pg: u8,
    /// egress queue index
    pub queue_index: u8,
    /// `sai_packet_color_t`
    pub color: i32,
    /// MPLS EXP
    pub mpls_exp: u8,
    /// forwarding class
    pub fc: u8,
    /// drop eligibility
    pub dei: u8,
    /// VLAN class
    pub vc: u8,
}

/// One entry of a traffic-class map: key to value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct QosMapEntry {
    /// what is matched
    pub key: QosParams,
    /// what it maps to
    pub value: QosParams,
}

/// One entry of `sai_acl_resource_list_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AclResource {
    /// `sai_acl_stage_t`
    pub stage: i32,
    /// `sai_acl_bind_point_type_t`
    pub bind_point: i32,
    /// free entries
    pub avail_num: u32,
}

/// Data and mask of an ACL match field (addresses in network order, as SAI carries them).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldMatch<T> {
    /// What the packet's field must equal under the mask.
    pub data: T,
    /// The bits that are compared.
    pub mask: T,
}

/// The data of an enabled ACL match field, by the member of `sai_acl_field_data_t` the
/// attribute uses ([`AclKind`]). Object ids and booleans have no mask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AclFieldData {
    /// `booldata`
    Bool(bool),
    /// `u8`
    U8(FieldMatch<u8>),
    /// `u16`
    U16(FieldMatch<u16>),
    /// `u32`
    U32(FieldMatch<u32>),
    /// `s32`
    I32(FieldMatch<i32>),
    /// `s32` holding an enumerator (the data; the mask is a plain `s32`)
    Enum(FieldMatch<i32>),
    /// `u64`
    U64(FieldMatch<u64>),
    /// `mac`
    Mac(FieldMatch<[u8; 6]>),
    /// `ip4`
    Ip4(FieldMatch<Ipv4Addr>),
    /// `ip6`
    Ip6(FieldMatch<Ipv6Addr>),
    /// `oid`
    Oid(RawOid),
    /// `objlist`
    OidList(Vec<RawOid>),
    /// `u8list` (data and mask lists)
    U8List(FieldMatch<Vec<u8>>),
}

impl AclFieldData {
    /// The union member it travels in.
    pub const fn kind(&self) -> AclKind {
        match self {
            Self::Bool(_) => AclKind::Bool,
            Self::U8(_) => AclKind::U8,
            Self::U16(_) => AclKind::U16,
            Self::U32(_) => AclKind::U32,
            Self::I32(_) => AclKind::I32,
            Self::Enum(_) => AclKind::Enum,
            Self::U64(_) => AclKind::U64,
            Self::Mac(_) => AclKind::Mac,
            Self::Ip4(_) => AclKind::Ip4,
            Self::Ip6(_) => AclKind::Ip6,
            Self::Oid(_) => AclKind::Oid,
            Self::OidList(_) => AclKind::OidList,
            Self::U8List(_) => AclKind::U8List,
        }
    }
}

/// The parameter of an enabled ACL action, by the member of `sai_acl_action_parameter_t`
/// the attribute uses ([`AclKind`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AclParam {
    /// `booldata`
    Bool(bool),
    /// `u8`
    U8(u8),
    /// `u16`
    U16(u16),
    /// `u32`
    U32(u32),
    /// `s32`
    I32(i32),
    /// `s32` holding an enumerator
    Enum(i32),
    /// `mac`
    Mac([u8; 6]),
    /// `ip4`
    Ip4(Ipv4Addr),
    /// `ip6`
    Ip6(Ipv6Addr),
    /// `oid`
    Oid(RawOid),
    /// `objlist`
    OidList(Vec<RawOid>),
    /// `ipaddr`
    IpAddress(IpAddr),
}

impl AclParam {
    /// The union member it travels in.
    pub const fn kind(&self) -> AclKind {
        match self {
            Self::Bool(_) => AclKind::Bool,
            Self::U8(_) => AclKind::U8,
            Self::U16(_) => AclKind::U16,
            Self::U32(_) => AclKind::U32,
            Self::I32(_) => AclKind::I32,
            Self::Enum(_) => AclKind::Enum,
            Self::Mac(_) => AclKind::Mac,
            Self::Ip4(_) => AclKind::Ip4,
            Self::Ip6(_) => AclKind::Ip6,
            Self::Oid(_) => AclKind::Oid,
            Self::OidList(_) => AclKind::OidList,
            Self::IpAddress(_) => AclKind::IpAddress,
        }
    }
}

/// An ACL entry's match field (`sai_acl_field_data_t`). A disabled field carries nothing but
/// its kind: SAI leaves data and mask undefined then, so they are never read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AclField {
    /// `enable = false`
    Disabled(AclKind),
    /// `enable = true` with data and mask
    Enabled(AclFieldData),
}

impl AclField {
    /// The union member the field's data travels in.
    pub const fn kind(&self) -> AclKind {
        match self {
            Self::Disabled(kind) => *kind,
            Self::Enabled(data) => data.kind(),
        }
    }
}

/// An ACL entry's action (`sai_acl_action_data_t`). A disabled action carries only its kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AclAction {
    /// `enable = false`
    Disabled(AclKind),
    /// `enable = true` with its parameter
    Enabled(AclParam),
}

impl AclAction {
    /// The union member the action's parameter travels in.
    pub const fn kind(&self) -> AclKind {
        match self {
            Self::Disabled(kind) => *kind,
            Self::Enabled(param) => param.kind(),
        }
    }
}

/// An opaque C pointer value (a notification callback). The core stores and returns it and
/// never looks at it; only the rim turns it back into something callable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Pointer(usize);

impl Pointer {
    /// The null pointer.
    pub const NULL: Self = Self(0);
    /// A pointer value from its address.
    pub const fn from_addr(addr: usize) -> Self {
        Self(addr)
    }
    /// The address.
    pub const fn addr(self) -> usize {
        self.0
    }
    /// Whether it is null.
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

/// An attribute value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `booldata`
    Bool(bool),
    /// `chardata`, at most 31 bytes (the rim truncates)
    CharData(String),
    /// `u8`
    U8(u8),
    /// `s8`
    I8(i8),
    /// `u16`
    U16(u16),
    /// `s16`
    I16(i16),
    /// `u32`
    U32(u32),
    /// `s32`
    I32(i32),
    /// `u64`
    U64(u64),
    /// `s64`
    I64(i64),
    /// `s32` holding an enumerator
    Enum(i32),
    /// `mac`
    Mac([u8; 6]),
    /// `oid`
    Oid(RawOid),
    /// `objlist`
    OidList(Vec<RawOid>),
    /// `u8list`
    U8List(Vec<u8>),
    /// `s8list`
    I8List(Vec<i8>),
    /// `u16list`
    U16List(Vec<u16>),
    /// `u32list`
    U32List(Vec<u32>),
    /// `s32list`
    I32List(Vec<i32>),
    /// `vlanlist`
    VlanList(Vec<u16>),
    /// `u32range`
    U32Range {
        /// lower bound
        min: u32,
        /// upper bound
        max: u32,
    },
    /// `ptr`
    Pointer(Pointer),
    /// `s16list`
    I16List(Vec<i16>),
    /// `u16range`
    U16Range {
        /// lower bound
        min: u16,
        /// upper bound
        max: u16,
    },
    /// `s32range`
    I32Range {
        /// lower bound
        min: i32,
        /// upper bound
        max: i32,
    },
    /// `ip4`
    Ip4(Ipv4Addr),
    /// `ip6`
    Ip6(Ipv6Addr),
    /// `ipaddr`
    IpAddress(IpAddr),
    /// `ipprefix`
    IpPrefix(IpPrefix),
    /// `qosmap`
    QosMapList(Vec<QosMapEntry>),
    /// `maplist`: key to value
    MapList(Vec<(u32, i32)>),
    /// `aclcapability`
    AclCapability {
        /// whether a table must list its actions
        action_list_mandatory: bool,
        /// the `sai_acl_action_type_t` values of the stage
        actions: Vec<i32>,
        /// whether exact matches on non-contiguous bits are possible
        non_contiguous_exact_match: bool,
        /// `sai_acl_table_supported_match_type_t`
        match_type: i32,
    },
    /// `aclresource`
    AclResourceList(Vec<AclResource>),
    /// `timespec`
    Timespec {
        /// seconds
        secs: u64,
        /// nanoseconds
        nanos: u32,
    },
    /// `aclfield`
    AclField(AclField),
    /// `aclaction`
    AclAction(AclAction),
    /// `macsecsak` (`MACSEC_SA_ATTR_SAK`): 32 bytes, a 128-bit SAK in the last 16. Key
    /// material ([`crate::macsec::Secret`]: wiped on drop, no `Debug` of the bytes).
    MacsecSak(crate::macsec::Secret<32>),
    /// `macsecsalt` (`MACSEC_SA_ATTR_SALT`), key material.
    MacsecSalt(crate::macsec::Secret<12>),
    /// `macsecauthkey` (`MACSEC_SA_ATTR_AUTH_KEY`), key material.
    MacsecAuthKey(crate::macsec::Secret<16>),
}

impl Value {
    /// The union member this value travels in.
    pub const fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::CharData(_) => ValueType::CharData,
            Self::U8(_) => ValueType::U8,
            Self::I8(_) => ValueType::I8,
            Self::U16(_) => ValueType::U16,
            Self::I16(_) => ValueType::I16,
            Self::U32(_) => ValueType::U32,
            Self::I32(_) => ValueType::I32,
            Self::U64(_) => ValueType::U64,
            Self::I64(_) => ValueType::I64,
            Self::Enum(_) => ValueType::Enum,
            Self::Mac(_) => ValueType::Mac,
            Self::Oid(_) => ValueType::Oid,
            Self::OidList(_) => ValueType::OidList,
            Self::U8List(_) => ValueType::U8List,
            Self::I8List(_) => ValueType::I8List,
            Self::U16List(_) => ValueType::U16List,
            Self::U32List(_) => ValueType::U32List,
            Self::I32List(_) => ValueType::I32List,
            Self::VlanList(_) => ValueType::VlanList,
            Self::U32Range { .. } => ValueType::U32Range,
            Self::Pointer(_) => ValueType::Pointer,
            Self::I16List(_) => ValueType::I16List,
            Self::U16Range { .. } => ValueType::U16Range,
            Self::I32Range { .. } => ValueType::I32Range,
            Self::Ip4(_) => ValueType::Ip4,
            Self::Ip6(_) => ValueType::Ip6,
            Self::IpAddress(_) => ValueType::IpAddress,
            Self::IpPrefix(_) => ValueType::IpPrefix,
            Self::QosMapList(_) => ValueType::QosMapList,
            Self::MapList(_) => ValueType::MapList,
            Self::AclCapability { .. } => ValueType::AclCapability,
            Self::AclResourceList(_) => ValueType::AclResourceList,
            Self::Timespec { .. } => ValueType::Timespec,
            Self::AclField(field) => ValueType::AclField(field.kind()),
            Self::AclAction(action) => ValueType::AclAction(action.kind()),
            // decoded by attribute id at the rim (sai-meta does not decode these members)
            Self::MacsecSak(_) => ValueType::MacsecSak,
            Self::MacsecSalt(_) => ValueType::MacsecSalt,
            Self::MacsecAuthKey(_) => ValueType::MacsecAuthKey,
        }
    }

    /// The object ids the value holds (null ids left out): what an object that stores the
    /// value refers to.
    pub fn object_ids(&self) -> Vec<RawOid> {
        match self {
            Self::Oid(oid) if !oid.is_null() => vec![*oid],
            Self::OidList(list)
            | Self::AclField(AclField::Enabled(AclFieldData::OidList(list)))
            | Self::AclAction(AclAction::Enabled(AclParam::OidList(list))) => {
                list.iter().copied().filter(|oid| !oid.is_null()).collect()
            }
            Self::AclField(AclField::Enabled(AclFieldData::Oid(oid)))
            | Self::AclAction(AclAction::Enabled(AclParam::Oid(oid)))
                if !oid.is_null() =>
            {
                vec![*oid]
            }
            _ => Vec::new(),
        }
    }
}

/// One attribute of a call: id and decoded value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    /// The attribute id.
    pub id: sai_sys::sai_attr_id_t,
    /// Its value.
    pub value: Value,
}

impl Attr {
    /// An attribute.
    pub const fn new(id: sai_sys::sai_attr_id_t, value: Value) -> Self {
        Self { id, value }
    }
}

/// A Rust type one [`Value`] variant carries (for the typed argument getters).
pub trait FromValue: Sized {
    /// The payload when `value` is the matching variant.
    fn from_value(value: &Value) -> Option<Self>;
}

macro_rules! from_value {
    ($($ty:ty: $($variant:ident)|+;)*) => {$(
        impl FromValue for $ty {
            fn from_value(value: &Value) -> Option<Self> {
                match value {
                    $(Value::$variant(v) => Some(v.clone()),)+
                    _ => None,
                }
            }
        }
    )*};
}

from_value! {
    bool: Bool;
    u8: U8;
    u16: U16;
    String: CharData;
    IpAddr: IpAddress;
    u32: U32;
    i32: I32 | Enum;
    [u8; 6]: Mac;
    RawOid: Oid;
    Pointer: Pointer;
    AclField: AclField;
    AclAction: AclAction;
}
