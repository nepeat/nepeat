//! `QoS`: maps, schedulers and scheduler groups (one per queue, for qosorch), WRED, buffer pools
//! and profiles, queue / priority group attributes and counters (`_ext READ_AND_CLEAR` = get then
//! clear), the port's and the switch's `QoS` attributes. With [`crate::backend::Qos::qos_supported`]
//! false (the hardware backends, and the virtual chip unless `qos_on`) everything is kept and nothing
//! programmed. Ports `sai_qos.cpp` (1359 lines).
//!
//! Hierarchy (as the C++): one scheduler group per queue (level 0, parent = the port, one child =
//! that queue), because qosorch looks a queue's group up and sets the group's
//! `SCHEDULER_PROFILE_ID`; a port's own `QOS_SCHEDULER_PROFILE_ID` is the port shaper. A queue's
//! scheduler is its group's, else the queue's own `SCHEDULER_PROFILE_ID`.
//!
//! Where the state lives: maps, schedulers, WRED profiles, buffer pools and profiles are typed
//! objects of this module ([`QosMap`], [`Scheduler`], [`Wred`], [`BufferPool`], [`BufferProfile`]);
//! queues, priority groups and scheduler groups are the attribute bags the switch builds (their
//! object-id attributes are references the store counts); a port's `QoS` attributes are in
//! [`crate::model::Port::kept`] and the switch's maps in [`crate::model::Switch::kept`], under their
//! attribute ids. "In use" (`OBJECT_IN_USE`) is therefore the store's reference count, which covers
//! the C++ `map_used` / `sched_used` / `profile_used` / WRED and pool checks exactly.
//!
//! Backend: [`crate::backend::Qos`], [`crate::backend::PortOps::port_pfc_set`]. The backend is
//! called first and the store changed second; where one SAI change re-programs several queues (a
//! scheduler or buffer profile many use) a failure re-programs the old state everywhere, as the
//! C++ does.
//!
//! Registration: [`crate::dispatch::Owner::Qos`] for `QUEUE`, `INGRESS_PRIORITY_GROUP`,
//! `SCHEDULER_GROUP`, `SCHEDULER`, `BUFFER_POOL`, `BUFFER_PROFILE`, `QOS_MAP`, `WRED` in
//! [`crate::dispatch::OWNERS`]; counters through [`stats_get`] / [`stats_clear`] /
//! [`stats_capability`] ([`crate::sai::Sai`] calls them); the port's and the switch's attributes
//! through [`port_qos_set`] / [`port_qos_get`] / [`switch_qos_set`] / [`switch_qos_get`] (the port and
//! switch code calls them, in the C++ order: `QoS`, storm, mirror, `MACsec`, stubs).
use crate::attrs;
use crate::backend::types::{
    BufferCfg, QosMapEntry as HwMapEntry, QosMapId as HwMapId, QosMapKind, SchedCfg, WredCfg, WredId as HwWredId,
};
use crate::backend::{Backend, HwPort};
use crate::dispatch::{Area, Cx};
use crate::error::{AttrError, Error, Result};
use crate::log;
use crate::model::{Bag, Kept, Port, PortKind, Switch};
use crate::oid::{
    BufferPoolId, BufferProfileId, ObjectType, PortId, QosMapId, RawOid, SchedulerId, SwitchId, TypedId, WredId,
};
use crate::store::Store;
use crate::value::{Attr, QosMapEntry, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

// ------------------------------------------------------------------ shared helpers

/// The C++ `attr_backend_status`: a backend's `INVALID_PARAMETER` / `NOT_SUPPORTED` /
/// `NOT_IMPLEMENTED` as the attribute's error, anything else as it is.
pub(crate) fn backend_attr_error(error: Error, index: usize) -> Error {
    match error {
        Error::InvalidParameter => Error::attr(AttrError::InvalidValue, index),
        Error::NotSupported => Error::attr(AttrError::NotSupported, index),
        Error::NotImplemented => Error::attr(AttrError::NotImplemented, index),
        other => other,
    }
}

/// C++ `attr_invalid`.
pub(crate) fn invalid(index: usize) -> Error {
    Error::attr(AttrError::InvalidValue, index)
}

/// C++ `attr_not_supported`.
pub(crate) fn not_supported(index: usize) -> Error {
    Error::attr(AttrError::NotSupported, index)
}

/// C++ `attr_unknown`.
pub(crate) fn unknown(index: usize) -> Error {
    Error::attr(AttrError::Unknown, index)
}

/// What the rim checks of a create attribute, again for callers of the core: an attribute of the
/// type, allowed on create, a value of its kind, an enumerator its header has. Object code then
/// takes the attributes one by one in the C++ order (no mandatory-attribute check from the
/// metadata: the C++ asks for fewer, e.g. no `THRESHOLD_MODE` for a buffer profile).
pub(crate) fn create_arg(object_type: ObjectType, attr: &Attr, index: usize) -> Result<&'static AttrMeta> {
    let meta = attrs::meta(object_type.sai(), attr.id, index)?;
    if !meta.flags.creatable() {
        return Err(Error::attr(AttrError::Invalid, index));
    }
    if attr.value.value_type() != meta.value_type {
        return Err(invalid(index));
    }
    if let Value::Enum(v) = attr.value {
        if !meta.enum_values.is_empty() && !meta.enum_values.contains(&v) {
            return Err(invalid(index));
        }
    }
    Ok(meta)
}

/// The object id a set or create attribute carries (the value kind was checked).
fn oid_of(attr: &Attr) -> Result<RawOid> {
    match attr.value {
        Value::Oid(oid) => Ok(oid),
        _ => Err(Error::Failure),
    }
}

/// A kept object id (null while unset).
pub(crate) fn kept_oid(kept: &Kept, attr: sys::sai_attr_id_t) -> RawOid {
    match kept.get(&attr) {
        Some(Value::Oid(oid)) => *oid,
        _ => RawOid::NULL,
    }
}

/// A kept object list (empty while unset).
pub(crate) fn kept_list(kept: &Kept, attr: sys::sai_attr_id_t) -> Vec<RawOid> {
    match kept.get(&attr) {
        Some(Value::OidList(list)) => list.clone(),
        _ => Vec::new(),
    }
}

fn kept_u8(kept: &Kept, attr: sys::sai_attr_id_t) -> u8 {
    match kept.get(&attr) {
        Some(Value::U8(v)) => *v,
        _ => 0,
    }
}

/// The chip port of a front-panel port; `None` for the CPU port.
pub(crate) fn front(port: &Port) -> Option<HwPort> {
    matches!(port.kind, PortKind::Front { .. }).then_some(port.hw)
}

/// Keeps a port attribute after the backend took it.
pub(crate) fn keep_on_port(store: &mut Store, port: PortId, attr: &Attr) -> Result<()> {
    store.update::<Port, _>(port, |p| drop(p.kept.insert(attr.id, attr.value.clone())))
}

fn the_switch(store: &Store) -> Option<&Switch> {
    store.iter::<Switch>().next().map(|(_, s)| s)
}

fn count(n: usize) -> Result<Value> {
    u32::try_from(n).map(Value::U32).map_err(|_| Error::Failure)
}

/// Implements [`crate::model::Kind`] for an area's typed object, stored as
/// `Object::$outer($area::$variant(_))`.
macro_rules! area_kind {
    ($ty:ident, $id:ty, $outer:ident, $area:ident :: $variant:ident) => {
        impl crate::model::Kind for $ty {
            type Id = $id;
            fn wrap(self) -> crate::model::Object {
                crate::model::Object::$outer($area::$variant(self))
            }
            fn peel(object: &crate::model::Object) -> Option<&Self> {
                if let crate::model::Object::$outer($area::$variant(o)) = object {
                    Some(o)
                } else {
                    None
                }
            }
            fn peel_mut(object: &mut crate::model::Object) -> Option<&mut Self> {
                if let crate::model::Object::$outer($area::$variant(o)) = object {
                    Some(o)
                } else {
                    None
                }
            }
            fn unwrap(object: crate::model::Object) -> Option<Self> {
                if let crate::model::Object::$outer($area::$variant(o)) = object {
                    Some(o)
                } else {
                    None
                }
            }
        }
    };
}
pub(crate) use area_kind;

/// This area's typed objects, as the store holds them ([`crate::model::Object::Qos`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QosObject {
    /// A `QOS_MAP`.
    Map(QosMap),
    /// A `SCHEDULER`.
    Scheduler(Scheduler),
    /// A `WRED` profile.
    Wred(Wred),
    /// A `BUFFER_POOL`.
    Pool(BufferPool),
    /// A `BUFFER_PROFILE`.
    Profile(BufferProfile),
}

impl QosObject {
    /// The object's type.
    pub const fn object_type(&self) -> ObjectType {
        match self {
            Self::Map(_) => ObjectType::QosMap,
            Self::Scheduler(_) => ObjectType::Scheduler,
            Self::Wred(_) => ObjectType::Wred,
            Self::Pool(_) => ObjectType::BufferPool,
            Self::Profile(_) => ObjectType::BufferProfile,
        }
    }

    /// What it refers to: a buffer profile its pool.
    pub fn references(&self) -> Vec<RawOid> {
        match self {
            Self::Profile(p) => vec![p.pool.raw()],
            Self::Map(_) | Self::Scheduler(_) | Self::Wred(_) | Self::Pool(_) => Vec::new(),
        }
    }
}

/// A typed object of this area by its raw id (`None`: no such object of the type).
fn lookup<K: crate::model::Kind>(store: &Store, oid: RawOid) -> Option<&K> {
    K::Id::try_from(oid).ok().and_then(|id| store.get::<K>(id).ok())
}

