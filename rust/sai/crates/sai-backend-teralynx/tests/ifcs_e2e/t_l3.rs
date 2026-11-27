//! t_l3.cpp: L3: default routes, RIF, neighbor, next hop, routes, ip2me (test_l3), and the more
//! detailed L3 (test_l3_more: LAG RIF, MTU / MAC, v4 / v6 neighbors, next hops, ECMP, VRF, neighbor
//! actions, failure, in-use).
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::support::*;

/// A route entry key of an IPv4 prefix (address and mask as the host-order numbers).
fn v4_key(vr: u64, addr: u32, mask: u32) -> Entry {
    let mut a = [0u8; 16];
    let mut m = [0u8; 16];
    a[..4].copy_from_slice(&addr.to_be_bytes());
    m[..4].copy_from_slice(&mask.to_be_bytes());
    Entry::Route {
        vr,
        prefix: Prefix {
            v6: false,
            addr: a,
            mask: m,
        },
    }
}

fn route4(vr: u64, addr: u32, len: u32) -> Entry {
    v4_key(vr, addr, if len != 0 { 0xffff_ffffu32 << (32 - len) } else { 0 })
}

fn nbr4(rif: u64, addr: u32) -> Entry {
    Entry::Neighbor {
        rif,
        ip: IpAddr::V4(Ipv4Addr::from(addr)),
    }
}

fn key4(vni: &str, addr: u32, len: u32) -> String {
    let mask = if len != 0 { 0xffff_ffffu32 << (32 - len) } else { 0 };
    format!("{vni}/4/{addr:08x}/{mask:08x}")
}

/// The handle (the "h=0x" after the newest occurrence of `what`) in the log.
fn last_handle(e: &Env, what: &str) -> u32 {
    let l = e.m().log();
    let Some(p) = l.rfind(what) else { return 0 };
    let Some(q) = l[p..].find("h=0x") else { return 0 };
    let hex: String = l[p + q + 4..].chars().take_while(char::is_ascii_hexdigit).collect();
    u32::from_str_radix(&hex, 16).unwrap_or(0)
}

/// The out-values of m_intf_info (kept between calls as the C++'s variables are).
#[derive(Default)]
struct IntfV {
    ty: u32,
    fw: u32,
    tr: u32,
    mtu: u32,
    mac: [u8; 6],
}

fn intf_info(e: &Env, h: u32, v: &mut IntfV) -> bool {
    match e.m().intf_info(h) {
        Some(i) => {
            v.ty = i.ty;
            v.fw = i.fwd;
            v.tr = i.transport;
            v.mtu = i.mtu;
            v.mac = i.mac;
            true
        }
        None => false,
    }
}

/// The out-values of m_nh_info.
#[derive(Default)]
struct NhV {
    intf: u32,
    mac: [u8; 6],
    dest: u32,
}

fn nh_info(e: &Env, h: u32, v: &mut NhV) -> bool {
    match e.m().nh_info(h) {
        Some(n) => {
            v.intf = n.intf;
            v.mac = n.mac;
            v.dest = n.dest;
            true
        }
        None => false,
    }
}

/// m_ecmp_members(h, ml, wl, 64): the member count; `ml` keeps the members of the last answer.
fn ecmp_members(e: &Env, h: u32, ml: &mut Vec<u32>) -> i32 {
    let (n, m, _w) = e.m().ecmp_members(h, 64);
    if n >= 0 {
        *ml = m;
    }
    n
}

