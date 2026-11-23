//! The objects that come with the switch, attribute-bag objects, entries, counters and
//! capacity attributes on the virtual backend, and what a backend without them answers.
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
use sai_core::backend::{Backend, HwPort, Refusing, StatsObject};
use sai_core::caps;
use sai_core::error::{AttrError, Error};
use sai_core::model::EntryKey;
use sai_core::oid::{ObjectType, RawOid};
use sai_core::sai::Sai;
use sai_core::value::{Attr, IpPrefix, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn switch_oid(sai: &Sai<VirtualBackend>, switch: RawOid, attr: sys::sai_attr_id_t) -> RawOid {
    oid(get1(sai, SWITCH, switch, attr).unwrap())
}

fn unsupported(index: usize) -> Error {
    Error::attr(AttrError::NotSupported, index)
}

#[test]
fn the_switch_comes_with_its_default_objects() {
    let (mut sai, switch) = switch(4);
    let store = sai.store().unwrap();
    for (attr, object_type) in [
        (sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID, VR),
        (sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID, sys::SAI_OBJECT_TYPE_STP),
        (
            sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP,
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP,
        ),
    ] {
        let id = switch_oid(&sai, switch, attr);
        assert_eq!(id.sai_object_type(), object_type);
        assert!(store.contains(id));
    }
    // the hash objects answer only with a hash backend (C++ switch_hash_get)
    for attr in [sys::SAI_SWITCH_ATTR_ECMP_HASH, sys::SAI_SWITCH_ATTR_LAG_HASH] {
        assert_eq!(get1(&sai, SWITCH, switch, attr), Err(unsupported(0)));
    }
    // the same ids as the C++ gives its default objects
    assert_eq!(
        switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID),
        RawOid(u64::from(VR) << 56 | 1)
    );

    // 8 queues and 8 priority groups per front-panel port, none on the CPU port (the scheduler
    // groups are the QoS area's, shown with QoS only)
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let cpu = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_CPU_PORT);
    for (count, list) in [
        (
            sys::SAI_PORT_ATTR_QOS_NUMBER_OF_QUEUES,
            sys::SAI_PORT_ATTR_QOS_QUEUE_LIST,
        ),
        (
            sys::SAI_PORT_ATTR_NUMBER_OF_INGRESS_PRIORITY_GROUPS,
            sys::SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST,
        ),
    ] {
        assert_eq!(get1(&sai, PORT, ports[1], count), Ok(Value::U32(8)));
        assert_eq!(oid_list(get1(&sai, PORT, ports[1], list).unwrap()).len(), 8);
        assert_eq!(get1(&sai, PORT, cpu, count), Ok(Value::U32(0)));
        assert_eq!(get1(&sai, PORT, cpu, list), Ok(Value::OidList(vec![])));
    }
    assert_eq!(store.bags(ObjectType::Queue).count(), 32);
    let queues = oid_list(get1(&sai, PORT, ports[1], sys::SAI_PORT_ATTR_QOS_QUEUE_LIST).unwrap());
    // id = type | port number << 8 | index, as in the C++
    assert_eq!(queues[3], RawOid(u64::from(QUEUE) << 56 | 2 << 8 | 3));
    assert_eq!(
        sai.get(
            QUEUE,
            queues[3],
            &[
                sys::SAI_QUEUE_ATTR_TYPE,
                sys::SAI_QUEUE_ATTR_PORT,
                sys::SAI_QUEUE_ATTR_INDEX,
                sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID
            ]
        ),
        Ok(vec![
            enumerator(sys::SAI_QUEUE_TYPE_UNICAST),
            Value::Oid(ports[1]),
            Value::U8(3),
            Value::Oid(RawOid::NULL)
        ])
    );
    // without QoS a queue's parent is its port and the port shows no scheduler groups (the C++
    // fake with qos_on false); with it, one group per queue
    assert_eq!(
        get1(&sai, QUEUE, queues[3], sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE),
        Ok(Value::Oid(ports[1]))
    );
    sai.backend_mut().toggles_mut().qos_on = true;
    let group = oid(get1(&sai, QUEUE, queues[3], sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE).unwrap());
    let sg = sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP;
    assert_eq!(
        sai.get(
            sg,
            group,
            &[
                sys::SAI_SCHEDULER_GROUP_ATTR_CHILD_COUNT,
                sys::SAI_SCHEDULER_GROUP_ATTR_CHILD_LIST,
                sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE,
                sys::SAI_SCHEDULER_GROUP_ATTR_LEVEL
            ]
        ),
        Ok(vec![
            Value::U32(1),
            Value::OidList(vec![queues[3]]),
            Value::Oid(ports[1]),
            Value::U8(0)
        ])
    );
    let stp = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID);
    assert_eq!(
        get1(&sai, sys::SAI_OBJECT_TYPE_STP, stp, sys::SAI_STP_ATTR_VLAN_LIST),
        Ok(Value::VlanList(vec![1]))
    );
    let default_vlan = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    assert_eq!(
        get1(&sai, VLAN, default_vlan, sys::SAI_VLAN_ATTR_STP_INSTANCE),
        Ok(Value::Oid(stp))
    );

    // Everything the switch was built with is what the SAI headers say it can be.
    let store = sai.store().unwrap();
    for object_type in ObjectType::ALL {
        for (id, bag) in store.bags(*object_type) {
            for (attr, value) in &bag.attrs {
                let meta = sai_meta::attribute(object_type.sai(), *attr).unwrap();
                assert_eq!(value.value_type(), meta.value_type, "{id:?} {}", meta.name);
            }
        }
    }
    store.check_invariants().unwrap();
    model_and_chip_agree(&sai).unwrap();

    // The objects that come with the switch stay: referenced by it, or not removable at all.
    let vr = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    assert_eq!(sai.remove(VR, vr), Err(Error::ObjectInUse));
    assert_eq!(sai.remove(sys::SAI_OBJECT_TYPE_STP, stp), Err(Error::NotSupported));
    assert_eq!(sai.remove(QUEUE, queues[0]), Err(Error::NotSupported));
    assert_eq!(sai.create(QUEUE, switch, &[]), Err(Error::NotSupported));
    assert_eq!(
        sai.create(sys::SAI_OBJECT_TYPE_HASH, switch, &[]),
        Err(Error::NotSupported)
    );
}

