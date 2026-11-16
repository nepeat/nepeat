// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, ports: list, lanes, speed, admin, FEC, MTU, link events, counters (harness: tests/harness.h).
#include "harness.h"

static int (*m_link_reads)();

static int32_t oper(sai_port_api_t* pa, sai_object_id_t p)
{
    sai_attribute_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = SAI_PORT_ATTR_OPER_STATUS;
    if (pa->get_port_attribute(p, 1, &a) != SAI_STATUS_SUCCESS) return -1;
    return a.value.s32;
}

void test_ports(Env& e)
{
    USE_ENV(e);
    hook(g_mock, m_link_reads, "mock_link_reads");
    // create_switch asked every port's link state: all known down, no IFCS read
    CHECK_EQ(m_link_reads(), 0);
    // ---- ports
    sai_attribute_t a;
    sai_object_id_t plist[8];
    a.id = SAI_SWITCH_ATTR_PORT_LIST;
    a.value.objlist.count = 8;
    a.value.objlist.list = plist;
    OK(sw->get_switch_attribute(s, 1, &a));
    CHECK_EQ(a.value.objlist.count, 3u);
    p1 = plist[0];
    p2 = plist[1];
    uint32_t lanes[8];
    a.id = SAI_PORT_ATTR_HW_LANE_LIST;
    a.value.u32list.count = 8;
    a.value.u32list.list = lanes;
    OK(pa->get_port_attribute(p1, 1, &a));
    CHECK_EQ(a.value.u32list.count, 8u);
    CHECK_EQ(lanes[0], 249u);
    CHECK_EQ(lanes[7], 256u);
    a.id = SAI_PORT_ATTR_SPEED;
    OK(pa->get_port_attribute(p1, 1, &a));
    CHECK_EQ(a.value.u32, 400000u);
    a.value.u32 = 100000;
    CHECK(pa->set_port_attribute(p1, &a) != SAI_STATUS_SUCCESS);   // no live speed change
    a.value.u32 = 400000;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_admin(1), 0u);
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = true;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_admin(1), 1u);
    a.id = SAI_PORT_ATTR_FEC_MODE;
    a.value.s32 = SAI_PORT_FEC_MODE_FC;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_fec(1), 3u);
    a.value.s32 = SAI_PORT_FEC_MODE_RS;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_fec(1), 2u);                  // 50G lanes -> RS544 (KP)
    p3 = plist[2];
    a.value.s32 = SAI_PORT_FEC_MODE_FC;
    OK(pa->set_port_attribute(p3, &a));
    a.value.s32 = SAI_PORT_FEC_MODE_RS;
    OK(pa->set_port_attribute(p3, &a));
    CHECK_EQ(m_fec(3), 1u);                  // 10G NRZ -> RS528 (KR)
    OK(pa->set_port_attribute(p3, &a));      // already RS: kept
    CHECK_EQ(m_fec(3), 1u);
    m_log_clear();
    a.id = SAI_PORT_ATTR_MTU;
    a.value.u32 = 9100;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=51 u32=9122"));

    // link event -> port state notification
    m_fire_link(1, 1);
    CHECK(wait_for([&] { return !g_port_events.empty(); }));
    {
        std::lock_guard<std::mutex> g(g_mu);
        CHECK(!g_port_events.empty() && g_port_events[0].port_id == p1 &&
              g_port_events[0].port_state == SAI_PORT_OPER_STATUS_UP);
    }
    m_fire_link(77, 1);                       // not a front-panel devport: ignored

    // link state: admin-up made port 1 unknown, so one IFCS read, which the
    // event above already settled; events and admin-down need no read
    CHECK_EQ(oper(pa, p1), (int32_t)SAI_PORT_OPER_STATUS_UP);
    CHECK_EQ(oper(pa, p1), (int32_t)SAI_PORT_OPER_STATUS_UP);
    CHECK_EQ(oper(pa, p2), (int32_t)SAI_PORT_OPER_STATUS_DOWN);   // admin-down since init
    CHECK_EQ(m_link_reads(), 0);
    m_fire_link(1, 0);
    CHECK(wait_for([&] { return g_port_events.size() >= 2; }));
    CHECK_EQ(oper(pa, p1), (int32_t)SAI_PORT_OPER_STATUS_DOWN);
    m_fire_link(1, 1);
    CHECK_EQ(oper(pa, p1), (int32_t)SAI_PORT_OPER_STATUS_UP);
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = true;
    OK(pa->set_port_attribute(p3, &a));       // admin-up: unknown, read once (mock link down)
    CHECK_EQ(oper(pa, p3), (int32_t)SAI_PORT_OPER_STATUS_DOWN);
    CHECK_EQ(oper(pa, p3), (int32_t)SAI_PORT_OPER_STATUS_DOWN);
    CHECK_EQ(m_link_reads(), 1);
    a.value.booldata = false;
    OK(pa->set_port_attribute(p3, &a));
    CHECK_EQ(oper(pa, p3), (int32_t)SAI_PORT_OPER_STATUS_DOWN);
    CHECK_EQ(m_link_reads(), 1);

    // stats
    m_set_stat(1, 47, 123456);                // RX_BYTES_ALL
    m_set_stat(1, 3, 10);                     // RX unicast
    m_set_stat(1, 4, 5);
    m_set_stat(1, 5, 2);
    sai_stat_id_t ids[3] = {SAI_PORT_STAT_IF_IN_OCTETS, SAI_PORT_STAT_IF_IN_UCAST_PKTS, SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS};
    uint64_t vals[3] = {0, 0, 0};
    m_log_clear();
    OK(pa->get_port_stats(p1, 3, ids, vals));
    // one stats call for the 12 mapped counters (id list), not all 112
    CHECK(has(m_log(), "ifcs_devport_stats_get dp=1 count=12"));
    CHECK_EQ(lines(m_log()).size(), 1u);
    CHECK_EQ(vals[0], 123456u);
    CHECK_EQ(vals[1], 10u);
    CHECK_EQ(vals[2], 7u);
    OK(pa->clear_port_all_stats(p1));
    CHECK(has(m_log(), "ifcs_devport_stats_clear dp=1 count=111"));

}
