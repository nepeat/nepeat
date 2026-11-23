//! The LAG area on the virtual backend: trunks, members (egress / ingress disable), the LAG's
//! PVID, a bridge port, VLAN membership and FDB entries on a LAG, a port joining and leaving with
//! that state replayed, a router interface on the LAG. The checks of the C++ suite's `test_lag`
//! (`sai-openbcm/tests/test_sai.cpp`), failure injection, and the backend call log against the
//! C++ oracle's (`tests/driver-scripts/04-lag.ops` on `frontend_driver`).
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
use sai_core::backend::types::{Dest, FdbChange, FdbEvent, Learn, MacAddr, TrunkId};
use sai_core::backend::{Backend, CallLog, HwPort, Mode, Profile, Recording, Vid};
use sai_core::error::{AttrError, Error};
use sai_core::l2::FdbEventKind;
use sai_core::model::{EntryKey, Lag};
use sai_core::oid::{LagId, RawOid, TypedId};
use sai_core::sai::{Notification, Sai};
use sai_core::value::{Attr, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;

const LAG: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_LAG;
const LAG_MEMBER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_LAG_MEMBER;

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

/// Takes a port's default bridge port and VLAN 1 membership away (orchagent does that before
/// a port joins a LAG; the C++ suite's `free_port`).
fn free_port<B: Backend>(sai: &mut Sai<B>, switch: RawOid, port: RawOid) {
    let vlan1 = sw_oid(sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    let bp = bport_of(sai, switch, port).unwrap();
    let members = oid_list(
        sai.get(VLAN, vlan1, &[sys::SAI_VLAN_ATTR_MEMBER_LIST])
            .unwrap()
            .remove(0),
    );
    for m in members {
        if sai
            .get(VLAN_MEMBER, m, &[sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID])
            .unwrap()[0]
            == Value::Oid(bp)
        {
            sai.remove(VLAN_MEMBER, m).unwrap();
        }
    }
    sai.remove(BRIDGE_PORT, bp).unwrap();
}

fn member_attrs(lag: RawOid, port: RawOid, disabled: bool) -> Vec<Attr> {
    let mut attrs = vec![
        Attr::new(sys::SAI_LAG_MEMBER_ATTR_LAG_ID, Value::Oid(lag)),
        Attr::new(sys::SAI_LAG_MEMBER_ATTR_PORT_ID, Value::Oid(port)),
    ];
    if disabled {
        attrs.push(Attr::new(sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, Value::Bool(true)));
        attrs.push(Attr::new(sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, Value::Bool(true)));
    }
    attrs
}

fn bport_attrs(on: RawOid, learning: u32) -> Vec<Attr> {
    vec![
        Attr::new(
            sys::SAI_BRIDGE_PORT_ATTR_TYPE,
            enumerator(sys::SAI_BRIDGE_PORT_TYPE_PORT),
        ),
        Attr::new(sys::SAI_BRIDGE_PORT_ATTR_PORT_ID, Value::Oid(on)),
        Attr::new(sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE, enumerator(learning)),
    ]
}

fn learning(mode: u32) -> Attr {
    Attr::new(sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE, enumerator(mode))
}

fn tagging(mode: u32) -> Attr {
    Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, enumerator(mode))
}

fn tid_of<B: Backend>(sai: &Sai<B>, lag: RawOid) -> TrunkId {
    sai.store()
        .unwrap()
        .get::<Lag>(LagId::try_from(lag).unwrap())
        .unwrap()
        .tid
}

fn trunk(sai: &Sai<VirtualBackend>, tid: TrunkId) -> Option<Vec<(u16, bool, bool)>> {
    sai.backend().lags().get(&tid).map(|m| {
        m.iter()
            .map(|x| (x.hw.0, x.egress_disable, x.ingress_disable))
            .collect()
    })
}

fn pvid(sai: &Sai<VirtualBackend>, hw: u16) -> u16 {
    sai.backend().port_state(HwPort(hw)).unwrap().pvid.get()
}

fn learn(sai: &Sai<VirtualBackend>, hw: u16) -> Learn {
    sai.backend().port_state(HwPort(hw)).unwrap().learn
}

fn chip_member(sai: &Sai<VirtualBackend>, vid: u16, hw: u16) -> Option<bool> {
    sai.backend()
        .vlans()
        .get(&Vid::new(vid).unwrap())
        .and_then(|m| m.get(&HwPort(hw)).copied())
}

#[test]
fn lags_as_the_cpp_suite() {
    let (mut sai, switch) = switch(8);
    let rx = sai.take_events().unwrap();
    let ports = ports(&sai, switch);
    let (p4, p5, p6) = (ports[4], ports[5], ports[6]); // hw 5, 6, 7
    for p in [p4, p5, p6] {
        free_port(&mut sai, switch, p);
    }
    let lag = sai.create(LAG, switch, &[]).unwrap();
    assert_eq!(lag.sai_object_type(), LAG);
    let tid = tid_of(&sai, lag);
    assert_eq!(trunk(&sai, tid), Some(vec![]));
    assert_eq!(
        sai.get(
            LAG,
            lag,
            &[
                sys::SAI_LAG_ATTR_PORT_VLAN_ID,
                sys::SAI_LAG_ATTR_TPID,
                sys::SAI_LAG_ATTR_DROP_TAGGED
            ]
        ),
        Ok(vec![Value::U16(1), Value::U16(0x8100), Value::Bool(false)]) // stubs
    );
    assert_eq!(
        sai.create(
            LAG,
            switch,
            &[Attr::new(sys::SAI_LAG_ATTR_SYSTEM_PORT_AGGREGATE_ID, Value::U32(1))]
        ),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    assert_eq!(
        sai.create(LAG, switch, &[Attr::new(sys::SAI_LAG_ATTR_PORT_VLAN_ID, Value::U16(0))]),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );

    // member, created disabled as orchagent does before LACP converges
    let ma = member_attrs(lag, p4, true);
    let m4 = sai.create(LAG_MEMBER, switch, &ma).unwrap();
    assert_eq!(m4.sai_object_type(), LAG_MEMBER);
    assert_eq!(trunk(&sai, tid), Some(vec![(5, true, true)]));
    assert_eq!(sai.create(LAG_MEMBER, switch, &ma), Err(Error::ItemAlreadyExists));
    assert_eq!(
        sai.get(
            LAG_MEMBER,
            m4,
            &[
                sys::SAI_LAG_MEMBER_ATTR_LAG_ID,
                sys::SAI_LAG_MEMBER_ATTR_PORT_ID,
                sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE,
                sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE,
                sys::SAI_LAG_MEMBER_ATTR_WEIGHT,
            ]
        ),
        Ok(vec![
            Value::Oid(lag),
            Value::Oid(p4),
            Value::Bool(true),
            Value::Bool(true),
            Value::U32(1)
        ])
    );
    // collecting, then distributing
    sai.set(
        LAG_MEMBER,
        m4,
        &Attr::new(sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, Value::Bool(false)),
    )
    .unwrap();
    assert_eq!(trunk(&sai, tid), Some(vec![(5, true, false)]));
    sai.set(
        LAG_MEMBER,
        m4,
        &Attr::new(sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, Value::Bool(false)),
    )
    .unwrap();
    assert_eq!(trunk(&sai, tid), Some(vec![(5, false, false)]));
    // create-only (refused by the metadata check: INVALID_ATTRIBUTE; the C++ says INVALID_ATTR_VALUE)
    assert!(
        sai.set(
            LAG_MEMBER,
            m4,
            &Attr::new(sys::SAI_LAG_MEMBER_ATTR_PORT_ID, Value::Oid(p5))
        )
        .is_err()
    );

    // bad members
    let cpu = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_CPU_PORT);
    assert_eq!(
        sai.create(LAG_MEMBER, switch, &member_attrs(lag, cpu, false)),
        Err(Error::attr(AttrError::InvalidValue, 1))
    );
    // still has its default bridge port
    assert_eq!(
        sai.create(LAG_MEMBER, switch, &member_attrs(lag, ports[7], false)),
        Err(Error::ObjectInUse)
    );
    assert_eq!(
        sai.create(LAG_MEMBER, switch, &member_attrs(lag, p5, false)[..1]),
        Err(Error::MandatoryAttributeMissing)
    );
    let m5 = sai.create(LAG_MEMBER, switch, &member_attrs(lag, p5, false)).unwrap();
    assert_eq!(
        sai.get(LAG, lag, &[sys::SAI_LAG_ATTR_PORT_LIST]),
        Ok(vec![Value::OidList(vec![m4, m5])])
    );
    assert_eq!(sai.remove(LAG, lag), Err(Error::ObjectInUse));
    // one LAG per port
    let other = sai.create(LAG, switch, &[]).unwrap();
    assert_eq!(
        sai.create(LAG_MEMBER, switch, &member_attrs(other, p4, false)),
        Err(Error::ItemAlreadyExists)
    );
    sai.remove(LAG, other).unwrap();
    model_and_chip_agree(&sai).unwrap();

    // LAG PVID goes to every member
    let pvid300 = Attr::new(sys::SAI_LAG_ATTR_PORT_VLAN_ID, Value::U16(300));
    sai.set(LAG, lag, &pvid300).unwrap();
    assert_eq!((pvid(&sai, 5), pvid(&sai, 6)), (300, 300));
    assert_eq!(
        sai.get(PORT, p4, &[sys::SAI_PORT_ATTR_PORT_VLAN_ID]),
        Ok(vec![Value::U16(300)])
    );
    // orchagent writes .u32 into this u16 attribute: on big-endian it reads as 0
    assert_eq!(
        sai.set(LAG, lag, &Attr::new(sys::SAI_LAG_ATTR_PORT_VLAN_ID, Value::U16(0))),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );

    // bridge port on the LAG + VLAN member: applied to every member port
    let bpa = bport_attrs(lag, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE);
    let lbp = sai.create(BRIDGE_PORT, switch, &bpa).unwrap();
    assert_eq!((learn(&sai, 5), learn(&sai, 6)), (Learn::Disable, Learn::Disable));
    assert_eq!(sai.create(BRIDGE_PORT, switch, &bpa), Err(Error::ItemAlreadyExists));
    // a member cannot have its own
    assert_eq!(
        sai.create(
            BRIDGE_PORT,
            switch,
            &bport_attrs(p4, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_HW)
        ),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        sai.get(BRIDGE_PORT, lbp, &[sys::SAI_BRIDGE_PORT_ATTR_PORT_ID]),
        Ok(vec![Value::Oid(lag)])
    );
    sai.set(BRIDGE_PORT, lbp, &learning(sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_HW))
        .unwrap();
    assert_eq!((learn(&sai, 5), learn(&sai, 6)), (Learn::Hw, Learn::Hw));
    let v300 = sai
        .create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(300))])
        .unwrap();
    let vma = [
        Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(v300)),
        Attr::new(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(lbp)),
        tagging(sys::SAI_VLAN_TAGGING_MODE_TAGGED),
    ];
    let vm = sai.create(VLAN_MEMBER, switch, &vma).unwrap();
    assert_eq!(
        (chip_member(&sai, 300, 5), chip_member(&sai, 300, 6)),
        (Some(false), Some(false))
    );
    sai.set(VLAN_MEMBER, vm, &tagging(sys::SAI_VLAN_TAGGING_MODE_UNTAGGED))
        .unwrap();
    assert_eq!(
        (chip_member(&sai, 300, 5), chip_member(&sai, 300, 6)),
        (Some(true), Some(true))
    );
    model_and_chip_agree(&sai).unwrap();

    // a port joining later gets the LAG's VLANs, PVID and learning; leaving undoes them
    sai.set(
        BRIDGE_PORT,
        lbp,
        &learning(sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE),
    )
    .unwrap();
    let m6 = sai.create(LAG_MEMBER, switch, &member_attrs(lag, p6, false)).unwrap();
    assert_eq!(chip_member(&sai, 300, 7), Some(true));
    assert_eq!(pvid(&sai, 7), 300);
    assert_eq!(learn(&sai, 7), Learn::Disable);
    assert_eq!(trunk(&sai, tid).unwrap().len(), 3);
    model_and_chip_agree(&sai).unwrap();
    sai.remove(LAG_MEMBER, m6).unwrap();
    assert_eq!(chip_member(&sai, 300, 7), None);
    assert_eq!(pvid(&sai, 7), 1);
    assert_eq!(learn(&sai, 7), Learn::Hw);
    assert_eq!(
        sai.get(PORT, p6, &[sys::SAI_PORT_ATTR_PORT_VLAN_ID]),
        Ok(vec![Value::U16(1)])
    );
    assert_eq!(trunk(&sai, tid).unwrap().len(), 2);
    assert_eq!(sai.remove(LAG_MEMBER, m6), Err(Error::InvalidObjectId));
    model_and_chip_agree(&sai).unwrap();

    // FDB on the LAG: the entry points at the trunk; hardware learning on the trunk is reported
    // with the LAG's bridge port
    let e = EntryKey::Fdb {
        bv_id: v300,
        mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x91],
    };
    let fat = [
        Attr::new(sys::SAI_FDB_ENTRY_ATTR_TYPE, enumerator(sys::SAI_FDB_ENTRY_TYPE_STATIC)),
        Attr::new(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(lbp)),
    ];
    sai.create_entry(switch, e, &fat).unwrap();
    let EntryKey::Fdb { mac, .. } = e else { panic!() };
    let vid300 = Vid::new(300).unwrap();
    assert_eq!(sai.backend().l2_table()[&(MacAddr(mac), vid300)].dest, Dest::Lag(tid));
    sai.backend_mut().inject_fdb(FdbEvent {
        change: FdbChange::Learned,
        mac: MacAddr([0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0x92]),
        vid: vid300,
        hw_port: Dest::Lag(tid),
    });
    let notes: Vec<Notification> = rx.try_iter().flat_map(|e| sai.handle_event(e)).collect();
    assert!(matches!(
        notes.as_slice(),
        [Notification::Fdb { kind: FdbEventKind::Learned, bridge_port: Some(bp), .. }] if bp.raw() == lbp
    ));
    let flush = [
        Attr::new(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, Value::Oid(lbp)),
        Attr::new(
            sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
            enumerator(sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC),
        ),
    ];
    assert_eq!(sai.flush_fdb(switch, &flush).map(|k| k.len()), Ok(1));
    assert_eq!(sai.backend().l2_table().len(), 1); // the static one stays
    assert_eq!(sai.remove(BRIDGE_PORT, lbp), Err(Error::ObjectInUse));
    sai.remove_entry(switch, &e).unwrap();
    sai.remove(VLAN_MEMBER, vm).unwrap();
    assert_eq!((chip_member(&sai, 300, 5), chip_member(&sai, 300, 6)), (None, None));
    sai.remove(VLAN, v300).unwrap();
    assert_eq!(sai.remove(LAG, lag), Err(Error::ObjectInUse)); // its bridge port
    sai.remove(BRIDGE_PORT, lbp).unwrap();
    assert_eq!(learn(&sai, 5), Learn::Disable); // removing a bridge port does not touch learning
    model_and_chip_agree(&sai).unwrap();

    sai.remove(LAG_MEMBER, m4).unwrap();
    sai.remove(LAG_MEMBER, m5).unwrap();
    assert_eq!(trunk(&sai, tid), Some(vec![]));
    assert_eq!(
        sai_core::lag::port_lag(sai.store().unwrap(), sai_core::oid::PortId::try_from(p4).unwrap()),
        None
    );
    sai.remove(LAG, lag).unwrap();
    assert_eq!(trunk(&sai, tid), None);
    assert_eq!(sai.remove(LAG, lag), Err(Error::InvalidObjectId));
    model_and_chip_agree(&sai).unwrap();
}

