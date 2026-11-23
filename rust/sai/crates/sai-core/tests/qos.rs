//! `QoS` on the virtual backend (agent A6): the checks of the C++ `tests/test_qos.inc` (`test_qos`,
//! the `QoS` part), failure injection (backend first, store second: a failed backend call leaves
//! the model and the chip as they were, and where one change re-programs several objects the old
//! state goes back), and the call log of driver script `08-qos.ops` against the C++ library's.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
mod a6_common;

use a6_common::*;
use sai_core::backend::StatsObject;
use sai_core::backend::types::QosMapKind;
use sai_core::error::{AttrError, Error};
use sai_core::oid::RawOid;
use sai_core::value::{QosMapEntry, QosParams, Value};
use sai_sys as sys;

const QOS_MAP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_QOS_MAP;
const SCHEDULER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SCHEDULER;
const GROUP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP;
const WRED: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_WRED;
const POOL: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_BUFFER_POOL;
const PROFILE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_BUFFER_PROFILE;
const QUEUE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_QUEUE;
const PG: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP;

fn dscp(d: u8, tc: u8) -> QosMapEntry {
    QosMapEntry {
        key: QosParams {
            dscp: d,
            ..QosParams::default()
        },
        value: QosParams {
            tc,
            ..QosParams::default()
        },
    }
}

/// The C++ test's `dscp_map`: DSCP d to TC d / 8, 46 to `tc46`.
fn dscp_map(tc46: u8) -> Vec<QosMapEntry> {
    (0..64).map(|d| dscp(d, if d == 46 { tc46 } else { d >> 3 })).collect()
}

fn mk_map(t: &mut T, sai_type: u32, list: Vec<QosMapEntry>) -> sai_core::Result<RawOid> {
    t.create(
        QOS_MAP,
        &[
            a(sys::SAI_QOS_MAP_ATTR_TYPE, e(sai_type)),
            a(sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST, Value::QosMapList(list)),
        ],
    )
}

fn mk_sched(t: &mut T, sai_type: u32, weight: u8, max_rate: u64) -> RawOid {
    let mut attrs = vec![
        a(sys::SAI_SCHEDULER_ATTR_SCHEDULING_TYPE, e(sai_type)),
        a(sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, Value::U8(weight)),
    ];
    if max_rate != 0 {
        attrs.push(a(sys::SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE, Value::U64(max_rate)));
        attrs.push(a(sys::SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_BURST_RATE, Value::U64(65536)));
    }
    t.create(SCHEDULER, &attrs).unwrap()
}

fn mk_pool(t: &mut T, sai_type: u32, size: u64) -> RawOid {
    t.create(
        POOL,
        &[
            a(sys::SAI_BUFFER_POOL_ATTR_TYPE, e(sai_type)),
            a(sys::SAI_BUFFER_POOL_ATTR_SIZE, Value::U64(size)),
            a(
                sys::SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE,
                e(sys::SAI_BUFFER_POOL_THRESHOLD_MODE_DYNAMIC),
            ),
        ],
    )
    .unwrap()
}

fn mk_profile(t: &mut T, pool: RawOid, reserved: u64, th: i8, xoff: u64) -> RawOid {
    let mut attrs = vec![
        a(sys::SAI_BUFFER_PROFILE_ATTR_POOL_ID, Value::Oid(pool)),
        a(sys::SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE, Value::U64(reserved)),
        a(sys::SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH, Value::I8(th)),
    ];
    if xoff != 0 {
        attrs.push(a(sys::SAI_BUFFER_PROFILE_ATTR_XOFF_TH, Value::U64(xoff)));
    }
    t.create(PROFILE, &attrs).unwrap()
}

fn map_hw(t: &T, map: RawOid) -> Option<u32> {
    let store = t.sai.store().unwrap();
    let id = sai_core::oid::QosMapId::try_from(map).ok()?;
    store
        .get::<sai_core::qos::QosMap>(id)
        .ok()?
        .hw()
        .map(sai_core::backend::types::QosMapId::get)
}

fn wred_hw(t: &T, wred: RawOid) -> Option<u32> {
    let store = t.sai.store().unwrap();
    let id = sai_core::oid::WredId::try_from(wred).ok()?;
    store
        .get::<sai_core::qos::Wred>(id)
        .ok()?
        .hw()
        .map(sai_core::backend::types::WredId::get)
}

fn port_map(t: &T, hw: sai_core::backend::HwPort, kind: QosMapKind) -> Option<u32> {
    t.virt().qos().port_qmap.get(&(hw, kind)).map(|id| id.get())
}

fn invalid() -> Error {
    attr(AttrError::InvalidValue)
}

#[test]
fn scheduler_groups_are_shown_only_with_qos_one_per_queue() {
    let mut t = T::new();
    let p1 = t.ports[1];
    let queues = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    assert_eq!(queues.len(), 8);
    assert_eq!(
        t.port_list(p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST),
        Vec::<RawOid>::new()
    );
    assert_eq!(
        t.get(PORT, p1, sys::SAI_PORT_ATTR_QOS_NUMBER_OF_SCHEDULER_GROUPS),
        Ok(Value::U32(0))
    );
    // without QoS a queue's parent is its port (as before groups existed)
    assert_eq!(
        t.get(QUEUE, queues[0], sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE),
        Ok(Value::Oid(p1))
    );
    t.qos(true);
    let groups = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);
    assert_eq!(
        t.get(PORT, p1, sys::SAI_PORT_ATTR_QOS_NUMBER_OF_SCHEDULER_GROUPS),
        Ok(Value::U32(8))
    );
    assert_eq!(groups.len(), 8);
    for (g, q) in groups.iter().zip(&queues) {
        let got = t
            .sai
            .get(
                GROUP,
                *g,
                &[
                    sys::SAI_SCHEDULER_GROUP_ATTR_CHILD_COUNT,
                    sys::SAI_SCHEDULER_GROUP_ATTR_CHILD_LIST,
                    sys::SAI_SCHEDULER_GROUP_ATTR_PORT_ID,
                    sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE,
                    sys::SAI_SCHEDULER_GROUP_ATTR_LEVEL,
                    sys::SAI_SCHEDULER_GROUP_ATTR_MAX_CHILDS,
                    sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID,
                ],
            )
            .unwrap();
        assert_eq!(
            got,
            vec![
                Value::U32(1),
                Value::OidList(vec![*q]),
                Value::Oid(p1),
                Value::Oid(p1),
                Value::U8(0),
                Value::U8(1),
                Value::Oid(RawOid::NULL)
            ]
        );
        assert_eq!(
            t.get(QUEUE, *q, sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE),
            Ok(Value::Oid(*g))
        );
    }
    assert_eq!(t.create(GROUP, &[]), Err(Error::NotSupported));
    assert_eq!(t.remove(GROUP, groups[0]), Err(Error::NotSupported));
    // the parent can be "set" only to what it is
    assert_eq!(
        t.set(
            GROUP,
            groups[0],
            sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE,
            Value::Oid(p1)
        ),
        Ok(())
    );
    assert_eq!(
        t.set(
            GROUP,
            groups[0],
            sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE,
            Value::Oid(t.ports[2])
        ),
        Err(attr(AttrError::NotSupported))
    );
    // the CPU port has none
    let cpu = oid(t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap());
    assert_eq!(
        t.get(PORT, cpu, sys::SAI_PORT_ATTR_QOS_NUMBER_OF_SCHEDULER_GROUPS),
        Ok(Value::U32(0))
    );
    assert_eq!(t.calls(), Vec::<String>::new());
}

