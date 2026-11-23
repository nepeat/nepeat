//! Mirror sessions and `SAMPLEPACKET` on the virtual backend (agent A6): the checks of the C++
//! `tests/test_qos.inc` (the mirror and sampling part of `test_qos`; the ACL mirror action and the
//! genetlink host interface belong to the ACL and host interface areas), failure injection, and
//! the call log of the mirror and sample part of driver script `09-mirror-sample-hash.ops`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
mod a6_common;

use a6_common::*;
use sai_core::backend::types::{MirrorId, SampleId};
use sai_core::error::{AttrError, Error};
use sai_core::oid::RawOid;
use sai_core::value::Value;
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr};

const MIRROR: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_MIRROR_SESSION;
const SAMPLE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SAMPLEPACKET;

fn span(t: &mut T, monitor: RawOid) -> sai_core::Result<RawOid> {
    t.create(
        MIRROR,
        &[
            a(sys::SAI_MIRROR_SESSION_ATTR_TYPE, e(sys::SAI_MIRROR_SESSION_TYPE_LOCAL)),
            a(sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, Value::Oid(monitor)),
        ],
    )
}

fn erspan_attrs(monitor: RawOid) -> Vec<sai_core::value::Attr> {
    vec![
        a(
            sys::SAI_MIRROR_SESSION_ATTR_TYPE,
            e(sys::SAI_MIRROR_SESSION_TYPE_ENHANCED_REMOTE),
        ),
        a(sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, Value::Oid(monitor)),
        a(
            sys::SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE,
            e(sys::SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL),
        ),
        a(sys::SAI_MIRROR_SESSION_ATTR_IPHDR_VERSION, Value::U8(4)),
        a(sys::SAI_MIRROR_SESSION_ATTR_TOS, Value::U8(32)),
        a(sys::SAI_MIRROR_SESSION_ATTR_TTL, Value::U8(64)),
        a(
            sys::SAI_MIRROR_SESSION_ATTR_SRC_IP_ADDRESS,
            Value::IpAddress(IpAddr::V4(Ipv4Addr::new(10, 99, 1, 1))),
        ),
        a(
            sys::SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS,
            Value::IpAddress(IpAddr::V4(Ipv4Addr::new(10, 99, 1, 2))),
        ),
        a(
            sys::SAI_MIRROR_SESSION_ATTR_SRC_MAC_ADDRESS,
            Value::Mac([0x10, 0x11, 0x12, 0x13, 0x14, 0x15]),
        ),
        a(
            sys::SAI_MIRROR_SESSION_ATTR_DST_MAC_ADDRESS,
            Value::Mac([0x20, 0x21, 0x22, 0x23, 0x24, 0x25]),
        ),
        a(sys::SAI_MIRROR_SESSION_ATTR_GRE_PROTOCOL_TYPE, Value::U16(0x88be)),
    ]
}

fn session_hw(t: &T, s: RawOid) -> Option<MirrorId> {
    let id = sai_core::oid::MirrorSessionId::try_from(s).ok()?;
    t.sai
        .store()
        .unwrap()
        .get::<sai_core::mirror::MirrorSession>(id)
        .ok()?
        .hw()
}

fn sample_hw(t: &T, s: RawOid) -> Option<SampleId> {
    let id = sai_core::oid::SamplepacketId::try_from(s).ok()?;
    t.sai
        .store()
        .unwrap()
        .get::<sai_core::mirror::Samplepacket>(id)
        .ok()?
        .hw()
}

fn invalid() -> Error {
    attr(AttrError::InvalidValue)
}

