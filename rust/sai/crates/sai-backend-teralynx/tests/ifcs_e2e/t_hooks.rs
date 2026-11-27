//! t_hooks.cpp: the optional backend hooks of the integration phase (RIF counters and admin state, the
//! glean egress, port counters by SAI id, hash objects, VLAN flood control, storm control, ACL ranges /
//! REDIRECT / SET_TC / SET_DSCP / MIRROR / egress tables + OUT_PORTS, entry admin state and in-place
//! update, AVAILABLE_ACL_ENTRY) through the public SAI API against the mock libifcs.
use sai_client::{AclAction, AclData, AclField, Prefix};

use crate::support::*;

/// `at(id)`: a zeroed attribute.
fn at(id: u32) -> Attr {
    Attr::new(id, Value::None)
}

/// `act(id, parameter)`: an enabled ACL action with its parameter.
fn act(id: u32, parameter: AclData) -> Attr {
    Attr::new(
        id,
        Value::AclAction(AclAction {
            enable: true,
            parameter,
        }),
    )
}

/// The handle after "key" in the first log line that has it ("h=0x...").
fn handle_after(log: &str, key: &str) -> String {
    let Some(p) = log.find(key) else { return String::new() };
    let p = p + key.len();
    let q = log[p..].find([' ', '\n']).map_or(log.len(), |i| p + i);
    log[p..q].to_owned()
}

/// `inet_pton` into a prefix: v4 address and mask bytes.
fn v4_prefix(addr: [u8; 4], mask: [u8; 4]) -> Prefix {
    let mut p = Prefix {
        v6: false,
        addr: [0; 16],
        mask: [0; 16],
    };
    p.addr[..4].copy_from_slice(&addr);
    p.mask[..4].copy_from_slice(&mask);
    p
}