#[test]
fn without_qos_in_the_backend_objects_are_kept_and_nothing_is_programmed() {
    let mut t = T::new();
    let p1 = t.ports[1];
    let m0 = mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(5)).unwrap();
    assert_eq!(map_hw(&t, m0), None);
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(m0))
        .unwrap();
    assert_eq!(
        t.get(PORT, p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP),
        Ok(Value::Oid(m0))
    );
    assert!(t.virt().qos().port_qmap.is_empty() && t.virt().qos().qmaps.is_empty());
    let s0 = mk_sched(&mut t, sys::SAI_SCHEDULING_TYPE_DWRR, 20, 0);
    t.qos(true);
    let groups = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);
    t.qos(false);
    t.set(
        GROUP,
        groups[3],
        sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID,
        Value::Oid(s0),
    )
    .unwrap();
    assert!(t.virt().qos().queue_sched.is_empty());
    let queues = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    let ids = [
        sys::SAI_QUEUE_STAT_PACKETS,
        sys::SAI_QUEUE_STAT_BYTES,
        sys::SAI_QUEUE_STAT_DROPPED_PACKETS,
    ];
    assert_eq!(t.sai.stats_get(QUEUE, queues[3], &ids, false), Err(Error::NotSupported));
    assert_eq!(t.sai.stats_capability(QUEUE), Err(Error::NotSupported));
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DEFAULT_TC, Value::U8(3)).unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, Value::U8(0x18))
        .unwrap();
    assert_eq!(t.get(PORT, p1, sys::SAI_PORT_ATTR_QOS_DEFAULT_TC), Ok(Value::U8(3)));
    let w = t
        .create(WRED, &[a(sys::SAI_WRED_ATTR_GREEN_ENABLE, Value::Bool(true))])
        .unwrap();
    assert_eq!(wred_hw(&t, w), None);
    assert_eq!(t.calls(), Vec::<String>::new(), "nothing reaches the chip");
    // in use, though not programmed
    assert_eq!(t.remove(QOS_MAP, m0), Err(Error::ObjectInUse));
    assert_eq!(t.remove(SCHEDULER, s0), Err(Error::ObjectInUse));
    t.set(
        GROUP,
        groups[3],
        sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID,
        Value::Oid(RawOid::NULL),
    )
    .unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(RawOid::NULL))
        .unwrap();
    t.remove(QOS_MAP, m0).unwrap();
    t.remove(SCHEDULER, s0).unwrap();
    t.remove(WRED, w).unwrap();
    assert_eq!(t.calls(), Vec::<String>::new());
    t.store_ok();
}