/// A change not stored yet, seen by the functions that compute what the chip gets.
#[derive(Clone, Copy, Debug)]
enum Pending<'a> {
    /// Nothing: the store as it is.
    Nothing,
    /// A scheduler's new profile.
    Scheduler(SchedulerId, &'a Scheduler),
    /// A buffer pool's new values.
    Pool(BufferPoolId, &'a BufferPool),
    /// A buffer profile's new values.
    Profile(BufferProfileId, &'a BufferProfile),
}

fn scheduler<'a>(store: &'a Store, oid: RawOid, pending: Pending<'a>) -> Option<&'a Scheduler> {
    match pending {
        Pending::Scheduler(id, s) if id.raw() == oid => Some(s),
        _ => lookup::<Scheduler>(store, oid),
    }
}

fn pool<'a>(store: &'a Store, oid: RawOid, pending: Pending<'a>) -> Option<&'a BufferPool> {
    match pending {
        Pending::Pool(id, p) if id.raw() == oid => Some(p),
        _ => lookup::<BufferPool>(store, oid),
    }
}

fn profile<'a>(store: &'a Store, oid: RawOid, pending: Pending<'a>) -> Option<&'a BufferProfile> {
    match pending {
        Pending::Profile(id, p) if id.raw() == oid => Some(p),
        _ => lookup::<BufferProfile>(store, oid),
    }
}

// ------------------------------------------------------------------ the area

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct QosArea;

impl Area for QosArea {
    /// [`refused_set`] (this module's types).
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
        refused_set(object_type, attr)
    }

    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        match object_type {
            ObjectType::QosMap => create_qos_map(cx.backend, cx.store, attrs),
            ObjectType::Scheduler => create_scheduler(cx.store, attrs),
            ObjectType::Wred => create_wred(cx.backend, cx.store, attrs),
            ObjectType::BufferPool => create_buffer_pool(cx.store, attrs),
            ObjectType::BufferProfile => create_buffer_profile(cx.store, attrs),
            // Fixed: one group per queue; queues and priority groups come with the port.
            ObjectType::SchedulerGroup => {
                log::warn(
                    sys::SAI_API_SCHEDULER_GROUP,
                    format_args!("scheduler groups are fixed (one per queue); create refused"),
                );
                Err(Error::NotSupported)
            }
            _ => Err(Error::NotSupported),
        }
    }

    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        match object_type {
            ObjectType::QosMap => {
                let id = QosMapId::try_from(id)?;
                cx.store.ensure_removable::<QosMap>(id)?;
                if let Some(hw) = cx.store.get::<QosMap>(id)?.hw {
                    cx.backend.qos_map_destroy(hw)?;
                }
                cx.store.remove::<QosMap>(id).map(drop)
            }
            ObjectType::Scheduler => cx.store.remove::<Scheduler>(SchedulerId::try_from(id)?).map(drop),
            ObjectType::Wred => {
                let id = WredId::try_from(id)?;
                cx.store.ensure_removable::<Wred>(id)?;
                if let Some(hw) = cx.store.get::<Wred>(id)?.hw {
                    cx.backend.wred_destroy(hw)?;
                }
                cx.store.remove::<Wred>(id).map(drop)
            }
            ObjectType::BufferPool => cx.store.remove::<BufferPool>(BufferPoolId::try_from(id)?).map(drop),
            ObjectType::BufferProfile => cx
                .store
                .remove::<BufferProfile>(BufferProfileId::try_from(id)?)
                .map(drop),
            _ => Err(Error::NotSupported),
        }
    }

    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        match object_type {
            ObjectType::QosMap => set_qos_map(cx.backend, cx.store, QosMapId::try_from(id)?, attr),
            ObjectType::Scheduler => set_scheduler(cx.backend, cx.store, SchedulerId::try_from(id)?, attr),
            ObjectType::SchedulerGroup => set_scheduler_group(cx.backend, cx.store, id, attr),
            ObjectType::Wred => set_wred(cx.backend, cx.store, WredId::try_from(id)?, attr),
            ObjectType::BufferPool => set_buffer_pool(cx.backend, cx.store, BufferPoolId::try_from(id)?, attr),
            ObjectType::BufferProfile => set_buffer_profile(cx.backend, cx.store, BufferProfileId::try_from(id)?, attr),
            ObjectType::Queue => set_queue(cx.backend, cx.store, id, attr),
            ObjectType::IngressPriorityGroup => set_priority_group(cx.backend, cx.store, id, attr),
            _ => Err(not_supported(0)),
        }
    }

    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        let attr = meta.id;
        match object_type {
            ObjectType::QosMap => get_qos_map(store.get::<QosMap>(QosMapId::try_from(id)?)?, attr),
            ObjectType::Scheduler => get_scheduler(store.get::<Scheduler>(SchedulerId::try_from(id)?)?, attr),
            ObjectType::SchedulerGroup => get_scheduler_group(store, id, attr),
            ObjectType::Wred => get_wred(store.get::<Wred>(WredId::try_from(id)?)?, attr),
            ObjectType::BufferPool => get_buffer_pool(store.get::<BufferPool>(BufferPoolId::try_from(id)?)?, attr),
            ObjectType::BufferProfile => {
                get_buffer_profile(store, store.get::<BufferProfile>(BufferProfileId::try_from(id)?)?, attr)
            }
            ObjectType::Queue => get_queue(backend, store, id, attr),
            ObjectType::IngressPriorityGroup => get_priority_group(store, id, attr),
            _ => Err(not_supported(0)),
        }
    }
}

// ------------------------------------------------------------------ QoS maps

/// Every map kind: its SAI type, the port attribute that binds one, the switch attribute whose map
/// ports without their own use (seven kinds have one).
const MAP_KINDS: [(
    QosMapKind,
    sys::sai_qos_map_type_t,
    sys::sai_attr_id_t,
    Option<sys::sai_attr_id_t>,
); 10] = [
    (
        QosMapKind::DscpToTc,
        sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC,
        sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP,
        Some(sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP),
    ),
    (
        QosMapKind::Dot1pToTc,
        sys::SAI_QOS_MAP_TYPE_DOT1P_TO_TC,
        sys::SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP,
        Some(sys::SAI_SWITCH_ATTR_QOS_DOT1P_TO_TC_MAP),
    ),
    (
        QosMapKind::TcToQueue,
        sys::SAI_QOS_MAP_TYPE_TC_TO_QUEUE,
        sys::SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP,
        Some(sys::SAI_SWITCH_ATTR_QOS_TC_TO_QUEUE_MAP),
    ),
    (
        QosMapKind::TcToPg,
        sys::SAI_QOS_MAP_TYPE_TC_TO_PRIORITY_GROUP,
        sys::SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP,
        None,
    ),
    (
        QosMapKind::PfcPrioToQueue,
        sys::SAI_QOS_MAP_TYPE_PFC_PRIORITY_TO_QUEUE,
        sys::SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_QUEUE_MAP,
        None,
    ),
    (
        QosMapKind::PfcPrioToPg,
        sys::SAI_QOS_MAP_TYPE_PFC_PRIORITY_TO_PRIORITY_GROUP,
        sys::SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_PRIORITY_GROUP_MAP,
        None,
    ),
    (
        QosMapKind::TcColorToDscp,
        sys::SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP,
        sys::SAI_PORT_ATTR_QOS_TC_AND_COLOR_TO_DSCP_MAP,
        Some(sys::SAI_SWITCH_ATTR_QOS_TC_AND_COLOR_TO_DSCP_MAP),
    ),
    (
        QosMapKind::TcColorToDot1p,
        sys::SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DOT1P,
        sys::SAI_PORT_ATTR_QOS_TC_AND_COLOR_TO_DOT1P_MAP,
        Some(sys::SAI_SWITCH_ATTR_QOS_TC_AND_COLOR_TO_DOT1P_MAP),
    ),
    (
        QosMapKind::DscpToColor,
        sys::SAI_QOS_MAP_TYPE_DSCP_TO_COLOR,
        sys::SAI_PORT_ATTR_QOS_DSCP_TO_COLOR_MAP,
        Some(sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_COLOR_MAP),
    ),
    (
        QosMapKind::Dot1pToColor,
        sys::SAI_QOS_MAP_TYPE_DOT1P_TO_COLOR,
        sys::SAI_PORT_ATTR_QOS_DOT1P_TO_COLOR_MAP,
        Some(sys::SAI_SWITCH_ATTR_QOS_DOT1P_TO_COLOR_MAP),
    ),
];

/// The kind of a SAI map type (C++ `map_kind`; `None`: a type the backends have no table for).
fn map_kind(sai_type: i32) -> Option<QosMapKind> {
    MAP_KINDS
        .iter()
        .find(|(_, t, _, _)| t.cast_signed() == sai_type)
        .map(|(k, ..)| *k)
}

fn map_sai_type(kind: QosMapKind) -> i32 {
    MAP_KINDS
        .iter()
        .find(|(k, ..)| *k == kind)
        .map_or(0, |(_, t, _, _)| t.cast_signed())
}

/// The kind a port attribute binds (C++ `port_map_attr`).
fn port_map_kind(attr: sys::sai_attr_id_t) -> Option<QosMapKind> {
    MAP_KINDS.iter().find(|(_, _, a, _)| *a == attr).map(|(k, ..)| *k)
}

fn port_map_attr(kind: QosMapKind) -> Option<sys::sai_attr_id_t> {
    MAP_KINDS.iter().find(|(k, ..)| *k == kind).map(|(_, _, a, _)| *a)
}

/// The kind a switch attribute binds (C++ `switch_map_attr`).
fn switch_map_kind(attr: sys::sai_attr_id_t) -> Option<QosMapKind> {
    MAP_KINDS.iter().find(|(_, _, _, a)| *a == Some(attr)).map(|(k, ..)| *k)
}

fn switch_map_attr(kind: QosMapKind) -> Option<sys::sai_attr_id_t> {
    MAP_KINDS.iter().find(|(k, ..)| *k == kind).and_then(|(_, _, _, a)| *a)
}

/// One SAI entry as the backend's key / value for a map kind (C++ `entry_kv`); `None`: out of
/// range. Colours are compared as the C++'s `uint32_t` copies of the `int32_t`.
fn entry_kv(kind: QosMapKind, m: &QosMapEntry) -> Option<HwMapEntry> {
    let (k, v) = (&m.key, &m.value);
    let entry = |key: u8, key_color: i32, value: u8, value_color: i32| HwMapEntry {
        key: u32::from(key),
        key_color: key_color.cast_unsigned(),
        value: u32::from(value),
        value_color: value_color.cast_unsigned(),
    };
    let (e, in_range) = match kind {
        QosMapKind::DscpToTc => (entry(k.dscp, 0, v.tc, 0), k.dscp < 64 && v.tc < 16),
        QosMapKind::DscpToColor => (
            entry(k.dscp, 0, 0, v.color),
            k.dscp < 64 && v.color.cast_unsigned() <= 2,
        ),
        QosMapKind::Dot1pToTc => (entry(k.dot1p, 0, v.tc, 0), k.dot1p < 8 && v.tc < 16),
        QosMapKind::Dot1pToColor => (
            entry(k.dot1p, 0, 0, v.color),
            k.dot1p < 8 && v.color.cast_unsigned() <= 2,
        ),
        QosMapKind::TcToQueue => (entry(k.tc, 0, v.queue_index, 0), k.tc < 16 && v.queue_index < 8),
        QosMapKind::TcToPg => (entry(k.tc, 0, v.pg, 0), k.tc < 16 && v.pg < 8),
        QosMapKind::PfcPrioToQueue => (entry(k.prio, 0, v.queue_index, 0), k.prio < 8 && v.queue_index < 8),
        QosMapKind::PfcPrioToPg => (entry(k.prio, 0, v.pg, 0), k.prio < 8 && v.pg < 8),
        QosMapKind::TcColorToDscp => (
            entry(k.tc, k.color, v.dscp, 0),
            k.tc < 16 && k.color.cast_unsigned() <= 2 && v.dscp < 64,
        ),
        QosMapKind::TcColorToDot1p => (
            entry(k.tc, k.color, v.dot1p, 0),
            k.tc < 16 && k.color.cast_unsigned() <= 2 && v.dot1p < 8,
        ),
    };
    in_range.then_some(e)
}