fn hooks_l3(e: &mut Env, vr: u64, cpu: u64) {
    let (s, p3) = (e.s, e.p3);
    // ---- port RIF on p3: L3 interface counters, admin state
    e.m().log_clear();
    let ra_ = [
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, vr),
        a_s32(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            sys::SAI_ROUTER_INTERFACE_TYPE_PORT as i32,
        ),
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, p3),
    ];
    let (st, rif) = e.create(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, &ra_);
    ok!(e, "t_hooks.cpp:52", st);
    let ih = handle_after(&e.m().log(), "ifcs_intf_create h=");
    check!(e, "t_hooks.cpp:54", !ih.is_empty());
    let ihv = u32::from_str_radix(ih.trim_start_matches("0x"), 16).unwrap_or(0);
    let (st, caps) = e.sai.stats_capability(s, sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, 64);
    ok!(e, "t_hooks.cpp:58", st);
    check_eq!(e, "t_hooks.cpp:59", caps.len(), 4usize);
    e.m().set_intf_stat(ihv, 24, 70); // IFCS L3INTF_INGRESS_IPV4_PKTS (ingress mode 4 = IPV4_IPV6_ANY)
    e.m().set_intf_stat(ihv, 26, 7); // L3INTF_INGRESS_IPV6_PKTS
    e.m().set_intf_stat(ihv, 28, 5); // L3INTF_EGRESS_IPV4_PKTS
    e.m().set_intf_stat(ihv, 30, 6); // L3INTF_EGRESS_IPV6_PKTS
    let ids = [
        sys::SAI_ROUTER_INTERFACE_STAT_IN_PACKETS,
        sys::SAI_ROUTER_INTERFACE_STAT_OUT_PACKETS,
        sys::SAI_ROUTER_INTERFACE_STAT_OUT_OCTETS,
    ];
    let (st, v) = e.sai.stats(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif, &ids);
    ok!(e, "t_hooks.cpp:67", st);
    check_eq!(e, "t_hooks.cpp:68", v[0], 77u64);
    check_eq!(e, "t_hooks.cpp:69", v[1], 11u64); // IPv4 + IPv6 egress
    let bad = sys::SAI_ROUTER_INTERFACE_STAT_IN_ERROR_PACKETS;
    check_eq!(
        e,
        "t_hooks.cpp:71",
        e.sai.stats(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif, &[bad]).0,
        sys::SAI_STATUS_NOT_SUPPORTED
    );
    e.m().log_clear();
    let (st, _) = e.sai.stats_ext(
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        rif,
        &ids[..1],
        sys::SAI_STATS_MODE_READ_AND_CLEAR,
    );
    ok!(e, "t_hooks.cpp:73", st);
    check!(e, "t_hooks.cpp:74", has(&e.m().log(), "ifcs_intf_stats_clear"));
    e.m().log_clear();
    let st = e.sai.clear_stats(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif, &ids[..1]);
    ok!(e, "t_hooks.cpp:76", st);
    check!(e, "t_hooks.cpp:77", has(&e.m().log(), "ifcs_intf_stats_clear"));
    e.m().log_clear();
    let mut a = a_bool(sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE, false);
    let st = e.set(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif, a.clone());
    ok!(e, "t_hooks.cpp:80", st);
    check!(
        e,
        "t_hooks.cpp:81",
        has(
            &e.m().log(),
            &format!("ifcs_intf_attr_set h=0x{} count=2 id=18", &ih[2..])
        )
    );
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif, a.clone()); // no change: no IFCS call
    ok!(e, "t_hooks.cpp:83", st);
    check!(e, "t_hooks.cpp:84", !has(&e.m().log(), "ifcs_intf_attr_set"));
    a.value = Value::Bool(true);
    let st = e.set(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif, a.clone());
    ok!(e, "t_hooks.cpp:86", st);
    a = Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE, Value::Bool(false));
    let mut ga = [a];
    let st = e.get(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif, &mut ga);
    let [a] = ga;
    ok!(e, "t_hooks.cpp:88", st);
    check!(e, "t_hooks.cpp:89", a.boolean());

    // ---- glean: a connected subnet copies to its own hostif trap, IP2ME to another
    let re = Entry::Route {
        vr,
        prefix: v4_prefix([10, 77, 0, 0], [255, 255, 255, 0]),
    };
    let a = a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, rif);
    let st = e.sai.entry(EntryOp::Create, s, &re, &[a]);
    ok!(e, "t_hooks.cpp:100", st);
    let me = Entry::Route {
        vr,
        prefix: v4_prefix([10, 77, 0, 1], [255, 255, 255, 255]),
    };
    let a = a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, cpu);
    let st = e.sai.entry(EntryOp::Create, s, &me, &[a]);
    ok!(e, "t_hooks.cpp:105", st);
    let glean = e.m().route_trap("26200001/4/0a4d0000/ffffff00");
    let ip2me = e.m().route_trap("26200001/4/0a4d0001/ffffffff");
    check!(e, "t_hooks.cpp:107", glean != 0 && ip2me != 0 && glean != ip2me);
    let st = e.sai.entry(EntryOp::Remove, s, &me, &[]);
    ok!(e, "t_hooks.cpp:108", st);
    let st = e.sai.entry(EntryOp::Remove, s, &re, &[]);
    ok!(e, "t_hooks.cpp:109", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif);
    ok!(e, "t_hooks.cpp:110", st);
}

