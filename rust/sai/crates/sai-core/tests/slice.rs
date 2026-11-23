//! Behaviour of the first slice on the virtual backend: switch, ports, VLANs, VLAN members,
//! bridge ports; error paths; what a failing backend leaves behind (nothing).
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
use sai_core::backend::{BackendEvent, HwPort, Profile, Vid};
use sai_core::error::{AttrError, Error};
use sai_core::oid::{PortId, RawOid, TypedId, VlanId};
use sai_core::sai::{Notification, Sai};
use sai_core::value::{Attr, Pointer, Value};
use sai_core::virt::{Op, VirtualBackend};
use sai_sys as sys;

fn vlan(sai: &mut Sai<VirtualBackend>, switch: RawOid, vid: u16) -> sai_core::Result<RawOid> {
    sai.create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(vid))])
}

fn member(
    sai: &mut Sai<VirtualBackend>,
    switch: RawOid,
    vlan: RawOid,
    bridge_port: RawOid,
    mode: u32,
) -> sai_core::Result<RawOid> {
    sai.create(
        VLAN_MEMBER,
        switch,
        &[
            Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(vlan)),
            Attr::new(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(bridge_port)),
            Attr::new(
                sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
                Value::Enum(mode.cast_signed()),
            ),
        ],
    )
}

fn bridge_ports(sai: &Sai<VirtualBackend>, switch: RawOid) -> Vec<RawOid> {
    let Value::Oid(bridge) = get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID).unwrap() else {
        panic!()
    };
    oid_list(get1(sai, BRIDGE, bridge, sys::SAI_BRIDGE_ATTR_PORT_LIST).unwrap())
}

