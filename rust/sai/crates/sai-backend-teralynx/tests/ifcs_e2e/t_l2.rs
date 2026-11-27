//! t_l2.cpp: L2: VLANs, members (per-member mc_dests, the CPU in no VLAN), FDB entries and events,
//! flush by port / VLAN / all, aging.
use crate::support::*;

fn fdb_key(e: &Env, vlan: u64, mac: [u8; 6]) -> Entry {
    let _ = e;
    Entry::Fdb { bv: vlan, mac }
}

pub fn test_l2(e: &mut Env) {
    e.ck.section("l2");
    let (cpu_sp, sp1, sp2) = (0x3e2003e8u32, 0x3e200001u32, 0x3e200002u32);
    let (p1, p2, s) = (e.p1, e.p2, e.s);

    // ---- VLAN 100: L2VNI with STP, flood group, learning on (isai's attributes); the CPU is in no VLAN
    e.m().log_clear();
    let (st, vlan) = e.create(sys::SAI_OBJECT_TYPE_VLAN, &[a_u16(sys::SAI_VLAN_ATTR_VLAN_ID, 100)]);
    e.vlan = vlan;
    ok!(e, "t_l2.cpp:62", st);
    check!(
        e,
        "t_l2.cpp:63",
        has(&e.m().log(), "ifcs_l2vni_create vni=100 count=4 stp=0x100")
    );
    check!(e, "t_l2.cpp:64", e.m().l2vni_has(100, cpu_sp) == 0);
    check!(e, "t_l2.cpp:65", !has(&e.m().log(), "ifcs_mc_dest_create"));

    // ---- port 1 untagged: L2VNI member + its own mc_dest in the flood group + untagged egress
    e.bp1 = find_bridge_port(e, p1);
    let bp1 = e.bp1;
    check!(e, "t_l2.cpp:69", bp1 != 0);
    let member = make_vlan_member(e, vlan, bp1, false);
    check!(e, "t_l2.cpp:71", member != 0);
    check!(e, "t_l2.cpp:72", e.m().l2vni_has(100, sp1) != 0);
    check!(
        e,
        "t_l2.cpp:73",
        has(&e.m().log(), "ifcs_mc_dest_create h=0x") && has(&e.m().log(), "dest=0x3e200001")
    );
    check!(e, "t_l2.cpp:74", has(&e.m().log(), "ifcs_mdg_mc_dest_add mdg="));
    check!(
        e,
        "t_l2.cpp:75",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=43 u32=100")
    );

    // ---- port 2 tagged in VLAN 100 and in VLAN 101; untagged in a second VLAN is refused (one per port)
    let bp2 = find_bridge_port(e, p2);
    check!(e, "t_l2.cpp:79", bp2 != 0);
    let (st, v101) = e.create(sys::SAI_OBJECT_TYPE_VLAN, &[a_u16(sys::SAI_VLAN_ATTR_VLAN_ID, 101)]);
    ok!(e, "t_l2.cpp:82", st);
    let m2 = make_vlan_member(e, vlan, bp2, true);
    let m2b = make_vlan_member(e, v101, bp2, true);
    check!(
        e,
        "t_l2.cpp:85",
        m2 != 0 && m2b != 0 && e.m().l2vni_has(100, sp2) != 0 && e.m().l2vni_has(101, sp2) != 0
    );
    // port 1 is untagged in 100
    check!(e, "t_l2.cpp:86", make_vlan_member(e, v101, bp1, false) == 0);
    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN_MEMBER, m2b);
    ok!(e, "t_l2.cpp:88", st);
    check!(
        e,
        "t_l2.cpp:89",
        e.m().l2vni_has(101, sp2) == 0 && e.m().l2vni_has(100, sp2) != 0
    );
    check!(
        e,
        "t_l2.cpp:90",
        has(&e.m().log(), "ifcs_mdg_mc_dest_remove")
            && has(&e.m().log(), "ifcs_mc_dest_delete")
            && has(&e.m().log(), "ifcs_l2vni_member_remove vni=101 n=1 first=0x3e200002")
    );
    let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN, v101);
    ok!(e, "t_l2.cpp:92", st);
    check!(e, "t_l2.cpp:93", has(&e.m().log(), "ifcs_l2vni_delete vni=101"));

    // ---- FDB events from the SDK -> SAI notifications (learned, aged)
    let mac = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
    e.with_events(|ev| ev.fdb.clear());
    e.m().fire_l2(0, &mac, 100, 1);
    e.m().fire_l2(1, &mac, 100, 1);
    check!(e, "t_l2.cpp:103", e.wait_for(|ev| ev.fdb.len() >= 2));
    let evs = e.with_events(|ev| ev.fdb.clone());
    check!(
        e,
        "t_l2.cpp:106",
        evs.len() >= 2
            && evs[0].event == sys::SAI_FDB_EVENT_LEARNED as i32
            && evs[0].bv == vlan
            && evs[1].event == sys::SAI_FDB_EVENT_AGED as i32
    );
    e.with_events(|ev| ev.fdb.clear());

    // ---- static / dynamic / drop entries; flush by bridge port, by VLAN, all
    let ma = [0x02, 0, 0, 0, 0, 0x91];
    let mb = [0x02, 0, 0, 0, 0, 0x92];
    let mc = [0x02, 0, 0, 0, 0, 0x93];
    let mut fa_ = [
        a_s32(sys::SAI_FDB_ENTRY_ATTR_TYPE, sys::SAI_FDB_ENTRY_TYPE_STATIC as i32),
        a_oid(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, bp1),
        a_s32(
            sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let (fea, feb, fec) = (fdb_key(e, vlan, ma), fdb_key(e, vlan, mb), fdb_key(e, vlan, mc));
    let l2_0 = e.m().l2_count();
    let st = e.sai.entry(EntryOp::Create, s, &fea, &fa_[..2]);
    ok!(e, "t_l2.cpp:122", st);
    check_eq!(e, "t_l2.cpp:123", e.m().l2_dest(&ma, 100), sp1);
    e.m().log_clear();
    // drop: FWD_POLICY added
    let st = e.sai.entry(EntryOp::Create, s, &fec, &fa_[..3]);
    ok!(e, "t_l2.cpp:125", st);
    check!(
        e,
        "t_l2.cpp:126",
        has(&e.m().log(), "vni=100 count=3 dest=0x3e200001 etype=0")
    );
    fa_[0] = a_s32(sys::SAI_FDB_ENTRY_ATTR_TYPE, sys::SAI_FDB_ENTRY_TYPE_DYNAMIC as i32);
    fa_[1] = a_oid(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, bp2);
    let st = e.sai.entry(EntryOp::Create, s, &feb, &fa_[..2]);
    ok!(e, "t_l2.cpp:129", st);
    check_eq!(e, "t_l2.cpp:130", e.m().l2_dest(&mb, 100), sp2);
    check_eq!(e, "t_l2.cpp:131", e.m().l2_count(), l2_0 + 3);
    let mut fl = [
        a_oid(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, bp2),
        a_s32(
            sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
            sys::SAI_FDB_FLUSH_ENTRY_TYPE_STATIC as i32,
        ),
    ];
    // port 2 has only a dynamic one
    let st = e.sai.flush_fdb(s, &fl);
    ok!(e, "t_l2.cpp:137", st);
    check_eq!(e, "t_l2.cpp:138", e.m().l2_count(), l2_0 + 3);
    fl[1] = a_s32(
        sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
        sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC as i32,
    );
    let st = e.sai.flush_fdb(s, &fl);
    ok!(e, "t_l2.cpp:140", st);
    check_eq!(e, "t_l2.cpp:141", e.m().l2_count(), l2_0 + 2);
    check_eq!(e, "t_l2.cpp:142", e.m().l2_dest(&mb, 100), 0u32);
    fl[0] = a_oid(sys::SAI_FDB_FLUSH_ATTR_BV_ID, vlan);
    fl[1] = a_s32(
        sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
        sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL as i32,
    );
    let st = e.sai.flush_fdb(s, &fl);
    ok!(e, "t_l2.cpp:146", st);
    check_eq!(e, "t_l2.cpp:147", e.m().l2_count(), l2_0);
    // fa_ is DYNAMIC on bp2 now (what the C++ leaves in its array)
    let st = e.sai.entry(EntryOp::Create, s, &fea, &fa_[..2]);
    ok!(e, "t_l2.cpp:148", st);
    let a = a_s32(
        sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
        sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL as i32,
    );
    let st = e.sai.flush_fdb(s, &[a]);
    ok!(e, "t_l2.cpp:151", st);
    check_eq!(e, "t_l2.cpp:152", e.m().l2_count(), 0);
    let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN_MEMBER, m2);
    ok!(e, "t_l2.cpp:153", st);
    check!(e, "t_l2.cpp:154", e.m().l2vni_has(100, sp2) == 0);

    // ---- aging (seconds)
    let st = e.set(
        sys::SAI_OBJECT_TYPE_SWITCH,
        s,
        a_u32(sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, 600),
    );
    ok!(e, "t_l2.cpp:159", st);
    check_eq!(e, "t_l2.cpp:160", e.m().aging(), 600u32);
    let _ = member;
}
