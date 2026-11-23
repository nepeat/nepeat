//! The entry points of the core: create, remove, set and get by object type, switch
//! creation (the second stage after API initialisation) and backend events.
//!
//! Shape of every operation: check the call against the metadata ([`crate::attrs`]), decode
//! raw ids into typed ids and resolve them in the store, call the backend, and only then
//! change the store.
use crate::attrs::{self, CreateArgs};
use crate::backend::{Backend, BackendEvent, EventSink, HwPort, Profile, StatsObject, Vid};
use crate::bag;
use crate::caps::{self, AttrCapability};
use crate::dispatch::{self, Cx};
use crate::error::{AttrError, Error, Result};
use crate::model::{Bag, Bridge, BridgePort, EntryKey, Kept, Port, PortKind, Switch, Vlan, VlanMember};
use crate::oid::{AnyId, BridgeId, BridgePortId, ObjectType, PortId, RawOid, SwitchId, TypedId, VlanId, VlanMemberId};
use crate::store::Store;
use crate::value::{Attr, Pointer, Value};
use sai_meta::{AttrMeta, ValueType};
use sai_sys as sys;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::mpsc;

const UNTAGGED: i32 = sys::SAI_VLAN_TAGGING_MODE_UNTAGGED.cast_signed();
const TAGGED: i32 = sys::SAI_VLAN_TAGGING_MODE_TAGGED.cast_signed();
const BRIDGE_PORT_TYPE_PORT: i32 = sys::SAI_BRIDGE_PORT_TYPE_PORT.cast_signed();

/// The MTU limit the switch reports (the C++ sai-openbcm's: SDK frame maximum minus FCS and tag).
const PORT_MAX_MTU: u32 = 9408;

/// Switch attributes accepted, kept and returned without effect (callbacks are kept too):
/// the "stub" and "ignored at create" lists of the C++ sai-openbcm.
const SWITCH_KEPT: &[sys::sai_attr_id_t] = &[
    sys::SAI_SWITCH_ATTR_SWITCH_PROFILE_ID,
    sys::SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO,
    sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED,
    sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED,
    sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_ALGORITHM,
    sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_ALGORITHM,
    sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_OFFSET,
    sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_OFFSET,
    sys::SAI_SWITCH_ATTR_RESTART_WARM,
    sys::SAI_SWITCH_ATTR_WARM_RECOVER,
    sys::SAI_SWITCH_ATTR_PRE_SHUTDOWN,
    sys::SAI_SWITCH_ATTR_FAST_API_ENABLE,
    sys::SAI_SWITCH_ATTR_UNINIT_DATA_PLANE_ON_REMOVAL,
    sys::SAI_SWITCH_ATTR_VXLAN_DEFAULT_PORT,
    sys::SAI_SWITCH_ATTR_BCAST_CPU_FLOOD_ENABLE,
    sys::SAI_SWITCH_ATTR_MCAST_CPU_FLOOD_ENABLE,
    sys::SAI_SWITCH_ATTR_SWITCHING_MODE,
    sys::SAI_SWITCH_ATTR_FDB_UNICAST_MISS_PACKET_ACTION,
    sys::SAI_SWITCH_ATTR_FDB_BROADCAST_MISS_PACKET_ACTION,
    sys::SAI_SWITCH_ATTR_FDB_MULTICAST_MISS_PACKET_ACTION,
    sys::SAI_SWITCH_ATTR_COUNTER_REFRESH_INTERVAL,
    sys::SAI_SWITCH_ATTR_SWITCH_ID,
    sys::SAI_SWITCH_ATTR_MAX_SYSTEM_CORES,
    sys::SAI_SWITCH_ATTR_FDB_AGING_TIME,
    sys::SAI_SWITCH_ATTR_HARDWARE_ACCESS_BUS,
    sys::SAI_SWITCH_ATTR_PLATFROM_CONTEXT,
    sys::SAI_SWITCH_ATTR_FIRMWARE_PATH_NAME,
    sys::SAI_SWITCH_ATTR_FIRMWARE_LOAD_METHOD,
    sys::SAI_SWITCH_ATTR_FIRMWARE_LOAD_TYPE,
    sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP,
    sys::SAI_SWITCH_ATTR_QOS_DOT1P_TO_TC_MAP,
    sys::SAI_SWITCH_ATTR_QOS_TC_TO_QUEUE_MAP,
];
/// Port attributes kept without effect (the C++ sai-openbcm's port stubs, and MTU).
const PORT_KEPT: &[sys::sai_attr_id_t] = &[
    sys::SAI_PORT_ATTR_MTU,
    sys::SAI_PORT_ATTR_MEDIA_TYPE,
    sys::SAI_PORT_ATTR_INTERFACE_TYPE,
    sys::SAI_PORT_ATTR_DEFAULT_VLAN_PRIORITY,
    sys::SAI_PORT_ATTR_DROP_UNTAGGED,
    sys::SAI_PORT_ATTR_DROP_TAGGED,
    sys::SAI_PORT_ATTR_LINK_TRAINING_ENABLE,
    sys::SAI_PORT_ATTR_GLOBAL_FLOW_CONTROL_MODE,
    sys::SAI_PORT_ATTR_UPDATE_DSCP,
    sys::SAI_PORT_ATTR_FULL_DUPLEX_MODE,
    sys::SAI_PORT_ATTR_IPG,
    sys::SAI_PORT_ATTR_PKT_TX_ENABLE,
    sys::SAI_PORT_ATTR_HOST_TX_SIGNAL_ENABLE,
    sys::SAI_PORT_ATTR_AUTO_NEG_FEC_MODE_OVERRIDE,
];
/// Port attributes with an effect on a chip that this library has no typed operation for
/// yet: each set is handed to [`Backend::object_set`] (a backend without it refuses) and
/// kept when the backend took it.
const PORT_BACKEND: &[sys::sai_attr_id_t] = &[
    sys::SAI_PORT_ATTR_FEC_MODE,
    sys::SAI_PORT_ATTR_AUTO_NEG_MODE,
    sys::SAI_PORT_ATTR_PORT_VLAN_ID,
    sys::SAI_PORT_ATTR_LOOPBACK_MODE,
    sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP,
    sys::SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP,
    sys::SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP,
    sys::SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP,
    sys::SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_QUEUE_MAP,
    sys::SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_PRIORITY_GROUP_MAP,
    sys::SAI_PORT_ATTR_QOS_DEFAULT_TC,
    sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL,
    sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE,
    sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX,
    sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX,
    sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID,
    sys::SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST,
    sys::SAI_PORT_ATTR_QOS_EGRESS_BUFFER_PROFILE_LIST,
    sys::SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID,
    sys::SAI_PORT_ATTR_MULTICAST_STORM_CONTROL_POLICER_ID,
    sys::SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID,
    sys::SAI_PORT_ATTR_INGRESS_ACL,
    sys::SAI_PORT_ATTR_EGRESS_ACL,
];
/// VLAN attributes kept without effect.
const VLAN_KEPT: &[sys::sai_attr_id_t] = &[
    sys::SAI_VLAN_ATTR_LEARN_DISABLE,
    sys::SAI_VLAN_ATTR_MAX_LEARNED_ADDRESSES,
    sys::SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE,
    sys::SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE,
    sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE,
    sys::SAI_VLAN_ATTR_IPV4_MCAST_LOOKUP_KEY_TYPE,
    sys::SAI_VLAN_ATTR_IPV6_MCAST_LOOKUP_KEY_TYPE,
    sys::SAI_VLAN_ATTR_META_DATA,
    sys::SAI_VLAN_ATTR_CUSTOM_IGMP_SNOOPING_ENABLE,
];
/// Bridge attributes kept without effect.
const BRIDGE_KEPT: &[sys::sai_attr_id_t] = &[
    sys::SAI_BRIDGE_ATTR_MAX_LEARNED_ADDRESSES,
    sys::SAI_BRIDGE_ATTR_LEARN_DISABLE,
    sys::SAI_BRIDGE_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE,
    sys::SAI_BRIDGE_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE,
    sys::SAI_BRIDGE_ATTR_BROADCAST_FLOOD_CONTROL_TYPE,
];
/// Bridge port attributes kept without effect.
const BRIDGE_PORT_KEPT: &[sys::sai_attr_id_t] = &[
    sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE,
    sys::SAI_BRIDGE_PORT_ATTR_MAX_LEARNED_ADDRESSES,
    sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_LIMIT_VIOLATION_PACKET_ACTION,
    sys::SAI_BRIDGE_PORT_ATTR_INGRESS_FILTERING,
    sys::SAI_BRIDGE_PORT_ATTR_EGRESS_FILTERING,
    sys::SAI_BRIDGE_PORT_ATTR_TAGGING_MODE,
];

