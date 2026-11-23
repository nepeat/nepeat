//! L2: VLANs, VLAN members, the default .1Q bridge, bridge ports on a port or a LAG, FDB
//! entries programmed into the L2 table, FDB flush with every filter combination, learn / age
//! events turned into SAI FDB events, STP (the default instance only, read-only). Ports
//! `sai_l2.cpp` (809 lines) and the FDB part of `sai_switch.cpp` (`deliver_fdb`: a learn of an
//! address known on another bridge port is a SAI `MOVE`).
//!
//! - VLAN ids are their VID (C++ layout: index = VID). VLAN 1 is the switch's and cannot be
//!   removed; a VID in use as a router interface's internal VLAN cannot be created.
//! - A VLAN member, a bridge port's learning mode and an FDB entry on a LAG apply to every member
//!   port of the LAG, and to a port when it joins ([`bridge_port_member_join`], called by
//!   [`crate::lag`]); leaving undoes them ([`bridge_port_member_leave`]).
//! - Removing a bridge port flushes its dynamic entries (static ones keep it in use); removing it
//!   does not touch the ports' learning.
//! - Attributes the C++ keeps without effect ("stubs") are kept and answer the C++ default.
//! - Where the C++ does not undo the backend calls that worked before one failed (VLAN member
//!   remove / tagging change, bridge port learning on a LAG, LAG PVID), neither does this: the
//!   model stays as it was, the chip has the calls that worked (listed for the differential).
//!
//! Backend: [`crate::backend::L2`], the VLAN operations of [`crate::backend::Backend`],
//! [`crate::backend::PortOps::port_learn_set`]; [`crate::hash::vlan_flood_apply`] for the VLAN
//! flood-control attributes; events [`crate::backend::BackendEvent::Fdb`].
//!
//! Registration: [`crate::dispatch::Owner::L2`] for `VLAN`, `VLAN_MEMBER`, `BRIDGE`, `BRIDGE_PORT`,
//! `STP` and the `FDB_ENTRY` entry type (with `flush_fdb_entries`); FDB events arrive in
//! [`fdb_event`].
use crate::attrs::{self, CreateArgs};
use crate::backend::types::{Dest, FdbChange, FdbEvent, Learn, MacAddr};
use crate::backend::{Backend, HwPort, Vid};
use crate::dispatch::{Area, Cx};
use crate::error::{AttrError, Error, Result};
use crate::lag::{self, Attach};
use crate::model::{Bridge, BridgePort, EntryKey, Kept, Switch, Vlan, VlanMember};
use crate::oid::{BridgeId, BridgePortId, LagId, ObjectType, RawOid, StpId, SwitchId, TypedId, VlanId, VlanMemberId};
use crate::sai::Notification;
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

/// An attribute kept without effect and the value a get answers while it is unset (the C++
/// `StubDef`).
pub(crate) type Stub = (sys::sai_attr_id_t, Value);

/// Keeps a stub attribute (C++ `stub_set` / `stub_store`): `ATTR_NOT_SUPPORTED` at `index` for one
/// that is not in `stubs`.
pub(crate) fn stub_set(stubs: &[Stub], kept: &mut Kept, attr: &Attr, index: usize) -> Result<()> {
    if !stubs.iter().any(|(id, _)| *id == attr.id) {
        return Err(Error::attr(AttrError::NotSupported, index));
    }
    kept.insert(attr.id, attr.value.clone());
    Ok(())
}

/// A stub attribute's value: what was set, else its default (C++ `stub_get` / `stub_load`).
pub(crate) fn stub_get(stubs: &[Stub], kept: &Kept, id: sys::sai_attr_id_t) -> Result<Value> {
    let (_, default) = stubs
        .iter()
        .find(|(stub, _)| *stub == id)
        .ok_or(Error::attr(AttrError::NotSupported, 0))?;
    Ok(kept.get(&id).unwrap_or(default).clone())
}

/// A backend failure while handling the attribute at `index`: the generic codes get the
/// attribute's index (C++ `attr_backend_status`).
pub(crate) fn attr_backend_status(error: Error, index: usize) -> Error {
    match error {
        Error::InvalidParameter => Error::attr(AttrError::InvalidValue, index),
        Error::NotSupported => Error::attr(AttrError::NotSupported, index),
        Error::NotImplemented => Error::attr(AttrError::NotImplemented, index),
        other => other,
    }
}

fn enumerator(value: u32) -> Value {
    Value::Enum(value.cast_signed())
}

const VLAN_STUBS: &[Stub] = &[
    (sys::SAI_VLAN_ATTR_LEARN_DISABLE, Value::Bool(false)),
    (sys::SAI_VLAN_ATTR_MAX_LEARNED_ADDRESSES, Value::U32(0)),
    (
        sys::SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE,
        Value::Enum(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL.cast_signed()),
    ),
    (
        sys::SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE,
        Value::Enum(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL.cast_signed()),
    ),
    (
        sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE,
        Value::Enum(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL.cast_signed()),
    ),
    (
        sys::SAI_VLAN_ATTR_IPV4_MCAST_LOOKUP_KEY_TYPE,
        Value::Enum(sys::SAI_VLAN_MCAST_LOOKUP_KEY_TYPE_MAC_DA.cast_signed()),
    ),
    (
        sys::SAI_VLAN_ATTR_IPV6_MCAST_LOOKUP_KEY_TYPE,
        Value::Enum(sys::SAI_VLAN_MCAST_LOOKUP_KEY_TYPE_MAC_DA.cast_signed()),
    ),
    (sys::SAI_VLAN_ATTR_META_DATA, Value::U32(0)),
    (sys::SAI_VLAN_ATTR_CUSTOM_IGMP_SNOOPING_ENABLE, Value::Bool(false)),
];
const BRIDGE_STUBS: &[Stub] = &[
    (sys::SAI_BRIDGE_ATTR_MAX_LEARNED_ADDRESSES, Value::U32(0)),
    (sys::SAI_BRIDGE_ATTR_LEARN_DISABLE, Value::Bool(false)),
    (
        sys::SAI_BRIDGE_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE,
        Value::Enum(sys::SAI_BRIDGE_FLOOD_CONTROL_TYPE_SUB_PORTS.cast_signed()),
    ),
    (
        sys::SAI_BRIDGE_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE,
        Value::Enum(sys::SAI_BRIDGE_FLOOD_CONTROL_TYPE_SUB_PORTS.cast_signed()),
    ),
    (
        sys::SAI_BRIDGE_ATTR_BROADCAST_FLOOD_CONTROL_TYPE,
        Value::Enum(sys::SAI_BRIDGE_FLOOD_CONTROL_TYPE_SUB_PORTS.cast_signed()),
    ),
];
const BRIDGE_PORT_STUBS: &[Stub] = &[
    (sys::SAI_BRIDGE_PORT_ATTR_MAX_LEARNED_ADDRESSES, Value::U32(0)),
    (
        sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_LIMIT_VIOLATION_PACKET_ACTION,
        Value::Enum(sys::SAI_PACKET_ACTION_DROP.cast_signed()),
    ),
    (sys::SAI_BRIDGE_PORT_ATTR_INGRESS_FILTERING, Value::Bool(false)),
    (sys::SAI_BRIDGE_PORT_ATTR_EGRESS_FILTERING, Value::Bool(false)),
    (
        sys::SAI_BRIDGE_PORT_ATTR_TAGGING_MODE,
        Value::Enum(sys::SAI_BRIDGE_PORT_TAGGING_MODE_TAGGED.cast_signed()),
    ),
];
const FDB_STUBS: &[Stub] = &[
    (sys::SAI_FDB_ENTRY_ATTR_META_DATA, Value::U32(0)),
    (sys::SAI_FDB_ENTRY_ATTR_ALLOW_MAC_MOVE, Value::Bool(false)),
];