#[test]
fn switch_creation_is_a_second_stage_with_its_own_rules() {
    let mut sai = Sai::new(VirtualBackend::new());
    // before a switch exists
    assert_eq!(vlan(&mut sai, RawOid(0x21 << 56), 10), Err(Error::Uninitialized));
    assert_eq!(
        sai.get(SWITCH, RawOid(0x21 << 56), &[sys::SAI_SWITCH_ATTR_PORT_NUMBER]),
        Err(Error::Uninitialized)
    );

    assert_eq!(
        sai.create_switch(&[], &Profile::default()),
        Err(Error::MandatoryAttributeMissing)
    );
    let attach = [
        Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac([2; 6])),
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(false)),
    ];
    // C++: INIT_SWITCH=false is NOT_SUPPORTED, not indexed
    assert_eq!(
        sai.create_switch(&attach, &Profile::default()),
        Err(Error::NotSupported)
    );
    // an attribute this library does nothing with is refused by index, not dropped
    let extra = [
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(sys::SAI_SWITCH_ATTR_CRC_CHECK_ENABLE, Value::Bool(false)),
    ];
    assert_eq!(
        sai.create_switch(&extra, &Profile::default()),
        Err(Error::attr(AttrError::NotSupported, 1))
    );
    // only an NPU
    let phy = [
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(
            sys::SAI_SWITCH_ATTR_TYPE,
            Value::Enum(sys::SAI_SWITCH_TYPE_PHY.cast_signed()),
        ),
    ];
    assert_eq!(
        sai.create_switch(&phy, &Profile::default()),
        Err(Error::attr(AttrError::InvalidValue, 1)) // C++ attr_invalid
    );
    // a profile typo is an error, not the default
    let typo = Profile::from_pairs([("SAI_RS_VIRTUAL_PORTS", "3x")]);
    assert_eq!(sai.create_switch(&init_attrs(), &typo), Err(Error::InvalidParameter));
    assert!(sai.store().is_none());

    let attrs = [
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac([0, 1, 2, 3, 4, 5])),
        Attr::new(
            sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
            Value::Pointer(Pointer::from_addr(0x1000)),
        ),
    ];
    let id = sai.create_switch(&attrs, &Profile::default()).unwrap().raw();
    assert_eq!(
        sai.create_switch(&attrs, &Profile::default()),
        Err(Error::InsufficientResources) // C++: "only one switch is supported"
    );
    assert_eq!(
        sai.switch_callback(sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY),
        Pointer::from_addr(0x1000)
    );
    assert_eq!(
        sai.switch_callback(sys::SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY),
        Pointer::NULL
    );

    let got = sai
        .get(
            SWITCH,
            id,
            &[
                sys::SAI_SWITCH_ATTR_PORT_NUMBER,
                sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS,
                sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
                sys::SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY,
                sys::SAI_SWITCH_ATTR_INIT_SWITCH,
            ],
        )
        .unwrap();
    assert_eq!(
        got,
        [
            Value::U32(32),
            Value::Mac([0, 1, 2, 3, 4, 5]),
            Value::Pointer(Pointer::from_addr(0x1000)),
            Value::Pointer(Pointer::NULL),
            Value::Bool(true)
        ]
    );
    // errors carry the position of the failing attribute
    assert_eq!(
        sai.get(
            SWITCH,
            id,
            &[sys::SAI_SWITCH_ATTR_PORT_NUMBER, sys::SAI_SWITCH_ATTR_MAX_STP_INSTANCE]
        ),
        Err(Error::attr(AttrError::NotSupported, 1))
    );
    assert_eq!(
        sai.get(SWITCH, id, &[sys::SAI_SWITCH_ATTR_PORT_NUMBER, 0x0fff_fff0]),
        Err(Error::attr(AttrError::NotSupported, 1)) // C++ get_one's default
    );
    model_and_chip_agree(&sai).unwrap();

    sai.set(
        SWITCH,
        id,
        &Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac([9; 6])),
    )
    .unwrap();
    assert_eq!(
        get1(&sai, SWITCH, id, sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS),
        Ok(Value::Mac([9; 6]))
    );
    assert_eq!(
        sai.set(
            SWITCH,
            id,
            &Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))
        ),
        Err(Error::attr(AttrError::NotSupported, 0)) // C++ set_switch_attribute's default
    );

    sai.remove(SWITCH, id).unwrap();
    assert!(sai.store().is_none() && sai.backend().vlans().is_empty());
    assert_eq!(sai.remove(SWITCH, id), Err(Error::Uninitialized));
    // and it can be created again
    sai.create_switch(&init_attrs(), &profile(2)).unwrap();
}

#[test]
fn ports_come_from_the_chip_disabled() {
    let (mut sai, switch) = switch(32);
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    assert_eq!(ports.len(), 32);
    assert_eq!(
        get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_HW_LANE_LIST),
        Ok(Value::U32List(vec![1, 2, 3, 4]))
    );
    assert_eq!(
        get1(&sai, PORT, ports[31], sys::SAI_PORT_ATTR_HW_LANE_LIST),
        Ok(Value::U32List(vec![125, 126, 127, 128]))
    );
    for port in &ports {
        assert_eq!(
            get1(&sai, PORT, *port, sys::SAI_PORT_ATTR_ADMIN_STATE),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            get1(&sai, PORT, *port, sys::SAI_PORT_ATTR_OPER_STATUS),
            Ok(Value::Enum(sys::SAI_PORT_OPER_STATUS_DOWN.cast_signed()))
        );
    }
    assert!((1..=32).all(|hw| sai.backend().port_admin(HwPort(hw)) == Some(false)));

    let Value::Oid(cpu) = get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap() else {
        panic!()
    };
    assert!(!ports.contains(&cpu));
    assert_eq!(
        get1(&sai, PORT, cpu, sys::SAI_PORT_ATTR_TYPE),
        Ok(Value::Enum(sys::SAI_PORT_TYPE_CPU.cast_signed()))
    );
    assert_eq!(
        get1(&sai, PORT, cpu, sys::SAI_PORT_ATTR_HW_LANE_LIST),
        Ok(Value::U32List(Vec::new()))
    );
    assert_eq!(
        sai.set(
            PORT,
            cpu,
            &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(false))
        ),
        Err(Error::attr(AttrError::NotSupported, 0))
    );

    // speed: only what the chip supports; a refused value changes nothing
    assert_eq!(
        sai.set(PORT, ports[0], &Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(12345))),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    sai.set(PORT, ports[0], &Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(25_000)))
        .unwrap();
    assert_eq!(
        get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_SPEED),
        Ok(Value::U32(25_000))
    );
    assert_eq!(
        get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_SUPPORTED_SPEED),
        Ok(Value::U32List(vec![10_000, 25_000, 40_000, 50_000, 100_000]))
    );
    // kept attribute: stored and returned; the header's default until it is set
    assert_eq!(get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_MTU), Ok(Value::U32(1514)));
    // an attribute nobody keeps is still refused
    assert_eq!(
        get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_ADVERTISED_SPEED),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    sai.set(PORT, ports[0], &Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(9100)))
        .unwrap();
    assert_eq!(get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_MTU), Ok(Value::U32(9100)));
    // ports are not created or removed (no breakout yet)
    assert_eq!(sai.remove(PORT, ports[0]), Err(Error::NotSupported));
    assert_eq!(sai.create(PORT, switch, &[]), Err(Error::NotSupported));
    model_and_chip_agree(&sai).unwrap();
}

