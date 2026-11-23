//! L3: virtual routers, router interfaces, neighbors, next hops, routes and next-hop groups on
//! the chip's L3 tables. Ports `sai_l3.cpp` (1161 lines), its behaviour, statuses and backend
//! calls in their order (the differential compares the call log line by line).
//!
//! The model, as the C++:
//!
//! - **Virtual router** = an SDK VRF id: the default VR is VRF 0 (made with the switch,
//!   [`default_virtual_router`]); the others take the lowest free id from 1 up to
//!   [`ChipLimits::vrf_max`]. `ADMIN_V4/V6_STATE` false is refused (routing is always on in a VRF).
//! - **Router interface** = an L3 interface on a VLAN, bound to its VR's VRF. A port RIF gets an
//!   internal VLAN, counting down from `SAI_OPENBCM_RIF_VLAN_BASE` (default 4094), with the port as
//!   untagged member and PVID; on a LAG every member port is in it (members that join or leave
//!   later: [`rif_member_join`] / [`rif_member_leave`]). A VLAN RIF sits on its VLAN. A loopback RIF
//!   has no SDK object. The router MAC is a my-station entry: the switch MAC's once (with the first
//!   RIF that has an interface; switch MAC changes: [`l3_switch_mac_changed`]), and one per RIF whose
//!   MAC differs.
//! - **Neighbor** = an egress object (port or LAG of a port RIF; for a VLAN RIF the port of the FDB
//!   entry of its MAC, else the CPU) + a host entry in the RIF's VRF unless `NO_HOST_ROUTE`.
//! - **Next hop** = its neighbor's egress object; the shared glean (CPU) egress until the neighbor
//!   exists, then it and everything on it follows.
//! - **Route** = a route entry in its VR's VRF to an egress object (CPU for IP2ME, glean for a
//!   connected subnet, drop for DROP / DENY / no next hop) or an ECMP group. Egress objects and ECMP
//!   groups have no VRF, so a route may use a next hop in another VR (route leaking).
//! - **Next-hop group** = an SDK ECMP group over its members' resolved next hops, a member's
//!   `WEIGHT` as repetitions, cut at [`ChipLimits::ecmp_paths`]; while no member is resolved there
//!   is no SDK group and routes on it drop.
//! - RIF counters through the optional [`crate::backend::L3::l3_intf_stats`] ([`rif_stats`]); the
//!   switch's `AVAILABLE_*` capacity attributes from [`crate::backend::Base::table_usage`] ([`available`]).
//!
//! Attributes the C++ only keeps (its `StubDef` tables): RIF `NEIGHBOR_MISS_PACKET_ACTION`,
//! `V4/V6_MCAST_ENABLE`, `LOOPBACK_PACKET_ACTION`, `IS_VIRTUAL` (create only), `NAT_ZONE_ID`,
//! `DISABLE_DECREMENT_TTL`, `ADMIN_MPLS_STATE`; VR `VIOLATION_TTL1_PACKET_ACTION`,
//! `VIOLATION_IP_OPTIONS_PACKET_ACTION`, `UNKNOWN_L3_MULTICAST_PACKET_ACTION`; route `META_DATA`;
//! next-hop group member `SEQUENCE_ID`. Everything else is refused `ATTR_NOT_SUPPORTED`.
//!
//! Storage: the five object types are typed objects of the store ([`L3Object`]); neighbor and route
//! entries live in the store's [`L3Tables`] (they reference objects the store does not count for
//! them; every removal checks them first, as the C++ does).
//!
//! Where the C++ changes its model before a backend call and puts it back when the call fails
//! (next-hop group members), this port does the same, so the model ends as it was; everywhere else
//! the backend goes first.
//!
//! Registration: [`crate::dispatch::Owner::L3`] for `VIRTUAL_ROUTER`, `ROUTER_INTERFACE`,
//! `NEXT_HOP`, `NEXT_HOP_GROUP`, `NEXT_HOP_GROUP_MEMBER` and the `NEIGHBOR_ENTRY` / `ROUTE_ENTRY`
//! entry types.
use crate::backend::types::{ChipLimits, Dest, EcmpId, EgressId, IntfId, MacAddr, RouteTarget, StationId, Vrf};
use crate::backend::{Backend, HwPort, Profile, Vid};
use crate::dispatch::{Area, Cx};
use crate::error::{AttrError, Error, Result};
use crate::lag::Attach;
use crate::log;
use crate::model::{EntryKey, Kept, Kind, Object, Switch, Vlan};
use crate::oid::{
    LagId, NextHopGroupId, NextHopGroupMemberId, NextHopId, ObjectType, RawOid, RouterInterfaceId, TypedId,
    VirtualRouterId, VlanId,
};
use crate::store::Store;
use crate::value::{Attr, FromValue, IpPrefix, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

/// The profile key of the internal VLANs' base (C++ `alloc_internal_vlan`).
pub const RIF_VLAN_BASE_KEY: &str = "SAI_OPENBCM_RIF_VLAN_BASE";

/// The default and highest internal VLAN.
const RIF_VLAN_BASE: u16 = 4094;

/// A next-hop group's ECMP list is cut at the chip's paths per group; when the chip does not say
/// (`chip_limits` failed: the C++ has no limit then) at this many.
const ECMP_PATHS_UNKNOWN: usize = 1 << 16;

/// The highest counter id `sai_query_stats_capability` probes for router interfaces (C++ `0x200`).
const RIF_STAT_PROBE: sys::sai_stat_id_t = 0x200;

const RIF_PORT: i32 = sys::SAI_ROUTER_INTERFACE_TYPE_PORT.cast_signed();
const RIF_VLAN: i32 = sys::SAI_ROUTER_INTERFACE_TYPE_VLAN.cast_signed();
const RIF_LOOPBACK: i32 = sys::SAI_ROUTER_INTERFACE_TYPE_LOOPBACK.cast_signed();
const FORWARD: i32 = sys::SAI_PACKET_ACTION_FORWARD.cast_signed();
const DROP: i32 = sys::SAI_PACKET_ACTION_DROP.cast_signed();
const DENY: i32 = sys::SAI_PACKET_ACTION_DENY.cast_signed();
const TRAP: i32 = sys::SAI_PACKET_ACTION_TRAP.cast_signed();
const NEXT_HOP_IP: i32 = sys::SAI_NEXT_HOP_TYPE_IP.cast_signed();
const GROUP_ECMP: i32 = sys::SAI_NEXT_HOP_GROUP_TYPE_ECMP.cast_signed();

// ------------------------------------------------------------------ kept ("stub") attributes

/// The value a kept attribute answers while unset (C++ `StubDef` kinds).
#[derive(Clone, Copy, Debug)]
enum StubDefault {
    Bool(bool),
    U8(u8),
    U32(u32),
    Enum(u32),
}

impl StubDefault {
    fn value(self) -> Value {
        match self {
            Self::Bool(b) => Value::Bool(b),
            Self::U8(n) => Value::U8(n),
            Self::U32(n) => Value::U32(n),
            Self::Enum(n) => Value::Enum(n.cast_signed()),
        }
    }
}

/// An attribute kept without effect (C++ `StubDef`).
#[derive(Clone, Copy, Debug)]
struct Stub {
    id: sys::sai_attr_id_t,
    default: StubDefault,
    create_only: bool,
}

const fn stub(id: sys::sai_attr_id_t, default: StubDefault, create_only: bool) -> Stub {
    Stub {
        id,
        default,
        create_only,
    }
}

const RIF_STUBS: &[Stub] = &[
    stub(
        sys::SAI_ROUTER_INTERFACE_ATTR_NEIGHBOR_MISS_PACKET_ACTION,
        StubDefault::Enum(sys::SAI_PACKET_ACTION_TRAP),
        false,
    ),
    stub(
        sys::SAI_ROUTER_INTERFACE_ATTR_V4_MCAST_ENABLE,
        StubDefault::Bool(false),
        false,
    ),
    stub(
        sys::SAI_ROUTER_INTERFACE_ATTR_V6_MCAST_ENABLE,
        StubDefault::Bool(false),
        false,
    ),
    stub(
        sys::SAI_ROUTER_INTERFACE_ATTR_LOOPBACK_PACKET_ACTION,
        StubDefault::Enum(sys::SAI_PACKET_ACTION_FORWARD),
        false,
    ),
    stub(
        sys::SAI_ROUTER_INTERFACE_ATTR_IS_VIRTUAL,
        StubDefault::Bool(false),
        true,
    ),
    stub(sys::SAI_ROUTER_INTERFACE_ATTR_NAT_ZONE_ID, StubDefault::U8(0), false),
    stub(
        sys::SAI_ROUTER_INTERFACE_ATTR_DISABLE_DECREMENT_TTL,
        StubDefault::Bool(false),
        false,
    ),
    stub(
        sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_MPLS_STATE,
        StubDefault::Bool(false),
        false,
    ),
];

const VR_STUBS: &[Stub] = &[
    stub(
        sys::SAI_VIRTUAL_ROUTER_ATTR_VIOLATION_TTL1_PACKET_ACTION,
        StubDefault::Enum(sys::SAI_PACKET_ACTION_TRAP),
        false,
    ),
    stub(
        sys::SAI_VIRTUAL_ROUTER_ATTR_VIOLATION_IP_OPTIONS_PACKET_ACTION,
        StubDefault::Enum(sys::SAI_PACKET_ACTION_TRAP),
        false,
    ),
    stub(
        sys::SAI_VIRTUAL_ROUTER_ATTR_UNKNOWN_L3_MULTICAST_PACKET_ACTION,
        StubDefault::Enum(sys::SAI_PACKET_ACTION_DROP),
        false,
    ),
];

const ROUTE_STUBS: &[Stub] = &[stub(sys::SAI_ROUTE_ENTRY_ATTR_META_DATA, StubDefault::U32(0), false)];

const MEMBER_STUBS: &[Stub] = &[stub(
    sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_SEQUENCE_ID,
    StubDefault::U32(0),
    false,
)];

fn find_stub(table: &[Stub], id: sys::sai_attr_id_t) -> Option<Stub> {
    table.iter().find(|s| s.id == id).copied()
}

/// Keeps a stub attribute (create: any; set: `create_only` ones are refused, as the C++ does).
fn keep_stub(table: &[Stub], kept: &mut Kept, attr: &Attr, index: usize, create: bool) -> Result<()> {
    let stub = find_stub(table, attr.id).ok_or(unsupported(index))?;
    if !create && stub.create_only {
        return Err(bad(index));
    }
    if attr.value.value_type() != stub.default.value().value_type() {
        return Err(bad(index));
    }
    kept.insert(attr.id, attr.value.clone());
    Ok(())
}

/// What a get of a stub attribute answers: the kept value, else the C++ default.
fn stub_get(table: &[Stub], kept: &Kept, id: sys::sai_attr_id_t) -> Result<Value> {
    let stub = find_stub(table, id).ok_or(unsupported(0))?;
    Ok(kept.get(&id).cloned().unwrap_or_else(|| stub.default.value()))
}

/// C++ `attr_invalid(i)`.
fn bad(index: usize) -> Error {
    Error::attr(AttrError::InvalidValue, index)
}

/// C++ `attr_not_supported(i)`.
fn unsupported(index: usize) -> Error {
    Error::attr(AttrError::NotSupported, index)
}

/// C++ `attr_backend_status(st, i)`.
fn attr_backend_status(e: Error, index: usize) -> Error {
    match e {
        Error::InvalidParameter => bad(index),
        Error::NotSupported => unsupported(index),
        Error::NotImplemented => Error::attr(AttrError::NotImplemented, index),
        other => other,
    }
}

/// The value of an attribute as a `T` (a value of another kind is the attribute's fault).
fn arg<T: FromValue>(attr: &Attr, index: usize) -> Result<T> {
    T::from_value(&attr.value).ok_or(bad(index))
}

// ------------------------------------------------------------------------------ the objects

/// A virtual router (C++ `VrObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualRouter {
    /// Its SDK VRF.
    pub vrf: Vrf,
    /// `SRC_MAC_ADDRESS` when set (an all-zero address counts as unset, as in the C++); else
    /// the switch's.
    pub mac: Option<MacAddr>,
    /// Kept attributes.
    pub kept: Kept,
}

