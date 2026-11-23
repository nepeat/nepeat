//! Attribute values of the MACsec objects and of the MACsec-stage ACL objects.
//!
//! A local type on purpose: `sai_core::value::Value` has no variant for key material or ACL
//! fields / actions yet (the crate documentation lists what it and the rim would need).
use crate::secret::Secret;
use sai_core::error::AttrError;
use sai_core::oid::RawOid;
use sai_core::{Error, Result};
use sai_sys as sys;

/// The data and mask of an enabled ACL field (the fields a MACsec-stage table can have).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AclField {
    /// `FIELD_DST_MAC`
    Mac {
        /// `data.mac`
        data: [u8; 6],
        /// `mask.mac`
        mask: [u8; 6],
    },
    /// `FIELD_ETHER_TYPE`
    U16 {
        /// `data.u16`
        data: u16,
        /// `mask.u16`
        mask: u16,
    },
    /// `FIELD_MACSEC_SCI`
    U64 {
        /// `data.u64`
        data: u64,
        /// `mask.u64`
        mask: u64,
    },
}

/// The parameter of an ACL action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AclParameter {
    /// `parameter.s32` (`ACTION_PACKET_ACTION`)
    S32(i32),
    /// `parameter.oid` (`ACTION_MACSEC_FLOW`)
    Oid(RawOid),
}

/// An attribute value.
#[derive(Clone, Debug)]
pub enum Value {
    /// `booldata`
    Bool(bool),
    /// `u8`
    U8(u8),
    /// `u32`
    U32(u32),
    /// `u64`. `SAI_MACSEC_SC_ATTR_MACSEC_SCI` travels here: see [`sci_from_attr`].
    U64(u64),
    /// `s32` holding an enumerator
    Enum(i32),
    /// `oid`
    Oid(RawOid),
    /// `objlist`
    OidList(Vec<RawOid>),
    /// `s32list`
    I32List(Vec<i32>),
    /// `macsecsak`: 32 bytes; a 128-bit SAK is the last 16 (SAI's rule).
    Sak(Secret<32>),
    /// `macsecsalt`
    Salt(Secret<12>),
    /// `macsecauthkey`
    AuthKey(Secret<16>),
    /// `aclfield`: `None` is a disabled field.
    AclField(Option<AclField>),
    /// `aclaction`
    AclAction {
        /// `enable`
        enable: bool,
        /// `parameter`
        parameter: AclParameter,
    },
}

/// One attribute of a call.
#[derive(Clone, Debug)]
pub struct Attr {
    /// The attribute id.
    pub id: sys::sai_attr_id_t,
    /// Its value.
    pub value: Value,
}

/// `SAI_STATUS_ATTR_NOT_SUPPORTED_0 - index`
pub(crate) fn not_supported(index: usize) -> Error {
    Error::attr(AttrError::NotSupported, index)
}

/// `SAI_STATUS_INVALID_ATTR_VALUE_0 - index`
pub(crate) fn invalid(index: usize) -> Error {
    Error::attr(AttrError::InvalidValue, index)
}

impl Attr {
    /// An attribute.
    pub const fn new(id: sys::sai_attr_id_t, value: Value) -> Self {
        Self { id, value }
    }

    // The typed readers. A value of another kind than the attribute's is the caller's (the
    // rim's) mistake and is answered "invalid value" at the attribute's index; the C++ reads
    // the union member blindly.
    pub(crate) fn bool(&self, index: usize) -> Result<bool> {
        match self.value {
            Value::Bool(v) => Ok(v),
            _ => Err(invalid(index)),
        }
    }

    pub(crate) fn u8(&self, index: usize) -> Result<u8> {
        match self.value {
            Value::U8(v) => Ok(v),
            _ => Err(invalid(index)),
        }
    }

    pub(crate) fn u32(&self, index: usize) -> Result<u32> {
        match self.value {
            Value::U32(v) => Ok(v),
            _ => Err(invalid(index)),
        }
    }

    pub(crate) fn u64(&self, index: usize) -> Result<u64> {
        match self.value {
            Value::U64(v) => Ok(v),
            _ => Err(invalid(index)),
        }
    }

    pub(crate) fn en(&self, index: usize) -> Result<i32> {
        match self.value {
            Value::Enum(v) => Ok(v),
            _ => Err(invalid(index)),
        }
    }

    pub(crate) fn oid(&self, index: usize) -> Result<RawOid> {
        match self.value {
            Value::Oid(v) => Ok(v),
            _ => Err(invalid(index)),
        }
    }
}

/// The first attribute with this id and its position (`find_attr`).
pub(crate) fn find(attrs: &[Attr], id: sys::sai_attr_id_t) -> Option<(usize, &Attr)> {
    attrs.iter().enumerate().find(|(_, a)| a.id == id)
}

/// The SCI bytes, as on the wire, of a `SAI_MACSEC_SC_ATTR_MACSEC_SCI` value.
///
/// The C++ copies the eight bytes of the `u64` union member in memory order
/// (`memcpy(sc.sci, &a.value.u64, 8)`, "the SCI as SONiC passes it"), so the wire bytes are
/// the value's bytes in the byte order of the machine. That is the attribute's contract as
/// the C++ implements it, not an accident of this port; it is spelled here once and nowhere
/// else. Verified on hardware only on the big-endian AS7772 (crate documentation, Q13).
pub const fn sci_from_attr(value: u64) -> [u8; 8] {
    value.to_ne_bytes()
}

/// The inverse of [`sci_from_attr`].
pub const fn sci_to_attr(sci: [u8; 8]) -> u64 {
    u64::from_ne_bytes(sci)
}
