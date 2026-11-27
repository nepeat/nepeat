//! t_lag.cpp: LAG: create, members (egress / ingress disable as teamd sets them), the LAG as
//! bridge port (VLAN members, PVID, FDB entries and events, flush) and as port RIF (internal
//! VLAN), member join / leave while in a VLAN, removal; the hash profile keys.
use crate::support::*;

fn make_member(e: &Env, lag: u64, port: u64, egress_disable: bool, ingress_disable: bool) -> u64 {
    let (st, o) = e.create(
        sys::SAI_OBJECT_TYPE_LAG_MEMBER,
        &[
            a_oid(sys::SAI_LAG_MEMBER_ATTR_LAG_ID, lag),
            a_oid(sys::SAI_LAG_MEMBER_ATTR_PORT_ID, port),
            a_bool(sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, egress_disable),
            a_bool(sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, ingress_disable),
        ],
    );
    if st == SUCCESS { o } else { 0 }
}

fn set_bool(e: &Env, member: u64, id: u32, v: bool) -> Status {
    e.set(sys::SAI_OBJECT_TYPE_LAG_MEMBER, member, a_bool(id, v))
}

/// The handle the mock gave the last ifcs_lag_create in the log.
fn last_lag_handle(e: &Env) -> u32 {
    let log = e.m().log();
    let Some(p) = log.rfind("ifcs_lag_create h=0x") else {
        return 0;
    };
    // strtoul(log + p + 18, nullptr, 16): the "0x" and the hex digits after it
    let digits: String = log[p + 20..].chars().take_while(char::is_ascii_hexdigit).collect();
    u32::from_str_radix(&digits, 16).unwrap_or(0)
}

