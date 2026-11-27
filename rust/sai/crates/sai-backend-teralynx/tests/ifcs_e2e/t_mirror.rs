//! t_mirror.cpp: mirror sessions and sampling on the mock libifcs: SPAN (DCDP on the monitor port,
//! collector + set per direction on first use, sysport MIRROR_* policy), ERSPAN collector
//! attributes, session updates inside the same set, one session per port, SAMPLEPACKET (the CPU
//! DCDP, one sFlow collector / set, SFLOW_INGRESS), egress sampling refused.
use std::net::{IpAddr, Ipv4Addr};

use crate::support::*;

pub fn test_mirror(e: &mut Env) {
    e.ck.section("mirror");
    let (p1, p2, p3) = (e.p1, e.p2, e.p3);
    ok!(e, "t_mirror.cpp:27", e.sai.query(Api::Mirror));
    ok!(e, "t_mirror.cpp:28", e.sai.query(Api::Samplepacket));

    // ---- SPAN to p2: a DCDP on p2's sysport at create, nothing else yet
    e.m().log_clear();
    let ms = [
        a_s32(
            sys::SAI_MIRROR_SESSION_ATTR_TYPE,
            sys::SAI_MIRROR_SESSION_TYPE_LOCAL as i32,
        ),
        a_oid(sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, p2),
    ];
    let (st, span) = e.create(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, &ms);
    ok!(e, "t_mirror.cpp:37", st);
    check!(
        e,
        "t_mirror.cpp:38",
        has(&e.m().log(), "ifcs_dup_copy_dest_port_create h=") && has(&e.m().log(), "sysport=0x3e200002")
    );
    check!(
        e,
        "t_mirror.cpp:39",
        e.m().mirror_dcdps() == 1 && e.m().mirror_collectors() == 0 && e.m().mirror_sets() == 0
    );
    // ingress on p1: collector (SPAN, ingress, dcdp, truncate) + set + member, sysport policy enable
    e.m().log_clear();
    let a = a_oids(sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, &[span]);
    ok!(e, "t_mirror.cpp:45", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a));
    let mut log = e.m().log();
    check!(
        e,
        "t_mirror.cpp:47",
        has(&log, "ifcs_collector_create") && has(&log, " n=4 0=0x0 1=0x0 2=0x") && has(&log, " 18=0x0")
    );
    check!(
        e,
        "t_mirror.cpp:48",
        has(&log, "ifcs_collector_set_create")
            && has(&log, "type=0 dir=0")
            && has(&log, "ifcs_collector_set_member_add")
    );
    check!(
        e,
        "t_mirror.cpp:49",
        has(&log, "ifcs_sysport_attr_set sp=0x3e200001 id=22 u32=0")
    ); // MIRROR_INGRESS, action ENABLE
    check!(
        e,
        "t_mirror.cpp:50",
        e.m().mirror_collectors() == 1 && e.m().mirror_sets() == 1 && e.m().mirror_members() == 1
    );
    // egress on p1: its own collector (no truncate attr) and set
    e.m().log_clear();
    let a = a_oids(sys::SAI_PORT_ATTR_EGRESS_MIRROR_SESSION, &[span]);
    ok!(e, "t_mirror.cpp:54", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a));
    check!(
        e,
        "t_mirror.cpp:55",
        has(&e.m().log(), " n=3 0=0x0 1=0x1 2=0x")
            && !has(&e.m().log(), " 18=")
            && has(&e.m().log(), "type=0 dir=1")
            && has(&e.m().log(), "id=24 u32=0")
    ); // no TRUNCATE on egress (refused)
    check!(
        e,
        "t_mirror.cpp:56",
        e.m().mirror_collectors() == 2 && e.m().mirror_sets() == 2
    );
    // a second port on the same session and direction shares the set
    let a = a_oids(sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, &[span]);
    ok!(e, "t_mirror.cpp:59", e.set(sys::SAI_OBJECT_TYPE_PORT, p3, a));
    check!(e, "t_mirror.cpp:60", e.m().mirror_sets() == 2);
    // two sessions on one port and direction: refused
    let (st, span2) = e.create(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, &ms);
    ok!(e, "t_mirror.cpp:63", st);
    check_eq!(e, "t_mirror.cpp:64", e.m().mirror_dcdps(), 1); // same monitor port: one DCDP
    let a = a_oids(sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, &[span, span2]);
    check_eq!(
        e,
        "t_mirror.cpp:68",
        e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a),
        sys::SAI_STATUS_ATTR_NOT_SUPPORTED_0
    );
    ok!(
        e,
        "t_mirror.cpp:69",
        e.remove(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, span2)
    );
    // monitor port change: new DCDP, collectors re-made inside the same sets
    e.m().log_clear();
    let a = a_oid(sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, p3);
    ok!(
        e,
        "t_mirror.cpp:74",
        e.set(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, span, a)
    );
    log = e.m().log();
    check!(
        e,
        "t_mirror.cpp:76",
        has(&log, "sysport=0x3e200003")
            && has(&log, "ifcs_collector_set_member_remove")
            && has(&log, "ifcs_dup_copy_dest_port_delete")
    );
    check!(
        e,
        "t_mirror.cpp:77",
        !has(&log, "ifcs_collector_set_create") && !has(&log, "ifcs_collector_set_delete")
    );
    check!(
        e,
        "t_mirror.cpp:78",
        e.m().mirror_dcdps() == 1
            && e.m().mirror_collectors() == 2
            && e.m().mirror_sets() == 2
            && e.m().mirror_members() == 2
    );
    check_eq!(
        e,
        "t_mirror.cpp:79",
        e.remove(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, span),
        sys::SAI_STATUS_OBJECT_IN_USE
    );
    // off: policy DISABLE (action 1) on the same set; the set goes with its last user
    e.m().log_clear();
    // count 0 (the C++'s list pointer is stale there and unused)
    let a = a_oids(sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, &[]);
    ok!(e, "t_mirror.cpp:84", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a.clone()));
    check!(
        e,
        "t_mirror.cpp:85",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=22 u32=1")
    );
    check_eq!(e, "t_mirror.cpp:86", e.m().mirror_sets(), 2); // p3 still uses it
    ok!(e, "t_mirror.cpp:87", e.set(sys::SAI_OBJECT_TYPE_PORT, p3, a));
    let a = a_oids(sys::SAI_PORT_ATTR_EGRESS_MIRROR_SESSION, &[]);
    ok!(e, "t_mirror.cpp:89", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a));
    check!(
        e,
        "t_mirror.cpp:90",
        e.m().mirror_collectors() == 0 && e.m().mirror_sets() == 0 && e.m().mirror_members() == 0
    );
    ok!(
        e,
        "t_mirror.cpp:91",
        e.remove(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, span)
    );
    check_eq!(e, "t_mirror.cpp:92", e.m().mirror_dcdps(), 0);

    // ---- ERSPAN: type II collector with MACs, IPs, DSCP = TOS >> 2, TTL, session 0
    let er6 = IpAddr::V4(Ipv4Addr::from(0x0a63_0102u32));
    let mut er = vec![
        a_s32(
            sys::SAI_MIRROR_SESSION_ATTR_TYPE,
            sys::SAI_MIRROR_SESSION_TYPE_ENHANCED_REMOTE as i32,
        ),
        a_oid(sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, p2),
        a_s32(
            sys::SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE,
            sys::SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL as i32,
        ),
        a_u8(sys::SAI_MIRROR_SESSION_ATTR_TOS, 32),
        a_u8(sys::SAI_MIRROR_SESSION_ATTR_TTL, 64),
        Attr::new(
            sys::SAI_MIRROR_SESSION_ATTR_SRC_IP_ADDRESS,
            Value::Ip(IpAddr::V4(Ipv4Addr::from(0x0a63_0101u32))),
        ),
        Attr::new(sys::SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS, Value::Ip(er6)),
    ];
    let mut smac = [0u8; 6];
    let mut dmac = [0u8; 6];
    for k in 0..6u8 {
        smac[usize::from(k)] = 0x10 + k;
        dmac[usize::from(k)] = 0x20 + k;
    }
    er.push(a_mac(sys::SAI_MIRROR_SESSION_ATTR_SRC_MAC_ADDRESS, smac));
    er.push(a_mac(sys::SAI_MIRROR_SESSION_ATTR_DST_MAC_ADDRESS, dmac));
    er.push(a_u16(sys::SAI_MIRROR_SESSION_ATTR_GRE_PROTOCOL_TYPE, 0x88be));
    let (st, ers) = e.create(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, &er);
    ok!(e, "t_mirror.cpp:112", st);
    e.m().log_clear();
    let a = a_oids(sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, &[ers]);
    ok!(e, "t_mirror.cpp:117", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a));
    log = e.m().log();
    check!(
        e,
        "t_mirror.cpp:119",
        has(&log, " 0=0x3 1=0x0 2=0x")
            && has(
                &log,
                " 9=202122232425 10=101112131415 11=0a630101 12=0a630102 13=8 14=64 17=0"
            )
    );
    check!(e, "t_mirror.cpp:120", !has(&log, " 18=")); // no TRUNCATE on ERSPAN (0.12.41 refuses it)
    check!(e, "t_mirror.cpp:121", has(&log, "type=3 dir=0"));
    e.m().log_clear();
    let a = Attr::new(
        sys::SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS,
        Value::Ip(IpAddr::V4(Ipv4Addr::from(0x0a63_0103u32))),
    );
    ok!(
        e,
        "t_mirror.cpp:126",
        e.set(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, ers, a)
    );
    check!(
        e,
        "t_mirror.cpp:127",
        has(&e.m().log(), " 12=0a630103 ") && !has(&e.m().log(), "ifcs_dup_copy_dest_port_create")
    );
    let a = a_oids(sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, &[]);
    ok!(e, "t_mirror.cpp:130", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a));
    ok!(
        e,
        "t_mirror.cpp:131",
        e.remove(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, ers)
    );
    check!(
        e,
        "t_mirror.cpp:132",
        e.m().mirror_dcdps() == 0 && e.m().mirror_collectors() == 0 && e.m().mirror_sets() == 0
    );

    // ---- SAMPLEPACKET: CPU DCDP + sFlow collector (isai's tags) + set, SFLOW_INGRESS on the port
    let mut sp = a_u32(sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, 1000);
    e.m().log_clear();
    let (st, smp) = e.create(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, std::slice::from_ref(&sp));
    ok!(e, "t_mirror.cpp:140", st);
    check!(e, "t_mirror.cpp:141", !has(&e.m().log(), "ifcs_"));
    let a = a_oid(sys::SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE, smp);
    ok!(e, "t_mirror.cpp:144", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a.clone()));
    log = e.m().log();
    check!(
        e,
        "t_mirror.cpp:146",
        has(&log, "sysport=0x3e2003e8") && has(&log, " 0=0x5 1=0x0 2=0x") && has(&log, " 13=2 14=100 17=111")
    );
    check!(
        e,
        "t_mirror.cpp:147",
        has(&log, "type=5 dir=0") && has(&log, "ifcs_sysport_attr_set sp=0x3e200001 id=25 u32=0")
    );
    ok!(e, "t_mirror.cpp:148", e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a.clone())); // shared collector / set
    check!(
        e,
        "t_mirror.cpp:149",
        e.m().mirror_collectors() == 1 && e.m().mirror_sets() == 1 && e.m().mirror_dcdps() == 1
    );
    e.m().log_clear();
    sp = a_u32(sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, 500);
    ok!(e, "t_mirror.cpp:152", e.set(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, smp, sp));
    check_eq!(e, "t_mirror.cpp:153", e.m().log().matches('\n').count() as i32, 2); // both ports' policy re-set
    let a = a_oid(sys::SAI_PORT_ATTR_EGRESS_SAMPLEPACKET_ENABLE, smp);
    check_eq!(
        e,
        "t_mirror.cpp:155",
        e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a),
        sys::SAI_STATUS_ATTR_NOT_SUPPORTED_0
    );
    check_eq!(
        e,
        "t_mirror.cpp:156",
        e.remove(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, smp),
        sys::SAI_STATUS_OBJECT_IN_USE
    );
    e.m().log_clear();
    let a = a_oid(sys::SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE, 0);
    ok!(e, "t_mirror.cpp:160", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a.clone()));
    check!(
        e,
        "t_mirror.cpp:161",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=25 u32=1")
    );
    ok!(e, "t_mirror.cpp:162", e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a));
    check!(
        e,
        "t_mirror.cpp:163",
        e.m().mirror_collectors() == 0 && e.m().mirror_sets() == 0 && e.m().mirror_dcdps() == 0
    );
    ok!(e, "t_mirror.cpp:164", e.remove(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, smp));
}
