//! Typed ids of the MACsec objects, direction, association number, cipher suite.
//!
//! The 64 bits are laid out as `sai-core`'s ids are (and the C++ library's): object type in
//! bits 63..56, switch index 55..48, index 47..0. A typed id proves the type, not existence.
//! `sai-core`'s `ObjectType` is a closed list without the MACsec types, so the ids are
//! defined here; they move there unchanged when its store serves these objects.
use sai_core::oid::RawOid;
use sai_core::{Error, Result};
use sai_sys as sys;

const TYPE_SHIFT: u32 = 56;
const SWITCH_SHIFT: u32 = 48;
const INDEX_MASK: u64 = (1 << SWITCH_SHIFT) - 1;

/// SAI enumerators are small non-negative numbers: bindgen types them `u32`, attribute
/// values carry them in `s32`.
pub(crate) const fn en(v: u32) -> i32 {
    v.cast_signed()
}

macro_rules! ids {
    ($($(#[$doc:meta])* $id:ident = $sai:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $id(RawOid);

        impl $id {
            /// The SAI object type of every id of this Rust type.
            pub const SAI_TYPE: sys::sai_object_type_t = sys::$sai;

            /// The id as the C ABI carries it.
            pub const fn raw(self) -> RawOid {
                self.0
            }

            /// The index within the type (bits 47..0).
            pub const fn index(self) -> u64 {
                self.0.0 & INDEX_MASK
            }

            pub(crate) const fn from_index(index: u64) -> Self {
                Self(RawOid(((sys::$sai as u64) << TYPE_SHIFT) | (index & INDEX_MASK)))
            }
        }

        impl From<$id> for RawOid {
            fn from(id: $id) -> Self {
                id.0
            }
        }

        impl TryFrom<RawOid> for $id {
            type Error = Error;
            /// Checks the type bits and that the id names switch 0, the only one.
            fn try_from(raw: RawOid) -> Result<Self> {
                if raw.is_null() || (raw.0 >> SWITCH_SHIFT) & 0xff != 0 {
                    Err(Error::InvalidObjectId)
                } else if raw.type_bits() != sys::$sai {
                    Err(Error::InvalidObjectType)
                } else {
                    Ok(Self(raw))
                }
            }
        }
    )*};
}

ids! {
    /// A MACSEC object (one per direction: capabilities only).
    MacsecId = SAI_OBJECT_TYPE_MACSEC;
    /// A MACSEC_PORT: the MACsec engine of a port, one direction.
    MacsecPortId = SAI_OBJECT_TYPE_MACSEC_PORT;
    /// A MACSEC_FLOW.
    FlowId = SAI_OBJECT_TYPE_MACSEC_FLOW;
    /// A MACSEC_SC (secure channel).
    ScId = SAI_OBJECT_TYPE_MACSEC_SC;
    /// A MACSEC_SA (secure association).
    SaId = SAI_OBJECT_TYPE_MACSEC_SA;
    /// An ACL table of a MACsec stage (kept in this model, never in the ASIC).
    AclTableId = SAI_OBJECT_TYPE_ACL_TABLE;
    /// An entry of such a table.
    AclEntryId = SAI_OBJECT_TYPE_ACL_ENTRY;
}

/// The chip's logical port number: the key sequences are run for ("hw" in the C++).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortKey(pub u32);

/// Direction of a MACsec object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dir {
    /// Transmit side (`SAI_MACSEC_DIRECTION_EGRESS`).
    Egress,
    /// Receive side (`SAI_MACSEC_DIRECTION_INGRESS`).
    Ingress,
}

impl Dir {
    /// Both, in the order the C++ walks them.
    pub const BOTH: [Self; 2] = [Self::Egress, Self::Ingress];

    /// From a `sai_macsec_direction_t`. As in the C++ (`dir_of`), every value that is not
    /// INGRESS is egress.
    pub const fn from_sai(v: i32) -> Self {
        if v == en(sys::SAI_MACSEC_DIRECTION_INGRESS) {
            Self::Ingress
        } else {
            Self::Egress
        }
    }

    /// The `sai_macsec_direction_t`.
    pub const fn sai(self) -> i32 {
        match self {
            Self::Egress => en(sys::SAI_MACSEC_DIRECTION_EGRESS),
            Self::Ingress => en(sys::SAI_MACSEC_DIRECTION_INGRESS),
        }
    }

    /// `egress` / `ingress`: the `<dir>` of sequence names.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Egress => "egress",
            Self::Ingress => "ingress",
        }
    }
}

