//! Object ids. Layout of the 64 bits (the same as the C++ sai-openbcm, so ids of the two
//! implementations read alike in `ASIC_DB`): bits 63..56 object type, 55..48 switch index,
//! 47..0 index within the type.
//!
//! [`RawOid`] is what the C caller holds and may be anything. A typed id ([`PortId`],
//! [`VlanId`], ...) can only be made by decoding a raw id of that type or by the store when
//! it creates an object, so a function that takes a `VlanId` cannot be handed a port. A typed
//! id says nothing about existence: that is the store's answer ([`crate::store::Store::get`]).
use crate::error::{Error, Result};
use sai_sys as sys;

const TYPE_SHIFT: u32 = 56;
const SWITCH_SHIFT: u32 = 48;
const INDEX_MASK: u64 = (1 << SWITCH_SHIFT) - 1;

/// An object id as the C ABI carries it. Not validated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RawOid(pub u64);

impl RawOid {
    /// `SAI_NULL_OBJECT_ID`
    pub const NULL: Self = Self(0);

    /// Whether this is the null id.
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    /// The SAI object type number in the id's type bits, without any check.
    pub const fn type_bits(self) -> sys::sai_object_type_t {
        (self.0 >> TYPE_SHIFT) as sys::sai_object_type_t
    }

    /// The SAI object type of the id, `SAI_OBJECT_TYPE_NULL` when the type bits name none
    /// (`sai_object_type_query`; decodes only, the object need not exist).
    pub fn sai_object_type(self) -> sys::sai_object_type_t {
        let t = self.type_bits();
        if !self.is_null() && sai_meta::object_type_name(t).is_some() {
            t
        } else {
            sys::SAI_OBJECT_TYPE_NULL
        }
    }

    /// The id of the switch the object belongs to, null for ids that decode to no type
    /// (`sai_switch_id_query`).
    #[must_use]
    pub fn switch_id(self) -> RawOid {
        if self.sai_object_type() == sys::SAI_OBJECT_TYPE_NULL {
            return Self::NULL;
        }
        let switch_index = (self.0 >> SWITCH_SHIFT) & 0xff;
        Self((u64::from(sys::SAI_OBJECT_TYPE_SWITCH) << TYPE_SHIFT) | (switch_index << SWITCH_SHIFT))
    }
}

/// A typed object id.
pub trait TypedId: Copy + Ord + TryFrom<RawOid, Error = Error> {
    /// The object type every id of this Rust type has.
    const TYPE: ObjectType;
    /// The id as the C ABI carries it.
    fn raw(self) -> RawOid;
    /// The index within the type (bits 47..0).
    fn index(self) -> u64 {
        self.raw().0 & INDEX_MASK
    }
    /// The id of index `index` (only its low 48 bits are used). For the store.
    #[doc(hidden)]
    fn from_index(index: u64) -> Self;
}