/// What a router interface is (`TYPE` and what it sits on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RifKind {
    /// `TYPE_PORT`: the port or LAG, and the internal VLAN allocated for it.
    Port {
        /// `PORT_ID`
        port: Attach,
        /// The internal VLAN.
        vid: Vid,
    },
    /// `TYPE_VLAN`: the VLAN.
    Vlan {
        /// `VLAN_ID`
        vlan: VlanId,
        /// Its 802.1Q id.
        vid: Vid,
    },
    /// `TYPE_LOOPBACK`: no SDK object; anchors IP2ME routes.
    Loopback,
}

/// A router interface (C++ `RifObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouterInterface {
    /// `VIRTUAL_ROUTER_ID`
    pub vr: VirtualRouterId,
    /// The VR's VRF.
    pub vrf: Vrf,
    /// Type and what it sits on.
    pub kind: RifKind,
    /// `SRC_MAC_ADDRESS` (default: the VR's).
    pub mac: MacAddr,
    /// `MTU`
    pub mtu: u32,
    /// `ADMIN_V4_STATE`
    pub v4: bool,
    /// `ADMIN_V6_STATE`
    pub v6: bool,
    /// The SDK L3 interface (none for a loopback RIF).
    pub intf: Option<IntfId>,
    /// Its own my-station entry, when its MAC is not the switch's.
    pub station: Option<StationId>,
    /// Kept attributes.
    pub kept: Kept,
}

impl RouterInterface {
    /// The VLAN its L3 interface sits on (none for a loopback RIF).
    pub const fn vid(&self) -> Option<Vid> {
        match self.kind {
            RifKind::Port { vid, .. } | RifKind::Vlan { vid, .. } => Some(vid),
            RifKind::Loopback => None,
        }
    }

    fn type_sai(&self) -> i32 {
        match self.kind {
            RifKind::Port { .. } => RIF_PORT,
            RifKind::Vlan { .. } => RIF_VLAN,
            RifKind::Loopback => RIF_LOOPBACK,
        }
    }

    fn references(&self) -> Vec<RawOid> {
        let on = match self.kind {
            RifKind::Port { port, .. } => Some(port.raw()),
            RifKind::Vlan { vlan, .. } => Some(vlan.raw()),
            RifKind::Loopback => None,
        };
        core::iter::once(self.vr.raw()).chain(on).collect()
    }
}

/// An IP next hop (C++ `NextHopObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextHop {
    /// `IP`
    pub ip: IpAddr,
    /// `ROUTER_INTERFACE_ID`
    pub rif: RouterInterfaceId,
    /// Its neighbor's egress object, the glean egress until the neighbor exists.
    pub egress: EgressId,
}

/// A next-hop group (C++ `NhgObj`; type ECMP only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NextHopGroup {
    /// Its members, in creation order.
    pub members: Vec<NextHopGroupMemberId>,
    /// The SDK ECMP group, while a member is resolved.
    pub ecmp: Option<EcmpId>,
    /// The egress objects in the SDK group (repeated by weight).
    pub programmed: Vec<EgressId>,
}

/// A member of a next-hop group (C++ `NhgMemberObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextHopGroupMember {
    /// `NEXT_HOP_GROUP_ID`
    pub group: NextHopGroupId,
    /// `NEXT_HOP_ID`
    pub next_hop: NextHopId,
    /// `WEIGHT`: repetitions of the egress object (1 or more).
    pub weight: u32,
    /// Kept attributes.
    pub kept: Kept,
}

/// The L3 objects as the store holds them ([`crate::model::Object::L3`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum L3Object {
    /// A virtual router.
    VirtualRouter(VirtualRouter),
    /// A router interface.
    RouterInterface(RouterInterface),
    /// A next hop.
    NextHop(NextHop),
    /// A next-hop group.
    NextHopGroup(NextHopGroup),
    /// A next-hop group member.
    NextHopGroupMember(NextHopGroupMember),
}

impl L3Object {
    /// The object's type.
    pub const fn object_type(&self) -> ObjectType {
        match self {
            Self::VirtualRouter(_) => ObjectType::VirtualRouter,
            Self::RouterInterface(_) => ObjectType::RouterInterface,
            Self::NextHop(_) => ObjectType::NextHop,
            Self::NextHopGroup(_) => ObjectType::NextHopGroup,
            Self::NextHopGroupMember(_) => ObjectType::NextHopGroupMember,
        }
    }

    /// What it refers to (the store keeps those alive): a RIF its VR and its port, LAG or VLAN; a
    /// next hop its RIF; a member its group and next hop.
    pub fn references(&self) -> Vec<RawOid> {
        match self {
            Self::VirtualRouter(_) | Self::NextHopGroup(_) => Vec::new(),
            Self::RouterInterface(r) => r.references(),
            Self::NextHop(n) => vec![n.rif.raw()],
            Self::NextHopGroupMember(m) => vec![m.group.raw(), m.next_hop.raw()],
        }
    }
}

macro_rules! l3_kinds {
    ($($ty:ident: $id:ty;)*) => {$(
        impl Kind for $ty {
            type Id = $id;
            fn wrap(self) -> Object {
                Object::L3(L3Object::$ty(self))
            }
            fn peel(object: &Object) -> Option<&Self> {
                if let Object::L3(L3Object::$ty(o)) = object { Some(o) } else { None }
            }
            fn peel_mut(object: &mut Object) -> Option<&mut Self> {
                if let Object::L3(L3Object::$ty(o)) = object { Some(o) } else { None }
            }
            fn unwrap(object: Object) -> Option<Self> {
                if let Object::L3(L3Object::$ty(o)) = object { Some(o) } else { None }
            }
        }
    )*};
}

l3_kinds! {
    VirtualRouter: VirtualRouterId;
    RouterInterface: RouterInterfaceId;
    NextHop: NextHopId;
    NextHopGroup: NextHopGroupId;
    NextHopGroupMember: NextHopGroupMemberId;
}

/// A route's `PACKET_ACTION` (those a route can have; others are refused `NOT_SUPPORTED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteAction {
    /// `FORWARD`: to `NEXT_HOP_ID`.
    Forward,
    /// `DROP`
    Drop,
    /// `DENY` (dropped too).
    Deny,
    /// `TRAP`: to the CPU.
    Trap,
}

impl RouteAction {
    const fn from_sai(action: i32) -> Option<Self> {
        match action {
            FORWARD => Some(Self::Forward),
            DROP => Some(Self::Drop),
            DENY => Some(Self::Deny),
            TRAP => Some(Self::Trap),
            _ => None,
        }
    }