fn hooks_hash_flood_storm(e: &mut Env) {
    let (s, p1, p2) = (e.s, e.p1, e.p2);
    // ---- port counters by SAI id: the counters area's map (stats capability too)
    let (st, caps) = e.sai.stats_capability(s, sys::SAI_OBJECT_TYPE_PORT, 512);
    ok!(e, "t_hooks.cpp:119", st);
    let (mut b64, mut fec) = (false, false);
    for (stat, _) in &caps {
        if *stat == sys::SAI_PORT_STAT_ETHER_STATS_PKTS_64_OCTETS {
            b64 = true;
        }
        if *stat == sys::SAI_PORT_STAT_IF_IN_FEC_CORRECTABLE_FRAMES {
            fec = true;
        }
    }
    check!(e, "t_hooks.cpp:125", b64 && fec && caps.len() > 50);
    let pid = [
        sys::SAI_PORT_STAT_ETHER_STATS_PKTS_64_OCTETS,
        sys::SAI_PORT_STAT_IF_IN_OCTETS,
    ];
    e.m().log_clear();
    let (st, _) = e.sai.stats(sys::SAI_OBJECT_TYPE_PORT, p1, &pid);
    ok!(e, "t_hooks.cpp:129", st);
    // one call for both
    check_eq!(
        e,
        "t_hooks.cpp:130",
        count(&e.m().log(), "ifcs_devport_stats_get dp=1"),
        1usize
    );

    // ---- hash: SWITCH ECMP_HASH / LAG_HASH objects, fields and seed
    ok!(e, "t_hooks.cpp:134", e.sai.query(Api::Hash));
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        s,
        sys::SAI_SWITCH_ATTR_ECMP_HASH,
        Value::Oid(0),
    );
    ok!(e, "t_hooks.cpp:136", st);
    let eh = a.oid();
    check!(e, "t_hooks.cpp:138", eh != 0);
    let f = vec![
        sys::SAI_NATIVE_HASH_FIELD_SRC_IP as i32,
        sys::SAI_NATIVE_HASH_FIELD_DST_IP as i32,
    ];
    let mut a = Attr::new(sys::SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST, Value::S32List(f));
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HASH, eh, a.clone());
    ok!(e, "t_hooks.cpp:144", st);
    // one per IP packet type (non-IP: no IP field)
    check_eq!(
        e,
        "t_hooks.cpp:145",
        count(&e.m().log(), "ifcs_extended_field_list_create"),
        4usize
    );
    a.value = Value::S32List(vec![sys::SAI_NATIVE_HASH_FIELD_INNER_SRC_IP as i32]);
    // no IFCS element
    let st = e.set(sys::SAI_OBJECT_TYPE_HASH, eh, a.clone());
    check!(e, "t_hooks.cpp:149", st != SUCCESS);
    let a = a_u32(sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED, 7);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_SWITCH, s, a);
    ok!(e, "t_hooks.cpp:152", st);
    check!(e, "t_hooks.cpp:153", has(&e.m().log(), "u32=7"));

    // ---- VLAN flood control: all three kinds off = an empty flood group
    let va_ = a_u32(sys::SAI_VLAN_ATTR_VLAN_ID, 300);
    let (st, v300) = e.create(sys::SAI_OBJECT_TYPE_VLAN, &[va_]);
    ok!(e, "t_hooks.cpp:158", st);
    e.m().log_clear();
    let mut a = a_s32(
        sys::SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE,
        sys::SAI_VLAN_FLOOD_CONTROL_TYPE_NONE as i32,
    );
    let st = e.set(sys::SAI_OBJECT_TYPE_VLAN, v300, a.clone());
    ok!(e, "t_hooks.cpp:161", st);
    // mixed: recorded only
    check!(e, "t_hooks.cpp:162", !has(&e.m().log(), "ifcs_l2vni_attr_set"));
    a.id = sys::SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE;
    let st = e.set(sys::SAI_OBJECT_TYPE_VLAN, v300, a.clone());
    ok!(e, "t_hooks.cpp:164", st);
    a.id = sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE;
    let st = e.set(sys::SAI_OBJECT_TYPE_VLAN, v300, a.clone());
    ok!(e, "t_hooks.cpp:166", st);
    check!(
        e,
        "t_hooks.cpp:167",
        has(&e.m().log(), "ifcs_mdg_create") && has(&e.m().log(), "ifcs_l2vni_attr_set vni=300 count=1")
    );
    e.m().log_clear();
    for id in [
        sys::SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE,
        sys::SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE,
        sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE,
    ] {
        a = a_s32(id, sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL as i32);
        let st = e.set(sys::SAI_OBJECT_TYPE_VLAN, v300, a.clone());
        ok!(e, "t_hooks.cpp:172", st);
    }
    check_eq!(
        e,
        "t_hooks.cpp:174",
        count(&e.m().log(), "ifcs_l2vni_attr_set vni=300"),
        1usize
    );
    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN, v300);
    ok!(e, "t_hooks.cpp:176", st);
    // members' and the empty one
    check_eq!(e, "t_hooks.cpp:177", count(&e.m().log(), "ifcs_mdg_delete"), 2usize);

    // ---- storm control: an ACL entry per port and kind, the policer's meter
    let pa_ = [
        a_s32(sys::SAI_POLICER_ATTR_METER_TYPE, sys::SAI_METER_TYPE_PACKETS as i32),
        a_s32(sys::SAI_POLICER_ATTR_MODE, sys::SAI_POLICER_MODE_SR_TCM as i32),
        a_u64(sys::SAI_POLICER_ATTR_CIR, 100),
        a_u64(sys::SAI_POLICER_ATTR_CBS, 100),
    ];
    let (st, pol) = e.create(sys::SAI_OBJECT_TYPE_POLICER, &pa_);
    ok!(e, "t_hooks.cpp:186", st);
    let aces = e.m().acl_aces();
    e.m().log_clear();
    let a = a_oid(sys::SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID, pol);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a);
    ok!(e, "t_hooks.cpp:190", st);
    let log = e.m().log();
    check!(
        e,
        "t_hooks.cpp:192",
        has(&log, "ifcs_acl_table_create") && has(&log, "ifcs_acl_meter_create")
    );
    check!(
        e,
        "t_hooks.cpp:193",
        has(&log, "type=17 value=ffffffffffff object=0x0 mask=ffffffffffff")
    );
    check_eq!(e, "t_hooks.cpp:194", e.m().acl_aces(), aces + 1);
    let a = a_oid(sys::SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID, pol);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a);
    ok!(e, "t_hooks.cpp:197", st);
    // DMAC missed the L2 table
    check!(e, "t_hooks.cpp:198", has(&e.m().log(), "type=82 value=00"));
    check_eq!(e, "t_hooks.cpp:199", e.m().acl_aces(), aces + 2);
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_PORT,
        p2,
        sys::SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID,
        Value::Oid(0),
    );
    ok!(e, "t_hooks.cpp:201", st);
    check_eq!(e, "t_hooks.cpp:202", a.oid(), pol);
    for id in [
        sys::SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID,
        sys::SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID,
    ] {
        let a = a_oid(id, 0);
        let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a);
        ok!(e, "t_hooks.cpp:205", st);
    }
    check_eq!(e, "t_hooks.cpp:207", e.m().acl_aces(), aces);
    let st = e.remove(sys::SAI_OBJECT_TYPE_POLICER, pol);
    ok!(e, "t_hooks.cpp:208", st);
}

