//! t_sflow.cpp: sFlow samples to psample on the mock libifcs and the mock ipd ioctls: a GENETLINK
//! host interface alone reaches no driver; the SAMPLEPACKET table entry creates the driver's sFlow
//! netlink (sample GRE protocol, then the netlink, then every sampled port's rate); sampler
//! changes push the rate; refusals (other trap types, a second sFlow netlink, failing ioctls);
//! removal deletes the netlink; the SAMPLEPACKET trap: system trap MIRROR_TO_CPU (isai) and its
//! group's policer as the meter of the samples' CPU queue, following policer, group, queue and
//! action changes.
use crate::support::*;

const EPERM: i32 = 1;
const EEXIST: i32 = 17;

/// The SAMPLEPACKET trap (t_copp.cpp keeps one): ours if creating works, else found among the
/// hostif trap ids below a fresh one.
fn samplepacket_trap(e: &mut Env, grp: u64, ours: &mut bool) -> u64 {
    let mut t = [
        a_s32(
            sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE,
            sys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET as i32,
        ),
        a_s32(
            sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_TRAP as i32,
        ),
        a_oid(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, grp),
    ];
    let (st, id) = e.create(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, &t);
    *ours = st == SUCCESS;
    if *ours {
        return id;
    }
    check_eq!(e, "t_sflow.cpp:32", st, sys::SAI_STATUS_ITEM_ALREADY_EXISTS);
    t[0] = a_s32(
        sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE,
        sys::SAI_HOSTIF_TRAP_TYPE_PIM as i32,
    ); // unmapped: kept, not programmed
    let (st, probe) = e.create(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, &t);
    ok!(e, "t_sflow.cpp:35", st);
    let mut found = 0u64;
    let mut o = probe.wrapping_sub(1);
    while o.wrapping_add(4096) > probe && (o & 0xffff_ffff_ffff) != 0 {
        let (st, a) = e.get1(
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP,
            o,
            sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE,
            Value::S32(0),
        );
        if st == SUCCESS && a.s32() == sys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET as i32 {
            found = o;
            break;
        }
        o = o.wrapping_sub(1);
    }
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, probe);
    ok!(e, "t_sflow.cpp:45", st);
    check!(e, "t_sflow.cpp:46", found != 0);
    found
}

fn genl_hostif(e: &Env, name: &str, grp: Option<&str>, id: &mut u64) -> Status {
    let mut h = vec![
        a_s32(sys::SAI_HOSTIF_ATTR_TYPE, sys::SAI_HOSTIF_TYPE_GENETLINK as i32),
        a_text(sys::SAI_HOSTIF_ATTR_NAME, name),
    ];
    if let Some(g) = grp {
        h.push(a_text(sys::SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME, g));
    }
    let (st, o) = e.create(sys::SAI_OBJECT_TYPE_HOSTIF, &h);
    if o != 0 {
        *id = o;
    }
    st
}

fn table_entry(e: &Env, trap: u64, hif: u64, id: &mut u64) -> Status {
    let t = [
        a_s32(
            sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE,
            sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_TRAP_ID as i32,
        ),
        a_oid(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID, trap),
        a_s32(
            sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE,
            sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK as i32,
        ),
        a_oid(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF, hif),
    ];
    let (st, o) = e.create(sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY, &t);
    if o != 0 {
        *id = o;
    }
    st
}