pub fn test_l3(e: &mut Env) {
    e.ck.section("l3");
    let (s, p2, vlan) = (e.s, e.p2, e.vlan);
    let _ = vlan;
    // ---- L3: the default routes orchagent installs first (DROP)
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        s,
        sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
        Value::Oid(0),
    );
    ok!(e, "t_l3.cpp:13", st);
    let vr = a.oid();
    let re = v4_key(vr, 0, 0);
    let re6 = Entry::Route {
        vr,
        prefix: Prefix {
            v6: true,
            addr: [0; 16],
            mask: [0; 16],
        },
    };
    let drop_action = a_s32(
        sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION,
        sys::SAI_PACKET_ACTION_DROP as i32,
    );
    let st = e.sai.entry(EntryOp::Create, s, &re, std::slice::from_ref(&drop_action));
    ok!(e, "t_l3.cpp:22", st);
    let st = e.sai.entry(EntryOp::Create, s, &re6, &[drop_action]);
    ok!(e, "t_l3.cpp:25", st);
    check_eq!(e, "t_l3.cpp:26", e.m().route_count(), 2);
    let l3vni0 = "26200001"; // IFCS_HANDLE_L3VNI(1) = VRF 0
    check!(
        e,
        "t_l3.cpp:30",
        e.m()
            .route_find(&format!("{l3vni0}/4/00000000/00000000"))
            .is_some_and(|r| r.fwd == 1 && r.ctc == 0)
    );
    check!(
        e,
        "t_l3.cpp:31",
        e.m()
            .route_find(&format!(
                "{l3vni0}/6/00000000000000000000000000000000/00000000000000000000000000000000"
            ))
            .is_some_and(|r| r.fwd == 1)
    );

    // port RIF on port 2, neighbor, next hop, a route, an ip2me route
    let rmac = [0x0c, 0x48, 0xc6, 0x7a, 0x55, 0xce];
    let ri = [
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, vr),
        a_s32(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            sys::SAI_ROUTER_INTERFACE_TYPE_PORT as i32,
        ),
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, p2),
        a_mac(sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS, rmac),
    ];
    let (st, rif) = e.create(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, &ri);
    ok!(e, "t_l3.cpp:46", st);
    check!(
        e,
        "t_l3.cpp:47",
        has(&e.m().log(), "ifcs_intf_create") && has(&e.m().log(), "type=1 fwd=0x26200001")
    );
    let ne_ip = IpAddr::V4(Ipv4Addr::from(0x0a00_0002u32));
    let ne = Entry::Neighbor { rif, ip: ne_ip };
    let nmac = [0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &ne,
        &[a_mac(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, nmac)],
    );
    ok!(e, "t_l3.cpp:57", st);
    check!(
        e,
        "t_l3.cpp:58",
        has(&e.m().log(), "ifcs_nexthop_create") && has(&e.m().log(), "dest=0x3e200002")
    );
    check!(
        e,
        "t_l3.cpp:59",
        e.m()
            .route_find(&format!("{l3vni0}/4/0a000002/ffffffff"))
            .is_some_and(|r| r.fwd == 0 && r.nh != 0)
    );
    let nhat = [
        a_s32(sys::SAI_NEXT_HOP_ATTR_TYPE, sys::SAI_NEXT_HOP_TYPE_IP as i32),
        Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::Ip(ne_ip)),
        a_oid(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID, rif),
    ];
    let (st, nhid) = e.create(sys::SAI_OBJECT_TYPE_NEXT_HOP, &nhat);
    ok!(e, "t_l3.cpp:68", st);
    let r24 = v4_key(vr, 0x0a01_0000, 0xffff_ff00);
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &r24,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, nhid)],
    );
    ok!(e, "t_l3.cpp:74", st);
    check!(
        e,
        "t_l3.cpp:75",
        e.m()
            .route_find(&format!("{l3vni0}/4/0a010000/ffffff00"))
            .is_some_and(|r| r.fwd == 0 && r.nh != 0)
    );
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        s,
        sys::SAI_SWITCH_ATTR_CPU_PORT,
        Value::Oid(0),
    );
    ok!(e, "t_l3.cpp:77", st);
    let cpu = a.oid();
    let me = v4_key(vr, 0x0a00_0001, 0xffff_ffff);
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &me,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, cpu)],
    );
    ok!(e, "t_l3.cpp:84", st);
    check!(
        e,
        "t_l3.cpp:85",
        has(&e.m().log(), "ifcs_hostif_trap_create") && has(&e.m().log(), "queue=0")
    );
    check!(
        e,
        "t_l3.cpp:86",
        e.m()
            .route_find(&format!("{l3vni0}/4/0a000001/ffffffff"))
            .is_some_and(|r| r.fwd == 1 && r.ctc == 1)
    );
    // IP2ME: TTL check profile 1 (threshold 0, set on the node once), so TTL 1 to the switch is not dropped;
    // the L3VNI copies TTL-check failures to the CPU (v4 + v6)
    {
        check_eq!(
            e,
            "t_l3.cpp:92",
            e.m().route_ttlp(&format!("{l3vni0}/4/0a000001/ffffffff")),
            1
        );
        // a neighbor's host route: default profile
        check_eq!(
            e,
            "t_l3.cpp:93",
            e.m().route_ttlp(&format!("{l3vni0}/4/0a000002/ffffffff")),
            0
        );
        check!(
            e,
            "t_l3.cpp:94",
            has(&e.m().log(), "ifcs_node_attr_set node=0 count=1 id=107 u32=0")
        );
        check!(
            e,
            "t_l3.cpp:95",
            has(
                &e.m().log(),
                "ifcs_l3vni_create h=0x26200001 count=4 attrs=5=1,7=1,9=1,10=1"
            )
        );
    }
    // route re-pointed to drop: kind change = delete + create
    let st = e.sai.entry(
        EntryOp::Set,
        s,
        &r24,
        &[a_s32(
            sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        )],
    );
    ok!(e, "t_l3.cpp:100", st);
    check!(
        e,
        "t_l3.cpp:101",
        e.m()
            .route_find(&format!("{l3vni0}/4/0a010000/ffffff00"))
            .is_some_and(|r| r.fwd == 1)
    );
    let st = e.sai.entry(EntryOp::Remove, s, &r24, &[]);
    ok!(e, "t_l3.cpp:102", st);
    check!(
        e,
        "t_l3.cpp:103",
        e.m().route_find(&format!("{l3vni0}/4/0a010000/ffffff00")).is_none()
    );

    test_l3_more(e);
}