/// The router interface part of `test_lag` (the L3 area's internal VLAN, through this area's
/// attachment helpers and member join / leave).
#[test]
fn a_router_interface_on_a_lag() {
    let (mut sai, switch) = switch(8);
    let ports = ports(&sai, switch);
    let (p4, p5, p6) = (ports[4], ports[5], ports[6]);
    for p in [p4, p5, p6] {
        free_port(&mut sai, switch, p);
    }
    let lag = sai.create(LAG, switch, &[]).unwrap();
    sai.create(LAG_MEMBER, switch, &member_attrs(lag, p4, false)).unwrap();
    sai.create(LAG_MEMBER, switch, &member_attrs(lag, p5, false)).unwrap();
    let vr = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    let rif = sai.create(RIF, switch, &port_rif(vr, lag)).unwrap();
    let ivid = match sai.get(LAG, lag, &[sys::SAI_LAG_ATTR_PORT_VLAN_ID]).unwrap()[0] {
        Value::U16(v) => v,
        ref other => panic!("{other:?}"),
    };
    assert_ne!(ivid, 1);
    assert_eq!(
        (chip_member(&sai, ivid, 5), chip_member(&sai, ivid, 6)),
        (Some(true), Some(true))
    );
    assert_eq!((pvid(&sai, 5), pvid(&sai, 6)), (ivid, ivid));
    // not on a member
    assert_eq!(sai.create(RIF, switch, &port_rif(vr, p4)), Err(Error::InvalidParameter));
    let m6 = sai.create(LAG_MEMBER, switch, &member_attrs(lag, p6, false)).unwrap();
    assert_eq!(chip_member(&sai, ivid, 7), Some(true));
    assert_eq!(pvid(&sai, 7), ivid);
    sai.remove(LAG_MEMBER, m6).unwrap();
    assert_eq!(chip_member(&sai, ivid, 7), None);
    assert_eq!(sai.remove(LAG, lag), Err(Error::ObjectInUse));
    sai.remove(RIF, rif).unwrap();
    assert!(!sai.backend().vlans().contains_key(&Vid::new(ivid).unwrap()));
    assert_eq!((pvid(&sai, 5), pvid(&sai, 6)), (1, 1));
    assert_eq!(
        sai.get(LAG, lag, &[sys::SAI_LAG_ATTR_PORT_VLAN_ID]),
        Ok(vec![Value::U16(1)])
    );
}