/// The complete table the backend gets: every key (times colour for the colour-keyed kinds),
/// unlisted keys 0 / green (C++ `full_entries`); `None`: an entry out of range. A traffic class
/// 8..15 as a key is dropped (no such queue / priority group here).
fn full_entries(kind: QosMapKind, list: &[QosMapEntry]) -> Option<Vec<HwMapEntry>> {
    let keys: u32 = if matches!(kind, QosMapKind::DscpToTc | QosMapKind::DscpToColor) {
        64
    } else {
        8
    };
    let colours: u32 = if matches!(kind, QosMapKind::TcColorToDscp | QosMapKind::TcColorToDot1p) {
        3
    } else {
        1
    };
    let mut out: Vec<HwMapEntry> = (0..keys * colours)
        .map(|i| HwMapEntry {
            key: i / colours,
            key_color: i % colours,
            value: 0,
            value_color: 0,
        })
        .collect();
    for m in list {
        let e = entry_kv(kind, m)?;
        if e.key >= keys {
            continue;
        }
        let slot = usize::try_from(e.key * colours + e.key_color)
            .ok()
            .and_then(|i| out.get_mut(i))?;
        slot.value = e.value;
        slot.value_color = e.value_color;
    }
    Some(out)
}

/// A `QOS_MAP` (C++ `QosMapObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QosMap {
    kind: QosMapKind,
    list: Vec<QosMapEntry>,
    hw: Option<HwMapId>,
}

impl QosMap {
    /// What the map maps.
    pub const fn kind(&self) -> QosMapKind {
        self.kind
    }
    /// The entries as the user gave them.
    pub fn list(&self) -> &[QosMapEntry] {
        &self.list
    }
    /// The backend's map, `None` while it is kept only (created without `QoS` in the backend).
    pub const fn hw(&self) -> Option<HwMapId> {
        self.hw
    }
}

area_kind!(QosMap, QosMapId, Qos, QosObject::Map);

fn create_qos_map<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let mut sai_type = None;
    let mut list = None;
    for (index, attr) in attrs.iter().enumerate() {
        create_arg(ObjectType::QosMap, attr, index)?;
        match (attr.id, &attr.value) {
            (sys::SAI_QOS_MAP_ATTR_TYPE, Value::Enum(t)) => sai_type = Some(*t),
            (sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST, Value::QosMapList(l)) => list = Some(l.clone()),
            _ => return Err(unknown(index)),
        }
    }
    let (Some(sai_type), Some(list)) = (sai_type, list) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    let kind = map_kind(sai_type).ok_or(Error::NotSupported)?;
    // The C++ names attribute 0 whatever the list's position.
    let full = full_entries(kind, &list).ok_or(invalid(0))?;
    let hw = if backend.qos_supported() {
        Some(backend.qos_map_create(kind, &full)?)
    } else {
        None
    };
    let entries = list.len();
    let id = store.insert(QosMap { kind, list, hw }).map(TypedId::raw)?;
    log::info(
        sys::SAI_API_QOS_MAP,
        format_args!(
            "qos map 0x{:x} type {sai_type}, {entries} entries, backend id {}",
            id.0,
            hw.map_or(-1, |h| i64::from(h.0))
        ),
    );
    Ok(id)
}

fn set_qos_map<B: Backend>(backend: &mut B, store: &mut Store, id: QosMapId, attr: &Attr) -> Result<()> {
    let map = store.get::<QosMap>(id)?;
    let list = match (attr.id, &attr.value) {
        (sys::SAI_QOS_MAP_ATTR_TYPE, _) => return Err(invalid(0)), // create only
        (sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST, Value::QosMapList(l)) => l.clone(),
        _ => return Err(unknown(0)),
    };
    let full = full_entries(map.kind, &list).ok_or(invalid(0))?;
    if let Some(hw) = map.hw {
        backend
            .qos_map_update(hw, &full)
            .map_err(|e| backend_attr_error(e, 0))?;
    }
    store.update::<QosMap, _>(id, |m| m.list = list)
}

fn get_qos_map(map: &QosMap, attr: sys::sai_attr_id_t) -> Result<Value> {
    match attr {
        sys::SAI_QOS_MAP_ATTR_TYPE => Ok(Value::Enum(map_sai_type(map.kind))),
        sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST => Ok(Value::QosMapList(map.list.clone())),
        _ => Err(unknown(0)),
    }
}

/// The backend's id of a map (`None`: no map, or one kept only).
fn map_hw(store: &Store, map: RawOid) -> Option<HwMapId> {
    lookup::<QosMap>(store, map).and_then(|m| m.hw)
}

/// Whether `map` is null or a map of `kind` (the check of a port or switch binding).
fn map_fits(store: &Store, map: RawOid, kind: QosMapKind) -> bool {
    map.is_null() || lookup::<QosMap>(store, map).is_some_and(|m| m.kind == kind)
}

/// The switch's map of a kind (null: none, or a kind the switch has no attribute for).
fn switch_map(store: &Store, kind: QosMapKind) -> RawOid {
    match (the_switch(store), switch_map_attr(kind)) {
        (Some(switch), Some(attr)) => kept_oid(&switch.kept, attr),
        _ => RawOid::NULL,
    }
}

fn own_map(port: &Port, kind: QosMapKind) -> RawOid {
    port_map_attr(kind).map_or(RawOid::NULL, |attr| kept_oid(&port.kept, attr))
}

/// Programs the map a port uses for a kind: its own (`own`), else the switch's (C++
/// `apply_port_map`). Nothing without `QoS` in the backend, nothing for the CPU port.
fn apply_port_map<B: Backend>(
    backend: &mut B,
    store: &Store,
    port: &Port,
    kind: QosMapKind,
    own: RawOid,
    switch: RawOid,
) -> Result<()> {
    let Some(hw) = front(port).filter(|_| backend.qos_supported()) else {
        return Ok(());
    };
    let map = if own.is_null() { switch } else { own };
    backend.port_qos_map_set(hw, kind, map_hw(store, map))
}

// ------------------------------------------------------------------ port and switch attributes

/// The two PFC modes (`SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PfcMode {
    Combined,
    Separate,
}

impl PfcMode {
    const fn from_sai(v: i32) -> Option<Self> {
        match v {
            v if v == sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_COMBINED.cast_signed() => Some(Self::Combined),
            v if v == sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE.cast_signed() => Some(Self::Separate),
            _ => None,
        }
    }

    fn of(kept: &Kept) -> Self {
        match kept.get(&sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE) {
            Some(Value::Enum(v)) => Self::from_sai(*v).unwrap_or(Self::Combined),
            _ => Self::Combined,
        }
    }

    const fn sai(self) -> i32 {
        match self {
            Self::Combined => sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_COMBINED.cast_signed(),
            Self::Separate => sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE.cast_signed(),
        }
    }
}

/// The port's `QoS` attributes (C++ `port_qos_set`): maps, default TC, PFC, the port shaper,
/// buffer profile lists. `None`: not a `QoS` attribute, the caller goes on (storm, mirror, ...).
/// The attribute was checked against the metadata and its object ids name live objects.
pub(crate) fn port_qos_set<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    port: PortId,
    attr: &Attr,
) -> Option<Result<()>> {
    if let Some(kind) = port_map_kind(attr.id) {
        return Some(set_port_map(backend, store, port, kind, attr));
    }
    Some(match attr.id {
        sys::SAI_PORT_ATTR_QOS_DEFAULT_TC => set_default_tc(backend, store, port, attr),
        sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL
        | sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX
        | sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX
        | sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE => set_pfc(backend, store, port, attr),
        sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID => set_port_scheduler(backend, store, port, attr),
        sys::SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST => set_buffer_list(store, port, attr, PoolType::Ingress),
        sys::SAI_PORT_ATTR_QOS_EGRESS_BUFFER_PROFILE_LIST => set_buffer_list(store, port, attr, PoolType::Egress),
        _ => return None,
    })
}

fn set_port_map<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    port: PortId,
    kind: QosMapKind,
    attr: &Attr,
) -> Result<()> {
    let map = oid_of(attr)?;
    if !map_fits(store, map, kind) {
        return Err(invalid(0));
    }
    let p = store.get::<Port>(port)?;
    apply_port_map(backend, store, p, kind, map, switch_map(store, kind)).map_err(|e| backend_attr_error(e, 0))?;
    keep_on_port(store, port, attr)
}

fn set_default_tc<B: Backend>(backend: &mut B, store: &mut Store, port: PortId, attr: &Attr) -> Result<()> {
    let Value::U8(tc) = attr.value else {
        return Err(Error::Failure);
    };
    if tc > 15 {
        return Err(invalid(0));
    }
    if let Some(hw) = front(store.get::<Port>(port)?).filter(|_| backend.qos_supported()) {
        backend
            .port_default_tc_set(hw, tc)
            .map_err(|e| backend_attr_error(e, 0))?;
    }
    keep_on_port(store, port, attr)
}