#[test]
fn switch_attributes_syncd_and_orchagent_read_at_start() {
    let (mut sai, switch) = switch(4);
    let get = |sai: &Sai<VirtualBackend>, attr| get1(sai, SWITCH, switch, attr);
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO),
        Ok(Value::I8List(vec![]))
    );
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_PORT_MAX_MTU), Ok(Value::U32(9408)));
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES),
        Ok(Value::U8(8))
    );
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_NUMBER_OF_QUEUES), Ok(Value::U32(8)));
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_NUMBER_OF_UNICAST_QUEUES),
        Ok(Value::U32(8))
    );
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_NUMBER_OF_MULTICAST_QUEUES),
        Ok(Value::U32(0))
    );
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY),
        Ok(Value::U32(0))
    );
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY),
        Ok(Value::U32(0x7fff_ffff))
    );
    // packet action, counter, policer (C++ acl_ingress_actions)
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT), Ok(Value::U32(3)));
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_ACL_STAGE_INGRESS),
        Ok(Value::AclCapability {
            action_list_mandatory: false,
            actions: [
                sys::SAI_ACL_ACTION_TYPE_PACKET_ACTION,
                sys::SAI_ACL_ACTION_TYPE_COUNTER,
                sys::SAI_ACL_ACTION_TYPE_SET_POLICER
            ]
            .map(u32::cast_signed)
            .to_vec(),
            non_contiguous_exact_match: false,
            match_type: 0
        })
    );
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS),
        Ok(Value::U32(4096))
    );
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_ECMP_MEMBERS), Ok(Value::U32(64)));
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_MAX_ECMP_MEMBER_COUNT),
        Ok(Value::U32(16384))
    );
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE), Ok(Value::U64(16384)));
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY),
        Ok(Value::U32(32768))
    );
    // kept attributes answer the header's default until set, then what was set
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_FDB_AGING_TIME), Ok(Value::U32(0)));
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_ALGORITHM),
        Ok(enumerator(sys::SAI_HASH_ALGORITHM_CRC))
    );
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_VXLAN_DEFAULT_PORT), Ok(Value::U16(4789)));
    sai.set(
        SWITCH,
        switch,
        &Attr::new(sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, Value::U32(600)),
    )
    .unwrap();
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_FDB_AGING_TIME), Ok(Value::U32(600)));
    // what the C++ does not answer either
    assert_eq!(get(&sai, sys::SAI_SWITCH_ATTR_MAX_STP_INSTANCE), Err(unsupported(0)));
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_VXLAN_DEFAULT_ROUTER_MAC),
        Err(unsupported(0))
    );
    assert_eq!(
        get(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_SNAT_ENTRY),
        Err(unsupported(0))
    );

    // Port attributes with a chip effect go through the backend and are kept.
    let ports = oid_list(get(&sai, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let fec = sys::SAI_PORT_ATTR_FEC_MODE;
    assert_eq!(
        get1(&sai, PORT, ports[0], fec),
        Ok(enumerator(sys::SAI_PORT_FEC_MODE_NONE))
    );
    sai.backend_mut().fail_nth("port_fec_set", 1, Error::Failure).unwrap();
    let rs = Attr::new(fec, enumerator(sys::SAI_PORT_FEC_MODE_RS));
    assert_eq!(sai.set(PORT, ports[0], &rs), Err(Error::Failure));
    assert_eq!(
        get1(&sai, PORT, ports[0], fec),
        Ok(enumerator(sys::SAI_PORT_FEC_MODE_NONE))
    );
    sai.set(PORT, ports[0], &rs).unwrap();
    assert_eq!(
        get1(&sai, PORT, ports[0], fec),
        Ok(enumerator(sys::SAI_PORT_FEC_MODE_RS))
    );
    assert_eq!(
        sai.set(PORT, ports[0], &Attr::new(fec, Value::Enum(99))),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
}

#[test]
fn attribute_bag_objects_follow_the_metadata_and_the_reference_rules() {
    let (mut sai, switch) = switch(4);
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let default_vr = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);

    // a virtual router: no mandatory attribute; defaults on get
    let vr = sai.create(VR, switch, &[]).unwrap();
    assert_ne!(vr, default_vr);
    assert_eq!(
        get1(&sai, VR, vr, sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE),
        Ok(Value::Bool(true))
    );
    // "attrvalue SAI_SWITCH_ATTR_SRC_MAC_ADDRESS"
    sai.set(
        SWITCH,
        switch,
        &Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac([2, 0, 0, 0, 0, 9])),
    )
    .unwrap();
    assert_eq!(
        get1(&sai, VR, vr, sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS),
        Ok(Value::Mac([2, 0, 0, 0, 0, 9]))
    );

    // a router interface: metadata rules first
    assert_eq!(sai.create(RIF, switch, &[]), Err(Error::MandatoryAttributeMissing));
    let mut attrs = port_rif(vr, ports[0]);
    // the condition the tables do not carry: a port interface needs its port
    assert_eq!(
        sai.create(RIF, switch, &attrs[..2]),
        Err(Error::MandatoryAttributeMissing)
    );
    // an id of a type the attribute does not accept; a dangling id; an enumerator that is none
    attrs[0].value = Value::Oid(ports[1]);
    assert_eq!(
        sai.create(RIF, switch, &attrs),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    attrs[0].value = Value::Oid(RawOid(vr.0 + 50));
    assert_eq!(
        sai.create(RIF, switch, &attrs),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    attrs[0].value = Value::Oid(vr);
    attrs[1].value = Value::Enum(1000);
    // a type the C++ does not implement (sai_l3.cpp: NOT_SUPPORTED)
    assert_eq!(sai.create(RIF, switch, &attrs), Err(Error::NotSupported));
    // an attribute the C++ does not take (attr_not_supported)
    let mut with_read_only = port_rif(vr, ports[0]);
    with_read_only.push(Attr::new(
        sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE + 1000,
        Value::Bool(true),
    ));
    assert_eq!(
        sai.create(RIF, switch, &with_read_only),
        Err(Error::attr(AttrError::NotSupported, 3))
    );
    let rif = sai.create(RIF, switch, &port_rif(vr, ports[0])).unwrap();
    assert_eq!(rif.sai_object_type(), RIF);
    assert_eq!(
        sai.get(
            RIF,
            rif,
            &[
                sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID,
                sys::SAI_ROUTER_INTERFACE_ATTR_MTU,
                sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS,
                sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE
            ]
        ),
        Ok(vec![
            Value::Oid(ports[0]),
            Value::U32(1514),
            Value::Mac([2, 0, 0, 0, 0, 9]),
            Value::Bool(true)
        ])
    );
    // references hold: the router and (were it removable) the port stay while the interface exists
    assert_eq!(sai.remove(VR, vr), Err(Error::ObjectInUse));
    // set: create-only refused, create-and-set kept
    assert_eq!(
        sai.set(
            RIF,
            rif,
            &Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, Value::Oid(ports[1]))
        ),
        Err(Error::attr(AttrError::NotSupported, 0)) // C++ set_rif: not a stub
    );
    sai.set(
        RIF,
        rif,
        &Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_MTU, Value::U32(9100)),
    )
    .unwrap();
    assert_eq!(
        get1(&sai, RIF, rif, sys::SAI_ROUTER_INTERFACE_ATTR_MTU),
        Ok(Value::U32(9100))
    );
    // a router's own MAC is its interfaces' default, taken when an interface is made (C++)
    sai.set(
        VR,
        vr,
        &Attr::new(sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS, Value::Mac([4; 6])),
    )
    .unwrap();
    assert_eq!(
        get1(&sai, RIF, rif, sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS),
        Ok(Value::Mac([2, 0, 0, 0, 0, 9]))
    );
    model_and_chip_agree(&sai).unwrap();
    // wrong function for the id, stale ids
    assert_eq!(sai.remove(VR, rif), Err(Error::InvalidObjectId)); // C++: no such VR
    sai.remove(RIF, rif).unwrap();
    assert_eq!(sai.remove(RIF, rif), Err(Error::InvalidObjectId));
    assert_eq!(sai.get(RIF, rif, &[]), Err(Error::InvalidObjectId));
    sai.remove(VR, vr).unwrap();
    // ids are not given out again
    assert_ne!(sai.create(VR, switch, &[]).unwrap(), vr);

    // a reference made by set: a queue's scheduler profile
    let scheduler = sai.create(SCHEDULER, switch, &[]).unwrap();
    let queues = oid_list(get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_QOS_QUEUE_LIST).unwrap());
    let profile = sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID;
    sai.set(QUEUE, queues[0], &Attr::new(profile, Value::Oid(scheduler)))
        .unwrap();
    assert_eq!(sai.remove(SCHEDULER, scheduler), Err(Error::ObjectInUse));
    assert_eq!(
        sai.set(
            QUEUE,
            queues[0],
            &Attr::new(profile, Value::Oid(RawOid(scheduler.0 + 9)))
        ),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    sai.set(QUEUE, queues[0], &Attr::new(profile, Value::Oid(RawOid::NULL)))
        .unwrap();
    sai.remove(SCHEDULER, scheduler).unwrap();
    // the same through a port's kept attribute
    let scheduler = sai.create(SCHEDULER, switch, &[]).unwrap();
    let on_port = sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID;
    sai.set(PORT, ports[0], &Attr::new(on_port, Value::Oid(scheduler)))
        .unwrap();
    assert_eq!(sai.remove(SCHEDULER, scheduler), Err(Error::ObjectInUse));
    sai.set(PORT, ports[0], &Attr::new(on_port, Value::Oid(RawOid::NULL)))
        .unwrap();
    sai.remove(SCHEDULER, scheduler).unwrap();

    // host interfaces: a netdev needs its port and its name; one netdev per name
    let hostif = |name: &str, port: RawOid| {
        vec![
            Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, enumerator(sys::SAI_HOSTIF_TYPE_NETDEV)),
            Attr::new(sys::SAI_HOSTIF_ATTR_OBJ_ID, Value::Oid(port)),
            Attr::new(sys::SAI_HOSTIF_ATTR_NAME, Value::CharData(name.into())),
        ]
    };
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("Ethernet0", ports[0])[..2]),
        Err(Error::MandatoryAttributeMissing)
    );
    let ethernet0 = sai.create(HOSTIF, switch, &hostif("Ethernet0", ports[0])).unwrap();
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("Ethernet0", ports[1])),
        Err(Error::ItemAlreadyExists)
    );
    sai.create(HOSTIF, switch, &hostif("Ethernet4", ports[1])).unwrap();
    assert_eq!(
        get1(&sai, HOSTIF, ethernet0, sys::SAI_HOSTIF_ATTR_NAME),
        Ok(Value::CharData("Ethernet0".into()))
    );
    sai.set(
        HOSTIF,
        ethernet0,
        &Attr::new(sys::SAI_HOSTIF_ATTR_OPER_STATUS, Value::Bool(true)),
    )
    .unwrap();
    // traps: one per trap type; the trap group they name is held
    let group = sai.create(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, switch, &[]).unwrap();
    let trap = [
        Attr::new(
            sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE,
            enumerator(sys::SAI_HOSTIF_TRAP_TYPE_LLDP),
        ),
        Attr::new(
            sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
            enumerator(sys::SAI_PACKET_ACTION_TRAP),
        ),
        Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, Value::Oid(group)),
    ];
    let lldp = sai.create(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, switch, &trap).unwrap();
    assert_eq!(
        sai.create(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, switch, &trap),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(
        sai.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, group),
        Err(Error::ObjectInUse)
    );
    sai.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, lldp).unwrap();
    sai.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, group).unwrap();
    model_and_chip_agree(&sai).unwrap();

    // a failing backend leaves no trace, and the id it would have had is still free
    let before = sai.store().unwrap().len();
    sai.backend_mut().fail_nth("chip_limits", 1, Error::TableFull).unwrap();
    assert_eq!(sai.create(VR, switch, &[]), Err(Error::TableFull));
    assert_eq!(sai.store().unwrap().len(), before);
    let made = sai.create(RIF, switch, &port_rif(default_vr, ports[1])).unwrap();
    sai.backend_mut()
        .fail_nth("l3_intf_destroy", 1, Error::Failure)
        .unwrap();
    assert_eq!(sai.remove(RIF, made), Err(Error::Failure));
    assert!(sai.store().unwrap().contains(made));
    model_and_chip_agree(&sai).unwrap();
}

fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(a, b, c, d))
}

#[test]
fn entries_are_objects_without_an_id() {
    let (mut sai, switch) = switch(4);
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let vr = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    let bridge = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    let bridge_ports = oid_list(get1(&sai, BRIDGE, bridge, sys::SAI_BRIDGE_ATTR_PORT_LIST).unwrap());
    let vlan = sai
        .create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(100))])
        .unwrap();
    let free = |sai: &Sai<VirtualBackend>, attr| match get1(sai, SWITCH, switch, attr).unwrap() {
        Value::U32(n) => n,
        other => panic!("{other:?}"),
    };

    // FDB
    let fdb = |mac: u8| EntryKey::Fdb {
        bv_id: vlan,
        mac: [2, 0, 0, 0, 0, mac],
    };
    let fdb_attrs = |entry_type: u32, bridge_port: RawOid| {
        vec![
            Attr::new(sys::SAI_FDB_ENTRY_ATTR_TYPE, enumerator(entry_type)),
            Attr::new(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(bridge_port)),
        ]
    };
    let dynamic = sys::SAI_FDB_ENTRY_TYPE_DYNAMIC;
    let fixed = sys::SAI_FDB_ENTRY_TYPE_STATIC;
    assert_eq!(
        sai.create_entry(switch, fdb(1), &[]),
        Err(Error::MandatoryAttributeMissing)
    );
    // the key must name a VLAN of this switch
    let bad_key = EntryKey::Fdb {
        bv_id: bridge_ports[0],
        mac: [2; 6],
    };
    assert_eq!(
        sai.create_entry(switch, bad_key, &fdb_attrs(dynamic, bridge_ports[0])),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        sai.create_entry(vlan, fdb(1), &fdb_attrs(dynamic, bridge_ports[0])),
        Err(Error::InvalidObjectId)
    );
    // a dangling bridge port
    assert_eq!(
        sai.create_entry(switch, fdb(1), &fdb_attrs(dynamic, RawOid(bridge_ports[0].0 + 100))),
        Err(Error::attr(AttrError::InvalidValue, 1))
    );
    sai.create_entry(switch, fdb(1), &fdb_attrs(dynamic, bridge_ports[0]))
        .unwrap();
    // a dynamic entry is replaced (the C++: it may have been learned), a static one is not
    sai.create_entry(switch, fdb(1), &fdb_attrs(dynamic, bridge_ports[0]))
        .unwrap();
    sai.create_entry(switch, fdb(2), &fdb_attrs(fixed, bridge_ports[0]))
        .unwrap();
    assert_eq!(
        sai.create_entry(switch, fdb(2), &fdb_attrs(fixed, bridge_ports[0])),
        Err(Error::ItemAlreadyExists)
    );
    sai.create_entry(switch, fdb(3), &fdb_attrs(dynamic, bridge_ports[1]))
        .unwrap();
    assert_eq!(free(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY), 32768 - 3);
    assert_eq!(
        sai.get_entry(
            switch,
            &fdb(1),
            &[
                sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID,
                sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION
            ]
        ),
        Ok(vec![
            Value::Oid(bridge_ports[0]),
            enumerator(sys::SAI_PACKET_ACTION_FORWARD)
        ])
    );
    // what the entries name is held: the VLAN (key) and the bridge port (attribute)
    assert_eq!(sai.remove(VLAN, vlan), Err(Error::ObjectInUse));
    assert_eq!(sai.remove(BRIDGE_PORT, bridge_ports[1]), Err(Error::ObjectInUse));
    sai.set_entry(
        switch,
        &fdb(3),
        &Attr::new(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(bridge_ports[2])),
    )
    .unwrap();
    assert_eq!(
        sai.set_entry(
            switch,
            &fdb(9),
            &Attr::new(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(bridge_ports[2]))
        ),
        Err(Error::ItemNotFound)
    );
    model_and_chip_agree(&sai).unwrap();
    // flush: dynamic entries on one bridge port (the default type is dynamic)
    let flushed = sai
        .flush_fdb(
            switch,
            &[Attr::new(
                sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID,
                Value::Oid(bridge_ports[0]),
            )],
        )
        .unwrap();
    assert_eq!(flushed, [fdb(1)]);
    // a failing backend fails the flush: nothing is removed
    sai.backend_mut().fail_nth("l2_flush", 1, Error::Failure).unwrap();
    let all = [Attr::new(
        sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
        enumerator(sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL),
    )];
    assert_eq!(sai.flush_fdb(switch, &all), Err(Error::Failure));
    model_and_chip_agree(&sai).unwrap();
    assert_eq!(sai.flush_fdb(switch, &all).map(|keys| keys.len()), Ok(2));
    assert_eq!(sai.remove_entry(switch, &fdb(2)), Err(Error::ItemNotFound));
    sai.remove(VLAN, vlan).unwrap();

    // neighbors and routes, both families
    let rif = sai.create(RIF, switch, &port_rif(vr, ports[0])).unwrap();
    let neighbor = |ip: IpAddr| EntryKey::Neighbor { rif, ip };
    let mac = [Attr::new(
        sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS,
        Value::Mac([2, 0, 0, 0, 1, 1]),
    )];
    assert_eq!(
        sai.create_entry(switch, neighbor(v4(10, 0, 0, 2)), &[]),
        Err(Error::MandatoryAttributeMissing)
    );
    sai.create_entry(switch, neighbor(v4(10, 0, 0, 2)), &mac).unwrap();
    let v6 = IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 2));
    sai.create_entry(switch, neighbor(v6), &mac).unwrap();
    assert_eq!(
        free(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY),
        16384 - 3
    );
    assert_eq!(
        free(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY),
        (16384 - 3) / 2
    );
    let next_hop = sai
        .create(
            NEXT_HOP,
            switch,
            &[
                Attr::new(sys::SAI_NEXT_HOP_ATTR_TYPE, enumerator(sys::SAI_NEXT_HOP_TYPE_IP)),
                Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::IpAddress(v4(10, 0, 0, 2))),
                Attr::new(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID, Value::Oid(rif)),
            ],
        )
        .unwrap();
    // the chip's free egress objects: one per neighbor, the next hop uses its neighbor's
    assert_eq!(free(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY), 32768 - 2);
    // two spellings of one prefix are one route
    let prefix = |addr: IpAddr| IpPrefix::new(addr, v4(255, 255, 255, 0)).unwrap();
    assert_eq!(prefix(v4(10, 1, 2, 3)), prefix(v4(10, 1, 2, 0)));
    assert!(IpPrefix::new(v6, v4(255, 0, 0, 0)).is_none());
    let route = EntryKey::Route {
        vr,
        prefix: prefix(v4(10, 1, 2, 3)),
    };
    let via = [Attr::new(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, Value::Oid(next_hop))];
    sai.create_entry(switch, route, &via).unwrap();
    assert_eq!(
        sai.create_entry(
            switch,
            EntryKey::Route {
                vr,
                prefix: prefix(v4(10, 1, 2, 0))
            },
            &via
        ),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(free(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY), 16384 - 1);
    assert_eq!(free(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY), 8192);
    assert_eq!(
        sai.get_entry(
            switch,
            &route,
            &[
                sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID,
                sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION
            ]
        ),
        Ok(vec![Value::Oid(next_hop), enumerator(sys::SAI_PACKET_ACTION_FORWARD)])
    );
    // the route holds its next hop, the next hop and the neighbors their interface
    assert_eq!(sai.remove(NEXT_HOP, next_hop), Err(Error::ObjectInUse));
    assert_eq!(sai.remove(RIF, rif), Err(Error::ObjectInUse));
    sai.set_entry(
        switch,
        &route,
        &Attr::new(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, Value::Oid(RawOid::NULL)),
    )
    .unwrap();
    sai.remove(NEXT_HOP, next_hop).unwrap();
    model_and_chip_agree(&sai).unwrap();
    sai.backend_mut().fail_nth("l3_route_add", 1, Error::TableFull).unwrap();
    let other = EntryKey::Route {
        vr,
        prefix: prefix(v4(10, 9, 9, 0)),
    };
    assert_eq!(sai.create_entry(switch, other, &[]), Err(Error::TableFull));
    assert_eq!(sai.get_entry(switch, &other, &[]), Err(Error::ItemNotFound));
    sai.remove_entry(switch, &route).unwrap();
    sai.remove_entry(switch, &neighbor(v6)).unwrap();
    sai.remove_entry(switch, &neighbor(v4(10, 0, 0, 2))).unwrap();
    sai.remove(RIF, rif).unwrap();
    model_and_chip_agree(&sai).unwrap();
    assert_eq!(sai.store().unwrap().entry_count(), 0);
}

