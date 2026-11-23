//! The switch's hash objects and seeds, port storm control and VLAN flood control on the virtual
//! backend (agent A6): the checks of the C++ `tests/test_qos.inc` `test_l2_extras`, failure
//! injection, and the call log of the hash / storm / flood part of driver script
//! `09-mirror-sample-hash.ops`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
mod a6_common;

use a6_common::*;
use sai_core::backend::Vid;
use sai_core::backend::types::{FloodKind, StormKind};
use sai_core::error::{AttrError, Error};
use sai_core::oid::RawOid;
use sai_core::value::Value;
use sai_sys as sys;

const HASH: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_HASH;
const VLAN: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_VLAN;
const POLICER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_POLICER;

fn fields() -> Value {
    Value::I32List(
        [
            sys::SAI_NATIVE_HASH_FIELD_SRC_IP,
            sys::SAI_NATIVE_HASH_FIELD_DST_IP,
            sys::SAI_NATIVE_HASH_FIELD_L4_SRC_PORT,
        ]
        .map(u32::cast_signed)
        .to_vec(),
    )
}

fn policer(t: &mut T) -> RawOid {
    t.create(
        POLICER,
        &[
            a(sys::SAI_POLICER_ATTR_METER_TYPE, e(sys::SAI_METER_TYPE_PACKETS)),
            a(sys::SAI_POLICER_ATTR_MODE, e(sys::SAI_POLICER_MODE_SR_TCM)),
            a(sys::SAI_POLICER_ATTR_CIR, Value::U64(1000)),
            a(sys::SAI_POLICER_ATTR_CBS, Value::U64(1000)),
        ],
    )
    .unwrap()
}

fn vlan(t: &mut T, vid: u16) -> RawOid {
    t.create(VLAN, &[a(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(vid))])
        .unwrap()
}

/// The switch's hash objects, read from the model (the attribute is "not supported" without a
/// hash backend).
fn hashes(t: &T) -> (RawOid, RawOid) {
    let store = t.sai.store().unwrap();
    let (_, s) = store.iter::<sai_core::model::Switch>().next().unwrap();
    (
        s.objects[&sys::SAI_SWITCH_ATTR_LAG_HASH],
        s.objects[&sys::SAI_SWITCH_ATTR_ECMP_HASH],
    )
}

#[test]
fn the_hash_objects_without_and_with_a_hash_backend() {
    let mut t = T::new();
    let list = sys::SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST;
    // no hash backend: unsupported, as before
    assert_eq!(
        t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_LAG_HASH),
        Err(attr(AttrError::NotSupported))
    );
    let (lh, eh) = hashes(&t);
    assert_eq!(t.get(HASH, eh, list), Err(Error::InvalidObjectId));
    assert_eq!(t.set(HASH, eh, list, fields()), Err(Error::InvalidObjectId));
    // the seed is kept as a stub
    t.set(
        SWITCH,
        t.switch,
        sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED,
        Value::U32(5),
    )
    .unwrap();
    assert_eq!(t.calls(), Vec::<String>::new());
    t.l2x(true);
    assert_eq!(
        t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_LAG_HASH),
        Ok(Value::Oid(lh))
    );
    assert_eq!(
        t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_ECMP_HASH),
        Ok(Value::Oid(eh))
    );
    assert!(lh != eh && lh.sai_object_type() == HASH);
    t.set(HASH, eh, list, fields()).unwrap();
    assert_eq!(
        t.virt().l2x().hash.get(&false),
        Some(&(vec![0, 1, 7], 5)),
        "the kept seed goes with the fields"
    );
    assert_eq!(t.get(HASH, eh, list), Ok(fields()));
    assert_eq!(t.get(HASH, lh, list), Ok(Value::I32List(vec![])));
    assert_eq!(t.create(HASH, &[]), Err(Error::NotSupported));
    assert_eq!(t.remove(HASH, eh), Err(Error::NotSupported));
    assert_eq!(
        t.set(HASH, eh, sys::SAI_HASH_ATTR_UDF_GROUP_LIST, Value::OidList(vec![])),
        Err(attr(AttrError::NotSupported))
    );
    t.set(HASH, lh, list, fields()).unwrap();
    t.set(
        SWITCH,
        t.switch,
        sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED,
        Value::U32(77),
    )
    .unwrap();
    assert_eq!(t.virt().l2x().hash.get(&true), Some(&(vec![0, 1, 7], 77)));
    assert_eq!(
        t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED),
        Ok(Value::U32(77))
    );
    t.store_ok();
}