fn set_pfc<B: Backend>(backend: &mut B, store: &mut Store, port: PortId, attr: &Attr) -> Result<()> {
    let p = store.get::<Port>(port)?;
    let mut pfc = kept_u8(&p.kept, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL);
    let mut tx = kept_u8(&p.kept, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX);
    let mut rx = kept_u8(&p.kept, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX);
    let mut mode = PfcMode::of(&p.kept);
    match (attr.id, &attr.value) {
        (sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, Value::U8(v)) => pfc = *v,
        (sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX, Value::U8(v)) => tx = *v,
        (sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX, Value::U8(v)) => rx = *v,
        (sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE, Value::Enum(v)) => {
            mode = PfcMode::from_sai(*v).ok_or(invalid(0))?;
        }
        _ => return Err(Error::Failure),
    }
    if let Some(hw) = front(p).filter(|_| backend.qos_supported()) {
        let separate = mode == PfcMode::Separate;
        let (tx, rx) = if separate { (tx, rx) } else { (pfc, pfc) };
        backend
            .port_pfc_set(hw, separate, tx, rx)
            .map_err(|e| backend_attr_error(e, 0))?;
    }
    keep_on_port(store, port, attr)
}

fn set_port_scheduler<B: Backend>(backend: &mut B, store: &mut Store, port: PortId, attr: &Attr) -> Result<()> {
    let sched = oid_of(attr)?;
    if !sched.is_null() && lookup::<Scheduler>(store, sched).is_none() {
        return Err(invalid(0));
    }
    let p = store.get::<Port>(port)?;
    apply_port_sched(backend, store, p, sched, Pending::Nothing).map_err(|e| backend_attr_error(e, 0))?;
    keep_on_port(store, port, attr)
}

/// Kept: per-port pool limits are not programmed (the queue / PG profiles are). Every profile
/// must be one of a pool of the list's direction.
fn set_buffer_list(store: &mut Store, port: PortId, attr: &Attr, direction: PoolType) -> Result<()> {
    let Value::OidList(list) = &attr.value else {
        return Err(Error::Failure);
    };
    let fits = |b: &RawOid| {
        lookup::<BufferProfile>(store, *b)
            .and_then(|p| store.get::<BufferPool>(p.pool).ok())
            .is_some_and(|pool| pool.kind == direction)
    };
    if !list.iter().all(fits) {
        return Err(invalid(0));
    }
    keep_on_port(store, port, attr)
}

/// The scheduler groups of a port (one per queue, in queue order).
fn port_groups(store: &Store, port: PortId) -> Vec<RawOid> {
    store
        .bags(ObjectType::SchedulerGroup)
        .filter(|(_, g)| g.attrs.get(&sys::SAI_SCHEDULER_GROUP_ATTR_PORT_ID) == Some(&Value::Oid(port.raw())))
        .map(|(id, _)| id)
        .collect()
}

/// Gets of the port's `QoS` attributes (C++ `port_qos_get`). `None`: not one. The scheduler
/// groups exist for every backend; one without `QoS` shows none (as before they existed).
pub(crate) fn port_qos_get<B: Backend>(
    backend: &B,
    store: &Store,
    port: PortId,
    attr: sys::sai_attr_id_t,
) -> Option<Result<Value>> {
    let p = match store.get::<Port>(port) {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    if port_map_kind(attr).is_some() {
        return Some(Ok(Value::Oid(kept_oid(&p.kept, attr))));
    }
    Some(match attr {
        sys::SAI_PORT_ATTR_QOS_DEFAULT_TC
        | sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL
        | sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX
        | sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX => Ok(Value::U8(kept_u8(&p.kept, attr))),
        sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE => Ok(Value::Enum(PfcMode::of(&p.kept).sai())),
        sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID => Ok(Value::Oid(kept_oid(&p.kept, attr))),
        sys::SAI_PORT_ATTR_QOS_NUMBER_OF_SCHEDULER_GROUPS => {
            if backend.qos_supported() {
                count(port_groups(store, port).len())
            } else {
                Ok(Value::U32(0))
            }
        }
        sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST => Ok(Value::OidList(if backend.qos_supported() {
            port_groups(store, port)
        } else {
            Vec::new()
        })),
        sys::SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST | sys::SAI_PORT_ATTR_QOS_EGRESS_BUFFER_PROFILE_LIST => {
            Ok(Value::OidList(kept_list(&p.kept, attr)))
        }
        _ => return None,
    })
}

/// The switch's `QOS_*_MAP` attributes (C++ `switch_qos_set`): every port without its own map of
/// the kind follows; on a failure the old map goes back on every such port. `None`: not one.
pub(crate) fn switch_qos_set<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    switch: SwitchId,
    attr: &Attr,
) -> Option<Result<()>> {
    let kind = switch_map_kind(attr.id)?;
    Some(set_switch_map(backend, store, switch, kind, attr))
}

fn set_switch_map<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    switch: SwitchId,
    kind: QosMapKind,
    attr: &Attr,
) -> Result<()> {
    let map = oid_of(attr)?;
    if !map_fits(store, map, kind) {
        return Err(invalid(0));
    }
    let old = switch_map(store, kind);
    let followers: Vec<&Port> = store
        .iter::<Port>()
        .map(|(_, p)| p)
        .filter(|p| front(p).is_some() && own_map(p, kind).is_null())
        .collect();
    for p in &followers {
        if let Err(e) = apply_port_map(backend, store, p, kind, RawOid::NULL, map) {
            for q in &followers {
                // Best effort, as the C++: the first error is the answer.
                let _ = apply_port_map(backend, store, q, kind, RawOid::NULL, old);
            }
            return Err(backend_attr_error(e, 0));
        }
    }
    store.update::<Switch, _>(switch, |s| drop(s.kept.insert(attr.id, attr.value.clone())))
}

/// Gets of the switch's `QOS_*_MAP` attributes (C++ `switch_qos_get`). `None`: not one.
pub(crate) fn switch_qos_get(store: &Store, switch: SwitchId, attr: sys::sai_attr_id_t) -> Option<Result<Value>> {
    switch_map_kind(attr)?;
    Some(store.get::<Switch>(switch).map(|s| Value::Oid(kept_oid(&s.kept, attr))))
}

/// What a set of an attribute the metadata does not let a set carry (create-only, read-only)
/// answers for this area's types and the mirror area's, as the C++ (the rim refuses those before
/// decoding the value; [`crate::sai::Sai::refused_set`]). `None`: another type.
pub(crate) fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
    Some(match (object_type, attr) {
        (ObjectType::QosMap, sys::SAI_QOS_MAP_ATTR_TYPE)
        | (ObjectType::BufferPool, sys::SAI_BUFFER_POOL_ATTR_TYPE | sys::SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE)
        | (
            ObjectType::BufferProfile,
            sys::SAI_BUFFER_PROFILE_ATTR_POOL_ID | sys::SAI_BUFFER_PROFILE_ATTR_THRESHOLD_MODE,
        ) => invalid(0),
        (ObjectType::QosMap | ObjectType::Scheduler, _) => unknown(0),
        (
            ObjectType::SchedulerGroup
            | ObjectType::Queue
            | ObjectType::IngressPriorityGroup
            | ObjectType::Wred
            | ObjectType::BufferPool
            | ObjectType::BufferProfile,
            _,
        ) => not_supported(0),
        (ObjectType::MirrorSession | ObjectType::Samplepacket, _) => {
            return crate::mirror::refused_set(object_type, attr);
        }
        _ => return None,
    })
}

/// `SAI_PORT_STAT_PFC_<n>_{RX,TX}_PKTS` as (priority, tx) (C++ `port_pfc_stat`).
pub(crate) fn port_pfc_stat(id: sys::sai_stat_id_t) -> Option<(u8, bool)> {
    const RX: [sys::sai_port_stat_t; 8] = [
        sys::SAI_PORT_STAT_PFC_0_RX_PKTS,
        sys::SAI_PORT_STAT_PFC_1_RX_PKTS,
        sys::SAI_PORT_STAT_PFC_2_RX_PKTS,
        sys::SAI_PORT_STAT_PFC_3_RX_PKTS,
        sys::SAI_PORT_STAT_PFC_4_RX_PKTS,
        sys::SAI_PORT_STAT_PFC_5_RX_PKTS,
        sys::SAI_PORT_STAT_PFC_6_RX_PKTS,
        sys::SAI_PORT_STAT_PFC_7_RX_PKTS,
    ];
    const TX: [sys::sai_port_stat_t; 8] = [
        sys::SAI_PORT_STAT_PFC_0_TX_PKTS,
        sys::SAI_PORT_STAT_PFC_1_TX_PKTS,
        sys::SAI_PORT_STAT_PFC_2_TX_PKTS,
        sys::SAI_PORT_STAT_PFC_3_TX_PKTS,
        sys::SAI_PORT_STAT_PFC_4_TX_PKTS,
        sys::SAI_PORT_STAT_PFC_5_TX_PKTS,
        sys::SAI_PORT_STAT_PFC_6_TX_PKTS,
        sys::SAI_PORT_STAT_PFC_7_TX_PKTS,
    ];
    (0u8..).zip(RX.iter().zip(TX.iter())).find_map(|(prio, (rx, tx))| {
        if *rx == id {
            Some((prio, false))
        } else if *tx == id {
            Some((prio, true))
        } else {
            None
        }
    })
}

// ------------------------------------------------------------------ schedulers

/// `SAI_SCHEDULING_TYPE_*`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheduling {
    /// Strict priority.
    Strict,
    /// Weighted round robin.
    Wrr,
    /// Deficit weighted round robin (the C++ default).
    Dwrr,
}

/// `SAI_METER_TYPE_*` of a scheduler's rates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meter {
    /// Packets per second.
    Packets,
    /// Bytes per second (the default).
    Bytes,
}

/// A `SCHEDULER` profile (C++ `SchedulerObj`, with its defaults: DWRR, weight 1, bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scheduler {
    scheduling: Scheduling,
    weight: u8,
    meter: Meter,
    min_rate: u64,
    min_burst: u64,
    max_rate: u64,
    max_burst: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            scheduling: Scheduling::Dwrr,
            weight: 1,
            meter: Meter::Bytes,
            min_rate: 0,
            min_burst: 0,
            max_rate: 0,
            max_burst: 0,
        }
    }
}

area_kind!(Scheduler, SchedulerId, Qos, QosObject::Scheduler);

impl Scheduler {
    /// What the chip gets (C++ `sched_cfg`).
    pub fn cfg(&self) -> SchedCfg {
        SchedCfg {
            strict: self.scheduling == Scheduling::Strict,
            weight: u32::from(self.weight),
            packets: self.meter == Meter::Packets,
            min_rate: self.min_rate,
            min_burst: self.min_burst,
            max_rate: self.max_rate,
            max_burst: self.max_burst,
        }
    }