/// Whether a port attribute is programmed through [`Backend::object_set`] (for the
/// capability queries, [`crate::caps`]).
pub(crate) fn port_attr_programmed(attr: sys::sai_attr_id_t) -> bool {
    PORT_BACKEND.contains(&attr)
}

/// Whether an object of this type keeps attribute `attr` (and so answers its default while
/// it is unset).
fn keeps(object_type: ObjectType, attr: sys::sai_attr_id_t) -> bool {
    match object_type {
        ObjectType::Switch => SWITCH_KEPT.contains(&attr),
        ObjectType::Port => PORT_KEPT.contains(&attr) || PORT_BACKEND.contains(&attr),
        ObjectType::Vlan => VLAN_KEPT.contains(&attr),
        ObjectType::Bridge => BRIDGE_KEPT.contains(&attr),
        ObjectType::BridgePort => BRIDGE_PORT_KEPT.contains(&attr),
        _ => false,
    }
}

/// Something to tell the SAI user. The rim delivers it through the callback the switch
/// was given, outside every lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notification {
    /// A port's operational status changed.
    PortOperStatus {
        /// The port.
        port: PortId,
        /// Up or down.
        up: bool,
    },
}

#[derive(Debug)]
struct Running {
    id: SwitchId,
    store: Store,
    events: Option<mpsc::Receiver<BackendEvent>>,
}

/// The SAI core over a backend. Created at API initialisation; holds a switch between
/// [`Sai::create_switch`] and its removal.
#[derive(Debug)]
pub struct Sai<B: Backend> {
    backend: B,
    switch: Option<Running>,
}

impl<B: Backend> Drop for Sai<B> {
    /// A switch still up when the core goes away (API uninitialised without removing it) is
    /// shut down: the chip must not be left running without its owner.
    fn drop(&mut self) {
        if self.switch.take().is_some() {
            self.backend.shutdown();
        }
    }
}

fn attr_err(kind: AttrError) -> Error {
    Error::attr(kind, 0)
}

impl<B: Backend> Sai<B> {
    /// The core with no switch yet.
    pub fn new(backend: B) -> Self {
        Self { backend, switch: None }
    }

    /// `sai_query_attribute_capability`: the capability table cut to what the backend
    /// programs ([`caps::attribute_for`]).
    pub fn attribute_capability(
        &self,
        object_type: sys::sai_object_type_t,
        attr: sys::sai_attr_id_t,
    ) -> AttrCapability {
        caps::attribute_for(&self.backend, object_type, attr)
    }

    /// `sai_query_attribute_enum_values_capability` over this backend ([`caps::enum_values_for`]).
    pub fn enum_values_capability(
        &self,
        object_type: sys::sai_object_type_t,
        attr: sys::sai_attr_id_t,
    ) -> Result<Vec<i32>> {
        caps::enum_values_for(&self.backend, object_type, attr)
    }

    /// The backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// The backend, mutably (tests: fault injection, link changes).
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// The object store of the switch, when there is one.
    pub fn store(&self) -> Option<&Store> {
        self.switch.as_ref().map(|r| &r.store)
    }

    /// The switch's id, when there is one.
    pub fn switch_id(&self) -> Option<SwitchId> {
        self.switch.as_ref().map(|r| r.id)
    }

    /// The receiving end of the backend's events, once per switch (for the notifier thread).
    pub fn take_events(&mut self) -> Option<mpsc::Receiver<BackendEvent>> {
        self.switch.as_mut().and_then(|r| r.events.take())
    }

    /// A callback the switch was given (`SAI_SWITCH_ATTR_*_NOTIFY`), null when unset.
    pub fn switch_callback(&self, attr: sys::sai_attr_id_t) -> Pointer {
        let Some(running) = &self.switch else {
            return Pointer::NULL;
        };
        match running.store.get::<Switch>(running.id).map(|s| s.kept.get(&attr)) {
            Ok(Some(Value::Pointer(p))) => *p,
            _ => Pointer::NULL,
        }
    }

