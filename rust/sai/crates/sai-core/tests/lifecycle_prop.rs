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
use sai_core::oid::RawOid;
use sai_core::value::{Attr, Value};
use sai_core::virt::Op;
use sai_sys as sys;

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
        let types = [SWITCH, PORT, VLAN, VLAN_MEMBER, BRIDGE, BRIDGE_PORT, sys::SAI_OBJECT_TYPE_NEXT_HOP];
        let ops = [Op::PortAdminSet, Op::PortSpeedSet, Op::VlanCreate, Op::VlanDestroy, Op::VlanPortAdd, Op::VlanPortRemove];
        let mode = |tagged: bool| Value::Enum(if tagged { sys::SAI_VLAN_TAGGING_MODE_TAGGED } else { sys::SAI_VLAN_TAGGING_MODE_UNTAGGED }.cast_signed());

        for step in steps {
            let objects_before = sai.store().map(sai_core::store::Store::len);
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
                    sai.backend_mut().fail_next(ops[i % ops.len()], Error::Failure);
                    Ok(())
                }
            };
            if let Err(e) = model_and_chip_agree(&sai) {
                return Err(TestCaseError::fail(format!("after {result:?}: {e}")));
            }
            // a failed call changed nothing that can be counted
            if result.is_err() {
                prop_assert_eq!(sai.store().map(sai_core::store::Store::len), objects_before);
            }
        }
        // every id of a removed object is dead for good
        let store = sai.store().unwrap();
        for id in vlans.iter().chain(&members).chain(&bridge_ports) {
            if !store.contains(*id) {
                prop_assert_eq!(sai.get(sys::SAI_OBJECT_TYPE_VLAN, *id, &[]).is_err() && sai.get(VLAN_MEMBER, *id, &[]).is_err() && sai.get(BRIDGE_PORT, *id, &[]).is_err(), true);
            }
        }
    }
}