    /// One attribute (C++ `sched_attr`).
    fn apply(&mut self, attr: &Attr, index: usize) -> Result<()> {
        match (attr.id, &attr.value) {
            (sys::SAI_SCHEDULER_ATTR_SCHEDULING_TYPE, Value::Enum(t)) => {
                self.scheduling = match *t {
                    t if t == sys::SAI_SCHEDULING_TYPE_STRICT.cast_signed() => Scheduling::Strict,
                    t if t == sys::SAI_SCHEDULING_TYPE_WRR.cast_signed() => Scheduling::Wrr,
                    t if t == sys::SAI_SCHEDULING_TYPE_DWRR.cast_signed() => Scheduling::Dwrr,
                    _ => return Err(invalid(index)),
                };
            }
            (sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, Value::U8(w)) => {
                if !(1..=100).contains(w) {
                    return Err(invalid(index));
                }
                self.weight = *w;
            }
            (sys::SAI_SCHEDULER_ATTR_METER_TYPE, Value::Enum(m)) => {
                self.meter = match *m {
                    m if m == sys::SAI_METER_TYPE_PACKETS.cast_signed() => Meter::Packets,
                    m if m == sys::SAI_METER_TYPE_BYTES.cast_signed() => Meter::Bytes,
                    _ => return Err(invalid(index)),
                };
            }
            (sys::SAI_SCHEDULER_ATTR_MIN_BANDWIDTH_RATE, Value::U64(v)) => self.min_rate = *v,
            (sys::SAI_SCHEDULER_ATTR_MIN_BANDWIDTH_BURST_RATE, Value::U64(v)) => self.min_burst = *v,
            (sys::SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE, Value::U64(v)) => self.max_rate = *v,
            (sys::SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_BURST_RATE, Value::U64(v)) => self.max_burst = *v,
            _ => return Err(unknown(index)),
        }
        Ok(())
    }
}

fn create_scheduler(store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let mut s = Scheduler::default();
    for (index, attr) in attrs.iter().enumerate() {
        create_arg(ObjectType::Scheduler, attr, index)?;
        s.apply(attr, index)?;
    }
    store.insert(s).map(TypedId::raw)
}

fn get_scheduler(s: &Scheduler, attr: sys::sai_attr_id_t) -> Result<Value> {
    Ok(match attr {
        sys::SAI_SCHEDULER_ATTR_SCHEDULING_TYPE => Value::Enum(
            match s.scheduling {
                Scheduling::Strict => sys::SAI_SCHEDULING_TYPE_STRICT,
                Scheduling::Wrr => sys::SAI_SCHEDULING_TYPE_WRR,
                Scheduling::Dwrr => sys::SAI_SCHEDULING_TYPE_DWRR,
            }
            .cast_signed(),
        ),
        sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT => Value::U8(s.weight),
        sys::SAI_SCHEDULER_ATTR_METER_TYPE => Value::Enum(
            match s.meter {
                Meter::Packets => sys::SAI_METER_TYPE_PACKETS,
                Meter::Bytes => sys::SAI_METER_TYPE_BYTES,
            }
            .cast_signed(),
        ),
        sys::SAI_SCHEDULER_ATTR_MIN_BANDWIDTH_RATE => Value::U64(s.min_rate),
        sys::SAI_SCHEDULER_ATTR_MIN_BANDWIDTH_BURST_RATE => Value::U64(s.min_burst),
        sys::SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE => Value::U64(s.max_rate),
        sys::SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_BURST_RATE => Value::U64(s.max_burst),
        _ => return Err(unknown(0)),
    })
}

/// A queue's chip port and index.
fn queue_target(store: &Store, queue: &Bag) -> Result<(HwPort, u8)> {
    port_and_index(store, queue, sys::SAI_QUEUE_ATTR_PORT, sys::SAI_QUEUE_ATTR_INDEX)
}

/// A priority group's chip port and index.
fn pg_target(store: &Store, group: &Bag) -> Result<(HwPort, u8)> {
    port_and_index(
        store,
        group,
        sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT,
        sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_INDEX,
    )
}

fn port_and_index(
    store: &Store,
    bag: &Bag,
    port_attr: sys::sai_attr_id_t,
    index_attr: sys::sai_attr_id_t,
) -> Result<(HwPort, u8)> {
    let port = lookup::<Port>(store, kept_oid(&bag.attrs, port_attr)).ok_or(Error::InvalidObjectId)?;
    match bag.attrs.get(&index_attr) {
        Some(Value::U8(index)) => Ok((port.hw, *index)),
        _ => Err(Error::Failure),
    }
}

/// The port a queue or priority group belongs to.
fn bag_port<'a>(store: &'a Store, bag: &Bag, port_attr: sys::sai_attr_id_t) -> Option<&'a Port> {
    lookup::<Port>(store, kept_oid(&bag.attrs, port_attr))
}

/// A queue's scheduler group (null: none).
fn queue_group(queue: &Bag) -> RawOid {
    kept_oid(&queue.attrs, sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE)
}

/// A queue's scheduler: its group's, else its own (C++ `queue_sched`).
fn queue_sched(store: &Store, queue: &Bag) -> RawOid {
    let group = store.bag(queue_group(queue)).map_or(RawOid::NULL, |g| {
        kept_oid(&g.attrs, sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID)
    });
    if group.is_null() {
        kept_oid(&queue.attrs, sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID)
    } else {
        group
    }
}

/// Programs a queue's scheduler (`sched`: the one in effect, null for the default) (C++
/// `apply_queue_sched`).
fn apply_queue_sched<B: Backend>(
    backend: &mut B,
    store: &Store,
    queue: &Bag,
    sched: RawOid,
    pending: Pending<'_>,
) -> Result<()> {
    if !backend.qos_supported() {
        return Ok(());
    }
    let (hw, index) = queue_target(store, queue)?;
    let cfg = scheduler(store, sched, pending).map(Scheduler::cfg);
    backend.queue_scheduler_set(hw, index, cfg.as_ref())
}

/// Programs a port's shaper (C++ `apply_port_sched`); nothing for the CPU port.
fn apply_port_sched<B: Backend>(
    backend: &mut B,
    store: &Store,
    port: &Port,
    sched: RawOid,
    pending: Pending<'_>,
) -> Result<()> {
    let Some(hw) = front(port).filter(|_| backend.qos_supported()) else {
        return Ok(());
    };
    let cfg = scheduler(store, sched, pending).map(Scheduler::cfg);
    backend.port_scheduler_set(hw, cfg.as_ref())
}

/// A profile change: every queue and port shaper that uses it follows; on a failure the old
/// profile goes back everywhere (C++ `set_scheduler_attribute`).
fn set_scheduler<B: Backend>(backend: &mut B, store: &mut Store, id: SchedulerId, attr: &Attr) -> Result<()> {
    let mut changed = store.get::<Scheduler>(id)?.clone();
    changed.apply(attr, 0)?;
    let sched = id.raw();
    let queues: Vec<&Bag> = store
        .bags(ObjectType::Queue)
        .map(|(_, q)| q)
        .filter(|q| queue_sched(store, q) == sched)
        .collect();
    let shaped: Vec<&Port> = store
        .iter::<Port>()
        .map(|(_, p)| p)
        .filter(|p| kept_oid(&p.kept, sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID) == sched)
        .collect();
    let pending = Pending::Scheduler(id, &changed);
    let mut result = Ok(());
    for q in &queues {
        result = apply_queue_sched(backend, store, q, sched, pending);
        if result.is_err() {
            break;
        }
    }
    for p in &shaped {
        if result.is_ok() {
            result = apply_port_sched(backend, store, p, sched, pending);
        }
    }
    if let Err(e) = result {
        for q in &queues {
            let _ = apply_queue_sched(backend, store, q, sched, Pending::Nothing);
        }
        for p in &shaped {
            let _ = apply_port_sched(backend, store, p, sched, Pending::Nothing);
        }
        return Err(backend_attr_error(e, 0));
    }
    store.update::<Scheduler, _>(id, |s| *s = changed)
}

// ------------------------------------------------------------------ scheduler groups

/// The queues whose group `group` is (one).
fn group_children(store: &Store, group: RawOid) -> Vec<RawOid> {
    store
        .bags(ObjectType::Queue)
        .filter(|(_, q)| queue_group(q) == group)
        .map(|(id, _)| id)
        .collect()
}

/// `PARENT_NODE` (only to what it is) and `SCHEDULER_PROFILE_ID` (the queue follows; on a failure
/// it gets its old scheduler back) (C++ `set_scheduler_group_attribute`).
fn set_scheduler_group<B: Backend>(backend: &mut B, store: &mut Store, group: RawOid, attr: &Attr) -> Result<()> {
    let g = store.bag(group)?;
    match (attr.id, &attr.value) {
        (sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE, Value::Oid(parent)) => {
            if *parent == kept_oid(&g.attrs, sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE) {
                Ok(())
            } else {
                Err(not_supported(0))
            }
        }
        (sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID, Value::Oid(sched)) => {
            if !sched.is_null() && lookup::<Scheduler>(store, *sched).is_none() {
                return Err(invalid(0));
            }
            for child in group_children(store, group) {
                let q = store.bag(child)?;
                let own = kept_oid(&q.attrs, sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID);
                let effective = if sched.is_null() { own } else { *sched };
                if let Err(e) = apply_queue_sched(backend, store, q, effective, Pending::Nothing) {
                    let _ = apply_queue_sched(backend, store, q, queue_sched(store, q), Pending::Nothing);
                    return Err(backend_attr_error(e, 0));
                }
            }
            store.update_bag(group, |attrs| drop(attrs.insert(attr.id, attr.value.clone())))
        }
        _ => Err(not_supported(0)),
    }
}

fn get_scheduler_group(store: &Store, group: RawOid, attr: sys::sai_attr_id_t) -> Result<Value> {
    let g = store.bag(group)?;
    let stored = |attr| g.attrs.get(&attr).cloned().ok_or(Error::Failure);
    match attr {
        sys::SAI_SCHEDULER_GROUP_ATTR_CHILD_COUNT => count(group_children(store, group).len()),
        sys::SAI_SCHEDULER_GROUP_ATTR_CHILD_LIST => Ok(Value::OidList(group_children(store, group))),
        sys::SAI_SCHEDULER_GROUP_ATTR_PORT_ID
        | sys::SAI_SCHEDULER_GROUP_ATTR_LEVEL
        | sys::SAI_SCHEDULER_GROUP_ATTR_MAX_CHILDS
        | sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE => stored(attr),
        sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID => Ok(Value::Oid(kept_oid(&g.attrs, attr))),
        _ => Err(unknown(0)),
    }
}