pub fn test_sflow(e: &mut Env) {
    e.ck.section("sflow");
    let (p1, p2) = (e.p1, e.p2);
    let st = e.sai.query(Api::Samplepacket);
    ok!(e, "t_sflow.cpp:90", st);
    let ingress = sys::SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
    let egress = sys::SAI_PORT_ATTR_EGRESS_SAMPLEPACKET_ENABLE;

    // ---- the genetlink host interface alone: recorded, no driver call
    let (mut gh, mut gx) = (0u64, 0u64);
    check_eq!(
        e,
        "t_sflow.cpp:95",
        genl_hostif(e, "psample", None, &mut gx),
        sys::SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING
    ); // no group (frontend)
    let calls0 = e.m().ipd_calls();
    ok!(e, "t_sflow.cpp:97", genl_hostif(e, "psample", Some("packets"), &mut gh));
    check_eq!(e, "t_sflow.cpp:98", e.m().ipd_calls(), calls0);
    check_eq!(
        e,
        "t_sflow.cpp:99",
        genl_hostif(e, "psample", Some("packets"), &mut gx),
        sys::SAI_STATUS_ITEM_ALREADY_EXISTS
    );
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_HOSTIF,
        gh,
        sys::SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME,
        Value::Chardata(vec![0; 32]),
    );
    ok!(e, "t_sflow.cpp:101", st);
    check!(e, "t_sflow.cpp:102", a.text() == "packets");

    // ---- trap group (queue 2, as SONiC's sflow group) + the SAMPLEPACKET trap
    let (st, grp) = e.create(
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP,
        &[a_u32(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, 2)],
    );
    ok!(e, "t_sflow.cpp:109", st);
    let mut own_trap = false;
    let trap = samplepacket_trap(e, grp, &mut own_trap);

    // ---- a sampler on p1 before the table entry: policy only, no rate push
    let mut sp = a_u32(sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, 1000);
    let (st, smp) = e.create(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, std::slice::from_ref(&sp));
    ok!(e, "t_sflow.cpp:118", st);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(ingress, smp));
    ok!(e, "t_sflow.cpp:122", st);
    check!(
        e,
        "t_sflow.cpp:123",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=25 u32=0")
    );
    check!(e, "t_sflow.cpp:124", !has(&e.m().log(), "ipd_ioctl"));

    // ---- the table entry: GRE protocol, the netlink, p1's rate, in that order
    e.m().log_clear();
    let mut te = 0u64;
    ok!(e, "t_sflow.cpp:129", table_entry(e, trap, gh, &mut te));
    let mut log = e.m().log();
    let npos = usize::MAX;
    let a1 = log.find("ipd_ioctl SET_PARAMS shim_gre_proto=0x8818").unwrap_or(npos);
    let a2 = log
        .find("ipd_ioctl CREATE_NETLINK type=16 app=1 name=psample mcgrp=packets")
        .unwrap_or(npos);
    let a3 = log
        .find("ipd_ioctl SET_NETDEV id=8 sysport=1 rate=1000")
        .unwrap_or(npos);
    check!(e, "t_sflow.cpp:134", a1 != npos && a2 != npos && a3 != npos);
    check!(e, "t_sflow.cpp:135", a1 < a2 && a2 < a3);
    check_eq!(e, "t_sflow.cpp:136", count(&log, "ipd_ioctl"), 3usize);

    // ---- sampler changes while active: the rate follows (update, second port, off)
    e.m().log_clear();
    sp = a_u32(sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, 500);
    let st = e.set(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, smp, sp.clone());
    ok!(e, "t_sflow.cpp:141", st);
    check!(
        e,
        "t_sflow.cpp:142",
        has(&e.m().log(), "ipd_ioctl SET_NETDEV id=8 sysport=1 rate=500")
    );
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a_oid(ingress, smp));
    ok!(e, "t_sflow.cpp:144", st);
    check!(
        e,
        "t_sflow.cpp:145",
        has(&e.m().log(), "ipd_ioctl SET_NETDEV id=8 sysport=2 rate=500")
    );
    e.m().log_clear();
    sp = a_u32(sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, 4096);
    let st = e.set(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, smp, sp.clone());
    ok!(e, "t_sflow.cpp:148", st);
    check_eq!(
        e,
        "t_sflow.cpp:149",
        count(&e.m().log(), "ipd_ioctl SET_NETDEV id=8"),
        2usize
    );
    check!(
        e,
        "t_sflow.cpp:150",
        has(&e.m().log(), "sysport=1 rate=4096") && has(&e.m().log(), "sysport=2 rate=4096")
    );
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(ingress, 0));
    ok!(e, "t_sflow.cpp:153", st);
    check!(
        e,
        "t_sflow.cpp:154",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=25 u32=1")
            && has(&e.m().log(), "ipd_ioctl SET_NETDEV id=8 sysport=1 rate=0")
    );
    // a driver without the rate ioctl (unpatched, legacy mode): sampling still works
    e.m().ipd_fail(26, EPERM);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(ingress, smp));
    ok!(e, "t_sflow.cpp:159", st);
    check!(
        e,
        "t_sflow.cpp:160",
        has(&e.m().log(), "id=25 u32=0") && has(&e.m().log(), "SET_NETDEV id=8 sysport=1 rate=4096")
    );
    // egress sampling stays refused (no rate pushed for it)
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(egress, smp));
    check_eq!(e, "t_sflow.cpp:164", st, sys::SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    check!(e, "t_sflow.cpp:165", !has(&e.m().log(), "ipd_ioctl"));
    // with node EGRESS_SFLOW_ENABLE (SAI_TERALYNX_EGRESS_SFLOW=on at init): an egress collector,
    // SFLOW_EGRESS (26) on the sysport, the egress rate (id 9) to the driver; off again
    e.m().sflow_egress(1);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(egress, smp));
    ok!(e, "t_sflow.cpp:170", st);
    log = e.m().log();
    check!(
        e,
        "t_sflow.cpp:172",
        has(&log, "ifcs_collector_create") && has(&log, "ifcs_sysport_attr_set sp=0x3e200001 id=26 u32=0")
    );
    check!(
        e,
        "t_sflow.cpp:173",
        has(&log, "ipd_ioctl SET_NETDEV id=9 sysport=1 rate=4096")
    );
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(egress, 0));
    ok!(e, "t_sflow.cpp:176", st);
    check!(
        e,
        "t_sflow.cpp:177",
        has(&e.m().log(), "id=26 u32=1")
            && has(&e.m().log(), "SET_NETDEV id=9 sysport=1 rate=0")
            && has(&e.m().log(), "ifcs_collector_delete")
    );
    e.m().sflow_egress(0);

    // ---- refusals: another trap type to a genetlink hostif; a second sFlow netlink
    let mut te2 = 0u64;
    let t = [
        a_s32(
            sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE,
            sys::SAI_HOSTIF_TRAP_TYPE_OSPF as i32,
        ), // unmapped: kept
        a_s32(
            sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_TRAP as i32,
        ),
        a_oid(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, grp),
    ];
    let (st, ttl) = e.create(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, &t);
    ok!(e, "t_sflow.cpp:191", st);
    check_eq!(
        e,
        "t_sflow.cpp:193",
        table_entry(e, ttl, gh, &mut te2),
        sys::SAI_STATUS_NOT_SUPPORTED
    );
    let mut gh2 = 0u64;
    ok!(
        e,
        "t_sflow.cpp:195",
        genl_hostif(e, "psample2", Some("packets"), &mut gh2)
    );
    e.m().log_clear();
    check_eq!(
        e,
        "t_sflow.cpp:197",
        table_entry(e, trap, gh2, &mut te2),
        sys::SAI_STATUS_ITEM_ALREADY_EXISTS
    );
    check!(e, "t_sflow.cpp:198", !has(&e.m().log(), "ipd_ioctl"));
    // the table entry uses it (frontend)
    check_eq!(
        e,
        "t_sflow.cpp:199",
        e.remove(sys::SAI_OBJECT_TYPE_HOSTIF, gh),
        sys::SAI_STATUS_OBJECT_IN_USE
    );

    // ---- removing the entry deletes the netlink; rates are no longer pushed
    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY, te);
    ok!(e, "t_sflow.cpp:203", st);
    check!(
        e,
        "t_sflow.cpp:204",
        has(
            &e.m().log(),
            "ipd_ioctl DELETE_NETLINK type=16 app=1 name=psample mcgrp=packets"
        )
    );
    e.m().log_clear();
    sp = a_u32(sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, 2000);
    let st = e.set(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, smp, sp.clone());
    ok!(e, "t_sflow.cpp:207", st);
    check!(e, "t_sflow.cpp:208", !has(&e.m().log(), "ipd_ioctl"));

    // ---- the second hostif can take over; a failing CREATE_NETLINK leaves nothing active
    e.m().ipd_fail(27, EEXIST);
    e.m().log_clear();
    check!(e, "t_sflow.cpp:213", table_entry(e, trap, gh2, &mut te2) != SUCCESS);
    check!(
        e,
        "t_sflow.cpp:214",
        has(&e.m().log(), "CREATE_NETLINK") && !has(&e.m().log(), "SET_NETDEV")
    );
    e.m().log_clear();
    ok!(e, "t_sflow.cpp:216", table_entry(e, trap, gh2, &mut te2));
    log = e.m().log();
    check!(
        e,
        "t_sflow.cpp:218",
        has(&log, "CREATE_NETLINK type=16 app=1 name=psample2")
            && has(&log, "SET_NETDEV id=8 sysport=1 rate=2000")
            && has(&log, "SET_NETDEV id=8 sysport=2 rate=2000")
    );
    // removing the hostif with its entry gone deletes the netlink (once)
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY, te2);
    ok!(e, "t_sflow.cpp:221", st);
    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF, gh2);
    ok!(e, "t_sflow.cpp:223", st);
    check!(e, "t_sflow.cpp:224", !has(&e.m().log(), "ipd_ioctl"));
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF, gh);
    ok!(e, "t_sflow.cpp:225", st);

    // ---- sample policing: SONiC's sflow group (queue 2, 1000 pps sr_tcm, red drop) on the trap.
    //      The samples' queue is 2 (test profile SAI_TERALYNX_SFLOW_CPU_QUEUE; the mock has no node
    //      TRAFFIC_MONITOR_CPU_QUEUE): CPU queue 2's token-bucket meter, 1000 x 84 B x 8 = 672 kbit/s, 84000 B
    let mut pp = [
        a_s32(sys::SAI_POLICER_ATTR_METER_TYPE, sys::SAI_METER_TYPE_PACKETS as i32),
        a_s32(sys::SAI_POLICER_ATTR_MODE, sys::SAI_POLICER_MODE_SR_TCM as i32),
        a_u64(sys::SAI_POLICER_ATTR_CIR, 1000),
        a_u64(sys::SAI_POLICER_ATTR_CBS, 1000),
        a_s32(
            sys::SAI_POLICER_ATTR_RED_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let (st, pol) = e.create(sys::SAI_OBJECT_TYPE_POLICER, &pp);
    ok!(e, "t_sflow.cpp:237", st);
    let gg = [
        a_u32(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, 2),
        a_oid(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER, pol),
    ];
    let (st, gp) = e.create(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, &gg);
    ok!(e, "t_sflow.cpp:242", st);
    // ifcs_hostif_trap_attr_set on system trap 512 (MIRROR_TO_CPU), ENABLE
    let m2c = "00200 id=1 u32=";
    let mut ta = a_oid(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, gp);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, trap, ta.clone());
    ok!(e, "t_sflow.cpp:248", st);
    log = e.m().log();
    check!(e, "t_sflow.cpp:250", !has(&log, "id=25 ")); // METER_MODE: refused by the SDK, not set
    check!(
        e,
        "t_sflow.cpp:251",
        has(&log, "ifcs_cpu_queue_attr_set q=2 id=28 u32=672")
    ); // CIR, kbit/s
    check!(
        e,
        "t_sflow.cpp:252",
        has(&log, "ifcs_cpu_queue_attr_set q=2 id=29 u32=84000")
    ); // CBS, bytes
    check!(
        e,
        "t_sflow.cpp:253",
        has(&log, "ifcs_cpu_queue_attr_set q=2 id=27 u32=1")
    ); // CIR enable
    // enabled last
    check!(
        e,
        "t_sflow.cpp:254",
        log.find("id=28 u32=672").unwrap_or(npos) < log.find("q=2 id=27 u32=1").unwrap_or(npos)
    );
    // no ACL objects (0.12.41: no MIRROR match)
    check!(e, "t_sflow.cpp:255", !has(&log, "ifcs_acl_") && !has(&log, "ifcs_ace_"));
    // policer update: the queue meter follows
    e.m().log_clear();
    pp[2] = a_u64(sys::SAI_POLICER_ATTR_CIR, 2000);
    let st = e.set(sys::SAI_OBJECT_TYPE_POLICER, pol, pp[2].clone());
    ok!(e, "t_sflow.cpp:259", st);
    check!(
        e,
        "t_sflow.cpp:260",
        has(&e.m().log(), "ifcs_cpu_queue_attr_set q=2 id=28 u32=1344")
    );
    // group policer off: meter off; on again: back
    let mut gpol = a_oid(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER, 0);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, gp, gpol.clone());
    ok!(e, "t_sflow.cpp:266", st);
    check!(
        e,
        "t_sflow.cpp:267",
        has(&e.m().log(), "ifcs_cpu_queue_attr_set q=2 id=27 u32=0") && !has(&e.m().log(), "id=27 u32=1")
    );
    gpol = a_oid(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER, pol);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, gp, gpol.clone());
    ok!(e, "t_sflow.cpp:270", st);
    check!(
        e,
        "t_sflow.cpp:271",
        has(&e.m().log(), "ifcs_cpu_queue_attr_set q=2 id=28 u32=1344") && has(&e.m().log(), "q=2 id=27 u32=1")
    );
    // action DROP: MIRROR_TO_CPU off (isai); TRAP: on again
    let mut act = a_s32(
        sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
        sys::SAI_PACKET_ACTION_DROP as i32,
    );
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, trap, act.clone());
    ok!(e, "t_sflow.cpp:277", st);
    check!(e, "t_sflow.cpp:278", has(&e.m().log(), &format!("{m2c}0")));
    act = a_s32(
        sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
        sys::SAI_PACKET_ACTION_TRAP as i32,
    );
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, trap, act.clone());
    ok!(e, "t_sflow.cpp:281", st);
    check!(e, "t_sflow.cpp:282", has(&e.m().log(), &format!("{m2c}1")));
    // samplers do not touch the meter
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(ingress, 0));
    ok!(e, "t_sflow.cpp:287", st);
    check!(e, "t_sflow.cpp:288", !has(&e.m().log(), "ifcs_cpu_queue_attr_set"));
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(ingress, smp));
    ok!(e, "t_sflow.cpp:290", st);
    // the group's queue is not the samples' queue: no meter there, ours goes
    let mut gq = a_u32(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, 3);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, gp, gq.clone());
    ok!(e, "t_sflow.cpp:296", st);
    check!(
        e,
        "t_sflow.cpp:297",
        has(&e.m().log(), "ifcs_cpu_queue_attr_set q=2 id=27 u32=0") && !has(&e.m().log(), "q=3 ")
    );
    gq = a_u32(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, 2);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, gp, gq.clone());
    ok!(e, "t_sflow.cpp:300", st);
    check!(e, "t_sflow.cpp:301", has(&e.m().log(), "q=2 id=27 u32=1"));
    // the trap back to its first group; ours and its policer go (meter off)
    ta = a_oid(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, grp);
    e.m().log_clear();
    let st = e.set(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, trap, ta.clone());
    ok!(e, "t_sflow.cpp:305", st);
    check!(
        e,
        "t_sflow.cpp:306",
        has(&e.m().log(), "ifcs_cpu_queue_attr_set q=2 id=27 u32=0")
    );
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, gp);
    ok!(e, "t_sflow.cpp:307", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_POLICER, pol);
    ok!(e, "t_sflow.cpp:308", st);

    // ---- clean up: samplers off, objects gone
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p1, a_oid(ingress, 0));
    ok!(e, "t_sflow.cpp:313", st);
    let st = e.set(sys::SAI_OBJECT_TYPE_PORT, p2, a_oid(ingress, 0));
    ok!(e, "t_sflow.cpp:314", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_SAMPLEPACKET, smp);
    ok!(e, "t_sflow.cpp:315", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, ttl);
    ok!(e, "t_sflow.cpp:316", st);
    if own_trap {
        let st = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, trap);
        ok!(e, "t_sflow.cpp:317", st);
    }
    let rg = e.remove(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, grp);
    check!(
        e,
        "t_sflow.cpp:319",
        rg == SUCCESS || (!own_trap && rg == sys::SAI_STATUS_OBJECT_IN_USE)
    );
}
