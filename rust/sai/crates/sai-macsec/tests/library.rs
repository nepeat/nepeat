//! What the C++ libsai needs when the model runs inside it (sai-macsec-capi): ids from the
//! library's allocator (D7), the table attached before the SAI ports exist and the ports
//! handed over later (no D5 any more).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test"
)]

mod common;

use common::{EGR, FakePhy, FakeTable, boolean, en, ms_table, oid, switch_ports};
use sai_core::Error;
use sai_core::oid::{RawOid, TypedId};
use sai_macsec::{ChipPort, Dir, Macsec, Options, PortKey};
use sai_sys as sys;
use std::sync::{Arc, Mutex};

fn chip_ports() -> Vec<ChipPort> {
    switch_ports(4)
        .into_iter()
        .map(|p| ChipPort {
            key: p.key,
            name: p.name,
            lanes: p.lanes,
        })
        .collect()
}

fn raw(ty: u32, index: u64) -> RawOid {
    RawOid((u64::from(ty) << 56) | index)
}

#[test]
fn ids_come_from_the_given_allocator() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&asked);
    let mut next = 100u64;
    let options = Options {
        alloc: Some(Box::new(move |ty| {
            seen.lock().unwrap().push(ty);
            next += 1;
            Some(raw(ty, next))
        })),
        ..Options::default()
    };
    let text = ms_table(true, true);
    let table = FakeTable::parse(&text);
    let ports = switch_ports(4);
    let mut phy = FakePhy::new(table.clone(), &ports);
    let mut m = Macsec::attach(&table.model_table(), &ports, options, &mut phy).unwrap();
    let id = m.create_macsec(&[en(sys::SAI_MACSEC_ATTR_DIRECTION, EGR)]).unwrap();
    assert_eq!(id.raw(), raw(sys::SAI_OBJECT_TYPE_MACSEC, 101));
    let t = m
        .create_acl_table(&[en(
            sys::SAI_ACL_TABLE_ATTR_ACL_STAGE,
            sys::SAI_ACL_STAGE_EGRESS_MACSEC.cast_signed(),
        )])
        .unwrap();
    assert_eq!(t.raw(), raw(sys::SAI_OBJECT_TYPE_ACL_TABLE, 102));
    assert_eq!(
        *asked.lock().unwrap(),
        [sys::SAI_OBJECT_TYPE_MACSEC, sys::SAI_OBJECT_TYPE_ACL_TABLE]
    );
    // detach keeps the allocator (the library's counters go on)
    m.detach();
    assert_eq!(
        m.create_macsec(&[en(sys::SAI_MACSEC_ATTR_DIRECTION, EGR)]),
        Err(Error::NotSupported)
    );
}

#[test]
fn an_allocator_that_refuses_fails_the_create_and_changes_nothing() {
    for answer in [
        None,
        Some(raw(sys::SAI_OBJECT_TYPE_PORT, 1)),
        Some(RawOid(raw(sys::SAI_OBJECT_TYPE_MACSEC_FLOW, 1).0 | 1 << 48)),
    ] {
        let options = Options {
            alloc: Some(Box::new(move |_| answer)),
            ..Options::default()
        };
        let text = ms_table(true, true);
        let table = FakeTable::parse(&text);
        let ports = switch_ports(4);
        let mut phy = FakePhy::new(table.clone(), &ports);
        let mut m = Macsec::attach(&table.model_table(), &ports, options, &mut phy).unwrap();
        assert_eq!(
            m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)]),
            Err(Error::Failure)
        );
        assert_eq!(m.counts().flows, 0);
        // a MACSEC_PORT whose sequences ran: the model goes back to "in bypass"
        let attrs = [
            en(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
            oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, ports[0].id.raw()),
        ];
        assert_eq!(m.create_macsec_port(&mut phy, &attrs), Err(Error::Failure));
        assert!(!m.engine_state(ports[0].key, Dir::Egress).unwrap().enabled);
        m.check_invariants().unwrap();
    }
}

#[test]
fn ports_handed_over_after_attach() {
    let text = ms_table(true, true);
    let table = FakeTable::parse(&text);
    let ports = switch_ports(4);
    let mut phy = FakePhy::new(table.clone(), &ports);
    let mut m = Macsec::attach_chip(&table.model_table(), &chip_ports(), Options::default(), &mut phy).unwrap();
    // the probes (and their clean-up: the fake PHYs read "not in bypass") ran as with attach()
    let mut other = FakePhy::new(table.clone(), &ports);
    Macsec::attach(&table.model_table(), &ports, Options::default(), &mut other).unwrap();
    let names = phy.names_take();
    assert_eq!(names, other.names_take());
    assert_eq!(names.len(), 12);
    let attrs = [
        en(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
        oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, ports[0].id.raw()),
    ];
    // no SAI port known yet: the port id is not one of this switch's
    assert_eq!(
        m.create_macsec_port(&mut phy, &attrs),
        Err(Error::attr(sai_core::error::AttrError::InvalidValue, 1))
    );
    let pairs: Vec<_> = ports.iter().map(|p| (p.id, p.key)).collect();
    m.set_switch_ports(&pairs);
    m.create_macsec_port(&mut phy, &attrs).unwrap();
    assert_eq!(phy.names_take(), ["reset.egress", "enable.egress"]);
    // a bound table reaches the port's engine through the handed-over id
    let t = m
        .create_acl_table(&[en(
            sys::SAI_ACL_TABLE_ATTR_ACL_STAGE,
            sys::SAI_ACL_STAGE_EGRESS_MACSEC.cast_signed(),
        )])
        .unwrap();
    m.set_port_acl(&mut phy, ports[0].id, Dir::Egress, t.raw()).unwrap();
    assert_eq!(m.port_acl(ports[0].id, Dir::Egress), t.raw());
    let _ = boolean;
    // ports taken away again: lookups fail as for an unknown port
    m.set_switch_ports(&[]);
    assert_eq!(m.port_ifg(PortKey(ports[0].key.0)), 384);
    m.check_invariants().unwrap();
}