// ---- more L3 (mock state hooks of mock_l3.c)
fn test_l3_more(e: &mut Env) {
    let (s, p3, vlan) = (e.s, e.p3, e.vlan);
    let st = e.sai.query(Api::VirtualRouter);
    ok!(e, "t_l3.cpp:166", st);
    let st = e.sai.query(Api::NextHopGroup);
    ok!(e, "t_l3.cpp:167", st);
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        s,
        sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
        Value::Oid(0),
    );
    ok!(e, "t_l3.cpp:170", st);
    let vr = a.oid();
    let (vni0, vni1) = ("26200001", "26200002");
    let intfs0 = e.m().intf_count();
    let nhs0 = e.m().nh_count();
    let ecmps0 = e.m().ecmp_count();
    let rmac = [0x0c, 0x48, 0xc6, 0x7a, 0x55, 0xce];

    // ---- port RIF on a LAG (member port 3): the neighbor's nexthop goes to the LAG handle
    {
        // a LAG member has no bridge port of its own (orchagent removes it first; t_lag.cpp does the same)
        let (st, a) = e.get1(
            sys::SAI_OBJECT_TYPE_SWITCH,
            s,
            sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID,
            Value::Oid(0),
        );
        ok!(e, "t_l3.cpp:181", st);
        let (st, q) = e.get1(
            sys::SAI_OBJECT_TYPE_BRIDGE,
            a.oid(),
            sys::SAI_BRIDGE_ATTR_PORT_LIST,
            Value::OidList(vec![0; 64]),
        );
        ok!(e, "t_l3.cpp:187", st);
        for bp in q.oids() {
            let (st, pq) = e.get1(
                sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
                bp,
                sys::SAI_BRIDGE_PORT_ATTR_PORT_ID,
                Value::Oid(0),
            );
            if st == SUCCESS && pq.oid() == p3 {
                let st = e.remove(sys::SAI_OBJECT_TYPE_BRIDGE_PORT, bp);
                ok!(e, "t_l3.cpp:191", st);
            }
        }
    }
    let (st, lag) = e.create(sys::SAI_OBJECT_TYPE_LAG, &[]);
    ok!(e, "t_l3.cpp:195", st);
    let lagh = last_handle(e, "ifcs_lag_create");
    let lma = [
        a_oid(sys::SAI_LAG_MEMBER_ATTR_LAG_ID, lag),
        a_oid(sys::SAI_LAG_MEMBER_ATTR_PORT_ID, p3),
    ];
    let (st, lm) = e.create(sys::SAI_OBJECT_TYPE_LAG_MEMBER, &lma);
    ok!(e, "t_l3.cpp:202", st);
    let rla = [
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, vr),
        a_s32(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            sys::SAI_ROUTER_INTERFACE_TYPE_PORT as i32,
        ),
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, lag),
    ];
    let (st, rifl) = e.create(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, &rla);
    ok!(e, "t_l3.cpp:211", st);
    let nl = nbr4(rifl, 0x0a0a_0002);
    let lmac = [0x02, 0xaa, 0, 0, 0, 0x0c];
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &nl,
        &[a_mac(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, lmac)],
    );
    ok!(e, "t_l3.cpp:216", st);
    let nhl = last_handle(e, "ifcs_nexthop_create");
    let mut l = NhV::default();
    check!(
        e,
        "t_l3.cpp:220",
        lagh != 0 && nh_info(e, nhl, &mut l) && l.dest == lagh
    );
    check!(
        e,
        "t_l3.cpp:221",
        e.m()
            .route_find(&key4(vni0, 0x0a0a_0002, 32))
            .is_some_and(|r| r.fwd == 0 && r.nh == nhl)
    );
    let st = e.sai.entry(EntryOp::Remove, s, &nl, &[]);
    ok!(e, "t_l3.cpp:222", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rifl);
    ok!(e, "t_l3.cpp:223", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_LAG_MEMBER, lm);
    ok!(e, "t_l3.cpp:224", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_LAG, lag);
    ok!(e, "t_l3.cpp:225", st);
    check_eq!(e, "t_l3.cpp:226", e.m().nh_count(), nhs0);

    // ---- port RIF on port 3 (MTU 9100): SVI on its internal VLAN in VRF 0
    let ri = [
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, vr),
        a_s32(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            sys::SAI_ROUTER_INTERFACE_TYPE_PORT as i32,
        ),
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, p3),
        a_mac(sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS, rmac),
        a_u32(sys::SAI_ROUTER_INTERFACE_ATTR_MTU, 9100),
    ];
    let (st, rif3) = e.create(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, &ri);
    ok!(e, "t_l3.cpp:241", st);
    let ih3 = last_handle(e, "ifcs_intf_create");
    let mut iv = IntfV::default();
    check!(
        e,
        "t_l3.cpp:244",
        intf_info(e, ih3, &mut iv) && iv.ty == 1 && iv.fw == 0x2620_0001u32 && iv.mtu == 9100 && iv.mac == rmac
    );
    check!(e, "t_l3.cpp:245", (iv.tr >> 25) == 18); // transport = an L2VNI handle (the internal VLAN)
    check_eq!(e, "t_l3.cpp:246", e.m().intf_count(), intfs0 + 1);
    // MTU and source MAC set: intf attr_set in place
    let st = e.set(
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        rif3,
        a_u32(sys::SAI_ROUTER_INTERFACE_ATTR_MTU, 1500),
    );
    ok!(e, "t_l3.cpp:250", st);
    let rmac2 = [0x02, 0, 0, 0, 0x33, 0x01];
    let st = e.set(
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        rif3,
        a_mac(sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS, rmac2),
    );
    ok!(e, "t_l3.cpp:254", st);
    check!(
        e,
        "t_l3.cpp:255",
        intf_info(e, ih3, &mut iv) && iv.mtu == 1500 && iv.mac == rmac2
    );
    check!(
        e,
        "t_l3.cpp:256",
        has(&e.m().log(), "ifcs_intf_attr_set h=0x") && has(&e.m().log(), "id=24") && has(&e.m().log(), "id=5")
    );

    // ---- neighbors on it: v4 and v6; the nexthop points at sysport 3
    let n1 = nbr4(rif3, 0x0a00_0102);
    let hmac = [0x02, 0xaa, 0, 0, 0, 0x01];
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &n1,
        &[a_mac(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, hmac)],
    );
    ok!(e, "t_l3.cpp:263", st);
    let nhh1 = last_handle(e, "ifcs_nexthop_create");
    let mut nv = NhV::default();
    check!(
        e,
        "t_l3.cpp:266",
        nh_info(e, nhh1, &mut nv) && nv.intf == ih3 && nv.dest == 0x3e20_0003u32 && nv.mac == hmac
    );
    check!(
        e,
        "t_l3.cpp:267",
        e.m()
            .route_find(&key4(vni0, 0x0a00_0102, 32))
            .is_some_and(|r| r.fwd == 0 && r.nh == nhh1)
    );
    let v6 = [0xfc, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
    let n6 = Entry::Neighbor {
        rif: rif3,
        ip: IpAddr::V6(Ipv6Addr::from(v6)),
    };
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &n6,
        &[a_mac(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, hmac)],
    );
    ok!(e, "t_l3.cpp:275", st);
    check!(
        e,
        "t_l3.cpp:276",
        e.m()
            .route_find(&format!(
                "{vni0}/6/fc000001000000000000000000000002/ffffffffffffffffffffffffffffffff"
            ))
            .is_some_and(|r| r.fwd == 0 && r.nh != 0 && r.nh != nhh1)
    ); // own egress object per neighbor
    // neighbor MAC change: nexthop DEST_MAC in place (routes keep the handle)
    let hmac2 = [0x02, 0xaa, 0, 0, 0, 0x09];
    let st = e.sai.entry(
        EntryOp::Set,
        s,
        &n1,
        &[a_mac(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, hmac2)],
    );
    ok!(e, "t_l3.cpp:281", st);
    check!(e, "t_l3.cpp:282", nh_info(e, nhh1, &mut nv) && nv.mac == hmac2);
    check!(
        e,
        "t_l3.cpp:283",
        e.m()
            .route_find(&key4(vni0, 0x0a00_0102, 32))
            .is_some_and(|r| r.nh == nhh1)
    );

    // ---- next hops: one resolved, one not (-> CPU until its neighbor exists)
    let n1_ip = IpAddr::V4(Ipv4Addr::from(0x0a00_0102u32));
    let mut nha_ = [
        a_s32(sys::SAI_NEXT_HOP_ATTR_TYPE, sys::SAI_NEXT_HOP_TYPE_IP as i32),
        Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::Ip(n1_ip)),
        a_oid(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID, rif3),
    ];
    let (st, nh_a) = e.create(sys::SAI_OBJECT_TYPE_NEXT_HOP, &nha_);
    ok!(e, "t_l3.cpp:294", st);
    let n2 = nbr4(rif3, 0x0a00_0109);
    nha_[1] = Attr::new(
        sys::SAI_NEXT_HOP_ATTR_IP,
        Value::Ip(IpAddr::V4(Ipv4Addr::from(0x0a00_0109u32))),
    );
    let (st, nh_b) = e.create(sys::SAI_OBJECT_TYPE_NEXT_HOP, &nha_);
    ok!(e, "t_l3.cpp:297", st);
    let r_b = route4(vr, 0x0a06_0000, 16);
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &r_b,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, nh_b)],
    );
    ok!(e, "t_l3.cpp:301", st);
    check!(
        e,
        "t_l3.cpp:302",
        e.m()
            .route_find(&key4(vni0, 0x0a06_0000, 16))
            .is_some_and(|r| r.fwd == 1 && r.ctc == 1 && r.nh == 0)
    );
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &n2,
        &[a_mac(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, hmac)],
    );
    ok!(e, "t_l3.cpp:305", st); // resolves: the route follows (CPU -> nexthop)
    let nhh2 = last_handle(e, "ifcs_nexthop_create");
    check!(
        e,
        "t_l3.cpp:307",
        e.m()
            .route_find(&key4(vni0, 0x0a06_0000, 16))
            .is_some_and(|r| r.fwd == 0 && r.ctc == 0 && r.nh == nhh2)
    );

    // ---- ECMP: group of nhA + nhB, members added / weighted / removed
    let (st, g) = e.create(
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP,
        &[a_s32(
            sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE,
            sys::SAI_NEXT_HOP_GROUP_TYPE_ECMP as i32,
        )],
    );
    ok!(e, "t_l3.cpp:313", st);
    check_eq!(e, "t_l3.cpp:314", e.m().ecmp_count(), ecmps0); // no members: no IFCS group
    let r_e = route4(vr, 0x0a07_0000, 16);
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &r_e,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, g)],
    );
    ok!(e, "t_l3.cpp:318", st);
    check!(
        e,
        "t_l3.cpp:319",
        e.m()
            .route_find(&key4(vni0, 0x0a07_0000, 16))
            .is_some_and(|r| r.fwd == 1 && r.nh == 0)
    ); // empty group drops
    let mut gm = [
        a_oid(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID, g),
        a_oid(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID, nh_a),
        Attr::default(),
    ];
    let (st, m_a) = e.create(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER, &gm[..2]);
    ok!(e, "t_l3.cpp:326", st);
    check_eq!(e, "t_l3.cpp:327", e.m().ecmp_count(), ecmps0 + 1);
    let eh = last_handle(e, "ifcs_ecmp_create");
    let mut ml: Vec<u32> = Vec::new();
    check!(e, "t_l3.cpp:329", ecmp_members(e, eh, &mut ml) == 1 && ml[0] == nhh1);
    check!(
        e,
        "t_l3.cpp:330",
        e.m()
            .route_find(&key4(vni0, 0x0a07_0000, 16))
            .is_some_and(|r| r.fwd == 0 && r.nh == eh)
    ); // route on the group
    gm[1] = a_oid(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID, nh_b);
    gm[2] = a_u32(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT, 3);
    let (st, m_b) = e.create(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER, &gm[..3]);
    ok!(e, "t_l3.cpp:334", st);
    let mut m_b = m_b;
    // weight 3 = nhB three times in the member list (one replace of the whole list)
    let nm = ecmp_members(e, eh, &mut ml);
    check!(
        e,
        "t_l3.cpp:337",
        nm == 4 && ml[0] == nhh1 && ml[1] == nhh2 && ml[2] == nhh2 && ml[3] == nhh2
    );
    check!(e, "t_l3.cpp:338", has(&e.m().log(), "ifcs_ecmp_member_replace ecmp=0x"));
    let st = e.set(
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER,
        m_b,
        a_u32(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT, 1),
    );
    ok!(e, "t_l3.cpp:341", st);
    let nm = ecmp_members(e, eh, &mut ml);
    check!(e, "t_l3.cpp:343", nm == 2 && ml[0] == nhh1 && ml[1] == nhh2);
    let st = e.remove(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER, m_a);
    ok!(e, "t_l3.cpp:344", st);
    check!(e, "t_l3.cpp:345", ecmp_members(e, eh, &mut ml) == 1 && ml[0] == nhh2);
    check!(
        e,
        "t_l3.cpp:346",
        e.m()
            .route_find(&key4(vni0, 0x0a07_0000, 16))
            .is_some_and(|r| r.nh == eh)
    );
    let st = e.remove(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER, m_b);
    ok!(e, "t_l3.cpp:347", st); // last member: route -> drop, IFCS group deleted
    check!(
        e,
        "t_l3.cpp:348",
        e.m()
            .route_find(&key4(vni0, 0x0a07_0000, 16))
            .is_some_and(|r| r.fwd == 1 && r.nh == 0)
    );
    check_eq!(e, "t_l3.cpp:349", e.m().ecmp_count(), ecmps0);
    let (st, mb2) = e.create(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER, &gm[..2]);
    m_b = mb2;
    ok!(e, "t_l3.cpp:350", st); // back: a new IFCS group, route follows
    let eh2 = last_handle(e, "ifcs_ecmp_create");
    check!(
        e,
        "t_l3.cpp:352",
        e.m()
            .route_find(&key4(vni0, 0x0a07_0000, 16))
            .is_some_and(|r| r.fwd == 0 && r.nh == eh2)
    );

    // ---- VRF: VR 2 = L3VNI(2); VLAN RIF in it (SVI on VLAN 100's L2VNI); routes keyed by its L3VNI;
    // a route leaked to a next hop of VRF 0
    let (st, vr2) = e.create(sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER, &[]);
    ok!(e, "t_l3.cpp:357", st);
    let rv = [
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, vr2),
        a_s32(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            sys::SAI_ROUTER_INTERFACE_TYPE_VLAN as i32,
        ),
        a_oid(sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID, vlan),
    ];
    let (st, rifv) = e.create(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, &rv);
    ok!(e, "t_l3.cpp:366", st);
    let ihv = last_handle(e, "ifcs_intf_create");
    check!(
        e,
        "t_l3.cpp:368",
        intf_info(e, ihv, &mut iv)
            && iv.ty == 1
            && iv.fw == 0x2620_0002u32
            && iv.tr == ((18u32 << 25) | (1u32 << 21) | 0x64u32)
    );
    check!(e, "t_l3.cpp:369", has(&e.m().log(), "ifcs_l3vni_create h=0x26200002"));
    let r_l = route4(vr2, 0x0a08_0000, 16);
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &r_l,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, nh_a)],
    );
    ok!(e, "t_l3.cpp:373", st);
    check!(
        e,
        "t_l3.cpp:374",
        e.m()
            .route_find(&key4(vni1, 0x0a08_0000, 16))
            .is_some_and(|r| r.fwd == 0 && r.nh == nhh1)
    );
    check!(
        e,
        "t_l3.cpp:375",
        e.m().route_find(&key4(vni0, 0x0a08_0000, 16)).is_none()
    );
    // a neighbor on the VLAN RIF without an FDB entry goes to the CPU egress (host route traps)
    let n_v = nbr4(rifv, 0x0a09_0002);
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &n_v,
        &[a_mac(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, hmac)],
    );
    ok!(e, "t_l3.cpp:380", st);
    check!(
        e,
        "t_l3.cpp:381",
        e.m()
            .route_find(&key4(vni1, 0x0a09_0002, 32))
            .is_some_and(|r| r.fwd == 1 && r.ctc == 1)
    );

    // ---- neighbor actions DROP / TRAP: host route policies, no nexthop object
    let nb_before = e.m().nh_count();
    let n_d = nbr4(rif3, 0x0a00_0150);
    let mut na2 = [
        a_mac(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, hmac),
        a_s32(
            sys::SAI_NEIGHBOR_ENTRY_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        ),
    ];
    let st = e.sai.entry(EntryOp::Create, s, &n_d, &na2);
    ok!(e, "t_l3.cpp:391", st);
    check!(
        e,
        "t_l3.cpp:392",
        e.m()
            .route_find(&key4(vni0, 0x0a00_0150, 32))
            .is_some_and(|r| r.fwd == 1 && r.ctc == 0 && r.nh == 0)
    );
    let n_t = nbr4(rif3, 0x0a00_0151);
    na2[1] = a_s32(
        sys::SAI_NEIGHBOR_ENTRY_ATTR_PACKET_ACTION,
        sys::SAI_PACKET_ACTION_TRAP as i32,
    );
    let st = e.sai.entry(EntryOp::Create, s, &n_t, &na2);
    ok!(e, "t_l3.cpp:395", st);
    check!(
        e,
        "t_l3.cpp:396",
        e.m()
            .route_find(&key4(vni0, 0x0a00_0151, 32))
            .is_some_and(|r| r.fwd == 1 && r.ctc == 1)
    );
    check_eq!(e, "t_l3.cpp:397", e.m().nh_count(), nb_before);

    // ---- an IFCS failure leaves no route behind (SAI error, nothing kept)
    let r_f = route4(vr, 0x0b00_0000, 8);
    e.m().fail_next_route();
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &r_f,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, nh_a)],
    );
    check!(e, "t_l3.cpp:404", st == sys::SAI_STATUS_TABLE_FULL); // IFCS_RESOURCE_FULL
    let mut ga = [Attr::new(sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION, Value::S32(0))];
    let st = e.sai.get_entry(s, &r_f, &mut ga);
    check!(e, "t_l3.cpp:406", st == sys::SAI_STATUS_ITEM_NOT_FOUND);
    let st = e.sai.entry(
        EntryOp::Create,
        s,
        &r_f,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, nh_a)],
    );
    ok!(e, "t_l3.cpp:409", st);
    check!(
        e,
        "t_l3.cpp:410",
        e.m()
            .route_find(&key4(vni0, 0x0b00_0000, 8))
            .is_some_and(|r| r.nh == nhh1)
    );
    // default route DROP -> next hop + FORWARD (orchagent's order): kind change = delete + create
    let r0 = route4(vr, 0, 0);
    let st = e.sai.entry(
        EntryOp::Set,
        s,
        &r0,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, nh_a)],
    );
    ok!(e, "t_l3.cpp:413", st);
    check!(
        e,
        "t_l3.cpp:414",
        e.m()
            .route_find(&key4(vni0, 0, 0))
            .is_some_and(|r| r.fwd == 1 && r.nh == 0)
    ); // still DROP
    let st = e.sai.entry(
        EntryOp::Set,
        s,
        &r0,
        &[a_s32(
            sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_FORWARD as i32,
        )],
    );
    ok!(e, "t_l3.cpp:417", st);
    check!(
        e,
        "t_l3.cpp:418",
        e.m()
            .route_find(&key4(vni0, 0, 0))
            .is_some_and(|r| r.fwd == 0 && r.nh == nhh1)
    );
    // same kind: attr_set
    e.m().log_clear();
    let st = e.sai.entry(
        EntryOp::Set,
        s,
        &r0,
        &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, nh_b)],
    );
    ok!(e, "t_l3.cpp:422", st);
    check!(
        e,
        "t_l3.cpp:423",
        has(
            &e.m().log(),
            "ifcs_route_entry_attr_set key=26200001/4/00000000/00000000"
        ) && e.m().route_find(&key4(vni0, 0, 0)).is_some_and(|r| r.nh == nhh2)
    );
    let st = e.sai.entry(
        EntryOp::Set,
        s,
        &r0,
        &[a_s32(
            sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION,
            sys::SAI_PACKET_ACTION_DROP as i32,
        )],
    );
    ok!(e, "t_l3.cpp:427", st);
    check!(
        e,
        "t_l3.cpp:428",
        e.m()
            .route_find(&key4(vni0, 0, 0))
            .is_some_and(|r| r.fwd == 1 && r.nh == 0)
    );
    check!(
        e,
        "t_l3.cpp:429",
        e.remove(sys::SAI_OBJECT_TYPE_NEXT_HOP, nh_b) == sys::SAI_STATUS_OBJECT_IN_USE
    ); // still the default route's NEXT_HOP_ID
    let st = e
        .sai
        .entry(EntryOp::Set, s, &r0, &[a_oid(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, 0)]);
    ok!(e, "t_l3.cpp:432", st);

    // ---- in use: a RIF with neighbors, a next hop in a group, a VR with routes
    check!(
        e,
        "t_l3.cpp:435",
        e.remove(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif3) == sys::SAI_STATUS_OBJECT_IN_USE
    );
    check!(
        e,
        "t_l3.cpp:436",
        e.remove(sys::SAI_OBJECT_TYPE_NEXT_HOP, nh_b) == sys::SAI_STATUS_OBJECT_IN_USE
    );
    check!(
        e,
        "t_l3.cpp:437",
        e.remove(sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER, vr2) == sys::SAI_STATUS_OBJECT_IN_USE
    );

    // ---- teardown in orchagent's order: routes, members, groups, next hops, neighbors, RIFs, VR
    let st = e.sai.entry(EntryOp::Remove, s, &r_e, &[]);
    ok!(e, "t_l3.cpp:440", st);
    let st = e.sai.entry(EntryOp::Remove, s, &r_b, &[]);
    ok!(e, "t_l3.cpp:441", st);
    let st = e.sai.entry(EntryOp::Remove, s, &r_l, &[]);
    ok!(e, "t_l3.cpp:442", st);
    let st = e.sai.entry(EntryOp::Remove, s, &r_f, &[]);
    ok!(e, "t_l3.cpp:443", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER, m_b);
    ok!(e, "t_l3.cpp:444", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP, g);
    ok!(e, "t_l3.cpp:445", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_NEXT_HOP, nh_a);
    ok!(e, "t_l3.cpp:446", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_NEXT_HOP, nh_b);
    ok!(e, "t_l3.cpp:447", st);
    for n in [&n1, &n2, &n6, &n_v, &n_d, &n_t] {
        let st = e.sai.entry(EntryOp::Remove, s, n, &[]);
        ok!(e, "t_l3.cpp:448", st);
    }
    let st = e.remove(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rif3);
    ok!(e, "t_l3.cpp:449", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, rifv);
    ok!(e, "t_l3.cpp:450", st);
    let st = e.remove(sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER, vr2);
    ok!(e, "t_l3.cpp:451", st);
    check_eq!(e, "t_l3.cpp:452", e.m().intf_count(), intfs0);
    check_eq!(e, "t_l3.cpp:453", e.m().nh_count(), nhs0);
    check_eq!(e, "t_l3.cpp:454", e.m().ecmp_count(), ecmps0);
    check!(
        e,
        "t_l3.cpp:455",
        e.m().route_find(&key4(vni0, 0x0a00_0102, 32)).is_none()
    );
    check!(
        e,
        "t_l3.cpp:456",
        e.m().route_find(&key4(vni1, 0x0a09_0002, 32)).is_none()
    );
}
