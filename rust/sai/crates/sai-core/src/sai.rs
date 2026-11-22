//! The entry points of the core: create, remove, set and get by object type, switch
//! creation (the second stage after API initialisation) and backend events.
//!
//! Shape of every operation: check the call against the metadata ([`crate::attrs`]), decode
//! raw ids into typed ids and resolve them in the store, call the backend, and only then
//! change the store.
use crate::attrs::{self, CreateArgs};
use crate::backend::{Backend, BackendEvent, EventSink, HwPort, Profile, Vid};
use crate::error::{AttrError, Error, Result};
use crate::model::{Bridge, BridgePort, Kept, Port, PortKind, Switch, Vlan, VlanMember};
use crate::oid::{AnyId, BridgeId, BridgePortId, ObjectType, PortId, RawOid, SwitchId, TypedId, VlanId, VlanMemberId};
use crate::store::Store;
use crate::value::{Attr, Pointer, Value};
use sai_meta::ValueType;
use sai_sys as sys;
use std::sync::mpsc;

const UNTAGGED: i32 = sys::SAI_VLAN_TAGGING_MODE_UNTAGGED.cast_signed();
const TAGGED: i32 = sys::SAI_VLAN_TAGGING_MODE_TAGGED.cast_signed();
const BRIDGE_PORT_TYPE_PORT: i32 = sys::SAI_BRIDGE_PORT_TYPE_PORT.cast_signed();

/// Switch attributes accepted, kept and returned without effect (callbacks are kept too).
const SWITCH_KEPT: &[sys::sai_attr_id_t] = &[
    sys::SAI_SWITCH_ATTR_SWITCH_PROFILE_ID,
    sys::SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO,
];
/// Port attributes kept without effect.
const PORT_KEPT: &[sys::sai_attr_id_t] = &[sys::SAI_PORT_ATTR_MTU];
/// Bridge port attributes kept without effect.
const BRIDGE_PORT_KEPT: &[sys::sai_attr_id_t] = &[sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE];

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
        let mut args = CreateArgs::check(ObjectType::Switch, attrs)?;
        if !args.require::<bool>(sys::SAI_SWITCH_ATTR_INIT_SWITCH)? {
            // Attaching to a running switch (warm boot, a second user) is not implemented.
            return Err(Error::attr(
                AttrError::NotSupported,
                args.index_of(sys::SAI_SWITCH_ATTR_INIT_SWITCH),
            ));
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
        }
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
        }
    }

    /// Sets one attribute.
    pub fn set(&mut self, object_type: sys::sai_object_type_t, oid: RawOid, attr: &Attr) -> Result<()> {
        let id = self.resolve(object_type, oid)?;
        let meta = attrs::check_set(type_of(id), attr)?;
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        let store = &mut running.store;
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
            _ => Err(attr_err(AttrError::NotSupported)),
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
                let meta = attrs::meta(type_of(id), *attr, index)?;
                let value = self
                    .get_one(&running.store, id, *attr, meta.value_type)
                    .map_err(|e| e.at_index(index))?;
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
        if type_of(id) != expected {
            return Err(Error::InvalidObjectType);
        }
        if running.store.contains(oid) {
            Ok(id)
        } else {
            Err(Error::InvalidObjectId)
        }
    }

    fn get_one(&self, store: &Store, id: AnyId, attr: sys::sai_attr_id_t, value_type: ValueType) -> Result<Value> {
        let own = match id {
            AnyId::Switch(id) => get_switch(store, id, attr)?,
            AnyId::Port(id) => self.get_port(store, id, attr)?,
            other => get_bridging(store, other, attr)?,
        };
        if let Some(value) = own {
            return Ok(value);
        }
        // Not an attribute the object computes: one it keeps, an unset callback, or nothing.
        match kept_of(store, id)?.and_then(|kept| kept.get(&attr)) {
            Some(value) => Ok(value.clone()),
            None if value_type == ValueType::Pointer => Ok(Value::Pointer(Pointer::NULL)),
            None => Err(attr_err(AttrError::NotSupported)),
        }
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
            _ => return Ok(None),
        }))
    }
}