#[test]
fn maps_are_complete_tables_bound_to_ports_and_the_switch() {
    let mut t = T::new();
    t.qos(true);
    let (p1, p2) = (t.ports[1], t.ports[2]);
    let (hw1, hw2) = (t.hw(p1), t.hw(p2));
    let m1 = mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(5)).unwrap();
    let id1 = map_hw(&t, m1).unwrap();
    assert!(id1 > 0);
    {
        let (kind, entries) = &t.virt().qos().qmaps[&sai_core::backend::types::QosMapId(id1)];
        assert_eq!(*kind, QosMapKind::DscpToTc);
        assert_eq!(entries.len(), 64);
        assert!(entries[46].key == 46 && entries[46].value == 5 && entries[8].value == 1);
    }
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(m1))
        .unwrap();
    assert_eq!(port_map(&t, hw1, QosMapKind::DscpToTc), Some(id1));
    // a partial list: every other key 0
    let partial = vec![QosMapEntry {
        key: QosParams {
            dot1p: 5,
            ..QosParams::default()
        },
        value: QosParams {
            tc: 3,
            ..QosParams::default()
        },
    }];
    let md = mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DOT1P_TO_TC, partial).unwrap();
    let idd = map_hw(&t, md).unwrap();
    {
        let (_, entries) = &t.virt().qos().qmaps[&sai_core::backend::types::QosMapId(idd)];
        assert!(entries.len() == 8 && entries[5].value == 3 && entries[4].value == 0);
    }
    // a map of another type
    assert_eq!(
        t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(md)),
        Err(invalid())
    );
    // the switch's map: ports without their own get it
    let m2 = mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(6)).unwrap();
    t.set(
        SWITCH,
        t.switch,
        sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP,
        Value::Oid(m2),
    )
    .unwrap();
    assert_eq!(port_map(&t, hw2, QosMapKind::DscpToTc), map_hw(&t, m2));
    assert_eq!(port_map(&t, hw1, QosMapKind::DscpToTc), Some(id1), "its own wins");
    assert_eq!(
        t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP),
        Ok(Value::Oid(m2))
    );
    assert_eq!(t.remove(QOS_MAP, m2), Err(Error::ObjectInUse));
    t.set(
        SWITCH,
        t.switch,
        sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP,
        Value::Oid(RawOid::NULL),
    )
    .unwrap();
    assert_eq!(port_map(&t, hw2, QosMapKind::DscpToTc), None);
    t.remove(QOS_MAP, m2).unwrap();
    // update in place; in use
    t.set(
        QOS_MAP,
        m1,
        sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST,
        Value::QosMapList(dscp_map(7)),
    )
    .unwrap();
    assert_eq!(
        t.virt().qos().qmaps[&sai_core::backend::types::QosMapId(id1)].1[46].value,
        7
    );
    assert_eq!(t.remove(QOS_MAP, m1), Err(Error::ObjectInUse));
    let mut bad = dscp_map(5);
    bad[3].key.dscp = 64;
    assert_eq!(
        mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC, bad),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    // TC and colour to DSCP: 8 x 3 entries
    let tcc = vec![QosMapEntry {
        key: QosParams {
            tc: 2,
            color: sys::SAI_PACKET_COLOR_YELLOW.cast_signed(),
            ..QosParams::default()
        },
        value: QosParams {
            dscp: 12,
            ..QosParams::default()
        },
    }];
    let mr = mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP, tcc).unwrap();
    {
        let (_, entries) = &t.virt().qos().qmaps[&sai_core::backend::types::QosMapId(map_hw(&t, mr).unwrap())];
        assert!(entries.len() == 24 && entries[7].value == 12 && entries[7].key_color == 1);
    }
    let got = t
        .sai
        .get(
            QOS_MAP,
            m1,
            &[sys::SAI_QOS_MAP_ATTR_TYPE, sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST],
        )
        .unwrap();
    assert_eq!(got[0], e(sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC));
    assert_eq!(got[1], Value::QosMapList(dscp_map(7)));
    t.remove(QOS_MAP, mr).unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP, Value::Oid(md))
        .unwrap();
    assert_eq!(port_map(&t, hw1, QosMapKind::Dot1pToTc), Some(idd));
    // a type no backend has a table for
    assert_eq!(
        mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DSCP_TO_FORWARDING_CLASS, vec![]),
        Err(Error::NotSupported)
    );
    // unbind, remove
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(RawOid::NULL))
        .unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP, Value::Oid(RawOid::NULL))
        .unwrap();
    assert!(t.virt().qos().port_qmap.is_empty());
    t.remove(QOS_MAP, m1).unwrap();
    t.remove(QOS_MAP, md).unwrap();
    assert!(t.virt().qos().qmaps.is_empty());
    t.store_ok();
}

#[test]
fn default_tc_and_pfc_combined_and_separate() {
    let mut t = T::new();
    t.qos(true);
    let p1 = t.ports[1];
    let hw1 = t.hw(p1);
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DEFAULT_TC, Value::U8(3)).unwrap();
    assert_eq!(t.virt().qos().default_tc.get(&hw1), Some(&3));
    assert_eq!(
        t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DEFAULT_TC, Value::U8(16)),
        Err(invalid())
    );
    t.port_set(p1, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, Value::U8(0x18))
        .unwrap();
    assert_eq!(t.virt().qos().pfc.get(&hw1), Some(&(false, 0x18, 0x18)));
    t.port_set(
        p1,
        sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE,
        e(sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE),
    )
    .unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX, Value::U8(0xff))
        .unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX, Value::U8(0x08))
        .unwrap();
    assert_eq!(t.virt().qos().pfc.get(&hw1), Some(&(true, 0x08, 0xff)));
    assert_eq!(
        t.get(PORT, p1, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL),
        Ok(Value::U8(0x18))
    );
    assert_eq!(
        t.get(PORT, p1, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE),
        Ok(e(sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE))
    );
    // back to the defaults
    t.port_set(
        p1,
        sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE,
        e(sys::SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_COMBINED),
    )
    .unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, Value::U8(0))
        .unwrap();
    assert_eq!(t.virt().qos().pfc.get(&hw1), Some(&(false, 0, 0)));
    // PFC counters: a port counter read through the QoS call
    t.virt_mut().set_pfc_stat(hw1, 3, false, 17);
    assert_eq!(
        t.sai.stats_get(PORT, p1, &[sys::SAI_PORT_STAT_PFC_3_RX_PKTS], false),
        Ok(vec![17])
    );
}

