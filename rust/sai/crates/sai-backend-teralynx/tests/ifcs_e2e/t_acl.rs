//! t_acl.cpp: SAI ACL tables (L3, L3V6), port binding through a table group, entries with every
//! match field and action the frontend passes, counters, IN_PORTS on part of the bound ports,
//! policers, removal.
use std::net::{Ipv4Addr, Ipv6Addr};

use sai_client::{AclAction, AclData, AclField};

use crate::support::*;

fn battr(id: u32) -> Attr {
    a_bool(id, true)
}
fn s32attr(id: u32, v: i32) -> Attr {
    a_s32(id, v)
}
fn u32attr(id: u32, v: u32) -> Attr {
    a_u32(id, v)
}
fn oidattr(id: u32, v: u64) -> Attr {
    a_oid(id, v)
}
fn field(id: u32, data: AclData, mask: AclData) -> Attr {
    Attr::new(
        id,
        Value::AclField(AclField {
            enable: true,
            data,
            mask,
        }),
    )
}
fn f_u8(id: u32, v: u8, m: u8) -> Attr {
    field(id, AclData::U8(v), AclData::U8(m))
}
fn f_u16(id: u32, v: u16, m: u16) -> Attr {
    field(id, AclData::U16(v), AclData::U16(m))
}
fn f_ip4(id: u32, ip: &str, mask: &str) -> Attr {
    let ip: Ipv4Addr = ip.parse().unwrap();
    let mask: Ipv4Addr = mask.parse().unwrap();
    field(id, AclData::Ip4(ip.octets()), AclData::Ip4(mask.octets()))
}
fn f_ip6(id: u32, ip: &str, mask: &str) -> Attr {
    let ip: Ipv6Addr = ip.parse().unwrap();
    let mask: Ipv6Addr = mask.parse().unwrap();
    field(id, AclData::Ip6(ip.octets()), AclData::Ip6(mask.octets()))
}
fn f_s32(id: u32, v: i32) -> Attr {
    field(id, AclData::S32(v), AclData::None)
}
fn act(id: u32, parameter: AclData) -> Attr {
    Attr::new(
        id,
        Value::AclAction(AclAction {
            enable: true,
            parameter,
        }),
    )
}
fn act_s32(id: u32, v: i32) -> Attr {
    act(id, AclData::S32(v))
}
fn act_oid(id: u32, v: u64) -> Attr {
    act(id, AclData::Oid(v))
}

