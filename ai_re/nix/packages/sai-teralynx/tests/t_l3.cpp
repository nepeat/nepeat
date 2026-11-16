// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, L3: default routes, RIF, neighbor, next hop, routes, ip2me (harness: tests/harness.h).
#include "harness.h"

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
    // route re-pointed to drop: kind change = delete + create
    a.id = SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(rta->set_route_entry_attribute(&r24, &a));
    CHECK(m_route_find((l3vni0 + "/4/0a010000/ffffff00").c_str(), &fwd, &ctc, &nh) && fwd == 1);
    OK(rta->remove_route_entry(&r24));
    CHECK(!m_route_find((l3vni0 + "/4/0a010000/ffffff00").c_str(), &fwd, &ctc, &nh));

}