#[test]
fn schedulers_on_groups_queues_and_the_port_shaper() {
    let mut t = T::new();
    t.qos(true);
    let p1 = t.ports[1];
    let hw1 = t.hw(p1);
    let groups = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);
    let queues = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    let qsch = |t: &T, q: u8| t.virt().qos().queue_sched.get(&(hw1, q)).copied();
    let sd = mk_sched(&mut t, sys::SAI_SCHEDULING_TYPE_DWRR, 20, 0);
    let ssp = mk_sched(&mut t, sys::SAI_SCHEDULING_TYPE_STRICT, 1, 0);
    let sh = mk_sched(&mut t, sys::SAI_SCHEDULING_TYPE_DWRR, 1, 125_000_000);
    let sp = sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    t.set(GROUP, groups[3], sp, Value::Oid(sd)).unwrap();
    assert!(qsch(&t, 3).is_some_and(|s| s.weight == 20 && !s.strict));
    // not a scheduler
    assert_eq!(t.set(GROUP, groups[4], sp, Value::Oid(RawOid(0x123))), Err(invalid()));
    t.set(GROUP, groups[4], sp, Value::Oid(sd)).unwrap();
    t.set(SCHEDULER, sd, sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, Value::U8(50))
        .unwrap();
    assert!(qsch(&t, 3).is_some_and(|s| s.weight == 50) && qsch(&t, 4).is_some_and(|s| s.weight == 50));
    assert_eq!(
        t.set(SCHEDULER, sd, sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, Value::U8(0)),
        Err(invalid())
    );
    assert_eq!(
        t.get(SCHEDULER, sd, sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT),
        Ok(Value::U8(50))
    );
    let qp = sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID;
    t.set(QUEUE, queues[5], qp, Value::Oid(ssp)).unwrap();
    assert!(qsch(&t, 5).is_some_and(|s| s.strict));
    t.set(QUEUE, queues[3], qp, Value::Oid(ssp)).unwrap();
    assert!(qsch(&t, 3).is_some_and(|s| !s.strict), "the group's profile wins");
    assert_eq!(t.get(QUEUE, queues[3], qp), Ok(Value::Oid(ssp)));
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID, Value::Oid(sh))
        .unwrap();
    let shaper = t.virt().qos().port_sched.get(&hw1).copied().unwrap();
    assert!(shaper.max_rate == 125_000_000 && shaper.max_burst == 65536);
    assert_eq!(t.remove(SCHEDULER, sd), Err(Error::ObjectInUse));
    t.set(GROUP, groups[3], sp, Value::Oid(RawOid::NULL)).unwrap();
    assert!(qsch(&t, 3).is_some_and(|s| s.strict), "falls back to the queue's own");
    t.set(GROUP, groups[4], sp, Value::Oid(RawOid::NULL)).unwrap();
    assert_eq!(qsch(&t, 4), None, "default");
    t.set(QUEUE, queues[3], qp, Value::Oid(RawOid::NULL)).unwrap();
    t.set(QUEUE, queues[5], qp, Value::Oid(RawOid::NULL)).unwrap();
    t.port_set(
        p1,
        sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID,
        Value::Oid(RawOid::NULL),
    )
    .unwrap();
    assert!(t.virt().qos().queue_sched.is_empty() && t.virt().qos().port_sched.is_empty());
    t.remove(SCHEDULER, sd).unwrap();
    t.remove(SCHEDULER, ssp).unwrap();
    t.remove(SCHEDULER, sh).unwrap();
    // the C++ defaults (DWRR, not the header's WRR) and attributes it does not know
    let s = t.create(SCHEDULER, &[]).unwrap();
    assert_eq!(
        t.get(SCHEDULER, s, sys::SAI_SCHEDULER_ATTR_SCHEDULING_TYPE),
        Ok(e(sys::SAI_SCHEDULING_TYPE_DWRR))
    );
    assert_eq!(
        t.get(SCHEDULER, s, sys::SAI_SCHEDULER_ATTR_METER_TYPE),
        Ok(e(sys::SAI_METER_TYPE_BYTES))
    );
    assert_eq!(
        t.create(
            SCHEDULER,
            &[
                a(sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, Value::U8(2)),
                a(sys::SAI_SCHEDULER_ATTR_LABEL, Value::CharData("x".to_owned()))
            ]
        ),
        Err(Error::attr(AttrError::Unknown, 1))
    );
    t.store_ok();
}

#[test]
fn wred_profiles_on_queues() {
    let mut t = T::new();
    t.qos(true);
    let p1 = t.ports[1];
    let hw1 = t.hw(p1);
    let queues = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    let attrs = [
        a(sys::SAI_WRED_ATTR_GREEN_ENABLE, Value::Bool(true)),
        a(sys::SAI_WRED_ATTR_GREEN_MIN_THRESHOLD, Value::U32(100_000)),
        a(sys::SAI_WRED_ATTR_GREEN_MAX_THRESHOLD, Value::U32(2_000_000)),
        a(sys::SAI_WRED_ATTR_GREEN_DROP_PROBABILITY, Value::U32(5)),
        a(sys::SAI_WRED_ATTR_ECN_MARK_MODE, e(sys::SAI_ECN_MARK_MODE_GREEN)),
    ];
    let wr = t.create(WRED, &attrs).unwrap();
    let wid = wred_hw(&t, wr).unwrap();
    let wred = |t: &T| {
        t.virt()
            .qos()
            .wreds
            .get(&sai_core::backend::types::WredId(wid))
            .copied()
    };
    let w = wred(&t).unwrap();
    assert!(
        w.enable[0]
            && w.max[0] == 2_000_000
            && w.ecn == sys::SAI_ECN_MARK_MODE_GREEN.cast_signed()
            && !w.enable[1]
            && w.prob == [5, 100, 100]
    );
    t.set(QUEUE, queues[3], sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID, Value::Oid(wr))
        .unwrap();
    assert_eq!(t.virt().qos().queue_wred.get(&(hw1, 3)).map(|i| i.get()), Some(wid));
    // above the maximum
    assert_eq!(
        t.set(WRED, wr, sys::SAI_WRED_ATTR_GREEN_MIN_THRESHOLD, Value::U32(3_000_000)),
        Err(invalid())
    );
    t.set(WRED, wr, sys::SAI_WRED_ATTR_GREEN_MAX_THRESHOLD, Value::U32(3_000_000))
        .unwrap();
    assert_eq!(wred(&t).unwrap().max[0], 3_000_000);
    assert_eq!(
        t.get(WRED, wr, sys::SAI_WRED_ATTR_GREEN_MAX_THRESHOLD),
        Ok(Value::U32(3_000_000))
    );
    assert_eq!(t.remove(WRED, wr), Err(Error::ObjectInUse));
    assert_eq!(
        t.set(WRED, wr, sys::SAI_WRED_ATTR_GREEN_DROP_PROBABILITY, Value::U32(101)),
        Err(invalid())
    );
    assert_eq!(
        t.set(WRED, wr, sys::SAI_WRED_ATTR_ECN_GREEN_MIN_THRESHOLD, Value::U32(1)),
        Err(attr(AttrError::NotSupported))
    );
    t.set(
        QUEUE,
        queues[3],
        sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID,
        Value::Oid(RawOid::NULL),
    )
    .unwrap();
    assert!(t.virt().qos().queue_wred.is_empty());
    t.remove(WRED, wr).unwrap();
    assert!(wred(&t).is_none());
    // an enabled colour with min > max
    assert_eq!(
        t.create(
            WRED,
            &[
                a(sys::SAI_WRED_ATTR_RED_ENABLE, Value::Bool(true)),
                a(sys::SAI_WRED_ATTR_RED_MIN_THRESHOLD, Value::U32(2))
            ]
        ),
        Err(Error::InvalidParameter)
    );
}

