// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, L3: default routes, RIF, neighbor, next hop, routes, ip2me (harness: tests/harness.h).
#include "harness.h"

void test_l3_more(Env& e);

void test_l3(Env& e)
{
    USE_ENV(e);
    sai_attribute_t a;
    // ---- L3: the default routes orchagent installs first (DROP)
    a.id = SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID;
    OK(sw->get_switch_attribute(s, 1, &a));
    sai_object_id_t vr = a.value.oid;
    sai_route_entry_t re;
    std::memset(&re, 0, sizeof(re));
    re.switch_id = s;
    re.vr_id = vr;
    re.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->create_route_entry(&re, 1, &a));
    sai_route_entry_t re6 = re;
    re6.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
    OK(rta->create_route_entry(&re6, 1, &a));
    CHECK_EQ(m_route_count(), 2);
    int fwd = -1, ctc = -1;
    uint32_t nh = 0;
    const std::string l3vni0 = "26200001";   // IFCS_HANDLE_L3VNI(1) = VRF 0
    CHECK(m_route_find((l3vni0 + "/4/00000000/00000000").c_str(), &fwd, &ctc, &nh) && fwd == 1 && ctc == 0);
    CHECK(m_route_find((l3vni0 + "/6/00000000000000000000000000000000/00000000000000000000000000000000").c_str(), &fwd,
                       &ctc, &nh) && fwd == 1);

    // port RIF on port 2, neighbor, next hop, a route, an ip2me route
    sai_attribute_t ri[4];
    ri[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    ri[0].value.oid = vr;
    ri[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    ri[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    ri[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    ri[2].value.oid = p2;
    ri[3].id = SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS;
    const uint8_t rmac[6] = {0x0c, 0x48, 0xc6, 0x7a, 0x55, 0xce};
    std::memcpy(ri[3].value.mac, rmac, 6);
    sai_object_id_t rif = 0;
    OK(ra->create_router_interface(&rif, s, 4, ri));
    CHECK(has(m_log(), "ifcs_intf_create") && has(m_log(), "type=1 fwd=0x26200001"));
    sai_neighbor_entry_t ne;
    std::memset(&ne, 0, sizeof(ne));
    ne.switch_id = s;
    ne.rif_id = rif;
    ne.ip_address.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    ne.ip_address.addr.ip4 = htonl(0x0a000002);
    a.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    const uint8_t nmac[6] = {0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee};
    std::memcpy(a.value.mac, nmac, 6);
    OK(na->create_neighbor_entry(&ne, 1, &a));
    CHECK(has(m_log(), "ifcs_nexthop_create") && has(m_log(), "dest=0x3e200002"));
    CHECK(m_route_find((l3vni0 + "/4/0a000002/ffffffff").c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh != 0);
    sai_attribute_t nhat[3];
    nhat[0].id = SAI_NEXT_HOP_ATTR_TYPE;
    nhat[0].value.s32 = SAI_NEXT_HOP_TYPE_IP;
    nhat[1].id = SAI_NEXT_HOP_ATTR_IP;
    nhat[1].value.ipaddr = ne.ip_address;
    nhat[2].id = SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID;
    nhat[2].value.oid = rif;
    sai_object_id_t nhid = 0;
    OK(nha->create_next_hop(&nhid, s, 3, nhat));
    sai_route_entry_t r24 = re;
    r24.destination.addr.ip4 = htonl(0x0a010000);
    r24.destination.mask.ip4 = htonl(0xffffff00);
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = nhid;
    OK(rta->create_route_entry(&r24, 1, &a));
    CHECK(m_route_find((l3vni0 + "/4/0a010000/ffffff00").c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh != 0);
    a.id = SAI_SWITCH_ATTR_CPU_PORT;
    OK(sw->get_switch_attribute(s, 1, &a));
    sai_object_id_t cpu = a.value.oid;
    sai_route_entry_t me = re;
    me.destination.addr.ip4 = htonl(0x0a000001);
    me.destination.mask.ip4 = 0xffffffff;
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = cpu;
    OK(rta->create_route_entry(&me, 1, &a));
    CHECK(has(m_log(), "ifcs_hostif_trap_create") && has(m_log(), "queue=0"));
    CHECK(m_route_find((l3vni0 + "/4/0a000001/ffffffff").c_str(), &fwd, &ctc, &nh) && fwd == 1 && ctc == 1);
    // IP2ME: TTL check profile 1 (threshold 0, set on the node once), so TTL 1 to the switch is not dropped;
    // the L3VNI copies TTL-check failures to the CPU (v4 + v6)
    {
        int (*ttlp)(const char*) = nullptr;
        hook(g_mock, ttlp, "mock_route_ttlp");
        CHECK_EQ(ttlp((l3vni0 + "/4/0a000001/ffffffff").c_str()), 1);
        CHECK_EQ(ttlp((l3vni0 + "/4/0a000002/ffffffff").c_str()), 0);   // a neighbor's host route: default profile
        CHECK(has(m_log(), "ifcs_node_attr_set node=0 count=1 id=107 u32=0"));
        CHECK(has(m_log(), "ifcs_l3vni_create h=0x26200001 count=2 attrs=5=1,7=1"));
    }
    // route re-pointed to drop: kind change = delete + create
    a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->set_route_entry_attribute(&r24, &a));
    CHECK(m_route_find((l3vni0 + "/4/0a010000/ffffff00").c_str(), &fwd, &ctc, &nh) && fwd == 1);
    OK(rta->remove_route_entry(&r24));
    CHECK(!m_route_find((l3vni0 + "/4/0a010000/ffffff00").c_str(), &fwd, &ctc, &nh));

    test_l3_more(e);
}

// ---- more L3 (mock state hooks of mock_l3.c)
static int (*m_intf_count)();
static int (*m_intf_info)(uint32_t, uint32_t*, uint32_t*, uint32_t*, uint32_t*, uint8_t*);
static int (*m_nh_count)();
static int (*m_nh_info)(uint32_t, uint32_t*, uint8_t*, uint32_t*);
static int (*m_ecmp_count)();
static int (*m_ecmp_members)(uint32_t, uint32_t*, uint32_t*, int);
static void (*m_fail_next_route)();

static sai_route_entry_t route4(sai_object_id_t s, sai_object_id_t vr, uint32_t addr, int len)
{
    sai_route_entry_t r;
    std::memset(&r, 0, sizeof(r));
    r.switch_id = s;
    r.vr_id = vr;
    r.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    r.destination.addr.ip4 = htonl(addr);
    r.destination.mask.ip4 = htonl(len ? 0xffffffffu << (32 - len) : 0);
    return r;
}
static sai_neighbor_entry_t nbr4(sai_object_id_t s, sai_object_id_t rif, uint32_t addr)
{
    sai_neighbor_entry_t n;
    std::memset(&n, 0, sizeof(n));
    n.switch_id = s;
    n.rif_id = rif;
    n.ip_address.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    n.ip_address.addr.ip4 = htonl(addr);
    return n;
}
static std::string key4(const std::string& vni, uint32_t addr, int len)
{
    char b[64];
    std::snprintf(b, sizeof(b), "%s/4/%08x/%08x", vni.c_str(), addr, len ? 0xffffffffu << (32 - len) : 0);
    return b;
}
// The intf handle of the newest ifcs_intf_create in the log.
static uint32_t last_handle(const char* what)
{
    std::string l = m_log();
    size_t p = l.rfind(what);
    if (p == std::string::npos) return 0;
    p = l.find("h=0x", p);
    return p == std::string::npos ? 0 : (uint32_t)std::strtoul(l.c_str() + p + 4, nullptr, 16);
}

void test_l3_more(Env& e)
{
    USE_ENV(e);
    hook(g_mock, m_intf_count, "mock_intf_count");
    hook(g_mock, m_intf_info, "mock_intf_info");
    hook(g_mock, m_nh_count, "mock_nh_count");
    hook(g_mock, m_nh_info, "mock_nh_info");
    hook(g_mock, m_ecmp_count, "mock_ecmp_count");
    hook(g_mock, m_ecmp_members, "mock_ecmp_members");
    hook(g_mock, m_fail_next_route, "mock_fail_next_route");
    sai_virtual_router_api_t* vra = nullptr;
    sai_next_hop_group_api_t* ga = nullptr;
    OK(sai_api_query(SAI_API_VIRTUAL_ROUTER, (void**)&vra));
    OK(sai_api_query(SAI_API_NEXT_HOP_GROUP, (void**)&ga));
    sai_attribute_t a;
    a.id = SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID;
    OK(sw->get_switch_attribute(s, 1, &a));
    sai_object_id_t vr = a.value.oid;
    const std::string vni0 = "26200001", vni1 = "26200002";
    int intfs0 = m_intf_count(), nhs0 = m_nh_count(), ecmps0 = m_ecmp_count();
    int fwd = -1, ctc = -1;
    uint32_t nh = 0;
    const uint8_t rmac[6] = {0x0c, 0x48, 0xc6, 0x7a, 0x55, 0xce};

    // ---- port RIF on a LAG (member port 3): the neighbor's nexthop goes to the LAG handle
    {   // a LAG member has no bridge port of its own (orchagent removes it first; t_lag.cpp does the same)
        a.id = SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID;
        OK(sw->get_switch_attribute(s, 1, &a));
        sai_object_id_t bps[64];
        sai_attribute_t q;
        q.id = SAI_BRIDGE_ATTR_PORT_LIST;
        q.value.objlist.count = 64;
        q.value.objlist.list = bps;
        OK(ba->get_bridge_attribute(a.value.oid, 1, &q));
        for (uint32_t i = 0; i < q.value.objlist.count; i++) {
            sai_attribute_t pq;
            pq.id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
            if (ba->get_bridge_port_attribute(bps[i], 1, &pq) == SAI_STATUS_SUCCESS && pq.value.oid == p3) OK(ba->remove_bridge_port(bps[i]));
        }
    }
    sai_object_id_t lag = 0, lm = 0;
    OK(la->create_lag(&lag, s, 0, nullptr));
    uint32_t lagh = last_handle("ifcs_lag_create");
    sai_attribute_t lma[2];
    lma[0].id = SAI_LAG_MEMBER_ATTR_LAG_ID;
    lma[0].value.oid = lag;
    lma[1].id = SAI_LAG_MEMBER_ATTR_PORT_ID;
    lma[1].value.oid = p3;
    OK(la->create_lag_member(&lm, s, 2, lma));
    sai_attribute_t rla[3];
    rla[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    rla[0].value.oid = vr;
    rla[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    rla[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    rla[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    rla[2].value.oid = lag;
    sai_object_id_t rifl = 0;
    OK(ra->create_router_interface(&rifl, s, 3, rla));
    sai_neighbor_entry_t nl = nbr4(s, rifl, 0x0a0a0002);
    a.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    const uint8_t lmac[6] = {0x02, 0xaa, 0, 0, 0, 0x0c};
    std::memcpy(a.value.mac, lmac, 6);
    OK(na->create_neighbor_entry(&nl, 1, &a));
    uint32_t nhl = last_handle("ifcs_nexthop_create");
    uint32_t lin = 0, ldest = 0;
    uint8_t lnm[6];
    CHECK(lagh != 0 && m_nh_info(nhl, &lin, lnm, &ldest) && ldest == lagh);
    CHECK(m_route_find(key4(vni0, 0x0a0a0002, 32).c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh == nhl);
    OK(na->remove_neighbor_entry(&nl));
    OK(ra->remove_router_interface(rifl));
    OK(la->remove_lag_member(lm));
    OK(la->remove_lag(lag));
    CHECK_EQ(m_nh_count(), nhs0);

    // ---- port RIF on port 3 (MTU 9100): SVI on its internal VLAN in VRF 0
    sai_attribute_t ri[5];
    ri[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    ri[0].value.oid = vr;
    ri[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    ri[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    ri[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    ri[2].value.oid = p3;
    ri[3].id = SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS;
    std::memcpy(ri[3].value.mac, rmac, 6);
    ri[4].id = SAI_ROUTER_INTERFACE_ATTR_MTU;
    ri[4].value.u32 = 9100;
    sai_object_id_t rif3 = 0;
    OK(ra->create_router_interface(&rif3, s, 5, ri));
    uint32_t ih3 = last_handle("ifcs_intf_create"), ty = 0, fw = 0, tr = 0, mtu = 0;
    uint8_t mac[6];
    CHECK(m_intf_info(ih3, &ty, &fw, &tr, &mtu, mac) && ty == 1 && fw == 0x26200001u && mtu == 9100 && !std::memcmp(mac, rmac, 6));
    CHECK((tr >> 25) == 18);   // transport = an L2VNI handle (the internal VLAN)
    CHECK_EQ(m_intf_count(), intfs0 + 1);
    // MTU and source MAC set: intf attr_set in place
    a.id = SAI_ROUTER_INTERFACE_ATTR_MTU;
    a.value.u32 = 1500;
    OK(ra->set_router_interface_attribute(rif3, &a));
    const uint8_t rmac2[6] = {0x02, 0, 0, 0, 0x33, 0x01};
    a.id = SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS;
    std::memcpy(a.value.mac, rmac2, 6);
    OK(ra->set_router_interface_attribute(rif3, &a));
    CHECK(m_intf_info(ih3, &ty, &fw, &tr, &mtu, mac) && mtu == 1500 && !std::memcmp(mac, rmac2, 6));
    CHECK(has(m_log(), "ifcs_intf_attr_set h=0x" ) && has(m_log(), "id=24") && has(m_log(), "id=5"));

    // ---- neighbors on it: v4 and v6; the nexthop points at sysport 3
    sai_neighbor_entry_t n1 = nbr4(s, rif3, 0x0a000102);
    const uint8_t hmac[6] = {0x02, 0xaa, 0, 0, 0, 0x01};
    a.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    std::memcpy(a.value.mac, hmac, 6);
    OK(na->create_neighbor_entry(&n1, 1, &a));
    uint32_t nhh1 = last_handle("ifcs_nexthop_create"), nin = 0, ndest = 0;
    uint8_t nmac[6];
    CHECK(m_nh_info(nhh1, &nin, nmac, &ndest) && nin == ih3 && ndest == 0x3e200003u && !std::memcmp(nmac, hmac, 6));
    CHECK(m_route_find(key4(vni0, 0x0a000102, 32).c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh == nhh1);
    sai_neighbor_entry_t n6;
    std::memset(&n6, 0, sizeof(n6));
    n6.switch_id = s;
    n6.rif_id = rif3;
    n6.ip_address.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
    const uint8_t v6[16] = {0xfc, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2};
    std::memcpy(n6.ip_address.addr.ip6, v6, 16);
    OK(na->create_neighbor_entry(&n6, 1, &a));
    CHECK(m_route_find((vni0 + "/6/fc000001000000000000000000000002/ffffffffffffffffffffffffffffffff").c_str(), &fwd, &ctc, &nh) &&
          fwd == 0 && nh != 0 && nh != nhh1);   // own egress object per neighbor
    // neighbor MAC change: nexthop DEST_MAC in place (routes keep the handle)
    const uint8_t hmac2[6] = {0x02, 0xaa, 0, 0, 0, 0x09};
    std::memcpy(a.value.mac, hmac2, 6);
    OK(na->set_neighbor_entry_attribute(&n1, &a));
    CHECK(m_nh_info(nhh1, &nin, nmac, &ndest) && !std::memcmp(nmac, hmac2, 6));
    CHECK(m_route_find(key4(vni0, 0x0a000102, 32).c_str(), &fwd, &ctc, &nh) && nh == nhh1);

    // ---- next hops: one resolved, one not (-> CPU until its neighbor exists)
    sai_attribute_t nha_[3];
    nha_[0].id = SAI_NEXT_HOP_ATTR_TYPE;
    nha_[0].value.s32 = SAI_NEXT_HOP_TYPE_IP;
    nha_[1].id = SAI_NEXT_HOP_ATTR_IP;
    nha_[1].value.ipaddr = n1.ip_address;
    nha_[2].id = SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID;
    nha_[2].value.oid = rif3;
    sai_object_id_t nhA = 0, nhB = 0;
    OK(nha->create_next_hop(&nhA, s, 3, nha_));
    sai_neighbor_entry_t n2 = nbr4(s, rif3, 0x0a000109);
    nha_[1].value.ipaddr = n2.ip_address;
    OK(nha->create_next_hop(&nhB, s, 3, nha_));
    sai_route_entry_t rB = route4(s, vr, 0x0a060000, 16);
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = nhB;
    OK(rta->create_route_entry(&rB, 1, &a));
    CHECK(m_route_find(key4(vni0, 0x0a060000, 16).c_str(), &fwd, &ctc, &nh) && fwd == 1 && ctc == 1 && nh == 0);
    a.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    std::memcpy(a.value.mac, hmac, 6);
    OK(na->create_neighbor_entry(&n2, 1, &a));   // resolves: the route follows (CPU -> nexthop)
    uint32_t nhh2 = last_handle("ifcs_nexthop_create");
    CHECK(m_route_find(key4(vni0, 0x0a060000, 16).c_str(), &fwd, &ctc, &nh) && fwd == 0 && ctc == 0 && nh == nhh2);

    // ---- ECMP: group of nhA + nhB, members added / weighted / removed
    a.id = SAI_NEXT_HOP_GROUP_ATTR_TYPE;
    a.value.s32 = SAI_NEXT_HOP_GROUP_TYPE_ECMP;
    sai_object_id_t g = 0;
    OK(ga->create_next_hop_group(&g, s, 1, &a));
    CHECK_EQ(m_ecmp_count(), ecmps0);   // no members: no IFCS group
    sai_route_entry_t rE = route4(s, vr, 0x0a070000, 16);
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = g;
    OK(rta->create_route_entry(&rE, 1, &a));
    CHECK(m_route_find(key4(vni0, 0x0a070000, 16).c_str(), &fwd, &ctc, &nh) && fwd == 1 && nh == 0);   // empty group drops
    sai_attribute_t gm[3];
    gm[0].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID;
    gm[0].value.oid = g;
    gm[1].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID;
    gm[1].value.oid = nhA;
    sai_object_id_t mA = 0, mB = 0;
    OK(ga->create_next_hop_group_member(&mA, s, 2, gm));
    CHECK_EQ(m_ecmp_count(), ecmps0 + 1);
    uint32_t eh = last_handle("ifcs_ecmp_create"), ml[64], wl[64];
    CHECK(m_ecmp_members(eh, ml, wl, 64) == 1 && ml[0] == nhh1);
    CHECK(m_route_find(key4(vni0, 0x0a070000, 16).c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh == eh);   // route on the group
    gm[1].value.oid = nhB;
    gm[2].id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT;
    gm[2].value.u32 = 3;
    OK(ga->create_next_hop_group_member(&mB, s, 3, gm));
    // weight 3 = nhB three times in the member list (one replace of the whole list)
    int nm = m_ecmp_members(eh, ml, wl, 64);
    CHECK(nm == 4 && ml[0] == nhh1 && ml[1] == nhh2 && ml[2] == nhh2 && ml[3] == nhh2);
    CHECK(has(m_log(), "ifcs_ecmp_member_replace ecmp=0x"));
    a.id = SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT;
    a.value.u32 = 1;
    OK(ga->set_next_hop_group_member_attribute(mB, &a));
    nm = m_ecmp_members(eh, ml, wl, 64);
    CHECK(nm == 2 && ml[0] == nhh1 && ml[1] == nhh2);
    OK(ga->remove_next_hop_group_member(mA));
    CHECK(m_ecmp_members(eh, ml, wl, 64) == 1 && ml[0] == nhh2);
    CHECK(m_route_find(key4(vni0, 0x0a070000, 16).c_str(), &fwd, &ctc, &nh) && nh == eh);
    OK(ga->remove_next_hop_group_member(mB));   // last member: route -> drop, IFCS group deleted
    CHECK(m_route_find(key4(vni0, 0x0a070000, 16).c_str(), &fwd, &ctc, &nh) && fwd == 1 && nh == 0);
    CHECK_EQ(m_ecmp_count(), ecmps0);
    OK(ga->create_next_hop_group_member(&mB, s, 2, gm));   // back: a new IFCS group, route follows
    uint32_t eh2 = last_handle("ifcs_ecmp_create");
    CHECK(m_route_find(key4(vni0, 0x0a070000, 16).c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh == eh2);

    // ---- VRF: VR 2 = L3VNI(2); VLAN RIF in it (SVI on VLAN 100's L2VNI); routes keyed by its L3VNI;
    // a route leaked to a next hop of VRF 0
    sai_object_id_t vr2 = 0;
    OK(vra->create_virtual_router(&vr2, s, 0, nullptr));
    sai_attribute_t rv[3];
    rv[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    rv[0].value.oid = vr2;
    rv[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    rv[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_VLAN;
    rv[2].id = SAI_ROUTER_INTERFACE_ATTR_VLAN_ID;
    rv[2].value.oid = vlan;
    sai_object_id_t rifv = 0;
    OK(ra->create_router_interface(&rifv, s, 3, rv));
    uint32_t ihv = last_handle("ifcs_intf_create");
    CHECK(m_intf_info(ihv, &ty, &fw, &tr, &mtu, mac) && ty == 1 && fw == 0x26200002u && tr == ((18u << 25) | (1u << 21) | 100u));
    CHECK(has(m_log(), "ifcs_l3vni_create h=0x26200002"));
    sai_route_entry_t rL = route4(s, vr2, 0x0a080000, 16);
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = nhA;
    OK(rta->create_route_entry(&rL, 1, &a));
    CHECK(m_route_find(key4(vni1, 0x0a080000, 16).c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh == nhh1);
    CHECK(!m_route_find(key4(vni0, 0x0a080000, 16).c_str(), &fwd, &ctc, &nh));
    // a neighbor on the VLAN RIF without an FDB entry goes to the CPU egress (host route traps)
    sai_neighbor_entry_t nv = nbr4(s, rifv, 0x0a090002);
    a.id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    std::memcpy(a.value.mac, hmac, 6);
    OK(na->create_neighbor_entry(&nv, 1, &a));
    CHECK(m_route_find(key4(vni1, 0x0a090002, 32).c_str(), &fwd, &ctc, &nh) && fwd == 1 && ctc == 1);

    // ---- neighbor actions DROP / TRAP: host route policies, no nexthop object
    int nb_before = m_nh_count();
    sai_neighbor_entry_t nd = nbr4(s, rif3, 0x0a000150);
    sai_attribute_t na2[2];
    na2[0].id = SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS;
    std::memcpy(na2[0].value.mac, hmac, 6);
    na2[1].id = SAI_NEIGHBOR_ENTRY_ATTR_PACKET_ACTION;
    na2[1].value.s32 = SAI_PACKET_ACTION_DROP;
    OK(na->create_neighbor_entry(&nd, 2, na2));
    CHECK(m_route_find(key4(vni0, 0x0a000150, 32).c_str(), &fwd, &ctc, &nh) && fwd == 1 && ctc == 0 && nh == 0);
    sai_neighbor_entry_t nt = nbr4(s, rif3, 0x0a000151);
    na2[1].value.s32 = SAI_PACKET_ACTION_TRAP;
    OK(na->create_neighbor_entry(&nt, 2, na2));
    CHECK(m_route_find(key4(vni0, 0x0a000151, 32).c_str(), &fwd, &ctc, &nh) && fwd == 1 && ctc == 1);
    CHECK_EQ(m_nh_count(), nb_before);

    // ---- an IFCS failure leaves no route behind (SAI error, nothing kept)
    sai_route_entry_t rF = route4(s, vr, 0x0b000000, 8);
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = nhA;
    m_fail_next_route();
    CHECK(rta->create_route_entry(&rF, 1, &a) == SAI_STATUS_TABLE_FULL);   // IFCS_RESOURCE_FULL
    a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    CHECK(rta->get_route_entry_attribute(&rF, 1, &a) == SAI_STATUS_ITEM_NOT_FOUND);
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = nhA;
    OK(rta->create_route_entry(&rF, 1, &a));
    CHECK(m_route_find(key4(vni0, 0x0b000000, 8).c_str(), &fwd, &ctc, &nh) && nh == nhh1);
    // default route DROP -> next hop + FORWARD (orchagent's order): kind change = delete + create
    sai_route_entry_t r0 = route4(s, vr, 0, 0);
    OK(rta->set_route_entry_attribute(&r0, &a));
    CHECK(m_route_find(key4(vni0, 0, 0).c_str(), &fwd, &ctc, &nh) && fwd == 1 && nh == 0);   // still DROP
    a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_FORWARD;
    OK(rta->set_route_entry_attribute(&r0, &a));
    CHECK(m_route_find(key4(vni0, 0, 0).c_str(), &fwd, &ctc, &nh) && fwd == 0 && nh == nhh1);
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = nhB;   // same kind: attr_set
    m_log_clear();
    OK(rta->set_route_entry_attribute(&r0, &a));
    CHECK(has(m_log(), "ifcs_route_entry_attr_set key=26200001/4/00000000/00000000") &&
          m_route_find(key4(vni0, 0, 0).c_str(), &fwd, &ctc, &nh) && nh == nhh2);
    a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->set_route_entry_attribute(&r0, &a));
    CHECK(m_route_find(key4(vni0, 0, 0).c_str(), &fwd, &ctc, &nh) && fwd == 1 && nh == 0);
    CHECK(nha->remove_next_hop(nhB) == SAI_STATUS_OBJECT_IN_USE);   // still the default route's NEXT_HOP_ID
    a.id = SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID;
    a.value.oid = SAI_NULL_OBJECT_ID;
    OK(rta->set_route_entry_attribute(&r0, &a));

    // ---- in use: a RIF with neighbors, a next hop in a group, a VR with routes
    CHECK(ra->remove_router_interface(rif3) == SAI_STATUS_OBJECT_IN_USE);
    CHECK(nha->remove_next_hop(nhB) == SAI_STATUS_OBJECT_IN_USE);
    CHECK(vra->remove_virtual_router(vr2) == SAI_STATUS_OBJECT_IN_USE);

    // ---- teardown in orchagent's order: routes, members, groups, next hops, neighbors, RIFs, VR
    OK(rta->remove_route_entry(&rE));
    OK(rta->remove_route_entry(&rB));
    OK(rta->remove_route_entry(&rL));
    OK(rta->remove_route_entry(&rF));
    OK(ga->remove_next_hop_group_member(mB));
    OK(ga->remove_next_hop_group(g));
    OK(nha->remove_next_hop(nhA));
    OK(nha->remove_next_hop(nhB));
    for (auto* n : {&n1, &n2, &n6, &nv, &nd, &nt}) OK(na->remove_neighbor_entry(n));
    OK(ra->remove_router_interface(rif3));
    OK(ra->remove_router_interface(rifv));
    OK(vra->remove_virtual_router(vr2));
    CHECK_EQ(m_intf_count(), intfs0);
    CHECK_EQ(m_nh_count(), nhs0);
    CHECK_EQ(m_ecmp_count(), ecmps0);
    CHECK(!m_route_find(key4(vni0, 0x0a000102, 32).c_str(), &fwd, &ctc, &nh));
    CHECK(!m_route_find(key4(vni1, 0x0a090002, 32).c_str(), &fwd, &ctc, &nh));

}