/// The attributes an object keeps without acting on them, if its type has any.
fn kept_of(store: &Store, id: AnyId) -> Result<Option<&Kept>> {
    Ok(match id {
        AnyId::Switch(id) => Some(&store.get::<Switch>(id)?.kept),
        AnyId::Port(id) => Some(&store.get::<Port>(id)?.kept),
        AnyId::Vlan(id) => Some(&store.get::<Vlan>(id)?.kept),
        AnyId::VlanMember(_) => None,
        AnyId::Bridge(id) => Some(&store.get::<Bridge>(id)?.kept),
        AnyId::BridgePort(id) => Some(&store.get::<BridgePort>(id)?.kept),
    })
}

fn get_switch(store: &Store, id: SwitchId, attr: sys::sai_attr_id_t) -> Result<Option<Value>> {
    let switch = store.get::<Switch>(id)?;
    let front = || {
        store
            .iter::<Port>()
            .filter(|(_, p)| matches!(p.kind, PortKind::Front { .. }))
    };
    Ok(Some(match attr {
        sys::SAI_SWITCH_ATTR_PORT_NUMBER | sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_SUPPORTED_PORTS => {
            Value::U32(u32::try_from(front().count()).map_err(|_| Error::Failure)?)
        }
        sys::SAI_SWITCH_ATTR_PORT_LIST => Value::OidList(front().map(|(id, _)| id.raw()).collect()),
        sys::SAI_SWITCH_ATTR_CPU_PORT => Value::Oid(switch.cpu_port.raw()),
        sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID => Value::Oid(switch.default_vlan.raw()),
        sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID => Value::Oid(switch.default_bridge.raw()),
        sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS => Value::Mac(switch.src_mac),
        sys::SAI_SWITCH_ATTR_INIT_SWITCH => Value::Bool(true),
        sys::SAI_SWITCH_ATTR_OPER_STATUS => Value::Enum(sys::SAI_SWITCH_OPER_STATUS_UP.cast_signed()),
        sys::SAI_SWITCH_ATTR_TYPE => Value::Enum(sys::SAI_SWITCH_TYPE_NPU.cast_signed()),
        _ => return Ok(None),
    }))
}

/// Gets of VLANs, VLAN members, the bridge and bridge ports.
fn get_bridging(store: &Store, id: AnyId, attr: sys::sai_attr_id_t) -> Result<Option<Value>> {
    Ok(Some(match (id, attr) {
        (AnyId::Vlan(id), sys::SAI_VLAN_ATTR_VLAN_ID) => Value::U16(store.get::<Vlan>(id)?.vid.get()),
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

const fn type_of(id: AnyId) -> ObjectType {
    match id {
        AnyId::Switch(_) => ObjectType::Switch,
        AnyId::Port(_) => ObjectType::Port,
        AnyId::Vlan(_) => ObjectType::Vlan,
        AnyId::VlanMember(_) => ObjectType::VlanMember,
        AnyId::Bridge(_) => ObjectType::Bridge,
        AnyId::BridgePort(_) => ObjectType::BridgePort,
    }
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
            kept,
        },
    )?;
    Ok((id, store))
}

fn create_vlan<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<VlanId> {
    let mut args = CreateArgs::check(ObjectType::Vlan, attrs)?;
    let index = args.index_of(sys::SAI_VLAN_ATTR_VLAN_ID);
    let vid = Vid::new(args.require::<u16>(sys::SAI_VLAN_ATTR_VLAN_ID)?)
        .map_err(|_| Error::attr(AttrError::InvalidValue, index))?;
    let kept = args.finish(&[], false)?;
    if store.iter::<Vlan>().any(|(_, v)| v.vid == vid) {
        return Err(Error::ItemAlreadyExists);
    }
    backend.vlan_create(vid)?;
    store.insert(Vlan { vid, kept })
}

fn create_vlan_member<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<VlanMemberId> {
    let mut args = CreateArgs::check(ObjectType::VlanMember, attrs)?;
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
    let mut args = CreateArgs::check(ObjectType::BridgePort, attrs)?;
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