// ------------------------------------------------------------------------- types

/// How a VLAN member leaves the chip: `SAI_VLAN_TAGGING_MODE_UNTAGGED` / `_TAGGED` (priority
/// tagging is not supported, as in the C++).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tagging {
    /// Untagged (the default).
    #[default]
    Untagged,
    /// Tagged.
    Tagged,
}

impl Tagging {
    /// The mode of a `sai_vlan_tagging_mode_t`; `None` for priority tagging or another number.
    pub fn from_sai(mode: i32) -> Option<Self> {
        match u32::try_from(mode).ok()? {
            sys::SAI_VLAN_TAGGING_MODE_UNTAGGED => Some(Self::Untagged),
            sys::SAI_VLAN_TAGGING_MODE_TAGGED => Some(Self::Tagged),
            _ => None,
        }
    }

    /// The `sai_vlan_tagging_mode_t`.
    pub const fn sai(self) -> i32 {
        match self {
            Self::Untagged => sys::SAI_VLAN_TAGGING_MODE_UNTAGGED.cast_signed(),
            Self::Tagged => sys::SAI_VLAN_TAGGING_MODE_TAGGED.cast_signed(),
        }
    }

    /// Whether the chip sends without a tag (the backend's `untagged` argument).
    pub const fn untagged(self) -> bool {
        matches!(self, Self::Untagged)
    }
}

/// A bridge port's `FDB_LEARNING_MODE` (`sai_bridge_port_fdb_learning_mode_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FdbLearning {
    /// `DROP`
    Drop,
    /// `DISABLE`
    Disable,
    /// `HW` (the default)
    #[default]
    Hw,
    /// `CPU_TRAP`
    CpuTrap,
    /// `CPU_LOG`
    CpuLog,
    /// `FDB_NOTIFICATION`
    FdbNotification,
}

impl FdbLearning {
    const ALL: [(Self, u32); 6] = [
        (Self::Drop, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DROP),
        (Self::Disable, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE),
        (Self::Hw, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_HW),
        (Self::CpuTrap, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_CPU_TRAP),
        (Self::CpuLog, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_CPU_LOG),
        (
            Self::FdbNotification,
            sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_FDB_NOTIFICATION,
        ),
    ];

    /// The mode of a `sai_bridge_port_fdb_learning_mode_t`.
    pub fn from_sai(mode: i32) -> Option<Self> {
        let mode = u32::try_from(mode).ok()?;
        Self::ALL.iter().find(|(_, sai)| *sai == mode).map(|(m, _)| *m)
    }

    /// The `sai_bridge_port_fdb_learning_mode_t`.
    pub fn sai(self) -> i32 {
        Self::ALL
            .iter()
            .find(|(m, _)| *m == self)
            .map_or(0, |(_, sai)| sai.cast_signed())
    }

    /// What the chip's ports are told (C++ `to_learn`): the CPU modes trap, the notification
    /// mode learns in hardware.
    pub const fn learn(self) -> Learn {
        match self {
            Self::Disable => Learn::Disable,
            Self::Drop => Learn::Drop,
            Self::CpuTrap | Self::CpuLog => Learn::Cpu,
            Self::Hw | Self::FdbNotification => Learn::Hw,
        }
    }
}

/// `SAI_FDB_ENTRY_TYPE_*`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FdbType {
    /// Learned, or created dynamic: flushed and aged.
    #[default]
    Dynamic,
    /// Static.
    Static,
}

/// The packet actions an FDB entry can have here (`FORWARD`, `DROP`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FdbAction {
    /// `SAI_PACKET_ACTION_FORWARD` (the default).
    #[default]
    Forward,
    /// `SAI_PACKET_ACTION_DROP`: a discard entry.
    Drop,
}

/// What an FDB notification reports (`sai_fdb_event_t`; this library never sends `FLUSHED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FdbEventKind {
    /// `SAI_FDB_EVENT_LEARNED`
    Learned,
    /// `SAI_FDB_EVENT_AGED`
    Aged,
    /// `SAI_FDB_EVENT_MOVE`: learned on another bridge port than the one known.
    Move,
}

impl FdbEventKind {
    /// The `sai_fdb_event_t`.
    pub const fn sai(self) -> sys::sai_fdb_event_t {
        match self {
            Self::Learned => sys::SAI_FDB_EVENT_LEARNED,
            Self::Aged => sys::SAI_FDB_EVENT_AGED,
            Self::Move => sys::SAI_FDB_EVENT_MOVE,
        }
    }
}

/// An FDB entry (stored under its key as attributes: `TYPE`, `PACKET_ACTION`, `BRIDGE_PORT_ID` when
/// there is one, and its stubs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fdb {
    /// `TYPE`
    pub kind: FdbType,
    /// `BRIDGE_PORT_ID` (none: a discard entry may have none).
    pub bridge_port: Option<BridgePortId>,
    /// `PACKET_ACTION`
    pub action: FdbAction,
    /// Kept attributes.
    pub kept: Kept,
}

impl Fdb {
    /// The entry an FDB key's stored attributes describe.
    pub fn load(stored: &Kept) -> Result<Self> {
        let enum_of = |id| match stored.get(&id) {
            Some(Value::Enum(v)) => u32::try_from(*v).ok(),
            _ => None,
        };
        let kind = match enum_of(sys::SAI_FDB_ENTRY_ATTR_TYPE) {
            Some(sys::SAI_FDB_ENTRY_TYPE_STATIC) => FdbType::Static,
            Some(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC) => FdbType::Dynamic,
            _ => return Err(Error::Failure),
        };
        let action = match enum_of(sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION) {
            Some(sys::SAI_PACKET_ACTION_DROP) => FdbAction::Drop,
            Some(sys::SAI_PACKET_ACTION_FORWARD) => FdbAction::Forward,
            _ => return Err(Error::Failure),
        };
        let bridge_port = match stored.get(&sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID) {
            Some(Value::Oid(bp)) => Some(BridgePortId::try_from(*bp)?),
            _ => None,
        };
        let kept = FDB_STUBS
            .iter()
            .filter_map(|(id, _)| stored.get(id).map(|v| (*id, v.clone())))
            .collect();
        Ok(Self {
            kind,
            bridge_port,
            action,
            kept,
        })
    }