#[test]
fn buffer_pools_and_profiles_on_queues_and_priority_groups() {
    let mut t = T::new();
    t.qos(true);
    let p1 = t.ports[1];
    let hw1 = t.hw(p1);
    let queues = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    let pgs = t.port_list(p1, sys::SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST);
    let pin = mk_pool(&mut t, sys::SAI_BUFFER_POOL_TYPE_INGRESS, 20_000_000);
    let peg = mk_pool(&mut t, sys::SAI_BUFFER_POOL_TYPE_EGRESS, 10_000_000);
    let bq = mk_profile(&mut t, peg, 1518, 3, 0);
    let bpg = mk_profile(&mut t, pin, 1248, 0, 100_000);
    let qb = sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID;
    assert_eq!(
        t.set(QUEUE, queues[0], qb, Value::Oid(bpg)),
        Err(invalid()),
        "an ingress pool's"
    );
    t.set(QUEUE, queues[0], qb, Value::Oid(bq)).unwrap();
    let qbuf = |t: &T| t.virt().qos().queue_buf.get(&(hw1, 0)).copied();
    let b = qbuf(&t).unwrap();
    assert!(!b.ingress && b.dynamic && b.dynamic_th == 3 && b.reserved == 1518 && b.pool_size == 10_000_000);
    let pb = sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE;
    t.set(PG, pgs[3], pb, Value::Oid(bpg)).unwrap();
    let g = t.virt().qos().pg_buf.get(&(hw1, 3)).copied().unwrap();
    assert!(g.xoff == 100_000 && g.ingress);
    assert_eq!(
        t.set(PG, pgs[3], pb, Value::Oid(bq)),
        Err(invalid()),
        "an egress pool's"
    );
    t.set(
        PROFILE,
        bq,
        sys::SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH,
        Value::I8(1),
    )
    .unwrap();
    assert_eq!(qbuf(&t).unwrap().dynamic_th, 1);
    t.set(POOL, peg, sys::SAI_BUFFER_POOL_ATTR_SIZE, Value::U64(12_000_000))
        .unwrap();
    assert_eq!(qbuf(&t).unwrap().pool_size, 12_000_000);
    assert_eq!(
        t.get(POOL, peg, sys::SAI_BUFFER_POOL_ATTR_SHARED_SIZE),
        Ok(Value::U64(12_000_000))
    );
    assert_eq!(
        t.get(PROFILE, bq, sys::SAI_BUFFER_PROFILE_ATTR_THRESHOLD_MODE),
        Ok(e(sys::SAI_BUFFER_PROFILE_THRESHOLD_MODE_DYNAMIC)),
        "the pool's"
    );
    // create only: the C++ status (INVALID_ATTR_VALUE_0), from the core as from the rim
    assert_eq!(
        t.set(PROFILE, bq, sys::SAI_BUFFER_PROFILE_ATTR_POOL_ID, Value::Oid(pin)),
        Err(invalid())
    );
    assert_eq!(
        t.sai.refused_set(PROFILE, bq, sys::SAI_BUFFER_PROFILE_ATTR_POOL_ID),
        Some(invalid())
    );
    assert_eq!(
        t.sai.refused_set(POOL, peg, sys::SAI_BUFFER_POOL_ATTR_SHARED_SIZE),
        Some(attr(AttrError::NotSupported))
    );
    assert_eq!(
        t.sai.refused_set(QOS_MAP, peg, sys::SAI_QOS_MAP_ATTR_TYPE),
        Some(Error::InvalidObjectId)
    );
    let il = sys::SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST;
    t.port_set(p1, il, Value::OidList(vec![bpg])).unwrap();
    assert_eq!(t.get(PORT, p1, il), Ok(Value::OidList(vec![bpg])));
    assert_eq!(
        t.port_set(p1, il, Value::OidList(vec![bq])),
        Err(invalid()),
        "an egress profile"
    );
    assert_eq!(t.remove(PROFILE, bpg), Err(Error::ObjectInUse));
    assert_eq!(t.remove(POOL, pin), Err(Error::ObjectInUse));
    t.port_set(p1, il, Value::OidList(vec![])).unwrap();
    t.set(PG, pgs[3], pb, Value::Oid(RawOid::NULL)).unwrap();
    t.set(QUEUE, queues[0], qb, Value::Oid(RawOid::NULL)).unwrap();
    assert!(t.virt().qos().queue_buf.is_empty() && t.virt().qos().pg_buf.is_empty());
    t.remove(PROFILE, bq).unwrap();
    t.remove(PROFILE, bpg).unwrap();
    t.remove(POOL, pin).unwrap();
    t.remove(POOL, peg).unwrap();
    // mandatory: the C++'s (TYPE and SIZE; POOL_ID and RESERVED_BUFFER_SIZE), not the header's
    assert_eq!(
        t.create(POOL, &[a(sys::SAI_BUFFER_POOL_ATTR_SIZE, Value::U64(1))]),
        Err(Error::MandatoryAttributeMissing)
    );
    t.store_ok();
}

