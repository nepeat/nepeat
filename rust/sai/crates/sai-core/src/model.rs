//! The objects of this slice. Each holds its own state only: lists such as a VLAN's members
//! or the switch's ports are read from the store, so there is no second copy to keep in step.
use crate::backend::{HwPort, Vid};
use crate::oid::{BridgeId, BridgePortId, EntryType, ObjectType, PortId, RawOid, TypedId, VlanId};
use crate::value::{IpPrefix, Value};
use sai_sys as sys;
use std::collections::BTreeMap;
use std::net::IpAddr;

/// Attributes an object accepts, keeps and returns without acting on them ("stubs", as in
/// the C++ implementation): id to value.
pub type Kept = BTreeMap<sys::sai_attr_id_t, Value>;

/// The switch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Switch {
    /// `SRC_MAC_ADDRESS`
    pub src_mac: [u8; 6],
    /// The CPU port.
    pub cpu_port: PortId,
    /// VLAN 1.
    pub default_vlan: VlanId,
    /// The default .1Q bridge.
    pub default_bridge: BridgeId,
    /// The other objects the switch owns, by the read-only switch attribute that names each
    /// (`DEFAULT_VIRTUAL_ROUTER_ID`, `DEFAULT_STP_INST_ID`, `DEFAULT_TRAP_GROUP`, ...).
    pub objects: BTreeMap<sys::sai_attr_id_t, RawOid>,
    /// Notification callbacks and other kept attributes.
    pub kept: Kept,
}

/// An object whose whole state is its attributes: stored as the metadata allows, with no
/// typed code of its own ([`crate::bag`]). Object id attributes are references the store
/// counts like any other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bag {
    /// What the object is.
    pub object_type: ObjectType,
    /// Its attributes: id to value.
    pub attrs: Kept,
}

/// The key of an object that has no object id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryKey {
    /// `sai_fdb_entry_t`
    Fdb {
        /// The VLAN (or bridge) the address is in.
        bv_id: RawOid,
        /// The address.
        mac: [u8; 6],
    },
    /// `sai_neighbor_entry_t`
    Neighbor {
        /// The router interface.
        rif: RawOid,
        /// The neighbor's address.
        ip: IpAddr,
    },
    /// `sai_route_entry_t`
    Route {
        /// The virtual router.
        vr: RawOid,
        /// The destination.
        prefix: IpPrefix,
    },
}

impl EntryKey {
    /// The entry's type.
    pub const fn entry_type(&self) -> EntryType {
        match self {
            Self::Fdb { .. } => EntryType::Fdb,
            Self::Neighbor { .. } => EntryType::Neighbor,
            Self::Route { .. } => EntryType::Route,
        }
    }

    /// The object the key names: the entry cannot outlive it.
    pub const fn owner(&self) -> RawOid {
        match self {
            Self::Fdb { bv_id, .. } => *bv_id,
            Self::Neighbor { rif, .. } => *rif,
            Self::Route { vr, .. } => *vr,
        }
    }
}

/// The objects a set of attributes refers to.
pub fn references_of(attrs: &Kept) -> Vec<RawOid> {
    attrs.values().flat_map(Value::object_ids).collect()
}

/// What a port is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortKind {
    /// A front-panel port.
    Front {
        /// Its physical lanes.
        lanes: Vec<u32>,
    },
    /// The CPU port.
    Cpu,
}

/// A port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    /// The chip's logical port.
    pub hw: HwPort,
    /// Front panel or CPU.
    pub kind: PortKind,
    /// `ADMIN_STATE`
    pub admin: bool,
    /// `SPEED` in Mb/s
    pub speed: u32,
    /// The link state last reported to the notification callback.
    pub oper_up: bool,
    /// Kept attributes.
    pub kept: Kept,
}

/// A VLAN.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vlan {
    /// The 802.1Q id.
    pub vid: Vid,
    /// Kept attributes.
    pub kept: Kept,
}