    /// The attributes the store keeps for the entry (an object id in them is a reference).
    pub fn to_kept(&self) -> Kept {
        let mut kept = self.kept.clone();
        kept.insert(
            sys::SAI_FDB_ENTRY_ATTR_TYPE,
            enumerator(match self.kind {
                FdbType::Static => sys::SAI_FDB_ENTRY_TYPE_STATIC,
                FdbType::Dynamic => sys::SAI_FDB_ENTRY_TYPE_DYNAMIC,
            }),
        );
        kept.insert(
            sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION,
            enumerator(match self.action {
                FdbAction::Forward => sys::SAI_PACKET_ACTION_FORWARD,
                FdbAction::Drop => sys::SAI_PACKET_ACTION_DROP,
            }),
        );
        if let Some(bp) = self.bridge_port {
            kept.insert(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(bp.raw()));
        }
        kept
    }
}

// ------------------------------------------------------------------------ lookups

fn the_switch(store: &Store) -> Result<(SwitchId, &Switch)> {
    store.iter::<Switch>().next().ok_or(Error::Uninitialized)
}

/// The default STP instance (the only one).
fn default_stp(store: &Store) -> RawOid {
    the_switch(store)
        .ok()
        .and_then(|(_, s)| s.objects.get(&sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID).copied())
        .unwrap_or(RawOid::NULL)
}

/// The VLAN of a raw id, if it is one that exists (C++ `vlan_of`).
pub(crate) fn vlan_of(store: &Store, raw: RawOid) -> Option<(VlanId, &Vlan)> {
    let id = VlanId::try_from(raw).ok()?;
    store.get::<Vlan>(id).ok().map(|v| (id, v))
}

/// The SAI VLAN with this VID (C++ `vid2vlan`).
pub(crate) fn vlan_of_vid(store: &Store, vid: Vid) -> Option<VlanId> {
    store.iter::<Vlan>().find(|(_, v)| v.vid == vid).map(|(id, _)| id)
}

/// Where a bridge port's traffic goes: its port or its LAG's trunk (C++ `bridge_port_dest`);
/// none for an id that names no bridge port.
pub(crate) fn bridge_port_dest(store: &Store, bridge_port: RawOid) -> Option<Dest> {
    dest_of(store, BridgePortId::try_from(bridge_port).ok()?)
}

/// [`bridge_port_dest`] of a typed id.
fn dest_of(store: &Store, bridge_port: BridgePortId) -> Option<Dest> {
    lag::attach_dest(store, store.get::<BridgePort>(bridge_port).ok()?.attach.raw())
}

/// The chip ports of a bridge port: its port, or its LAG's members (C++ `bridge_port_phys`).
pub(crate) fn bridge_port_phys(store: &Store, bridge_port: BridgePortId) -> Vec<HwPort> {
    store
        .get::<BridgePort>(bridge_port)
        .map(|b| lag::attach_phys(store, b.attach.raw()))
        .unwrap_or_default()
}

/// The bridge port on a port or LAG (C++ `bridge_port_of`).
pub(crate) fn bridge_port_of(store: &Store, attach: Attach) -> Option<BridgePortId> {
    store
        .iter::<BridgePort>()
        .find(|(_, b)| b.attach == attach)
        .map(|(id, _)| id)
}

/// The bridge port of a chip destination, a port or a trunk (C++ `bridge_port_of_dest`).
pub(crate) fn bridge_port_of_dest(store: &Store, dest: Dest) -> Option<BridgePortId> {
    match dest {
        Dest::Lag(tid) => {
            let (lag, _) = store.iter::<lag::Lag>().find(|(_, l)| l.tid == tid)?;
            bridge_port_of(store, Attach::Lag(lag))
        }
        Dest::Port(hw) => {
            let (port, _) = store.iter::<crate::model::Port>().find(|(_, p)| p.hw == hw)?;
            bridge_port_of(store, Attach::Port(port))
        }
    }
}

/// The bridge port of the FDB entry of `mac` in `vlan`, if there is one with a bridge port (L3:
/// where a VLAN router interface's neighbor is).
pub(crate) fn fdb_bridge_port(store: &Store, vlan: VlanId, mac: [u8; 6]) -> Option<RawOid> {
    let key = EntryKey::Fdb { bv_id: vlan.raw(), mac };
    Fdb::load(store.entry(&key).ok()?).ok()?.bridge_port.map(TypedId::raw)
}

/// The VLAN memberships of a bridge port: VID and tagging, in member id order.
fn memberships(store: &Store, bridge_port: BridgePortId) -> Vec<(Vid, Tagging)> {
    store
        .iter::<VlanMember>()
        .filter(|(_, m)| m.bridge_port == bridge_port)
        .filter_map(|(_, m)| Some((store.get::<Vlan>(m.vlan).ok()?.vid, m.tagging)))
        .collect()
}

/// A port joins a LAG that has a bridge port: the bridge port's learning mode and VLAN
/// memberships are applied to it; on a failure what was applied is undone (C++
/// `bridge_port_member_join`).
pub(crate) fn bridge_port_member_join<B: Backend>(
    backend: &mut B,
    store: &Store,
    lag: LagId,
    hw: HwPort,
) -> Result<()> {
    let Some(bridge_port) = bridge_port_of(store, Attach::Lag(lag)) else {
        return Ok(());
    };
    let learn = store.get::<BridgePort>(bridge_port)?.learn;
    let result = backend.port_learn_set(hw, learn.learn()).and_then(|()| {
        memberships(store, bridge_port)
            .into_iter()
            .try_for_each(|(vid, tagging)| backend.vlan_port_add(vid, hw, tagging.untagged()))
    });
    if result.is_err() {
        bridge_port_member_leave(backend, store, lag, hw);
    }
    result
}

/// A port leaves a LAG that has a bridge port: out of its VLANs, learning back to hardware
/// (C++ `bridge_port_member_leave`; the backend's answers are not looked at).
pub(crate) fn bridge_port_member_leave<B: Backend>(backend: &mut B, store: &Store, lag: LagId, hw: HwPort) {
    let Some(bridge_port) = bridge_port_of(store, Attach::Lag(lag)) else {
        return;
    };
    for (vid, _) in memberships(store, bridge_port) {
        let _ = backend.vlan_port_remove(vid, hw);
    }
    let _ = backend.port_learn_set(hw, Learn::Hw);
}

