//! Shared by the integration tests: a switch on the virtual backend and the check that the
//! object model and the chip agree.
#![allow(dead_code, missing_docs, unreachable_pub, clippy::unwrap_used, clippy::panic)]
use sai_core::backend::{Backend, HwPort, Profile, Vid};
use sai_core::model::{BridgePort, Port, PortKind, Switch, Vlan, VlanMember};
use sai_core::oid::ObjectType;
use sai_core::oid::{RawOid, TypedId};
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;
use std::collections::BTreeMap;

pub const SWITCH: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SWITCH;
pub const PORT: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_PORT;
pub const VLAN: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_VLAN;
pub const VLAN_MEMBER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_VLAN_MEMBER;
pub const BRIDGE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_BRIDGE;
pub const BRIDGE_PORT: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_BRIDGE_PORT;
pub const VR: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER;
pub const RIF: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE;
pub const NEXT_HOP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_NEXT_HOP;
pub const QUEUE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_QUEUE;
pub const SCHEDULER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SCHEDULER;
pub const HOSTIF: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_HOSTIF;

pub fn oid(value: Value) -> RawOid {
    match value {
        Value::Oid(oid) => oid,
        other => panic!("not an object id: {other:?}"),
    }
}

pub fn enumerator(value: u32) -> Value {
    Value::Enum(value.cast_signed())
}

/// The attributes of a router interface on a port.
pub fn port_rif(vr: RawOid, port: RawOid) -> Vec<Attr> {
    vec![
        Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, Value::Oid(vr)),
        Attr::new(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            enumerator(sys::SAI_ROUTER_INTERFACE_TYPE_PORT),
        ),
        Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, Value::Oid(port)),
    ]
}

pub fn profile(ports: u32) -> Profile {
    Profile::from_pairs([("SAI_RS_VIRTUAL_PORTS", ports.to_string())])
}

pub fn init_attrs() -> Vec<Attr> {
    vec![Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))]
}

/// A switch with `ports` front-panel ports; returns the core and the switch's raw id.
pub fn switch(ports: u32) -> (Sai<VirtualBackend>, RawOid) {
    let mut sai = Sai::new(VirtualBackend::new());
    let id = sai.create_switch(&init_attrs(), &profile(ports)).unwrap();
    (sai, id.raw())
}

pub fn oid_list(value: Value) -> Vec<RawOid> {
    match value {
        Value::OidList(list) => list,
        other => panic!("not an object list: {other:?}"),
    }
}

pub fn get1(
    sai: &Sai<VirtualBackend>,
    object_type: sys::sai_object_type_t,
    oid: RawOid,
    attr: sys::sai_attr_id_t,
) -> sai_core::Result<Value> {
    sai.get(object_type, oid, &[attr]).map(|mut v| v.remove(0))
}

/// The store's own rules hold, and the chip has exactly what the model says: the same VLANs
/// with the same members and tagging, and the same admin state per port.
pub fn model_and_chip_agree(sai: &Sai<VirtualBackend>) -> Result<(), String> {
    let Some(store) = sai.store() else { return Ok(()) };
    store.check_invariants()?;
    let mut expected: BTreeMap<Vid, BTreeMap<HwPort, bool>> =
        store.iter::<Vlan>().map(|(_, v)| (v.vid, BTreeMap::new())).collect();
    for (id, member) in store.iter::<VlanMember>() {
        let vid = store
            .get::<Vlan>(member.vlan)
            .map_err(|e| format!("{id:?}: vlan {e}"))?
            .vid;
        let bridge_port = store
            .get::<BridgePort>(member.bridge_port)
            .map_err(|e| format!("{id:?}: bridge port {e}"))?;
        let hw = store
            .get::<Port>(bridge_port.port)
            .map_err(|e| format!("{id:?}: port {e}"))?
            .hw;
        if expected.entry(vid).or_default().insert(hw, member.untagged).is_some() {
            return Err(format!("{id:?}: second member for {hw:?} in {vid:?}"));
        }
    }
    if &expected != sai.backend().vlans() {
        return Err(format!(
            "VLANs differ: model {expected:?}, chip {:?}",
            sai.backend().vlans()
        ));
    }
    // Attribute-bag objects and entries: the chip has what the user created (the objects that
    // come with the switch are the model's alone), of the same type, and nothing else.
    let builtin: Vec<RawOid> = store
        .iter::<Switch>()
        .flat_map(|(_, s)| s.objects.values().copied().collect::<Vec<_>>())
        .collect();
    for (id, object_type) in sai.backend().objects() {
        match store.bag(*id) {
            Ok(bag) if bag.object_type.sai() == *object_type => {}
            other => return Err(format!("the chip has {id:?} ({object_type}), the model {other:?}")),
        }
    }
    for object_type in ObjectType::ALL {
        let fixed = matches!(
            object_type,
            ObjectType::Queue
                | ObjectType::IngressPriorityGroup
                | ObjectType::SchedulerGroup
                | ObjectType::Stp
                | ObjectType::Hash
        );
        for (id, _) in store.bags(*object_type) {
            if !fixed && !builtin.contains(&id) && !sai.backend().objects().contains_key(&id) {
                return Err(format!("the model has {id:?}, the chip does not"));
            }
        }
    }
    let model_entries: std::collections::BTreeSet<_> = store.entries().map(|(key, _)| *key).collect();
    if &model_entries != sai.backend().entries() {
        return Err(format!(
            "entries differ: model {model_entries:?}, chip {:?}",
            sai.backend().entries()
        ));
    }
    for (id, port) in store.iter::<Port>() {
        if matches!(port.kind, PortKind::Front { .. }) && sai.backend().port_admin(port.hw) != Some(port.admin) {
            return Err(format!(
                "{id:?}: admin {} in the model, {:?} on the chip",
                port.admin,
                sai.backend().port_admin(port.hw)
            ));
        }
        if matches!(port.kind, PortKind::Front { .. }) && sai.backend().port_speed_get(port.hw) != Ok(port.speed) {
            return Err(format!("{id:?}: speed differs"));
        }
    }
    Ok(())
}
