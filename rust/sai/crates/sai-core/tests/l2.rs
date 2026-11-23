//! The L2 area on the virtual backend: VLANs, VLAN members, the bridge, bridge ports (on ports and
//! LAGs), FDB entries, flush, FDB events, STP. The checks of the C++ suite's `test_vlan_bridge` and
//! `test_fdb` (`sai-openbcm/tests/test_sai.cpp`), failure injection (a failed backend call leaves model
//! and chip as they were, where the C++ undoes; where it does not, the chip keeps the calls that
//! worked and the model is unchanged), and the backend call log against the C++ oracle's
//! (`tests/driver-scripts/03-vlan-fdb.ops` on `frontend_driver`).
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
use sai_core::backend::types::{Dest, FdbChange, FdbEvent, Learn, MacAddr};
use sai_core::backend::{Backend, HwPort, Mode, Profile, Recording, Vid};
use sai_core::error::{AttrError, Error};
use sai_core::l2::FdbEventKind;
use sai_core::model::EntryKey;
use sai_core::oid::{BridgePortId, RawOid, TypedId, VlanId};
use sai_core::sai::{Notification, Sai};
use sai_core::value::{Attr, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;

const FDB_TYPE: sys::sai_attr_id_t = sys::SAI_FDB_ENTRY_ATTR_TYPE;
const FDB_BP: sys::sai_attr_id_t = sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
const FDB_ACTION: sys::sai_attr_id_t = sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION;

fn sw_oid<B: Backend>(sai: &Sai<B>, switch: RawOid, attr: sys::sai_attr_id_t) -> RawOid {
    oid(sai.get(SWITCH, switch, &[attr]).unwrap().remove(0))
}

fn ports<B: Backend>(sai: &Sai<B>, switch: RawOid) -> Vec<RawOid> {
    oid_list(
        sai.get(SWITCH, switch, &[sys::SAI_SWITCH_ATTR_PORT_LIST])
            .unwrap()
            .remove(0),
    )
}

/// The bridge port on a port or LAG (the C++ suite's `bport_of`).
fn bport_of<B: Backend>(sai: &Sai<B>, switch: RawOid, on: RawOid) -> Option<RawOid> {
    let bridge = sw_oid(sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    oid_list(
        sai.get(BRIDGE, bridge, &[sys::SAI_BRIDGE_ATTR_PORT_LIST])
            .unwrap()
            .remove(0),
    )
    .into_iter()
    .find(|bp| sai.get(BRIDGE_PORT, *bp, &[sys::SAI_BRIDGE_PORT_ATTR_PORT_ID]).unwrap()[0] == Value::Oid(on))
}

fn members_of<B: Backend>(sai: &Sai<B>, vlan: RawOid) -> Vec<RawOid> {
    oid_list(
        sai.get(VLAN, vlan, &[sys::SAI_VLAN_ATTR_MEMBER_LIST])
            .unwrap()
            .remove(0),
    )
}

/// The VLAN member of a bridge port in a VLAN.
fn member_of<B: Backend>(sai: &Sai<B>, vlan: RawOid, bridge_port: RawOid) -> Option<RawOid> {
    members_of(sai, vlan).into_iter().find(|m| {
        sai.get(VLAN_MEMBER, *m, &[sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID])
            .unwrap()[0]
            == Value::Oid(bridge_port)
    })
}

fn vlan_attrs(vid: u16) -> Vec<Attr> {
    vec![Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(vid))]
}

fn member_attrs(vlan: RawOid, bridge_port: RawOid, mode: u32) -> Vec<Attr> {
    vec![
        Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(vlan)),
        Attr::new(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(bridge_port)),
        Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, enumerator(mode)),
    ]
}