// --------------------------------------------------------------------- dispatch

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct L2Area;

impl Area for L2Area {
    /// C++ `set_vlan_attribute` (`VLAN_ID`), `set_vlan_member_attribute` (every attribute but
    /// `VLAN_TAGGING_MODE` is `attr_invalid`), `set_bridge_attribute`, `set_bridge_port_attribute`
    /// (`TYPE`, `PORT_ID`, `BRIDGE_ID`), `set_stp_attribute` (`sai_l2.cpp:198`-751).
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
        Some(match object_type {
            ObjectType::Vlan => crate::dispatch::cpp_refusal(attr, &[sys::SAI_VLAN_ATTR_VLAN_ID]),
            ObjectType::VlanMember => Error::attr(AttrError::InvalidValue, 0),
            ObjectType::BridgePort => crate::dispatch::cpp_refusal(
                attr,
                &[
                    sys::SAI_BRIDGE_PORT_ATTR_TYPE,
                    sys::SAI_BRIDGE_PORT_ATTR_PORT_ID,
                    sys::SAI_BRIDGE_PORT_ATTR_BRIDGE_ID,
                ],
            ),
            _ => crate::dispatch::cpp_refusal(attr, &[]),
        })
    }

    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        match object_type {
            ObjectType::Vlan => create_vlan(cx.backend, cx.store, attrs).map(TypedId::raw),
            ObjectType::VlanMember => create_vlan_member(cx.backend, cx.store, attrs).map(TypedId::raw),
            ObjectType::BridgePort => create_bridge_port(cx.backend, cx.store, attrs).map(TypedId::raw),
            // Only the default .1Q bridge (no .1D bridges) and the default STP instance exist.
            ObjectType::Bridge | ObjectType::Stp => Err(Error::NotSupported),
            _ => Err(Error::InvalidObjectType),
        }
    }

    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        match object_type {
            ObjectType::Vlan => remove_vlan(cx.backend, cx.store, VlanId::try_from(id)?),
            ObjectType::VlanMember => remove_vlan_member(cx.backend, cx.store, VlanMemberId::try_from(id)?),
            ObjectType::BridgePort => remove_bridge_port(cx.backend, cx.store, BridgePortId::try_from(id)?),
            // The default bridge is the only one (C++: `OBJECT_IN_USE` for it).
            ObjectType::Bridge => {
                store_get_bridge(cx.store, id)?;
                Err(Error::ObjectInUse)
            }
            ObjectType::Stp => Err(Error::NotSupported),
            _ => Err(Error::InvalidObjectType),
        }
    }

    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        match object_type {
            ObjectType::Vlan => set_vlan(cx.backend, cx.store, VlanId::try_from(id)?, attr),
            ObjectType::VlanMember => set_vlan_member(cx.backend, cx.store, VlanMemberId::try_from(id)?, attr),
            ObjectType::Bridge => {
                let bridge = store_get_bridge(cx.store, id)?;
                let mut kept = cx.store.get::<Bridge>(bridge)?.kept.clone();
                stub_set(BRIDGE_STUBS, &mut kept, attr, 0)?;
                cx.store.update::<Bridge, _>(bridge, |b| b.kept = kept)
            }
            ObjectType::BridgePort => set_bridge_port(cx.backend, cx.store, BridgePortId::try_from(id)?, attr),
            // Every STP attribute is read-only.
            ObjectType::Stp => Err(Error::attr(AttrError::NotSupported, 0)),
            _ => Err(Error::InvalidObjectType),
        }
    }

    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        let _ = backend;
        match object_type {
            ObjectType::Vlan => get_vlan(store, VlanId::try_from(id)?, meta.id),
            ObjectType::VlanMember => get_vlan_member(store, VlanMemberId::try_from(id)?, meta.id),
            ObjectType::Bridge => get_bridge(store, store_get_bridge(store, id)?, meta.id),
            ObjectType::BridgePort => get_bridge_port(store, BridgePortId::try_from(id)?, meta.id),
            ObjectType::Stp => get_stp(store, StpId::try_from(id)?, meta.id),
            _ => Err(Error::InvalidObjectType),
        }
    }

    fn create_entry<B: Backend>(cx: Cx<'_, B>, key: EntryKey, attrs: &[Attr]) -> Result<()> {
        create_fdb(cx.backend, cx.store, key, attrs)
    }

    fn remove_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey) -> Result<()> {
        remove_fdb(cx.backend, cx.store, key)
    }

    fn set_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey, attr: &Attr) -> Result<()> {
        set_fdb(cx.backend, cx.store, key, attr)
    }

    fn get_entry<B: Backend>(
        backend: &B,
        store: &Store,
        key: &EntryKey,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        let _ = backend;
        get_fdb(store, key, ids)
    }

    fn flush_fdb<B: Backend>(cx: Cx<'_, B>, attrs: &[Attr]) -> Result<Vec<EntryKey>> {
        flush(cx.backend, cx.store, attrs)
    }
}

fn store_get_bridge(store: &Store, raw: RawOid) -> Result<BridgeId> {
    let id = BridgeId::try_from(raw)?;
    store.get::<Bridge>(id).map(|_| id).map_err(|_| Error::InvalidObjectId)
}

// -------------------------------------------------------------------------- VLAN

fn create_vlan<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<VlanId> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_VLAN, attrs)?;
    let (index, raw) = attrs
        .iter()
        .enumerate()
        .find_map(|(i, a)| match (a.id, &a.value) {
            (sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(vid)) => Some((i, *vid)),
            _ => None,
        })
        .ok_or(Error::MandatoryAttributeMissing)?;
    let vid = Vid::new(raw).map_err(|_| Error::attr(AttrError::InvalidValue, index))?;
    if vlan_of_vid(store, vid).is_some() || crate::l3::is_internal_vlan(store, vid) {
        return Err(Error::ItemAlreadyExists);
    }
    let stp = default_stp(store);
    let mut kept = Kept::new();
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_VLAN_ATTR_VLAN_ID, _) => {}
            // Every VLAN is in the default instance.
            (sys::SAI_VLAN_ATTR_STP_INSTANCE, Value::Oid(instance)) if *instance == stp => {}
            (sys::SAI_VLAN_ATTR_STP_INSTANCE, _) => return Err(Error::attr(AttrError::InvalidValue, index)),
            _ => stub_set(VLAN_STUBS, &mut kept, attr, index)?,
        }
    }
    backend.vlan_create(vid)?;
    let id = VlanId::from_index(u64::from(vid.get()));
    store.insert_at(id, Vlan { vid, kept })?;
    Ok(id)
}

