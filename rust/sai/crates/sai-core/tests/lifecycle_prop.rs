//! Property test of the object lifecycle: any sequence of creates, removes and sets, with
//! stale ids, ids of the wrong type and a backend that fails at random, leaves the store's
//! rules intact and the object model in step with the chip.
// Test code: a failed unwrap, index or panic is the test failing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
mod common;

use common::*;
use proptest::prelude::*;
use sai_core::error::Error;
use sai_core::model::EntryKey;
use sai_core::oid::RawOid;
use sai_core::value::{Attr, IpPrefix, Value};
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Clone, Debug)]
enum Step {
    CreateVlan(u16),
    RemoveVlan(usize),
    CreateMember {
        vlan: usize,
        bridge_port: usize,
        tagged: bool,
    },
    RemoveMember(usize),
    SetTagging {
        member: usize,
        tagged: bool,
    },
    CreateBridgePort(usize),
    RemoveBridgePort(usize),
    SetAdmin {
        port: usize,
        up: bool,
    },
    SetSpeed {
        port: usize,
        mbps: u32,
    },
    /// Any remove with any id: wrong types, stale ids, the null id.
    RemoveAny {
        object_type: usize,
        id: usize,
    },
    FailNext(usize),
    // attribute-bag objects
    CreateRouter,
    CreateInterface {
        router: usize,
        port: usize,
    },
    CreateNextHop {
        interface: usize,
        ip: u8,
    },
    CreateScheduler,
    /// Points a queue at a scheduler (or at nothing): a reference made and dropped by set.
    SetQueueScheduler {
        queue: usize,
        scheduler: Option<usize>,
    },
    /// Removes any attribute-bag object the test saw, through any of the bag types.
    RemoveBag {
        object_type: usize,
        id: usize,
    },
    // entries
    CreateFdb {
        vlan: usize,
        mac: u8,
        bridge_port: usize,
        fixed: bool,
    },
    CreateNeighbor {
        interface: usize,
        ip: u8,
    },
    CreateRoute {
        router: usize,
        net: u8,
        next_hop: Option<usize>,
    },
    SetRouteNextHop {
        entry: usize,
        next_hop: Option<usize>,
    },
    RemoveEntry(usize),
    FlushFdb {
        bridge_port: Option<usize>,
        all_types: bool,
    },
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0u16..12).prop_map(Step::CreateVlan),
        2 => any::<usize>().prop_map(Step::RemoveVlan),
        4 => (any::<usize>(), any::<usize>(), any::<bool>()).prop_map(|(vlan, bridge_port, tagged)| Step::CreateMember { vlan, bridge_port, tagged }),
        3 => any::<usize>().prop_map(Step::RemoveMember),
        1 => (any::<usize>(), any::<bool>()).prop_map(|(member, tagged)| Step::SetTagging { member, tagged }),
        2 => any::<usize>().prop_map(Step::CreateBridgePort),
        2 => any::<usize>().prop_map(Step::RemoveBridgePort),
        1 => (any::<usize>(), any::<bool>()).prop_map(|(port, up)| Step::SetAdmin { port, up }),
        1 => (any::<usize>(), prop_oneof![Just(25_000u32), Just(100_000), Just(7)]).prop_map(|(port, mbps)| Step::SetSpeed { port, mbps }),
        2 => (any::<usize>(), any::<usize>()).prop_map(|(object_type, id)| Step::RemoveAny { object_type, id }),
        2 => any::<usize>().prop_map(Step::FailNext),
        2 => Just(Step::CreateRouter),
        3 => (any::<usize>(), any::<usize>()).prop_map(|(router, port)| Step::CreateInterface { router, port }),
        2 => (any::<usize>(), 0u8..4).prop_map(|(interface, ip)| Step::CreateNextHop { interface, ip }),
        1 => Just(Step::CreateScheduler),
        2 => (any::<usize>(), proptest::option::of(any::<usize>())).prop_map(|(queue, scheduler)| Step::SetQueueScheduler { queue, scheduler }),
        4 => (any::<usize>(), any::<usize>()).prop_map(|(object_type, id)| Step::RemoveBag { object_type, id }),
        3 => (any::<usize>(), 0u8..4, any::<usize>(), any::<bool>()).prop_map(|(vlan, mac, bridge_port, fixed)| Step::CreateFdb { vlan, mac, bridge_port, fixed }),
        2 => (any::<usize>(), 0u8..4).prop_map(|(interface, ip)| Step::CreateNeighbor { interface, ip }),
        3 => (any::<usize>(), 0u8..4, proptest::option::of(any::<usize>())).prop_map(|(router, net, next_hop)| Step::CreateRoute { router, net, next_hop }),
        1 => (any::<usize>(), proptest::option::of(any::<usize>())).prop_map(|(entry, next_hop)| Step::SetRouteNextHop { entry, next_hop }),
        3 => any::<usize>().prop_map(Step::RemoveEntry),
        1 => (proptest::option::of(any::<usize>()), any::<bool>()).prop_map(|(bridge_port, all_types)| Step::FlushFdb { bridge_port, all_types }),
    ]
}