    /// The `sai_packet_action_t`.
    pub const fn sai(self) -> i32 {
        match self {
            Self::Forward => FORWARD,
            Self::Drop => DROP,
            Self::Deny => DENY,
            Self::Trap => TRAP,
        }
    }
}

/// A neighbor entry (C++ `NeighborObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Neighbor {
    /// `DST_MAC_ADDRESS`
    pub mac: MacAddr,
    /// `NO_HOST_ROUTE`
    pub no_host: bool,
    /// Its egress object (its own, or the shared CPU / drop one).
    pub egress: EgressId,
    /// Where its own egress object points (none: a shared one).
    pub dest: Option<Dest>,
    /// A host entry was added.
    pub host: bool,
    /// The VRF of the host entry.
    pub vrf: Vrf,
}

/// A route entry (C++ `RouteObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// `PACKET_ACTION`
    pub action: RouteAction,
    /// `NEXT_HOP_ID` as given (a next hop, group, RIF, the CPU port or null; read only with
    /// `FORWARD`, as the C++ does).
    pub next_hop: RawOid,
    /// What the chip's route points at.
    pub target: RouteTarget,
    /// Kept attributes.
    pub kept: Kept,
}

/// The settings of the area read from the profile at switch creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// The highest internal VLAN (`SAI_OPENBCM_RIF_VLAN_BASE`, 2..=4094).
    pub rif_vlan_base: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rif_vlan_base: RIF_VLAN_BASE,
        }
    }
}

impl Config {
    /// The settings in a profile. A value that does not parse or is out of range is an error
    /// (the C++ falls back to 4094).
    pub fn from_profile(profile: &Profile) -> Result<Self> {
        let base = profile.number(
            RIF_VLAN_BASE_KEY,
            u32::from(RIF_VLAN_BASE),
            2..=u32::from(RIF_VLAN_BASE),
        )?;
        Ok(Self {
            rif_vlan_base: u16::try_from(base).map_err(|_| Error::InvalidParameter)?,
        })
    }
}

/// The L3 state that is not an object: neighbor and route entries, the switch's my-station entry,
/// the settings. Part of the [`Store`].
#[derive(Debug, Default)]
pub struct L3Tables {
    pub(crate) neighbors: BTreeMap<(RouterInterfaceId, IpAddr), Neighbor>,
    pub(crate) routes: BTreeMap<(VirtualRouterId, IpPrefix), Route>,
    pub(crate) switch_station: Option<StationId>,
    pub(crate) config: Config,
}

impl L3Tables {
    /// The neighbor entries.
    pub fn neighbors(&self) -> &BTreeMap<(RouterInterfaceId, IpAddr), Neighbor> {
        &self.neighbors
    }

    /// The route entries (prefixes canonical: host bits cleared).
    pub fn routes(&self) -> &BTreeMap<(VirtualRouterId, IpPrefix), Route> {
        &self.routes
    }

    /// The my-station entry of the switch MAC, once made.
    pub fn switch_station(&self) -> Option<StationId> {
        self.switch_station
    }

    /// The settings.
    pub fn config(&self) -> Config {
        self.config
    }
}

/// The L3 tables of a store.
pub fn tables(store: &Store) -> &L3Tables {
    &store.l3
}

/// The backend egress object of a neighbor (test port `neighbor_egress`).
pub fn neighbor_egress(store: &Store, rif: RouterInterfaceId, ip: IpAddr) -> Option<EgressId> {
    store.l3.neighbors.get(&(rif, ip)).map(|n| n.egress)
}

// ------------------------------------------------------------------------- area entry points

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct L3Area;

impl Area for L3Area {
    /// C++ `set_vr` (`vr_attr`: no create-only stub), `set_rif` (stub `IS_VIRTUAL` create-only),
    /// `set_next_hop` (nothing settable), `set_nhg` (`TYPE`), `set_nhgm` (`NEXT_HOP_GROUP_ID`),
    /// `sai_l3.cpp:154`-1120.
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
        let invalid: &[sys::sai_attr_id_t] = match object_type {
            ObjectType::RouterInterface => &[sys::SAI_ROUTER_INTERFACE_ATTR_IS_VIRTUAL],
            ObjectType::NextHopGroup => &[sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE],
            ObjectType::NextHopGroupMember => &[sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID],
            _ => &[],
        };
        Some(crate::dispatch::cpp_refusal(attr, invalid))
    }

    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        match object_type {
            ObjectType::VirtualRouter => create_vr(cx.backend, cx.store, attrs).map(TypedId::raw),
            ObjectType::RouterInterface => create_rif(cx, attrs).map(TypedId::raw),
            ObjectType::NextHop => create_next_hop(cx, attrs).map(TypedId::raw),
            ObjectType::NextHopGroup => create_group(cx.store, attrs).map(TypedId::raw),
            ObjectType::NextHopGroupMember => create_member(cx, attrs).map(TypedId::raw),
            _ => Err(Error::NotImplemented), // not this area's (dispatch never asks)
        }
    }

    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        match object_type {
            ObjectType::VirtualRouter => remove_vr(cx, VirtualRouterId::try_from(id)?),
            ObjectType::RouterInterface => remove_rif(cx, RouterInterfaceId::try_from(id)?),
            ObjectType::NextHop => remove_next_hop(cx, NextHopId::try_from(id)?),
            ObjectType::NextHopGroup => remove_group(cx, NextHopGroupId::try_from(id)?),
            ObjectType::NextHopGroupMember => remove_member(cx, NextHopGroupMemberId::try_from(id)?),
            _ => Err(Error::NotImplemented),
        }
    }

    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        match object_type {
            ObjectType::VirtualRouter => set_vr(cx.store, VirtualRouterId::try_from(id)?, attr),
            ObjectType::RouterInterface => set_rif(cx, RouterInterfaceId::try_from(id)?, attr),
            // C++ `set_next_hop`: nothing is settable.
            ObjectType::NextHop => Err(unsupported(0)),
            ObjectType::NextHopGroup => set_group(attr),
            ObjectType::NextHopGroupMember => set_member(cx, NextHopGroupMemberId::try_from(id)?, attr),
            _ => Err(Error::NotImplemented),
        }
    }

    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        match object_type {
            ObjectType::VirtualRouter => get_vr(store, VirtualRouterId::try_from(id)?, meta.id),
            ObjectType::RouterInterface => get_rif(store, RouterInterfaceId::try_from(id)?, meta.id),
            ObjectType::NextHop => get_next_hop(store, NextHopId::try_from(id)?, meta.id),
            ObjectType::NextHopGroup => get_group(store, NextHopGroupId::try_from(id)?, meta.id),
            ObjectType::NextHopGroupMember => get_member(store, NextHopGroupMemberId::try_from(id)?, meta.id),
            _ => {
                let _ = backend;
                Err(Error::NotImplemented)
            }
        }
    }

    fn create_entry<B: Backend>(cx: Cx<'_, B>, key: EntryKey, attrs: &[Attr]) -> Result<()> {
        match key {
            EntryKey::Neighbor { rif, ip } => create_neighbor(cx, rif, ip, attrs),
            EntryKey::Route { vr, prefix } => create_route(cx, vr, prefix, attrs),
            EntryKey::Fdb { .. } => Err(Error::NotImplemented), // the L2 area's
        }
    }

    fn remove_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey) -> Result<()> {
        match *key {
            EntryKey::Neighbor { rif, ip } => remove_neighbor(cx, rif, ip),
            EntryKey::Route { vr, prefix } => remove_route(cx, vr, prefix),
            EntryKey::Fdb { .. } => Err(Error::NotImplemented),
        }
    }

    fn set_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey, attr: &Attr) -> Result<()> {
        match *key {
            EntryKey::Neighbor { rif, ip } => set_neighbor(cx, rif, ip, attr),
            EntryKey::Route { vr, prefix } => set_route(cx, vr, prefix, attr),
            EntryKey::Fdb { .. } => Err(Error::NotImplemented),
        }
    }

    fn get_entry<B: Backend>(
        backend: &B,
        store: &Store,
        key: &EntryKey,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        match *key {
            EntryKey::Neighbor { rif, ip } => get_neighbor(store, rif, ip, ids),
            EntryKey::Route { vr, prefix } => get_route(store, vr, prefix, ids),
            EntryKey::Fdb { .. } => {
                let _ = backend;
                Err(Error::NotImplemented)
            }
        }
    }
}

// --------------------------------------------------------------------------------- helpers

fn switch(store: &Store) -> Result<&Switch> {
    store.iter::<Switch>().next().map(|(_, s)| s).ok_or(Error::Failure)
}

fn switch_mac(store: &Store) -> Result<MacAddr> {
    switch(store).map(|s| MacAddr(s.src_mac))
}

/// A VR that exists, with its VRF (C++ `vr_vrf`; `None` for -1).
fn vr_vrf(store: &Store, raw: RawOid) -> Option<(VirtualRouterId, Vrf)> {
    let id = VirtualRouterId::try_from(raw).ok()?;
    store.get::<VirtualRouter>(id).ok().map(|v| (id, v.vrf))
}

/// The MAC a VR's interfaces get by default: its own if set, else the switch's (C++ `vr_mac`).
fn vr_mac(store: &Store, vr: &VirtualRouter) -> Result<MacAddr> {
    vr.mac.map_or_else(|| switch_mac(store), Ok)
}