#[test]
fn link_changes_become_notifications_once() {
    let (mut sai, switch) = switch(4);
    let events = sai.take_events().unwrap();
    assert!(sai.take_events().is_none());
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let port = PortId::try_from(ports[1]).unwrap();

    sai.set(
        PORT,
        ports[1],
        &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
    )
    .unwrap();
    assert_eq!(
        get1(&sai, PORT, ports[1], sys::SAI_PORT_ATTR_OPER_STATUS),
        Ok(Value::Enum(sys::SAI_PORT_OPER_STATUS_UP.cast_signed()))
    );
    let event = events.try_recv().unwrap();
    assert_eq!(
        event,
        BackendEvent::Link {
            hw: HwPort(2),
            up: true
        }
    );
    assert_eq!(
        sai.handle_event(event),
        [Notification::PortOperStatus { port, up: true }]
    );
    assert_eq!(sai.handle_event(event), []); // the same state again is not news

    sai.backend_mut().inject_link(HwPort(2), false).unwrap();
    assert_eq!(
        sai.handle_event(events.try_recv().unwrap()),
        [Notification::PortOperStatus { port, up: false }]
    );
    assert!(events.try_recv().is_err());
    // an event for a port that does not exist is dropped
    assert_eq!(
        sai.handle_event(BackendEvent::Link {
            hw: HwPort(99),
            up: true
        }),
        []
    );

    // after the switch is gone the chip's sink leads nowhere and events are not applied
    sai.remove(SWITCH, switch).unwrap();
    assert!(events.try_recv().is_err());
    assert_eq!(
        sai.handle_event(BackendEvent::Link {
            hw: HwPort(2),
            up: true
        }),
        []
    );
}