/// An id out of everything the test ever saw (live or removed), or a made-up one.
fn pick(ids: &[RawOid], index: usize) -> RawOid {
    if ids.is_empty() {
        RawOid(index as u64)
    } else {
        ids[index % ids.len()]
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    #[test]
    fn any_sequence_keeps_the_rules(steps in proptest::collection::vec(step(), 1..80)) {
        let (mut sai, switch) = switch(4);
        let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
        let Value::Oid(bridge) = get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID).unwrap() else { panic!() };
        let Value::Oid(default_vlan) = get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID).unwrap() else { panic!() };
        // every id ever handed out, including removed ones
        let mut vlans = vec![default_vlan];
        let mut bridge_ports = oid_list(get1(&sai, BRIDGE, bridge, sys::SAI_BRIDGE_ATTR_PORT_LIST).unwrap());
        let mut members = oid_list(get1(&sai, VLAN, default_vlan, sys::SAI_VLAN_ATTR_MEMBER_LIST).unwrap());
        let types = [SWITCH, PORT, VLAN, VLAN_MEMBER, BRIDGE, BRIDGE_PORT, sys::SAI_OBJECT_TYPE_TUNNEL];
        // the chip operations a failure is injected into (by name: the first slice's and the
        // typed ones the routers, interfaces, next hops, schedulers and entries below use)
        let ops = [
            "port_admin_set", "port_speed_set", "vlan_create", "vlan_destroy", "vlan_port_add", "vlan_port_remove",
            "l3_intf_create", "l3_intf_update", "l3_intf_destroy", "l3_egress_create", "l3_egress_destroy",
            "l3_host_add", "l3_host_delete", "l3_route_add", "l3_route_delete", "queue_scheduler_set",
        ];
        // attribute-bag objects and entries: every id and key the test ever saw
        let default_router = oid(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID).unwrap());
        let mut routers = vec![default_router];
        let mut interfaces: Vec<RawOid> = Vec::new();
        let mut next_hops: Vec<RawOid> = Vec::new();
        let mut schedulers: Vec<RawOid> = Vec::new();
        let queues = oid_list(get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_QOS_QUEUE_LIST).unwrap());
        let mut keys: Vec<EntryKey> = Vec::new();
        let bag_types = [VR, RIF, NEXT_HOP, SCHEDULER, QUEUE, sys::SAI_OBJECT_TYPE_STP];
        let address = |n: u8| IpAddr::V4(Ipv4Addr::new(10, 0, n, 1));
        let mode = |tagged: bool| Value::Enum(if tagged { sys::SAI_VLAN_TAGGING_MODE_TAGGED } else { sys::SAI_VLAN_TAGGING_MODE_UNTAGGED }.cast_signed());

        for step in steps {
            let objects_before = sai.store().map(|s| (s.len(), s.entry_count()));
            let result: Result<(), Error> = match step {
                Step::CreateVlan(vid) => sai.create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(vid))]).map(|id| vlans.push(id)),
                Step::RemoveVlan(i) => sai.remove(VLAN, pick(&vlans, i)),
                Step::CreateMember { vlan, bridge_port, tagged } => sai
                    .create(VLAN_MEMBER, switch, &[
                        Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(pick(&vlans, vlan))),
                        Attr::new(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(pick(&bridge_ports, bridge_port))),
                        Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, mode(tagged)),
                    ])
                    .map(|id| members.push(id)),
                Step::RemoveMember(i) => sai.remove(VLAN_MEMBER, pick(&members, i)),
                Step::SetTagging { member, tagged } => {
                    sai.set(VLAN_MEMBER, pick(&members, member), &Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, mode(tagged)))
                }
                Step::CreateBridgePort(port) => sai
                    .create(BRIDGE_PORT, switch, &[
                        Attr::new(sys::SAI_BRIDGE_PORT_ATTR_TYPE, Value::Enum(sys::SAI_BRIDGE_PORT_TYPE_PORT.cast_signed())),
                        Attr::new(sys::SAI_BRIDGE_PORT_ATTR_PORT_ID, Value::Oid(pick(&ports, port))),
                    ])
                    .map(|id| bridge_ports.push(id)),
                Step::RemoveBridgePort(i) => sai.remove(BRIDGE_PORT, pick(&bridge_ports, i)),
                Step::SetAdmin { port, up } => sai.set(PORT, pick(&ports, port), &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(up))),
                Step::SetSpeed { port, mbps } => sai.set(PORT, pick(&ports, port), &Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(mbps))),
                Step::RemoveAny { object_type, id } => {
                    let all: Vec<RawOid> = [&vlans[..], &bridge_ports[..], &members[..], &ports[..], &[RawOid::NULL, bridge]].concat();
                    let object_type = types[object_type % types.len()];
                    // not the switch itself: the sequence is about the objects inside it
                    if object_type == SWITCH { Ok(()) } else { sai.remove(object_type, pick(&all, id)) }
                }
                Step::FailNext(i) => {
                    // one armed failure at a time (a failing roll-back is not what this checks)
                    sai.backend_mut().clear_faults();
                    sai.backend_mut().fail_nth(ops[i % ops.len()], 1, Error::Failure).unwrap();
                    Ok(())
                }
                Step::CreateRouter => sai.create(VR, switch, &[]).map(|id| routers.push(id)),
                Step::CreateInterface { router, port } => sai
                    .create(RIF, switch, &port_rif(pick(&routers, router), pick(&ports, port)))
                    .map(|id| interfaces.push(id)),
                Step::CreateNextHop { interface, ip } => sai
                    .create(NEXT_HOP, switch, &[
                        Attr::new(sys::SAI_NEXT_HOP_ATTR_TYPE, enumerator(sys::SAI_NEXT_HOP_TYPE_IP)),
                        Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::IpAddress(address(ip))),
                        Attr::new(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID, Value::Oid(pick(&interfaces, interface))),
                    ])
                    .map(|id| next_hops.push(id)),
                Step::CreateScheduler => sai.create(SCHEDULER, switch, &[]).map(|id| schedulers.push(id)),
                Step::SetQueueScheduler { queue, scheduler } => {
                    let target = scheduler.map_or(RawOid::NULL, |i| pick(&schedulers, i));
                    sai.set(QUEUE, pick(&queues, queue), &Attr::new(sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID, Value::Oid(target)))
                }
                Step::RemoveBag { object_type, id } => {
                    let all: Vec<RawOid> = [&routers[..], &interfaces[..], &next_hops[..], &schedulers[..], &queues[..], &[RawOid::NULL]].concat();
                    let (object_type, target) = (bag_types[object_type % bag_types.len()], pick(&all, id));
                    // The C++ takes a port RIF's internal VLAN apart ignoring failures (a finding of the
                    // L3 port, kept): what the chip could not remove stays there. Put the chip back in
                    // step so the rest of the sequence is checked.
                    let internal = sai_core::oid::RouterInterfaceId::try_from(target)
                        .ok()
                        .filter(|_| object_type == RIF)
                        .and_then(|rif| sai.store()?.get::<sai_core::l3::RouterInterface>(rif).ok())
                        .and_then(|rif| match rif.kind {
                            sai_core::l3::RifKind::Port { vid, .. } => Some(vid),
                            _ => None,
                        });
                    let result = sai.remove(object_type, target);
                    if let (Ok(()), Some(vid)) = (result, internal) {
                        use sai_core::backend::Backend as _;
                        let chip = sai.backend_mut();
                        if let Some(left) = chip.vlans().get(&vid).cloned() {
                            for hw in left.keys() {
                                chip.vlan_port_remove(vid, *hw).unwrap();
                            }
                            chip.vlan_destroy(vid).unwrap();
                        }
                    }
                    result
                }
                Step::CreateFdb { vlan, mac, bridge_port, fixed } => {
                    let key = EntryKey::Fdb { bv_id: pick(&vlans, vlan), mac: [2, 0, 0, 0, 0, mac] };
                    let entry_type = if fixed { sys::SAI_FDB_ENTRY_TYPE_STATIC } else { sys::SAI_FDB_ENTRY_TYPE_DYNAMIC };
                    sai.create_entry(switch, key, &[
                        Attr::new(sys::SAI_FDB_ENTRY_ATTR_TYPE, enumerator(entry_type)),
                        Attr::new(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(pick(&bridge_ports, bridge_port))),
                    ])
                    .map(|()| keys.push(key))
                }
                Step::CreateNeighbor { interface, ip } => {
                    let key = EntryKey::Neighbor { rif: pick(&interfaces, interface), ip: address(ip) };
                    sai.create_entry(switch, key, &[Attr::new(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, Value::Mac([2; 6]))])
                        .map(|()| keys.push(key))
                }
                Step::CreateRoute { router, net, next_hop } => {
                    let prefix = IpPrefix::new(address(net), IpAddr::V4(Ipv4Addr::new(255, 255, 255, 0))).unwrap();
                    let key = EntryKey::Route { vr: pick(&routers, router), prefix };
                    let attrs: Vec<Attr> = next_hop
                        .map(|i| Attr::new(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, Value::Oid(pick(&next_hops, i))))
                        .into_iter()
                        .collect();
                    sai.create_entry(switch, key, &attrs).map(|()| keys.push(key))
                }
                Step::SetRouteNextHop { entry, next_hop } => match keys.get(entry % keys.len().max(1)) {
                    Some(key @ EntryKey::Route { .. }) => {
                        let target = next_hop.map_or(RawOid::NULL, |i| pick(&next_hops, i));
                        sai.set_entry(switch, key, &Attr::new(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, Value::Oid(target)))
                    }
                    _ => Ok(()),
                },
                Step::RemoveEntry(i) => match keys.get(i % keys.len().max(1)) {
                    Some(key) => sai.remove_entry(switch, key),
                    None => Ok(()),
                },
                Step::FlushFdb { bridge_port, all_types } => {
                    let mut attrs = Vec::new();
                    if let Some(i) = bridge_port {
                        attrs.push(Attr::new(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, Value::Oid(pick(&bridge_ports, i))));
                    }
                    if all_types {
                        attrs.push(Attr::new(sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE, enumerator(sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL)));
                    }
                    // A flush may stop half-way when the backend fails: it is checked by the
                    // agreement of model and chip below, not by the counts.
                    match sai.flush_fdb(switch, &attrs) {
                        Ok(_) | Err(Error::Failure) => Ok(()),
                        Err(e) => Err(e),
                    }
                }
            };
            if let Err(e) = model_and_chip_agree(&sai) {
                return Err(TestCaseError::fail(format!("after {result:?}: {e}")));
            }
            // a failed call changed nothing that can be counted
            if result.is_err() {
                prop_assert_eq!(sai.store().map(|s| (s.len(), s.entry_count())), objects_before);
            }
        }
        // every id of a removed object is dead for good
        let store = sai.store().unwrap();
        for id in routers.iter().chain(&interfaces).chain(&next_hops).chain(&schedulers) {
            if !store.contains(*id) {
                prop_assert!(sai.get(VR, *id, &[]).is_err() && sai.get(RIF, *id, &[]).is_err());
                prop_assert!(sai.get(NEXT_HOP, *id, &[]).is_err() && sai.get(SCHEDULER, *id, &[]).is_err());
                // and nothing still names it
                prop_assert_eq!(store.referrers(*id), 0);
            }
        }
        // an entry whose key or attributes name a removed object cannot exist
        for (key, attrs) in store.entries() {
            prop_assert!(store.contains(key.owner()), "{:?}", key);
            for value in attrs.values() {
                for target in value.object_ids() {
                    prop_assert!(store.contains(target), "{:?} names {:?}", key, target);
                }
            }
        }
        for id in vlans.iter().chain(&members).chain(&bridge_ports) {
            if !store.contains(*id) {
                prop_assert_eq!(sai.get(sys::SAI_OBJECT_TYPE_VLAN, *id, &[]).is_err() && sai.get(VLAN_MEMBER, *id, &[]).is_err() && sai.get(BRIDGE_PORT, *id, &[]).is_err(), true);
            }
        }
    }
}
