//! Attribute values as Rust data: what the rim decodes a `sai_attribute_value_t` into and
//! encodes it from, by the attribute's [`sai_meta::ValueType`].
use crate::oid::RawOid;
use sai_meta::ValueType;

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
    u16: U16;
    u32: U32;
    i32: I32 | Enum;
    [u8; 6]: Mac;
    RawOid: Oid;
    Pointer: Pointer;
}