fn remove_vlan<B: Backend>(backend: &mut B, store: &mut Store, id: VlanId) -> Result<()> {
    let vid = store.get::<Vlan>(id)?.vid;
    if vid == Vid::DEFAULT {
        return Err(Error::ObjectInUse);
    }
    // Members, router interfaces and FDB entries name it (the C++ checks), or anything else.
    store.ensure_removable::<Vlan>(id)?;
    backend.vlan_destroy(vid)?;
    store.remove::<Vlan>(id).map(|_| ())
}

fn set_vlan<B: Backend>(backend: &mut B, store: &mut Store, id: VlanId, attr: &Attr) -> Result<()> {
    let vlan = store.get::<Vlan>(id)?;
    match (attr.id, &attr.value) {
        // Every VLAN is in the default instance (not kept).
        (sys::SAI_VLAN_ATTR_STP_INSTANCE, Value::Oid(instance)) if *instance == default_stp(store) => Ok(()),
        // Another instance; VLAN_ID is create-only (refused before this, with the C++ status: `refused_set`).
        (sys::SAI_VLAN_ATTR_STP_INSTANCE | sys::SAI_VLAN_ATTR_VLAN_ID, _) => {
            Err(Error::attr(AttrError::InvalidValue, 0))
        }
        _ => {
            let vid = vlan.vid;
            let mut kept = vlan.kept.clone();
            if crate::hash::vlan_flood_attr(attr.id) {
                // The backend's flood control, else only kept.
                crate::hash::vlan_flood_apply(backend, vid, attr)?;
            }
            stub_set(VLAN_STUBS, &mut kept, attr, 0)?;
            store.update::<Vlan, _>(id, |v| v.kept = kept)
        }
    }
}

fn get_vlan(store: &Store, id: VlanId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let vlan = store.get::<Vlan>(id)?;
    Ok(match attr {
        sys::SAI_VLAN_ATTR_VLAN_ID => Value::U16(vlan.vid.get()),
        sys::SAI_VLAN_ATTR_MEMBER_LIST => Value::OidList(
            store
                .iter::<VlanMember>()
                .filter(|(_, m)| m.vlan == id)
                .map(|(m, _)| m.raw())
                .collect(),
        ),
        sys::SAI_VLAN_ATTR_STP_INSTANCE => Value::Oid(default_stp(store)),
        _ => stub_get(VLAN_STUBS, &vlan.kept, attr)?,
    })
}

// ------------------------------------------------------------------- VLAN member

fn create_vlan_member<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<VlanMemberId> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_VLAN_MEMBER, attrs)?;
    let (mut vlan, mut bridge_port, mut tagging) = (None, None, Tagging::Untagged);
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(v)) => vlan = Some(*v),
            (sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(b)) => bridge_port = Some(*b),
            (sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, Value::Enum(mode)) => {
                if *mode == sys::SAI_VLAN_TAGGING_MODE_PRIORITY_TAGGED.cast_signed() {
                    return Err(Error::attr(AttrError::NotSupported, index));
                }
                tagging = Tagging::from_sai(*mode).ok_or(Error::attr(AttrError::InvalidValue, index))?;
            }
            _ => return Err(Error::attr(AttrError::NotSupported, index)),
        }
    }
    let (Some(vlan), Some(bridge_port)) = (vlan, bridge_port) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    vlan_member_create_internal(backend, store, vlan, bridge_port, tagging, true)
}

/// Makes a VLAN member (C++ `vlan_member_create_internal`); with `program` every chip port of the
/// bridge port joins the VLAN first (all or none: the ports that joined leave again when one
/// fails).
pub(crate) fn vlan_member_create_internal<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    vlan: RawOid,
    bridge_port: RawOid,
    tagging: Tagging,
    program: bool,
) -> Result<VlanMemberId> {
    let (vlan, vid) = vlan_of(store, vlan)
        .map(|(id, v)| (id, v.vid))
        .ok_or(Error::InvalidParameter)?;
    let bridge_port = BridgePortId::try_from(bridge_port)
        .ok()
        .filter(|bp| store.get::<BridgePort>(*bp).is_ok())
        .ok_or(Error::InvalidParameter)?;
    dest_of(store, bridge_port).ok_or(Error::NotSupported)?;
    if store
        .iter::<VlanMember>()
        .any(|(_, m)| m.vlan == vlan && m.bridge_port == bridge_port)
    {
        return Err(Error::ItemAlreadyExists);
    }
    if program {
        let mut done = Vec::new();
        for hw in bridge_port_phys(store, bridge_port) {
            if let Err(e) = backend.vlan_port_add(vid, hw, tagging.untagged()) {
                for hw in done {
                    let _ = backend.vlan_port_remove(vid, hw);
                }
                return Err(e);
            }
            done.push(hw);
        }
    }
    store.insert(VlanMember {
        vlan,
        bridge_port,
        tagging,
    })
}

fn remove_vlan_member<B: Backend>(backend: &mut B, store: &mut Store, id: VlanMemberId) -> Result<()> {
    store.ensure_removable::<VlanMember>(id)?;
    let member = store.get::<VlanMember>(id)?;
    let vid = store.get::<Vlan>(member.vlan)?.vid;
    // As the C++: stops at the first port the chip refuses (the ports before it have left).
    for hw in bridge_port_phys(store, member.bridge_port) {
        backend.vlan_port_remove(vid, hw)?;
    }
    store.remove::<VlanMember>(id).map(|_| ())
}

fn set_vlan_member<B: Backend>(backend: &mut B, store: &mut Store, id: VlanMemberId, attr: &Attr) -> Result<()> {
    let member = store.get::<VlanMember>(id)?;
    let (sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, Value::Enum(mode)) = (attr.id, &attr.value) else {
        // VLAN_ID and BRIDGE_PORT_ID are create-only (refused before this, with the C++ status: `refused_set`).
        return Err(Error::attr(AttrError::InvalidValue, 0));
    };
    let tagging = Tagging::from_sai(*mode).ok_or(Error::attr(AttrError::NotSupported, 0))?;
    let vid = store.get::<Vlan>(member.vlan).map_err(|_| Error::Failure)?.vid;
    dest_of(store, member.bridge_port).ok_or(Error::Failure)?;
    // As the C++: stops at the first port the chip refuses (the ports before it have changed).
    for hw in bridge_port_phys(store, member.bridge_port) {
        backend.vlan_port_add(vid, hw, tagging.untagged())?;
    }
    store.update::<VlanMember, _>(id, |m| m.tagging = tagging)
}

fn get_vlan_member(store: &Store, id: VlanMemberId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let member = store.get::<VlanMember>(id)?;
    Ok(match attr {
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID => Value::Oid(member.vlan.raw()),
        sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID => Value::Oid(member.bridge_port.raw()),
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE => Value::Enum(member.tagging.sai()),
        _ => return Err(Error::attr(AttrError::NotSupported, 0)),
    })
}