    /// Creates the switch: brings the backend up and builds the objects every switch has
    /// (ports, CPU port, default bridge with one bridge port per port, VLAN 1 with the
    /// chip's default membership). Front-panel ports are disabled: SAI's default admin state
    /// is down, and a port must only pass traffic once the user enabled it.
    pub fn create_switch(&mut self, attrs: &[Attr], profile: &Profile) -> Result<SwitchId> {
        if self.switch.is_some() {
            return Err(Error::ItemAlreadyExists);
        }
        let mut args = CreateArgs::check(sys::SAI_OBJECT_TYPE_SWITCH, attrs)?;
        if !args.require::<bool>(sys::SAI_SWITCH_ATTR_INIT_SWITCH)? {
            // Attaching to a running switch (warm boot, a second user) is not implemented.
            return Err(Error::attr(
                AttrError::NotSupported,
                args.index_of(sys::SAI_SWITCH_ATTR_INIT_SWITCH),
            ));
        }
        let index_of_type = args.index_of(sys::SAI_SWITCH_ATTR_TYPE);
        if args
            .take::<i32>(sys::SAI_SWITCH_ATTR_TYPE)?
            .is_some_and(|t| t != sys::SAI_SWITCH_TYPE_NPU.cast_signed())
        {
            return Err(Error::attr(AttrError::Invalid, index_of_type));
        }
        let src_mac = args
            .take::<[u8; 6]>(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS)?
            .unwrap_or_default();
        let kept = args.finish(SWITCH_KEPT, true)?;

        let (sink, events) = EventSink::channel();
        self.backend.init(profile, sink)?;
        match build_switch(&mut self.backend, src_mac, kept) {
            Ok((id, store)) => {
                self.switch = Some(Running {
                    id,
                    store,
                    events: Some(events),
                });
                Ok(id)
            }
            Err(e) => {
                self.backend.shutdown();
                Err(e)
            }
        }
    }

    /// Creates an object of a type other than the switch.
    pub fn create(&mut self, object_type: sys::sai_object_type_t, switch: RawOid, attrs: &[Attr]) -> Result<RawOid> {
        let object_type = ObjectType::from_sai(object_type).ok_or(Error::NotImplemented)?;
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        if SwitchId::try_from(switch)? != running.id {
            return Err(Error::InvalidObjectId);
        }
        let store = &mut running.store;
        match object_type {
            ObjectType::Switch => Err(Error::InvalidParameter),
            // Ports come from the chip configuration (no breakout yet); one bridge.
            ObjectType::Port | ObjectType::Bridge => Err(Error::NotSupported),
            ObjectType::Vlan => create_vlan(backend, store, attrs).map(TypedId::raw),
            ObjectType::VlanMember => create_vlan_member(backend, store, attrs).map(TypedId::raw),
            ObjectType::BridgePort => create_bridge_port(store, running.id, attrs).map(TypedId::raw),
            other => dispatch::create(
                Cx {
                    backend,
                    store,
                    switch: running.id,
                },
                other,
                attrs,
            ),
        }
    }