#[test]
fn vlans_and_members() {
    let (mut sai, switch) = switch(4);
    let bps = bridge_ports(&sai, switch);
    assert_eq!(bps.len(), 4);

    // VLAN 1 exists with every port, as the chip has it
    let Value::Oid(default_vlan) = get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID).unwrap() else {
        panic!()
    };
    assert_eq!(
        get1(&sai, VLAN, default_vlan, sys::SAI_VLAN_ATTR_VLAN_ID),
        Ok(Value::U16(1))
    );
    let default_members = oid_list(get1(&sai, VLAN, default_vlan, sys::SAI_VLAN_ATTR_MEMBER_LIST).unwrap());
    assert_eq!(default_members.len(), 4);
    assert_eq!(sai.remove(VLAN, default_vlan), Err(Error::ObjectInUse)); // the switch and its members refer to it

    for bad in [0, 4095] {
        assert_eq!(
            vlan(&mut sai, switch, bad),
            Err(Error::attr(AttrError::InvalidValue, 0))
        );
    }
    assert_eq!(vlan(&mut sai, switch, 1), Err(Error::ItemAlreadyExists));
    let v100 = vlan(&mut sai, switch, 100).unwrap();
    assert_eq!(vlan(&mut sai, switch, 100), Err(Error::ItemAlreadyExists));
    assert_eq!(
        vlan(&mut sai, RawOid(switch.0 | 1 << 48), 101),
        Err(Error::InvalidObjectId)
    ); // not this switch
    assert_eq!(vlan(&mut sai, v100, 101), Err(Error::InvalidObjectId)); // not a switch at all

    let m0 = member(&mut sai, switch, v100, bps[0], sys::SAI_VLAN_TAGGING_MODE_TAGGED).unwrap();
    let m1 = member(&mut sai, switch, v100, bps[1], sys::SAI_VLAN_TAGGING_MODE_UNTAGGED).unwrap();
    assert_eq!(
        member(&mut sai, switch, v100, bps[1], sys::SAI_VLAN_TAGGING_MODE_TAGGED),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(
        member(
            &mut sai,
            switch,
            v100,
            bps[2],
            sys::SAI_VLAN_TAGGING_MODE_PRIORITY_TAGGED
        ),
        Err(Error::attr(AttrError::NotSupported, 2))
    );
    // object ids of the wrong type, and of the right type naming nothing
    assert_eq!(
        member(&mut sai, switch, bps[0], bps[2], 0),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    assert_eq!(
        member(&mut sai, switch, v100, v100, 0),
        Err(Error::attr(AttrError::InvalidValue, 1))
    );
    // (a VLAN that names nothing is INVALID_PARAMETER, as in the C++)
    assert_eq!(
        member(
            &mut sai,
            switch,
            VlanId::try_from(RawOid(v100.0 + 50)).unwrap().raw(),
            bps[2],
            0
        ),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        oid_list(get1(&sai, VLAN, v100, sys::SAI_VLAN_ATTR_MEMBER_LIST).unwrap()),
        [m0, m1]
    );
    assert_eq!(
        sai.backend().vlans()[&Vid::new(100).unwrap()]
            .iter()
            .map(|(hw, untagged)| (hw.0, *untagged))
            .collect::<Vec<_>>(),
        [(0, false), (1, false), (2, true)] // the CPU port, tagged, as the SDK makes a VLAN
    );
    model_and_chip_agree(&sai).unwrap();

    assert_eq!(
        sai.get(
            VLAN_MEMBER,
            m0,
            &[
                sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID,
                sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID,
                sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE
            ]
        ),
        Ok(vec![
            Value::Oid(v100),
            Value::Oid(bps[0]),
            Value::Enum(sys::SAI_VLAN_TAGGING_MODE_TAGGED.cast_signed())
        ])
    );
    sai.set(
        VLAN_MEMBER,
        m0,
        &Attr::new(
            sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
            Value::Enum(sys::SAI_VLAN_TAGGING_MODE_UNTAGGED.cast_signed()),
        ),
    )
    .unwrap();
    assert_eq!(
        sai.set(
            VLAN_MEMBER,
            m0,
            &Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(default_vlan))
        ),
        Err(Error::attr(AttrError::InvalidValue, 0)) // C++: all but the tagging mode
    );
    model_and_chip_agree(&sai).unwrap();

    // lifecycle: referenced objects stay; a wrong type is no such object, as a stale id (C++)
    assert_eq!(sai.remove(VLAN, v100), Err(Error::ObjectInUse));
    assert_eq!(sai.remove(BRIDGE_PORT, bps[0]), Err(Error::ObjectInUse));
    assert_eq!(sai.remove(VLAN, m0), Err(Error::InvalidObjectId));
    assert_eq!(sai.remove(VLAN_MEMBER, v100), Err(Error::InvalidObjectId));
    assert_eq!(sai.remove(VLAN, RawOid::NULL), Err(Error::InvalidObjectId));
    sai.remove(VLAN_MEMBER, m0).unwrap();
    assert_eq!(sai.remove(VLAN_MEMBER, m0), Err(Error::InvalidObjectId));
    assert_eq!(
        sai.get(VLAN_MEMBER, m0, &[sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID]),
        Err(Error::InvalidObjectId)
    );
    sai.remove(VLAN_MEMBER, m1).unwrap();
    sai.remove(VLAN, v100).unwrap();
    assert_eq!(
        get1(&sai, VLAN, v100, sys::SAI_VLAN_ATTR_VLAN_ID),
        Err(Error::InvalidObjectId)
    );
    // the same VID again: a VLAN's id is its VID (the C++ layout), so the same id
    let again = vlan(&mut sai, switch, 100).unwrap();
    assert_eq!(again, v100);

    // what orchagent does first: remove the default members, then the default bridge ports
    for m in default_members {
        sai.remove(VLAN_MEMBER, m).unwrap();
    }
    for bp in &bps {
        sai.remove(BRIDGE_PORT, *bp).unwrap();
    }
    assert_eq!(bridge_ports(&sai, switch), []);
    // only the CPU port is left in VLAN 1 (tagged, the SDK's)
    assert_eq!(
        sai.backend().vlans()[&Vid::DEFAULT].iter().collect::<Vec<_>>(),
        [(&HwPort(0), &false)]
    );
    model_and_chip_agree(&sai).unwrap();

    // and bridge ports are created again as ports join VLANs
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let bp_attrs = |port: RawOid| {
        vec![
            Attr::new(
                sys::SAI_BRIDGE_PORT_ATTR_TYPE,
                Value::Enum(sys::SAI_BRIDGE_PORT_TYPE_PORT.cast_signed()),
            ),
            Attr::new(sys::SAI_BRIDGE_PORT_ATTR_PORT_ID, Value::Oid(port)),
            Attr::new(sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
        ]
    };
    let bp = sai.create(BRIDGE_PORT, switch, &bp_attrs(ports[0])).unwrap();
    assert!(!bps.contains(&bp));
    assert_eq!(
        sai.create(BRIDGE_PORT, switch, &bp_attrs(ports[0])),
        Err(Error::ItemAlreadyExists)
    );
    let Value::Oid(cpu) = get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap() else {
        panic!()
    };
    // (the CPU port cannot carry one: INVALID_PARAMETER, as in the C++)
    assert_eq!(
        sai.create(BRIDGE_PORT, switch, &bp_attrs(cpu)),
        Err(Error::InvalidParameter)
    );
    let sub_port = [Attr::new(
        sys::SAI_BRIDGE_PORT_ATTR_TYPE,
        Value::Enum(sys::SAI_BRIDGE_PORT_TYPE_SUB_PORT.cast_signed()),
    )];
    assert_eq!(
        sai.create(BRIDGE_PORT, switch, &sub_port),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    assert_eq!(
        get1(&sai, BRIDGE_PORT, bp, sys::SAI_BRIDGE_PORT_ATTR_PORT_ID),
        Ok(Value::Oid(ports[0]))
    );
    member(&mut sai, switch, again, bp, sys::SAI_VLAN_TAGGING_MODE_TAGGED).unwrap();
    model_and_chip_agree(&sai).unwrap();

    // object types SAI has and this slice does not
    assert_eq!(
        sai.create(sys::SAI_OBJECT_TYPE_TUNNEL, switch, &[]),
        Err(Error::NotImplemented)
    );
    assert_eq!(sai.remove(sys::SAI_OBJECT_TYPE_TUNNEL, bp), Err(Error::NotImplemented));
}

#[test]
fn a_failing_backend_leaves_model_and_chip_as_they_were() {
    let (mut sai, switch) = switch(4);
    let bps = bridge_ports(&sai, switch);
    let ports = oid_list(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let objects = |sai: &Sai<VirtualBackend>| sai.store().unwrap().len();
    let before = objects(&sai);

    sai.backend_mut().fail_next(Op::VlanCreate, Error::TableFull);
    assert_eq!(vlan(&mut sai, switch, 7), Err(Error::TableFull));
    assert_eq!(objects(&sai), before);
    let v7 = vlan(&mut sai, switch, 7).unwrap();

    sai.backend_mut().fail_next(Op::VlanPortAdd, Error::Failure);
    assert_eq!(member(&mut sai, switch, v7, bps[0], 0), Err(Error::Failure));
    assert_eq!(objects(&sai), before + 1);
    let m = member(&mut sai, switch, v7, bps[0], sys::SAI_VLAN_TAGGING_MODE_UNTAGGED).unwrap();

    sai.backend_mut().fail_next(Op::VlanPortAdd, Error::Failure);
    let tagged = Attr::new(
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
        Value::Enum(sys::SAI_VLAN_TAGGING_MODE_TAGGED.cast_signed()),
    );
    assert_eq!(sai.set(VLAN_MEMBER, m, &tagged), Err(Error::Failure));
    assert_eq!(
        get1(&sai, VLAN_MEMBER, m, sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE),
        Ok(Value::Enum(sys::SAI_VLAN_TAGGING_MODE_UNTAGGED.cast_signed()))
    );

    sai.backend_mut().fail_next(Op::VlanPortRemove, Error::Failure);
    assert_eq!(sai.remove(VLAN_MEMBER, m), Err(Error::Failure));
    assert_eq!(
        get1(&sai, VLAN_MEMBER, m, sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID),
        Ok(Value::Oid(v7))
    );
    sai.remove(VLAN_MEMBER, m).unwrap();

    sai.backend_mut().fail_next(Op::VlanDestroy, Error::Failure);
    assert_eq!(sai.remove(VLAN, v7), Err(Error::Failure));
    assert_eq!(get1(&sai, VLAN, v7, sys::SAI_VLAN_ATTR_VLAN_ID), Ok(Value::U16(7)));

    sai.backend_mut().fail_next(Op::PortAdminSet, Error::Failure);
    assert_eq!(
        sai.set(
            PORT,
            ports[0],
            &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true))
        ),
        Err(Error::Failure)
    );
    assert_eq!(
        get1(&sai, PORT, ports[0], sys::SAI_PORT_ATTR_ADMIN_STATE),
        Ok(Value::Bool(false))
    );
    sai.backend_mut().fail_next(Op::PortSpeedSet, Error::Failure);
    assert_eq!(
        sai.set(PORT, ports[0], &Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(40_000))),
        Err(Error::Failure)
    );
    model_and_chip_agree(&sai).unwrap();

    // switch creation: a chip that does not come up, or fails while the objects are built
    // (the first port cannot be disabled), leaves no switch and a stopped chip
    sai.remove(SWITCH, switch).unwrap();
    sai.backend_mut().fail_next(Op::Init, Error::Failure);
    assert_eq!(sai.create_switch(&init_attrs(), &profile(4)), Err(Error::Failure));
    assert!(sai.store().is_none());
    // a port that cannot be disabled is logged and the switch is created; the model says admin
    // down although the chip port stays as the SDK left it (C++ switch_build_objects)
    sai.backend_mut().fail_next(Op::PortAdminSet, Error::Failure);
    sai.create_switch(&init_attrs(), &profile(4)).unwrap();
    assert_eq!(sai.backend().port_admin(HwPort(1)), Some(true));
    assert!(model_and_chip_agree(&sai).is_err());
}