// ------------------------------------------------------------------------ bridge

fn get_bridge(store: &Store, id: BridgeId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let bridge = store.get::<Bridge>(id)?;
    Ok(match attr {
        sys::SAI_BRIDGE_ATTR_TYPE => enumerator(sys::SAI_BRIDGE_TYPE_1Q),
        sys::SAI_BRIDGE_ATTR_PORT_LIST => Value::OidList(
            store
                .iter::<BridgePort>()
                .filter(|(_, b)| b.bridge == id)
                .map(|(b, _)| b.raw())
                .collect(),
        ),
        _ => stub_get(BRIDGE_STUBS, &bridge.kept, attr)?,
    })
}

// ------------------------------------------------------------------- bridge port

fn create_bridge_port<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<BridgePortId> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_BRIDGE_PORT, attrs)?;
    let bridge = the_switch(store)?.1.default_bridge;
    let (mut typed, mut port, mut admin, mut learn) = (false, RawOid::NULL, false, FdbLearning::Hw);
    let mut kept = Kept::new();
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_BRIDGE_PORT_ATTR_TYPE, Value::Enum(t)) => {
                if *t != sys::SAI_BRIDGE_PORT_TYPE_PORT.cast_signed() {
                    return Err(Error::attr(AttrError::NotSupported, index));
                }
                typed = true;
            }
            (sys::SAI_BRIDGE_PORT_ATTR_PORT_ID, Value::Oid(p)) => port = *p,
            (sys::SAI_BRIDGE_PORT_ATTR_BRIDGE_ID, Value::Oid(b)) => {
                if *b != bridge.raw() {
                    return Err(Error::attr(AttrError::InvalidValue, index));
                }
            }
            (sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE, Value::Bool(up)) => admin = *up,
            (sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE, Value::Enum(mode)) => {
                learn = FdbLearning::from_sai(*mode).ok_or(Error::attr(AttrError::InvalidValue, index))?;
            }
            _ => stub_set(BRIDGE_PORT_STUBS, &mut kept, attr, index)?,
        }
    }
    if !typed || port.is_null() {
        return Err(Error::MandatoryAttributeMissing);
    }
    // The CPU port, a LAG member, nothing.
    let attach = lag::attach_of(store, port).ok_or(Error::InvalidParameter)?;
    if bridge_port_of(store, attach).is_some() {
        return Err(Error::ItemAlreadyExists);
    }
    // As the C++: stops at the first port the chip refuses (no undo).
    for hw in lag::attach_phys(store, port) {
        backend.port_learn_set(hw, learn.learn())?;
    }
    store.insert(BridgePort {
        bridge,
        attach,
        admin,
        learn,
        kept,
    })
}

/// The FDB entries on a bridge port, by type.
fn fdbs_on(store: &Store, bridge_port: BridgePortId) -> Vec<(EntryKey, FdbType)> {
    store
        .entries()
        .filter_map(|(key, stored)| {
            let fdb = Fdb::load(stored).ok()?;
            (matches!(key, EntryKey::Fdb { .. }) && fdb.bridge_port == Some(bridge_port)).then_some((*key, fdb.kind))
        })
        .collect()
}

fn remove_bridge_port<B: Backend>(backend: &mut B, store: &mut Store, id: BridgePortId) -> Result<()> {
    store.get::<BridgePort>(id)?;
    if store.iter::<VlanMember>().any(|(_, m)| m.bridge_port == id) {
        return Err(Error::ObjectInUse);
    }
    let fdbs = fdbs_on(store, id);
    if fdbs.iter().any(|(_, kind)| *kind == FdbType::Static) {
        return Err(Error::ObjectInUse);
    }
    // Nothing else may name it (checked before the chip is touched).
    if usize::try_from(store.referrers(id.raw())).unwrap_or(usize::MAX) > fdbs.len() {
        return Err(Error::ObjectInUse);
    }
    if let Some(dest) = dest_of(store, id) {
        // As the C++: the answer is not looked at.
        let _ = backend.l2_flush(Some(dest), None, false, true);
    }
    for (key, _) in fdbs {
        store.remove_entry(&key)?;
    }
    store.remove::<BridgePort>(id).map(|_| ())
}

fn set_bridge_port<B: Backend>(backend: &mut B, store: &mut Store, id: BridgePortId, attr: &Attr) -> Result<()> {
    let bridge_port = store.get::<BridgePort>(id)?;
    match (attr.id, &attr.value) {
        (sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE, Value::Bool(up)) => {
            store.update::<BridgePort, _>(id, |b| b.admin = *up)
        }
        (sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE, Value::Enum(mode)) => {
            let learn = FdbLearning::from_sai(*mode).ok_or(Error::attr(AttrError::InvalidValue, 0))?;
            dest_of(store, id).ok_or(Error::Failure)?;
            // As the C++: stops at the first port the chip refuses (no undo).
            for hw in bridge_port_phys(store, id) {
                backend
                    .port_learn_set(hw, learn.learn())
                    .map_err(|e| attr_backend_status(e, 0))?;
            }
            store.update::<BridgePort, _>(id, |b| b.learn = learn)
        }
        // Create-only (refused before this, with the C++ status: `refused_set`).
        (
            sys::SAI_BRIDGE_PORT_ATTR_TYPE | sys::SAI_BRIDGE_PORT_ATTR_PORT_ID | sys::SAI_BRIDGE_PORT_ATTR_BRIDGE_ID,
            _,
        ) => Err(Error::attr(AttrError::InvalidValue, 0)),
        _ => {
            let mut kept = bridge_port.kept.clone();
            stub_set(BRIDGE_PORT_STUBS, &mut kept, attr, 0)?;
            store.update::<BridgePort, _>(id, |b| b.kept = kept)
        }
    }
}

fn get_bridge_port(store: &Store, id: BridgePortId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let bridge_port = store.get::<BridgePort>(id)?;
    Ok(match attr {
        sys::SAI_BRIDGE_PORT_ATTR_TYPE => enumerator(sys::SAI_BRIDGE_PORT_TYPE_PORT),
        sys::SAI_BRIDGE_PORT_ATTR_PORT_ID => Value::Oid(bridge_port.attach.raw()),
        sys::SAI_BRIDGE_PORT_ATTR_BRIDGE_ID => Value::Oid(bridge_port.bridge.raw()),
        sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE => Value::Bool(bridge_port.admin),
        sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE => Value::Enum(bridge_port.learn.sai()),
        _ => stub_get(BRIDGE_PORT_STUBS, &bridge_port.kept, attr)?,
    })
}

// --------------------------------------------------------------------------- STP