pub fn test_acl(e: &mut Env) {
    e.ck.section("acl");
    let (p1, p2) = (e.p1, e.p2);
    let aces0 = e.m().acl_aces();

    // ---- L3 table as aclorch: node-scope IFCS table, PORT_SHARING profile
    let bind = [
        sys::SAI_ACL_BIND_POINT_TYPE_PORT as i32,
        sys::SAI_ACL_BIND_POINT_TYPE_LAG as i32,
    ];
    let mut t: Vec<Attr> = Vec::new();
    t.push(s32attr(
        sys::SAI_ACL_TABLE_ATTR_ACL_STAGE,
        sys::SAI_ACL_STAGE_INGRESS as i32,
    ));
    t.push(a_s32s(sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST, &bind));
    for f in [
        sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_ID,
        sys::SAI_ACL_TABLE_ATTR_FIELD_ACL_IP_TYPE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_SRC_IP,
        sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP,
        sys::SAI_ACL_TABLE_ATTR_FIELD_IP_PROTOCOL,
        sys::SAI_ACL_TABLE_ATTR_FIELD_L4_SRC_PORT,
        sys::SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT,
        sys::SAI_ACL_TABLE_ATTR_FIELD_TCP_FLAGS,
        sys::SAI_ACL_TABLE_ATTR_FIELD_ICMP_TYPE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_ICMP_CODE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_DSCP,
        sys::SAI_ACL_TABLE_ATTR_FIELD_IN_PORTS,
    ] {
        t.push(battr(f));
    }
    e.m().log_clear();
    let (st, t4) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE, &t);
    ok!(e, "t_acl.cpp:86", st);
    let mut log = e.m().log();
    check!(
        e,
        "t_acl.cpp:88",
        has(&log, "ifcs_acl_match_profile_create") && has(&log, "sharing=1")
    );
    check!(e, "t_acl.cpp:89", has(&log, "list=4,18,27,25,56,28,29,59,60,68")); // in-port, ether, L3 type, ARP op, proto, IPs, ports, flags
    check!(
        e,
        "t_acl.cpp:90",
        has(&log, "ifcs_acl_table_create") && has(&log, "scope=0 type=9 size=256 prio=15 meters=256")
    );
    check!(
        e,
        "t_acl.cpp:91",
        has(&log, "ifcs_acl_create h=") && has(&log, "scope=0 ib=0")
    );
    check!(
        e,
        "t_acl.cpp:92",
        has(&log, "ifcs_acl_table_acl_add table=") && has(&log, " n=1")
    );

    // ---- bound to Ethernet0 (p1) through a table group
    let pb = [sys::SAI_ACL_BIND_POINT_TYPE_PORT as i32];
    let ga = [
        s32attr(
            sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE,
            sys::SAI_ACL_STAGE_INGRESS as i32,
        ),
        a_s32s(sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST, &pb),
        s32attr(
            sys::SAI_ACL_TABLE_GROUP_ATTR_TYPE,
            sys::SAI_ACL_TABLE_GROUP_TYPE_PARALLEL as i32,
        ),
    ];
    let (st, grp) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP, &ga);
    ok!(e, "t_acl.cpp:102", st);
    let gm = [
        oidattr(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, grp),
        oidattr(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, t4),
        u32attr(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY, 100),
    ];
    let (st, mem) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER, &gm);
    ok!(e, "t_acl.cpp:107", st);
    let a = oidattr(sys::SAI_PORT_ATTR_INGRESS_ACL, grp);
    ok!(e, "t_acl.cpp:109", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a));

    // ---- drop dst 10.99.0.1 tcp/179, counted
    let ca = [
        oidattr(sys::SAI_ACL_COUNTER_ATTR_TABLE_ID, t4),
        battr(sys::SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT),
        battr(sys::SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT),
    ];
    let (st, c1) = e.create(sys::SAI_OBJECT_TYPE_ACL_COUNTER, &ca);
    ok!(e, "t_acl.cpp:115", st);
    e.m().log_clear();
    let e1a = [
        oidattr(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, t4),
        u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 4000),
        f_ip4(sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IP, "10.99.0.1", "255.255.255.255"),
        f_u8(sys::SAI_ACL_ENTRY_ATTR_FIELD_IP_PROTOCOL, 6, 0xff),
        f_u16(sys::SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, 179, 0xffff),
        act_s32(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
        act_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER, c1),
    ];
    let (st, e1) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e1a);
    ok!(e, "t_acl.cpp:124", st);
    log = e.m().log();
    check!(
        e,
        "t_acl.cpp:126",
        has(&log, "ifcs_acl_port_add acl=") && has(&log, "sysport=0x3e200001")
    );
    check!(
        e,
        "t_acl.cpp:127",
        has(&log, "type=29 value=0a630001 object=0x0 mask=ffffffff")
    );
    check!(e, "t_acl.cpp:128", has(&log, "type=56 value=06 object=0x0 mask=ff"));
    check!(e, "t_acl.cpp:129", has(&log, "type=60 value=00b3 object=0x0 mask=ffff"));
    check!(e, "t_acl.cpp:130", has(&log, "type=drop green=1 yellow=1 red=1"));
    check!(
        e,
        "t_acl.cpp:131",
        has(&log, "ifcs_acl_counter_create") && has(&log, "mode=0")
    );
    let pr = format!("ib=99 prio={} matches=3 actions=2", 0xffff_ffffu32 - 2 * 4000u32);
    check!(e, "t_acl.cpp:134", has(&log, &pr)); // node scope, isai's priority mapping
    check_eq!(e, "t_acl.cpp:135", e.m().acl_aces(), aces0 + 1);
    check_eq!(e, "t_acl.cpp:136", e.m().acl_bound(), 1);
    e.m().acl_set_counter(42);
    let mut cg = [
        Attr::new(sys::SAI_ACL_COUNTER_ATTR_PACKETS, Value::U64(0)),
        Attr::new(sys::SAI_ACL_COUNTER_ATTR_BYTES, Value::U64(0)),
    ];
    let st = e.get(sys::SAI_OBJECT_TYPE_ACL_COUNTER, c1, &mut cg);
    ok!(e, "t_acl.cpp:141", st);
    check_eq!(e, "t_acl.cpp:142", cg[0].u64(), 42u64);
    check_eq!(e, "t_acl.cpp:143", cg[1].u64(), 42u64 * 64);
    e.m().acl_set_counter(0);

    // ---- FORWARD (a zero drop action), TRAP (copy with the ACL trap + drop), COPY
    e.m().log_clear();
    let e2a = [
        oidattr(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, t4),
        u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 3900),
        f_ip4(sys::SAI_ACL_ENTRY_ATTR_FIELD_SRC_IP, "10.0.0.0", "255.0.0.0"),
        f_u8(sys::SAI_ACL_ENTRY_ATTR_FIELD_TCP_FLAGS, 0x02, 0x3f),
        act_s32(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_FORWARD as i32,
        ),
    ];
    let (st, e2) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e2a);
    ok!(e, "t_acl.cpp:153", st);
    log = e.m().log();
    check!(
        e,
        "t_acl.cpp:155",
        has(&log, "type=28 value=0a000000 object=0x0 mask=ff000000")
    );
    check!(e, "t_acl.cpp:156", has(&log, "type=68 value=02 object=0x0 mask=3f"));
    check!(e, "t_acl.cpp:157", has(&log, "type=drop green=0 yellow=0 red=0"));
    e.m().log_clear();
    let mut e3a = [
        oidattr(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, t4),
        u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 3800),
        f_u16(sys::SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, 9999, 0xffff),
        act_s32(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_TRAP as i32,
        ),
    ];
    let (st, e3) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e3a);
    ok!(e, "t_acl.cpp:163", st);
    log = e.m().log();
    check!(
        e,
        "t_acl.cpp:165",
        has(&log, "ifcs_hostif_trap_create h=") && has(&log, "queue=0")
    ); // the ACL trap, first use
    check!(
        e,
        "t_acl.cpp:166",
        has(&log, "type=ctc hostif=0x") && has(&log, "green=1 yellow=1 red=1 dirty=f")
    );
    check!(e, "t_acl.cpp:167", has(&log, "type=drop green=1 yellow=1 red=1"));
    e.m().log_clear();
    e3a[1] = u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 3790);
    e3a[2] = f_u16(sys::SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, 9998, 0xffff);
    e3a[3] = act_s32(
        sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
        sys::SAI_PACKET_ACTION_COPY as i32,
    );
    let (st, e3c) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e3a);
    ok!(e, "t_acl.cpp:173", st);
    log = e.m().log();
    check!(e, "t_acl.cpp:175", !has(&log, "ifcs_hostif_trap_create")); // shared
    check!(
        e,
        "t_acl.cpp:176",
        has(&log, "type=ctc hostif=0x") && !has(&log, "type=drop")
    );

    // ---- ICMP echo (v4: type/code in the L4 source port field + protocol 1), DSCP, IPV4ANY, VLAN
    e.m().log_clear();
    let e4a = [
        oidattr(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, t4),
        u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 3700),
        f_u8(sys::SAI_ACL_ENTRY_ATTR_FIELD_ICMP_TYPE, 8, 0xff),
        f_u8(sys::SAI_ACL_ENTRY_ATTR_FIELD_ICMP_CODE, 0, 0xff),
        f_u8(sys::SAI_ACL_ENTRY_ATTR_FIELD_DSCP, 46, 0x3f),
        f_s32(
            sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_IP_TYPE,
            sys::SAI_ACL_IP_TYPE_IPV4ANY as i32,
        ),
        act_s32(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let (st, e4) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e4a);
    ok!(e, "t_acl.cpp:185", st);
    log = e.m().log();
    check!(e, "t_acl.cpp:187", has(&log, "type=59 value=0800 object=0x0 mask=ffff"));
    check!(e, "t_acl.cpp:188", has(&log, "type=56 value=01 object=0x0 mask=ff"));
    check!(e, "t_acl.cpp:189", has(&log, "type=54 value=2e object=0x0 mask=3f"));
    check!(e, "t_acl.cpp:190", has(&log, "type=27 value=04 object=0x0 mask=06"));
    e.m().log_clear();
    let mut e5a = [
        oidattr(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, t4),
        u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 3600),
        f_u16(sys::SAI_ACL_ENTRY_ATTR_FIELD_OUTER_VLAN_ID, 100, 0xfff),
        f_u16(sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE, 0x0800, 0xffff),
        act_s32(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let (st, e5) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e5a);
    ok!(e, "t_acl.cpp:196", st);
    log = e.m().log();
    check!(e, "t_acl.cpp:198", has(&log, "type=21 value=0064 object=0x0 mask=0fff"));
    check!(e, "t_acl.cpp:199", has(&log, "type=18 value=0800 object=0x0 mask=ffff"));
    // ARP request: ether type 0x0806 + ARP operation 1; NON_IPV4 has no IFCS form
    e.m().log_clear();
    e5a[2] = f_s32(
        sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_IP_TYPE,
        sys::SAI_ACL_IP_TYPE_ARP_REQUEST as i32,
    );
    let (st, e5b) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e5a[..3]);
    ok!(e, "t_acl.cpp:204", st);
    // (3 attrs: table, priority, ip type)
    log = e.m().log();
    check!(
        e,
        "t_acl.cpp:207",
        has(&log, "type=18 value=0806 object=0x0 mask=ffff") && has(&log, "type=25 value=0001 object=0x0 mask=ffff")
    );
    if !has(&log, "type=25") {
        eprintln!("--- ARP entry log ---\n{log}");
    }
    e5a[2] = f_s32(
        sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_IP_TYPE,
        sys::SAI_ACL_IP_TYPE_NON_IPV4 as i32,
    );
    let (st, _bad) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e5a[..3]);
    check!(e, "t_acl.cpp:211", st != SUCCESS);

    // ---- bind Ethernet120-like p2 too: every entry now spans 2 ports, one ACE each;
    // an IN_PORTS [p1] entry covers part of them: one ACE with the port match
    e.m().log_clear();
    let a = oidattr(sys::SAI_PORT_ATTR_INGRESS_ACL, grp);
    ok!(e, "t_acl.cpp:217", e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a));
    check_eq!(e, "t_acl.cpp:218", e.m().acl_bound(), 2);
    check!(e, "t_acl.cpp:219", has(&e.m().log(), "sysport=0x3e200002"));
    let aces1 = e.m().acl_aces();
    e.m().log_clear();
    let mut e6a = [
        oidattr(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, t4),
        u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 3500),
        Attr::new(
            sys::SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS,
            Value::AclField(AclField {
                enable: true,
                data: AclData::Oids(vec![p1]),
                mask: AclData::None,
            }),
        ),
        act_s32(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let (st, e6) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e6a);
    ok!(e, "t_acl.cpp:230", st);
    log = e.m().log();
    check!(e, "t_acl.cpp:232", has(&log, "type=4 value= object=0x3e200001"));
    check_eq!(e, "t_acl.cpp:233", e.m().acl_aces(), aces1 + 1);

    // ---- failed ACE: the entry is refused and leaves nothing behind
    let aces2 = e.m().acl_aces();
    e.m().acl_fail_next_ace();
    e6a[1] = u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 3400);
    let (st, _e7) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e6a);
    check!(e, "t_acl.cpp:240", st != SUCCESS);
    check_eq!(e, "t_acl.cpp:241", e.m().acl_aces(), aces2 + 1); // the mock counts the refused one
    check_eq!(e, "t_acl.cpp:242", e.m().acl_bound(), 2);

    // ---- policer on an entry: a meter in this table (node scope), METER action, red dropped
    let p = [
        a_s32(sys::SAI_POLICER_ATTR_METER_TYPE, sys::SAI_METER_TYPE_PACKETS as i32),
        a_s32(sys::SAI_POLICER_ATTR_MODE, sys::SAI_POLICER_MODE_SR_TCM as i32),
        a_u64(sys::SAI_POLICER_ATTR_CIR, 1000),
        a_u64(sys::SAI_POLICER_ATTR_CBS, 1000),
        a_s32(
            sys::SAI_POLICER_ATTR_RED_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let (st, pol) = e.create(sys::SAI_OBJECT_TYPE_POLICER, &p);
    ok!(e, "t_acl.cpp:252", st);
    e.m().log_clear();
    let e8a = [
        oidattr(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, t4),
        u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 3300),
        f_u16(sys::SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, 5001, 0xffff),
        act_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_POLICER, pol),
    ];
    let (st, e8) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e8a);
    ok!(e, "t_acl.cpp:257", st);
    log = e.m().log();
    check!(
        e,
        "t_acl.cpp:259",
        has(&log, "ifcs_acl_meter_create") && has(&log, "ib=99 mode=1 cir=672000 cbs=84000")
    );
    check!(e, "t_acl.cpp:260", has(&log, "type=drop green=0 yellow=0 red=1"));
    check!(e, "t_acl.cpp:261", has(&log, "type=0 object=0x")); // METER action
    check!(
        e,
        "t_acl.cpp:262",
        e.remove(sys::SAI_OBJECT_TYPE_POLICER, pol) != SUCCESS
    ); // in use

    // ---- L3V6 table: v6 addresses (16 bytes), next header, ICMPv6 type in L4 byte 1
    let mut t6: Vec<Attr> = Vec::new();
    t6.push(s32attr(
        sys::SAI_ACL_TABLE_ATTR_ACL_STAGE,
        sys::SAI_ACL_STAGE_INGRESS as i32,
    ));
    for f in [
        sys::SAI_ACL_TABLE_ATTR_FIELD_SRC_IPV6,
        sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6,
        sys::SAI_ACL_TABLE_ATTR_FIELD_IPV6_NEXT_HEADER,
        sys::SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT,
        sys::SAI_ACL_TABLE_ATTR_FIELD_ICMPV6_TYPE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_IN_PORTS,
    ] {
        t6.push(battr(f));
    }
    e.m().log_clear();
    let (st, tv6) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE, &t6);
    ok!(e, "t_acl.cpp:273", st);
    check!(e, "t_acl.cpp:274", has(&e.m().log(), "prio=14")); // the second table
    let gm6 = [
        oidattr(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, grp),
        oidattr(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, tv6),
        u32attr(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY, 200),
    ];
    let (st, mem6) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER, &gm6);
    ok!(e, "t_acl.cpp:279", st);
    e.m().log_clear();
    let e9a = [
        oidattr(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, tv6),
        u32attr(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 4000),
        f_ip6(
            sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IPV6,
            "2001:db8::1",
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
        ),
        f_u8(sys::SAI_ACL_ENTRY_ATTR_FIELD_ICMPV6_TYPE, 128, 0xff),
        act_s32(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let (st, e9) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e9a);
    ok!(e, "t_acl.cpp:286", st);
    log = e.m().log();
    check!(
        e,
        "t_acl.cpp:288",
        has(
            &log,
            "type=31 value=20010db8000000000000000000000001 object=0x0 mask=ffffffffffffffffffffffffffffffff"
        )
    );
    check!(
        e,
        "t_acl.cpp:289",
        has(&log, "type=61 value=80 object=0x0 mask=ff") && has(&log, "type=56 value=3a object=0x0 mask=ff")
    );
    check_eq!(e, "t_acl.cpp:290", count(&log, "ifcs_ace_create"), 1usize); // both ports bound: one ACE

    // ---- removal: entries, unbinding, tables
    for x in [e1, e2, e3, e3c, e4, e5, e5b, e6, e8, e9] {
        ok!(e, "t_acl.cpp:293", e.remove(sys::SAI_OBJECT_TYPE_ACL_ENTRY, x));
    }
    check_eq!(e, "t_acl.cpp:294", e.m().acl_aces(), aces0 + 1); // + the refused one the mock counted
    check_eq!(e, "t_acl.cpp:295", e.m().acl_bound(), 0); // no entry: no port bound
    e.m().log_clear();
    ok!(e, "t_acl.cpp:297", e.remove(sys::SAI_OBJECT_TYPE_POLICER, pol));
    check!(e, "t_acl.cpp:298", has(&e.m().log(), "ifcs_acl_meter_delete"));
    let a = oidattr(sys::SAI_PORT_ATTR_INGRESS_ACL, u64::from(sys::SAI_NULL_OBJECT_ID));
    ok!(e, "t_acl.cpp:300", e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a.clone()));
    ok!(e, "t_acl.cpp:301", e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a));
    ok!(
        e,
        "t_acl.cpp:302",
        e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER, mem)
    );
    ok!(
        e,
        "t_acl.cpp:303",
        e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER, mem6)
    );
    ok!(e, "t_acl.cpp:304", e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP, grp));
    ok!(e, "t_acl.cpp:305", e.remove(sys::SAI_OBJECT_TYPE_ACL_COUNTER, c1));
    e.m().log_clear();
    ok!(e, "t_acl.cpp:307", e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE, t4));
    ok!(e, "t_acl.cpp:308", e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE, tv6));
    log = e.m().log();
    check_eq!(e, "t_acl.cpp:310", count(&log, "ifcs_acl_table_delete"), 2usize);
    check!(
        e,
        "t_acl.cpp:311",
        has(&log, "ifcs_acl_table_acl_remove") && has(&log, "ifcs_acl_delete")
    );
}