    /// The backend and the store of the switch, for an operation that changes both.
    fn parts(&mut self) -> Result<Cx<'_, B>> {
        match self {
            Self {
                backend,
                switch: Some(running),
            } => Ok(Cx {
                backend,
                store: &mut running.store,
                switch: running.id,
            }),
            _ => Err(Error::Uninitialized),
        }
    }

    /// Checks the switch id an entry's key carries.
    fn check_switch(&self, switch: RawOid) -> Result<()> {
        let running = self.switch.as_ref().ok_or(Error::Uninitialized)?;
        if SwitchId::try_from(switch)? == running.id {
            Ok(())
        } else {
            Err(Error::InvalidObjectId)
        }
    }

    /// Creates an FDB, neighbor or route entry. `switch` is the switch id of its key.
    pub fn create_entry(&mut self, switch: RawOid, key: EntryKey, attrs: &[Attr]) -> Result<()> {
        self.check_switch(switch)?;
        dispatch::create_entry(self.parts()?, key, attrs)
    }

    /// Removes an entry.
    pub fn remove_entry(&mut self, switch: RawOid, key: &EntryKey) -> Result<()> {
        self.check_switch(switch)?;
        dispatch::remove_entry(self.parts()?, key)
    }

    /// Sets one attribute of an entry.
    pub fn set_entry(&mut self, switch: RawOid, key: &EntryKey, attr: &Attr) -> Result<()> {
        self.check_switch(switch)?;
        dispatch::set_entry(self.parts()?, key, attr)
    }

    /// Gets attributes of an entry, in the order of `ids`.
    pub fn get_entry(&self, switch: RawOid, key: &EntryKey, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        self.check_switch(switch)?;
        let running = self.switch.as_ref().ok_or(Error::Uninitialized)?;
        dispatch::get_entry(&self.backend, &running.store, key, ids)
    }

    /// `flush_fdb_entries`: removes the FDB entries matching the attributes and returns
    /// their keys.
    pub fn flush_fdb(&mut self, switch: RawOid, attrs: &[Attr]) -> Result<Vec<EntryKey>> {
        self.check_switch(switch)?;
        dispatch::flush_fdb(self.parts()?, attrs)
    }

    /// Reads counters of an object, in the order of `counters`; with `clear` they are zeroed
    /// after the read. A counter id the object type does not have is `InvalidParameter`; one
    /// the chip does not have is the backend's answer (`NotSupported`).
    pub fn stats_get(
        &mut self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        counters: &[sys::sai_stat_id_t],
        clear: bool,
    ) -> Result<Vec<u64>> {
        let object = self.stats_object(object_type, oid, counters)?;
        self.backend.stats_get(object, counters, clear)
    }

    /// Zeroes counters of an object.
    pub fn stats_clear(
        &mut self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        counters: &[sys::sai_stat_id_t],
    ) -> Result<()> {
        let object = self.stats_object(object_type, oid, counters)?;
        self.backend.stats_clear(object, counters)
    }

    /// `sai_query_stats_capability`: the counters the chip has for objects of a type.
    pub fn stats_capability(&self, object_type: sys::sai_object_type_t) -> Result<Vec<sys::sai_stat_id_t>> {
        if self.switch.is_none() {
            return Err(Error::Uninitialized);
        }
        self.backend.stats_supported(object_type)
    }

    /// Resolves the object of a counter call into the chip's terms.
    fn stats_object(
        &self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        counters: &[sys::sai_stat_id_t],
    ) -> Result<StatsObject> {
        let id = self.resolve(object_type, oid)?;
        let known = sai_meta::stat_ids(object_type);
        if known.is_empty() {
            return Err(Error::NotSupported);
        }
        if counters.iter().any(|c| !known.contains(&c.cast_signed())) {
            return Err(Error::InvalidParameter);
        }
        let store = self.store().ok_or(Error::Uninitialized)?;
        let of_port = |bag_id: RawOid, port_attr, index_attr| -> Result<(HwPort, u8)> {
            let bag = store.bag(bag_id)?;
            match (bag.attrs.get(&port_attr), bag.attrs.get(&index_attr)) {
                (Some(Value::Oid(port)), Some(Value::U8(index))) => {
                    Ok((store.get::<Port>(PortId::try_from(*port)?)?.hw, *index))
                }
                _ => Err(Error::Failure),
            }
        };
        Ok(match id {
            // The CPU port has no counters.
            AnyId::Port(port) => StatsObject::Port(front_port(store.get::<Port>(port)?).ok_or(Error::NotSupported)?),
            AnyId::Queue(queue) => {
                let (port, index) = of_port(queue.raw(), sys::SAI_QUEUE_ATTR_PORT, sys::SAI_QUEUE_ATTR_INDEX)?;
                StatsObject::Queue { port, index }
            }
            AnyId::IngressPriorityGroup(group) => {
                let (port, index) = of_port(
                    group.raw(),
                    sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT,
                    sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_INDEX,
                )?;
                StatsObject::PriorityGroup { port, index }
            }
            other => StatsObject::Other {
                object_type,
                id: other.raw(),
            },
        })
    }

    /// Removes an object. Removing the switch shuts the backend down.
    pub fn remove(&mut self, object_type: sys::sai_object_type_t, oid: RawOid) -> Result<()> {
        let id = self.resolve(object_type, oid)?;
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        let store = &mut running.store;
        match id {
            AnyId::Switch(_) => {
                backend.shutdown();
                self.switch = None;
                Ok(())
            }
            AnyId::Port(_) | AnyId::Bridge(_) => Err(Error::NotSupported),
            AnyId::Vlan(id) => {
                store.ensure_removable::<Vlan>(id)?;
                let vid = store.get::<Vlan>(id)?.vid;
                backend.vlan_destroy(vid)?;
                store.remove::<Vlan>(id).map(|_| ())
            }
            AnyId::VlanMember(id) => {
                store.ensure_removable::<VlanMember>(id)?;
                let (vid, hw, _) = member_target(store, store.get::<VlanMember>(id)?)?;
                backend.vlan_port_remove(vid, hw)?;
                store.remove::<VlanMember>(id).map(|_| ())
            }
            AnyId::BridgePort(id) => store.remove::<BridgePort>(id).map(|_| ()),
            other => dispatch::remove(
                Cx {
                    backend,
                    store,
                    switch: running.id,
                },
                other.object_type(),
                other.raw(),
            ),
        }
    }

    /// Sets one attribute.
    pub fn set(&mut self, object_type: sys::sai_object_type_t, oid: RawOid, attr: &Attr) -> Result<()> {
        let id = self.resolve(object_type, oid)?;
        let meta = attrs::check_set(id.object_type().sai(), attr)?;
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        let store = &mut running.store;
        // An object id must name a live object, whoever keeps the attribute.
        bag::check_references(store, core::slice::from_ref(attr))?;
        // The metadata check passed, so each value below has its attribute's kind.
        match (id, attr.id, &attr.value) {
            (AnyId::Switch(id), sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac(mac)) => {
                store.update::<Switch, _>(id, |s| s.src_mac = *mac)
            }
            (AnyId::Switch(id), attr_id, value)
                if meta.value_type == ValueType::Pointer || SWITCH_KEPT.contains(&attr_id) =>
            {
                store.update::<Switch, _>(id, |s| drop(s.kept.insert(attr_id, value.clone())))
            }
            (AnyId::Port(id), sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(up)) => {
                let hw = front_port(store.get::<Port>(id)?).ok_or(attr_err(AttrError::NotSupported))?;
                backend.port_admin_set(hw, *up)?;
                store.update::<Port, _>(id, |p| p.admin = *up)
            }
            (AnyId::Port(id), sys::SAI_PORT_ATTR_SPEED, Value::U32(mbps)) => {
                let hw = front_port(store.get::<Port>(id)?).ok_or(attr_err(AttrError::NotSupported))?;
                if !backend.port_supported_speeds(hw)?.contains(mbps) {
                    return Err(attr_err(AttrError::InvalidValue));
                }
                backend.port_speed_set(hw, *mbps)?;
                store.update::<Port, _>(id, |p| p.speed = *mbps)
            }
            (AnyId::Port(id), attr_id, value) if PORT_KEPT.contains(&attr_id) => {
                store.update::<Port, _>(id, |p| drop(p.kept.insert(attr_id, value.clone())))
            }
            (AnyId::Port(id), attr_id, value) if PORT_BACKEND.contains(&attr_id) => {
                backend.object_set(sys::SAI_OBJECT_TYPE_PORT, id.raw(), attr)?;
                store.update::<Port, _>(id, |p| drop(p.kept.insert(attr_id, value.clone())))
            }
            (AnyId::Vlan(id), attr_id, value) if VLAN_KEPT.contains(&attr_id) => {
                store.update::<Vlan, _>(id, |v| drop(v.kept.insert(attr_id, value.clone())))
            }
            (AnyId::Bridge(id), attr_id, value) if BRIDGE_KEPT.contains(&attr_id) => {
                store.update::<Bridge, _>(id, |b| drop(b.kept.insert(attr_id, value.clone())))
            }
            (AnyId::VlanMember(id), sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, Value::Enum(mode)) => {
                let untagged = tagging(*mode).ok_or(attr_err(AttrError::NotSupported))?;
                let (vid, hw, _) = member_target(store, store.get::<VlanMember>(id)?)?;
                backend.vlan_port_add(vid, hw, untagged)?;
                store.update::<VlanMember, _>(id, |m| m.untagged = untagged)
            }
            (AnyId::BridgePort(id), sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE, Value::Bool(up)) => {
                store.update::<BridgePort, _>(id, |b| b.admin = *up)
            }
            (AnyId::BridgePort(id), attr_id, value) if BRIDGE_PORT_KEPT.contains(&attr_id) => {
                store.update::<BridgePort, _>(id, |b| drop(b.kept.insert(attr_id, value.clone())))
            }
            (
                AnyId::Switch(_)
                | AnyId::Port(_)
                | AnyId::Vlan(_)
                | AnyId::VlanMember(_)
                | AnyId::Bridge(_)
                | AnyId::BridgePort(_),
                ..,
            ) => Err(attr_err(AttrError::NotSupported)),
            (other, ..) => dispatch::set(
                Cx {
                    backend,
                    store,
                    switch: running.id,
                },
                other.object_type(),
                other.raw(),
                attr,
            ),
        }
    }

    /// Gets attributes. The values come back in the order of `ids`; an error names the
    /// position of the attribute that failed.
    pub fn get(
        &self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        let id = self.resolve(object_type, oid)?;
        let Some(running) = &self.switch else {
            return Err(Error::Uninitialized);
        };
        ids.iter()
            .enumerate()
            .map(|(index, attr)| {
                let meta = attrs::meta(id.object_type().sai(), *attr, index)?;
                let value = self.get_one(&running.store, id, meta).map_err(|e| e.at_index(index))?;
                // An answer of the wrong kind would be encoded into the wrong union member.
                if value.value_type() == meta.value_type {
                    Ok(value)
                } else {
                    Err(Error::Failure)
                }
            })
            .collect()
    }

    /// Applies an event the backend raised and returns what the user is to be told.
    pub fn handle_event(&mut self, event: BackendEvent) -> Vec<Notification> {
        let Some(running) = &mut self.switch else {
            return Vec::new();
        };
        match event {
            BackendEvent::Fdb(event) => crate::l2::fdb_event(&mut running.store, event),
            BackendEvent::Link { hw, up } => {
                let port = PortId::from_index(u64::from(hw.0));
                match running
                    .store
                    .update::<Port, _>(port, |p| core::mem::replace(&mut p.oper_up, up))
                {
                    Ok(was) if was != up => vec![Notification::PortOperStatus { port, up }],
                    _ => Vec::new(),
                }
            }
        }
    }

    /// Decodes a raw id for an operation on `object_type`: the id's type must be that type
    /// and the object must exist.
    fn resolve(&self, object_type: sys::sai_object_type_t, oid: RawOid) -> Result<AnyId> {
        let expected = ObjectType::from_sai(object_type).ok_or(Error::NotImplemented)?;
        let Some(running) = &self.switch else {
            return Err(Error::Uninitialized);
        };
        let id = AnyId::decode(oid)?;
        if id.object_type() != expected {
            return Err(Error::InvalidObjectType);
        }
        if running.store.contains(oid) {
            Ok(id)
        } else {
            Err(Error::InvalidObjectId)
        }
    }

    fn get_one(&self, store: &Store, id: AnyId, meta: &AttrMeta) -> Result<Value> {
        let attr = meta.id;
        let own = match id {
            AnyId::Switch(id) => self.get_switch(store, id, attr)?,
            AnyId::Port(id) => self.get_port(store, id, attr)?,
            AnyId::Vlan(_) | AnyId::VlanMember(_) | AnyId::Bridge(_) | AnyId::BridgePort(_) => {
                get_bridging(store, id, attr)?
            }
            other => return dispatch::get(&self.backend, store, other.object_type(), other.raw(), meta),
        };
        if let Some(value) = own {
            return Ok(value);
        }
        // Not an attribute the object computes: one it keeps (unset: the header's default),
        // an unset callback, or nothing.
        match kept_of(store, id)?.and_then(|kept| kept.get(&attr)) {
            Some(value) => Ok(value.clone()),
            None if meta.value_type == ValueType::Pointer => Ok(Value::Pointer(Pointer::NULL)),
            None if keeps(id.object_type(), attr) => {
                attrs::default_value(meta).ok_or(attr_err(AttrError::NotSupported))
            }
            None => Err(attr_err(AttrError::NotSupported)),
        }
    }

    fn get_switch(&self, store: &Store, id: SwitchId, attr: sys::sai_attr_id_t) -> Result<Option<Value>> {
        let switch = store.get::<Switch>(id)?;
        if let Some(object) = switch.objects.get(&attr) {
            return Ok(Some(Value::Oid(*object)));
        }
        let front = || {
            store
                .iter::<Port>()
                .filter(|(_, p)| matches!(p.kind, PortKind::Front { .. }))
        };
        let count = |n: usize| u32::try_from(n).map_err(|_| Error::Failure);
        let queues = u32::from(self.backend.port_queues());
        Ok(Some(match attr {
            sys::SAI_SWITCH_ATTR_PORT_NUMBER | sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_SUPPORTED_PORTS => {
                Value::U32(count(front().count())?)
            }
            sys::SAI_SWITCH_ATTR_PORT_LIST => Value::OidList(front().map(|(id, _)| id.raw()).collect()),
            sys::SAI_SWITCH_ATTR_CPU_PORT => Value::Oid(switch.cpu_port.raw()),
            sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID => Value::Oid(switch.default_vlan.raw()),
            sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID => Value::Oid(switch.default_bridge.raw()),
            sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS => Value::Mac(switch.src_mac),
            sys::SAI_SWITCH_ATTR_INIT_SWITCH => Value::Bool(true),
            sys::SAI_SWITCH_ATTR_OPER_STATUS => Value::Enum(sys::SAI_SWITCH_OPER_STATUS_UP.cast_signed()),
            sys::SAI_SWITCH_ATTR_TYPE => Value::Enum(sys::SAI_SWITCH_TYPE_NPU.cast_signed()),
            sys::SAI_SWITCH_ATTR_PORT_MAX_MTU => Value::U32(PORT_MAX_MTU),
            sys::SAI_SWITCH_ATTR_QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES => Value::U8(self.backend.port_queues()),
            sys::SAI_SWITCH_ATTR_NUMBER_OF_QUEUES | sys::SAI_SWITCH_ATTR_NUMBER_OF_UNICAST_QUEUES => Value::U32(queues),
            // No multicast queues; no ACL objects here, so no actions in either stage.
            sys::SAI_SWITCH_ATTR_NUMBER_OF_MULTICAST_QUEUES | sys::SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT => {
                Value::U32(0)
            }
            // Asked for by syncd when it was given none: nothing to identify.
            sys::SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO if !switch.kept.contains_key(&attr) => Value::I8List(Vec::new()),
            sys::SAI_SWITCH_ATTR_ACL_STAGE_INGRESS | sys::SAI_SWITCH_ATTR_ACL_STAGE_EGRESS => Value::AclCapability {
                action_list_mandatory: false,
                actions: Vec::new(),
                non_contiguous_exact_match: false,
                match_type: 0,
            },
            _ => return self.get_capacity(store, attr),
        }))
    }

    /// The table sizes and free counts orchagent reads at start and CRM polls: the chip's
    /// limits ([`Backend::limits`]) minus what the model holds.
    fn get_capacity(&self, store: &Store, attr: sys::sai_attr_id_t) -> Result<Option<Value>> {
        let is_capacity = matches!(
            attr,
            sys::SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY
                | sys::SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY
                | sys::SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS
                | sys::SAI_SWITCH_ATTR_ECMP_MEMBERS
                | sys::SAI_SWITCH_ATTR_MAX_ECMP_MEMBER_COUNT
                | sys::SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE
                | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY
                | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY
                | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY
                | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY
                | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY
                | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY
                | sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY
                | sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY
                | sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY
        );
        if !is_capacity {
            return Ok(None);
        }
        let limits = self.backend.limits().map_err(|e| match e {
            Error::NotSupported => attr_err(AttrError::NotSupported),
            other => other,
        })?;
        let used = Usage::of(store);
        let left = |capacity: u32, used: usize| capacity.saturating_sub(u32::try_from(used).unwrap_or(u32::MAX));
        let next_hops_free = left(limits.next_hops, used.next_hops);
        let hosts_free = left(limits.hosts, used.v4_neighbors + 2 * used.v6_neighbors);
        Ok(Some(match attr {
            sys::SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY => Value::U32(limits.acl_priority_min),
            sys::SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY => Value::U32(limits.acl_priority_max),
            sys::SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS => Value::U32(limits.ecmp_groups),
            sys::SAI_SWITCH_ATTR_ECMP_MEMBERS => Value::U32(limits.ecmp_paths),
            sys::SAI_SWITCH_ATTR_MAX_ECMP_MEMBER_COUNT => Value::U32(limits.ecmp_members),
            sys::SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE => Value::U64(limits.buffer_kb),
            sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY => Value::U32(left(limits.v4_routes, used.v4_routes)),
            sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY => Value::U32(left(limits.v6_routes, used.v6_routes)),
            // A next hop uses its neighbor's egress object.
            sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY => {
                Value::U32(next_hops_free)
            }
            // A neighbor takes a host entry (IPv6: two) and an egress object.
            sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY => Value::U32(hosts_free.min(next_hops_free)),
            sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY => Value::U32((hosts_free / 2).min(next_hops_free)),
            sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY => Value::U32(left(limits.ecmp_groups, used.groups)),
            sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY => {
                Value::U32(left(limits.ecmp_members, used.group_members))
            }
            _ => Value::U32(left(limits.fdb, used.fdb)),
        }))
    }

    fn get_port(&self, store: &Store, id: PortId, attr: sys::sai_attr_id_t) -> Result<Option<Value>> {
        let port = store.get::<Port>(id)?;
        let front = front_port(port);
        Ok(Some(match (attr, &port.kind) {
            (sys::SAI_PORT_ATTR_TYPE, PortKind::Front { .. }) => Value::Enum(sys::SAI_PORT_TYPE_LOGICAL.cast_signed()),
            (sys::SAI_PORT_ATTR_TYPE, PortKind::Cpu) => Value::Enum(sys::SAI_PORT_TYPE_CPU.cast_signed()),
            (sys::SAI_PORT_ATTR_HW_LANE_LIST, PortKind::Front { lanes }) => Value::U32List(lanes.clone()),
            (sys::SAI_PORT_ATTR_ADMIN_STATE, _) => Value::Bool(port.admin),
            (sys::SAI_PORT_ATTR_SPEED | sys::SAI_PORT_ATTR_OPER_SPEED, _) => Value::U32(port.speed),
            (sys::SAI_PORT_ATTR_OPER_STATUS, _) => {
                // Live from the chip; the CPU port is always up.
                let up = front.map_or(Ok(true), |hw| self.backend.port_link_get(hw))?;
                let status = if up {
                    sys::SAI_PORT_OPER_STATUS_UP
                } else {
                    sys::SAI_PORT_OPER_STATUS_DOWN
                };
                Value::Enum(status.cast_signed())
            }
            (sys::SAI_PORT_ATTR_SUPPORTED_SPEED, _) => {
                Value::U32List(front.map_or(Ok(Vec::new()), |hw| self.backend.port_supported_speeds(hw))?)
            }
            // What the C++ sai-openbcm answers for every front-panel port.
            (sys::SAI_PORT_ATTR_SUPPORTED_FEC_MODE, PortKind::Front { .. }) => Value::I32List(
                [
                    sys::SAI_PORT_FEC_MODE_NONE,
                    sys::SAI_PORT_FEC_MODE_RS,
                    sys::SAI_PORT_FEC_MODE_FC,
                ]
                .map(u32::cast_signed)
                .to_vec(),
            ),
            (sys::SAI_PORT_ATTR_SUPPORTED_AUTO_NEG_MODE, PortKind::Front { .. }) => Value::Bool(true),
            (sys::SAI_PORT_ATTR_QOS_NUMBER_OF_QUEUES, _) => count_of(&port_children(store, ObjectType::Queue, id))?,
            (sys::SAI_PORT_ATTR_QOS_QUEUE_LIST, _) => Value::OidList(port_children(store, ObjectType::Queue, id)),
            (sys::SAI_PORT_ATTR_NUMBER_OF_INGRESS_PRIORITY_GROUPS, _) => {
                count_of(&port_children(store, ObjectType::IngressPriorityGroup, id))?
            }
            (sys::SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST, _) => {
                Value::OidList(port_children(store, ObjectType::IngressPriorityGroup, id))
            }
            (sys::SAI_PORT_ATTR_QOS_NUMBER_OF_SCHEDULER_GROUPS, _) => {
                count_of(&port_children(store, ObjectType::SchedulerGroup, id))?
            }
            (sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST, _) => {
                Value::OidList(port_children(store, ObjectType::SchedulerGroup, id))
            }
            _ => return Ok(None),
        }))
    }
}

