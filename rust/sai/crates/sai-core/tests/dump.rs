//! The dump text (`sai_dbg_generate_dump`, [`sai_core::dump`]) on the virtual backend: the
//! lines of the C++ `sai_api.cpp` dump.
// Test code: a failed unwrap, index or panic is the test failing.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
use sai_core::backend::Profile;
use sai_core::dump;
use sai_core::oid::{RawOid, TypedId};
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;

/// The bridge port of `port` on the default .1Q bridge (through the attributes).
fn bridge_port_of(sai: &Sai<VirtualBackend>, switch: RawOid, port: RawOid) -> RawOid {
    let one = |object_type, id, attr| sai.get(object_type, id, &[attr]).unwrap().remove(0);
    let Value::Oid(bridge) = one(
        sys::SAI_OBJECT_TYPE_SWITCH,
        switch,
        sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID,
    ) else {
        panic!("no default bridge")
    };
    let Value::OidList(members) = one(sys::SAI_OBJECT_TYPE_BRIDGE, bridge, sys::SAI_BRIDGE_ATTR_PORT_LIST) else {
        panic!("PORT_LIST is not a list")
    };
    members
        .into_iter()
        .find(|bp| one(sys::SAI_OBJECT_TYPE_BRIDGE_PORT, *bp, sys::SAI_BRIDGE_PORT_ATTR_PORT_ID) == Value::Oid(port))
        .unwrap()
}

fn text(sai: &Sai<VirtualBackend>) -> String {
    dump::text(sai.backend(), sai.store(), sai.switch_id())
}

#[test]
fn without_a_switch_the_dump_has_no_backend_and_no_objects() {
    let sai = Sai::new(VirtualBackend::new());
    assert_eq!(
        text(&sai),
        "sai-openbcm dump\n\
         backend none\n\
         switch 0 mac 02:00:00:00:00:01\n\
         bridge_ports 0 fdb 0 hostifs 0 traps 0 rifs 0 neighbors 0 next_hops 0 routes 0 lags 0 lag_members 0 \
         virtual_routers 0 next_hop_groups 0 next_hop_group_members 0\n\
         policers 0 acl_tables 0 acl_entries 0 acl_counters 0 acl_table_groups 0 acl_bindings 0 copp_entries 0\n"
    );
}

#[test]
fn a_virtual_switch_dumps_the_cpp_lines() {
    let mut sai = Sai::new(VirtualBackend::new());
    let profile = Profile::from_pairs([("SAI_OPENBCM_FAKE_PORTS", "2")]);
    let attrs = [
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(
            sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS,
            Value::Mac([0x02, 0x11, 0x22, 0x33, 0x44, 0x5f]),
        ),
    ];
    let switch = sai.create_switch(&attrs, &profile).unwrap().raw();
    let events = sai.take_events().unwrap();
    let Value::OidList(ports) = sai
        .get(sys::SAI_OBJECT_TYPE_SWITCH, switch, &[sys::SAI_SWITCH_ATTR_PORT_LIST])
        .unwrap()
        .remove(0)
    else {
        panic!("PORT_LIST is not a list")
    };
    assert_eq!(ports.len(), 2);

    // port 0: admin up (the virtual link follows), MTU 9100, PVID 10
    let port = sys::SAI_OBJECT_TYPE_PORT;
    sai.set(
        port,
        ports[0],
        &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
    )
    .unwrap();
    sai.set(port, ports[0], &Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(9100)))
        .unwrap();
    sai.set(
        port,
        ports[0],
        &Attr::new(sys::SAI_PORT_ATTR_PORT_VLAN_ID, Value::U16(10)),
    )
    .unwrap();
    for event in events.try_iter() {
        sai.handle_event(event);
    }
    // VLAN 10 with port 0 tagged; a LAG without members
    let vlan = sai
        .create(
            sys::SAI_OBJECT_TYPE_VLAN,
            switch,
            &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(10))],
        )
        .unwrap();
    let bridge_port = bridge_port_of(&sai, switch, ports[0]);
    sai.create(
        sys::SAI_OBJECT_TYPE_VLAN_MEMBER,
        switch,
        &[
            Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(vlan)),
            Attr::new(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(bridge_port)),
            Attr::new(
                sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
                Value::Enum(sys::SAI_VLAN_TAGGING_MODE_TAGGED.cast_signed()),
            ),
        ],
    )
    .unwrap();
    let lag: RawOid = sai.create(sys::SAI_OBJECT_TYPE_LAG, switch, &[]).unwrap();

    let dump = text(&sai);
    let lines: Vec<&str> = dump.lines().collect();
    let expected = [
        "sai-openbcm dump".to_owned(),
        "backend fake".to_owned(),
        format!("switch {:x} mac 02:11:22:33:44:5f", switch.0),
        format!(
            "port {:x} hw 1 ce0 lanes 1 2 3 4 admin 1 oper {} pvid 10 mtu 9100",
            ports[0].0,
            sys::SAI_PORT_OPER_STATUS_UP
        ),
        format!(
            "port {:x} hw 2 ce1 lanes 5 6 7 8 admin 0 oper {} pvid 1 mtu 1514",
            ports[1].0,
            sys::SAI_PORT_OPER_STATUS_DOWN
        ),
        "vlan 1 members 2".to_owned(),
        "vlan 10 members 1".to_owned(),
        "bridge_ports 2 fdb 0 hostifs 0 traps 0 rifs 0 neighbors 0 next_hops 0 routes 0 lags 1 lag_members 0 \
         virtual_routers 1 next_hop_groups 0 next_hop_group_members 0"
            .to_owned(),
        "policers 0 acl_tables 0 acl_entries 0 acl_counters 0 acl_table_groups 0 acl_bindings 0 copp_entries 0"
            .to_owned(),
        format!("lag {:x} trunk -1 members 0 pvid 1", lag.0),
    ];
    assert_eq!(lines, expected.iter().map(String::as_str).collect::<Vec<_>>());
    // the C++ prints ids with std::hex: lower case, no prefix, no padding
    assert!(lines[2].starts_with("switch 21"), "{}", lines[2]);
    assert!(dump.ends_with('\n'));
}