/// Whether an egress object is one of the backend's shared ones (C++ `egress_shared`: it asks
/// the backend for the CPU, drop and glean egress each time, in that order).
fn egress_shared<B: Backend>(backend: &mut B, egress: EgressId) -> bool {
    let cpu = backend.l3_egress_cpu().ok();
    let drop = backend.l3_egress_drop().ok();
    let glean = backend.l3_egress_glean().ok();
    [cpu, drop, glean].contains(&Some(egress))
}

fn rif_of(store: &Store, id: RouterInterfaceId) -> Result<RouterInterface> {
    store.get::<RouterInterface>(id).cloned()
}

// ------------------------------------------------------------------------------ my-station

/// The switch MAC's my-station entry, made once (C++ `ensure_switch_station`; never removed).
fn ensure_switch_station<B: Backend>(backend: &mut B, store: &mut Store) -> Result<()> {
    if store.l3.switch_station.is_some() {
        return Ok(());
    }
    let mac = switch_mac(store)?;
    store.l3.switch_station = Some(backend.l3_station_add(mac)?);
    Ok(())
}

/// The switch MAC changed from `old` to `new` (C++ `l3_switch_mac_changed`, called by the switch
/// area): once a my-station entry for the switch exists it moves to the new MAC, new one first.
/// On an error the caller keeps the old MAC.
pub(crate) fn l3_switch_mac_changed<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    old: [u8; 6],
    new: [u8; 6],
) -> Result<()> {
    let Some(station) = store.l3.switch_station else {
        return Ok(());
    };
    if old == new {
        return Ok(());
    }
    let next = backend.l3_station_add(MacAddr(new))?;
    let _ = backend.l3_station_delete(station);
    store.l3.switch_station = Some(next);
    Ok(())
}

/// Sets the area's profile settings (at switch creation).
pub(crate) fn configure(store: &mut Store, config: Config) {
    store.l3.config = config;
}

// -------------------------------------------------------------------------- virtual router

/// The default virtual router (VRF 0), made with the switch.
pub(crate) fn default_virtual_router(store: &mut Store) -> Result<VirtualRouterId> {
    store.insert(VirtualRouter {
        vrf: Vrf::DEFAULT,
        mac: None,
        kept: Kept::new(),
    })
}

/// C++ `vr_attr`.
fn vr_attr(vr: &mut VirtualRouter, attr: &Attr, index: usize, create: bool) -> Result<()> {
    match attr.id {
        sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE | sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V6_STATE => {
            // Routing is always on in an SDK VRF.
            if arg::<bool>(attr, index)? {
                Ok(())
            } else {
                Err(unsupported(index))
            }
        }
        sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS => {
            let mac: [u8; 6] = arg(attr, index)?;
            vr.mac = (mac != [0; 6]).then_some(MacAddr(mac));
            Ok(())
        }
        _ => keep_stub(VR_STUBS, &mut vr.kept, attr, index, create),
    }
}

fn create_vr<B: Backend>(backend: &B, store: &mut Store, attrs: &[Attr]) -> Result<VirtualRouterId> {
    let mut vr = VirtualRouter {
        vrf: Vrf::DEFAULT,
        mac: None,
        kept: Kept::new(),
    };
    for (index, attr) in attrs.iter().enumerate() {
        vr_attr(&mut vr, attr, index, true)?;
    }
    let ChipLimits { vrf_max, .. } = backend.chip_limits()?;
    let used: BTreeSet<Vrf> = store.iter::<VirtualRouter>().map(|(_, v)| v.vrf).collect();
    vr.vrf = (1..=vrf_max).map(Vrf).find(|vrf| !used.contains(vrf)).ok_or_else(|| {
        log::error(
            sys::SAI_API_VIRTUAL_ROUTER,
            format_args!("no free VRF id (chip maximum {vrf_max})"),
        );
        Error::InsufficientResources
    })?;
    let vrf = vr.vrf;
    let id = store.insert(vr)?;
    log::notice(
        sys::SAI_API_VIRTUAL_ROUTER,
        format_args!("virtual router: SDK VRF {}", vrf.0),
    );
    Ok(id)
}

fn default_vr_of(store: &Store) -> Option<RawOid> {
    switch(store)
        .ok()
        .and_then(|s| s.objects.get(&sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID).copied())
}

fn remove_vr<B: Backend>(cx: Cx<'_, B>, id: VirtualRouterId) -> Result<()> {
    let store = cx.store;
    store.get::<VirtualRouter>(id).map_err(|_| Error::InvalidObjectId)?;
    let in_use = default_vr_of(store) == Some(id.raw())
        || store.iter::<RouterInterface>().any(|(_, r)| r.vr == id)
        || store.l3.routes.keys().any(|(vr, _)| *vr == id);
    if in_use {
        return Err(Error::ObjectInUse);
    }
    store.remove::<VirtualRouter>(id).map(|_| ())
}

fn set_vr(store: &mut Store, id: VirtualRouterId, attr: &Attr) -> Result<()> {
    let mut vr = store.get::<VirtualRouter>(id)?.clone();
    vr_attr(&mut vr, attr, 0, false)?;
    store.update::<VirtualRouter, _>(id, |v| *v = vr)
}

fn get_vr(store: &Store, id: VirtualRouterId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let vr = store.get::<VirtualRouter>(id)?;
    match attr {
        sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE | sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V6_STATE => {
            Ok(Value::Bool(true))
        }
        sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS => vr_mac(store, vr).map(|m| Value::Mac(m.0)),
        other => stub_get(VR_STUBS, &vr.kept, other),
    }
}

// ------------------------------------------------------------------------ router interface

/// Whether a VLAN id is allocated to a port RIF (VLAN creation refuses it).
pub(crate) fn is_internal_vlan(store: &Store, vid: Vid) -> bool {
    store
        .iter::<RouterInterface>()
        .any(|(_, r)| matches!(r.kind, RifKind::Port { vid: v, .. } if v == vid))
}

/// The highest free VLAN id at or below the base (C++ `alloc_internal_vlan`).
fn alloc_internal_vlan(store: &Store) -> Option<Vid> {
    let used: BTreeSet<Vid> = store
        .iter::<Vlan>()
        .map(|(_, v)| v.vid)
        .chain(store.iter::<RouterInterface>().filter_map(|(_, r)| match r.kind {
            RifKind::Port { vid, .. } => Some(vid),
            _ => None,
        }))
        .collect();
    (2..=store.l3.config.rif_vlan_base)
        .rev()
        .filter_map(|v| Vid::new(v).ok())
        .find(|vid| !used.contains(vid))
}

/// Takes a port RIF's internal VLAN apart again (C++ `rif_undo_port_vlan`; failures ignored).
fn undo_port_vlan<B: Backend>(backend: &mut B, store: &mut Store, port: Attach, vid: Vid) {
    for hw in crate::lag::attach_phys(store, port.raw()) {
        let _ = backend.vlan_port_remove(vid, hw);
    }
    let _ = crate::lag::attach_pvid_set(backend, store, port.raw(), Vid::DEFAULT);
    let _ = backend.vlan_destroy(vid);
}

/// A port joins a LAG that has a port RIF: it joins the internal VLAN (the PVID is the LAG's, set
/// by the LAG area). C++ `rif_member_join`.
pub(crate) fn rif_member_join<B: Backend>(backend: &mut B, store: &Store, lag: LagId, hw: HwPort) -> Result<()> {
    match lag_rif_vid(store, lag) {
        Some(vid) => backend.vlan_port_add(vid, hw, true),
        None => Ok(()),
    }
}

/// A port leaves a LAG that has a port RIF: it leaves the internal VLAN. C++ `rif_member_leave`.
pub(crate) fn rif_member_leave<B: Backend>(backend: &mut B, store: &Store, lag: LagId, hw: HwPort) {
    if let Some(vid) = lag_rif_vid(store, lag) {
        let _ = backend.vlan_port_remove(vid, hw);
    }
}

fn lag_rif_vid(store: &Store, lag: LagId) -> Option<Vid> {
    store.iter::<RouterInterface>().find_map(|(_, r)| match r.kind {
        RifKind::Port {
            port: Attach::Lag(l),
            vid,
        } if l == lag => Some(vid),
        _ => None,
    })
}

/// Whether a port or LAG has a port router interface (a LAG member must not).
pub(crate) fn has_port_rif(store: &Store, obj: RawOid) -> bool {
    store
        .iter::<RouterInterface>()
        .any(|(_, r)| matches!(r.kind, RifKind::Port { port, .. } if port.raw() == obj))
}