/// A bridge port's membership of a VLAN.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VlanMember {
    /// The VLAN.
    pub vlan: VlanId,
    /// The member.
    pub bridge_port: BridgePortId,
    /// Egress untagged (true) or tagged.
    pub untagged: bool,
}

/// A bridge. Only the default .1Q bridge exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bridge {
    /// Kept attributes.
    pub kept: Kept,
}

/// A port attached to the .1Q bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgePort {
    /// The bridge.
    pub bridge: BridgeId,
    /// The port.
    pub port: PortId,
    /// `ADMIN_STATE`
    pub admin: bool,
    /// Kept attributes.
    pub kept: Kept,
}

/// Any stored object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Object {
    /// The switch.
    Switch(Switch),
    /// A port.
    Port(Port),
    /// A VLAN.
    Vlan(Vlan),
    /// A VLAN member.
    VlanMember(VlanMember),
    /// A bridge.
    Bridge(Bridge),
    /// A bridge port.
    BridgePort(BridgePort),
    /// An object of any other type.
    Bag(Bag),
}

impl Object {
    /// The type of the object.
    pub const fn object_type(&self) -> ObjectType {
        match self {
            Self::Switch(_) => ObjectType::Switch,
            Self::Port(_) => ObjectType::Port,
            Self::Vlan(_) => ObjectType::Vlan,
            Self::VlanMember(_) => ObjectType::VlanMember,
            Self::Bridge(_) => ObjectType::Bridge,
            Self::BridgePort(_) => ObjectType::BridgePort,
            Self::Bag(bag) => bag.object_type,
        }
    }

    /// The objects this one refers to. The store keeps every one of them alive while this
    /// object exists.
    pub fn references(&self) -> Vec<RawOid> {
        match self {
            Self::Switch(s) => [s.cpu_port.raw(), s.default_vlan.raw(), s.default_bridge.raw()]
                .into_iter()
                .chain(s.objects.values().copied())
                .chain(references_of(&s.kept))
                .collect(),
            Self::Bag(bag) => references_of(&bag.attrs),
            // Kept attributes that name objects (a port's QoS maps, ...) hold them too.
            Self::Port(p) => references_of(&p.kept),
            Self::Vlan(v) => references_of(&v.kept),
            Self::Bridge(b) => references_of(&b.kept),
            Self::VlanMember(m) => vec![m.vlan.raw(), m.bridge_port.raw()],
            Self::BridgePort(b) => [b.bridge.raw(), b.port.raw()]
                .into_iter()
                .chain(references_of(&b.kept))
                .collect(),
        }
    }
}

/// A Rust type stored as one [`Object`] variant, with its id type.
pub trait Kind: Sized + 'static {
    /// The typed id of such objects.
    type Id: TypedId;
    /// Wraps into the stored form.
    fn wrap(self) -> Object;
    /// The object as this type.
    fn peel(object: &Object) -> Option<&Self>;
    /// The object as this type, mutably.
    fn peel_mut(object: &mut Object) -> Option<&mut Self>;
    /// Takes the object out of its stored form.
    fn unwrap(object: Object) -> Option<Self>;
}

macro_rules! kinds {
    ($($ty:ident: $id:ty;)*) => {$(
        impl Kind for $ty {
            type Id = $id;
            fn wrap(self) -> Object {
                Object::$ty(self)
            }
            fn peel(object: &Object) -> Option<&Self> {
                if let Object::$ty(o) = object { Some(o) } else { None }
            }
            fn peel_mut(object: &mut Object) -> Option<&mut Self> {
                if let Object::$ty(o) = object { Some(o) } else { None }
            }
            fn unwrap(object: Object) -> Option<Self> {
                if let Object::$ty(o) = object { Some(o) } else { None }
            }
        }
    )*};
}

kinds! {
    Switch: crate::oid::SwitchId;
    Port: PortId;
    Vlan: VlanId;
    VlanMember: crate::oid::VlanMemberId;
    Bridge: BridgeId;
    BridgePort: BridgePortId;
}