#[test]
fn without_mirroring_in_the_backend_sessions_are_kept_and_ports_refuse() {
    let mut t = T::new();
    let (p1, p2) = (t.ports[1], t.ports[2]);
    let s = span(&mut t, p2).unwrap();
    assert_eq!(session_hw(&t, s), None);
    assert!(t.virt().mirror().mirrors.is_empty());
    // as before mirroring existed: the port has no such attribute
    assert_eq!(
        t.port_set(p1, sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, Value::OidList(vec![])),
        Err(attr(AttrError::NotSupported))
    );
    assert_eq!(
        t.get(PORT, p1, sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    let smp = t
        .create(SAMPLE, &[a(sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, Value::U32(10))])
        .unwrap();
    assert_eq!(sample_hw(&t, smp), None);
    t.set(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_TRUNCATE_SIZE, Value::U16(128))
        .unwrap();
    assert_eq!(
        t.get(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_TRUNCATE_SIZE),
        Ok(Value::U16(128))
    );
    t.remove(MIRROR, s).unwrap();
    t.remove(SAMPLE, smp).unwrap();
    assert_eq!(t.calls(), Vec::<String>::new());
    t.store_ok();
}

#[test]
fn span_and_erspan_sessions_on_ports() {
    let mut t = T::new();
    t.mirror(true);
    let (p1, p2, p3) = (t.ports[1], t.ports[2], t.ports[3]);
    let hw1 = t.hw(p1);
    let sp = span(&mut t, p2).unwrap();
    let mid = session_hw(&t, sp).unwrap();
    let cfg = t.virt().mirror().mirrors[&mid];
    assert!(cfg.monitor == t.hw(p2) && !cfg.erspan);
    let ing = sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    let egr = sys::SAI_PORT_ATTR_EGRESS_MIRROR_SESSION;
    t.port_set(p1, ing, Value::OidList(vec![sp])).unwrap();
    assert_eq!(t.virt().mirror().port_mirrors.get(&(hw1, true)), Some(&vec![mid]));
    t.port_set(p1, egr, Value::OidList(vec![sp])).unwrap();
    assert_eq!(t.virt().mirror().port_mirrors.get(&(hw1, false)), Some(&vec![mid]));
    assert_eq!(t.port_list(p1, ing), vec![sp]);
    assert_eq!(t.remove(MIRROR, sp), Err(Error::ObjectInUse));
    // the CPU port is no monitor port
    let cpu = oid(t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap());
    assert_eq!(
        t.set(MIRROR, sp, sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, Value::Oid(cpu)),
        Err(invalid())
    );
    t.set(MIRROR, sp, sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, Value::Oid(p3))
        .unwrap();
    assert_eq!(t.virt().mirror().mirrors[&mid].monitor, t.hw(p3));
    assert_eq!(
        t.get(MIRROR, sp, sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT),
        Ok(Value::Oid(p3))
    );
    // the CPU port mirrors nothing
    assert_eq!(
        t.port_set(cpu, ing, Value::OidList(vec![sp])),
        Err(attr(AttrError::NotSupported))
    );
    let er = t.create(MIRROR, &erspan_attrs(p2)).unwrap();
    let eid = session_hw(&t, er).unwrap();
    let c = t.virt().mirror().mirrors[&eid];
    assert!(c.erspan && c.tos == 32 && c.ttl == 64 && c.gre_type == 0x88be);
    assert_eq!(c.dst, IpAddr::V4(Ipv4Addr::new(10, 99, 1, 2)));
    assert_eq!(c.dmac.0[5], 0x25);
    t.set(
        MIRROR,
        er,
        sys::SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS,
        Value::IpAddress(IpAddr::V4(Ipv4Addr::new(10, 99, 1, 3))),
    )
    .unwrap();
    assert_eq!(
        t.virt().mirror().mirrors[&eid].dst,
        IpAddr::V4(Ipv4Addr::new(10, 99, 1, 3))
    );
    assert_eq!(
        t.get(MIRROR, er, sys::SAI_MIRROR_SESSION_ATTR_IPHDR_VERSION),
        Ok(Value::U8(4))
    );
    assert_eq!(
        t.get(MIRROR, er, sys::SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE),
        Ok(e(sys::SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL))
    );
    t.port_set(p1, ing, Value::OidList(vec![sp, er])).unwrap();
    assert_eq!(t.virt().mirror().port_mirrors.get(&(hw1, true)), Some(&vec![mid, eid]));
    // a session that is none
    assert_eq!(
        t.port_set(p1, ing, Value::OidList(vec![sp, RawOid(0x1234)])),
        Err(invalid())
    );
    t.port_set(p1, ing, Value::OidList(vec![])).unwrap();
    t.port_set(p1, egr, Value::OidList(vec![])).unwrap();
    assert!(t.virt().mirror().port_mirrors.is_empty());
    t.remove(MIRROR, er).unwrap();
    t.remove(MIRROR, sp).unwrap();
    assert!(t.virt().mirror().mirrors.is_empty());
    // RSPAN, sFlow-type sessions: not implemented; type and monitor mandatory
    assert_eq!(
        t.create(
            MIRROR,
            &[
                a(
                    sys::SAI_MIRROR_SESSION_ATTR_TYPE,
                    e(sys::SAI_MIRROR_SESSION_TYPE_REMOTE)
                ),
                a(sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, Value::Oid(p2))
            ]
        ),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    assert_eq!(
        t.create(
            MIRROR,
            &[a(
                sys::SAI_MIRROR_SESSION_ATTR_TYPE,
                e(sys::SAI_MIRROR_SESSION_TYPE_LOCAL)
            )]
        ),
        Err(Error::MandatoryAttributeMissing)
    );
    // create-only attributes in a set: what the rim answers (the C++ statuses)
    let s = span(&mut t, p2).unwrap();
    assert_eq!(
        t.sai.refused_set(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_TYPE),
        Some(invalid())
    );
    assert_eq!(
        t.sai
            .refused_set(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORTLIST_VALID),
        Some(attr(AttrError::NotSupported))
    );
    // the C++ defaults on get
    assert_eq!(t.get(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_TTL), Ok(Value::U8(255)));
    assert_eq!(
        t.get(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_VLAN_TPID),
        Ok(Value::U16(0x8100))
    );
    assert_eq!(
        t.get(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_SAMPLE_RATE),
        Ok(Value::U32(0))
    );
    t.store_ok();
}

#[test]
fn samplepacket_on_a_port_rate_change_and_in_use() {
    let mut t = T::new();
    t.mirror(true);
    let p1 = t.ports[1];
    let hw1 = t.hw(p1);
    let rate = sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE;
    let smp = t.create(SAMPLE, &[a(rate, Value::U32(1000))]).unwrap();
    let sid = sample_hw(&t, smp).unwrap();
    assert_eq!(t.virt().mirror().samples.get(&sid), Some(&1000));
    let en = sys::SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
    t.port_set(p1, en, Value::Oid(smp)).unwrap();
    assert_eq!(t.virt().mirror().port_samples.get(&(hw1, true)), Some(&sid));
    assert_eq!(t.get(PORT, p1, en), Ok(Value::Oid(smp)));
    t.set(SAMPLE, smp, rate, Value::U32(500)).unwrap();
    assert_eq!(t.virt().mirror().samples.get(&sid), Some(&500));
    assert_eq!(t.set(SAMPLE, smp, rate, Value::U32(0)), Err(invalid()));
    assert_eq!(t.remove(SAMPLE, smp), Err(Error::ObjectInUse));
    t.port_set(p1, en, Value::Oid(RawOid::NULL)).unwrap();
    assert!(t.virt().mirror().port_samples.is_empty());
    t.remove(SAMPLE, smp).unwrap();
    assert!(t.virt().mirror().samples.is_empty());
    // only the slow path; rate mandatory, not 0
    assert_eq!(
        t.create(
            SAMPLE,
            &[
                a(rate, Value::U32(1)),
                a(
                    sys::SAI_SAMPLEPACKET_ATTR_TYPE,
                    e(sys::SAI_SAMPLEPACKET_TYPE_MIRROR_SESSION)
                )
            ]
        ),
        Err(Error::attr(AttrError::NotSupported, 1))
    );
    assert_eq!(t.create(SAMPLE, &[]), Err(Error::MandatoryAttributeMissing));
    assert_eq!(
        t.create(SAMPLE, &[a(rate, Value::U32(0))]),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    // TRUNCATE_ENABLE is settable for SAI, not here (C++: INVALID_ATTR_VALUE_0)
    let s = t.create(SAMPLE, &[a(rate, Value::U32(7))]).unwrap();
    assert_eq!(
        t.set(SAMPLE, s, sys::SAI_SAMPLEPACKET_ATTR_TRUNCATE_ENABLE, Value::Bool(true)),
        Err(invalid())
    );
    assert_eq!(
        t.get(SAMPLE, s, sys::SAI_SAMPLEPACKET_ATTR_MODE),
        Ok(e(sys::SAI_SAMPLEPACKET_MODE_EXCLUSIVE))
    );
    assert_eq!(
        t.sai.refused_set(SAMPLE, s, sys::SAI_SAMPLEPACKET_ATTR_MODE),
        Some(invalid())
    );
    // a failed rate change keeps the old rate on both sides
    t.virt_mut().fail_nth("sample_update", 1, Error::Failure).unwrap();
    assert_eq!(t.set(SAMPLE, s, rate, Value::U32(8)), Err(Error::Failure));
    assert_eq!(t.get(SAMPLE, s, rate), Ok(Value::U32(7)));
    assert_eq!(t.virt().mirror().samples.get(&sample_hw(&t, s).unwrap()), Some(&7));
    t.virt_mut().fail_nth("port_sample_set", 1, Error::Failure).unwrap();
    assert_eq!(t.port_set(p1, en, Value::Oid(s)), Err(Error::Failure));
    assert_eq!(t.get(PORT, p1, en), Ok(Value::Oid(RawOid::NULL)));
    t.remove(SAMPLE, s).unwrap();
    t.store_ok();
}

#[test]
fn a_failed_backend_call_leaves_model_and_chip_as_they_were() {
    let mut t = T::new();
    t.mirror(true);
    let (p1, p2) = (t.ports[1], t.ports[2]);
    let objects = t.sai.store().unwrap().len();
    t.virt_mut().fail_nth("mirror_create", 1, Error::TableFull).unwrap();
    assert_eq!(span(&mut t, p2), Err(Error::TableFull));
    t.virt_mut().fail_nth("sample_create", 1, Error::TableFull).unwrap();
    assert_eq!(
        t.create(SAMPLE, &[a(sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, Value::U32(9))]),
        Err(Error::TableFull)
    );
    assert_eq!(t.sai.store().unwrap().len(), objects);
    let s = span(&mut t, p2).unwrap();
    let chip = |t: &T| format!("{:?}", t.virt().mirror());
    let before = chip(&t);
    t.virt_mut()
        .fail_nth("port_mirror_set", 1, Error::InvalidParameter)
        .unwrap();
    assert_eq!(
        t.port_set(p1, sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, Value::OidList(vec![s])),
        Err(invalid())
    );
    assert_eq!(
        t.port_list(p1, sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION),
        Vec::<RawOid>::new()
    );
    assert_eq!(chip(&t), before);
    t.virt_mut().fail_nth("mirror_update", 1, Error::Failure).unwrap();
    assert_eq!(
        t.set(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_TRUNCATE_SIZE, Value::U16(64)),
        Err(Error::Failure)
    );
    assert_eq!(
        t.get(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_TRUNCATE_SIZE),
        Ok(Value::U16(0))
    );
    assert_eq!(chip(&t), before);
    t.virt_mut().fail_nth("mirror_destroy", 1, Error::Failure).unwrap();
    assert_eq!(t.remove(MIRROR, s), Err(Error::Failure));
    assert!(t.get(MIRROR, s, sys::SAI_MIRROR_SESSION_ATTR_TYPE).is_ok());
    assert_eq!(chip(&t), before);
    t.store_ok();
}

/// The mirror and sample calls of `tests/driver-scripts/09-mirror-sample-hash.ops` on the C++
/// library (`frontend_driver`, recording backend).
const CPP_09_MIRROR: &[&str] = &[
    "mirror_create(cfg={erspan=false,monitor=9,truncate=0,sample_rate=0,tos=0,ttl=255,src=0.0.0.0,dst=0.0.0.0,smac=00:00:00:00:00:00,dmac=00:00:00:00:00:00,gre_type=35006,vlan=false,vlan_tpid=33024,vlan_id=0,vlan_pri=0,vlan_cfi=0}) -> SUCCESS id=1",
    "port_mirror_set(hw=5, ingress=true, ids=[1]) -> SUCCESS",
    "port_mirror_set(hw=5, ingress=false, ids=[1]) -> SUCCESS",
    "mirror_update(id=1, cfg={erspan=false,monitor=13,truncate=0,sample_rate=0,tos=0,ttl=255,src=0.0.0.0,dst=0.0.0.0,smac=00:00:00:00:00:00,dmac=00:00:00:00:00:00,gre_type=35006,vlan=false,vlan_tpid=33024,vlan_id=0,vlan_pri=0,vlan_cfi=0}) -> SUCCESS",
    "mirror_create(cfg={erspan=true,monitor=9,truncate=0,sample_rate=0,tos=32,ttl=64,src=10.99.1.1,dst=10.99.1.2,smac=10:11:12:13:14:15,dmac=20:21:22:23:24:25,gre_type=35006,vlan=false,vlan_tpid=33024,vlan_id=0,vlan_pri=0,vlan_cfi=0}) -> SUCCESS id=2",
    "mirror_update(id=2, cfg={erspan=true,monitor=9,truncate=0,sample_rate=0,tos=32,ttl=64,src=10.99.1.1,dst=10.99.1.3,smac=10:11:12:13:14:15,dmac=20:21:22:23:24:25,gre_type=35006,vlan=false,vlan_tpid=33024,vlan_id=0,vlan_pri=0,vlan_cfi=0}) -> SUCCESS",
    "port_mirror_set(hw=5, ingress=true, ids=[]) -> SUCCESS",
    "port_mirror_set(hw=5, ingress=false, ids=[]) -> SUCCESS",
    "mirror_destroy(id=2) -> SUCCESS",
    "mirror_destroy(id=1) -> SUCCESS",
    "sample_create(rate=1000) -> SUCCESS id=3",
    "port_sample_set(hw=5, ingress=true, id=3) -> SUCCESS",
    "sample_update(id=3, rate=500) -> SUCCESS",
    "port_sample_set(hw=5, ingress=true, id=-1) -> SUCCESS",
    "sample_destroy(id=3) -> SUCCESS",
];

#[test]
fn the_mirror_script_makes_the_cpp_librarys_backend_calls() {
    let mut t = T::new();
    t.mirror(true);
    let (p1, p2, p3) = (t.ports[1], t.ports[2], t.ports[3]);
    let ing = sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    let egr = sys::SAI_PORT_ATTR_EGRESS_MIRROR_SESSION;
    let sp = span(&mut t, p2).unwrap();
    t.port_set(p1, ing, Value::OidList(vec![sp])).unwrap();
    t.port_set(p1, egr, Value::OidList(vec![sp])).unwrap();
    assert_eq!(t.port_list(p1, ing), vec![sp]);
    t.set(MIRROR, sp, sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, Value::Oid(p3))
        .unwrap();
    let er = t.create(MIRROR, &erspan_attrs(p2)).unwrap();
    t.set(
        MIRROR,
        er,
        sys::SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS,
        Value::IpAddress(IpAddr::V4(Ipv4Addr::new(10, 99, 1, 3))),
    )
    .unwrap();
    t.port_set(p1, ing, Value::OidList(vec![])).unwrap();
    t.port_set(p1, egr, Value::OidList(vec![])).unwrap();
    t.remove(MIRROR, er).unwrap();
    t.remove(MIRROR, sp).unwrap();
    let rate = sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE;
    let smp = t.create(SAMPLE, &[a(rate, Value::U32(1000))]).unwrap();
    let en = sys::SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
    t.port_set(p1, en, Value::Oid(smp)).unwrap();
    t.set(SAMPLE, smp, rate, Value::U32(500)).unwrap();
    assert_eq!(t.remove(SAMPLE, smp), Err(Error::ObjectInUse));
    t.port_set(p1, en, Value::Oid(RawOid::NULL)).unwrap();
    t.remove(SAMPLE, smp).unwrap();
    let expected: Vec<String> = CPP_09_MIRROR.iter().map(|l| (*l).to_owned()).collect();
    assert_eq!(t.calls(), expected);
}