/// What the model holds of the tables the capacity attributes are about.
#[derive(Default)]
struct Usage {
    v4_routes: usize,
    v6_routes: usize,
    v4_neighbors: usize,
    v6_neighbors: usize,
    fdb: usize,
    next_hops: usize,
    groups: usize,
    group_members: usize,
}

impl Usage {
    fn of(store: &Store) -> Self {
        let mut usage = Self {
            next_hops: store.bags(ObjectType::NextHop).count(),
            groups: store.bags(ObjectType::NextHopGroup).count(),
            group_members: store.bags(ObjectType::NextHopGroupMember).count(),
            ..Self::default()
        };
        for (key, _) in store.entries() {
            match key {
                EntryKey::Fdb { .. } => usage.fdb += 1,
                EntryKey::Neighbor { ip: IpAddr::V4(_), .. } => usage.v4_neighbors += 1,
                EntryKey::Neighbor { ip: IpAddr::V6(_), .. } => usage.v6_neighbors += 1,
                EntryKey::Route { prefix, .. } if prefix.addr().is_ipv4() => usage.v4_routes += 1,
                EntryKey::Route { .. } => usage.v6_routes += 1,
            }
        }
        usage
    }
}

fn count_of(list: &[RawOid]) -> Result<Value> {
    u32::try_from(list.len()).map(Value::U32).map_err(|_| Error::Failure)
}