fn get_stp(store: &Store, id: StpId, attr: sys::sai_attr_id_t) -> Result<Value> {
    if id.raw() != default_stp(store) {
        return Err(Error::InvalidObjectId);
    }
    Ok(match attr {
        sys::SAI_STP_ATTR_VLAN_LIST => {
            let mut vids: Vec<u16> = store.iter::<Vlan>().map(|(_, v)| v.vid.get()).collect();
            vids.sort_unstable();
            Value::VlanList(vids)
        }
        sys::SAI_STP_ATTR_BRIDGE_ID => Value::Oid(the_switch(store)?.1.default_bridge.raw()),
        // No STP ports: the instance has none.
        sys::SAI_STP_ATTR_PORT_LIST => Value::OidList(Vec::new()),
        _ => return Err(Error::attr(AttrError::NotSupported, 0)),
    })
}

// --------------------------------------------------------------------------- FDB

/// The VLAN and address of an FDB key whose VLAN exists.
fn fdb_target(store: &Store, key: &EntryKey) -> Option<(Vid, MacAddr)> {
    let EntryKey::Fdb { bv_id, mac } = key else {
        return None;
    };
    vlan_of(store, *bv_id).map(|(_, v)| (v.vid, MacAddr(*mac)))
}

/// One attribute into an entry (C++ `fdb_attr`).
fn fdb_attr(store: &Store, fdb: &mut Fdb, attr: &Attr, index: usize) -> Result<()> {
    match (attr.id, &attr.value) {
        (sys::SAI_FDB_ENTRY_ATTR_TYPE, Value::Enum(kind)) => {
            fdb.kind = match u32::try_from(*kind) {
                Ok(sys::SAI_FDB_ENTRY_TYPE_STATIC) => FdbType::Static,
                Ok(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC) => FdbType::Dynamic,
                _ => return Err(Error::attr(AttrError::InvalidValue, index)),
            };
        }
        (sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(bp)) => {
            fdb.bridge_port = if bp.is_null() {
                None
            } else {
                let bp = BridgePortId::try_from(*bp)
                    .ok()
                    .filter(|bp| dest_of(store, *bp).is_some())
                    .ok_or(Error::attr(AttrError::InvalidValue, index))?;
                Some(bp)
            };
        }
        (sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION, Value::Enum(action)) => {
            fdb.action = match u32::try_from(*action) {
                Ok(sys::SAI_PACKET_ACTION_FORWARD) => FdbAction::Forward,
                Ok(sys::SAI_PACKET_ACTION_DROP) => FdbAction::Drop,
                _ => return Err(Error::attr(AttrError::NotSupported, index)),
            };
        }
        _ => stub_set(FDB_STUBS, &mut fdb.kept, attr, index)?,
    }
    Ok(())
}

/// Writes an entry into the chip's L2 table (C++ `fdb_program`): to its bridge port's port or
/// trunk; a discard entry without a bridge port goes to the CPU port.
fn fdb_program<B: Backend>(backend: &mut B, store: &Store, vid: Vid, mac: MacAddr, fdb: &Fdb) -> Result<()> {
    let discard = fdb.action == FdbAction::Drop;
    let dest = match (fdb.bridge_port.and_then(|bp| dest_of(store, bp)), discard) {
        (Some(dest), _) => dest,
        (None, false) => return Err(Error::InvalidParameter),
        (None, true) => Dest::Port(backend.cpu_port()),
    };
    backend.l2_add(mac, vid, dest, fdb.kind == FdbType::Static, discard)
}

fn create_fdb<B: Backend>(backend: &mut B, store: &mut Store, key: EntryKey, attrs: &[Attr]) -> Result<()> {
    let EntryKey::Fdb { bv_id, .. } = key else {
        return Err(Error::InvalidParameter);
    };
    // .1D bridges are not supported; the VLAN must exist.
    if bv_id.type_bits() == sys::SAI_OBJECT_TYPE_BRIDGE {
        return Err(Error::NotSupported);
    }
    let (vid, mac) = fdb_target(store, &key).ok_or(Error::InvalidParameter)?;
    // A dynamic entry (learned, or created so) is replaced; a static one stays.
    let existing = store.entry(&key).ok().map(Fdb::load).transpose()?;
    if existing.as_ref().is_some_and(|f| f.kind == FdbType::Static) {
        return Err(Error::ItemAlreadyExists);
    }
    CreateArgs::check(sys::SAI_OBJECT_TYPE_FDB_ENTRY, attrs)?;
    let mut fdb = Fdb::default();
    for (index, attr) in attrs.iter().enumerate() {
        fdb_attr(store, &mut fdb, attr, index)?;
    }
    fdb_program(backend, store, vid, mac, &fdb)?;
    if existing.is_some() {
        store.update_entry(&key, |stored| *stored = fdb.to_kept())
    } else {
        store.insert_entry(key, fdb.to_kept())
    }
}

fn remove_fdb<B: Backend>(backend: &mut B, store: &mut Store, key: &EntryKey) -> Result<()> {
    store.entry(key)?;
    if let Some((vid, mac)) = fdb_target(store, key) {
        match backend.l2_delete(mac, vid) {
            // Aged out of the chip already.
            Ok(()) | Err(Error::ItemNotFound) => {}
            Err(e) => return Err(e),
        }
    }
    store.remove_entry(key).map(|_| ())
}

fn set_fdb<B: Backend>(backend: &mut B, store: &mut Store, key: &EntryKey, attr: &Attr) -> Result<()> {
    let mut fdb = Fdb::load(store.entry(key)?)?;
    attrs::check_set(sys::SAI_OBJECT_TYPE_FDB_ENTRY, attr)?;
    fdb_attr(store, &mut fdb, attr, 0)?;
    if matches!(
        attr.id,
        sys::SAI_FDB_ENTRY_ATTR_TYPE | sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID | sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION
    ) {
        let (vid, mac) = fdb_target(store, key).ok_or(Error::InvalidParameter)?;
        fdb_program(backend, store, vid, mac, &fdb)?;
    }
    store.update_entry(key, |stored| *stored = fdb.to_kept())
}

fn get_fdb(store: &Store, key: &EntryKey, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
    let fdb = Fdb::load(store.entry(key)?)?;
    let stored = fdb.to_kept();
    ids.iter()
        .enumerate()
        .map(|(index, id)| match *id {
            sys::SAI_FDB_ENTRY_ATTR_TYPE | sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION => {
                stored.get(id).cloned().ok_or(Error::Failure)
            }
            sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID => {
                Ok(Value::Oid(fdb.bridge_port.map_or(RawOid::NULL, TypedId::raw)))
            }
            _ => stub_get(FDB_STUBS, &fdb.kept, *id).map_err(|e| e.at_index(index)),
        })
        .collect()
}