#[test]
fn counters() {
    let (mut sai, switch) = switch(4);
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let cpu = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_CPU_PORT);
    let (octets, pkts) = (sys::SAI_PORT_STAT_IF_IN_OCTETS, sys::SAI_PORT_STAT_IF_OUT_UCAST_PKTS);
    assert_eq!(sai.stats_get(PORT, ports[1], &[octets, pkts], false), Ok(vec![0, 0]));
    assert_eq!(sai.stats_get(PORT, ports[1], &[], false), Ok(vec![]));
    sai.backend_mut()
        .set_counter(StatsObject::Port(HwPort(2)), octets, 1500);
    // in the order asked
    assert_eq!(sai.stats_get(PORT, ports[1], &[pkts, octets], false), Ok(vec![0, 1500]));
    assert_eq!(sai.stats_get(PORT, ports[0], &[octets], false), Ok(vec![0]));
    // no read-and-clear, no per-counter clear (C++: the SDK has none); clear_port_all_stats
    assert_eq!(sai.stats_get(PORT, ports[1], &[octets], true), Err(Error::NotSupported));
    assert_eq!(sai.stats_clear(PORT, ports[1], &[octets]), Err(Error::NotSupported));
    sai.clear_port_all_stats(ports[1]).unwrap();
    assert_eq!(sai.stats_get(PORT, ports[1], &[octets], false), Ok(vec![0]));
    // a counter the chip does not have; an id that is no counter of ports; a wrong object
    assert_eq!(
        sai.stats_get(PORT, ports[1], &[sys::SAI_PORT_STAT_ETHER_STATS_JABBERS], false),
        Err(Error::NotSupported)
    );
    assert_eq!(
        sai.stats_get(PORT, ports[1], &[0x7fff_fff0], false),
        Err(Error::NotSupported)
    );
    // C++: the CPU port has no counters (INVALID_OBJECT_ID)
    assert_eq!(sai.stats_get(PORT, cpu, &[octets], false), Err(Error::InvalidObjectId));
    // C++ port_of: an id of another type is no port (INVALID_OBJECT_ID)
    assert_eq!(
        sai.stats_get(PORT, switch, &[octets], false),
        Err(Error::InvalidObjectId)
    );
    assert_eq!(sai.stats_get(SWITCH, switch, &[], false), Err(Error::NotSupported));
    assert_eq!(
        sai.stats_clear(VLAN_MEMBER, RawOid(u64::from(VLAN_MEMBER) << 56 | 1), &[]),
        Err(Error::NotSupported)
    );
    // queues and priority groups are counted per port and index, with the QoS model only (C++
    // fake: qos_on; without it NOT_SUPPORTED and no counters advertised)
    let queues = oid_list(get1(&sai, PORT, ports[1], sys::SAI_PORT_ATTR_QOS_QUEUE_LIST).unwrap());
    assert_eq!(
        sai.stats_get(QUEUE, queues[5], &[sys::SAI_QUEUE_STAT_PACKETS], false),
        Err(Error::NotSupported)
    );
    assert_eq!(sai.stats_capability(QUEUE).map(|c| c.len()), Err(Error::NotSupported));
    sai.backend_mut().toggles_mut().qos_on = true;
    let queue = StatsObject::Queue {
        port: HwPort(2),
        index: 5,
    };
    sai.backend_mut().set_counter(queue, sys::SAI_QUEUE_STAT_PACKETS, 42);
    assert_eq!(
        sai.stats_get(QUEUE, queues[5], &[sys::SAI_QUEUE_STAT_PACKETS], false),
        Ok(vec![42])
    );
    assert_eq!(
        sai.stats_get(QUEUE, queues[4], &[sys::SAI_QUEUE_STAT_PACKETS], false),
        Ok(vec![0])
    );
    assert_eq!(
        sai.stats_get(QUEUE, queues[4], &[sys::SAI_QUEUE_STAT_WRED_DROPPED_PACKETS], false),
        Err(Error::NotSupported)
    );
    let groups = oid_list(get1(&sai, PORT, ports[1], sys::SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST).unwrap());
    assert_eq!(
        sai.stats_get(
            sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP,
            groups[0],
            &[sys::SAI_INGRESS_PRIORITY_GROUP_STAT_BYTES],
            false
        ),
        Ok(vec![0])
    );
    // what the chip counts (with the QoS model: the ports' 16 PFC frame counters too)
    assert_eq!(sai.stats_capability(PORT).map(|c| c.len()), Ok(31));
    assert_eq!(sai.stats_capability(QUEUE).map(|c| c.len()), Ok(3));
    // the RIF counters the chip has (C++: probed, an empty list)
    assert_eq!(sai.stats_capability(RIF), Ok(vec![]));
}