// ------------------------------------------------------------------ WRED

/// A `WRED` profile (C++ `WredObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wred {
    cfg: WredCfg,
    hw: Option<HwWredId>,
}

area_kind!(Wred, WredId, Qos, QosObject::Wred);

impl Wred {
    /// The profile.
    pub const fn cfg(&self) -> &WredCfg {
        &self.cfg
    }
    /// The backend's profile, `None` while it is kept only.
    pub const fn hw(&self) -> Option<HwWredId> {
        self.hw
    }
}

/// The C++ `WredCfg` defaults: off, thresholds 0, probability 100 %.
const WRED_DEFAULT: WredCfg = WredCfg {
    enable: [false; 3],
    min: [0; 3],
    max: [0; 3],
    prob: [100; 3],
    weight: 0,
    ecn: 0,
};

/// One attribute (C++ `wred_attr`): [green, yellow, red].
fn wred_attr(w: &mut WredCfg, attr: &Attr, index: usize) -> Result<()> {
    match (attr.id, &attr.value) {
        (sys::SAI_WRED_ATTR_GREEN_ENABLE, Value::Bool(v)) => w.enable[0] = *v,
        (sys::SAI_WRED_ATTR_GREEN_MIN_THRESHOLD, Value::U32(v)) => w.min[0] = *v,
        (sys::SAI_WRED_ATTR_GREEN_MAX_THRESHOLD, Value::U32(v)) => w.max[0] = *v,
        (sys::SAI_WRED_ATTR_GREEN_DROP_PROBABILITY, Value::U32(v)) => w.prob[0] = *v,
        (sys::SAI_WRED_ATTR_YELLOW_ENABLE, Value::Bool(v)) => w.enable[1] = *v,
        (sys::SAI_WRED_ATTR_YELLOW_MIN_THRESHOLD, Value::U32(v)) => w.min[1] = *v,
        (sys::SAI_WRED_ATTR_YELLOW_MAX_THRESHOLD, Value::U32(v)) => w.max[1] = *v,
        (sys::SAI_WRED_ATTR_YELLOW_DROP_PROBABILITY, Value::U32(v)) => w.prob[1] = *v,
        (sys::SAI_WRED_ATTR_RED_ENABLE, Value::Bool(v)) => w.enable[2] = *v,
        (sys::SAI_WRED_ATTR_RED_MIN_THRESHOLD, Value::U32(v)) => w.min[2] = *v,
        (sys::SAI_WRED_ATTR_RED_MAX_THRESHOLD, Value::U32(v)) => w.max[2] = *v,
        (sys::SAI_WRED_ATTR_RED_DROP_PROBABILITY, Value::U32(v)) => w.prob[2] = *v,
        (sys::SAI_WRED_ATTR_WEIGHT, Value::U8(v)) => {
            if *v > 15 {
                return Err(invalid(index));
            }
            w.weight = *v;
        }
        (sys::SAI_WRED_ATTR_ECN_MARK_MODE, Value::Enum(v)) => {
            let modes = sys::SAI_ECN_MARK_MODE_NONE.cast_signed()..=sys::SAI_ECN_MARK_MODE_ALL.cast_signed();
            if !modes.contains(v) {
                return Err(invalid(index));
            }
            w.ecn = *v;
        }
        _ => return Err(not_supported(index)),
    }
    if w.prob.iter().any(|p| *p > 100) {
        return Err(invalid(index));
    }
    Ok(())
}

/// An enabled colour's minimum is at most its maximum (C++ `wred_valid`).
fn wred_valid(w: &WredCfg) -> bool {
    w.enable
        .iter()
        .zip(w.min.iter().zip(w.max.iter()))
        .all(|(on, (min, max))| !on || min <= max)
}

fn create_wred<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let mut cfg = WRED_DEFAULT;
    for (index, attr) in attrs.iter().enumerate() {
        create_arg(ObjectType::Wred, attr, index)?;
        wred_attr(&mut cfg, attr, index)?;
    }
    if !wred_valid(&cfg) {
        return Err(Error::InvalidParameter);
    }
    let hw = if backend.qos_supported() {
        Some(backend.wred_create(&cfg)?)
    } else {
        None
    };
    store.insert(Wred { cfg, hw }).map(TypedId::raw)
}

fn set_wred<B: Backend>(backend: &mut B, store: &mut Store, id: WredId, attr: &Attr) -> Result<()> {
    let wred = store.get::<Wred>(id)?;
    let mut cfg = wred.cfg;
    wred_attr(&mut cfg, attr, 0)?;
    if !wred_valid(&cfg) {
        return Err(invalid(0));
    }
    if let Some(hw) = wred.hw {
        backend.wred_update(hw, &cfg).map_err(|e| backend_attr_error(e, 0))?;
    }
    store.update::<Wred, _>(id, |w| w.cfg = cfg)
}

fn get_wred(w: &Wred, attr: sys::sai_attr_id_t) -> Result<Value> {
    let c = &w.cfg;
    Ok(match attr {
        sys::SAI_WRED_ATTR_GREEN_ENABLE => Value::Bool(c.enable[0]),
        sys::SAI_WRED_ATTR_GREEN_MIN_THRESHOLD => Value::U32(c.min[0]),
        sys::SAI_WRED_ATTR_GREEN_MAX_THRESHOLD => Value::U32(c.max[0]),
        sys::SAI_WRED_ATTR_GREEN_DROP_PROBABILITY => Value::U32(c.prob[0]),
        sys::SAI_WRED_ATTR_YELLOW_ENABLE => Value::Bool(c.enable[1]),
        sys::SAI_WRED_ATTR_YELLOW_MIN_THRESHOLD => Value::U32(c.min[1]),
        sys::SAI_WRED_ATTR_YELLOW_MAX_THRESHOLD => Value::U32(c.max[1]),
        sys::SAI_WRED_ATTR_YELLOW_DROP_PROBABILITY => Value::U32(c.prob[1]),
        sys::SAI_WRED_ATTR_RED_ENABLE => Value::Bool(c.enable[2]),
        sys::SAI_WRED_ATTR_RED_MIN_THRESHOLD => Value::U32(c.min[2]),
        sys::SAI_WRED_ATTR_RED_MAX_THRESHOLD => Value::U32(c.max[2]),
        sys::SAI_WRED_ATTR_RED_DROP_PROBABILITY => Value::U32(c.prob[2]),
        sys::SAI_WRED_ATTR_WEIGHT => Value::U8(c.weight),
        sys::SAI_WRED_ATTR_ECN_MARK_MODE => Value::Enum(c.ecn),
        _ => return Err(not_supported(0)),
    })
}

/// Programs a queue's WRED profile (C++ `apply_queue_wred`).
fn apply_queue_wred<B: Backend>(backend: &mut B, store: &Store, queue: &Bag, wred: RawOid) -> Result<()> {
    if !backend.qos_supported() {
        return Ok(());
    }
    let (hw, index) = queue_target(store, queue)?;
    backend.queue_wred_set(hw, index, lookup::<Wred>(store, wred).and_then(|w| w.hw))
}

// ------------------------------------------------------------------ buffer pools and profiles

/// `SAI_BUFFER_POOL_TYPE_*` (the C++ takes ingress and egress).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolType {
    /// Ingress (priority groups).
    Ingress,
    /// Egress (queues).
    Egress,
}

/// `SAI_BUFFER_POOL_THRESHOLD_MODE_*` / `SAI_BUFFER_PROFILE_THRESHOLD_MODE_*` (the same numbers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThresholdMode {
    /// Static.
    Static,
    /// Dynamic.
    Dynamic,
}

impl ThresholdMode {
    const fn from_sai(v: i32) -> Option<Self> {
        match v {
            v if v == sys::SAI_BUFFER_POOL_THRESHOLD_MODE_STATIC.cast_signed() => Some(Self::Static),
            v if v == sys::SAI_BUFFER_POOL_THRESHOLD_MODE_DYNAMIC.cast_signed() => Some(Self::Dynamic),
            _ => None,
        }
    }

    const fn sai(self) -> i32 {
        match self {
            Self::Static => sys::SAI_BUFFER_POOL_THRESHOLD_MODE_STATIC.cast_signed(),
            Self::Dynamic => sys::SAI_BUFFER_POOL_THRESHOLD_MODE_DYNAMIC.cast_signed(),
        }
    }
}

/// A `BUFFER_POOL` (C++ `BufferPoolObj`; default ingress, dynamic).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferPool {
    kind: PoolType,
    size: u64,
    xoff: u64,
    mode: ThresholdMode,
}

area_kind!(BufferPool, BufferPoolId, Qos, QosObject::Pool);

impl BufferPool {
    /// Ingress or egress.
    pub const fn kind(&self) -> PoolType {
        self.kind
    }
    /// `SIZE` in bytes.
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// One attribute (C++ `pool_attr`).
    fn apply(&mut self, attr: &Attr, index: usize, create: bool) -> Result<()> {
        match (attr.id, &attr.value) {
            (sys::SAI_BUFFER_POOL_ATTR_TYPE, Value::Enum(t)) => {
                if !create {
                    return Err(invalid(index));
                }
                self.kind = match *t {
                    t if t == sys::SAI_BUFFER_POOL_TYPE_INGRESS.cast_signed() => PoolType::Ingress,
                    t if t == sys::SAI_BUFFER_POOL_TYPE_EGRESS.cast_signed() => PoolType::Egress,
                    _ => return Err(invalid(index)),
                };
            }
            (sys::SAI_BUFFER_POOL_ATTR_SIZE, Value::U64(v)) => self.size = *v,
            (sys::SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE, Value::Enum(m)) => {
                if !create {
                    return Err(invalid(index));
                }
                self.mode = ThresholdMode::from_sai(*m).ok_or(invalid(index))?;
            }
            (sys::SAI_BUFFER_POOL_ATTR_XOFF_SIZE, Value::U64(v)) => self.xoff = *v,
            _ => return Err(not_supported(index)),
        }
        Ok(())
    }
}

/// A `BUFFER_PROFILE` (C++ `BufferProfileObj`; no threshold mode of its own: the pool's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferProfile {
    pool: BufferPoolId,
    reserved: u64,
    static_th: u64,
    xoff: u64,
    xon: u64,
    xon_offset: u64,
    dynamic_th: i8,
    mode: Option<ThresholdMode>,
}