#[test]
fn storm_control_policers_on_ports() {
    let mut t = T::new();
    let p1 = t.ports[1];
    let hw1 = t.hw(p1);
    let pol = policer(&mut t);
    let bc = sys::SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID;
    let fl = sys::SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID;
    // no hook: as before
    assert_eq!(t.port_set(p1, bc, Value::Oid(pol)), Err(attr(AttrError::NotSupported)));
    assert!(t.virt().l2x().storm.is_empty());
    assert_eq!(t.get(PORT, p1, bc), Ok(Value::Oid(RawOid::NULL)));
    t.l2x(true);
    t.port_set(p1, fl, Value::Oid(pol)).unwrap();
    let pid = t.virt().fp().policers.keys().next().copied();
    assert!(pid.is_some());
    assert_eq!(
        t.virt().l2x().storm.get(&(hw1, StormKind::UnknownUnicast)).copied(),
        pid
    );
    assert_eq!(t.get(PORT, p1, fl), Ok(Value::Oid(pol)));
    assert_eq!(t.remove(POLICER, pol), Err(Error::ObjectInUse));
    // not a policer
    assert_eq!(
        t.port_set(p1, fl, Value::Oid(RawOid(0x1234))),
        Err(attr(AttrError::InvalidValue))
    );
    t.port_set(p1, fl, Value::Oid(RawOid::NULL)).unwrap();
    t.port_set(p1, bc, Value::Oid(RawOid::NULL)).unwrap();
    assert!(t.virt().l2x().storm.is_empty());
    t.remove(POLICER, pol).unwrap();
    t.store_ok();
}

#[test]
fn vlan_flood_control_is_programmed_or_kept() {
    let mut t = T::new();
    let v = vlan(&mut t, 300);
    let bc = sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE;
    let vid = Vid::new(300).unwrap();
    // no flood control in the backend: kept
    t.set(VLAN, v, bc, e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_NONE)).unwrap();
    assert!(t.virt().l2x().flood.is_empty());
    assert_eq!(t.get(VLAN, v, bc), Ok(e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_NONE)));
    t.l2x(true);
    t.set(VLAN, v, bc, e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_NONE)).unwrap();
    assert_eq!(t.virt().l2x().flood.get(&(vid, FloodKind::Broadcast)), Some(&false));
    t.set(VLAN, v, bc, e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL)).unwrap();
    assert_eq!(t.virt().l2x().flood.get(&(vid, FloodKind::Broadcast)), Some(&true));
    // an L2MC group: kept, not programmed
    t.calls();
    t.set(
        VLAN,
        v,
        sys::SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE,
        e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_L2MC_GROUP),
    )
    .unwrap();
    assert_eq!(t.calls(), Vec::<String>::new());
    // a failure the chip reports is the attribute's
    t.virt_mut()
        .fail_nth("vlan_flood_set", 1, Error::InvalidParameter)
        .unwrap();
    assert_eq!(
        t.set(VLAN, v, bc, e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_NONE)),
        Err(attr(AttrError::InvalidValue))
    );
    assert_eq!(t.virt().l2x().flood.get(&(vid, FloodKind::Broadcast)), Some(&true));
    assert_eq!(t.get(VLAN, v, bc), Ok(e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL)));
    t.store_ok();
}