#[test]
fn every_advertised_get_works_on_a_live_object() {
    let (mut sai, switch) = switch(4);
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let vr = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    let default_vlan = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    let bridge = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    let queues = oid_list(get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_QOS_QUEUE_LIST).unwrap());
    let rif = sai.create(RIF, switch, &port_rif(vr, ports[0])).unwrap();
    let next_hop = sai
        .create(
            NEXT_HOP,
            switch,
            &[
                Attr::new(sys::SAI_NEXT_HOP_ATTR_TYPE, enumerator(sys::SAI_NEXT_HOP_TYPE_IP)),
                Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::IpAddress(v4(10, 0, 0, 2))),
                Attr::new(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID, Value::Oid(rif)),
            ],
        )
        .unwrap();
    let group = sai
        .create(
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP,
            switch,
            &[Attr::new(
                sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE,
                enumerator(sys::SAI_NEXT_HOP_GROUP_TYPE_ECMP),
            )],
        )
        .unwrap();
    let member = sai
        .create(
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER,
            switch,
            &[
                Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID, Value::Oid(group)),
                Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID, Value::Oid(next_hop)),
            ],
        )
        .unwrap();
    assert_eq!(
        sai.get(
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP,
            group,
            &[
                sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_COUNT,
                sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_MEMBER_LIST
            ]
        ),
        Ok(vec![Value::U32(1), Value::OidList(vec![member])])
    );
    let hostif = sai
        .create(
            HOSTIF,
            switch,
            &[
                Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, enumerator(sys::SAI_HOSTIF_TYPE_NETDEV)),
                Attr::new(sys::SAI_HOSTIF_ATTR_OBJ_ID, Value::Oid(ports[0])),
                Attr::new(sys::SAI_HOSTIF_ATTR_NAME, Value::CharData("Ethernet0".into())),
            ],
        )
        .unwrap();
    let (lag, lag_member) = lag_on_last_port(&mut sai, switch, &ports);
    // the lists again: the last port's default bridge port and VLAN 1 membership are gone
    let bridge_ports = oid_list(get1(&sai, BRIDGE, bridge, sys::SAI_BRIDGE_ATTR_PORT_LIST).unwrap());
    let members = oid_list(get1(&sai, VLAN, default_vlan, sys::SAI_VLAN_ATTR_MEMBER_LIST).unwrap());
    let trap_group = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP);
    let trap = sai
        .create(
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP,
            switch,
            &[
                Attr::new(
                    sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE,
                    enumerator(sys::SAI_HOSTIF_TRAP_TYPE_LLDP),
                ),
                Attr::new(
                    sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
                    enumerator(sys::SAI_PACKET_ACTION_TRAP),
                ),
            ],
        )
        .unwrap();
    let policer = sai
        .create(
            sys::SAI_OBJECT_TYPE_POLICER,
            switch,
            &[
                Attr::new(
                    sys::SAI_POLICER_ATTR_METER_TYPE,
                    enumerator(sys::SAI_METER_TYPE_PACKETS),
                ),
                Attr::new(sys::SAI_POLICER_ATTR_MODE, enumerator(sys::SAI_POLICER_MODE_SR_TCM)),
                // a meter that passes nothing is refused (the C++ fake's rule)
                Attr::new(sys::SAI_POLICER_ATTR_CIR, Value::U64(600)),
            ],
        )
        .unwrap();
    let stp = switch_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID);
    let vlan = sai
        .create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(100))])
        .unwrap();
    let fdb = EntryKey::Fdb {
        bv_id: vlan,
        mac: [2; 6],
    };
    sai.create_entry(
        switch,
        fdb,
        &[
            Attr::new(sys::SAI_FDB_ENTRY_ATTR_TYPE, enumerator(sys::SAI_FDB_ENTRY_TYPE_STATIC)),
            Attr::new(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(bridge_ports[0])),
        ],
    )
    .unwrap();
    let neighbor = EntryKey::Neighbor {
        rif,
        ip: v4(10, 0, 0, 2),
    };
    sai.create_entry(
        switch,
        neighbor,
        &[Attr::new(
            sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS,
            Value::Mac([2; 6]),
        )],
    )
    .unwrap();
    let route = EntryKey::Route {
        vr,
        prefix: IpPrefix::new(v4(10, 2, 0, 0), v4(255, 255, 0, 0)).unwrap(),
    };
    sai.create_entry(switch, route, &[]).unwrap();
    // ACL: a table, its counter, an entry counting into it, a group with the table
    let acl_table = sai
        .create(
            sys::SAI_OBJECT_TYPE_ACL_TABLE,
            switch,
            &[
                Attr::new(
                    sys::SAI_ACL_TABLE_ATTR_ACL_STAGE,
                    enumerator(sys::SAI_ACL_STAGE_INGRESS),
                ),
                Attr::new(sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP, Value::Bool(true)),
            ],
        )
        .unwrap();
    let acl_counter = sai
        .create(
            sys::SAI_OBJECT_TYPE_ACL_COUNTER,
            switch,
            &[Attr::new(sys::SAI_ACL_COUNTER_ATTR_TABLE_ID, Value::Oid(acl_table))],
        )
        .unwrap();
    let acl_entry = sai
        .create(
            sys::SAI_OBJECT_TYPE_ACL_ENTRY,
            switch,
            &[
                Attr::new(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, Value::Oid(acl_table)),
                Attr::new(
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER,
                    Value::AclAction(sai_core::value::AclAction::Enabled(sai_core::value::AclParam::Oid(
                        acl_counter,
                    ))),
                ),
            ],
        )
        .unwrap();
    let acl_group = sai
        .create(
            sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP,
            switch,
            &[Attr::new(
                sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE,
                enumerator(sys::SAI_ACL_STAGE_INGRESS),
            )],
        )
        .unwrap();
    let acl_member = sai
        .create(
            sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER,
            switch,
            &[
                Attr::new(
                    sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID,
                    Value::Oid(acl_group),
                ),
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, Value::Oid(acl_table)),
            ],
        )
        .unwrap();

    let live = |object_type: sys::sai_object_type_t| match object_type {
        sys::SAI_OBJECT_TYPE_SWITCH => switch,
        sys::SAI_OBJECT_TYPE_PORT => ports[0],
        sys::SAI_OBJECT_TYPE_VLAN => default_vlan,
        sys::SAI_OBJECT_TYPE_VLAN_MEMBER => members[0],
        sys::SAI_OBJECT_TYPE_BRIDGE => bridge,
        sys::SAI_OBJECT_TYPE_BRIDGE_PORT => bridge_ports[0],
        sys::SAI_OBJECT_TYPE_STP => stp,
        sys::SAI_OBJECT_TYPE_QUEUE => queues[0],
        sys::SAI_OBJECT_TYPE_HOSTIF => hostif,
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP => trap,
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP => trap_group,
        sys::SAI_OBJECT_TYPE_POLICER => policer,
        sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER => vr,
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => rif,
        sys::SAI_OBJECT_TYPE_NEXT_HOP => next_hop,
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP => group,
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER => member,
        sys::SAI_OBJECT_TYPE_LAG => lag,
        sys::SAI_OBJECT_TYPE_LAG_MEMBER => lag_member,
        sys::SAI_OBJECT_TYPE_ACL_TABLE => acl_table,
        sys::SAI_OBJECT_TYPE_ACL_ENTRY => acl_entry,
        sys::SAI_OBJECT_TYPE_ACL_COUNTER => acl_counter,
        sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP => acl_group,
        sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER => acl_member,
        other => panic!("no live object of type {other}"),
    };
    let mut failed = Vec::new();
    for (object_type, attr, cap) in caps::attributes() {
        let name = sai_meta::attribute(object_type, attr).unwrap().name;
        if !cap.get {
            continue;
        }
        let answer = match object_type {
            sys::SAI_OBJECT_TYPE_FDB_ENTRY => sai.get_entry(switch, &fdb, &[attr]),
            sys::SAI_OBJECT_TYPE_NEIGHBOR_ENTRY => sai.get_entry(switch, &neighbor, &[attr]),
            sys::SAI_OBJECT_TYPE_ROUTE_ENTRY => sai.get_entry(switch, &route, &[attr]),
            _ => sai.get(object_type, live(object_type), &[attr]),
        };
        if let Err(e) = answer {
            failed.push(format!("{name}: {e}"));
        }
    }
    assert!(failed.is_empty(), "advertised and not answered: {failed:#?}");
}

