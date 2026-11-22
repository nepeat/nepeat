//! The objects of this slice. Each holds its own state only: lists such as a VLAN's members
//! or the switch's ports are read from the store, so there is no second copy to keep in step.
use crate::backend::{HwPort, Vid};
use crate::oid::{BridgeId, BridgePortId, PortId, RawOid, TypedId, VlanId};
use crate::value::Value;
use sai_sys as sys;
use std::collections::BTreeMap;

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
    /// Notification callbacks and other kept attributes.
    pub kept: Kept,
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
}

impl Object {
    /// The objects this one refers to. The store keeps every one of them alive while this
    /// object exists.
    pub fn references(&self) -> Vec<RawOid> {
        match self {
            Self::Switch(s) => vec![s.cpu_port.raw(), s.default_vlan.raw(), s.default_bridge.raw()],
            Self::Port(_) | Self::Vlan(_) | Self::Bridge(_) => Vec::new(),
            Self::VlanMember(m) => vec![m.vlan.raw(), m.bridge_port.raw()],
            Self::BridgePort(b) => vec![b.bridge.raw(), b.port.raw()],
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