area_kind!(BufferProfile, BufferProfileId, Qos, QosObject::Profile);

impl BufferProfile {
    /// The pool (a reference the store counts).
    pub const fn pool(&self) -> BufferPoolId {
        self.pool
    }
}

/// The profile fields being built by a create or changed by a set (the pool may be missing
/// until the create's attributes are all read).
#[derive(Clone, Debug)]
struct ProfileArgs {
    pool: Option<BufferPoolId>,
    reserved: u64,
    static_th: u64,
    xoff: u64,
    xon: u64,
    xon_offset: u64,
    dynamic_th: i8,
    mode: Option<ThresholdMode>,
}

impl From<&BufferProfile> for ProfileArgs {
    fn from(p: &BufferProfile) -> Self {
        Self {
            pool: Some(p.pool),
            reserved: p.reserved,
            static_th: p.static_th,
            xoff: p.xoff,
            xon: p.xon,
            xon_offset: p.xon_offset,
            dynamic_th: p.dynamic_th,
            mode: p.mode,
        }
    }
}

impl ProfileArgs {
    /// One attribute (C++ `profile_attr`).
    fn apply(&mut self, store: &Store, attr: &Attr, index: usize, create: bool) -> Result<()> {
        match (attr.id, &attr.value) {
            (sys::SAI_BUFFER_PROFILE_ATTR_POOL_ID, Value::Oid(pool)) => {
                if !create {
                    return Err(invalid(index));
                }
                let id = BufferPoolId::try_from(*pool).map_err(|_| invalid(index))?;
                store.get::<BufferPool>(id).map_err(|_| invalid(index))?;
                self.pool = Some(id);
            }
            (sys::SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE, Value::U64(v)) => self.reserved = *v,
            (sys::SAI_BUFFER_PROFILE_ATTR_THRESHOLD_MODE, Value::Enum(m)) => {
                if !create {
                    return Err(invalid(index));
                }
                self.mode = Some(ThresholdMode::from_sai(*m).ok_or(invalid(index))?);
            }
            (sys::SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH, Value::I8(v)) => self.dynamic_th = *v,
            (sys::SAI_BUFFER_PROFILE_ATTR_SHARED_STATIC_TH, Value::U64(v)) => self.static_th = *v,
            (sys::SAI_BUFFER_PROFILE_ATTR_XOFF_TH, Value::U64(v)) => self.xoff = *v,
            (sys::SAI_BUFFER_PROFILE_ATTR_XON_TH, Value::U64(v)) => self.xon = *v,
            (sys::SAI_BUFFER_PROFILE_ATTR_XON_OFFSET_TH, Value::U64(v)) => self.xon_offset = *v,
            _ => return Err(not_supported(index)),
        }
        Ok(())
    }

    fn profile(self) -> Option<BufferProfile> {
        Some(BufferProfile {
            pool: self.pool?,
            reserved: self.reserved,
            static_th: self.static_th,
            xoff: self.xoff,
            xon: self.xon,
            xon_offset: self.xon_offset,
            dynamic_th: self.dynamic_th,
            mode: self.mode,
        })
    }
}

/// A profile with its pool's values, as the chip gets it (C++ `buffer_cfg`).
fn buffer_cfg(store: &Store, profile_id: RawOid, pending: Pending<'_>) -> Option<BufferCfg> {
    let p = profile(store, profile_id, pending)?;
    let pool = pool(store, p.pool.raw(), pending)?;
    Some(BufferCfg {
        ingress: pool.kind == PoolType::Ingress,
        dynamic: p.mode.unwrap_or(pool.mode) == ThresholdMode::Dynamic,
        dynamic_th: p.dynamic_th,
        static_th: p.static_th,
        reserved: p.reserved,
        xoff: p.xoff,
        xon: p.xon,
        xon_offset: p.xon_offset,
        pool_size: pool.size,
        pool_xoff: pool.xoff,
    })
}

/// Whether `profile_id` is a profile of a pool of `direction`.
fn profile_fits(store: &Store, profile_id: RawOid, direction: PoolType) -> bool {
    lookup::<BufferProfile>(store, profile_id)
        .and_then(|p| store.get::<BufferPool>(p.pool).ok())
        .is_some_and(|pool| pool.kind == direction)
}

/// Programs a queue's buffer profile (C++ `apply_queue_buffer`).
fn apply_queue_buffer<B: Backend>(
    backend: &mut B,
    store: &Store,
    queue: &Bag,
    profile_id: RawOid,
    pending: Pending<'_>,
) -> Result<()> {
    if !backend.qos_supported() {
        return Ok(());
    }
    let (hw, index) = queue_target(store, queue)?;
    let cfg = buffer_cfg(store, profile_id, pending);
    backend.queue_buffer_set(hw, index, cfg.as_ref())
}

/// Programs a priority group's buffer profile (C++ `apply_pg_buffer`).
fn apply_pg_buffer<B: Backend>(
    backend: &mut B,
    store: &Store,
    group: &Bag,
    profile_id: RawOid,
    pending: Pending<'_>,
) -> Result<()> {
    if !backend.qos_supported() {
        return Ok(());
    }
    let (hw, index) = pg_target(store, group)?;
    let cfg = buffer_cfg(store, profile_id, pending);
    backend.pg_buffer_set(hw, index, cfg.as_ref())
}

/// Which queues and priority groups a buffer change concerns.
#[derive(Clone, Copy, Debug)]
enum Uses {
    /// Those with this profile.
    Profile(RawOid),
    /// Those with any profile of this pool.
    Pool(BufferPoolId),
}

/// Re-programs every queue and priority group the change concerns, all of them even after a
/// failure; the first failure is the answer (C++ `reapply_buffers`).
fn reapply_buffers<B: Backend>(backend: &mut B, store: &Store, uses: Uses, pending: Pending<'_>) -> Result<()> {
    let concerned = |b: RawOid| {
        !b.is_null()
            && match uses {
                Uses::Profile(p) => b == p,
                Uses::Pool(pool) => lookup::<BufferProfile>(store, b).is_some_and(|p| p.pool == pool),
            }
    };
    let mut first = Ok(());
    for (_, q) in store.bags(ObjectType::Queue) {
        let b = kept_oid(&q.attrs, sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID);
        if concerned(b) {
            let result = apply_queue_buffer(backend, store, q, b, pending);
            if first.is_ok() {
                first = result;
            }
        }
    }
    for (_, g) in store.bags(ObjectType::IngressPriorityGroup) {
        let b = kept_oid(&g.attrs, sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE);
        if concerned(b) {
            let result = apply_pg_buffer(backend, store, g, b, pending);
            if first.is_ok() {
                first = result;
            }
        }
    }
    first
}

fn create_buffer_pool(store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let mut pool = BufferPool {
        kind: PoolType::Ingress,
        size: 0,
        xoff: 0,
        mode: ThresholdMode::Dynamic,
    };
    let (mut kind, mut size) = (false, false);
    for (index, attr) in attrs.iter().enumerate() {
        create_arg(ObjectType::BufferPool, attr, index)?;
        pool.apply(attr, index, true)?;
        kind |= attr.id == sys::SAI_BUFFER_POOL_ATTR_TYPE;
        size |= attr.id == sys::SAI_BUFFER_POOL_ATTR_SIZE;
    }
    if !kind || !size {
        return Err(Error::MandatoryAttributeMissing);
    }
    store.insert(pool).map(TypedId::raw)
}

fn set_buffer_pool<B: Backend>(backend: &mut B, store: &mut Store, id: BufferPoolId, attr: &Attr) -> Result<()> {
    let mut changed = store.get::<BufferPool>(id)?.clone();
    changed.apply(attr, 0, false)?;
    if let Err(e) = reapply_buffers(backend, store, Uses::Pool(id), Pending::Pool(id, &changed)) {
        let _ = reapply_buffers(backend, store, Uses::Pool(id), Pending::Nothing);
        return Err(backend_attr_error(e, 0));
    }
    store.update::<BufferPool, _>(id, |p| *p = changed)
}

fn get_buffer_pool(p: &BufferPool, attr: sys::sai_attr_id_t) -> Result<Value> {
    Ok(match attr {
        // Reservations are not subtracted (as the C++).
        sys::SAI_BUFFER_POOL_ATTR_SHARED_SIZE | sys::SAI_BUFFER_POOL_ATTR_SIZE => Value::U64(p.size),
        sys::SAI_BUFFER_POOL_ATTR_TYPE => Value::Enum(
            match p.kind {
                PoolType::Ingress => sys::SAI_BUFFER_POOL_TYPE_INGRESS,
                PoolType::Egress => sys::SAI_BUFFER_POOL_TYPE_EGRESS,
            }
            .cast_signed(),
        ),
        sys::SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE => Value::Enum(p.mode.sai()),
        sys::SAI_BUFFER_POOL_ATTR_XOFF_SIZE => Value::U64(p.xoff),
        _ => return Err(not_supported(0)),
    })
}

fn create_buffer_profile(store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let mut args = ProfileArgs {
        pool: None,
        reserved: 0,
        static_th: 0,
        xoff: 0,
        xon: 0,
        xon_offset: 0,
        dynamic_th: 0,
        mode: None,
    };
    let mut reserved = false;
    for (index, attr) in attrs.iter().enumerate() {
        create_arg(ObjectType::BufferProfile, attr, index)?;
        args.apply(store, attr, index, true)?;
        reserved |= attr.id == sys::SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE;
    }
    let profile = args
        .profile()
        .filter(|_| reserved)
        .ok_or(Error::MandatoryAttributeMissing)?;
    store.insert(profile).map(TypedId::raw)
}

fn set_buffer_profile<B: Backend>(backend: &mut B, store: &mut Store, id: BufferProfileId, attr: &Attr) -> Result<()> {
    let mut args = ProfileArgs::from(store.get::<BufferProfile>(id)?);
    args.apply(store, attr, 0, false)?;
    let changed = args.profile().ok_or(Error::Failure)?;
    let uses = Uses::Profile(id.raw());
    if let Err(e) = reapply_buffers(backend, store, uses, Pending::Profile(id, &changed)) {
        let _ = reapply_buffers(backend, store, uses, Pending::Nothing);
        return Err(backend_attr_error(e, 0));
    }
    store.update::<BufferProfile, _>(id, |p| *p = changed)
}