#[test]
fn a_backend_without_the_optional_operations_refuses_instead_of_pretending() {
    // Boxed, as the C-ABI rim holds it: the forwarding impl must not hide the refusals.
    let backend: Box<dyn Backend> = Box::new(Refusing(VirtualBackend::new()));
    let mut sai = Sai::new(backend);
    let switch = sai.create_switch(&init_attrs(), &profile(2)).unwrap();
    let switch = sai_core::oid::TypedId::raw(switch);
    let get = |sai: &Sai<Box<dyn Backend>>, t, id, attr| sai.get(t, id, &[attr]).map(|mut v| v.remove(0));
    // The switch still has its default objects, 8 queues per port (the C++ builds them whatever the chip).
    let vr = oid(get(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID).unwrap());
    let ports = oid_list(get(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    assert_eq!(
        get(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_QOS_NUMBER_OF_QUEUES),
        Ok(Value::U32(8))
    );
    assert_eq!(
        get(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_NUMBER_OF_QUEUES),
        Ok(Value::U32(8))
    );
    // Nothing is stored that the chip was not given.
    let before = sai.store().unwrap().len();
    assert_eq!(sai.create(VR, switch, &[]), Err(Error::NotSupported));
    assert_eq!(
        sai.create(RIF, switch, &port_rif(vr, ports[0])),
        Err(Error::NotSupported)
    );
    // (a VR's ADMIN state is no chip call: the C++ answers it itself)
    assert_eq!(
        sai.set(
            VR,
            vr,
            &Attr::new(sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE, Value::Bool(true))
        ),
        Ok(())
    );
    assert_eq!(
        sai.set(
            PORT,
            ports[0],
            &Attr::new(sys::SAI_PORT_ATTR_FEC_MODE, enumerator(sys::SAI_PORT_FEC_MODE_RS))
        ),
        Err(unsupported(0))
    );
    let route = EntryKey::Route {
        vr,
        prefix: IpPrefix::new(v4(10, 0, 0, 0), v4(255, 0, 0, 0)).unwrap(),
    };
    assert_eq!(sai.create_entry(switch, route, &[]), Err(Error::NotSupported));
    assert_eq!(sai.store().unwrap().len(), before);
    assert_eq!(sai.store().unwrap().entry_count(), 0);
    sai.store().unwrap().check_invariants().unwrap();
    // No table sizes, no counters.
    assert_eq!(
        get(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY),
        Err(unsupported(0))
    );
    assert_eq!(
        sai.stats_get(PORT, ports[0], &[sys::SAI_PORT_STAT_IF_IN_OCTETS], false),
        Err(Error::NotSupported)
    );
    // the C++ lists the 15 IF_* counters whatever the chip says
    assert_eq!(sai.stats_capability(PORT).map(|c| c.len()), Ok(15));
    // The first slice is untouched by all of this.
    sai.set(
        PORT,
        ports[0],
        &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
    )
    .unwrap();
    sai.create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(7))])
        .unwrap();
}

