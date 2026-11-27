//! t_port.cpp: ports: list, lanes, speed, admin, FEC, MTU, link events, counters.
use crate::support::*;

/// `oper()` of the C++: the port's oper status, -1 when the get fails.
fn oper(e: &Env, p: u64) -> i32 {
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_PORT,
        p,
        sys::SAI_PORT_ATTR_OPER_STATUS,
        Value::S32(0),
    );
    if st == SUCCESS { a.s32() } else { -1 }
}

pub fn test_ports(e: &mut Env) {
    e.ck.section("ports");
    // create_switch asked every port's link state: all known down, no IFCS read
    check_eq!(e, "t_port.cpp:21", e.m().link_reads(), 0);
    // ---- ports
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        e.s,
        sys::SAI_SWITCH_ATTR_PORT_LIST,
        Value::OidList(vec![0; 8]),
    );
    ok!(e, "t_port.cpp:28", st);
    check_eq!(e, "t_port.cpp:29", a.count(), 3u32);
    let plist = a.oids();
    e.p1 = plist[0];
    e.p2 = plist[1];
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_PORT,
        e.p1,
        sys::SAI_PORT_ATTR_HW_LANE_LIST,
        Value::U32List(vec![0; 8]),
    );
    ok!(e, "t_port.cpp:36", st);
    check_eq!(e, "t_port.cpp:37", a.count(), 8u32);
    let lanes = a.u32s();
    check_eq!(e, "t_port.cpp:38", lanes[0], 249u32);
    check_eq!(e, "t_port.cpp:39", lanes[7], 256u32);
    let (st, a) = e.get1(sys::SAI_OBJECT_TYPE_PORT, e.p1, sys::SAI_PORT_ATTR_SPEED, Value::U32(0));
    ok!(e, "t_port.cpp:41", st);
    check_eq!(e, "t_port.cpp:42", a.u32(), 400000u32);
    // no live speed change
    let st = e.set(
        sys::SAI_OBJECT_TYPE_PORT,
        e.p1,
        a_u32(sys::SAI_PORT_ATTR_SPEED, 100_000),
    );
    check!(e, "t_port.cpp:44", st != SUCCESS);
    let st = e.set(
        sys::SAI_OBJECT_TYPE_PORT,
        e.p1,
        a_u32(sys::SAI_PORT_ATTR_SPEED, 400_000),
    );
    ok!(e, "t_port.cpp:46", st);
    check_eq!(e, "t_port.cpp:47", e.m().admin(1), 0u32);
    let st = e.set(
        sys::SAI_OBJECT_TYPE_PORT,
        e.p1,
        a_bool(sys::SAI_PORT_ATTR_ADMIN_STATE, true),
    );
    ok!(e, "t_port.cpp:50", st);
    check_eq!(e, "t_port.cpp:51", e.m().admin(1), 1u32);
    let fec = |v: u32| a_s32(sys::SAI_PORT_ATTR_FEC_MODE, v as i32);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, e.p1, fec(sys::SAI_PORT_FEC_MODE_FC));
    ok!(e, "t_port.cpp:54", st);
    check_eq!(e, "t_port.cpp:55", e.m().fec(1), 3u32);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, e.p1, fec(sys::SAI_PORT_FEC_MODE_RS));
    ok!(e, "t_port.cpp:57", st);
    check_eq!(e, "t_port.cpp:58", e.m().fec(1), 2u32); // 50G lanes -> RS544 (KP)
    e.p3 = plist[2];
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, e.p3, fec(sys::SAI_PORT_FEC_MODE_FC));
    ok!(e, "t_port.cpp:61", st);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, e.p3, fec(sys::SAI_PORT_FEC_MODE_RS));
    ok!(e, "t_port.cpp:63", st);
    check_eq!(e, "t_port.cpp:64", e.m().fec(3), 1u32); // 10G NRZ -> RS528 (KR)
    // already RS: kept
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, e.p3, fec(sys::SAI_PORT_FEC_MODE_RS));
    ok!(e, "t_port.cpp:65", st);
    check_eq!(e, "t_port.cpp:66", e.m().fec(3), 1u32);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, e.p1, a_u32(sys::SAI_PORT_ATTR_MTU, 9100));
    ok!(e, "t_port.cpp:70", st);
    check!(
        e,
        "t_port.cpp:71",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=51 u32=9122")
    );

    // link event -> port state notification
    e.m().fire_link(1, 1);
    check!(e, "t_port.cpp:75", e.wait_for(|ev| !ev.port.is_empty()));
    let p1 = e.p1;
    let first = e.with_events(|ev| ev.port.first().copied());
    check!(
        e,
        "t_port.cpp:78",
        first.is_some_and(|(port, state)| port == p1 && state == sys::SAI_PORT_OPER_STATUS_UP as i32)
    );
    e.m().fire_link(77, 1); // not a front-panel devport: ignored

    // link state: admin-up made port 1 unknown, so one IFCS read, which the
    // event above already settled; events and admin-down need no read
    let (up, down) = (
        sys::SAI_PORT_OPER_STATUS_UP as i32,
        sys::SAI_PORT_OPER_STATUS_DOWN as i32,
    );
    check_eq!(e, "t_port.cpp:85", oper(e, e.p1), up);
    check_eq!(e, "t_port.cpp:86", oper(e, e.p1), up);
    check_eq!(e, "t_port.cpp:87", oper(e, e.p2), down); // admin-down since init
    check_eq!(e, "t_port.cpp:88", e.m().link_reads(), 0);
    e.m().fire_link(1, 0);
    check!(e, "t_port.cpp:90", e.wait_for(|ev| ev.port.len() >= 2));
    check_eq!(e, "t_port.cpp:91", oper(e, e.p1), down);
    e.m().fire_link(1, 1);
    check_eq!(e, "t_port.cpp:93", oper(e, e.p1), up);
    // admin-up: unknown, read once (mock link down)
    let st = e.set(
        sys::SAI_OBJECT_TYPE_PORT,
        e.p3,
        a_bool(sys::SAI_PORT_ATTR_ADMIN_STATE, true),
    );
    ok!(e, "t_port.cpp:96", st);
    check_eq!(e, "t_port.cpp:97", oper(e, e.p3), down);
    check_eq!(e, "t_port.cpp:98", oper(e, e.p3), down);
    check_eq!(e, "t_port.cpp:99", e.m().link_reads(), 1);
    let st = e.set(
        sys::SAI_OBJECT_TYPE_PORT,
        e.p3,
        a_bool(sys::SAI_PORT_ATTR_ADMIN_STATE, false),
    );
    ok!(e, "t_port.cpp:101", st);
    check_eq!(e, "t_port.cpp:102", oper(e, e.p3), down);
    check_eq!(e, "t_port.cpp:103", e.m().link_reads(), 1);

    // stats
    e.m().set_stat(1, 47, 123_456); // RX_BYTES_ALL
    e.m().set_stat(1, 3, 10); // RX unicast
    e.m().set_stat(1, 4, 5);
    e.m().set_stat(1, 5, 2);
    let ids = [
        sys::SAI_PORT_STAT_IF_IN_OCTETS,
        sys::SAI_PORT_STAT_IF_IN_UCAST_PKTS,
        sys::SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS,
    ];
    e.m().log_clear();
    let (st, vals) = e.sai.stats(sys::SAI_OBJECT_TYPE_PORT, e.p1, &ids);
    ok!(e, "t_port.cpp:113", st);
    // one stats call for the IFCS counters these SAI ids need (port_stats_by_id), not all 112
    check!(
        e,
        "t_port.cpp:115",
        has(&e.m().log(), "ifcs_devport_stats_get dp=1 count=")
    );
    check!(e, "t_port.cpp:116", !has(&e.m().log(), "count=112"));
    check_eq!(e, "t_port.cpp:117", lines(&e.m().log()).len(), 1usize);
    check_eq!(e, "t_port.cpp:118", vals[0], 123_456u64);
    check_eq!(e, "t_port.cpp:119", vals[1], 10u64);
    check_eq!(e, "t_port.cpp:120", vals[2], 7u64);
    let st = e.sai.clear_port_all_stats(e.p1);
    ok!(e, "t_port.cpp:121", st);
    check!(
        e,
        "t_port.cpp:122",
        has(&e.m().log(), "ifcs_devport_stats_clear dp=1 count=111")
    );
}