/// The queues, priority groups or scheduler groups of a port: the bags of that type whose
/// port attribute names it.
fn port_children(store: &Store, object_type: ObjectType, port: PortId) -> Vec<RawOid> {
    let attr = match object_type {
        ObjectType::Queue => sys::SAI_QUEUE_ATTR_PORT,
        ObjectType::IngressPriorityGroup => sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT,
        _ => sys::SAI_SCHEDULER_GROUP_ATTR_PORT_ID,
    };
    store
        .bags(object_type)
        .filter(|(_, bag)| bag.attrs.get(&attr) == Some(&Value::Oid(port.raw())))
        .map(|(id, _)| id)
        .collect()
}

/// The attributes an object keeps without acting on them, if its type has any.
fn kept_of(store: &Store, id: AnyId) -> Result<Option<&Kept>> {
    Ok(match id {
        AnyId::Switch(id) => Some(&store.get::<Switch>(id)?.kept),
        AnyId::Port(id) => Some(&store.get::<Port>(id)?.kept),
        AnyId::Vlan(id) => Some(&store.get::<Vlan>(id)?.kept),
        AnyId::Bridge(id) => Some(&store.get::<Bridge>(id)?.kept),
        AnyId::BridgePort(id) => Some(&store.get::<BridgePort>(id)?.kept),
        // A VLAN member keeps nothing; attribute-bag objects are not asked here.
        _ => None,
    })
}