#[allow(clippy::too_many_lines, reason = "one to one with hooks_acl of the C++")]
fn hooks_acl(e: &mut Env) {
    let (p1, p2) = (e.p1, e.p2);
    ok!(e, "t_hooks.cpp:216", e.sai.query(Api::Mirror));
    // ---- an ingress table with ranges and the extra actions
    let bind = vec![sys::SAI_ACL_BIND_POINT_TYPE_PORT as i32];
    let rtypes = vec![sys::SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE as i32];
    let acts = [
        sys::SAI_ACL_ACTION_TYPE_PACKET_ACTION as i32,
        sys::SAI_ACL_ACTION_TYPE_COUNTER as i32,
        sys::SAI_ACL_ACTION_TYPE_REDIRECT as i32,
        sys::SAI_ACL_ACTION_TYPE_SET_TC as i32,
        sys::SAI_ACL_ACTION_TYPE_MIRROR_INGRESS as i32,
        sys::SAI_ACL_ACTION_TYPE_SET_DSCP as i32,
    ];
    let mut t = vec![
        a_s32(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, sys::SAI_ACL_STAGE_INGRESS as i32),
        at(sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST),
        at(sys::SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE),
        at(sys::SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST),
        a_bool(sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP, true),
        a_bool(sys::SAI_ACL_TABLE_ATTR_FIELD_IP_PROTOCOL, true),
    ];
    t[1].value = Value::S32List(bind.clone());
    t[2].value = Value::S32List(rtypes);
    t[3].value = Value::S32List(acts.to_vec());
    // SET_DSCP: refused by 0.12.41
    let st = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE, &t).0;
    check!(e, "t_hooks.cpp:230", st != SUCCESS);
    t[3].value = Value::S32List(acts[..5].to_vec());
    e.m().log_clear();
    let (st, tab) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE, &t);
    ok!(e, "t_hooks.cpp:233", st);
    // drop, copy, copy override, counter, meter + redirect, tc, mirror
    check!(
        e,
        "t_hooks.cpp:234",
        has(&e.m().log(), "ifcs_acl_action_profile_create") && has(&e.m().log(), "types=8")
    );
    let mut b = a_oid(sys::SAI_PORT_ATTR_INGRESS_ACL, tab);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, b.clone());
    ok!(e, "t_hooks.cpp:236", st);
    let rg = [
        a_s32(
            sys::SAI_ACL_RANGE_ATTR_TYPE,
            sys::SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE as i32,
        ),
        Attr::new(sys::SAI_ACL_RANGE_ATTR_LIMIT, Value::U32Range(1000, 2000)),
    ];
    let (st, range) = e.create(sys::SAI_OBJECT_TYPE_ACL_RANGE, &rg);
    ok!(e, "t_hooks.cpp:241", st);
    let kc = [
        a_oid(sys::SAI_ACL_COUNTER_ATTR_TABLE_ID, tab),
        a_bool(sys::SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT, true),
        a_bool(sys::SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT, true),
    ];
    let (st, k1) = e.create(sys::SAI_OBJECT_TYPE_ACL_COUNTER, &kc);
    ok!(e, "t_hooks.cpp:245", st);
    // entry 1: dst port 1000-2000 -> TC 5, counted
    let e1 = [
        a_oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, tab),
        a_u32(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 10),
        Attr::new(
            sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE,
            Value::AclField(AclField {
                enable: true,
                data: AclData::Oids(vec![range]),
                mask: AclData::None,
            }),
        ),
        act(sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_TC, AclData::U8(5)),
        act(sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER, AclData::Oid(k1)),
    ];
    e.m().log_clear();
    let cmps = e.m().acl_comparators();
    let (st, en1) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e1);
    ok!(e, "t_hooks.cpp:257", st);
    let log = e.m().log();
    check!(
        e,
        "t_hooks.cpp:259",
        has(&log, "ifcs_acl_comparator_create") && has(&log, "type=0 op=7 value=1000 hi=2000 dir=0")
    );
    check!(
        e,
        "t_hooks.cpp:260",
        has(&log, "ifcs_acl_comparator_group_create") && has(&log, "n=1")
    );
    check!(e, "t_hooks.cpp:261", has(&log, "type=67 value= object=0x"));
    check!(
        e,
        "t_hooks.cpp:262",
        has(&log, "type=set_tc green=1 tc=5 yellow=5 red=5 dirty=3f")
    );
    check_eq!(e, "t_hooks.cpp:263", e.m().acl_comparators(), cmps + 1);
    let mut ga = [Attr::new(
        sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE,
        Value::AclField(AclField {
            enable: false,
            data: AclData::Oids(vec![0, 0]),
            mask: AclData::None,
        }),
    )];
    let st = e.get(sys::SAI_OBJECT_TYPE_ACL_ENTRY, en1, &mut ga);
    let [ga] = ga;
    ok!(e, "t_hooks.cpp:267", st);
    check!(
        e,
        "t_hooks.cpp:268",
        ga.count() == 1 && ga.oids().first() == Some(&range)
    );
    check_eq!(
        e,
        "t_hooks.cpp:269",
        e.remove(sys::SAI_OBJECT_TYPE_ACL_RANGE, range),
        sys::SAI_STATUS_OBJECT_IN_USE
    );
    // admin down / up without re-creation; priority change in place (counter kept)
    e.m().log_clear();
    let mut ad = a_bool(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, false);
    let st = e.set(sys::SAI_OBJECT_TYPE_ACL_ENTRY, en1, ad.clone());
    ok!(e, "t_hooks.cpp:273", st);
    check!(
        e,
        "t_hooks.cpp:274",
        has(&e.m().log(), "ifcs_ace_attr_set")
            && has(&e.m().log(), "id=3 data=0")
            && !has(&e.m().log(), "ifcs_ace_delete")
    );
    ad.value = Value::Bool(true);
    let st = e.set(sys::SAI_OBJECT_TYPE_ACL_ENTRY, en1, ad);
    ok!(e, "t_hooks.cpp:276", st);
    check!(e, "t_hooks.cpp:277", has(&e.m().log(), "id=3 data=1"));
    e.m().log_clear();
    let pr = a_u32(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 20);
    let st = e.set(sys::SAI_OBJECT_TYPE_ACL_ENTRY, en1, pr);
    ok!(e, "t_hooks.cpp:280", st);
    check!(
        e,
        "t_hooks.cpp:281",
        has(&e.m().log(), "ifcs_ace_delete") && has(&e.m().log(), "ifcs_ace_create")
    );
    check!(
        e,
        "t_hooks.cpp:282",
        !has(&e.m().log(), "ifcs_acl_counter_delete") && !has(&e.m().log(), "ifcs_acl_counter_create")
    );
    // entry 2: redirect to p2 (SET_DSCP refused); entry 3: mirror to a SPAN session on p2
    let mut e2 = vec![
        a_oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, tab),
        a_u32(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 30),
        act(sys::SAI_ACL_ENTRY_ATTR_ACTION_REDIRECT, AclData::Oid(p2)),
        act(sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_DSCP, AclData::U8(46)),
    ];
    let st = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e2).0;
    check!(e, "t_hooks.cpp:289", st != SUCCESS);
    e2.pop();
    e.m().log_clear();
    let (st, en2) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e2);
    ok!(e, "t_hooks.cpp:292", st);
    // sysport of devport 2
    check!(e, "t_hooks.cpp:293", has(&e.m().log(), "type=10 object=0x3e200002"));
    let ms = [
        a_s32(
            sys::SAI_MIRROR_SESSION_ATTR_TYPE,
            sys::SAI_MIRROR_SESSION_TYPE_LOCAL as i32,
        ),
        a_oid(sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, p2),
    ];
    let (st, sess) = e.create(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, &ms);
    ok!(e, "t_hooks.cpp:297", st);
    let e3 = [
        a_oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, tab),
        a_u32(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 40),
        act(sys::SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS, AclData::Oids(vec![sess])),
    ];
    e.m().log_clear();
    let (st, en3) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &e3);
    ok!(e, "t_hooks.cpp:303", st);
    check!(
        e,
        "t_hooks.cpp:304",
        has(&e.m().log(), "ifcs_collector_set_create") && has(&e.m().log(), "type=7 object=0x")
    );
    let (st, av) = e.get1(
        sys::SAI_OBJECT_TYPE_ACL_TABLE,
        tab,
        sys::SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY,
        Value::U32(0),
    );
    ok!(e, "t_hooks.cpp:306", st);
    check_eq!(e, "t_hooks.cpp:307", av.u32(), 253u32);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_ENTRY, en3);
    ok!(e, "t_hooks.cpp:308", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_MIRROR_SESSION, sess);
    ok!(e, "t_hooks.cpp:309", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_ENTRY, en2);
    ok!(e, "t_hooks.cpp:310", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_ENTRY, en1);
    ok!(e, "t_hooks.cpp:311", st);
    check_eq!(e, "t_hooks.cpp:312", e.m().acl_comparators(), cmps);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_RANGE, range);
    ok!(e, "t_hooks.cpp:313", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_COUNTER, k1);
    ok!(e, "t_hooks.cpp:314", st);
    b.value = Value::Oid(0);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, b);
    ok!(e, "t_hooks.cpp:316", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE, tab);
    ok!(e, "t_hooks.cpp:317", st);

    // ---- an egress table with OUT_PORTS, bound through an egress group
    let eacts = vec![
        sys::SAI_ACL_ACTION_TYPE_PACKET_ACTION as i32,
        sys::SAI_ACL_ACTION_TYPE_COUNTER as i32,
    ];
    let mut et = vec![
        a_s32(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, sys::SAI_ACL_STAGE_EGRESS as i32),
        at(sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST),
        at(sys::SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST),
        a_bool(sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP, true),
        a_bool(sys::SAI_ACL_TABLE_ATTR_FIELD_OUT_PORTS, true),
    ];
    et[1].value = Value::S32List(bind.clone());
    et[2].value = Value::S32List(eacts);
    let (st, etab) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE, &et);
    ok!(e, "t_hooks.cpp:327", st);
    let mut gr = [
        a_s32(
            sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE,
            sys::SAI_ACL_STAGE_EGRESS as i32,
        ),
        at(sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST),
        a_s32(
            sys::SAI_ACL_TABLE_GROUP_ATTR_TYPE,
            sys::SAI_ACL_TABLE_GROUP_TYPE_PARALLEL as i32,
        ),
    ];
    gr[1].value = Value::S32List(bind);
    let (st, eg) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP, &gr);
    ok!(e, "t_hooks.cpp:333", st);
    let gm = [
        a_oid(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, eg),
        a_oid(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, etab),
    ];
    let (st, egm) = e.create(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER, &gm);
    ok!(e, "t_hooks.cpp:336", st);
    let wrong = a_oid(sys::SAI_PORT_ATTR_INGRESS_ACL, eg);
    // an egress group as ingress ACL
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p2, wrong);
    check!(e, "t_hooks.cpp:338", st != SUCCESS);
    let mut eb = a_oid(sys::SAI_PORT_ATTR_EGRESS_ACL, eg);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p2, eb.clone());
    ok!(e, "t_hooks.cpp:340", st);
    let (st, g2) = e.get1(
        sys::SAI_OBJECT_TYPE_PORT,
        p2,
        sys::SAI_PORT_ATTR_EGRESS_ACL,
        Value::Oid(0),
    );
    ok!(e, "t_hooks.cpp:342", st);
    check_eq!(e, "t_hooks.cpp:343", g2.oid(), eg);
    let ee = [
        a_oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, etab),
        a_u32(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 10),
        Attr::new(
            sys::SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORTS,
            Value::AclField(AclField {
                enable: true,
                data: AclData::Oids(vec![p2]),
                mask: AclData::None,
            }),
        ),
        act(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            AclData::S32(sys::SAI_PACKET_ACTION_DROP as i32),
        ),
    ];
    e.m().log_clear();
    let (st, een) = e.create(sys::SAI_OBJECT_TYPE_ACL_ENTRY, &ee);
    ok!(e, "t_hooks.cpp:351", st);
    check!(
        e,
        "t_hooks.cpp:352",
        has(&e.m().log(), "ifcs_acl_port_add") && has(&e.m().log(), "sysport=0x3e200002")
    );
    check!(
        e,
        "t_hooks.cpp:353",
        has(&e.m().log(), "type=drop green=1 yellow=1 red=1")
    );
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_ENTRY, een);
    ok!(e, "t_hooks.cpp:354", st);
    eb.value = Value::Oid(0);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p2, eb);
    ok!(e, "t_hooks.cpp:356", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER, egm);
    ok!(e, "t_hooks.cpp:357", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP, eg);
    ok!(e, "t_hooks.cpp:358", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ACL_TABLE, etab);
    ok!(e, "t_hooks.cpp:359", st);
}

pub fn test_hooks(e: &mut Env) {
    e.ck.section("hooks");
    let s = e.s;
    let mut a = [
        Attr::new(sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID, Value::Oid(0)),
        Attr::new(sys::SAI_SWITCH_ATTR_CPU_PORT, Value::Oid(0)),
    ];
    let st = e.get(sys::SAI_OBJECT_TYPE_SWITCH, s, &mut a);
    ok!(e, "t_hooks.cpp:373", st);
    let (vr, cpu) = (a[0].oid(), a[1].oid());
    hooks_l3(e, vr, cpu);
    hooks_hash_flood_storm(e);
    hooks_acl(e);
}