#[allow(clippy::too_many_lines, reason = "the C++ create_rif, one check after the other")]
fn create_rif<B: Backend>(cx: Cx<'_, B>, attrs: &[Attr]) -> Result<RouterInterfaceId> {
    let Cx { backend, store, .. } = cx;
    let mut vr = None;
    let mut rif_type = None;
    let mut port = RawOid::NULL;
    let mut vlan = RawOid::NULL;
    let mut mac = None;
    let mut mtu = 1514;
    let (mut v4, mut v6) = (true, true);
    let mut kept = Kept::new();
    for (index, attr) in attrs.iter().enumerate() {
        match attr.id {
            sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID => {
                vr = Some(vr_vrf(store, arg(attr, index)?).ok_or(bad(index))?);
            }
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE => rif_type = Some(arg::<i32>(attr, index)?),
            sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID => port = arg(attr, index)?,
            sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID => vlan = arg(attr, index)?,
            sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS => mac = Some(MacAddr(arg(attr, index)?)),
            sys::SAI_ROUTER_INTERFACE_ATTR_MTU => mtu = arg(attr, index)?,
            sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE => v4 = arg(attr, index)?,
            sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V6_STATE => v6 = arg(attr, index)?,
            _ => keep_stub(RIF_STUBS, &mut kept, attr, index, true)?,
        }
    }
    let (Some((vr, vrf)), Some(rif_type)) = (vr, rif_type) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    let mac = match mac {
        Some(mac) => mac,
        None => vr_mac(store, store.get::<VirtualRouter>(vr)?)?,
    };
    let kind = match rif_type {
        RIF_PORT => {
            if port.is_null() {
                return Err(Error::MandatoryAttributeMissing);
            }
            // The CPU port, a LAG member, anything that is no port or LAG.
            let attach = Attach::decode(port)
                .filter(|_| crate::lag::attach_valid(store, port))
                .ok_or(Error::InvalidParameter)?;
            if has_port_rif(store, port) {
                return Err(Error::ItemAlreadyExists);
            }
            let vid = alloc_internal_vlan(store).ok_or(Error::InsufficientResources)?;
            backend.vlan_create(vid)?;
            let mut status = Ok(());
            for hw in crate::lag::attach_phys(store, port) {
                if status.is_ok() {
                    status = backend.vlan_port_add(vid, hw, true);
                }
            }
            if status.is_ok() {
                status = crate::lag::attach_pvid_set(backend, store, port, vid);
            }
            if let Err(e) = status {
                undo_port_vlan(backend, store, attach, vid);
                return Err(e);
            }
            RifKind::Port { port: attach, vid }
        }
        RIF_VLAN => {
            if vlan.is_null() {
                return Err(Error::MandatoryAttributeMissing);
            }
            let id = VlanId::try_from(vlan).map_err(|_| Error::InvalidParameter)?;
            let vid = store.get::<Vlan>(id).map_err(|_| Error::InvalidParameter)?.vid;
            let taken = store
                .iter::<RouterInterface>()
                .any(|(_, r)| matches!(r.kind, RifKind::Vlan { vlan: v, .. } if v == id));
            if taken {
                return Err(Error::ItemAlreadyExists);
            }
            RifKind::Vlan { vlan: id, vid }
        }
        RIF_LOOPBACK => RifKind::Loopback,
        other => {
            log::error(
                sys::SAI_API_ROUTER_INTERFACE,
                format_args!("router interface type {other} not supported"),
            );
            return Err(Error::NotSupported);
        }
    };
    let mut rif = RouterInterface {
        vr,
        vrf,
        kind,
        mac,
        mtu,
        v4,
        v6,
        intf: None,
        station: None,
        kept,
    };
    if let Some(vid) = rif.vid() {
        if let Err(e) = rif_program(backend, store, &mut rif, vid) {
            if let Some(station) = rif.station {
                let _ = backend.l3_station_delete(station);
            }
            if let RifKind::Port { port, vid } = rif.kind {
                undo_port_vlan(backend, store, port, vid);
            }
            return Err(e);
        }
    }
    let line = format!(
        "rif type {} vid {} intf {} vrf {}",
        rif.type_sai(),
        rif.vid().map_or(0, Vid::get),
        rif.intf.map_or(-1, |i| i64::from(i.0)),
        rif.vrf.0
    );
    let id = store.insert(rif)?;
    log::notice(sys::SAI_API_ROUTER_INTERFACE, format_args!("{line}"));
    Ok(id)
}

/// The SDK side of a RIF with an interface: the switch's and the RIF's my-station entries, the L3
/// interface, its admin state (`NotSupported`: kept only). What was made stays in `rif` for the
/// caller to take apart on an error (the L3 interface is destroyed here when its admin state fails).
fn rif_program<B: Backend>(backend: &mut B, store: &mut Store, rif: &mut RouterInterface, vid: Vid) -> Result<()> {
    ensure_switch_station(backend, store)?;
    if rif.mac != switch_mac(store)? {
        rif.station = Some(backend.l3_station_add(rif.mac)?);
    }
    let intf = backend.l3_intf_create(rif.mac, vid, rif.mtu, rif.vrf)?;
    if !rif.v4 || !rif.v6 {
        match backend.l3_intf_admin_set(intf, rif.v4, rif.v6) {
            Ok(()) | Err(Error::NotSupported) => {}
            Err(e) => {
                let _ = backend.l3_intf_destroy(intf);
                return Err(e);
            }
        }
    }
    rif.intf = Some(intf);
    Ok(())
}

fn remove_rif<B: Backend>(cx: Cx<'_, B>, id: RouterInterfaceId) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let rif = rif_of(store, id).map_err(|_| Error::InvalidObjectId)?;
    let in_use = store.l3.neighbors.keys().any(|(r, _)| *r == id)
        || store.iter::<NextHop>().any(|(_, n)| n.rif == id)
        || store.l3.routes.values().any(|r| r.next_hop == id.raw());
    if in_use {
        return Err(Error::ObjectInUse);
    }
    store.ensure_removable::<RouterInterface>(id)?;
    if let Some(intf) = rif.intf {
        backend.l3_intf_destroy(intf)?;
    }
    if let Some(station) = rif.station {
        let _ = backend.l3_station_delete(station);
    }
    if let RifKind::Port { port, vid } = rif.kind {
        undo_port_vlan(backend, store, port, vid);
    }
    store.remove::<RouterInterface>(id).map(|_| ())
}

fn set_rif<B: Backend>(cx: Cx<'_, B>, id: RouterInterfaceId, attr: &Attr) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let mut rif = rif_of(store, id)?;
    match attr.id {
        sys::SAI_ROUTER_INTERFACE_ATTR_MTU => {
            let mtu: u32 = arg(attr, 0)?;
            if let Some(intf) = rif.intf {
                let vid = rif.vid().ok_or(Error::Failure)?;
                backend.l3_intf_update(intf, rif.mac, vid, mtu, rif.vrf)?;
            }
            rif.mtu = mtu;
        }
        sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS => {
            let mac = MacAddr(arg(attr, 0)?);
            if let Some(intf) = rif.intf {
                let vid = rif.vid().ok_or(Error::Failure)?;
                let station = if mac == switch_mac(store)? {
                    None
                } else {
                    Some(backend.l3_station_add(mac)?)
                };
                if let Err(e) = backend.l3_intf_update(intf, mac, vid, rif.mtu, rif.vrf) {
                    if let Some(station) = station {
                        let _ = backend.l3_station_delete(station);
                    }
                    return Err(e);
                }
                if let Some(old) = rif.station {
                    let _ = backend.l3_station_delete(old);
                }
                rif.station = station;
            }
            rif.mac = mac;
        }
        sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE | sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V6_STATE => {
            let on: bool = arg(attr, 0)?;
            let (v4, v6) = if attr.id == sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE {
                (on, rif.v6)
            } else {
                (rif.v4, on)
            };
            if let Some(intf) = rif.intf {
                if (v4, v6) != (rif.v4, rif.v6) {
                    match backend.l3_intf_admin_set(intf, v4, v6) {
                        // NotSupported: kept only.
                        Ok(()) | Err(Error::NotSupported) => {}
                        Err(e) => return Err(attr_backend_status(e, 0)),
                    }
                }
            }
            rif.v4 = v4;
            rif.v6 = v6;
        }
        _ => keep_stub(RIF_STUBS, &mut rif.kept, attr, 0, false)?,
    }
    store.update::<RouterInterface, _>(id, |r| *r = rif)
}

fn get_rif(store: &Store, id: RouterInterfaceId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let rif = store.get::<RouterInterface>(id)?;
    Ok(match attr {
        sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID => Value::Oid(rif.vr.raw()),
        sys::SAI_ROUTER_INTERFACE_ATTR_TYPE => Value::Enum(rif.type_sai()),
        sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID => Value::Oid(match rif.kind {
            RifKind::Port { port, .. } => port.raw(),
            _ => RawOid::NULL,
        }),
        sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID => Value::Oid(match rif.kind {
            RifKind::Vlan { vlan, .. } => vlan.raw(),
            _ => RawOid::NULL,
        }),
        sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS => Value::Mac(rif.mac.0),
        sys::SAI_ROUTER_INTERFACE_ATTR_MTU => Value::U32(rif.mtu),
        sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE => Value::Bool(rif.v4),
        sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V6_STATE => Value::Bool(rif.v6),
        other => return stub_get(RIF_STUBS, &rif.kept, other),
    })
}

/// Router interface counters (C++ `rif_stats`): every id must be one the backend has, and the RIF
/// an L3 interface (a loopback RIF has none); `clear`: read and reset.
pub(crate) fn rif_stats<B: Backend>(
    backend: &mut B,
    store: &Store,
    id: RawOid,
    ids: &[sys::sai_stat_id_t],
    clear: bool,
) -> Result<Vec<u64>> {
    let rif = RouterInterfaceId::try_from(id)
        .ok()
        .and_then(|id| store.get::<RouterInterface>(id).ok())
        .ok_or(Error::InvalidObjectId)?;
    if ids.iter().any(|stat| !backend.l3_intf_stat_supported(*stat)) {
        return Err(Error::NotSupported);
    }
    let intf = rif.intf.ok_or(Error::NotSupported)?;
    backend.l3_intf_stats(intf, ids, clear)
}

