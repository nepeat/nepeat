//! t_copp.cpp: host interface traps + CoPP (Backend::trap_program): hostif traps with the group
//! queue, the CoPP ACL (one per inno block; the mock has 6), trap entries with copy / drop / meter /
//! counter, policer updates, system traps (TTL, MTU), IP2ME-based traps.
use crate::support::*;

/// make_trap with no status out-parameter: the create must succeed (the OK of the C++ helper).
fn make_trap(e: &mut Env, ty: u32, action: u32, group: u64) -> u64 {
    let (r, id) = e.create(
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP,
        &[
            a_s32(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE, ty as i32),
            a_s32(sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION, action as i32),
            a_oid(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, group),
        ],
    );
    ok!(e, "t_copp.cpp:30", r);
    id
}

pub fn test_copp(e: &mut Env) {
    e.ck.section("copp");
    let s = e.s;

    // policer 600 pps, red drop -> group on queue 4
    let p = [
        a_s32(sys::SAI_POLICER_ATTR_METER_TYPE, sys::SAI_METER_TYPE_PACKETS as i32),
        a_s32(sys::SAI_POLICER_ATTR_MODE, sys::SAI_POLICER_MODE_SR_TCM as i32),
        a_u64(sys::SAI_POLICER_ATTR_CIR, 600),
        a_u64(sys::SAI_POLICER_ATTR_CBS, 600),
        a_s32(
            sys::SAI_POLICER_ATTR_RED_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let (st, pid) = e.create(sys::SAI_OBJECT_TYPE_POLICER, &p);
    ok!(e, "t_copp.cpp:50", st);
    let g = [
        a_u32(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, 4),
        a_oid(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER, pid),
    ];
    let (st, grp) = e.create(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, &g);
    ok!(e, "t_copp.cpp:55", st);

    // ---- LLDP (TRAP): CoPP table on first use, 3 rules x 6 IBs
    e.m().log_clear();
    let lldp = make_trap(e, sys::SAI_HOSTIF_TRAP_TYPE_LLDP, sys::SAI_PACKET_ACTION_TRAP, grp);
    let log = e.m().log();
    check!(
        e,
        "t_copp.cpp:61",
        has(&log, "ifcs_acl_table_create") && has(&log, "scope=1 type=1 size=128")
    );
    check_eq!(e, "t_copp.cpp:62", count(&log, "ifcs_acl_create h="), 6);
    check!(e, "t_copp.cpp:63", has(&log, "ifcs_acl_create ib=6 refused"));
    check!(
        e,
        "t_copp.cpp:64",
        has(&log, "ifcs_acl_table_acl_add table=") && has(&log, " n=6")
    );
    check!(
        e,
        "t_copp.cpp:65",
        has(&log, "ifcs_hostif_trap_create h=") && has(&log, "queue=4")
    );
    check_eq!(e, "t_copp.cpp:66", count(&log, "ifcs_acl_meter_create"), 6);
    // 600 pps at 84 B, as isai converts
    check!(e, "t_copp.cpp:67", has(&log, "ib=0 mode=1 cir=403200 cbs=50400"));
    check_eq!(e, "t_copp.cpp:68", count(&log, "type=17 value=0180c200000e"), 6);
    check_eq!(e, "t_copp.cpp:69", count(&log, "type=18 value=88cc"), 18);
    check!(
        e,
        "t_copp.cpp:70",
        has(&log, "type=ctc hostif=0x") && has(&log, "green=1 yellow=1 red=0 dirty=f")
    );
    check_eq!(
        e,
        "t_copp.cpp:71",
        count(&log, "type=ctc_override green=0 yellow=0 red=1"),
        18
    );
    check_eq!(e, "t_copp.cpp:72", count(&log, "type=drop green=1 yellow=1 red=1"), 18);
    // ctc, override, drop, meter, counter
    check_eq!(e, "t_copp.cpp:73", count(&log, "matches=2 actions=5"), 18);
    check_eq!(e, "t_copp.cpp:74", e.m().acl_aces(), 18);
    check_eq!(e, "t_copp.cpp:75", e.m().acl_meters(), 6);

    // policer CIR change: every meter of it updated, entries kept
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_POLICER, pid, a_u64(sys::SAI_POLICER_ATTR_CIR, 10));
    ok!(e, "t_copp.cpp:81", st);
    check_eq!(
        e,
        "t_copp.cpp:82",
        count(&e.m().log(), "ifcs_acl_meter_attr_set") >= 6,
        true
    );
    check!(e, "t_copp.cpp:83", has(&e.m().log(), "id=7 u64=6720"));
    check_eq!(e, "t_copp.cpp:84", e.m().acl_aces(), 18);

    // action DROP: entries re-installed without the copy
    e.m().log_clear();
    let st = e.set(
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP,
        lldp,
        a_s32(
            sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    );
    ok!(e, "t_copp.cpp:90", st);
    check_eq!(e, "t_copp.cpp:91", count(&e.m().log(), "type=ctc "), 0);
    check_eq!(e, "t_copp.cpp:92", count(&e.m().log(), "type=drop green=1"), 18);
    check_eq!(e, "t_copp.cpp:93", e.m().acl_aces(), 18);
    let st = e.set(
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP,
        lldp,
        a_s32(
            sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_TRAP as i32,
        ),
    );
    ok!(e, "t_copp.cpp:95", st);

    // ---- TTL error: system trap 17 (queue + enable) + one entry on it; MTU: no entry
    let (st, grp1) = e.create(
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP,
        &[a_u32(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, 1)],
    );
    ok!(e, "t_copp.cpp:102", st);
    e.m().log_clear();
    let mut aces = e.m().acl_aces();
    make_trap(
        e,
        sys::SAI_HOSTIF_TRAP_TYPE_TTL_ERROR,
        sys::SAI_PACKET_ACTION_TRAP,
        grp1,
    );
    check!(
        e,
        "t_copp.cpp:106",
        has(&e.m().log(), "ifcs_hostif_trap_attr_set h=0x18200011 id=0 u32=1")
    );
    check!(
        e,
        "t_copp.cpp:107",
        has(&e.m().log(), "ifcs_hostif_trap_attr_set h=0x18200011 id=1 u32=1")
    );
    check!(
        e,
        "t_copp.cpp:108",
        has(&e.m().log(), "type=98 value= object=0x18200011")
    );
    check_eq!(e, "t_copp.cpp:109", e.m().acl_aces(), aces + 6);
    // no policer in this group
    check_eq!(e, "t_copp.cpp:110", e.m().acl_meters(), 6);
    e.m().log_clear();
    make_trap(
        e,
        sys::SAI_HOSTIF_TRAP_TYPE_L3_MTU_ERROR,
        sys::SAI_PACKET_ACTION_TRAP,
        grp1,
    );
    check!(
        e,
        "t_copp.cpp:113",
        has(&e.m().log(), "ifcs_hostif_trap_attr_set h=0x18200017 id=1 u32=1")
    );
    check_eq!(e, "t_copp.cpp:114", e.m().acl_aces(), aces + 6);

    // ---- IP2ME: the TRAP routes' hostif trap gets the group queue; BGP = ip2me + port 179
    e.m().log_clear();
    make_trap(e, sys::SAI_HOSTIF_TRAP_TYPE_IP2ME, sys::SAI_PACKET_ACTION_TRAP, grp1);
    let l2 = e.m().log();
    let q = l2.find("ifcs_hostif_trap_attr_set h=");
    check!(e, "t_copp.cpp:121", q.is_some_and(|q| l2[q..].contains("id=0 u32=1")));
    let ip2me: String = q
        .map_or("", |q| l2.get(q + 28..).unwrap_or(""))
        .chars()
        .take(10)
        .collect();
    // ip2me.substr(0, ip2me.find(' ')): up to the first space
    let ip2me_h = ip2me.split(' ').next().unwrap_or("").to_owned();
    check!(
        e,
        "t_copp.cpp:123",
        has(&l2, &format!("type=98 value= object={ip2me_h}"))
    );
    e.m().log_clear();
    make_trap(e, sys::SAI_HOSTIF_TRAP_TYPE_BGP, sys::SAI_PACKET_ACTION_TRAP, grp);
    // dst port 179
    check_eq!(e, "t_copp.cpp:126", count(&e.m().log(), "type=60 value=00b3"), 6);
    // src port 179
    check_eq!(e, "t_copp.cpp:127", count(&e.m().log(), "type=59 value=00b3"), 6);
    check_eq!(
        e,
        "t_copp.cpp:128",
        count(&e.m().log(), &format!("type=98 value= object={ip2me_h}")),
        12
    );

    // ---- SAMPLEPACKET makes no CoPP entry (tl_sflow: system trap + the samples' queue meter, t_sflow.cpp)
    e.m().log_clear();
    aces = e.m().acl_aces();
    make_trap(
        e,
        sys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET,
        sys::SAI_PACKET_ACTION_TRAP,
        grp,
    );
    check_eq!(e, "t_copp.cpp:134", e.m().acl_aces(), aces);
    check!(e, "t_copp.cpp:135", !has(&e.m().log(), "ifcs_acl_"));

    // ---- IGMP / MLD: ether type + protocol + message type in L4 byte 1, as isai's ACEs
    e.m().log_clear();
    let igmp = make_trap(
        e,
        sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY,
        sys::SAI_PACKET_ACTION_TRAP,
        grp1,
    );
    check_eq!(e, "t_copp.cpp:140", e.m().acl_aces(), aces + 6);
    check_eq!(e, "t_copp.cpp:141", count(&e.m().log(), "type=18 value=0800"), 6);
    check_eq!(e, "t_copp.cpp:142", count(&e.m().log(), "type=56 value=02"), 6);
    check_eq!(e, "t_copp.cpp:143", count(&e.m().log(), "type=61 value=11"), 6);
    e.m().log_clear();
    make_trap(
        e,
        sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT,
        sys::SAI_PACKET_ACTION_TRAP,
        grp1,
    );
    check_eq!(e, "t_copp.cpp:146", count(&e.m().log(), "type=61 value=22"), 6);
    e.m().log_clear();
    make_trap(
        e,
        sys::SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT,
        sys::SAI_PACKET_ACTION_TRAP,
        grp1,
    );
    check_eq!(e, "t_copp.cpp:149", count(&e.m().log(), "type=18 value=86dd"), 6);
    check_eq!(e, "t_copp.cpp:150", count(&e.m().log(), "type=56 value=3a"), 6);
    check_eq!(e, "t_copp.cpp:151", count(&e.m().log(), "type=61 value=8f"), 6);
    make_trap(
        e,
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2,
        sys::SAI_PACKET_ACTION_TRAP,
        grp1,
    );
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, igmp);
    ok!(e, "t_copp.cpp:153", st);

    // ---- NEIGHBOR_MISS: a hostif trap of its own (glean), the group's queue, an ACE on it
    e.m().log_clear();
    let nm = make_trap(
        e,
        sys::SAI_HOSTIF_TRAP_TYPE_NEIGHBOR_MISS,
        sys::SAI_PACKET_ACTION_TRAP,
        grp1,
    );
    let l3 = e.m().log();
    // the glean trap exists already when L3 made glean routes (t_l3): then only its queue is set
    // (the text after the marker up to the next space, as substr(pos, find(' ') - pos))
    let word_after = |at: usize| -> String {
        let rest = l3.get(at..).unwrap_or("");
        rest[..rest.find(' ').unwrap_or(rest.len())].to_owned()
    };
    let mut glean = l3
        .find("ifcs_hostif_trap_create h=")
        .map_or_else(String::new, |gq| word_after(gq + 26));
    if glean.is_empty() {
        if let Some(q) = l3.find("ifcs_hostif_trap_attr_set h=") {
            glean = word_after(q + 28);
        }
    }
    check!(e, "t_copp.cpp:166", !glean.is_empty());
    check!(
        e,
        "t_copp.cpp:167",
        has(&l3, &format!("ifcs_hostif_trap_attr_set h={glean} id=0 u32=1"))
    );
    check_eq!(
        e,
        "t_copp.cpp:168",
        count(&l3, &format!("type=98 value= object={glean}")),
        6
    );
    check!(e, "t_copp.cpp:169", glean != ip2me_h);
    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, nm);
    ok!(e, "t_copp.cpp:171", st);
    // back to queue 0, kept
    check!(
        e,
        "t_copp.cpp:172",
        has(&e.m().log(), &format!("ifcs_hostif_trap_attr_set h={glean} id=0 u32=0"))
    );
    check!(e, "t_copp.cpp:173", !has(&e.m().log(), "ifcs_hostif_trap_delete"));

    // ---- diag "copp": the CPU queue counters (which queue a trap's frames used)
    // (t_counters.cpp reads it through the diag socket)

    // ---- group queue change: the trap follows (attr_set on its hostif trap)
    e.m().log_clear();
    let st = e.set(
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP,
        grp,
        a_u32(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, 5),
    );
    ok!(e, "t_copp.cpp:182", st);
    check!(
        e,
        "t_copp.cpp:183",
        has(&e.m().log(), "ifcs_hostif_trap_attr_set") && has(&e.m().log(), "id=0 u32=5")
    );

    // ---- remove LLDP: its 18 entries and its hostif trap go
    aces = e.m().acl_aces();
    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, lldp);
    ok!(e, "t_copp.cpp:188", st);
    check_eq!(e, "t_copp.cpp:189", e.m().acl_aces(), aces - 18);
    check!(
        e,
        "t_copp.cpp:190",
        has(&e.m().log(), "ifcs_acl_ace_remove") && has(&e.m().log(), "ifcs_hostif_trap_delete")
    );
    // a used policer cannot go; the meters stay with it
    check!(
        e,
        "t_copp.cpp:192",
        e.remove(sys::SAI_OBJECT_TYPE_POLICER, pid) != SUCCESS
    );
    let _ = s;
}