fn bport_attrs(on: RawOid, learning: Option<u32>) -> Vec<Attr> {
    let mut attrs = vec![
        Attr::new(
            sys::SAI_BRIDGE_PORT_ATTR_TYPE,
            enumerator(sys::SAI_BRIDGE_PORT_TYPE_PORT),
        ),
        Attr::new(sys::SAI_BRIDGE_PORT_ATTR_PORT_ID, Value::Oid(on)),
        Attr::new(sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
    ];
    if let Some(mode) = learning {
        attrs.push(Attr::new(sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE, enumerator(mode)));
    }
    attrs
}

fn fdb_key(vlan: RawOid, last: u8) -> EntryKey {
    EntryKey::Fdb {
        bv_id: vlan,
        mac: [0x00, 0x11, 0x22, 0x33, 0x44, last],
    }
}

fn fdb_attrs(entry_type: u32, bridge_port: RawOid) -> Vec<Attr> {
    vec![
        Attr::new(FDB_TYPE, enumerator(entry_type)),
        Attr::new(FDB_BP, Value::Oid(bridge_port)),
        Attr::new(FDB_ACTION, enumerator(sys::SAI_PACKET_ACTION_FORWARD)),
    ]
}

fn chip_member(sai: &Sai<VirtualBackend>, vid: u16, hw: u16) -> Option<bool> {
    sai.backend()
        .vlans()
        .get(&Vid::new(vid).unwrap())
        .and_then(|m| m.get(&HwPort(hw)).copied())
}

fn learn_of(sai: &Sai<VirtualBackend>, hw: u16) -> Learn {
    sai.backend().port_state(HwPort(hw)).unwrap().learn
}

fn event(change: FdbChange, mac: [u8; 6], vid: u16, dest: Dest) -> FdbEvent {
    FdbEvent {
        change,
        mac: MacAddr(mac),
        vid: Vid::new(vid).unwrap(),
        hw_port: dest,
    }
}

/// Raises the events in the chip and applies them as the notifier thread does; returns the
/// notifications.
fn inject(
    sai: &mut Sai<VirtualBackend>,
    rx: &std::sync::mpsc::Receiver<sai_core::backend::BackendEvent>,
    events: &[FdbEvent],
) -> Vec<Notification> {
    for e in events {
        sai.backend_mut().inject_fdb(*e);
    }
    let mut out = Vec::new();
    while let Ok(e) = rx.try_recv() {
        out.extend(sai.handle_event(e));
    }
    out
}

fn fdb_notes(notes: &[Notification]) -> Vec<(FdbEventKind, Option<RawOid>)> {
    notes
        .iter()
        .filter_map(|n| match n {
            Notification::Fdb { kind, bridge_port, .. } => Some((*kind, bridge_port.map(TypedId::raw))),
            Notification::PortOperStatus { .. } => None,
        })
        .collect()
}

#[test]
fn vlans_bridge_ports_and_members_as_the_cpp_suite() {
    let (mut sai, switch) = switch(8);
    let ports = ports(&sai, switch);
    let vlan1 = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    // The C++ layout: a VLAN's index is its VID.
    assert_eq!(vlan1, VlanId::from_index(1).raw());
    // SDK default: every front-panel port in VLAN 1.
    assert_eq!(members_of(&sai, vlan1).len(), 8);

    // remove port0 from VLAN 1 (what orchagent does at init)
    let bp0 = bport_of(&sai, switch, ports[0]).unwrap();
    let m0 = member_of(&sai, vlan1, bp0).unwrap();
    assert_eq!(
        sai.get(
            VLAN_MEMBER,
            m0,
            &[
                sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID,
                sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE
            ]
        ),
        Ok(vec![Value::Oid(vlan1), enumerator(sys::SAI_VLAN_TAGGING_MODE_UNTAGGED)])
    );
    assert_eq!(sai.remove(BRIDGE_PORT, bp0), Err(Error::ObjectInUse)); // still a VLAN member
    sai.remove(VLAN_MEMBER, m0).unwrap();
    assert_eq!(chip_member(&sai, 1, 1), None);
    model_and_chip_agree(&sai).unwrap();

    // bridge port re-created with learning disabled
    sai.remove(BRIDGE_PORT, bp0).unwrap();
    let bpa = bport_attrs(ports[0], Some(sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE));
    let bp0 = sai.create(BRIDGE_PORT, switch, &bpa).unwrap();
    assert_eq!(learn_of(&sai, 1), Learn::Disable);
    assert_eq!(sai.create(BRIDGE_PORT, switch, &bpa), Err(Error::ItemAlreadyExists));
    let mut sub = bpa.clone();
    sub[0].value = enumerator(sys::SAI_BRIDGE_PORT_TYPE_SUB_PORT);
    assert_eq!(
        sai.create(BRIDGE_PORT, switch, &sub),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    // the CPU port cannot carry one; the type is mandatory; the port too (a condition)
    let cpu = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_CPU_PORT);
    assert_eq!(
        sai.create(BRIDGE_PORT, switch, &bport_attrs(cpu, None)),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        sai.create(BRIDGE_PORT, switch, &bpa[1..]),
        Err(Error::MandatoryAttributeMissing)
    );
    assert_eq!(
        sai.create(BRIDGE_PORT, switch, &bpa[..1]),
        Err(Error::MandatoryAttributeMissing)
    );
    assert_eq!(
        sai.get(
            BRIDGE_PORT,
            bp0,
            &[
                sys::SAI_BRIDGE_PORT_ATTR_TYPE,
                sys::SAI_BRIDGE_PORT_ATTR_PORT_ID,
                sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE,
                sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE,
                // a stub: the C++ default
                sys::SAI_BRIDGE_PORT_ATTR_TAGGING_MODE,
            ]
        ),
        Ok(vec![
            enumerator(sys::SAI_BRIDGE_PORT_TYPE_PORT),
            Value::Oid(ports[0]),
            Value::Bool(true),
            enumerator(sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE),
            enumerator(sys::SAI_BRIDGE_PORT_TAGGING_MODE_TAGGED),
        ])
    );
    // learning: every mode maps to the chip's (C++ to_learn)
    for (mode, learn) in [
        (sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_CPU_LOG, Learn::Cpu),
        (sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DROP, Learn::Drop),
        (sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_FDB_NOTIFICATION, Learn::Hw),
        (sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE, Learn::Disable),
    ] {
        sai.set(
            BRIDGE_PORT,
            bp0,
            &Attr::new(sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE, enumerator(mode)),
        )
        .unwrap();
        assert_eq!(learn_of(&sai, 1), learn);
    }

    // VLAN 100
    let v100 = sai.create(VLAN, switch, &vlan_attrs(100)).unwrap();
    assert_eq!(v100, VlanId::from_index(100).raw());
    assert!(sai.backend().vlans().contains_key(&Vid::new(100).unwrap()));
    assert_eq!(
        sai.create(VLAN, switch, &vlan_attrs(100)),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(
        sai.create(VLAN, switch, &vlan_attrs(4095)),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    assert_eq!(sai.create(VLAN, switch, &[]), Err(Error::MandatoryAttributeMissing));
    // the default STP instance only; anything not a stub is not supported
    let stp = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID);
    let mut with_stp = vlan_attrs(101);
    with_stp.push(Attr::new(sys::SAI_VLAN_ATTR_STP_INSTANCE, Value::Oid(switch)));
    assert!(sai.create(VLAN, switch, &with_stp).is_err());
    with_stp[1].value = Value::Oid(stp);
    let v101 = sai.create(VLAN, switch, &with_stp).unwrap();
    sai.remove(VLAN, v101).unwrap();

    let ma = member_attrs(v100, bp0, sys::SAI_VLAN_TAGGING_MODE_TAGGED);
    let m100 = sai.create(VLAN_MEMBER, switch, &ma).unwrap();
    assert_eq!(chip_member(&sai, 100, 1), Some(false));
    assert_eq!(sai.create(VLAN_MEMBER, switch, &ma), Err(Error::ItemAlreadyExists));
    // priority tagging is not supported; a VLAN that is gone is the caller's mistake
    let mut prio = ma.clone();
    prio[2].value = enumerator(sys::SAI_VLAN_TAGGING_MODE_PRIORITY_TAGGED);
    assert_eq!(
        sai.create(VLAN_MEMBER, switch, &prio),
        Err(Error::attr(AttrError::NotSupported, 2))
    );
    let mut gone = ma.clone();
    gone[0].value = Value::Oid(VlanId::from_index(999).raw());
    assert_eq!(sai.create(VLAN_MEMBER, switch, &gone), Err(Error::InvalidParameter));
    assert_eq!(
        sai.create(VLAN_MEMBER, switch, &ma[..1]),
        Err(Error::MandatoryAttributeMissing)
    );
    let untagged = Attr::new(
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
        enumerator(sys::SAI_VLAN_TAGGING_MODE_UNTAGGED),
    );
    sai.set(VLAN_MEMBER, m100, &untagged).unwrap();
    assert_eq!(chip_member(&sai, 100, 1), Some(true));
    let prio_set = Attr::new(
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
        enumerator(sys::SAI_VLAN_TAGGING_MODE_PRIORITY_TAGGED),
    );
    assert_eq!(
        sai.set(VLAN_MEMBER, m100, &prio_set),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    model_and_chip_agree(&sai).unwrap();

    assert_eq!(sai.remove(VLAN, v100), Err(Error::ObjectInUse));
    assert_eq!(sai.remove(VLAN, vlan1), Err(Error::ObjectInUse));
    sai.remove(VLAN_MEMBER, m100).unwrap();
    sai.remove(VLAN, v100).unwrap();
    assert!(!sai.backend().vlans().contains_key(&Vid::new(100).unwrap()));
    assert_eq!(sai.remove(VLAN, v100), Err(Error::InvalidObjectId));

    // STP default instance lists the VLANs
    assert_eq!(
        sai.get(
            sys::SAI_OBJECT_TYPE_STP,
            stp,
            &[
                sys::SAI_STP_ATTR_VLAN_LIST,
                sys::SAI_STP_ATTR_BRIDGE_ID,
                sys::SAI_STP_ATTR_PORT_LIST
            ]
        ),
        Ok(vec![
            Value::VlanList(vec![1]),
            Value::Oid(sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID)),
            Value::OidList(vec![]),
        ])
    );
    assert_eq!(sai.remove(sys::SAI_OBJECT_TYPE_STP, stp), Err(Error::NotSupported));
    assert_eq!(
        sai.create(sys::SAI_OBJECT_TYPE_STP, switch, &[]),
        Err(Error::NotSupported)
    );
    model_and_chip_agree(&sai).unwrap();
}

#[test]
fn the_bridge_and_kept_attributes() {
    let (mut sai, switch) = switch(2);
    let bridge = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    assert_eq!(sai.create(BRIDGE, switch, &[]), Err(Error::NotSupported));
    assert_eq!(sai.remove(BRIDGE, bridge), Err(Error::ObjectInUse));
    assert_eq!(
        sai.get(
            BRIDGE,
            bridge,
            &[
                sys::SAI_BRIDGE_ATTR_TYPE,
                sys::SAI_BRIDGE_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE
            ]
        ),
        Ok(vec![
            enumerator(sys::SAI_BRIDGE_TYPE_1Q),
            enumerator(sys::SAI_BRIDGE_FLOOD_CONTROL_TYPE_SUB_PORTS)
        ])
    );
    let learn_disable = Attr::new(sys::SAI_BRIDGE_ATTR_LEARN_DISABLE, Value::Bool(true));
    sai.set(BRIDGE, bridge, &learn_disable).unwrap();
    assert_eq!(
        sai.get(BRIDGE, bridge, &[sys::SAI_BRIDGE_ATTR_LEARN_DISABLE]),
        Ok(vec![Value::Bool(true)])
    );

    // VLAN stubs: the C++ defaults, kept on set
    let vlan1 = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    assert_eq!(
        sai.get(
            VLAN,
            vlan1,
            &[
                sys::SAI_VLAN_ATTR_LEARN_DISABLE,
                sys::SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE,
                sys::SAI_VLAN_ATTR_IPV4_MCAST_LOOKUP_KEY_TYPE,
                sys::SAI_VLAN_ATTR_META_DATA,
                sys::SAI_VLAN_ATTR_STP_INSTANCE,
            ]
        ),
        Ok(vec![
            Value::Bool(false),
            enumerator(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL),
            enumerator(sys::SAI_VLAN_MCAST_LOOKUP_KEY_TYPE_MAC_DA),
            Value::U32(0),
            Value::Oid(sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID)),
        ])
    );
    sai.set(VLAN, vlan1, &Attr::new(sys::SAI_VLAN_ATTR_META_DATA, Value::U32(7)))
        .unwrap();
    assert_eq!(
        sai.get(VLAN, vlan1, &[sys::SAI_VLAN_ATTR_META_DATA]),
        Ok(vec![Value::U32(7)])
    );
    // an attribute the C++ neither programs nor keeps
    assert_eq!(
        sai.get(VLAN, vlan1, &[sys::SAI_VLAN_ATTR_INGRESS_ACL]),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    model_and_chip_agree(&sai).unwrap();
}

#[test]
fn fdb_entries_and_events_as_the_cpp_suite() {
    let (mut sai, switch) = switch(8);
    let rx = sai.take_events().unwrap();
    let ports = ports(&sai, switch);
    let vlan1 = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    let bp1 = bport_of(&sai, switch, ports[1]).unwrap();
    let e = fdb_key(vlan1, 0x55);
    let st = fdb_attrs(sys::SAI_FDB_ENTRY_TYPE_STATIC, bp1);
    sai.create_entry(switch, e, &st).unwrap();
    assert_eq!(sai.create_entry(switch, e, &st), Err(Error::ItemAlreadyExists));
    let chip = sai.backend().l2_table()[&(MacAddr([0, 0x11, 0x22, 0x33, 0x44, 0x55]), Vid::DEFAULT)];
    assert_eq!(chip.dest, Dest::Port(HwPort(2)));
    assert!(chip.is_static && !chip.discard);
    assert_eq!(
        sai.get_entry(
            switch,
            &e,
            &[FDB_BP, FDB_TYPE, FDB_ACTION, sys::SAI_FDB_ENTRY_ATTR_META_DATA]
        ),
        Ok(vec![
            Value::Oid(bp1),
            enumerator(sys::SAI_FDB_ENTRY_TYPE_STATIC),
            enumerator(sys::SAI_PACKET_ACTION_FORWARD),
            Value::U32(0),
        ])
    );

    let d = fdb_key(vlan1, 0x66);
    sai.create_entry(switch, d, &fdb_attrs(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC, bp1))
        .unwrap();
    // an existing dynamic entry is replaced, but the type stays mandatory
    assert_eq!(sai.create_entry(switch, d, &[]), Err(Error::MandatoryAttributeMissing));

    // flush dynamic on the port: static stays
    let flush = [
        Attr::new(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, Value::Oid(bp1)),
        Attr::new(
            sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
            enumerator(sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC),
        ),
    ];
    assert_eq!(sai.flush_fdb(switch, &flush), Ok(vec![d]));
    assert_eq!(sai.get_entry(switch, &d, &[FDB_TYPE]), Err(Error::ItemNotFound));
    sai.get_entry(switch, &e, &[FDB_TYPE]).unwrap();
    assert_eq!(sai.backend().l2_table().len(), 1);
    model_and_chip_agree(&sai).unwrap();
    sai.remove_entry(switch, &e).unwrap();
    assert_eq!(sai.backend().l2_table().len(), 0);
    assert_eq!(sai.remove_entry(switch, &e), Err(Error::ItemNotFound));
    // .1D bridge keys are rejected, unknown VLANs too
    let bridge = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    assert_eq!(
        sai.create_entry(switch, fdb_key(bridge, 1), &st),
        Err(Error::NotSupported)
    );
    let nowhere = fdb_key(VlanId::from_index(999).raw(), 1);
    assert_eq!(sai.create_entry(switch, nowhere, &st), Err(Error::InvalidParameter));
    // a forward entry needs a bridge port; a discard one goes to the CPU port
    let mut no_port = st.clone();
    no_port[1].value = Value::Oid(RawOid::NULL);
    assert_eq!(
        sai.create_entry(switch, fdb_key(vlan1, 2), &no_port),
        Err(Error::InvalidParameter)
    );
    no_port[2].value = enumerator(sys::SAI_PACKET_ACTION_DROP);
    sai.create_entry(switch, fdb_key(vlan1, 2), &no_port).unwrap();
    let chip = sai.backend().l2_table()[&(MacAddr([0, 0x11, 0x22, 0x33, 0x44, 2]), Vid::DEFAULT)];
    assert!(chip.discard && chip.dest == Dest::Port(HwPort(0)));
    // trap is no FDB action here
    no_port[2].value = enumerator(sys::SAI_PACKET_ACTION_TRAP);
    assert_eq!(
        sai.create_entry(switch, fdb_key(vlan1, 3), &no_port),
        Err(Error::attr(AttrError::NotSupported, 2))
    );
    sai.remove_entry(switch, &fdb_key(vlan1, 2)).unwrap();

    // hardware learning -> FDB_EVENT notification with the bridge port
    let mac = [0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0x01];
    let notes = inject(
        &mut sai,
        &rx,
        &[event(FdbChange::Learned, mac, 1, Dest::Port(HwPort(3)))],
    );
    let bp2 = bport_of(&sai, switch, ports[2]).unwrap();
    assert_eq!(fdb_notes(&notes), [(FdbEventKind::Learned, Some(bp2))]);
    let Notification::Fdb {
        switch: of,
        mac: seen,
        vlan,
        ..
    } = notes[0]
    else {
        panic!()
    };
    assert_eq!((of.raw(), seen, vlan.raw()), (switch, mac, vlan1));
    let learned = EntryKey::Fdb { bv_id: vlan1, mac };
    assert_eq!(
        sai.get_entry(switch, &learned, &[FDB_TYPE, FDB_BP]),
        Ok(vec![enumerator(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC), Value::Oid(bp2)])
    );
    model_and_chip_agree(&sai).unwrap();
    let notes = inject(&mut sai, &rx, &[event(FdbChange::Aged, mac, 1, Dest::Port(HwPort(3)))]);
    assert_eq!(fdb_notes(&notes), [(FdbEventKind::Aged, Some(bp2))]);
    assert_eq!(sai.get_entry(switch, &learned, &[FDB_TYPE]), Err(Error::ItemNotFound));

    // An entry created through SAI and then reported by the SDK's learn callback is not
    // announced again; a move to another port is.
    let own = fdb_key(vlan1, 0x77);
    sai.create_entry(switch, own, &fdb_attrs(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC, bp1))
        .unwrap();
    let EntryKey::Fdb { mac: own_mac, .. } = own else {
        panic!()
    };
    let notes = inject(
        &mut sai,
        &rx,
        &[
            event(FdbChange::Learned, own_mac, 1, Dest::Port(HwPort(2))),
            event(FdbChange::Learned, own_mac, 1, Dest::Port(HwPort(3))),
        ],
    );
    assert_eq!(fdb_notes(&notes), [(FdbEventKind::Move, Some(bp2))]);
    sai.remove_entry(switch, &own).unwrap();

    // Not reported: a VLAN that is not a SAI VLAN, a port without a bridge port, a static
    // entry, an age of something unknown.
    let bp0 = bport_of(&sai, switch, ports[0]).unwrap();
    sai.remove(VLAN_MEMBER, member_of(&sai, vlan1, bp0).unwrap()).unwrap();
    sai.remove(BRIDGE_PORT, bp0).unwrap();
    let fixed = fdb_key(vlan1, 0x88);
    sai.create_entry(switch, fixed, &st).unwrap();
    let EntryKey::Fdb { mac: fixed_mac, .. } = fixed else {
        panic!()
    };
    let notes = inject(
        &mut sai,
        &rx,
        &[
            event(FdbChange::Learned, mac, 77, Dest::Port(HwPort(3))),
            event(FdbChange::Learned, mac, 1, Dest::Port(HwPort(1))),
            event(FdbChange::Learned, fixed_mac, 1, Dest::Port(HwPort(3))),
            event(FdbChange::Aged, fixed_mac, 1, Dest::Port(HwPort(2))),
            event(FdbChange::Aged, [2; 6], 1, Dest::Port(HwPort(2))),
        ],
    );
    assert_eq!(fdb_notes(&notes), []);
    assert_eq!(
        sai.get_entry(switch, &fixed, &[FDB_TYPE]),
        Ok(vec![enumerator(sys::SAI_FDB_ENTRY_TYPE_STATIC)])
    );
    // set: the entry follows (static -> dynamic, another bridge port)
    sai.set_entry(switch, &fixed, &Attr::new(FDB_BP, Value::Oid(bp2)))
        .unwrap();
    assert_eq!(
        sai.backend().l2_table()[&(MacAddr(fixed_mac), Vid::DEFAULT)].dest,
        Dest::Port(HwPort(3))
    );
    assert_eq!(
        sai.set_entry(
            switch,
            &fixed,
            &Attr::new(FDB_BP, Value::Oid(BridgePortId::from_index(99).raw()))
        ),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    assert_eq!(
        sai.set_entry(switch, &fdb_key(vlan1, 0x99), &Attr::new(FDB_BP, Value::Oid(bp2))),
        Err(Error::ItemNotFound)
    );
    sai.set_entry(
        switch,
        &fixed,
        &Attr::new(sys::SAI_FDB_ENTRY_ATTR_META_DATA, Value::U32(5)),
    )
    .unwrap();
    assert_eq!(
        sai.get_entry(switch, &fixed, &[sys::SAI_FDB_ENTRY_ATTR_META_DATA, FDB_BP]),
        Ok(vec![Value::U32(5), Value::Oid(bp2)])
    );
    // a bridge port with a static entry is in use; with only dynamic ones it is flushed
    assert_eq!(sai.remove(BRIDGE_PORT, bp2), Err(Error::ObjectInUse));
    model_and_chip_agree(&sai).unwrap();
}

/// Every flush filter combination: bridge port (none, one), VLAN (none, one), type (default,
/// static, dynamic, all), on the same population each time; the model drops exactly the
/// matching entries and the chip agrees.
#[test]
fn flush_with_every_filter_combination() {
    let types = [
        None,
        Some(sys::SAI_FDB_FLUSH_ENTRY_TYPE_STATIC),
        Some(sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC),
        Some(sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL),
    ];
    for by_port in [false, true] {
        for by_vlan in [false, true] {
            for kind in types {
                let (mut sai, switch) = switch(4);
                let ports = ports(&sai, switch);
                let vlan1 = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
                let v20 = sai.create(VLAN, switch, &vlan_attrs(20)).unwrap();
                let bps: Vec<RawOid> = ports[..2].iter().map(|p| bport_of(&sai, switch, *p).unwrap()).collect();
                let mut all = Vec::new();
                let mut n = 0;
                for vlan in [vlan1, v20] {
                    for bp in &bps {
                        for t in [sys::SAI_FDB_ENTRY_TYPE_STATIC, sys::SAI_FDB_ENTRY_TYPE_DYNAMIC] {
                            n += 1;
                            let key = fdb_key(vlan, n);
                            sai.create_entry(switch, key, &fdb_attrs(t, *bp)).unwrap();
                            all.push((key, vlan, *bp, t));
                        }
                    }
                }
                let mut attrs = Vec::new();
                if by_port {
                    attrs.push(Attr::new(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, Value::Oid(bps[0])));
                }
                if by_vlan {
                    attrs.push(Attr::new(sys::SAI_FDB_FLUSH_ATTR_BV_ID, Value::Oid(v20)));
                }
                if let Some(kind) = kind {
                    attrs.push(Attr::new(sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE, enumerator(kind)));
                }
                let wanted_type = |t: u32| match kind {
                    None | Some(sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC) => t == sys::SAI_FDB_ENTRY_TYPE_DYNAMIC,
                    Some(sys::SAI_FDB_FLUSH_ENTRY_TYPE_STATIC) => t == sys::SAI_FDB_ENTRY_TYPE_STATIC,
                    _ => true,
                };
                let mut expected: Vec<EntryKey> = all
                    .iter()
                    .filter(|(_, vlan, bp, t)| {
                        wanted_type(*t) && (!by_port || *bp == bps[0]) && (!by_vlan || *vlan == v20)
                    })
                    .map(|(key, ..)| *key)
                    .collect();
                let mut removed = sai.flush_fdb(switch, &attrs).unwrap();
                removed.sort();
                expected.sort();
                assert_eq!(removed, expected, "port {by_port} vlan {by_vlan} type {kind:?}");
                assert_eq!(
                    sai.store().unwrap().entry_count(),
                    all.len() - expected.len(),
                    "port {by_port} vlan {by_vlan} type {kind:?}"
                );
                assert_eq!(sai.backend().l2_table().len(), all.len() - expected.len());
                model_and_chip_agree(&sai).unwrap();
            }
        }
    }
    // bad filters: a bridge port or VLAN that names nothing; an attribute flush does not have
    let (mut sai, switch) = switch(2);
    let bad_bp = [Attr::new(
        sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID,
        Value::Oid(BridgePortId::from_index(99).raw()),
    )];
    assert_eq!(sai.flush_fdb(switch, &bad_bp), Err(Error::InvalidParameter));
    let bad_vlan = [Attr::new(
        sys::SAI_FDB_FLUSH_ATTR_BV_ID,
        Value::Oid(VlanId::from_index(99).raw()),
    )];
    assert_eq!(sai.flush_fdb(switch, &bad_vlan), Err(Error::InvalidParameter));
}

/// Backend first, store second: where the C++ undoes the calls that worked (VLAN member on a
/// LAG, member create), model and chip end as they were; a single failed call changes nothing.
#[test]
fn a_failed_backend_call_leaves_model_and_chip_as_they_were() {
    let (mut sai, switch) = switch(4);
    let ports = ports(&sai, switch);
    let vlan1 = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    let bp0 = bport_of(&sai, switch, ports[0]).unwrap();
    let objects = |sai: &Sai<VirtualBackend>| (sai.store().unwrap().len(), sai.store().unwrap().entry_count());

    // VLAN create / remove
    let before = objects(&sai);
    sai.backend_mut().fail_nth("vlan_create", 1, Error::Failure).unwrap();
    assert_eq!(sai.create(VLAN, switch, &vlan_attrs(30)), Err(Error::Failure));
    assert_eq!(objects(&sai), before);
    model_and_chip_agree(&sai).unwrap();
    let v30 = sai.create(VLAN, switch, &vlan_attrs(30)).unwrap();
    sai.backend_mut().fail_nth("vlan_destroy", 1, Error::Failure).unwrap();
    assert_eq!(sai.remove(VLAN, v30), Err(Error::Failure));
    model_and_chip_agree(&sai).unwrap();

    // VLAN member create and tagging
    let before = objects(&sai);
    sai.backend_mut().fail_nth("vlan_port_add", 1, Error::Failure).unwrap();
    assert_eq!(
        sai.create(
            VLAN_MEMBER,
            switch,
            &member_attrs(v30, bp0, sys::SAI_VLAN_TAGGING_MODE_TAGGED)
        ),
        Err(Error::Failure)
    );
    assert_eq!(objects(&sai), before);
    model_and_chip_agree(&sai).unwrap();
    let m = sai
        .create(
            VLAN_MEMBER,
            switch,
            &member_attrs(v30, bp0, sys::SAI_VLAN_TAGGING_MODE_TAGGED),
        )
        .unwrap();
    sai.backend_mut().fail_nth("vlan_port_add", 1, Error::Failure).unwrap();
    let untagged = Attr::new(
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
        enumerator(sys::SAI_VLAN_TAGGING_MODE_UNTAGGED),
    );
    assert_eq!(sai.set(VLAN_MEMBER, m, &untagged), Err(Error::Failure));
    assert_eq!(
        sai.get(VLAN_MEMBER, m, &[sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE]),
        Ok(vec![enumerator(sys::SAI_VLAN_TAGGING_MODE_TAGGED)])
    );
    model_and_chip_agree(&sai).unwrap();
    sai.backend_mut()
        .fail_nth("vlan_port_remove", 1, Error::Failure)
        .unwrap();
    assert_eq!(sai.remove(VLAN_MEMBER, m), Err(Error::Failure));
    model_and_chip_agree(&sai).unwrap();
    sai.remove(VLAN_MEMBER, m).unwrap();

    // bridge port: learning
    sai.backend_mut().fail_nth("port_learn_set", 1, Error::Failure).unwrap();
    let learn = Attr::new(
        sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE,
        enumerator(sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE),
    );
    assert_eq!(sai.set(BRIDGE_PORT, bp0, &learn), Err(Error::Failure));
    assert_eq!(
        sai.get(BRIDGE_PORT, bp0, &[sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE]),
        Ok(vec![enumerator(sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_HW)])
    );
    assert_eq!(learn_of(&sai, 1), Learn::Hw);
    // an INVALID_PARAMETER from the chip is the attribute's (C++ attr_backend_status)
    sai.backend_mut()
        .fail_nth("port_learn_set", 1, Error::InvalidParameter)
        .unwrap();
    assert_eq!(
        sai.set(BRIDGE_PORT, bp0, &learn),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    let bp3 = bport_of(&sai, switch, ports[3]).unwrap();
    sai.remove(VLAN_MEMBER, member_of(&sai, vlan1, bp3).unwrap()).unwrap();
    sai.remove(BRIDGE_PORT, bp3).unwrap();
    let before = objects(&sai);
    sai.backend_mut().fail_nth("port_learn_set", 1, Error::Failure).unwrap();
    assert_eq!(
        sai.create(BRIDGE_PORT, switch, &bport_attrs(ports[3], None)),
        Err(Error::Failure)
    );
    assert_eq!(objects(&sai), before);
    model_and_chip_agree(&sai).unwrap();

    // FDB: add, delete, flush
    let key = fdb_key(vlan1, 1);
    let attrs = fdb_attrs(sys::SAI_FDB_ENTRY_TYPE_STATIC, bp0);
    sai.backend_mut().fail_nth("l2_add", 1, Error::Failure).unwrap();
    assert_eq!(sai.create_entry(switch, key, &attrs), Err(Error::Failure));
    assert_eq!(objects(&sai), before);
    model_and_chip_agree(&sai).unwrap();
    sai.create_entry(switch, key, &attrs).unwrap();
    sai.backend_mut().fail_nth("l2_add", 1, Error::Failure).unwrap();
    let to_dynamic = Attr::new(FDB_TYPE, enumerator(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC));
    assert_eq!(sai.set_entry(switch, &key, &to_dynamic), Err(Error::Failure));
    assert_eq!(
        sai.get_entry(switch, &key, &[FDB_TYPE]),
        Ok(vec![enumerator(sys::SAI_FDB_ENTRY_TYPE_STATIC)])
    );
    sai.backend_mut().fail_nth("l2_delete", 1, Error::Failure).unwrap();
    assert_eq!(sai.remove_entry(switch, &key), Err(Error::Failure));
    model_and_chip_agree(&sai).unwrap();
    sai.backend_mut().fail_nth("l2_flush", 1, Error::Failure).unwrap();
    let all = [Attr::new(
        sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
        enumerator(sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL),
    )];
    assert_eq!(sai.flush_fdb(switch, &all), Err(Error::Failure));
    assert_eq!(sai.store().unwrap().entry_count(), 1);
    model_and_chip_agree(&sai).unwrap();
    // an entry the chip aged already: removed from the model all the same (C++)
    sai.backend_mut().fail_nth("l2_delete", 1, Error::ItemNotFound).unwrap();
    sai.remove_entry(switch, &key).unwrap();
    model_and_chip_agree(&sai).unwrap();
}

// ---------------------------------------------------------------------------- call log

/// A switch over the recording backend with the C++ fake's port numbering (8 ports, 4 lanes:
/// hw 1, 5, 9, ...), its creation lines dropped.
fn recorded() -> (Sai<Recording<VirtualBackend>>, RawOid, sai_core::backend::CallLog) {
    let backend = Recording::new(VirtualBackend::new(), Mode::Normal);
    let log = backend.log();
    let mut sai = Sai::new(backend);
    let profile = Profile::from_pairs([("SAI_RS_VIRTUAL_PORTS", "8"), ("SAI_RS_VIRTUAL_COMPAT", "cpp")]);
    let switch = sai.create_switch(&init_attrs(), &profile).unwrap().raw();
    log.take();
    (sai, switch, log)
}

/// The L2 part of `03-vlan-fdb.ops` (all but its port PVID line), with the statuses and backend
/// call lines of the C++ library on its fake backend (`frontend_driver`, 2026-11-22).
#[test]
fn the_call_log_is_the_cpp_librarys() {
    let (mut sai, switch, log) = recorded();
    let rx = sai.take_events().unwrap();
    let ports = ports(&sai, switch);
    let vlan1 = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    let bport = |sai: &Sai<Recording<VirtualBackend>>, i: usize| bport_of(sai, switch, ports[i]).unwrap();
    let mut steps: Vec<(String, Vec<String>)> = Vec::new();
    let mut step = |name: &str, status: Result<(), Error>, log: &sai_core::backend::CallLog| {
        steps.push((format!("{name} = {status:?}"), log.take()));
    };
    let (bp0, bp1, bp2) = (bport(&sai, 0), bport(&sai, 1), bport(&sai, 2));
    let m1 = members_of(&sai, vlan1)[0];
    step("remove bport0", sai.remove(BRIDGE_PORT, bp0), &log);
    step("remove vlan_member#1", sai.remove(VLAN_MEMBER, m1), &log);
    step("remove bport0", sai.remove(BRIDGE_PORT, bp0), &log);
    let bp0 = sai
        .create(
            BRIDGE_PORT,
            switch,
            &bport_attrs(ports[0], Some(sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE)),
        )
        .unwrap();
    step("create bp0", Ok(()), &log);
    step(
        "create bpx",
        sai.create(BRIDGE_PORT, switch, &bport_attrs(ports[0], None))
            .map(|_| ()),
        &log,
    );
    let v100 = sai.create(VLAN, switch, &vlan_attrs(100)).unwrap();
    step("create v100", Ok(()), &log);
    step(
        "create v100b",
        sai.create(VLAN, switch, &vlan_attrs(100)).map(|_| ()),
        &log,
    );
    step(
        "create bad",
        sai.create(VLAN, switch, &vlan_attrs(4095)).map(|_| ()),
        &log,
    );
    let m100 = sai
        .create(
            VLAN_MEMBER,
            switch,
            &member_attrs(v100, bp0, sys::SAI_VLAN_TAGGING_MODE_TAGGED),
        )
        .unwrap();
    step("create m100", Ok(()), &log);
    let untagged = Attr::new(
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
        enumerator(sys::SAI_VLAN_TAGGING_MODE_UNTAGGED),
    );
    step("set m100", sai.set(VLAN_MEMBER, m100, &untagged), &log);
    let instance = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID);
    assert_eq!(
        sai.get(sys::SAI_OBJECT_TYPE_STP, instance, &[sys::SAI_STP_ATTR_VLAN_LIST]),
        Ok(vec![Value::VlanList(vec![1, 100])])
    );
    let mut fwd = fdb_attrs(sys::SAI_FDB_ENTRY_TYPE_STATIC, bp1);
    step(
        "create fdb 55",
        sai.create_entry(switch, fdb_key(vlan1, 0x55), &fwd),
        &log,
    );
    step(
        "create fdb 55 again",
        sai.create_entry(switch, fdb_key(vlan1, 0x55), &fwd[..2]),
        &log,
    );
    fwd[0].value = enumerator(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC);
    step(
        "create fdb 66",
        sai.create_entry(switch, fdb_key(vlan1, 0x66), &fwd[..2]),
        &log,
    );
    let flush = [
        Attr::new(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, Value::Oid(bp1)),
        Attr::new(
            sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
            enumerator(sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC),
        ),
    ];
    step("flush bport1 dynamic", sai.flush_fdb(switch, &flush).map(|_| ()), &log);
    let mac = [0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0x01];
    let mut notes = Vec::new();
    for e in [
        event(FdbChange::Learned, mac, 1, Dest::Port(HwPort(9))),
        event(FdbChange::Aged, mac, 1, Dest::Port(HwPort(9))),
    ] {
        sai.backend_mut().inner_mut().inject_fdb(e);
        while let Ok(e) = rx.try_recv() {
            notes.extend(sai.handle_event(e));
        }
    }
    step("inject learned, aged 01", Ok(()), &log);
    step(
        "create fdb 77",
        sai.create_entry(switch, fdb_key(vlan1, 0x77), &fwd[..2]),
        &log,
    );
    let mac77 = [0x00, 0x11, 0x22, 0x33, 0x44, 0x77];
    for e in [
        event(FdbChange::Learned, mac77, 1, Dest::Port(HwPort(5))),
        event(FdbChange::Learned, mac77, 1, Dest::Port(HwPort(9))),
    ] {
        sai.backend_mut().inner_mut().inject_fdb(e);
        while let Ok(e) = rx.try_recv() {
            notes.extend(sai.handle_event(e));
        }
    }
    step("inject learned 77 twice", Ok(()), &log);
    assert_eq!(
        fdb_notes(&notes),
        [
            (FdbEventKind::Learned, Some(bp2)),
            (FdbEventKind::Aged, Some(bp2)),
            (FdbEventKind::Move, Some(bp2))
        ]
    );
    step("remove fdb 55", sai.remove_entry(switch, &fdb_key(vlan1, 0x55)), &log);
    step(
        "remove fdb 55 again",
        sai.remove_entry(switch, &fdb_key(vlan1, 0x55)),
        &log,
    );
    step("flush", sai.flush_fdb(switch, &[]).map(|_| ()), &log);
    step("remove v100", sai.remove(VLAN, v100), &log);
    step("remove m100", sai.remove(VLAN_MEMBER, m100), &log);
    step("remove v100", sai.remove(VLAN, v100), &log);

    let expected: Vec<(&str, Vec<&str>)> = vec![
        ("remove bport0 = Err(ObjectInUse)", vec![]),
        (
            "remove vlan_member#1 = Ok(())",
            vec!["vlan_port_remove(vid=1, hw=1) -> SUCCESS"],
        ),
        (
            "remove bport0 = Ok(())",
            vec!["l2_flush(hw=1, vid=-1, statics=false, dynamics=true) -> SUCCESS"],
        ),
        (
            "create bp0 = Ok(())",
            vec!["port_learn_set(hw=1, mode=Disable) -> SUCCESS"],
        ),
        ("create bpx = Err(ItemAlreadyExists)", vec![]),
        ("create v100 = Ok(())", vec!["vlan_create(vid=100) -> SUCCESS"]),
        ("create v100b = Err(ItemAlreadyExists)", vec![]),
        ("create bad = Err(Attr(InvalidValue, 0))", vec![]),
        (
            "create m100 = Ok(())",
            vec!["vlan_port_add(vid=100, hw=1, untagged=false) -> SUCCESS"],
        ),
        (
            "set m100 = Ok(())",
            vec!["vlan_port_add(vid=100, hw=1, untagged=true) -> SUCCESS"],
        ),
        (
            "create fdb 55 = Ok(())",
            vec!["l2_add(mac=00:11:22:33:44:55, vid=1, hw=5, is_static=true, discard=false) -> SUCCESS"],
        ),
        ("create fdb 55 again = Err(ItemAlreadyExists)", vec![]),
        (
            "create fdb 66 = Ok(())",
            vec!["l2_add(mac=00:11:22:33:44:66, vid=1, hw=5, is_static=false, discard=false) -> SUCCESS"],
        ),
        (
            "flush bport1 dynamic = Ok(())",
            vec!["l2_flush(hw=5, vid=-1, statics=false, dynamics=true) -> SUCCESS"],
        ),
        (
            "inject learned, aged 01 = Ok(())",
            vec![
                "event fdb(learned=true, mac=00:aa:bb:cc:dd:01, vid=1, hw_port=9)",
                "event fdb(learned=false, mac=00:aa:bb:cc:dd:01, vid=1, hw_port=9)",
            ],
        ),
        (
            "create fdb 77 = Ok(())",
            vec!["l2_add(mac=00:11:22:33:44:77, vid=1, hw=5, is_static=false, discard=false) -> SUCCESS"],
        ),
        (
            "inject learned 77 twice = Ok(())",
            vec![
                "event fdb(learned=true, mac=00:11:22:33:44:77, vid=1, hw_port=5)",
                "event fdb(learned=true, mac=00:11:22:33:44:77, vid=1, hw_port=9)",
            ],
        ),
        (
            "remove fdb 55 = Ok(())",
            vec!["l2_delete(mac=00:11:22:33:44:55, vid=1) -> SUCCESS"],
        ),
        ("remove fdb 55 again = Err(ItemNotFound)", vec![]),
        (
            "flush = Ok(())",
            vec!["l2_flush(hw=-1, vid=-1, statics=false, dynamics=true) -> SUCCESS"],
        ),
        ("remove v100 = Err(ObjectInUse)", vec![]),
        (
            "remove m100 = Ok(())",
            vec!["vlan_port_remove(vid=100, hw=1) -> SUCCESS"],
        ),
        ("remove v100 = Ok(())", vec!["vlan_destroy(vid=100) -> SUCCESS"]),
    ];
    let expected: Vec<(String, Vec<String>)> = expected
        .into_iter()
        .map(|(s, l)| (s.to_owned(), l.into_iter().map(str::to_owned).collect()))
        .collect();
    assert_eq!(steps, expected);
}
