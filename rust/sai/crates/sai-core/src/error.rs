//! SAI status codes as a Rust error type.
use sai_sys as sys;

/// Result of a core operation.
pub type Result<T> = core::result::Result<T, Error>;

/// Why an attribute of a call was refused (the `SAI_STATUS_*_0` ranges, indexed by the
/// attribute's position in the call).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrError {
    /// The attribute may not be used in this call (read-only, create-only on set, twice).
    Invalid,
    /// The value is out of range or names the wrong object.
    InvalidValue,
    /// Known to SAI, not implemented by this library.
    NotImplemented,
    /// Not an attribute of this object type.
    Unknown,
    /// Known, and deliberately not supported.
    NotSupported,
}

/// A failed SAI call. [`Error::status`] is the code the C caller gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `SAI_STATUS_FAILURE`
    Failure,
    /// `SAI_STATUS_NOT_SUPPORTED`
    NotSupported,
    /// `SAI_STATUS_NOT_IMPLEMENTED`
    NotImplemented,
    /// `SAI_STATUS_INSUFFICIENT_RESOURCES`
    InsufficientResources,
    /// `SAI_STATUS_INVALID_PARAMETER`
    InvalidParameter,
    /// `SAI_STATUS_ITEM_ALREADY_EXISTS`
    ItemAlreadyExists,
    /// `SAI_STATUS_ITEM_NOT_FOUND`
    ItemNotFound,
    /// `SAI_STATUS_INVALID_VLAN_ID`
    InvalidVlanId,
    /// `SAI_STATUS_UNINITIALIZED`
    Uninitialized,
    /// `SAI_STATUS_TABLE_FULL`
    TableFull,
    /// `SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING`
    MandatoryAttributeMissing,
    /// `SAI_STATUS_OBJECT_IN_USE`
    ObjectInUse,
    /// `SAI_STATUS_INVALID_OBJECT_TYPE`
    InvalidObjectType,
    /// `SAI_STATUS_INVALID_OBJECT_ID`
    InvalidObjectId,
    /// `SAI_STATUS_INVALID_PORT_NUMBER`
    InvalidPortNumber,
    /// `SAI_STATUS_INVALID_PORT_MEMBER`
    InvalidPortMember,
    /// `SAI_STATUS_NO_MEMORY`
    NoMemory,
    /// `SAI_STATUS_BUFFER_OVERFLOW`
    BufferOverflow,
    /// `SAI_STATUS_ADDR_NOT_FOUND`
    AddrNotFound,
    /// One of the per-attribute ranges, with the attribute's index in the call.
    Attr(AttrError, u16),
}

impl Error {
    /// A per-attribute error for the attribute at `index` of the call.
    pub fn attr(kind: AttrError, index: usize) -> Self {
        // SAI reserves 0x10000 codes per range; a longer list reports its last slot.
        Self::Attr(kind, u16::try_from(index).unwrap_or(u16::MAX))
    }

    /// The same error for a call that carries one attribute (set).
    #[must_use]
    pub fn at_index(self, index: usize) -> Self {
        match self {
            Self::Attr(kind, _) => Self::attr(kind, index),
            other => other,
        }
    }

    /// The SAI status code.
    pub fn status(self) -> sys::sai_status_t {
        match self {
            Self::Failure => sys::SAI_STATUS_FAILURE,
            Self::NotSupported => sys::SAI_STATUS_NOT_SUPPORTED,
            Self::NotImplemented => sys::SAI_STATUS_NOT_IMPLEMENTED,
            Self::InsufficientResources => sys::SAI_STATUS_INSUFFICIENT_RESOURCES,
            Self::InvalidParameter => sys::SAI_STATUS_INVALID_PARAMETER,
            Self::ItemAlreadyExists => sys::SAI_STATUS_ITEM_ALREADY_EXISTS,
            Self::ItemNotFound => sys::SAI_STATUS_ITEM_NOT_FOUND,
            Self::InvalidVlanId => sys::SAI_STATUS_INVALID_VLAN_ID,
            Self::Uninitialized => sys::SAI_STATUS_UNINITIALIZED,
            Self::TableFull => sys::SAI_STATUS_TABLE_FULL,
            Self::MandatoryAttributeMissing => sys::SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING,
            Self::ObjectInUse => sys::SAI_STATUS_OBJECT_IN_USE,
            Self::InvalidObjectType => sys::SAI_STATUS_INVALID_OBJECT_TYPE,
            Self::InvalidObjectId => sys::SAI_STATUS_INVALID_OBJECT_ID,
            Self::InvalidPortNumber => sys::SAI_STATUS_INVALID_PORT_NUMBER,
            Self::InvalidPortMember => sys::SAI_STATUS_INVALID_PORT_MEMBER,
            Self::NoMemory => sys::SAI_STATUS_NO_MEMORY,
            Self::BufferOverflow => sys::SAI_STATUS_BUFFER_OVERFLOW,
            Self::AddrNotFound => sys::SAI_STATUS_ADDR_NOT_FOUND,
            // The ranges count downwards: SAI_STATUS_CODE(base + index) = -(base + index).
            Self::Attr(kind, index) => {
                let base = match kind {
                    AttrError::Invalid => sys::SAI_STATUS_INVALID_ATTRIBUTE_0,
                    AttrError::InvalidValue => sys::SAI_STATUS_INVALID_ATTR_VALUE_0,
                    AttrError::NotImplemented => sys::SAI_STATUS_ATTR_NOT_IMPLEMENTED_0,
                    AttrError::Unknown => sys::SAI_STATUS_UNKNOWN_ATTRIBUTE_0,
                    AttrError::NotSupported => sys::SAI_STATUS_ATTR_NOT_SUPPORTED_0,
                };
                base - sys::sai_status_t::from(index)
            }
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Attr(kind, index) => write!(f, "{kind:?} attribute at index {index} (status {})", self.status()),
            other => write!(f, "{other:?} (status {})", self.status()),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_ranges_count_downwards_and_stay_in_range() {
        assert_eq!(
            Error::attr(AttrError::Invalid, 0).status(),
            sys::SAI_STATUS_INVALID_ATTRIBUTE_0
        );
        assert_eq!(
            Error::attr(AttrError::Invalid, 3).status(),
            sys::SAI_STATUS_INVALID_ATTRIBUTE_0 - 3
        );
        assert_eq!(
            Error::attr(AttrError::Invalid, 1 << 20).status(),
            sys::SAI_STATUS_INVALID_ATTRIBUTE_MAX
        );
        assert_eq!(
            Error::attr(AttrError::NotSupported, 0xffff).status(),
            sys::SAI_STATUS_ATTR_NOT_SUPPORTED_MAX
        );
        assert_eq!(
            Error::attr(AttrError::Unknown, 2).at_index(0).status(),
            sys::SAI_STATUS_UNKNOWN_ATTRIBUTE_0
        );
        assert_eq!(Error::ObjectInUse.at_index(4), Error::ObjectInUse);
    }
}
