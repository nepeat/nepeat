// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, switch: init sequence, init commands, temperatures, removal (harness: tests/harness.h).
#include "harness.h"

void test_switch_init(Env& e)
{
    USE_ENV(e);
    const std::string &run = e.run, &init = e.init, &isg = e.isg;
    CHECK(get(run + "/config.yaml") == get(init) + get(isg));
    std::string log = m_log();
    std::vector<std::string> L = lines(log);
    // The vendor innovium.user sequence, then our log hook in between.
    std::vector<std::string> want = {
        "ifcs_init cfg=" + run + "/config.yaml log=sai-teralynx boot=0 first=# generated init config",
        "ifcs_log_function_register",
        "ifcs_node_create node=0 count=0 attrs=null",
        "ifcs_node_attr_set node=0 count=1 id=82 u32=1",   // multicast LAG resolution: only before ENABLE
        "ifcs_attr_t_init",
        "ifcs_attr_t_id_set id=0",
        "ifcs_attr_t_value_data_set 1",
        "ifcs_node_attr_set node=0 count=1 id=0 u32=1",
    };
    CHECK(L.size() > want.size());
    for (size_t i = 0; i < want.size() && i < L.size(); i++) {
        if (L[i] != want[i]) std::fprintf(stderr, "init call %zu: got '%s' want '%s'\n", i, L[i].c_str(), want[i].c_str());
        CHECK(L[i] == want[i]);
    }
    // ports down before any init command, then the commands in file order
    size_t down3 = log.find("ifcs_devport_attr_set dp=3 id=23 u32=0");
    size_t c1 = log.find("ifcs_devport_attr_set dp=1 id=49 u32=1");
    CHECK(down3 != std::string::npos && c1 != std::string::npos && down3 < c1);
    std::vector<std::string> cmds;
    for (auto& l : L)
        if (l.rfind("ifcs_devport_attr_set", 0) == 0 && l.find("id=23 ") == std::string::npos) cmds.push_back(l);
    std::vector<std::string> want_cmds = {
        "ifcs_devport_attr_set dp=1 id=49 u32=1",
        "ifcs_devport_attr_set dp=1 id=43 i8=[2,2,2,2,2,2,2,2]",
        "ifcs_devport_attr_set dp=2 id=50 u8=[1,2,3,4]",
        "ifcs_devport_attr_set dp=3 id=53 u32l=[5]",
        "ifcs_devport_attr_set dp=1 id=21 u32=1",
    };
    CHECK_EQ(cmds.size(), want_cmds.size());
    for (size_t i = 0; i < want_cmds.size() && i < cmds.size(); i++) {
        if (cmds[i] != want_cmds[i]) std::fprintf(stderr, "cmd %zu: got '%s' want '%s'\n", i, cmds[i].c_str(), want_cmds[i].c_str());
        CHECK(cmds[i] == want_cmds[i]);
    }
    CHECK(!has(log, "id=23 u32=1"));            // "port enable" in a vendor file never enables a port
    CHECK(has(log, "ifcs_linkscan_event_register_notify node=0"));
    CHECK(has(log, "ifcs_l2_event_register_notify node=0"));
    CHECK(has(log, "ifcs_stp_port_add stp=0x100 members=4 attrs=1 state=3"));
    CHECK(has(log, "ifcs_l2vni_create vni=1 count=4 stp=0x100"));
    CHECK(!has(log, "ifcs_l2vni_member_add vni=1 n=1 first=0x3e2003e8"));  // the CPU is in no VLAN (traps bring what it needs)

}

void test_switch_misc(Env& e)
{
    USE_ENV(e);
    sai_attribute_t a;
    // ---- temperature: master 55.5, remotes 54.0 53.5 (-300 left out) 52.0.
    // One monitor per IFCS read (each is ~400 ms on the chip), round-robin,
    // at most one read per SAI_TERALYNX_TEMP_PERIOD (15 s) once all were read.
    int (*m_temp_reads)() = nullptr;
    hook(g_mock, m_temp_reads, "mock_temp_reads");
    int32_t temps[8];
    a.id = SAI_SWITCH_ATTR_TEMP_LIST;
    for (int i = 0; i < 5; i++) {
        a.value.s32list.count = 8;
        a.value.s32list.list = temps;
        OK(sw->get_switch_attribute(s, 1, &a));
    }
    CHECK_EQ(m_temp_reads(), 5);
    CHECK_EQ(a.value.s32list.count, 4u);
    a.value.s32list.count = 8;
    OK(sw->get_switch_attribute(s, 1, &a));   // within the period: cached
    CHECK_EQ(m_temp_reads(), 5);
    CHECK_EQ(a.value.s32list.count, 4u);
    a.id = SAI_SWITCH_ATTR_MAX_TEMP;
    OK(sw->get_switch_attribute(s, 1, &a));
    CHECK(a.value.s32 == 55 || a.value.s32 == 56);
    a.id = SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY;
    OK(sw->get_switch_attribute(s, 1, &a));
    CHECK(a.value.u32 > 0 && a.value.u32 < 24576u);

}

void test_switch_remove(Env& e)
{
    USE_ENV(e);
    // ---- remove: ports down, callbacks gone, deinit
    m_log_clear();
    OK(sw->remove_switch(s));
    std::string log = m_log();
    size_t d1 = log.find("ifcs_devport_attr_set dp=1 id=23 u32=0");
    size_t un = log.find("ifcs_linkscan_event_unregister_notify");
    size_t de = log.find("ifcs_deinit type=0");
    CHECK(d1 != std::string::npos && un != std::string::npos && de != std::string::npos && d1 < un && un < de);
    CHECK(has(log, "ifcs_netdev_delete"));
    CHECK_EQ(m_inited(), 0);

}