/// An association number, 0..=3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct An(u8);

impl An {
    /// Every association number.
    pub const ALL: [Self; 4] = [Self(0), Self(1), Self(2), Self(3)];

    /// `None` above 3.
    pub const fn new(an: u8) -> Option<Self> {
        if an <= 3 { Some(Self(an)) } else { None }
    }

    /// The number.
    pub const fn get(self) -> u8 {
        self.0
    }

    pub(crate) const fn idx(self) -> usize {
        self.0 as usize
    }
}

/// A MACsec cipher suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cipher {
    /// GCM-AES-128, 32-bit packet number.
    GcmAes128,
    /// GCM-AES-256, 32-bit packet number.
    GcmAes256,
    /// GCM-AES-XPN-128, 64-bit packet number.
    GcmAesXpn128,
    /// GCM-AES-XPN-256, 64-bit packet number.
    GcmAesXpn256,
}

impl Cipher {
    /// From a `sai_macsec_cipher_suite_t`; `None` for a value SAI does not define.
    pub const fn from_sai(v: i32) -> Option<Self> {
        if v == en(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128) {
            Some(Self::GcmAes128)
        } else if v == en(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_256) {
            Some(Self::GcmAes256)
        } else if v == en(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_128) {
            Some(Self::GcmAesXpn128)
        } else if v == en(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_256) {
            Some(Self::GcmAesXpn256)
        } else {
            None
        }
    }

    /// The `sai_macsec_cipher_suite_t`.
    pub const fn sai(self) -> i32 {
        match self {
            Self::GcmAes128 => en(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128),
            Self::GcmAes256 => en(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_256),
            Self::GcmAesXpn128 => en(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_128),
            Self::GcmAesXpn256 => en(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_256),
        }
    }

    /// The `<cipher>` of sequence names.
    pub const fn name(self) -> &'static str {
        match self {
            Self::GcmAes128 => "gcm-aes-128",
            Self::GcmAes256 => "gcm-aes-256",
            Self::GcmAesXpn128 => "gcm-aes-xpn-128",
            Self::GcmAesXpn256 => "gcm-aes-xpn-256",
        }
    }

    /// Whether the packet number has 64 bits (salt and SSCI are then needed).
    pub const fn is_xpn(self) -> bool {
        matches!(self, Self::GcmAesXpn128 | Self::GcmAesXpn256)
    }

    /// Length of the SAK in bytes: 16 or 32.
    pub const fn key_len(self) -> usize {
        match self {
            Self::GcmAes128 | Self::GcmAesXpn128 => 16,
            Self::GcmAes256 | Self::GcmAesXpn256 => 32,
        }
    }
}

/// Counts indices up per type; an index is never given out twice.
#[derive(Debug, Default)]
pub(crate) struct Alloc {
    next: u64,
}

impl Alloc {
    pub(crate) fn next(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_decode_only_their_own_type() {
        let sa = SaId::from_index(7);
        assert_eq!(sa.index(), 7);
        assert_eq!(sa.raw().type_bits(), sys::SAI_OBJECT_TYPE_MACSEC_SA);
        assert_eq!(SaId::try_from(sa.raw()), Ok(sa));
        assert_eq!(ScId::try_from(sa.raw()), Err(Error::InvalidObjectType));
        assert_eq!(SaId::try_from(RawOid::NULL), Err(Error::InvalidObjectId));
        assert_eq!(
            SaId::try_from(RawOid(sa.raw().0 | 1 << 48)),
            Err(Error::InvalidObjectId)
        );
    }

    #[test]
    fn enumerations_round_trip() {
        for dir in Dir::BOTH {
            assert_eq!(Dir::from_sai(dir.sai()), dir);
        }
        assert_eq!(Dir::from_sai(77), Dir::Egress); // the C++ dir_of
        for c in [
            Cipher::GcmAes128,
            Cipher::GcmAes256,
            Cipher::GcmAesXpn128,
            Cipher::GcmAesXpn256,
        ] {
            assert_eq!(Cipher::from_sai(c.sai()), Some(c));
            assert_eq!(c.key_len() == 32, c.name().ends_with("256"));
            assert_eq!(c.is_xpn(), c.name().contains("-xpn-"));
        }
        assert_eq!(Cipher::from_sai(99), None);
        assert_eq!(An::new(3).map(An::get), Some(3));
        assert_eq!(An::new(4), None);
    }
}