/// Backend first, store second; where the C++ undoes (member create), model and chip end as
/// they were.
#[test]
fn a_failed_backend_call_leaves_model_and_chip_as_they_were() {
    let (mut sai, switch) = switch(4);
    let ports = ports(&sai, switch);
    for p in &ports[..3] {
        free_port(&mut sai, switch, *p);
    }
    let count = |sai: &Sai<VirtualBackend>| sai.store().unwrap().len();

    let before = count(&sai);
    sai.backend_mut()
        .fail_nth("lag_create", 1, Error::InsufficientResources)
        .unwrap();
    assert_eq!(sai.create(LAG, switch, &[]), Err(Error::InsufficientResources));
    assert_eq!(count(&sai), before);
    let lag = sai.create(LAG, switch, &[]).unwrap();
    let tid = tid_of(&sai, lag);

    // the trunk refuses the member
    let before = count(&sai);
    sai.backend_mut()
        .fail_nth("lag_set_members", 1, Error::Failure)
        .unwrap();
    assert_eq!(
        sai.create(LAG_MEMBER, switch, &member_attrs(lag, ports[0], false)),
        Err(Error::Failure)
    );
    assert_eq!(count(&sai), before);
    model_and_chip_agree(&sai).unwrap();
    let m0 = sai
        .create(LAG_MEMBER, switch, &member_attrs(lag, ports[0], false))
        .unwrap();

    // a bridge port and VLAN on the LAG; the replay on a joining port fails half-way: the
    // trunk, VLAN and learning of the port are undone
    let lbp = sai
        .create(
            BRIDGE_PORT,
            switch,
            &bport_attrs(lag, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE),
        )
        .unwrap();
    let v40 = sai
        .create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(40))])
        .unwrap();
    let v41 = sai
        .create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(41))])
        .unwrap();
    for v in [v40, v41] {
        sai.create(
            VLAN_MEMBER,
            switch,
            &[
                Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(v)),
                Attr::new(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(lbp)),
            ],
        )
        .unwrap();
    }
    model_and_chip_agree(&sai).unwrap();
    let before = count(&sai);
    for (op, n) in [("vlan_port_add", 2), ("port_learn_set", 1), ("port_pvid_set", 1)] {
        sai.backend_mut().fail_nth(op, n, Error::Failure).unwrap();
        assert_eq!(
            sai.create(LAG_MEMBER, switch, &member_attrs(lag, ports[1], false)),
            Err(Error::Failure),
            "{op}"
        );
        assert_eq!(count(&sai), before, "{op}");
        assert_eq!(trunk(&sai, tid).unwrap().len(), 1, "{op}");
        assert_eq!(
            (chip_member(&sai, 40, 2), chip_member(&sai, 41, 2)),
            (None, None),
            "{op}"
        );
        assert_eq!(learn(&sai, 2), Learn::Hw, "{op}");
        model_and_chip_agree(&sai).unwrap();
    }

    // member flags: the trunk refuses, the flag stays
    sai.backend_mut()
        .fail_nth("lag_set_members", 1, Error::InvalidParameter)
        .unwrap();
    assert_eq!(
        sai.set(
            LAG_MEMBER,
            m0,
            &Attr::new(sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, Value::Bool(true))
        ),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    assert_eq!(
        sai.get(LAG_MEMBER, m0, &[sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE]),
        Ok(vec![Value::Bool(false)])
    );
    model_and_chip_agree(&sai).unwrap();

    // member remove: the trunk refuses, nothing changes
    sai.backend_mut()
        .fail_nth("lag_set_members", 1, Error::Failure)
        .unwrap();
    assert_eq!(sai.remove(LAG_MEMBER, m0), Err(Error::Failure));
    model_and_chip_agree(&sai).unwrap();
    assert_eq!(learn(&sai, 1), Learn::Disable);

    // LAG PVID: as the C++, the LAG keeps the new PVID and every member is tried (the one that
    // failed keeps its old one)
    sai.backend_mut().fail_nth("port_pvid_set", 1, Error::Failure).unwrap();
    assert_eq!(
        sai.set(LAG, lag, &Attr::new(sys::SAI_LAG_ATTR_PORT_VLAN_ID, Value::U16(40))),
        Err(Error::Failure)
    );
    assert_eq!(
        sai.get(LAG, lag, &[sys::SAI_LAG_ATTR_PORT_VLAN_ID]),
        Ok(vec![Value::U16(40)])
    );
    assert_eq!(pvid(&sai, 1), 1);

    // LAG remove: the trunk refuses (ITEM_NOT_FOUND is taken as gone already)
    let members: Vec<RawOid> = oid_list(sai.get(VLAN, v40, &[sys::SAI_VLAN_ATTR_MEMBER_LIST]).unwrap().remove(0))
        .into_iter()
        .chain(oid_list(
            sai.get(VLAN, v41, &[sys::SAI_VLAN_ATTR_MEMBER_LIST]).unwrap().remove(0),
        ))
        .collect();
    for m in members {
        sai.remove(VLAN_MEMBER, m).unwrap();
    }
    sai.remove(BRIDGE_PORT, lbp).unwrap();
    sai.remove(LAG_MEMBER, m0).unwrap();
    sai.backend_mut().fail_nth("lag_destroy", 1, Error::Failure).unwrap();
    assert_eq!(sai.remove(LAG, lag), Err(Error::Failure));
    model_and_chip_agree(&sai).unwrap();
    sai.backend_mut()
        .fail_nth("lag_destroy", 1, Error::ItemNotFound)
        .unwrap();
    sai.remove(LAG, lag).unwrap();
    // the chip still has the trunk it says it does not know: nothing in the model
    assert_eq!(sai.store().unwrap().iter::<Lag>().count(), 0);
}