#[test]
fn queue_and_priority_group_counters_read_and_clear_and_capability() {
    let mut t = T::new();
    t.qos(true);
    let p1 = t.ports[1];
    let hw1 = t.hw(p1);
    let queues = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    let pgs = t.port_list(p1, sys::SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST);
    let q3 = StatsObject::Queue { port: hw1, index: 3 };
    t.virt_mut().set_counter(q3, sys::SAI_QUEUE_STAT_PACKETS, 42);
    t.virt_mut().set_counter(q3, sys::SAI_QUEUE_STAT_BYTES, 4200);
    let ids = [
        sys::SAI_QUEUE_STAT_PACKETS,
        sys::SAI_QUEUE_STAT_BYTES,
        sys::SAI_QUEUE_STAT_DROPPED_PACKETS,
    ];
    assert_eq!(t.sai.stats_get(QUEUE, queues[3], &ids, false), Ok(vec![42, 4200, 0]));
    assert_eq!(t.sai.stats_get(QUEUE, queues[3], &ids[..2], true), Ok(vec![42, 4200]));
    assert_eq!(t.sai.stats_get(QUEUE, queues[3], &ids[..2], false), Ok(vec![0, 0]));
    assert_eq!(
        t.sai
            .stats_get(QUEUE, queues[3], &[sys::SAI_QUEUE_STAT_SHARED_WATERMARK_BYTES], false),
        Err(Error::NotSupported)
    );
    t.virt_mut().set_counter(
        StatsObject::PriorityGroup { port: hw1, index: 3 },
        sys::SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS,
        9,
    );
    assert_eq!(
        t.sai
            .stats_get(PG, pgs[3], &[sys::SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS], false),
        Ok(vec![9])
    );
    t.sai
        .stats_clear(PG, pgs[3], &[sys::SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS])
        .unwrap();
    assert_eq!(
        t.sai
            .stats_get(PG, pgs[3], &[sys::SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS], false),
        Ok(vec![0])
    );
    assert_eq!(
        t.sai.stats_capability(QUEUE),
        Ok([
            sys::SAI_QUEUE_STAT_PACKETS,
            sys::SAI_QUEUE_STAT_BYTES,
            sys::SAI_QUEUE_STAT_DROPPED_PACKETS
        ]
        .map(|id| (id, sys::SAI_STATS_MODE_READ | sys::SAI_STATS_MODE_READ_AND_CLEAR))
        .to_vec())
    );
    assert_eq!(t.sai.stats_capability(PG).map(|v| v.len()), Ok(2));
    // pools: no counters on this chip, read only, no clear
    let pool = mk_pool(&mut t, sys::SAI_BUFFER_POOL_TYPE_EGRESS, 1);
    let watermark = [sys::SAI_BUFFER_POOL_STAT_WATERMARK_BYTES];
    assert_eq!(t.sai.stats_get(POOL, pool, &watermark, false), Err(Error::NotSupported));
    assert_eq!(t.sai.stats_get(POOL, pool, &watermark, true), Err(Error::NotSupported));
    assert_eq!(t.sai.stats_clear(POOL, pool, &watermark), Err(Error::NotSupported));
    // an id of another object
    assert_eq!(t.sai.stats_get(QUEUE, pgs[0], &ids, false), Err(Error::InvalidObjectId));
}

// ------------------------------------------------------------------ failure injection

/// What the chip holds of `QoS`, to compare before and after a failed call.
fn chip(t: &T) -> String {
    format!("{:?}", t.virt().qos())
}

#[test]
fn a_failed_backend_call_leaves_model_and_chip_as_they_were() {
    let mut t = T::new();
    t.qos(true);
    let (p1, p2) = (t.ports[1], t.ports[2]);
    let queues = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    let groups = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);

    // a create the chip refuses stores nothing
    t.virt_mut().fail_nth("qos_map_create", 1, Error::TableFull).unwrap();
    let objects = t.sai.store().unwrap().len();
    assert_eq!(
        mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(5)),
        Err(Error::TableFull)
    );
    assert_eq!(t.sai.store().unwrap().len(), objects);
    t.virt_mut().fail_nth("wred_create", 1, Error::TableFull).unwrap();
    assert_eq!(t.create(WRED, &[]), Err(Error::TableFull));
    assert_eq!(t.sai.store().unwrap().len(), objects);

    // a port binding the chip refuses is not kept; INVALID_PARAMETER is the attribute's
    let m = mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(5)).unwrap();
    let before = chip(&t);
    t.virt_mut()
        .fail_nth("port_qos_map_set", 1, Error::InvalidParameter)
        .unwrap();
    assert_eq!(
        t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(m)),
        Err(invalid())
    );
    assert_eq!(
        t.get(PORT, p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP),
        Ok(Value::Oid(RawOid::NULL))
    );
    assert_eq!(chip(&t), before);
    t.remove(QOS_MAP, m).unwrap(); // not in use: the failed binding holds nothing

    // the switch map: the second port fails, the first gets the old (none) back
    let m = mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(5)).unwrap();
    let before = chip(&t);
    t.calls();
    t.virt_mut().fail_nth("port_qos_map_set", 2, Error::Failure).unwrap();
    assert_eq!(
        t.set(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(m)),
        Err(Error::Failure)
    );
    assert_eq!(chip(&t), before);
    assert_eq!(
        t.get(SWITCH, t.switch, sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP),
        Ok(Value::Oid(RawOid::NULL))
    );
    let calls = t.calls();
    assert_eq!(calls.len(), 2 + 8, "two tries, then the old map on all eight ports");
    assert!(calls[1].ends_with("-> FAILURE"), "{calls:?}");
    t.remove(QOS_MAP, m).unwrap();

    // a scheduler two queues use: the second fails, both get the old profile back
    let s = mk_sched(&mut t, sys::SAI_SCHEDULING_TYPE_DWRR, 20, 0);
    let sp = sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    t.set(GROUP, groups[1], sp, Value::Oid(s)).unwrap();
    t.set(GROUP, groups[2], sp, Value::Oid(s)).unwrap();
    let before = chip(&t);
    t.virt_mut().fail_nth("queue_scheduler_set", 2, Error::Failure).unwrap();
    assert_eq!(
        t.set(SCHEDULER, s, sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, Value::U8(70)),
        Err(Error::Failure)
    );
    assert_eq!(chip(&t), before);
    assert_eq!(
        t.get(SCHEDULER, s, sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT),
        Ok(Value::U8(20))
    );
    // a group whose queue refuses keeps its old profile
    t.virt_mut().fail_nth("queue_scheduler_set", 1, Error::Failure).unwrap();
    assert_eq!(
        t.set(GROUP, groups[1], sp, Value::Oid(RawOid::NULL)),
        Err(Error::Failure)
    );
    assert_eq!(t.get(GROUP, groups[1], sp), Ok(Value::Oid(s)));
    assert_eq!(chip(&t), before);

    // a pool change two queues see: the first fails, every one is re-programmed with the old
    let pool = mk_pool(&mut t, sys::SAI_BUFFER_POOL_TYPE_EGRESS, 1000);
    let prof = mk_profile(&mut t, pool, 10, 0, 0);
    let qb = sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID;
    t.set(QUEUE, queues[0], qb, Value::Oid(prof)).unwrap();
    t.set(QUEUE, queues[1], qb, Value::Oid(prof)).unwrap();
    let before = chip(&t);
    t.virt_mut().fail_nth("queue_buffer_set", 1, Error::Failure).unwrap();
    assert_eq!(
        t.set(POOL, pool, sys::SAI_BUFFER_POOL_ATTR_SIZE, Value::U64(5)),
        Err(Error::Failure)
    );
    assert_eq!(chip(&t), before);
    assert_eq!(t.get(POOL, pool, sys::SAI_BUFFER_POOL_ATTR_SIZE), Ok(Value::U64(1000)));
    t.virt_mut().fail_nth("queue_buffer_set", 2, Error::Failure).unwrap();
    assert_eq!(
        t.set(PROFILE, prof, sys::SAI_BUFFER_PROFILE_ATTR_XON_TH, Value::U64(5)),
        Err(Error::Failure)
    );
    assert_eq!(chip(&t), before);
    assert_eq!(
        t.get(PROFILE, prof, sys::SAI_BUFFER_PROFILE_ATTR_XON_TH),
        Ok(Value::U64(0))
    );

    // a destroy the chip refuses keeps the object
    let w = t.create(WRED, &[]).unwrap();
    t.virt_mut().fail_nth("wred_destroy", 1, Error::Failure).unwrap();
    assert_eq!(t.remove(WRED, w), Err(Error::Failure));
    assert!(t.get(WRED, w, sys::SAI_WRED_ATTR_WEIGHT).is_ok());
    t.remove(WRED, w).unwrap();

    // a read-and-clear whose clear fails is a failure
    t.virt_mut().fail_nth("queue_stats_clear", 1, Error::Failure).unwrap();
    assert_eq!(
        t.sai.stats_get(QUEUE, queues[0], &[sys::SAI_QUEUE_STAT_PACKETS], true),
        Err(Error::Failure)
    );
    let _ = p2;
    t.store_ok();
}