/// The router interface counters the backend has (C++ `sai_query_stats_capability`, ids below
/// `0x200`).
pub(crate) fn rif_stat_capability<B: Backend>(backend: &B) -> Vec<sys::sai_stat_id_t> {
    (0..RIF_STAT_PROBE)
        .filter(|id| backend.l3_intf_stat_supported(*id))
        .collect()
}

// ------------------------------------------------------------------------------- next hops

fn create_next_hop<B: Backend>(cx: Cx<'_, B>, attrs: &[Attr]) -> Result<NextHopId> {
    let Cx { backend, store, .. } = cx;
    let mut typed = false;
    let mut ip = None;
    let mut rif = None;
    for (index, attr) in attrs.iter().enumerate() {
        match attr.id {
            sys::SAI_NEXT_HOP_ATTR_TYPE => {
                if arg::<i32>(attr, index)? != NEXT_HOP_IP {
                    return Err(unsupported(index));
                }
                typed = true;
            }
            sys::SAI_NEXT_HOP_ATTR_IP => ip = Some(arg::<IpAddr>(attr, index)?),
            sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID => rif = Some(arg::<RawOid>(attr, index)?),
            _ => return Err(unsupported(index)),
        }
    }
    let (true, Some(ip), Some(rif)) = (typed, ip, rif) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    let rif = RouterInterfaceId::try_from(rif)
        .ok()
        .filter(|id| store.get::<RouterInterface>(*id).is_ok())
        .ok_or(Error::InvalidParameter)?;
    let egress = if let Some(neighbor) = store.l3.neighbors.get(&(rif, ip)) {
        neighbor.egress
    } else {
        // Points at the CPU (glean) until the neighbor resolves it.
        log::info(
            sys::SAI_API_NEXT_HOP,
            format_args!("next hop without neighbor: points at the CPU (glean) until it resolves"),
        );
        backend.l3_egress_glean()?
    };
    store.insert(NextHop { ip, rif, egress })
}

/// A next hop's egress object (C++ `lookup(c.next_hops, o)->egress`, an ACL redirect to a next
/// hop); `None` for an id that is no next hop.
pub(crate) fn next_hop_egress(store: &Store, next_hop: RawOid) -> Option<EgressId> {
    let id = NextHopId::try_from(next_hop).ok()?;
    store.get::<NextHop>(id).ok().map(|n| n.egress)
}

fn remove_next_hop<B: Backend>(cx: Cx<'_, B>, id: NextHopId) -> Result<()> {
    let store = cx.store;
    store.get::<NextHop>(id).map_err(|_| Error::InvalidObjectId)?;
    let in_use = store.l3.routes.values().any(|r| r.next_hop == id.raw())
        || store.iter::<NextHopGroupMember>().any(|(_, m)| m.next_hop == id);
    if in_use {
        return Err(Error::ObjectInUse);
    }
    store.remove::<NextHop>(id).map(|_| ())
}

fn get_next_hop(store: &Store, id: NextHopId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let nh = store.get::<NextHop>(id)?;
    Ok(match attr {
        sys::SAI_NEXT_HOP_ATTR_TYPE => Value::Enum(NEXT_HOP_IP),
        sys::SAI_NEXT_HOP_ATTR_IP => Value::IpAddress(nh.ip),
        sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID => Value::Oid(nh.rif.raw()),
        _ => return Err(unsupported(0)),
    })
}

// --------------------------------------------------------------------------------- routes

/// What a route with this action and next hop points at (C++ `route_egress`).
fn route_egress<B: Backend>(
    backend: &mut B,
    store: &Store,
    action: RouteAction,
    next_hop: RawOid,
) -> Result<RouteTarget> {
    let egress = match action {
        RouteAction::Drop | RouteAction::Deny => backend.l3_egress_drop()?,
        RouteAction::Trap => backend.l3_egress_cpu()?,
        RouteAction::Forward => match next_hop.type_bits() {
            // Forward to nothing.
            sys::SAI_OBJECT_TYPE_NULL => backend.l3_egress_drop()?,
            sys::SAI_OBJECT_TYPE_NEXT_HOP => {
                let id = NextHopId::try_from(next_hop).map_err(|_| Error::InvalidParameter)?;
                store.get::<NextHop>(id).map_err(|_| Error::InvalidParameter)?.egress
            }
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP => {
                let id = NextHopGroupId::try_from(next_hop).map_err(|_| Error::InvalidParameter)?;
                match store.get::<NextHopGroup>(id).map_err(|_| Error::InvalidParameter)?.ecmp {
                    Some(ecmp) => return Ok(RouteTarget::Ecmp(ecmp)),
                    // No resolved member yet.
                    None => backend.l3_egress_drop()?,
                }
            }
            // A directly connected subnet: glean through the CPU.
            sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => {
                let id = RouterInterfaceId::try_from(next_hop).map_err(|_| Error::InvalidParameter)?;
                store.get::<RouterInterface>(id).map_err(|_| Error::InvalidParameter)?;
                backend.l3_egress_glean()?
            }
            // IP2ME: the next hop is the CPU port.
            sys::SAI_OBJECT_TYPE_PORT => {
                if switch(store)?.cpu_port.raw() != next_hop {
                    return Err(Error::InvalidParameter);
                }
                backend.l3_egress_cpu()?
            }
            _ => return Err(Error::InvalidParameter),
        },
    };
    Ok(RouteTarget::Egress(egress))
}

/// The target of a next hop or group changed: re-points the forwarding routes on it (C++
/// `reprogram_routes_using`; a failed route update is left as it was).
fn reprogram_routes_using<B: Backend>(backend: &mut B, store: &mut Store, object: RawOid) {
    let keys: Vec<(VirtualRouterId, IpPrefix)> = store
        .l3
        .routes
        .iter()
        .filter(|(_, r)| r.next_hop == object && r.action == RouteAction::Forward)
        .map(|(key, _)| *key)
        .collect();
    for key in keys {
        let Ok(target) = route_egress(backend, store, RouteAction::Forward, object) else {
            continue;
        };
        if store.l3.routes.get(&key).is_none_or(|r| r.target == target) {
            continue;
        }
        let Some((_, vrf)) = vr_vrf(store, key.0.raw()) else {
            continue;
        };
        match backend.l3_route_add(vrf, key.1, target, true) {
            Ok(()) => {
                if let Some(route) = store.l3.routes.get_mut(&key) {
                    route.target = target;
                }
            }
            Err(err) => log::error(
                sys::SAI_API_ROUTE,
                format_args!("re-pointing a route failed ({})", err.status()),
            ),
        }
    }
}

fn create_route<B: Backend>(cx: Cx<'_, B>, vr: RawOid, prefix: IpPrefix, attrs: &[Attr]) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let (vr, vrf) = vr_vrf(store, vr).ok_or(Error::InvalidParameter)?;
    if store.l3.routes.contains_key(&(vr, prefix)) {
        return Err(Error::ItemAlreadyExists);
    }
    let mut action = FORWARD;
    let mut next_hop = RawOid::NULL;
    let mut kept = Kept::new();
    for (index, attr) in attrs.iter().enumerate() {
        match attr.id {
            sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION => action = arg(attr, index)?,
            sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID => next_hop = arg(attr, index)?,
            _ => keep_stub(ROUTE_STUBS, &mut kept, attr, index, true)?,
        }
    }
    let action = RouteAction::from_sai(action).ok_or(Error::NotSupported)?;
    let target = route_egress(backend, store, action, next_hop)?;
    backend.l3_route_add(vrf, prefix, target, false)?;
    store.l3.routes.insert(
        (vr, prefix),
        Route {
            action,
            next_hop,
            target,
            kept,
        },
    );
    Ok(())
}

/// The route of an entry key (`ItemNotFound` for none, whatever the VR id is).
fn route_key(store: &Store, vr: RawOid, prefix: IpPrefix) -> Result<((VirtualRouterId, IpPrefix), Vrf)> {
    let (vr, vrf) = vr_vrf(store, vr).ok_or(Error::ItemNotFound)?;
    if store.l3.routes.contains_key(&(vr, prefix)) {
        Ok(((vr, prefix), vrf))
    } else {
        Err(Error::ItemNotFound)
    }
}

fn remove_route<B: Backend>(cx: Cx<'_, B>, vr: RawOid, prefix: IpPrefix) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let (key, vrf) = route_key(store, vr, prefix)?;
    match backend.l3_route_delete(vrf, prefix) {
        Ok(()) | Err(Error::ItemNotFound) => {}
        Err(e) => return Err(e),
    }
    store.l3.routes.remove(&key);
    Ok(())
}

fn set_route<B: Backend>(cx: Cx<'_, B>, vr: RawOid, prefix: IpPrefix, attr: &Attr) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let (key, vrf) = route_key(store, vr, prefix)?;
    let Some(mut route) = store.l3.routes.get(&key).cloned() else {
        return Err(Error::ItemNotFound);
    };
    let mut action = route.action.sai();
    match attr.id {
        sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION => action = arg(attr, 0)?,
        sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID => route.next_hop = arg(attr, 0)?,
        _ => {
            keep_stub(ROUTE_STUBS, &mut route.kept, attr, 0, true)?;
            store.l3.routes.insert(key, route);
            return Ok(());
        }
    }
    route.action = RouteAction::from_sai(action).ok_or(Error::NotSupported)?;
    route.target = route_egress(backend, store, route.action, route.next_hop)?;
    backend.l3_route_add(vrf, prefix, route.target, true)?;
    store.l3.routes.insert(key, route);
    Ok(())
}