// ---------------------------------------------------------------------------- call log

fn recorded() -> (Sai<Recording<VirtualBackend>>, RawOid, CallLog) {
    let backend = Recording::new(VirtualBackend::new(), Mode::Normal);
    let log = backend.log();
    let mut sai = Sai::new(backend);
    let profile = Profile::from_pairs([("SAI_RS_VIRTUAL_PORTS", "8"), ("SAI_RS_VIRTUAL_COMPAT", "cpp")]);
    let switch = sai.create_switch(&init_attrs(), &profile).unwrap().raw();
    log.take();
    (sai, switch, log)
}

/// `04-lag.ops` up to the router interface, with the statuses and backend call lines of the C++
/// library on its fake backend (`frontend_driver`, 2026-11-22).
#[test]
fn the_call_log_is_the_cpp_librarys() {
    let (mut sai, switch, log) = recorded();
    let rx = sai.take_events().unwrap();
    let ports = ports(&sai, switch);
    let mut steps: Vec<(String, Vec<String>)> = Vec::new();
    let mut step = |name: &str, status: Result<(), Error>, log: &CallLog| {
        steps.push((format!("{name} = {status:?}"), log.take()));
    };
    let vlan1 = sw_oid(&sai, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    let members = oid_list(
        sai.get(VLAN, vlan1, &[sys::SAI_VLAN_ATTR_MEMBER_LIST])
            .unwrap()
            .remove(0),
    );
    for i in 4..7 {
        step("remove vlan_member", sai.remove(VLAN_MEMBER, members[i]), &log);
        let bp = bport_of(&sai, switch, ports[i]).unwrap();
        step("remove bport", sai.remove(BRIDGE_PORT, bp), &log);
    }
    let lag = sai.create(LAG, switch, &[]).unwrap();
    step("create lag1", Ok(()), &log);
    assert_eq!(
        sai.get(
            LAG,
            lag,
            &[
                sys::SAI_LAG_ATTR_PORT_VLAN_ID,
                sys::SAI_LAG_ATTR_TPID,
                sys::SAI_LAG_ATTR_PORT_LIST
            ]
        ),
        Ok(vec![Value::U16(1), Value::U16(33024), Value::OidList(vec![])])
    );
    let lm4 = sai
        .create(LAG_MEMBER, switch, &member_attrs(lag, ports[4], true))
        .unwrap();
    step("create lm4", Ok(()), &log);
    step(
        "set lm4 ingress",
        sai.set(
            LAG_MEMBER,
            lm4,
            &Attr::new(sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, Value::Bool(false)),
        ),
        &log,
    );
    step(
        "set lm4 egress",
        sai.set(
            LAG_MEMBER,
            lm4,
            &Attr::new(sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, Value::Bool(false)),
        ),
        &log,
    );
    let lm5 = sai
        .create(LAG_MEMBER, switch, &member_attrs(lag, ports[5], false))
        .unwrap();
    step("create lm5", Ok(()), &log);
    step(
        "create lmx",
        sai.create(LAG_MEMBER, switch, &member_attrs(lag, ports[7], false))
            .map(|_| ()),
        &log,
    );
    assert_eq!(
        sai.get(LAG, lag, &[sys::SAI_LAG_ATTR_PORT_LIST]),
        Ok(vec![Value::OidList(vec![lm4, lm5])])
    );
    step(
        "set lag pvid",
        sai.set(LAG, lag, &Attr::new(sys::SAI_LAG_ATTR_PORT_VLAN_ID, Value::U16(300))),
        &log,
    );
    let lbp = sai
        .create(
            BRIDGE_PORT,
            switch,
            &bport_attrs(lag, sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_DISABLE),
        )
        .unwrap();
    step("create lbp", Ok(()), &log);
    step(
        "set lbp",
        sai.set(BRIDGE_PORT, lbp, &learning(sys::SAI_BRIDGE_PORT_FDB_LEARNING_MODE_HW)),
        &log,
    );
    let v300 = sai
        .create(VLAN, switch, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(300))])
        .unwrap();
    step("create v300", Ok(()), &log);
    let vm = sai
        .create(
            VLAN_MEMBER,
            switch,
            &[
                Attr::new(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, Value::Oid(v300)),
                Attr::new(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, Value::Oid(lbp)),
                tagging(sys::SAI_VLAN_TAGGING_MODE_TAGGED),
            ],
        )
        .unwrap();
    step("create vm", Ok(()), &log);
    step(
        "set vm",
        sai.set(VLAN_MEMBER, vm, &tagging(sys::SAI_VLAN_TAGGING_MODE_UNTAGGED)),
        &log,
    );
    let lm6 = sai
        .create(LAG_MEMBER, switch, &member_attrs(lag, ports[6], false))
        .unwrap();
    step("create lm6", Ok(()), &log);
    step("remove lm6", sai.remove(LAG_MEMBER, lm6), &log);
    let e = EntryKey::Fdb {
        bv_id: v300,
        mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x91],
    };
    step(
        "create fdb 91",
        sai.create_entry(
            switch,
            e,
            &[
                Attr::new(sys::SAI_FDB_ENTRY_ATTR_TYPE, enumerator(sys::SAI_FDB_ENTRY_TYPE_STATIC)),
                Attr::new(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(lbp)),
            ],
        ),
        &log,
    );
    let tid = tid_of(&sai, lag);
    sai.backend_mut().inner_mut().inject_fdb(FdbEvent {
        change: FdbChange::Learned,
        mac: MacAddr([0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0x92]),
        vid: Vid::new(300).unwrap(),
        hw_port: Dest::Lag(tid),
    });
    let notes: Vec<Notification> = rx.try_iter().flat_map(|e| sai.handle_event(e)).collect();
    assert!(matches!(
        notes.as_slice(),
        [Notification::Fdb { kind: FdbEventKind::Learned, bridge_port: Some(bp), .. }] if bp.raw() == lbp
    ));
    step("inject learned 92", Ok(()), &log);
    let flush = [
        Attr::new(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, Value::Oid(lbp)),
        Attr::new(
            sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
            enumerator(sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC),
        ),
    ];
    step("flush lbp", sai.flush_fdb(switch, &flush).map(|_| ()), &log);
    step("remove fdb 91", sai.remove_entry(switch, &e), &log);
    step("remove vm", sai.remove(VLAN_MEMBER, vm), &log);
    step("remove v300", sai.remove(VLAN, v300), &log);
    step("remove lbp", sai.remove(BRIDGE_PORT, lbp), &log);
    step("remove lm4", sai.remove(LAG_MEMBER, lm4), &log);
    step("remove lm5", sai.remove(LAG_MEMBER, lm5), &log);
    step("remove lag1", sai.remove(LAG, lag), &log);

    let m = |hw: u16, e: bool, i: bool| format!("{{hw={hw},egress_disable={e},ingress_disable={i}}}");
    let expected: Vec<(&str, Vec<String>)> = vec![
        (
            "remove vlan_member",
            vec!["vlan_port_remove(vid=1, hw=17) -> SUCCESS".into()],
        ),
        (
            "remove bport",
            vec!["l2_flush(hw=17, vid=-1, statics=false, dynamics=true) -> SUCCESS".into()],
        ),
        (
            "remove vlan_member",
            vec!["vlan_port_remove(vid=1, hw=21) -> SUCCESS".into()],
        ),
        (
            "remove bport",
            vec!["l2_flush(hw=21, vid=-1, statics=false, dynamics=true) -> SUCCESS".into()],
        ),
        (
            "remove vlan_member",
            vec!["vlan_port_remove(vid=1, hw=25) -> SUCCESS".into()],
        ),
        (
            "remove bport",
            vec!["l2_flush(hw=25, vid=-1, statics=false, dynamics=true) -> SUCCESS".into()],
        ),
        ("create lag1", vec!["lag_create() -> SUCCESS tid=1".into()]),
        (
            "create lm4",
            vec![
                format!("lag_set_members(tid=1, members=[{}]) -> SUCCESS", m(17, true, true)),
                "port_pvid_set(hw=17, vid=1) -> SUCCESS".into(),
            ],
        ),
        (
            "set lm4 ingress",
            vec![format!(
                "lag_set_members(tid=1, members=[{}]) -> SUCCESS",
                m(17, true, false)
            )],
        ),
        (
            "set lm4 egress",
            vec![format!(
                "lag_set_members(tid=1, members=[{}]) -> SUCCESS",
                m(17, false, false)
            )],
        ),
        (
            "create lm5",
            vec![
                format!(
                    "lag_set_members(tid=1, members=[{},{}]) -> SUCCESS",
                    m(17, false, false),
                    m(21, false, false)
                ),
                "port_pvid_set(hw=21, vid=1) -> SUCCESS".into(),
            ],
        ),
        ("create lmx", vec![]),
        (
            "set lag pvid",
            vec![
                "port_pvid_set(hw=17, vid=300) -> SUCCESS".into(),
                "port_pvid_set(hw=21, vid=300) -> SUCCESS".into(),
            ],
        ),
        (
            "create lbp",
            vec![
                "port_learn_set(hw=17, mode=Disable) -> SUCCESS".into(),
                "port_learn_set(hw=21, mode=Disable) -> SUCCESS".into(),
            ],
        ),
        (
            "set lbp",
            vec![
                "port_learn_set(hw=17, mode=Hw) -> SUCCESS".into(),
                "port_learn_set(hw=21, mode=Hw) -> SUCCESS".into(),
            ],
        ),
        ("create v300", vec!["vlan_create(vid=300) -> SUCCESS".into()]),
        (
            "create vm",
            vec![
                "vlan_port_add(vid=300, hw=17, untagged=false) -> SUCCESS".into(),
                "vlan_port_add(vid=300, hw=21, untagged=false) -> SUCCESS".into(),
            ],
        ),
        (
            "set vm",
            vec![
                "vlan_port_add(vid=300, hw=17, untagged=true) -> SUCCESS".into(),
                "vlan_port_add(vid=300, hw=21, untagged=true) -> SUCCESS".into(),
            ],
        ),
        (
            "create lm6",
            vec![
                format!(
                    "lag_set_members(tid=1, members=[{},{},{}]) -> SUCCESS",
                    m(17, false, false),
                    m(21, false, false),
                    m(25, false, false)
                ),
                "port_learn_set(hw=25, mode=Hw) -> SUCCESS".into(),
                "vlan_port_add(vid=300, hw=25, untagged=true) -> SUCCESS".into(),
                "port_pvid_set(hw=25, vid=300) -> SUCCESS".into(),
            ],
        ),
        (
            "remove lm6",
            vec![
                format!(
                    "lag_set_members(tid=1, members=[{},{}]) -> SUCCESS",
                    m(17, false, false),
                    m(21, false, false)
                ),
                "vlan_port_remove(vid=300, hw=25) -> SUCCESS".into(),
                "port_learn_set(hw=25, mode=Hw) -> SUCCESS".into(),
                "port_pvid_set(hw=25, vid=1) -> SUCCESS".into(),
            ],
        ),
        (
            "create fdb 91",
            vec![
                "l2_add(mac=00:11:22:33:44:91, vid=300, hw=1073741825, is_static=true, discard=false) -> SUCCESS"
                    .into(),
            ],
        ),
        (
            "inject learned 92",
            vec!["event fdb(learned=true, mac=00:aa:bb:cc:dd:92, vid=300, hw_port=1073741825)".into()],
        ),
        (
            "flush lbp",
            vec!["l2_flush(hw=1073741825, vid=-1, statics=false, dynamics=true) -> SUCCESS".into()],
        ),
        (
            "remove fdb 91",
            vec!["l2_delete(mac=00:11:22:33:44:91, vid=300) -> SUCCESS".into()],
        ),
        (
            "remove vm",
            vec![
                "vlan_port_remove(vid=300, hw=17) -> SUCCESS".into(),
                "vlan_port_remove(vid=300, hw=21) -> SUCCESS".into(),
            ],
        ),
        ("remove v300", vec!["vlan_destroy(vid=300) -> SUCCESS".into()]),
        (
            "remove lbp",
            vec!["l2_flush(hw=1073741825, vid=-1, statics=false, dynamics=true) -> SUCCESS".into()],
        ),
        (
            "remove lm4",
            vec![
                format!("lag_set_members(tid=1, members=[{}]) -> SUCCESS", m(21, false, false)),
                "port_pvid_set(hw=17, vid=1) -> SUCCESS".into(),
            ],
        ),
        (
            "remove lm5",
            vec![
                "lag_set_members(tid=1, members=[]) -> SUCCESS".into(),
                "port_pvid_set(hw=21, vid=1) -> SUCCESS".into(),
            ],
        ),
        ("remove lag1", vec!["lag_destroy(tid=1) -> SUCCESS".into()]),
    ];
    let statuses = ["create lmx"];
    let expected: Vec<(String, Vec<String>)> = expected
        .into_iter()
        .map(|(s, l)| {
            let status = if statuses.contains(&s) {
                "Err(ObjectInUse)"
            } else {
                "Ok(())"
            };
            (format!("{s} = {status}"), l)
        })
        .collect();
    assert_eq!(steps, expected);
}