pub fn test_lag(e: &mut Env) {
    e.ck.section("lag");
    let (s, p1, p3, vlan) = (e.s, e.p1, e.p3, e.vlan);
    let (sp1, sp3) = (0x3e200001u32, 0x3e200003u32);

    // ---- hash from the profile at init (SAI_TERALYNX_HASH_FIELDS 5-tuple + in_port, SEED 10):
    // one field list per packet type, as isai (IPv4/IPv6 fields per family, L4 ports for TCP/UDP)
    check_eq!(e, "t_lag.cpp:94", e.m().efl_count(), 5);
    check!(
        e,
        "t_lag.cpp:95",
        e.m().efl_find("6,5,7,12,13,16") != 0
            && e.m().efl_find("6,5,7,16") != 0
            && e.m().efl_find("9,8,10,12,13,16") != 0
            && e.m().efl_find("9,8,10,16") != 0
            && e.m().efl_find("16") != 0
    );

    // ---- members: ports 1 and 3 (port 2 keeps t_l3's RIF). orchagent first removes their VLAN
    // members and bridge ports; until then (and for a port with a RIF) the frontend refuses
    let p2 = e.p2;
    let (st, lag0) = e.create(sys::SAI_OBJECT_TYPE_LAG, &[]);
    ok!(e, "t_lag.cpp:101", st);
    // port 1: bridge port, VLAN 100
    check!(e, "t_lag.cpp:102", make_member(e, lag0, p1, false, false) == 0);
    // port 2: router interface
    check!(e, "t_lag.cpp:103", make_member(e, lag0, p2, false, false) == 0);
    let st = e.remove(sys::SAI_OBJECT_TYPE_LAG, lag0);
    ok!(e, "t_lag.cpp:104", st);
    {
        let (st, a) = e.get1(
            sys::SAI_OBJECT_TYPE_VLAN,
            vlan,
            sys::SAI_VLAN_ATTR_MEMBER_LIST,
            Value::OidList(vec![0; 8]),
        );
        ok!(e, "t_lag.cpp:110", st);
        for m in a.oids() {
            let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN_MEMBER, m);
            ok!(e, "t_lag.cpp:111", st);
        }
    }
    for p in [p1, p3] {
        let bp = find_bridge_port(e, p);
        if bp != 0 {
            let st = e.remove(sys::SAI_OBJECT_TYPE_BRIDGE_PORT, bp);
            ok!(e, "t_lag.cpp:115", st);
        }
    }
    e.bp1 = 0;

    // ---- LAG + member: ifcs_lag_create, ifcs_lag_member_add of the sysport
    e.m().log_clear();
    let (st, lag) = e.create(sys::SAI_OBJECT_TYPE_LAG, &[]);
    ok!(e, "t_lag.cpp:122", st);
    let lh = last_lag_handle(e);
    check!(e, "t_lag.cpp:124", lh != 0);
    let lm1 = make_member(e, lag, p1, false, false);
    check!(e, "t_lag.cpp:126", lm1 != 0);
    let (n, first) = e.m().lag_members(lh);
    check!(e, "t_lag.cpp:128", n == 1 && first == sp1);
    check!(
        e,
        "t_lag.cpp:129",
        has(&e.m().log(), "ifcs_lag_member_add lag=0x") && has(&e.m().log(), "first=0x3e200001 attrs=0")
    );

    // ---- member created egress+ingress disabled: not in the IFCS LAG, learning off
    e.m().log_clear();
    let mut lm3 = make_member(e, lag, p3, true, true);
    check!(e, "t_lag.cpp:134", lm3 != 0);
    check_eq!(e, "t_lag.cpp:135", e.m().lag_members(lh).0, 1);
    check!(
        e,
        "t_lag.cpp:136",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200003 id=28 u32=0")
    );
    // ingress enabled (egress still disabled): joins (isai: out only while both are disabled)
    let st = set_bool(e, lm3, sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, false);
    ok!(e, "t_lag.cpp:138", st);
    check_eq!(e, "t_lag.cpp:139", e.m().lag_members(lh).0, 2);
    let st = set_bool(e, lm3, sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, false);
    ok!(e, "t_lag.cpp:140", st);
    check_eq!(e, "t_lag.cpp:141", e.m().lag_members(lh).0, 2);
    // teamd takes a member out (both flags) and back
    e.m().log_clear();
    let st = set_bool(e, lm1, sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, true);
    ok!(e, "t_lag.cpp:144", st);
    check_eq!(e, "t_lag.cpp:145", e.m().lag_members(lh).0, 2);
    let st = set_bool(e, lm1, sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, true);
    ok!(e, "t_lag.cpp:146", st);
    let (n, first) = e.m().lag_members(lh);
    check!(e, "t_lag.cpp:147", n == 1 && first == sp3);
    check!(
        e,
        "t_lag.cpp:148",
        has(&e.m().log(), "ifcs_lag_member_remove lag=0x")
            && has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=28 u32=0")
    );
    let st = set_bool(e, lm1, sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, false);
    ok!(e, "t_lag.cpp:149", st);
    let st = set_bool(e, lm1, sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, false);
    ok!(e, "t_lag.cpp:150", st);
    check_eq!(e, "t_lag.cpp:151", e.m().lag_members(lh).0, 2);
    check!(
        e,
        "t_lag.cpp:152",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=28 u32=1")
    );

    // ---- the LAG as bridge port: VLAN 200 tagged, VLAN 300 untagged = the LAG handle in the L2VNI,
    // one mc_dest for the LAG; per member: untagged egress, PVID
    let (st, bpl) = e.create(
        sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
        &[
            a_s32(sys::SAI_BRIDGE_PORT_ATTR_TYPE, sys::SAI_BRIDGE_PORT_TYPE_PORT as i32),
            a_oid(sys::SAI_BRIDGE_PORT_ATTR_PORT_ID, lag),
            a_bool(sys::SAI_BRIDGE_PORT_ATTR_ADMIN_STATE, true),
        ],
    );
    ok!(e, "t_lag.cpp:164", st);
    let (st, v200) = e.create(sys::SAI_OBJECT_TYPE_VLAN, &[a_u16(sys::SAI_VLAN_ATTR_VLAN_ID, 200)]);
    ok!(e, "t_lag.cpp:168", st);
    let (st, v300) = e.create(sys::SAI_OBJECT_TYPE_VLAN, &[a_u16(sys::SAI_VLAN_ATTR_VLAN_ID, 300)]);
    ok!(e, "t_lag.cpp:170", st);
    e.m().log_clear();
    let vm200 = make_vlan_member(e, v200, bpl, true);
    check!(e, "t_lag.cpp:173", vm200 != 0);
    check!(
        e,
        "t_lag.cpp:174",
        e.m().l2vni_has(200, lh) != 0 && e.m().l2vni_has(200, sp1) == 0 && e.m().l2vni_has(200, sp3) == 0
    );
    let want = format!("count=1 dest=0x{lh:x}");
    // one mc_dest, for the LAG
    check!(e, "t_lag.cpp:177", has(&e.m().log(), &want));
    let want = format!("ifcs_stp_port_add stp=0x100 members=1 attrs=1 state=3 first=0x{lh:x}");
    // STP is per sysport (chip: a LAG is refused)
    check!(e, "t_lag.cpp:179", !has(&e.m().log(), &want));
    e.m().log_clear();
    let vm300 = make_vlan_member(e, v300, bpl, false);
    check!(e, "t_lag.cpp:182", vm300 != 0 && e.m().l2vni_has(300, lh) != 0);
    check!(
        e,
        "t_lag.cpp:183",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=43 u32=300")
            && has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200003 id=43 u32=300")
    );
    e.m().log_clear();
    let st = e.set(
        sys::SAI_OBJECT_TYPE_LAG,
        lag,
        a_u16(sys::SAI_LAG_ATTR_PORT_VLAN_ID, 300),
    );
    ok!(e, "t_lag.cpp:188", st);
    check!(
        e,
        "t_lag.cpp:189",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200001 id=32 u32=300")
            && has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200003 id=32 u32=300")
    );

    // ---- FDB on the LAG: static entry -> ENTRY_DEST = LAG handle; SDK events with the LAG handle or a
    // member sysport -> the LAG's bridge port; flush by the LAG's bridge port
    let m1 = [0x02, 0xaa, 0, 0, 0, 1];
    let m2 = [0x02, 0xaa, 0, 0, 0, 2];
    let fe = Entry::Fdb { bv: v200, mac: m1 };
    let fa_ = [
        a_s32(sys::SAI_FDB_ENTRY_ATTR_TYPE, sys::SAI_FDB_ENTRY_TYPE_STATIC as i32),
        a_oid(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, bpl),
    ];
    let st = e.sai.entry(EntryOp::Create, s, &fe, &fa_);
    ok!(e, "t_lag.cpp:205", st);
    check_eq!(e, "t_lag.cpp:206", e.m().l2_dest(&m1, 200), lh);
    e.with_events(|ev| ev.fdb.clear());
    let _ = e.m().fire_l2_dest(0, &m2, 200, lh);
    let _ = e.m().fire_l2_dest(0, &m2, 300, sp3);
    check!(e, "t_lag.cpp:213", e.wait_for(|ev| ev.fdb.len() >= 2));
    {
        let evs = e.with_events(|ev| ev.fdb.clone());
        let mut ok = evs.len() >= 2;
        let mut i = 0;
        while ok && i < 2 {
            ok = evs[i].event == sys::SAI_FDB_EVENT_LEARNED as i32;
            i += 1;
        }
        check!(e, "t_lag.cpp:222", ok && evs[0].bv == v200 && evs[1].bv == v300);
        e.with_events(|ev| ev.fdb.clear());
    }
    let fl = [
        a_oid(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID, bpl),
        a_s32(
            sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE,
            sys::SAI_FDB_FLUSH_ENTRY_TYPE_ALL as i32,
        ),
    ];
    let st = e.sai.flush_fdb(s, &fl);
    ok!(e, "t_lag.cpp:230", st);
    check_eq!(e, "t_lag.cpp:231", e.m().l2_dest(&m1, 200), 0u32);

    // ---- members leave and join while the LAG is in VLANs: the LAG stays an L2VNI member until
    // the last member leaves; a joining member gets the LAG's untagged VLAN and PVID
    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_LAG_MEMBER, lm3);
    ok!(e, "t_lag.cpp:236", st);
    check!(
        e,
        "t_lag.cpp:237",
        e.m().l2vni_has(200, lh) != 0 && e.m().l2vni_has(300, lh) != 0
    );
    // untagged egress off
    check!(
        e,
        "t_lag.cpp:238",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200003 id=42 u32=0")
    );
    e.m().log_clear();
    lm3 = make_member(e, lag, p3, false, false);
    check!(e, "t_lag.cpp:241", lm3 != 0 && e.m().lag_members(lh).0 == 2);
    check!(
        e,
        "t_lag.cpp:242",
        has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200003 id=43 u32=300")
            && has(&e.m().log(), "ifcs_sysport_attr_set sp=0x3e200003 id=32 u32=300")
    );
    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_LAG_MEMBER, lm3);
    ok!(e, "t_lag.cpp:245", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_LAG_MEMBER, lm1);
    ok!(e, "t_lag.cpp:246", st);
    // no member left behind it
    check!(
        e,
        "t_lag.cpp:247",
        e.m().l2vni_has(200, lh) == 0 && e.m().l2vni_has(300, lh) == 0
    );
    check_eq!(e, "t_lag.cpp:248", e.m().lag_members(lh).0, 0);
    // back: the LAG rejoins both VLANs
    let lm1 = make_member(e, lag, p1, false, false);
    check!(
        e,
        "t_lag.cpp:250",
        lm1 != 0 && e.m().l2vni_has(200, lh) != 0 && e.m().l2vni_has(300, lh) != 0 && e.m().l2vni_has(200, sp1) == 0
    );

    // ---- teardown in orchagent's order: VLAN members, bridge port, VLANs, members, LAG
    let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN_MEMBER, vm200);
    ok!(e, "t_lag.cpp:253", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN_MEMBER, vm300);
    ok!(e, "t_lag.cpp:254", st);
    check!(
        e,
        "t_lag.cpp:255",
        e.m().l2vni_has(200, lh) == 0 && e.m().l2vni_has(300, lh) == 0
    );
    let st = e.remove(sys::SAI_OBJECT_TYPE_BRIDGE_PORT, bpl);
    ok!(e, "t_lag.cpp:256", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN, v200);
    ok!(e, "t_lag.cpp:257", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_VLAN, v300);
    ok!(e, "t_lag.cpp:258", st);
    let st = e.set(sys::SAI_OBJECT_TYPE_LAG, lag, a_u16(sys::SAI_LAG_ATTR_PORT_VLAN_ID, 1));
    ok!(e, "t_lag.cpp:261", st);

    // ---- the LAG as port RIF: the frontend's internal VLAN holds the LAG (its members join as the LAG)
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        s,
        sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
        Value::Oid(0),
    );
    ok!(e, "t_lag.cpp:266", st);
    let rmac = [0x02, 0, 0, 0, 0x0b, 0x01];
    let ra_ = [
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, a.oid()),
        a_s32(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            sys::SAI_ROUTER_INTERFACE_TYPE_PORT as i32,
        ),
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, lag),
        a_mac(sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS, rmac),
    ];
    e.m().log_clear();
    let (st, rif) = e.create(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, &ra_);
    ok!(e, "t_lag.cpp:278", st);
    let want = format!("n=1 first=0x{lh:x}");
    let log = e.m().log();
    let needle = "ifcs_l2vni_member_add vni=";
    let mut p = log.find(needle);
    let mut lag_in_rif_vlan = false;
    while let Some(at) = p {
        let eol = log[at..].find('\n').map_or(log.len(), |x| at + x);
        if log[at..eol].contains(&want) {
            lag_in_rif_vlan = true;
        }
        p = log[at + 1..].find(needle).map(|x| at + 1 + x);
    }
    check!(e, "t_lag.cpp:288", lag_in_rif_vlan);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif);
    ok!(e, "t_lag.cpp:289", st);

    e.m().log_clear();
    let st = e.remove(sys::SAI_OBJECT_TYPE_LAG_MEMBER, lm1);
    ok!(e, "t_lag.cpp:292", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_LAG, lag);
    ok!(e, "t_lag.cpp:293", st);
    check!(e, "t_lag.cpp:294", has(&e.m().log(), "ifcs_lag_delete h=0x"));
    check_eq!(e, "t_lag.cpp:295", e.m().lag_count(), 0);
}