/// Gets of VLANs, VLAN members, the bridge and bridge ports.
fn get_bridging(store: &Store, id: AnyId, attr: sys::sai_attr_id_t) -> Result<Option<Value>> {
    Ok(Some(match (id, attr) {
        (AnyId::Vlan(id), sys::SAI_VLAN_ATTR_VLAN_ID) => Value::U16(store.get::<Vlan>(id)?.vid.get()),
        // Every VLAN is in the default spanning tree instance (the only one).
        (AnyId::Vlan(_), sys::SAI_VLAN_ATTR_STP_INSTANCE) => {
            Value::Oid(store.bags(ObjectType::Stp).next().map_or(RawOid::NULL, |(stp, _)| stp))
        }
        (AnyId::Vlan(id), sys::SAI_VLAN_ATTR_MEMBER_LIST) => Value::OidList(
            store
                .iter::<VlanMember>()
                .filter(|(_, m)| m.vlan == id)
                .map(|(m, _)| m.raw())
                .collect(),
        ),
        (AnyId::VlanMember(id), sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID) => {
            Value::Oid(store.get::<VlanMember>(id)?.vlan.raw())
        }
        (AnyId::VlanMember(id), sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID) => {
            Value::Oid(store.get::<VlanMember>(id)?.bridge_port.raw())
        }
        (AnyId::VlanMember(id), sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE) => {
            Value::Enum(if store.get::<VlanMember>(id)?.untagged {
                UNTAGGED
            } else {
                TAGGED
            })
        }
        (AnyId::Bridge(_), sys::SAI_BRIDGE_ATTR_TYPE) => Value::Enum(sys::SAI_BRIDGE_TYPE_1Q.cast_signed()),
        (AnyId::Bridge(id), sys::SAI_BRIDGE_ATTR_PORT_LIST) => Value::OidList(
            store
                .iter::<BridgePort>()
                .filter(|(_, b)| b.bridge == id)
                .map(|(b, _)| b.raw())
                .collect(),
        ),
        (AnyId::BridgePort(_), sys::SAI_BRIDGE_PORT_ATTR_TYPE) => Value::Enum(BRIDGE_PORT_TYPE_PORT),
        (AnyId::BridgePort(id), sys::SAI_BRIDGE_PORT_ATTR_PORT_ID) => {
            Value::Oid(store.get::<BridgePort>(id)?.port.raw())
        }
        (AnyId::BridgePort(id), sys::SAI_BRIDGE_PORT_ATTR_BRIDGE_ID) => {
            Value::Oid(store.get::<BridgePort>(id)?.bridge.raw())
        }
        (AnyId::BridgePort(id), sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE) => {
            Value::Bool(store.get::<BridgePort>(id)?.admin)
        }
        _ => return Ok(None),
    }))
}

/// The chip port of a front-panel port; `None` for the CPU port.
fn front_port(port: &Port) -> Option<HwPort> {
    matches!(port.kind, PortKind::Front { .. }).then_some(port.hw)
}

/// `SAI_VLAN_TAGGING_MODE_*` as "untagged?"; priority tagging is not implemented.
const fn tagging(mode: i32) -> Option<bool> {
    match mode {
        UNTAGGED => Some(true),
        TAGGED => Some(false),
        _ => None,
    }
}

/// The VLAN id and chip port a member stands for, and its tagging.
fn member_target(store: &Store, member: &VlanMember) -> Result<(Vid, HwPort, bool)> {
    let vid = store.get::<Vlan>(member.vlan)?.vid;
    let port = store.get::<Port>(store.get::<BridgePort>(member.bridge_port)?.port)?;
    Ok((vid, port.hw, member.untagged))
}

fn build_switch<B: Backend>(backend: &mut B, src_mac: [u8; 6], kept: Kept) -> Result<(SwitchId, Store)> {
    let mut store = Store::new();
    let cpu_hw = backend.cpu_port();
    let cpu_port = PortId::from_index(u64::from(cpu_hw.0));
    store.insert_at(
        cpu_port,
        Port {
            hw: cpu_hw,
            kind: PortKind::Cpu,
            admin: true,
            speed: 0,
            oper_up: true,
            kept: Kept::new(),
        },
    )?;
    let default_bridge: BridgeId = store.insert(Bridge { kept: Kept::new() })?;
    for info in backend.ports() {
        backend.port_admin_set(info.hw, false)?;
        let port = PortId::from_index(u64::from(info.hw.0));
        store.insert_at(
            port,
            Port {
                hw: info.hw,
                kind: PortKind::Front { lanes: info.lanes },
                admin: false,
                speed: info.speed,
                oper_up: false,
                kept: Kept::new(),
            },
        )?;
        store.insert(BridgePort {
            bridge: default_bridge,
            port,
            admin: true,
            kept: Kept::new(),
        })?;
        build_port_qos(backend, &mut store, port, info.hw)?;
    }
    // The objects every switch has besides ports and bridging: the default virtual router,
    // spanning tree instance and trap group, and the two hash objects. Named by the
    // read-only switch attribute that answers with them.
    let mut objects = BTreeMap::new();
    for (attr, object_type) in [
        (
            sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
            ObjectType::VirtualRouter,
        ),
        (sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID, ObjectType::Stp),
        (sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP, ObjectType::HostifTrapGroup),
        (sys::SAI_SWITCH_ATTR_LAG_HASH, ObjectType::Hash),
        (sys::SAI_SWITCH_ATTR_ECMP_HASH, ObjectType::Hash),
    ] {
        let object = store.insert_bag(Bag {
            object_type,
            attrs: Kept::new(),
        })?;
        objects.insert(attr, object);
    }
    // VLAN 1 as the chip has it; the user (orchagent) finds these members and removes them.
    let default_vlan = store.insert(Vlan {
        vid: Vid::DEFAULT,
        kept: Kept::new(),
    })?;
    let bridge_ports: Vec<(BridgePortId, HwPort)> = store
        .iter::<BridgePort>()
        .filter_map(|(id, b)| Some((id, store.get::<Port>(b.port).ok()?.hw)))
        .collect();
    for (hw, untagged) in backend.vlan_members(Vid::DEFAULT)? {
        if let Some((bridge_port, _)) = bridge_ports.iter().find(|(_, port_hw)| *port_hw == hw) {
            store.insert(VlanMember {
                vlan: default_vlan,
                bridge_port: *bridge_port,
                untagged,
            })?;
        }
    }
    let id = SwitchId::from_index(0);
    store.insert_at(
        id,
        Switch {
            src_mac,
            cpu_port,
            default_vlan,
            default_bridge,
            objects,
            kept,
        },
    )?;
    Ok((id, store))
}