fn get_route(store: &Store, vr: RawOid, prefix: IpPrefix, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
    let (key, _) = route_key(store, vr, prefix)?;
    let route = store.l3.routes.get(&key).ok_or(Error::ItemNotFound)?;
    ids.iter()
        .enumerate()
        .map(|(index, id)| match *id {
            sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION => Ok(Value::Enum(route.action.sai())),
            sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID => Ok(Value::Oid(route.next_hop)),
            other => stub_get(ROUTE_STUBS, &route.kept, other).map_err(|e| e.at_index(index)),
        })
        .collect()
}

// ------------------------------------------------------------------------------ neighbors

/// The neighbor of an entry key that exists.
fn neighbor_key(store: &Store, rif: RawOid, ip: IpAddr) -> Result<(RouterInterfaceId, IpAddr)> {
    let rif = RouterInterfaceId::try_from(rif).map_err(|_| Error::ItemNotFound)?;
    if store.l3.neighbors.contains_key(&(rif, ip)) {
        Ok((rif, ip))
    } else {
        Err(Error::ItemNotFound)
    }
}

/// A neighbor's egress object and where it points (C++ `neighbor_egress`): drop / CPU for those
/// actions; else to the port or LAG of a port RIF, or for a VLAN RIF to where the FDB entry of the
/// neighbor's MAC points; the CPU while that is not known.
fn egress_for_neighbor<B: Backend>(
    backend: &mut B,
    store: &Store,
    rif: &RouterInterface,
    mac: MacAddr,
    action: i32,
) -> Result<(EgressId, Option<Dest>)> {
    let (Some(intf), Some(vid)) = (rif.intf, rif.vid()) else {
        return Err(Error::InvalidParameter);
    };
    match action {
        DROP => return Ok((backend.l3_egress_drop()?, None)),
        TRAP => return Ok((backend.l3_egress_cpu()?, None)),
        _ => {}
    }
    let dest = match rif.kind {
        RifKind::Port { port, .. } => crate::lag::attach_dest(store, port.raw()),
        RifKind::Vlan { vlan, .. } => {
            crate::l2::fdb_bridge_port(store, vlan, mac.0).and_then(|bport| crate::l2::bridge_port_dest(store, bport))
        }
        RifKind::Loopback => None,
    };
    if let Some(dest) = dest {
        Ok((backend.l3_egress_create(intf, mac, dest, vid)?, Some(dest)))
    } else {
        // The egress port is unknown (no FDB entry yet): to the CPU.
        log::warn(
            sys::SAI_API_NEIGHBOR,
            format_args!("neighbor {mac}: egress port unknown (no FDB entry yet); sending to CPU"),
        );
        Ok((backend.l3_egress_cpu()?, None))
    }
}

fn create_neighbor<B: Backend>(cx: Cx<'_, B>, rif: RawOid, ip: IpAddr, attrs: &[Attr]) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let rif_id = RouterInterfaceId::try_from(rif).ok();
    if rif_id.is_some_and(|r| store.l3.neighbors.contains_key(&(r, ip))) {
        return Err(Error::ItemAlreadyExists);
    }
    let (rif_id, rif) = rif_id
        .and_then(|id| rif_of(store, id).ok().map(|r| (id, r)))
        .ok_or(Error::InvalidParameter)?;
    if rif.kind == RifKind::Loopback {
        return Err(Error::NotSupported);
    }
    let mut mac = None;
    let mut action = FORWARD;
    let mut no_host = false;
    for (index, attr) in attrs.iter().enumerate() {
        match attr.id {
            sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS => mac = Some(MacAddr(arg(attr, index)?)),
            sys::SAI_NEIGHBOR_ENTRY_ATTR_PACKET_ACTION => {
                action = arg(attr, index)?;
                if ![FORWARD, DROP, TRAP].contains(&action) {
                    return Err(unsupported(index));
                }
            }
            sys::SAI_NEIGHBOR_ENTRY_ATTR_NO_HOST_ROUTE => no_host = arg(attr, index)?,
            _ => return Err(unsupported(index)),
        }
    }
    let mac = mac.ok_or(Error::MandatoryAttributeMissing)?;
    let (egress, dest) = egress_for_neighbor(backend, store, &rif, mac, action)?;
    if !no_host {
        if let Err(e) = backend.l3_host_add(rif.vrf, ip, egress) {
            if !egress_shared(backend, egress) {
                let _ = backend.l3_egress_destroy(egress);
            }
            return Err(e);
        }
    }
    store.l3.neighbors.insert(
        (rif_id, ip),
        Neighbor {
            mac,
            no_host,
            egress,
            dest,
            host: !no_host,
            vrf: rif.vrf,
        },
    );
    // Next hops created before this neighbor pointed at the CPU; their routes and next-hop
    // groups follow.
    let next_hops: Vec<NextHopId> = store
        .iter::<NextHop>()
        .filter(|(_, n)| n.rif == rif_id && n.ip == ip)
        .map(|(id, _)| id)
        .collect();
    for next_hop in next_hops {
        store.update::<NextHop, _>(next_hop, |n| n.egress = egress)?;
        reprogram_routes_using(backend, store, next_hop.raw());
        let groups: Vec<NextHopGroupId> = store
            .iter::<NextHopGroupMember>()
            .filter(|(_, m)| m.next_hop == next_hop)
            .map(|(_, m)| m.group)
            .collect();
        for group in groups {
            // As the C++: a group that cannot follow stays as it was, logged.
            if group_sync(backend, store, group).is_err() {
                log::error(
                    sys::SAI_API_NEXT_HOP_GROUP,
                    format_args!("next-hop group update after neighbor resolution failed"),
                );
            }
        }
    }
    Ok(())
}

fn remove_neighbor<B: Backend>(cx: Cx<'_, B>, rif: RawOid, ip: IpAddr) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let key = neighbor_key(store, rif, ip)?;
    let neighbor = store.l3.neighbors.get(&key).cloned().ok_or(Error::ItemNotFound)?;
    if store.iter::<NextHop>().any(|(_, n)| n.rif == key.0 && n.ip == ip) {
        return Err(Error::ObjectInUse);
    }
    if neighbor.host {
        match backend.l3_host_delete(neighbor.vrf, ip) {
            Ok(()) | Err(Error::ItemNotFound) => {}
            Err(e) => return Err(e),
        }
    }
    if !egress_shared(backend, neighbor.egress) {
        let _ = backend.l3_egress_destroy(neighbor.egress);
    }
    store.l3.neighbors.remove(&key);
    Ok(())
}

fn set_neighbor<B: Backend>(cx: Cx<'_, B>, rif: RawOid, ip: IpAddr, attr: &Attr) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let key = neighbor_key(store, rif, ip)?;
    let neighbor = store.l3.neighbors.get(&key).cloned().ok_or(Error::ItemNotFound)?;
    if attr.id != sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS {
        return Err(unsupported(0));
    }
    let rif = rif_of(store, key.0).map_err(|_| Error::Failure)?;
    // A neighbor on a shared egress object (drop, CPU) has no MAC of its own in the chip.
    let shared = egress_shared(backend, neighbor.egress);
    let Some(dest) = neighbor.dest.filter(|_| !shared) else {
        return Err(unsupported(0));
    };
    let mac = MacAddr(arg(attr, 0)?);
    let (Some(intf), Some(vid)) = (rif.intf, rif.vid()) else {
        return Err(Error::Failure);
    };
    backend.l3_egress_update(neighbor.egress, intf, mac, dest, vid)?;
    if let Some(n) = store.l3.neighbors.get_mut(&key) {
        n.mac = mac;
    }
    Ok(())
}

fn get_neighbor(store: &Store, rif: RawOid, ip: IpAddr, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
    let key = neighbor_key(store, rif, ip)?;
    let neighbor = store.l3.neighbors.get(&key).ok_or(Error::ItemNotFound)?;
    ids.iter()
        .enumerate()
        .map(|(index, id)| match *id {
            sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS => Ok(Value::Mac(neighbor.mac.0)),
            sys::SAI_NEIGHBOR_ENTRY_ATTR_NO_HOST_ROUTE => Ok(Value::Bool(neighbor.no_host)),
            _ => Err(unsupported(index)),
        })
        .collect()
}

// ------------------------------------------------------------------------- next-hop groups