/// `flush_fdb_entries`: the chip's entries of a bridge port and / or VLAN, static and / or dynamic
/// (default: dynamic), then the model's (C++ `flush_fdb_entries`). Returns the keys removed.
fn flush<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<Vec<EntryKey>> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_FDB_FLUSH, attrs)?;
    let (mut bridge_port, mut bv_id) = (RawOid::NULL, RawOid::NULL);
    let mut kind = sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC;
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, Value::Oid(bp)) => bridge_port = *bp,
            (sys::SAI_FDB_FLUSH_ATTR_BV_ID, Value::Oid(bv)) => bv_id = *bv,
            (sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE, Value::Enum(t)) => {
                kind = u32::try_from(*t).map_err(|_| Error::attr(AttrError::InvalidValue, index))?;
            }
            _ => return Err(Error::attr(AttrError::NotSupported, index)),
        }
    }
    let dest = if bridge_port.is_null() {
        None
    } else {
        let bp = BridgePortId::try_from(bridge_port).map_err(|_| Error::InvalidParameter)?;
        Some(dest_of(store, bp).ok_or(Error::InvalidParameter)?)
    };
    let vid = if bv_id.is_null() {
        None
    } else {
        Some(vlan_of(store, bv_id).ok_or(Error::InvalidParameter)?.1.vid)
    };
    let statics = matches!(
        kind,
        sys::SAI_FDB_FLUSH_ENTRY_TYPE_STATIC | sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL
    );
    let dynamics = matches!(
        kind,
        sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC | sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL
    );
    backend.l2_flush(dest, vid, statics, dynamics)?;
    let matching: Vec<EntryKey> = store
        .entries()
        .filter(|(key, stored)| {
            let EntryKey::Fdb { bv_id: in_vlan, .. } = key else {
                return false;
            };
            let Ok(fdb) = Fdb::load(stored) else { return false };
            let by_type = match fdb.kind {
                FdbType::Static => statics,
                FdbType::Dynamic => dynamics,
            };
            by_type
                && (bridge_port.is_null() || fdb.bridge_port.map(TypedId::raw) == Some(bridge_port))
                && (bv_id.is_null() || *in_vlan == bv_id)
        })
        .map(|(key, _)| *key)
        .collect();
    for key in &matching {
        store.remove_entry(key)?;
    }
    Ok(matching)
}

// -------------------------------------------------------------------- FDB events

/// An address the chip learned or aged: the model follows and the SAI user is told (C++
/// `deliver_fdb`). Nothing for an address in a VLAN that is not a SAI VLAN (a router interface's
/// internal one), a learn on a port without a bridge port, a learn of a static entry or of one
/// already known on that bridge port (created through SAI, then reported by the chip), and an
/// age of an unknown or static entry. A learn of an address known on another bridge port is a
/// `MOVE`; an age reports the bridge port of where it was seen, else the entry's.
pub(crate) fn fdb_event(store: &mut Store, event: FdbEvent) -> Vec<Notification> {
    let Ok((switch, _)) = the_switch(store) else {
        return Vec::new();
    };
    let Some(vlan) = vlan_of_vid(store, event.vid) else {
        return Vec::new();
    };
    let seen_on = bridge_port_of_dest(store, event.hw_port);
    let key = EntryKey::Fdb {
        bv_id: vlan.raw(),
        mac: event.mac.0,
    };
    let known = store.entry(&key).ok().and_then(|stored| Fdb::load(stored).ok());
    let (kind, bridge_port) = match event.change {
        FdbChange::Learned => {
            let Some(bridge_port) = seen_on else {
                return Vec::new();
            };
            if known
                .as_ref()
                .is_some_and(|f| f.kind == FdbType::Static || f.bridge_port == Some(bridge_port))
            {
                return Vec::new();
            }
            let learned = Fdb {
                bridge_port: Some(bridge_port),
                ..Fdb::default()
            }
            .to_kept();
            let stored = if known.is_some() {
                store.update_entry(&key, |entry| *entry = learned)
            } else {
                store.insert_entry(key, learned)
            };
            if stored.is_err() {
                return Vec::new();
            }
            let kind = if known.is_some() {
                FdbEventKind::Move
            } else {
                FdbEventKind::Learned
            };
            (kind, Some(bridge_port))
        }
        FdbChange::Aged => {
            let Some(entry) = known.filter(|f| f.kind == FdbType::Dynamic) else {
                return Vec::new();
            };
            if store.remove_entry(&key).is_err() {
                return Vec::new();
            }
            (FdbEventKind::Aged, seen_on.or(entry.bridge_port))
        }
    };
    vec![Notification::Fdb {
        switch,
        kind,
        mac: event.mac.0,
        vlan,
        bridge_port,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_defaults_are_values_of_their_attributes() {
        for (object_type, stubs) in [
            (sys::SAI_OBJECT_TYPE_VLAN, VLAN_STUBS),
            (sys::SAI_OBJECT_TYPE_BRIDGE, BRIDGE_STUBS),
            (sys::SAI_OBJECT_TYPE_BRIDGE_PORT, BRIDGE_PORT_STUBS),
            (sys::SAI_OBJECT_TYPE_FDB_ENTRY, FDB_STUBS),
        ] {
            for (id, default) in stubs {
                let meta = sai_meta::attribute(object_type, *id);
                assert_eq!(
                    meta.map(|m| m.value_type),
                    Some(default.value_type()),
                    "{object_type} {id}"
                );
            }
        }
    }

    #[test]
    fn enums_round_trip_their_sai_numbers() {
        for mode in [
            sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DROP,
            sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE,
            sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_HW,
            sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_CPU_TRAP,
            sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_CPU_LOG,
            sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_FDB_NOTIFICATION,
        ] {
            let learning = FdbLearning::from_sai(mode.cast_signed());
            assert_eq!(learning.map(FdbLearning::sai), Some(mode.cast_signed()));
        }
        assert_eq!(FdbLearning::from_sai(-1), None);
        // C++ to_learn
        assert_eq!(FdbLearning::CpuLog.learn(), Learn::Cpu);
        assert_eq!(FdbLearning::FdbNotification.learn(), Learn::Hw);
        assert_eq!(
            Tagging::from_sai(sys::SAI_VLAN_TAGGING_MODE_PRIORITY_TAGGED.cast_signed()),
            None
        );
        assert_eq!(Tagging::from_sai(Tagging::Tagged.sai()), Some(Tagging::Tagged));
        let fdb = Fdb {
            kind: FdbType::Static,
            bridge_port: Some(BridgePortId::from_index(3)),
            action: FdbAction::Drop,
            kept: Kept::from([(sys::SAI_FDB_ENTRY_ATTR_META_DATA, Value::U32(7))]),
        };
        assert_eq!(Fdb::load(&fdb.to_kept()), Ok(fdb));
        assert_eq!(Fdb::load(&Kept::new()), Err(Error::Failure));
    }
}
