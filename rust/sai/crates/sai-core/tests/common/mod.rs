//! Shared by the integration tests: a switch on the virtual backend and the check that the
//! object model and the chip agree.
#![allow(dead_code, missing_docs, unreachable_pub, clippy::unwrap_used, clippy::panic)]
use sai_core::backend::types::Dest;
use sai_core::backend::{Backend, HwPort, Profile, Vid};
use sai_core::l2::Fdb;
use sai_core::model::{Attach, BridgePort, EntryKey, Lag, LagMember, Port, PortKind, Vlan, VlanMember};
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
    vlans_agree(sai, store)?;
    l3_agrees(sai, store)?;
    fdb_agrees(sai, store)?;
    lags_and_ports_agree(sai, store)
}

/// The chip's VLANs are the model's (members, tagging; the internal VLANs of port interfaces).
fn vlans_agree(sai: &Sai<VirtualBackend>, store: &sai_core::store::Store) -> Result<(), String> {
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
        // A bridge port on a LAG stands for every member port.
        let ports: Vec<_> = match bridge_port.attach {
            Attach::Port(port) => vec![port],
            Attach::Lag(lag) => store
                .iter::<LagMember>()
                .filter(|(_, m)| m.lag == lag)
                .map(|(_, m)| m.port)
                .collect(),
        };
        for port in ports {
            let hw = store.get::<Port>(port).map_err(|e| format!("{id:?}: port {e}"))?.hw;
            if expected
                .entry(vid)
                .or_default()
                .insert(hw, member.tagging.untagged())
                .is_some()
            {
                return Err(format!("{id:?}: second member for {hw:?} in {vid:?}"));
            }
        }
    }
    internal_vlans(store, &mut expected)?;
    // The chip keeps its CPU port in every VLAN (tagged), which the model does not list.
    let cpu = sai.backend().cpu_port();
    let chip: BTreeMap<Vid, BTreeMap<HwPort, bool>> = sai
        .backend()
        .vlans()
        .iter()
        .map(|(vid, members)| {
            (
                *vid,
                members
                    .iter()
                    .filter(|(hw, _)| **hw != cpu)
                    .map(|(h, u)| (*h, *u))
                    .collect(),
            )
        })
        .collect();
    if expected != chip {
        return Err(format!("VLANs differ: model {expected:?}, chip {chip:?}"));
    }
    Ok(())
}

/// Every FDB entry of the model is in the chip's L2 table.
fn fdb_agrees(sai: &Sai<VirtualBackend>, store: &sai_core::store::Store) -> Result<(), String> {
    // Every FDB entry of the model is in the chip's L2 table, where its bridge port is (the chip
    // may have more: learned on ports without a bridge port).
    for (key, stored) in store.entries() {
        let EntryKey::Fdb { bv_id, mac } = key else { continue };
        let fdb = Fdb::load(stored).map_err(|e| format!("{key:?}: {e}"))?;
        let vid = store
            .iter::<Vlan>()
            .find(|(id, _)| id.raw() == *bv_id)
            .map(|(_, v)| v.vid)
            .ok_or(format!("{key:?}: no VLAN"))?;
        let chip = sai
            .backend()
            .l2_table()
            .get(&(sai_core::backend::types::MacAddr(*mac), vid))
            .ok_or(format!("{key:?}: not in the chip's L2 table"))?;
        let dest = match fdb.bridge_port.map(|bp| store.get::<BridgePort>(bp).map(|b| b.attach)) {
            Some(Ok(Attach::Port(port))) => Dest::Port(store.get::<Port>(port).map_err(|e| e.to_string())?.hw),
            Some(Ok(Attach::Lag(lag))) => Dest::Lag(store.get::<Lag>(lag).map_err(|e| e.to_string())?.tid),
            Some(Err(e)) => return Err(format!("{key:?}: bridge port {e}")),
            None => Dest::Port(HwPort(0)),
        };
        if chip.dest != dest {
            return Err(format!("{key:?}: chip dest {:?}, model {dest:?}", chip.dest));
        }
    }
    Ok(())
}

/// The chip's trunks and ports are the model's LAGs and ports.
fn lags_and_ports_agree(sai: &Sai<VirtualBackend>, store: &sai_core::store::Store) -> Result<(), String> {
    // The chip's trunks are the model's LAGs, members in order.
    let trunks: BTreeMap<_, Vec<_>> = store
        .iter::<Lag>()
        .map(|(id, lag)| {
            let members = store
                .iter::<LagMember>()
                .filter(|(_, m)| m.lag == id)
                .filter_map(|(_, m)| {
                    Some(sai_core::backend::types::LagMember {
                        hw: store.get::<Port>(m.port).ok()?.hw,
                        egress_disable: m.egress_disable,
                        ingress_disable: m.ingress_disable,
                    })
                })
                .collect();
            (lag.tid, members)
        })
        .collect();
    if &trunks != sai.backend().lags() {
        return Err(format!(
            "LAGs differ: model {trunks:?}, chip {:?}",
            sai.backend().lags()
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

/// L3: the chip's L3 interfaces are the router interfaces', its host entries the neighbors' (those
/// with a host route), its routes the route entries' in their VR's VRF with the same target.
fn l3_agrees(sai: &Sai<VirtualBackend>, store: &sai_core::store::Store) -> Result<(), String> {
    use sai_core::l3::{RouterInterface, VirtualRouter};
    let chip = sai.backend().l3();
    let intfs: std::collections::BTreeSet<_> = store.iter::<RouterInterface>().filter_map(|(_, r)| r.intf).collect();
    if intfs != chip.intfs.keys().copied().collect() {
        return Err(format!("L3 interfaces differ: model {intfs:?}, chip {:?}", chip.intfs));
    }
    let tables = sai_core::l3::tables(store);
    let hosts: BTreeMap<_, _> = tables
        .neighbors()
        .iter()
        .filter(|(_, n)| n.host)
        .map(|((_, ip), n)| ((n.vrf, *ip), n.egress))
        .collect();
    if hosts != chip.hosts {
        return Err(format!("host entries differ: model {hosts:?}, chip {:?}", chip.hosts));
    }
    let mut routes = BTreeMap::new();
    for ((vr, prefix), route) in tables.routes() {
        let vrf = store.get::<VirtualRouter>(*vr).map_err(|e| format!("{vr:?}: {e}"))?.vrf;
        routes.insert((vrf, *prefix), route.target);
    }
    if routes != chip.routes {
        return Err(format!("routes differ: model {routes:?}, chip {:?}", chip.routes));
    }
    Ok(())
}

/// A port router interface's internal VLAN: its port, or every member port of its LAG, untagged.
fn internal_vlans(
    store: &sai_core::store::Store,
    expected: &mut BTreeMap<Vid, BTreeMap<HwPort, bool>>,
) -> Result<(), String> {
    for (id, rif) in store.iter::<sai_core::l3::RouterInterface>() {
        let sai_core::l3::RifKind::Port { port, vid } = rif.kind else {
            continue;
        };
        let ports: Vec<_> = match port {
            Attach::Port(port) => vec![port],
            Attach::Lag(lag) => store
                .iter::<LagMember>()
                .filter(|(_, m)| m.lag == lag)
                .map(|(_, m)| m.port)
                .collect(),
        };
        for port in ports {
            let hw = store.get::<Port>(port).map_err(|e| format!("{id:?}: port {e}"))?.hw;
            expected.entry(vid).or_default().insert(hw, true);
        }
    }
    Ok(())
}