// ------------------------------------------------------------------ call log

/// The backend calls of `tests/driver-scripts/08-qos.ops` on the C++ library (`frontend_driver`,
/// recording backend), after the switch is up.
const CPP_08: &[&str] = &[
    "qos_map_create(kind=DscpToTc, entries=[0.0=0.0,1.0=0.0,2.0=0.0,3.0=0.0,4.0=0.0,5.0=0.0,6.0=0.0,7.0=0.0,8.0=1.0,9.0=0.0,10.0=0.0,11.0=0.0,12.0=0.0,13.0=0.0,14.0=0.0,15.0=0.0,16.0=0.0,17.0=0.0,18.0=0.0,19.0=0.0,20.0=0.0,21.0=0.0,22.0=0.0,23.0=0.0,24.0=0.0,25.0=0.0,26.0=0.0,27.0=0.0,28.0=0.0,29.0=0.0,30.0=0.0,31.0=0.0,32.0=0.0,33.0=0.0,34.0=0.0,35.0=0.0,36.0=0.0,37.0=0.0,38.0=0.0,39.0=0.0,40.0=0.0,41.0=0.0,42.0=0.0,43.0=0.0,44.0=0.0,45.0=0.0,46.0=5.0,47.0=0.0,48.0=0.0,49.0=0.0,50.0=0.0,51.0=0.0,52.0=0.0,53.0=0.0,54.0=0.0,55.0=0.0,56.0=0.0,57.0=0.0,58.0=0.0,59.0=0.0,60.0=0.0,61.0=0.0,62.0=0.0,63.0=0.0]) -> SUCCESS id=1",
    "port_qos_map_set(hw=5, kind=DscpToTc, id=1) -> SUCCESS",
    "qos_map_create(kind=Dot1pToTc, entries=[0.0=0.0,1.0=0.0,2.0=0.0,3.0=0.0,4.0=0.0,5.0=3.0,6.0=0.0,7.0=0.0]) -> SUCCESS id=2",
    "qos_map_update(id=1, entries=[0.0=0.0,1.0=0.0,2.0=0.0,3.0=0.0,4.0=0.0,5.0=0.0,6.0=0.0,7.0=0.0,8.0=0.0,9.0=0.0,10.0=0.0,11.0=0.0,12.0=0.0,13.0=0.0,14.0=0.0,15.0=0.0,16.0=0.0,17.0=0.0,18.0=0.0,19.0=0.0,20.0=0.0,21.0=0.0,22.0=0.0,23.0=0.0,24.0=0.0,25.0=0.0,26.0=0.0,27.0=0.0,28.0=0.0,29.0=0.0,30.0=0.0,31.0=0.0,32.0=0.0,33.0=0.0,34.0=0.0,35.0=0.0,36.0=0.0,37.0=0.0,38.0=0.0,39.0=0.0,40.0=0.0,41.0=0.0,42.0=0.0,43.0=0.0,44.0=0.0,45.0=0.0,46.0=7.0,47.0=0.0,48.0=0.0,49.0=0.0,50.0=0.0,51.0=0.0,52.0=0.0,53.0=0.0,54.0=0.0,55.0=0.0,56.0=0.0,57.0=0.0,58.0=0.0,59.0=0.0,60.0=0.0,61.0=0.0,62.0=0.0,63.0=0.0]) -> SUCCESS",
    "port_default_tc_set(hw=5, tc=3) -> SUCCESS",
    "port_pfc_set(hw=5, separate=false, tx=24, rx=24) -> SUCCESS",
    "queue_scheduler_set(hw=5, queue=3, cfg={strict=false,weight=20,packets=false,min_rate=0,min_burst=0,max_rate=0,max_burst=0}) -> SUCCESS",
    "queue_scheduler_set(hw=5, queue=3, cfg={strict=false,weight=50,packets=false,min_rate=0,min_burst=0,max_rate=0,max_burst=0}) -> SUCCESS",
    "port_scheduler_set(hw=5, cfg={strict=false,weight=1,packets=false,min_rate=0,min_burst=0,max_rate=125000000,max_burst=65536}) -> SUCCESS",
    "wred_create(cfg={enable=[true,false,false],min=[100000,0,0],max=[2000000,0,0],prob=[5,100,100],weight=0,ecn=1}) -> SUCCESS id=3",
    "queue_wred_set(hw=5, queue=3, id=3) -> SUCCESS",
    "queue_buffer_set(hw=5, queue=0, cfg={ingress=false,dynamic=true,dynamic_th=3,static_th=0,reserved=1518,xoff=0,xon=0,xon_offset=0,pool_size=10000000,pool_xoff=0}) -> SUCCESS",
    "queue_stats_get(hw=5, queue=3, ids=[0,1]) -> SUCCESS values=[0,0]",
    "queue_buffer_set(hw=5, queue=0, cfg=none) -> SUCCESS",
    "queue_wred_set(hw=5, queue=3, id=-1) -> SUCCESS",
    "port_scheduler_set(hw=5, cfg=none) -> SUCCESS",
    "queue_scheduler_set(hw=5, queue=3, cfg=none) -> SUCCESS",
    "port_qos_map_set(hw=5, kind=DscpToTc, id=-1) -> SUCCESS",
    "wred_destroy(id=3) -> SUCCESS",
    "qos_map_destroy(id=1) -> SUCCESS",
    "qos_map_destroy(id=2) -> SUCCESS",
    "shutdown() -> void",
];