fn get_buffer_profile(store: &Store, p: &BufferProfile, attr: sys::sai_attr_id_t) -> Result<Value> {
    Ok(match attr {
        sys::SAI_BUFFER_PROFILE_ATTR_POOL_ID => Value::Oid(p.pool.raw()),
        sys::SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE => Value::U64(p.reserved),
        sys::SAI_BUFFER_PROFILE_ATTR_THRESHOLD_MODE => {
            let pool_mode = store
                .get::<BufferPool>(p.pool)
                .map_or(ThresholdMode::Dynamic, |pool| pool.mode);
            Value::Enum(p.mode.unwrap_or(pool_mode).sai())
        }
        sys::SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH => Value::I8(p.dynamic_th),
        sys::SAI_BUFFER_PROFILE_ATTR_SHARED_STATIC_TH => Value::U64(p.static_th),
        sys::SAI_BUFFER_PROFILE_ATTR_XOFF_TH => Value::U64(p.xoff),
        sys::SAI_BUFFER_PROFILE_ATTR_XON_TH => Value::U64(p.xon),
        sys::SAI_BUFFER_PROFILE_ATTR_XON_OFFSET_TH => Value::U64(p.xon_offset),
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ queues and priority groups

fn get_queue<B: Backend>(backend: &B, store: &Store, queue: RawOid, attr: sys::sai_attr_id_t) -> Result<Value> {
    let q = store.bag(queue)?;
    let stored = |attr| q.attrs.get(&attr).cloned().ok_or(Error::Failure);
    match attr {
        sys::SAI_QUEUE_ATTR_TYPE | sys::SAI_QUEUE_ATTR_INDEX | sys::SAI_QUEUE_ATTR_PORT => stored(attr),
        // The group for a backend with QoS, else the port (as before groups existed).
        sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE => {
            let group = queue_group(q);
            if !group.is_null() && backend.qos_supported() {
                Ok(Value::Oid(group))
            } else {
                Ok(Value::Oid(kept_oid(&q.attrs, sys::SAI_QUEUE_ATTR_PORT)))
            }
        }
        sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID
        | sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID
        | sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID => Ok(Value::Oid(kept_oid(&q.attrs, attr))),
        sys::SAI_QUEUE_ATTR_PFC_DLR_INIT => Ok(Value::Bool(matches!(q.attrs.get(&attr), Some(Value::Bool(true))))),
        _ => Err(not_supported(0)),
    }
}

/// The queue's own scheduler, WRED and buffer profile, and `PFC_DLR_INIT` (kept: no deadlock
/// recovery in the backends) (C++ `set_queue_attribute`).
fn set_queue<B: Backend>(backend: &mut B, store: &mut Store, queue: RawOid, attr: &Attr) -> Result<()> {
    let q = store.bag(queue)?;
    match (attr.id, &attr.value) {
        (sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID, Value::Oid(sched)) => {
            if !sched.is_null() && lookup::<Scheduler>(store, *sched).is_none() {
                return Err(invalid(0));
            }
            let group = store.bag(queue_group(q)).map_or(RawOid::NULL, |g| {
                kept_oid(&g.attrs, sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID)
            });
            let effective = if group.is_null() { *sched } else { group };
            apply_queue_sched(backend, store, q, effective, Pending::Nothing).map_err(|e| backend_attr_error(e, 0))?;
        }
        (sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID, Value::Oid(wred)) => {
            if !wred.is_null() && lookup::<Wred>(store, *wred).is_none() {
                return Err(invalid(0));
            }
            apply_queue_wred(backend, store, q, *wred).map_err(|e| backend_attr_error(e, 0))?;
        }
        (sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID, Value::Oid(profile_id)) => {
            if !profile_id.is_null() && !profile_fits(store, *profile_id, PoolType::Egress) {
                return Err(invalid(0));
            }
            apply_queue_buffer(backend, store, q, *profile_id, Pending::Nothing)
                .map_err(|e| backend_attr_error(e, 0))?;
        }
        (sys::SAI_QUEUE_ATTR_PFC_DLR_INIT, Value::Bool(_)) => {}
        _ => return Err(not_supported(0)),
    }
    store.update_bag(queue, |attrs| drop(attrs.insert(attr.id, attr.value.clone())))
}

fn get_priority_group(store: &Store, group: RawOid, attr: sys::sai_attr_id_t) -> Result<Value> {
    let g = store.bag(group)?;
    match attr {
        sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT | sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_INDEX => {
            g.attrs.get(&attr).cloned().ok_or(Error::Failure)
        }
        sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE => Ok(Value::Oid(kept_oid(&g.attrs, attr))),
        _ => Err(not_supported(0)),
    }
}

/// `BUFFER_PROFILE` only, a profile of an ingress pool (C++ `set_ipg_attribute`).
fn set_priority_group<B: Backend>(backend: &mut B, store: &mut Store, group: RawOid, attr: &Attr) -> Result<()> {
    let g = store.bag(group)?;
    let (sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE, Value::Oid(profile_id)) = (attr.id, &attr.value) else {
        return Err(not_supported(0));
    };
    if !profile_id.is_null() && !profile_fits(store, *profile_id, PoolType::Ingress) {
        return Err(invalid(0));
    }
    apply_pg_buffer(backend, store, g, *profile_id, Pending::Nothing).map_err(|e| backend_attr_error(e, 0))?;
    store.update_bag(group, |attrs| drop(attrs.insert(attr.id, attr.value.clone())))
}

// ------------------------------------------------------------------ counters

/// The bag of `expected` type the id names (C++ `lookup`: any other id is `INVALID_OBJECT_ID`).
fn bag_of(store: &Store, oid: RawOid, expected: ObjectType) -> Result<&Bag> {
    match store.bag(oid) {
        Ok(bag) if bag.object_type == expected => Ok(bag),
        _ => Err(Error::InvalidObjectId),
    }
}

/// Counters of a queue, priority group or buffer pool (C++ `get_queue_stats` / `_ext`,
/// `get_ipg_stats` / `_ext`, `get_buffer_pool_stats` / `_ext`); `clear`: `READ_AND_CLEAR` = the
/// read, then a clear. `NOT_SUPPORTED` without `QoS` in the backend (and for a queue of the CPU
/// port). `None`: another object type.
pub(crate) fn stats_get<B: Backend>(
    backend: &mut B,
    store: &Store,
    object_type: sys::sai_object_type_t,
    oid: RawOid,
    ids: &[sys::sai_stat_id_t],
    clear: bool,
) -> Option<Result<Vec<u64>>> {
    Some(match object_type {
        sys::SAI_OBJECT_TYPE_QUEUE => queue_stats(backend, store, oid).and_then(|(hw, index)| {
            let values = backend.queue_stats_get(hw, index, ids)?;
            if clear {
                backend.queue_stats_clear(hw, index, ids)?;
            }
            Ok(values)
        }),
        sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP => pg_stats(backend, store, oid).and_then(|(hw, index)| {
            let values = backend.pg_stats_get(hw, index, ids)?;
            if clear {
                backend.pg_stats_clear(hw, index, ids)?;
            }
            Ok(values)
        }),
        sys::SAI_OBJECT_TYPE_BUFFER_POOL => {
            if clear {
                // Pools are read only, and asked before anything else (as the C++).
                return Some(Err(Error::NotSupported));
            }
            match lookup::<BufferPool>(store, oid) {
                None => Err(Error::InvalidObjectId),
                Some(_) if !backend.qos_supported() => Err(Error::NotSupported),
                Some(pool) => backend.pool_stats_get(pool.kind == PoolType::Ingress, ids),
            }
        }
        _ => return None,
    })
}

/// Zeroes counters of a queue or priority group; a buffer pool's cannot be (C++
/// `clear_queue_stats`, `clear_ipg_stats`, `clear_buffer_pool_stats`). `None`: another type.
pub(crate) fn stats_clear<B: Backend>(
    backend: &mut B,
    store: &Store,
    object_type: sys::sai_object_type_t,
    oid: RawOid,
    ids: &[sys::sai_stat_id_t],
) -> Option<Result<()>> {
    Some(match object_type {
        sys::SAI_OBJECT_TYPE_QUEUE => {
            queue_stats(backend, store, oid).and_then(|(hw, index)| backend.queue_stats_clear(hw, index, ids))
        }
        sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP => {
            pg_stats(backend, store, oid).and_then(|(hw, index)| backend.pg_stats_clear(hw, index, ids))
        }
        sys::SAI_OBJECT_TYPE_BUFFER_POOL => Err(Error::NotSupported),
        _ => return None,
    })
}

fn queue_stats<B: Backend>(backend: &B, store: &Store, oid: RawOid) -> Result<(HwPort, u8)> {
    let q = bag_of(store, oid, ObjectType::Queue)?;
    let port = bag_port(store, q, sys::SAI_QUEUE_ATTR_PORT)
        .and_then(front)
        .filter(|_| backend.qos_supported())
        .ok_or(Error::NotSupported)?;
    let (_, index) = queue_target(store, q)?;
    Ok((port, index))
}

fn pg_stats<B: Backend>(backend: &B, store: &Store, oid: RawOid) -> Result<(HwPort, u8)> {
    let g = bag_of(store, oid, ObjectType::IngressPriorityGroup)?;
    if !backend.qos_supported() || bag_port(store, g, sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT).is_none() {
        return Err(Error::NotSupported);
    }
    pg_target(store, g)
}

/// `sai_query_stats_capability` for queues and priority groups (C++ `qos_stats_capability`):
/// each counter id up to the last the backends know, probed on queue / priority group 0 of the
/// first front-panel port; the backend answers per id. `None`: another type.
pub(crate) fn stats_capability<B: Backend>(
    backend: &B,
    store: Option<&Store>,
    object_type: sys::sai_object_type_t,
) -> Option<Result<Vec<sys::sai_stat_id_t>>> {
    let queue = match object_type {
        sys::SAI_OBJECT_TYPE_QUEUE => true,
        sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP => false,
        _ => return None,
    };
    let first = store.and_then(|s| s.iter::<Port>().find_map(|(_, p)| front(p)));
    let Some(hw) = first.filter(|_| backend.qos_supported()) else {
        return Some(Err(Error::NotSupported));
    };
    let last: sys::sai_stat_id_t = if queue { 0x40 } else { 0x20 };
    Some(Ok((0..=last)
        .filter(|id| {
            let probe = [*id];
            if queue {
                backend.queue_stats_get(hw, 0, &probe).is_ok()
            } else {
                backend.pg_stats_get(hw, 0, &probe).is_ok()
            }
        })
        .collect()))
}