#[test]
fn a_backend_that_refuses_bag_objects_does_not_advertise_them() {
    let refusing: Box<dyn Backend> = Box::new(Refusing(VirtualBackend::new()));
    let mut sai = Sai::new(refusing);
    let switch = sai_core::oid::TypedId::raw(sai.create_switch(&init_attrs(), &profile(2)).unwrap());
    let (virt, _) = common::switch(2);
    // The C++ answers the capability queries from its tables, whatever the backend programs.
    for (object_type, attr, full) in caps::attributes() {
        let name = sai_meta::attribute(object_type, attr).unwrap().name;
        assert_eq!(virt.attribute_capability(object_type, attr), full, "{name}");
        assert_eq!(sai.attribute_capability(object_type, attr), full, "{name}");
    }
    let _ = switch;
    assert_eq!(
        sai.enum_values_capability(HOSTIF, sys::SAI_HOSTIF_ATTR_TYPE),
        virt.enum_values_capability(HOSTIF, sys::SAI_HOSTIF_ATTR_TYPE)
    );
}

const LAG: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_LAG;
const LAG_MEMBER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_LAG_MEMBER;

fn lag_member_attrs(lag: RawOid, port: RawOid) -> Vec<Attr> {
    vec![
        Attr::new(sys::SAI_LAG_MEMBER_ATTR_LAG_ID, Value::Oid(lag)),
        Attr::new(sys::SAI_LAG_MEMBER_ATTR_PORT_ID, Value::Oid(port)),
    ]
}

