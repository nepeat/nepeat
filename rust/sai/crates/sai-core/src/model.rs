//! The objects of this slice. Each holds its own state only: lists such as a VLAN's members
//! or the switch's ports are read from the store, so there is no second copy to keep in step.
use crate::backend::types::{Fec, Loopback};
use crate::backend::{HwPort, Vid};
use crate::oid::{BridgeId, BridgePortId, EntryType, ObjectType, PortId, RawOid, TypedId, VlanId};
use crate::value::{IpPrefix, Value};

pub use crate::l2::{FdbLearning, Tagging};
pub use crate::lag::{Attach, Lag, LagMember};
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
    /// ACL bindings and the `CoPP` field group ([`crate::acl`], [`crate::copp`]).
    pub acl: crate::acl::AclState,
    /// `FDB_AGING_TIME` as last programmed (0 until set).
    pub fdb_aging: u32,
    /// The board quirks (`SAI_OPENBCM_PORT_*`), parsed at creation ([`crate::port`]).
    pub quirks: crate::port::PortQuirks,
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

/// A port (the C++ `PortObj`; `QoS`, mirror, storm and `MACsec` state of other areas lives in
/// [`Port::kept`] or in those areas).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "each is one SAI attribute's state")]
pub struct Port {
    /// The chip's logical port.
    pub hw: HwPort,
    /// Front panel or CPU.
    pub kind: PortKind,
    /// The SDK's name (`ce0`; `cpu0` for the CPU port), for logs and the dump.
    pub name: String,
    /// `ADMIN_STATE`
    pub admin: bool,
    /// `SPEED` in Mb/s as the chip came up (the attribute is read from the chip).
    pub speed: u32,
    /// The link state last reported to the notification callback.
    pub oper_up: bool,
    /// `MTU` (1514 until set).
    pub mtu: u32,
    /// `FEC_MODE` (with a fixed-FEC board quirk: kept, not programmed).
    pub fec: Fec,
    /// `AUTO_NEG_MODE` (the same with a fixed-autoneg quirk).
    pub autoneg: bool,
    /// `LOOPBACK_MODE`
    pub loopback: Loopback,
    /// `PORT_VLAN_ID`
    pub pvid: Vid,
    /// The `MACsec` inter-frame gap is programmed (`crate::port::port_ifg_apply`).
    pub macsec_ifg: bool,
    /// Kept attributes.
    pub kept: Kept,
}

impl Port {
    /// A front-panel port as the chip came up: admin down, link down, SAI defaults.
    pub fn front(hw: HwPort, lanes: Vec<u32>, speed: u32, name: String) -> Self {
        Self {
            hw,
            kind: PortKind::Front { lanes },
            name,
            admin: false,
            speed,
            oper_up: false,
            mtu: 1514,
            fec: Fec::None,
            autoneg: false,
            loopback: Loopback::None,
            pvid: Vid::DEFAULT,
            macsec_ifg: false,
            kept: Kept::new(),
        }
    }

    /// The CPU port: always enabled and up.
    pub fn cpu(hw: HwPort) -> Self {
        Self {
            kind: PortKind::Cpu,
            admin: true,
            oper_up: true,
            ..Self::front(hw, Vec::new(), 0, "cpu0".to_owned())
        }
    }
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
    /// Egress untagged or tagged.
    pub tagging: Tagging,
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
    /// What it sits on: a port or a LAG.
    pub attach: Attach,
    /// `ADMIN_STATE`
    pub admin: bool,
    /// `FDB_LEARNING_MODE`, programmed on its ports.
    pub learn: FdbLearning,
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
    /// A LAG ([`crate::lag`]).
    Lag(Lag),
    /// A LAG member ([`crate::lag`]).
    LagMember(LagMember),
    /// An ACL object or a policer ([`crate::acl`]).
    Acl(crate::acl::AclObject),
    /// A host interface, trap group, trap or host interface table entry ([`crate::hostif`]).
    Hostif(crate::hostif::HostifObject),
    /// A virtual router, router interface, next hop, next-hop group or member ([`crate::l3`]).
    L3(crate::l3::L3Object),
    /// A `QoS` map, scheduler, WRED profile, buffer pool or profile ([`crate::qos`]).
    Qos(crate::qos::QosObject),
    /// A mirror session or packet sampler ([`crate::mirror`]).
    Mirror(crate::mirror::MirrorObject),
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
            Self::Lag(_) => ObjectType::Lag,
            Self::LagMember(_) => ObjectType::LagMember,
            Self::Acl(acl) => acl.object_type(),
            Self::Hostif(o) => o.object_type(),
            Self::L3(o) => o.object_type(),
            Self::Qos(o) => o.object_type(),
            Self::Mirror(o) => o.object_type(),
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
                .chain(s.acl.references())
                .collect(),
            Self::Acl(acl) => acl.references(),
            Self::Hostif(o) => o.references(),
            Self::L3(o) => o.references(),
            Self::Qos(o) => o.references(),
            Self::Mirror(_) => Vec::new(),
            Self::Bag(bag) => references_of(&bag.attrs),
            // Kept attributes that name objects (a port's QoS maps, ...) hold them too.
            Self::Port(p) => references_of(&p.kept),
            Self::Vlan(v) => references_of(&v.kept),
            Self::Bridge(b) => references_of(&b.kept),
            Self::VlanMember(m) => vec![m.vlan.raw(), m.bridge_port.raw()],
            Self::BridgePort(b) => [b.bridge.raw(), b.attach.raw()]
                .into_iter()
                .chain(references_of(&b.kept))
                .collect(),
            Self::Lag(l) => references_of(&l.kept),
            Self::LagMember(m) => [m.lag.raw(), m.port.raw()]
                .into_iter()
                .chain(references_of(&m.kept))
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
    Lag: crate::oid::LagId;
    LagMember: crate::oid::LagMemberId;
}