macro_rules! object_types {
    ($($(#[$doc:meta])* $variant:ident = $sai:ident, $id:ident;)*) => {
        /// The object types this library implements.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ObjectType {
            $($(#[$doc])* $variant,)*
        }

        impl ObjectType {
            /// Every implemented type.
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The SAI object type number.
            pub const fn sai(self) -> sys::sai_object_type_t {
                match self { $(Self::$variant => sys::$sai,)* }
            }

            /// The implemented type with this SAI number.
            pub const fn from_sai(t: sys::sai_object_type_t) -> Option<Self> {
                match t { $(sys::$sai => Some(Self::$variant),)* _ => None }
            }
        }

        $(
            $(#[$doc])*
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
            pub struct $id(RawOid);

            impl TypedId for $id {
                const TYPE: ObjectType = ObjectType::$variant;
                fn raw(self) -> RawOid {
                    self.0
                }
                fn from_index(index: u64) -> Self {
                    Self(RawOid((u64::from(sys::$sai) << TYPE_SHIFT) | (index & INDEX_MASK)))
                }
            }

            impl TryFrom<RawOid> for $id {
                type Error = Error;
                /// Checks the type bits (and that the id names switch 0, the only one).
                fn try_from(raw: RawOid) -> Result<Self> {
                    if raw.is_null() {
                        Err(Error::InvalidObjectId)
                    } else if raw.type_bits() != sys::$sai {
                        Err(Error::InvalidObjectType)
                    } else if (raw.0 >> SWITCH_SHIFT) & 0xff != 0 {
                        Err(Error::InvalidObjectId)
                    } else {
                        Ok(Self(raw))
                    }
                }
            }
        )*

        /// A raw id decoded to whichever typed id its type bits name.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum AnyId {
            $($(#[$doc])* $variant($id),)*
        }

        impl AnyId {
            /// Decodes a raw id; an id of a type this library does not implement is
            /// `InvalidObjectType`.
            pub fn decode(raw: RawOid) -> Result<Self> {
                match ObjectType::from_sai(raw.type_bits()) {
                    $(Some(ObjectType::$variant) => $id::try_from(raw).map(Self::$variant),)*
                    None if raw.is_null() => Err(Error::InvalidObjectId),
                    None => Err(Error::InvalidObjectType),
                }
            }

            /// The object type of the id.
            pub const fn object_type(self) -> ObjectType {
                match self { $(Self::$variant(_) => ObjectType::$variant,)* }
            }

            /// The id as the C ABI carries it.
            pub fn raw(self) -> RawOid {
                match self { $(Self::$variant(id) => id.raw(),)* }
            }
        }
    };
}

object_types! {
    /// The switch.
    Switch = SAI_OBJECT_TYPE_SWITCH, SwitchId;
    /// A front-panel port or the CPU port.
    Port = SAI_OBJECT_TYPE_PORT, PortId;
    /// A VLAN.
    Vlan = SAI_OBJECT_TYPE_VLAN, VlanId;
    /// A bridge port's membership of a VLAN.
    VlanMember = SAI_OBJECT_TYPE_VLAN_MEMBER, VlanMemberId;
    /// A bridge (only the default .1Q bridge exists).
    Bridge = SAI_OBJECT_TYPE_BRIDGE, BridgeId;
    /// A port's attachment to a bridge.
    BridgePort = SAI_OBJECT_TYPE_BRIDGE_PORT, BridgePortId;
    /// A spanning tree instance (only the default one exists).
    Stp = SAI_OBJECT_TYPE_STP, StpId;
    /// A virtual router.
    VirtualRouter = SAI_OBJECT_TYPE_VIRTUAL_ROUTER, VirtualRouterId;
    /// A router interface.
    RouterInterface = SAI_OBJECT_TYPE_ROUTER_INTERFACE, RouterInterfaceId;
    /// A next hop.
    NextHop = SAI_OBJECT_TYPE_NEXT_HOP, NextHopId;
    /// A next hop group.
    NextHopGroup = SAI_OBJECT_TYPE_NEXT_HOP_GROUP, NextHopGroupId;
    /// A next hop's membership of a group.
    NextHopGroupMember = SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER, NextHopGroupMemberId;
    /// A host interface (a netdev for a port).
    Hostif = SAI_OBJECT_TYPE_HOSTIF, HostifId;
    /// A trap group.
    HostifTrapGroup = SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, HostifTrapGroupId;
    /// A trap.
    HostifTrap = SAI_OBJECT_TYPE_HOSTIF_TRAP, HostifTrapId;
    /// A user-defined trap.
    HostifUserDefinedTrap = SAI_OBJECT_TYPE_HOSTIF_USER_DEFINED_TRAP, HostifUserDefinedTrapId;
    /// A host interface table entry.
    HostifTableEntry = SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY, HostifTableEntryId;
    /// An egress queue of a port.
    Queue = SAI_OBJECT_TYPE_QUEUE, QueueId;
    /// An ingress priority group of a port.
    IngressPriorityGroup = SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP, IngressPriorityGroupId;
    /// A scheduler group of a port.
    SchedulerGroup = SAI_OBJECT_TYPE_SCHEDULER_GROUP, SchedulerGroupId;
    /// A scheduler profile.
    Scheduler = SAI_OBJECT_TYPE_SCHEDULER, SchedulerId;
    /// A policer.
    Policer = SAI_OBJECT_TYPE_POLICER, PolicerId;
    /// A buffer pool.
    BufferPool = SAI_OBJECT_TYPE_BUFFER_POOL, BufferPoolId;
    /// A buffer profile.
    BufferProfile = SAI_OBJECT_TYPE_BUFFER_PROFILE, BufferProfileId;
    /// A traffic-class map (`QOS_MAP`).
    QosMap = SAI_OBJECT_TYPE_QOS_MAP, QosMapId;
    /// A WRED profile.
    Wred = SAI_OBJECT_TYPE_WRED, WredId;
    /// A hash object (the switch's LAG and ECMP hash).
    Hash = SAI_OBJECT_TYPE_HASH, HashId;
    /// A link aggregation group (a `PortChannel`).
    Lag = SAI_OBJECT_TYPE_LAG, LagId;
    /// A port's membership of a LAG.
    LagMember = SAI_OBJECT_TYPE_LAG_MEMBER, LagMemberId;
}

/// The object types that have no object id: their key is a struct (`sai_fdb_entry_t`, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryType {
    /// `SAI_OBJECT_TYPE_FDB_ENTRY`
    Fdb,
    /// `SAI_OBJECT_TYPE_NEIGHBOR_ENTRY`
    Neighbor,
    /// `SAI_OBJECT_TYPE_ROUTE_ENTRY`
    Route,
}

impl EntryType {
    /// Every implemented entry type.
    pub const ALL: &[Self] = &[Self::Fdb, Self::Neighbor, Self::Route];

    /// The SAI object type number.
    pub const fn sai(self) -> sys::sai_object_type_t {
        match self {
            Self::Fdb => sys::SAI_OBJECT_TYPE_FDB_ENTRY,
            Self::Neighbor => sys::SAI_OBJECT_TYPE_NEIGHBOR_ENTRY,
            Self::Route => sys::SAI_OBJECT_TYPE_ROUTE_ENTRY,
        }
    }

    /// The implemented entry type with this SAI number.
    pub const fn from_sai(t: sys::sai_object_type_t) -> Option<Self> {
        match t {
            sys::SAI_OBJECT_TYPE_FDB_ENTRY => Some(Self::Fdb),
            sys::SAI_OBJECT_TYPE_NEIGHBOR_ENTRY => Some(Self::Neighbor),
            sys::SAI_OBJECT_TYPE_ROUTE_ENTRY => Some(Self::Route),
            _ => None,
        }
    }
}

impl ObjectType {
    /// The raw id of index `index` of this type (only its low 48 bits are used).
    pub(crate) fn id(self, index: u64) -> RawOid {
        RawOid((u64::from(self.sai()) << TYPE_SHIFT) | (index & INDEX_MASK))
    }

    /// The lowest and highest raw id an object of this type can have (for range scans).
    pub(crate) fn id_range(self) -> core::ops::RangeInclusive<RawOid> {
        let base = u64::from(self.sai()) << TYPE_SHIFT;
        RawOid(base)..=RawOid(base | INDEX_MASK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_ids_decode_only_their_own_type() {
        let port = PortId::from_index(7);
        assert_eq!(port.index(), 7);
        assert_eq!(port.raw().sai_object_type(), sys::SAI_OBJECT_TYPE_PORT);
        assert_eq!(PortId::try_from(port.raw()), Ok(port));
        assert_eq!(VlanId::try_from(port.raw()), Err(Error::InvalidObjectType));
        assert_eq!(PortId::try_from(RawOid::NULL), Err(Error::InvalidObjectId));
        assert_eq!(AnyId::decode(port.raw()), Ok(AnyId::Port(port)));
        assert_eq!(AnyId::decode(RawOid::NULL), Err(Error::InvalidObjectId));
        // a type SAI knows and this library does not implement
        let route = RawOid(u64::from(sys::SAI_OBJECT_TYPE_TUNNEL) << 56 | 1);
        assert_eq!(AnyId::decode(route), Err(Error::InvalidObjectType));
        assert_eq!(route.sai_object_type(), sys::SAI_OBJECT_TYPE_TUNNEL);
        let queue = ObjectType::Queue.id(0x0305);
        assert_eq!(AnyId::decode(queue).map(AnyId::object_type), Ok(ObjectType::Queue));
        assert_eq!(AnyId::decode(queue).map(AnyId::raw), Ok(queue));
        for entry in EntryType::ALL {
            assert_eq!(EntryType::from_sai(entry.sai()), Some(*entry));
            assert_eq!(ObjectType::from_sai(entry.sai()), None);
        }
        // type bits that name nothing
        assert_eq!(
            RawOid(0xff00_0000_0000_0001).sai_object_type(),
            sys::SAI_OBJECT_TYPE_NULL
        );
        assert_eq!(RawOid(0xff00_0000_0000_0001).switch_id(), RawOid::NULL);
        // another switch index is not this switch's object
        assert_eq!(
            PortId::try_from(RawOid(port.raw().0 | 1 << 48)),
            Err(Error::InvalidObjectId)
        );
        assert_eq!(port.raw().switch_id(), SwitchId::from_index(0).raw());
    }
}