/// `08-qos.ops` step by step: the same calls, line for line, as the C++ library makes.
#[test]
fn the_qos_script_makes_the_cpp_librarys_backend_calls() {
    let mut t = T::new();
    let p1 = t.ports[1];
    let dscp_to_tc = sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC;
    let m0 = mk_map(&mut t, dscp_to_tc, vec![dscp(46, 5)]).unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(m0))
        .unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(RawOid::NULL))
        .unwrap();
    t.remove(QOS_MAP, m0).unwrap();
    t.qos(true);
    let groups = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);
    let queues = t.port_list(p1, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST);
    let m1 = mk_map(&mut t, dscp_to_tc, vec![dscp(46, 5), dscp(8, 1)]).unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(m1))
        .unwrap();
    let dot1p = vec![QosMapEntry {
        key: QosParams {
            dot1p: 5,
            ..QosParams::default()
        },
        value: QosParams {
            tc: 3,
            ..QosParams::default()
        },
    }];
    let md = mk_map(&mut t, sys::SAI_QOS_MAP_TYPE_DOT1P_TO_TC, dot1p.clone()).unwrap();
    assert_eq!(
        t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(md)),
        Err(invalid())
    );
    t.set(
        QOS_MAP,
        m1,
        sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST,
        Value::QosMapList(vec![dscp(46, 7)]),
    )
    .unwrap();
    assert_eq!(
        t.get(QOS_MAP, md, sys::SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST),
        Ok(Value::QosMapList(dot1p))
    );
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DEFAULT_TC, Value::U8(3)).unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, Value::U8(0x18))
        .unwrap();
    let sd = mk_sched(&mut t, sys::SAI_SCHEDULING_TYPE_DWRR, 20, 0);
    let sh = mk_sched(&mut t, sys::SAI_SCHEDULING_TYPE_DWRR, 1, 125_000_000);
    let sp = sys::SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    t.set(GROUP, groups[3], sp, Value::Oid(sd)).unwrap();
    t.set(SCHEDULER, sd, sys::SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT, Value::U8(50))
        .unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID, Value::Oid(sh))
        .unwrap();
    let w = t
        .create(
            WRED,
            &[
                a(sys::SAI_WRED_ATTR_GREEN_ENABLE, Value::Bool(true)),
                a(sys::SAI_WRED_ATTR_GREEN_MIN_THRESHOLD, Value::U32(100_000)),
                a(sys::SAI_WRED_ATTR_GREEN_MAX_THRESHOLD, Value::U32(2_000_000)),
                a(sys::SAI_WRED_ATTR_GREEN_DROP_PROBABILITY, Value::U32(5)),
                a(sys::SAI_WRED_ATTR_ECN_MARK_MODE, e(sys::SAI_ECN_MARK_MODE_GREEN)),
            ],
        )
        .unwrap();
    t.set(QUEUE, queues[3], sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID, Value::Oid(w))
        .unwrap();
    let pin = mk_pool(&mut t, sys::SAI_BUFFER_POOL_TYPE_INGRESS, 20_000_000);
    let peg = mk_pool(&mut t, sys::SAI_BUFFER_POOL_TYPE_EGRESS, 10_000_000);
    let bq = mk_profile(&mut t, peg, 1518, 3, 0);
    t.set(QUEUE, queues[0], sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID, Value::Oid(bq))
        .unwrap();
    assert_eq!(
        t.sai.stats_get(
            QUEUE,
            queues[3],
            &[sys::SAI_QUEUE_STAT_PACKETS, sys::SAI_QUEUE_STAT_BYTES],
            false
        ),
        Ok(vec![0, 0])
    );
    t.set(
        QUEUE,
        queues[0],
        sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID,
        Value::Oid(RawOid::NULL),
    )
    .unwrap();
    t.set(
        QUEUE,
        queues[3],
        sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID,
        Value::Oid(RawOid::NULL),
    )
    .unwrap();
    t.port_set(
        p1,
        sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID,
        Value::Oid(RawOid::NULL),
    )
    .unwrap();
    t.set(GROUP, groups[3], sp, Value::Oid(RawOid::NULL)).unwrap();
    t.port_set(p1, sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, Value::Oid(RawOid::NULL))
        .unwrap();
    for (ty, id) in [
        (PROFILE, bq),
        (POOL, pin),
        (POOL, peg),
        (WRED, w),
        (SCHEDULER, sd),
        (SCHEDULER, sh),
        (QOS_MAP, m1),
        (QOS_MAP, md),
    ] {
        t.remove(ty, id).unwrap();
    }
    t.qos(false);
    t.sai.remove(SWITCH, t.switch).unwrap();
    let mut expected: Vec<String> = CPP_08.iter().map(|l| (*l).to_owned()).collect();
    expected.retain(|l| !l.starts_with("shutdown()"));
    let mut got = t.calls();
    got.retain(|l| !l.starts_with("shutdown()"));
    assert_eq!(got, expected);
}