#[test]
fn a_failed_backend_call_leaves_model_and_chip_as_they_were() {
    let mut t = T::new();
    t.l2x(true);
    let (_, eh) = hashes(&t);
    let list = sys::SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST;
    t.virt_mut().fail_nth("hash_set", 1, Error::Failure).unwrap();
    assert_eq!(t.set(HASH, eh, list, fields()), Err(Error::Failure));
    assert_eq!(t.get(HASH, eh, list), Ok(Value::I32List(vec![])));
    assert!(t.virt().l2x().hash.is_empty());
    t.virt_mut().fail_nth("hash_set", 1, Error::Failure).unwrap();
    assert_eq!(
        t.set(
            SWITCH,
            t.switch,
            sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED,
            Value::U32(9)
        ),
        Err(Error::Failure)
    );
    assert_eq!(
        t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED),
        Ok(Value::U32(0))
    );
    let p1 = t.ports[1];
    let pol = policer(&mut t);
    let fl = sys::SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID;
    t.virt_mut().fail_nth("port_storm_set", 1, Error::Failure).unwrap();
    assert_eq!(t.port_set(p1, fl, Value::Oid(pol)), Err(Error::Failure));
    assert_eq!(t.get(PORT, p1, fl), Ok(Value::Oid(RawOid::NULL)));
    assert!(t.virt().l2x().storm.is_empty());
    t.remove(POLICER, pol).unwrap(); // the failed binding holds nothing
    t.store_ok();
}

/// The hash, storm and VLAN flood calls of `tests/driver-scripts/09-mirror-sample-hash.ops` on
/// the C++ library (`frontend_driver`, recording backend), with the policer and VLAN calls of the
/// ACL and L2 areas between them.
const CPP_09_HASH: &[&str] = &[
    "hash_set(lag=false, fields=[0,1,7], seed=0) -> SUCCESS",
    "hash_set(lag=true, fields=[], seed=77) -> SUCCESS",
    "policer_create(cfg={tr_tcm=false,packets=true,color_blind=false,cir=1000,cbs=1000,pir=0,pbs=0}) -> SUCCESS pid=1",
    "port_storm_set(hw=5, kind=UnknownUnicast, policer=1) -> SUCCESS",
    "vlan_create(vid=300) -> SUCCESS",
    "vlan_flood_set(vid=300, kind=Broadcast, on=false) -> SUCCESS",
    "vlan_flood_set(vid=300, kind=Broadcast, on=true) -> SUCCESS",
    "port_storm_set(hw=5, kind=UnknownUnicast, policer=-1) -> SUCCESS",
    "policer_destroy(pid=1) -> SUCCESS",
    "vlan_destroy(vid=300) -> SUCCESS",
];

#[test]
fn the_hash_script_makes_the_cpp_librarys_backend_calls() {
    let mut t = T::new();
    assert_eq!(
        t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_LAG_HASH),
        Err(attr(AttrError::NotSupported))
    );
    t.l2x(true);
    let lh = oid(t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_LAG_HASH).unwrap());
    let eh = oid(t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_ECMP_HASH).unwrap());
    assert_ne!(lh, eh);
    let list = sys::SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST;
    t.set(HASH, eh, list, fields()).unwrap();
    assert_eq!(t.get(HASH, eh, list), Ok(fields()));
    t.set(
        SWITCH,
        t.switch,
        sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED,
        Value::U32(77),
    )
    .unwrap();
    let pol = policer(&mut t);
    let p1 = t.ports[1];
    let fl = sys::SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID;
    t.port_set(p1, fl, Value::Oid(pol)).unwrap();
    let v = vlan(&mut t, 300);
    let bc = sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE;
    t.set(VLAN, v, bc, e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_NONE)).unwrap();
    t.set(VLAN, v, bc, e(sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL)).unwrap();
    t.port_set(p1, fl, Value::Oid(RawOid::NULL)).unwrap();
    t.remove(POLICER, pol).unwrap();
    t.remove(VLAN, v).unwrap();
    let expected: Vec<String> = CPP_09_HASH.iter().map(|l| (*l).to_owned()).collect();
    assert_eq!(t.calls(), expected);
}