/// Brings a group's SDK ECMP group in line with its members (C++ `nhg_sync`): created with the
/// first resolved member, updated, destroyed when none is left; routes are re-pointed after a
/// create and before a destroy. Unresolved members (on a shared egress) are left out; a member
/// counts `WEIGHT` times; the list is cut at the chip's paths per group.
fn group_sync<B: Backend>(backend: &mut B, store: &mut Store, id: NextHopGroupId) -> Result<()> {
    let group = store
        .get::<NextHopGroup>(id)
        .map_err(|_| Error::InvalidObjectId)?
        .clone();
    let paths = backend
        .chip_limits()
        .ok()
        .and_then(|l| usize::try_from(l.ecmp_paths).ok())
        .filter(|p| *p > 0)
        .unwrap_or(ECMP_PATHS_UNKNOWN);
    let mut egress = Vec::new();
    // every path asked for (members x weight), also those past the chip's limit
    let mut asked: u64 = 0;
    for member in &group.members {
        let Ok(member) = store.get::<NextHopGroupMember>(*member) else {
            continue;
        };
        let Ok(next_hop) = store.get::<NextHop>(member.next_hop) else {
            continue;
        };
        let (weight, nh_egress) = (member.weight, next_hop.egress);
        if egress_shared(backend, nh_egress) {
            continue; // unresolved: not in the group
        }
        asked = asked.saturating_add(u64::from(weight));
        for _ in 0..weight {
            if egress.len() >= paths {
                break; // more paths (members x weight) than the chip takes: the rest is left out
            }
            egress.push(nh_egress);
        }
    }
    if usize::try_from(asked).is_ok_and(|asked| asked > paths) && paths != ECMP_PATHS_UNKNOWN {
        log::warn(
            sys::SAI_API_NEXT_HOP_GROUP,
            format_args!(
                "next-hop group: {asked} paths (members x weight), the chip takes {paths}; the rest is left out"
            ),
        );
    }
    if egress == group.programmed && group.ecmp.is_some() != egress.is_empty() {
        return Ok(());
    }
    match group.ecmp {
        Some(old) if egress.is_empty() => {
            store.update::<NextHopGroup, _>(id, |g| {
                g.ecmp = None;
                g.programmed.clear();
            })?;
            reprogram_routes_using(backend, store, id.raw()); // -> drop
            // As the C++: a failed destroy is only logged.
            if let Err(err) = backend.l3_ecmp_destroy(old) {
                log::error(
                    sys::SAI_API_NEXT_HOP_GROUP,
                    format_args!("ECMP group {} destroy failed ({})", old.0, err.status()),
                );
            }
            Ok(())
        }
        Some(ecmp) => {
            backend.l3_ecmp_update(ecmp, &egress)?;
            store.update::<NextHopGroup, _>(id, |g| g.programmed = egress)
        }
        None => {
            let ecmp = backend.l3_ecmp_create(&egress)?;
            store.update::<NextHopGroup, _>(id, |g| {
                g.ecmp = Some(ecmp);
                g.programmed = egress;
            })?;
            reprogram_routes_using(backend, store, id.raw());
            Ok(())
        }
    }
}

fn create_group(store: &mut Store, attrs: &[Attr]) -> Result<NextHopGroupId> {
    let mut typed = false;
    for (index, attr) in attrs.iter().enumerate() {
        if attr.id != sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE || arg::<i32>(attr, index)? != GROUP_ECMP {
            return Err(unsupported(index));
        }
        typed = true;
    }
    if !typed {
        return Err(Error::MandatoryAttributeMissing);
    }
    store.insert(NextHopGroup::default())
}

fn remove_group<B: Backend>(cx: Cx<'_, B>, id: NextHopGroupId) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let group = store
        .get::<NextHopGroup>(id)
        .map_err(|_| Error::InvalidObjectId)?
        .clone();
    if !group.members.is_empty() || store.l3.routes.values().any(|r| r.next_hop == id.raw()) {
        return Err(Error::ObjectInUse);
    }
    // Not reached: without members there is no SDK group.
    if let Some(ecmp) = group.ecmp {
        backend.l3_ecmp_destroy(ecmp)?;
    }
    store.remove::<NextHopGroup>(id).map(|_| ())
}

fn set_group(attr: &Attr) -> Result<()> {
    if attr.id == sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE {
        Err(bad(0)) // create only
    } else {
        Err(unsupported(0))
    }
}

fn get_group(store: &Store, id: NextHopGroupId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let group = store.get::<NextHopGroup>(id)?;
    Ok(match attr {
        sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE => Value::Enum(GROUP_ECMP),
        sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_COUNT => {
            Value::U32(u32::try_from(group.members.len()).map_err(|_| Error::Failure)?)
        }
        sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_MEMBER_LIST => {
            Value::OidList(group.members.iter().map(|m| m.raw()).collect())
        }
        _ => return Err(unsupported(0)),
    })
}

fn create_member<B: Backend>(cx: Cx<'_, B>, attrs: &[Attr]) -> Result<NextHopGroupMemberId> {
    let Cx { backend, store, .. } = cx;
    let mut group = None;
    let mut next_hop = None;
    let mut weight = 1;
    let mut kept = Kept::new();
    for (index, attr) in attrs.iter().enumerate() {
        match attr.id {
            sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID => {
                let id = NextHopGroupId::try_from(arg::<RawOid>(attr, index)?).map_err(|_| bad(index))?;
                store.get::<NextHopGroup>(id).map_err(|_| bad(index))?;
                group = Some(id);
            }
            sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID => {
                let id = NextHopId::try_from(arg::<RawOid>(attr, index)?).map_err(|_| bad(index))?;
                store.get::<NextHop>(id).map_err(|_| bad(index))?;
                next_hop = Some(id);
            }
            sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT => {
                weight = arg(attr, index)?;
                if weight == 0 {
                    return Err(bad(index));
                }
            }
            _ => keep_stub(MEMBER_STUBS, &mut kept, attr, index, true)?,
        }
    }
    let (Some(group), Some(next_hop)) = (group, next_hop) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    // As the C++: the member joins the model, the group follows, and on an error both are put
    // back (the member's id stays used).
    let id = store.insert(NextHopGroupMember {
        group,
        next_hop,
        weight,
        kept,
    })?;
    store.update::<NextHopGroup, _>(group, |g| g.members.push(id))?;
    if let Err(e) = group_sync(backend, store, group) {
        let _ = store.update::<NextHopGroup, _>(group, |g| g.members.retain(|m| *m != id));
        let _ = store.remove::<NextHopGroupMember>(id);
        return Err(e);
    }
    Ok(id)
}

fn remove_member<B: Backend>(cx: Cx<'_, B>, id: NextHopGroupMemberId) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let member = store
        .get::<NextHopGroupMember>(id)
        .map_err(|_| Error::InvalidObjectId)?
        .clone();
    let group = member.group;
    let position = store.update::<NextHopGroup, _>(group, |g| {
        let position = g.members.iter().position(|m| *m == id);
        if let Some(p) = position {
            g.members.remove(p);
        }
        position
    })?;
    if let Err(e) = group_sync(backend, store, group) {
        // The SDK group is unchanged: the member stays where it was.
        if let Some(p) = position {
            let _ = store.update::<NextHopGroup, _>(group, |g| g.members.insert(p.min(g.members.len()), id));
        }
        return Err(e);
    }
    store.remove::<NextHopGroupMember>(id).map(|_| ())
}

fn set_member<B: Backend>(cx: Cx<'_, B>, id: NextHopGroupMemberId, attr: &Attr) -> Result<()> {
    let Cx { backend, store, .. } = cx;
    let old = store.get::<NextHopGroupMember>(id)?.clone();
    let mut member = old.clone();
    match attr.id {
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID => {
            let next_hop = NextHopId::try_from(arg::<RawOid>(attr, 0)?).map_err(|_| bad(0))?;
            store.get::<NextHop>(next_hop).map_err(|_| bad(0))?;
            member.next_hop = next_hop;
        }
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT => {
            member.weight = arg(attr, 0)?;
            if member.weight == 0 {
                return Err(bad(0));
            }
        }
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID => return Err(bad(0)), // create only
        _ => {
            keep_stub(MEMBER_STUBS, &mut member.kept, attr, 0, true)?;
            return store.update::<NextHopGroupMember, _>(id, |m| *m = member);
        }
    }
    store.update::<NextHopGroupMember, _>(id, |m| *m = member)?;
    if let Err(e) = group_sync(backend, store, old.group) {
        let _ = store.update::<NextHopGroupMember, _>(id, |m| *m = old);
        return Err(e);
    }
    Ok(())
}

fn get_member(store: &Store, id: NextHopGroupMemberId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let member = store.get::<NextHopGroupMember>(id)?;
    Ok(match attr {
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID => Value::Oid(member.group.raw()),
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID => Value::Oid(member.next_hop.raw()),
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT => Value::U32(member.weight),
        other => return stub_get(MEMBER_STUBS, &member.kept, other),
    })
}

// -------------------------------------------------------------------------------- capacity

/// The switch's `AVAILABLE_*` route, next hop, neighbor, next-hop group (member) and FDB entry
/// counts: the backend's free table entries (C++ `get_capacity`). `None` for another attribute.
pub(crate) fn available<B: Backend>(backend: &B, attr: sys::sai_attr_id_t) -> Option<Result<Value>> {
    let pick: fn(&crate::backend::types::TableUsage) -> u32 = match attr {
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY => |u| u.v4_routes_free,
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY => |u| u.v6_routes_free,
        // A next hop uses its neighbor's egress object.
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY => {
            |u| u.egress_free
        }
        // A neighbor takes a host entry (IPv6: two) and an egress object.
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY => |u| u.hosts_free.min(u.egress_free),
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY => |u| (u.hosts_free / 2).min(u.egress_free),
        // An empty (or unresolved) next-hop group holds no SDK group.
        sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY => |u| u.ecmp_groups_free,
        sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY => |u| u.ecmp_members_free,
        sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY => |u| u.fdb_free,
        _ => return None,
    };
    Some(
        backend
            .table_usage()
            .map(|u| Value::U32(pick(&u)))
            .map_err(|e| attr_backend_status(e, 0)),
    )
}