/// The queues, scheduler groups and priority groups of a front-panel port, as many as the
/// chip has per port. Their ids carry the port and the index (the C++ layout): one flat
/// level, a scheduler group per queue with the port as its parent.
fn build_port_qos<B: Backend>(backend: &B, store: &mut Store, port: PortId, hw: HwPort) -> Result<()> {
    let index_of = |n: u8| (u64::from(hw.0) << 8) | u64::from(n);
    for queue in 0..backend.port_queues() {
        let group = ObjectType::SchedulerGroup.id(index_of(queue));
        store.insert_bag_at(
            group,
            Bag {
                object_type: ObjectType::SchedulerGroup,
                attrs: Kept::from([
                    (sys::SAI_SCHEDULER_GROUP_ATTR_PORT_ID, Value::Oid(port.raw())),
                    (sys::SAI_SCHEDULER_GROUP_ATTR_LEVEL, Value::U8(0)),
                    (sys::SAI_SCHEDULER_GROUP_ATTR_MAX_CHILDS, Value::U8(1)),
                    (sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE, Value::Oid(port.raw())),
                ]),
            },
        )?;
        store.insert_bag_at(
            ObjectType::Queue.id(index_of(queue)),
            Bag {
                object_type: ObjectType::Queue,
                attrs: Kept::from([
                    (
                        sys::SAI_QUEUE_ATTR_TYPE,
                        Value::Enum(sys::SAI_QUEUE_TYPE_UNICAST.cast_signed()),
                    ),
                    (sys::SAI_QUEUE_ATTR_PORT, Value::Oid(port.raw())),
                    (sys::SAI_QUEUE_ATTR_INDEX, Value::U8(queue)),
                    (sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE, Value::Oid(group)),
                ]),
            },
        )?;
    }
    for group in 0..backend.port_priority_groups() {
        store.insert_bag_at(
            ObjectType::IngressPriorityGroup.id(index_of(group)),
            Bag {
                object_type: ObjectType::IngressPriorityGroup,
                attrs: Kept::from([
                    (sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT, Value::Oid(port.raw())),
                    (sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_INDEX, Value::U8(group)),
                ]),
            },
        )?;
    }
    Ok(())
}

fn create_vlan<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<VlanId> {
    let mut args = CreateArgs::check(sys::SAI_OBJECT_TYPE_VLAN, attrs)?;
    let index = args.index_of(sys::SAI_VLAN_ATTR_VLAN_ID);
    let vid = Vid::new(args.require::<u16>(sys::SAI_VLAN_ATTR_VLAN_ID)?)
        .map_err(|_| Error::attr(AttrError::InvalidValue, index))?;
    bag::check_references(store, attrs)?;
    let kept = args.finish(VLAN_KEPT, false)?;
    if store.iter::<Vlan>().any(|(_, v)| v.vid == vid) {
        return Err(Error::ItemAlreadyExists);
    }
    backend.vlan_create(vid)?;
    store.insert(Vlan { vid, kept })
}

fn create_vlan_member<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<VlanMemberId> {
    let mut args = CreateArgs::check(sys::SAI_OBJECT_TYPE_VLAN_MEMBER, attrs)?;
    let bad = |args: &CreateArgs<'_>, id| Error::attr(AttrError::InvalidValue, args.index_of(id));
    let vlan: VlanId = args
        .take_id(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID)?
        .ok_or(Error::MandatoryAttributeMissing)?;
    let bridge_port: BridgePortId = args
        .take_id(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID)?
        .ok_or(Error::MandatoryAttributeMissing)?;
    let mode = args
        .take::<i32>(sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE)?
        .unwrap_or(UNTAGGED);
    let untagged = tagging(mode).ok_or_else(|| {
        Error::attr(
            AttrError::NotSupported,
            args.index_of(sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE),
        )
    })?;
    // Ids of the right type that name nothing (never created, or removed): the attribute's fault.
    store
        .get::<Vlan>(vlan)
        .map_err(|_| bad(&args, sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID))?;
    store
        .get::<BridgePort>(bridge_port)
        .map_err(|_| bad(&args, sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID))?;
    args.finish(&[], false)?;
    if store
        .iter::<VlanMember>()
        .any(|(_, m)| m.vlan == vlan && m.bridge_port == bridge_port)
    {
        return Err(Error::ItemAlreadyExists);
    }
    let member = VlanMember {
        vlan,
        bridge_port,
        untagged,
    };
    let (vid, hw, _) = member_target(store, &member)?;
    backend.vlan_port_add(vid, hw, untagged)?;
    store.insert(member)
}

fn create_bridge_port(store: &mut Store, switch: SwitchId, attrs: &[Attr]) -> Result<BridgePortId> {
    let mut args = CreateArgs::check(sys::SAI_OBJECT_TYPE_BRIDGE_PORT, attrs)?;
    let index_of_type = args.index_of(sys::SAI_BRIDGE_PORT_ATTR_TYPE);
    if args.require::<i32>(sys::SAI_BRIDGE_PORT_ATTR_TYPE)? != BRIDGE_PORT_TYPE_PORT {
        return Err(Error::attr(AttrError::NotSupported, index_of_type));
    }
    let index_of_port = args.index_of(sys::SAI_BRIDGE_PORT_ATTR_PORT_ID);
    let bad_port = Error::attr(AttrError::InvalidValue, index_of_port);
    // PORT_ID is mandatory for type PORT (a condition the metadata tables do not carry).
    let port: PortId = args
        .take_id(sys::SAI_BRIDGE_PORT_ATTR_PORT_ID)?
        .ok_or(Error::MandatoryAttributeMissing)?;
    if front_port(store.get::<Port>(port).map_err(|_| bad_port)?).is_none() {
        return Err(bad_port);
    }
    let default_bridge = store.get::<Switch>(switch)?.default_bridge;
    let index_of_bridge = args.index_of(sys::SAI_BRIDGE_PORT_ATTR_BRIDGE_ID);
    let bridge: BridgeId = args
        .take_id(sys::SAI_BRIDGE_PORT_ATTR_BRIDGE_ID)?
        .unwrap_or(default_bridge);
    store
        .get::<Bridge>(bridge)
        .map_err(|_| Error::attr(AttrError::InvalidValue, index_of_bridge))?;
    let admin = args
        .take::<bool>(sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE)?
        .unwrap_or(false);
    let kept = args.finish(BRIDGE_PORT_KEPT, false)?;
    if store.iter::<BridgePort>().any(|(_, b)| b.port == port) {
        return Err(Error::ItemAlreadyExists);
    }
    store.insert(BridgePort {
        bridge,
        port,
        admin,
        kept,
    })
}