/// Removes a port's default VLAN 1 membership and bridge port, as orchagent does before it
/// puts the port into a LAG.
fn unbridge(sai: &mut Sai<VirtualBackend>, switch: RawOid, port: RawOid) {
    let vlan = oid(get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID).unwrap());
    let bridge = oid(get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID).unwrap());
    let bridge_port = oid_list(get1(sai, BRIDGE, bridge, sys::SAI_BRIDGE_ATTR_PORT_LIST).unwrap())
        .into_iter()
        .find(|bp| get1(sai, BRIDGE_PORT, *bp, sys::SAI_BRIDGE_PORT_ATTR_PORT_ID) == Ok(Value::Oid(port)))
        .unwrap();
    for member in oid_list(get1(sai, VLAN, vlan, sys::SAI_VLAN_ATTR_MEMBER_LIST).unwrap()) {
        if get1(sai, VLAN_MEMBER, member, sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID) == Ok(Value::Oid(bridge_port)) {
            sai.remove(VLAN_MEMBER, member).unwrap();
        }
    }
    sai.remove(BRIDGE_PORT, bridge_port).unwrap();
}

/// A LAG with the last port as its member.
fn lag_on_last_port(sai: &mut Sai<VirtualBackend>, switch: RawOid, ports: &[RawOid]) -> (RawOid, RawOid) {
    let port = *ports.last().unwrap();
    unbridge(sai, switch, port);
    let lag = sai.create(LAG, switch, &[]).unwrap();
    let member = sai.create(LAG_MEMBER, switch, &lag_member_attrs(lag, port)).unwrap();
    (lag, member)
}

#[test]
fn lags_follow_the_cpp_rules() {
    let (mut sai, switch) = switch(4);
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let cpu = oid(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap());
    let vr = oid(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID).unwrap());
    let lag = sai.create(LAG, switch, &[]).unwrap();
    let other = sai.create(LAG, switch, &[]).unwrap();
    // a port that still has its default bridge port carries L2 state of its own
    assert_eq!(
        sai.create(LAG_MEMBER, switch, &lag_member_attrs(lag, ports[0])),
        Err(Error::ObjectInUse)
    );
    // a front-panel port only; the LAG must be one
    assert_eq!(
        sai.create(LAG_MEMBER, switch, &lag_member_attrs(lag, cpu)),
        Err(Error::attr(AttrError::InvalidValue, 1))
    );
    assert!(sai.create(LAG_MEMBER, switch, &lag_member_attrs(vr, ports[0])).is_err());
    unbridge(&mut sai, switch, ports[0]);
    unbridge(&mut sai, switch, ports[1]);
    let m0 = sai
        .create(LAG_MEMBER, switch, &lag_member_attrs(lag, ports[0]))
        .unwrap();
    // one LAG per port
    assert_eq!(
        sai.create(LAG_MEMBER, switch, &lag_member_attrs(other, ports[0])),
        Err(Error::ItemAlreadyExists)
    );
    let m1 = sai
        .create(LAG_MEMBER, switch, &lag_member_attrs(lag, ports[1]))
        .unwrap();
    // the members (LAG_MEMBER objects, as the header and the C++ say), in creation order
    assert_eq!(
        get1(&sai, LAG, lag, sys::SAI_LAG_ATTR_PORT_LIST),
        Ok(Value::OidList(vec![m0, m1]))
    );
    assert_eq!(
        get1(&sai, LAG, other, sys::SAI_LAG_ATTR_PORT_LIST),
        Ok(Value::OidList(vec![]))
    );
    // members are switched on and off for LACP
    sai.set(
        LAG_MEMBER,
        m1,
        &Attr::new(sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, Value::Bool(true)),
    )
    .unwrap();
    assert_eq!(
        get1(&sai, LAG_MEMBER, m1, sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        get1(&sai, LAG_MEMBER, m0, sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE),
        Ok(Value::Bool(false))
    );
    // a router interface on the LAG (a PortChannel with an address) holds it
    let rif = sai.create(RIF, switch, &port_rif(vr, lag)).unwrap();
    assert_eq!(sai.remove(LAG, lag), Err(Error::ObjectInUse));
    sai.remove(RIF, rif).unwrap();
    // so do its members
    assert_eq!(sai.remove(LAG, lag), Err(Error::ObjectInUse));
    sai.remove(LAG_MEMBER, m0).unwrap();
    sai.remove(LAG_MEMBER, m1).unwrap();
    sai.remove(LAG, lag).unwrap();
    // the port is free again
    sai.create(LAG_MEMBER, switch, &lag_member_attrs(other, ports[0]))
        .unwrap();
    model_and_chip_agree(&sai).unwrap();
}
