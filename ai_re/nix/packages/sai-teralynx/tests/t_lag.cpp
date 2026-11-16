// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, LAG: create, members (egress/ingress disable as teamd sets
// them), the LAG as bridge port (VLAN members, PVID, FDB entries and events,
// flush) and as port RIF (internal VLAN), member join/leave while in a VLAN,
// removal; the hash profile keys (harness: tests/harness.h).
#include "harness.h"

static sai_object_id_t find_bridge_port(sai_bridge_api_t* ba, sai_object_id_t s, sai_switch_api_t* sw, sai_object_id_t port)
{
    sai_attribute_t a;
    a.id = SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID;
    if (sw->get_switch_attribute(s, 1, &a) != SAI_STATUS_SUCCESS) return 0;
    sai_object_id_t bridge = a.value.oid, bps[64];
    a.id = SAI_BRIDGE_ATTR_PORT_LIST;
    a.value.objlist.count = 64;
    a.value.objlist.list = bps;
    if (ba->get_bridge_attribute(bridge, 1, &a) != SAI_STATUS_SUCCESS) return 0;
    for (uint32_t i = 0; i < a.value.objlist.count; i++) {
        sai_attribute_t q;
        q.id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
        if (ba->get_bridge_port_attribute(bps[i], 1, &q) == SAI_STATUS_SUCCESS && q.value.oid == port) return bps[i];
    }
    return 0;
}

static sai_object_id_t make_vlan_member(sai_vlan_api_t* va, sai_object_id_t s, sai_object_id_t vlan, sai_object_id_t bp, bool tagged)
{
    sai_attribute_t vm[3];
    vm[0].id = SAI_VLAN_MEMBER_ATTR_VLAN_ID;
    vm[0].value.oid = vlan;
    vm[1].id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
    vm[1].value.oid = bp;
    vm[2].id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
    vm[2].value.s32 = tagged ? SAI_VLAN_TAGGING_MODE_TAGGED : SAI_VLAN_TAGGING_MODE_UNTAGGED;
    sai_object_id_t m = 0;
    return va->create_vlan_member(&m, s, 3, vm) == SAI_STATUS_SUCCESS ? m : 0;
}


static sai_object_id_t make_member(sai_lag_api_t* la, sai_object_id_t s, sai_object_id_t lag, sai_object_id_t port,
                                   bool egress_disable = false, bool ingress_disable = false)
{
    sai_attribute_t m[4];
    m[0].id = SAI_LAG_MEMBER_ATTR_LAG_ID;
    m[0].value.oid = lag;
    m[1].id = SAI_LAG_MEMBER_ATTR_PORT_ID;
    m[1].value.oid = port;
    m[2].id = SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE;
    m[2].value.booldata = egress_disable;
    m[3].id = SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE;
    m[3].value.booldata = ingress_disable;
    sai_object_id_t o = 0;
    return la->create_lag_member(&o, s, 4, m) == SAI_STATUS_SUCCESS ? o : 0;
}

static sai_status_t set_bool(sai_lag_api_t* la, sai_object_id_t member, sai_attr_id_t id, bool v)
{
    sai_attribute_t a;
    a.id = id;
    a.value.booldata = v;
    return la->set_lag_member_attribute(member, &a);
}

// The handle the mock gave the last ifcs_lag_create in the log.
static uint32_t last_lag_handle()
{
    std::string log = m_log();
    size_t p = log.rfind("ifcs_lag_create h=0x");
    return p == std::string::npos ? 0 : (uint32_t)std::strtoul(log.c_str() + p + 18, nullptr, 16);
}

void test_lag(Env& e)
{
    USE_ENV(e);
    int (*m_lag_members)(uint32_t, uint32_t*) = nullptr;
    int (*m_lag_count)() = nullptr;
    int (*m_l2vni_has)(uint16_t, uint32_t) = nullptr;
    uint32_t (*m_l2_dest)(const uint8_t*, uint16_t) = nullptr;
    int (*m_fire_l2_dest)(int, const uint8_t*, uint16_t, uint32_t) = nullptr;
    int (*m_efl_find)(const char*) = nullptr;
    int (*m_efl_count)() = nullptr;
    hook(g_mock, m_lag_members, "mock_lag_members");
    hook(g_mock, m_lag_count, "mock_lag_count");
    hook(g_mock, m_l2vni_has, "mock_l2vni_has");
    hook(g_mock, m_l2_dest, "mock_l2_dest");
    hook(g_mock, m_fire_l2_dest, "mock_fire_l2_dest");
    hook(g_mock, m_efl_find, "mock_efl_find");
    hook(g_mock, m_efl_count, "mock_efl_count");
    const uint32_t sp1 = 0x3e200001, sp3 = 0x3e200003;
    sai_attribute_t a;

    // ---- hash from the profile at init (SAI_TERALYNX_HASH_FIELDS 5-tuple + in_port, SEED 10):
    // one field list per packet type, as isai (IPv4/IPv6 fields per family, L4 ports for TCP/UDP)
    CHECK_EQ(m_efl_count(), 5);
    CHECK(m_efl_find("6,5,7,12,13,16") && m_efl_find("6,5,7,16") && m_efl_find("9,8,10,12,13,16") &&
          m_efl_find("9,8,10,16") && m_efl_find("16"));

    // ---- members: ports 1 and 3 (port 2 keeps t_l3's RIF). orchagent first removes their VLAN
    // members and bridge ports; until then (and for a port with a RIF) the frontend refuses
    sai_object_id_t lag0 = 0;
    OK(la->create_lag(&lag0, s, 0, nullptr));
    CHECK(make_member(la, s, lag0, p1) == 0);                               // port 1: bridge port, VLAN 100
    CHECK(make_member(la, s, lag0, p2) == 0);                               // port 2: router interface
    OK(la->remove_lag(lag0));
    {
        sai_object_id_t ms[8];
        a.id = SAI_VLAN_ATTR_MEMBER_LIST;
        a.value.objlist.count = 8;
        a.value.objlist.list = ms;
        OK(va->get_vlan_attribute(vlan, 1, &a));
        for (uint32_t i = 0; i < a.value.objlist.count; i++) OK(va->remove_vlan_member(ms[i]));
    }
    for (sai_object_id_t p : {p1, p3}) {
        sai_object_id_t bp = find_bridge_port(ba, s, sw, p);
        if (bp) OK(ba->remove_bridge_port(bp));
    }
    bp1 = 0;

    // ---- LAG + member: ifcs_lag_create, ifcs_lag_member_add of the sysport
    m_log_clear();
    sai_object_id_t lag = 0;
    OK(la->create_lag(&lag, s, 0, nullptr));
    uint32_t lh = last_lag_handle();
    CHECK(lh != 0);
    sai_object_id_t lm1 = make_member(la, s, lag, p1);
    CHECK(lm1 != 0);
    uint32_t first = 0;
    CHECK(m_lag_members(lh, &first) == 1 && first == sp1);
    CHECK(has(m_log(), "ifcs_lag_member_add lag=0x" ) && has(m_log(), "first=0x3e200001 attrs=0"));

    // ---- member created egress+ingress disabled: not in the IFCS LAG, learning off
    m_log_clear();
    sai_object_id_t lm3 = make_member(la, s, lag, p3, true, true);
    CHECK(lm3 != 0);
    CHECK_EQ(m_lag_members(lh, nullptr), 1);
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200003 id=28 u32=0"));
    // ingress enabled (egress still disabled): joins (isai: out only while both are disabled)
    OK(set_bool(la, lm3, SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, false));
    CHECK_EQ(m_lag_members(lh, nullptr), 2);
    OK(set_bool(la, lm3, SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, false));
    CHECK_EQ(m_lag_members(lh, nullptr), 2);
    // teamd takes a member out (both flags) and back
    m_log_clear();
    OK(set_bool(la, lm1, SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, true));
    CHECK_EQ(m_lag_members(lh, nullptr), 2);
    OK(set_bool(la, lm1, SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, true));
    CHECK(m_lag_members(lh, &first) == 1 && first == sp3);
    CHECK(has(m_log(), "ifcs_lag_member_remove lag=0x") && has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=28 u32=0"));
    OK(set_bool(la, lm1, SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, false));
    OK(set_bool(la, lm1, SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, false));
    CHECK_EQ(m_lag_members(lh, nullptr), 2);
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=28 u32=1"));

    // ---- the LAG as bridge port: VLAN 200 tagged, VLAN 300 untagged = the LAG handle in the L2VNI,
    // one mc_dest for the LAG; per member: untagged egress, PVID
    sai_attribute_t bpa[3];
    bpa[0].id = SAI_BRIDGE_PORT_ATTR_TYPE;
    bpa[0].value.s32 = SAI_BRIDGE_PORT_TYPE_PORT;
    bpa[1].id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
    bpa[1].value.oid = lag;
    bpa[2].id = SAI_BRIDGE_PORT_ATTR_ADMIN_STATE;
    bpa[2].value.booldata = true;
    sai_object_id_t bpl = 0;
    OK(ba->create_bridge_port(&bpl, s, 3, bpa));
    sai_object_id_t v200 = 0, v300 = 0;
    a.id = SAI_VLAN_ATTR_VLAN_ID;
    a.value.u16 = 200;
    OK(va->create_vlan(&v200, s, 1, &a));
    a.value.u16 = 300;
    OK(va->create_vlan(&v300, s, 1, &a));
    m_log_clear();
    sai_object_id_t vm200 = make_vlan_member(va, s, v200, bpl, true);
    CHECK(vm200 != 0);
    CHECK(m_l2vni_has(200, lh) && !m_l2vni_has(200, sp1) && !m_l2vni_has(200, sp3));
    char want[96];
    std::snprintf(want, sizeof(want), "count=1 dest=0x%x", lh);
    CHECK(has(m_log(), want));                                             // one mc_dest, for the LAG
    std::snprintf(want, sizeof(want), "ifcs_stp_port_add stp=0x100 members=1 attrs=1 state=3 first=0x%x", lh);
    CHECK(!has(m_log(), want));                                            // STP is per sysport (chip: a LAG is refused)
    m_log_clear();
    sai_object_id_t vm300 = make_vlan_member(va, s, v300, bpl, false);
    CHECK(vm300 != 0 && m_l2vni_has(300, lh));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=43 u32=300") &&
          has(m_log(), "ifcs_sysport_attr_set sp=0x3e200003 id=43 u32=300"));
    a.id = SAI_LAG_ATTR_PORT_VLAN_ID;
    a.value.u16 = 300;
    m_log_clear();
    OK(la->set_lag_attribute(lag, &a));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=32 u32=300") &&
          has(m_log(), "ifcs_sysport_attr_set sp=0x3e200003 id=32 u32=300"));

    // ---- FDB on the LAG: static entry -> ENTRY_DEST = LAG handle; SDK events with the LAG handle or a
    // member sysport -> the LAG's bridge port; flush by the LAG's bridge port
    const uint8_t m1[6] = {0x02, 0xaa, 0, 0, 0, 1}, m2[6] = {0x02, 0xaa, 0, 0, 0, 2};
    sai_fdb_entry_t fe;
    std::memset(&fe, 0, sizeof(fe));
    fe.switch_id = s;
    fe.bv_id = v200;
    std::memcpy(fe.mac_address, m1, 6);
    sai_attribute_t fa_[2];
    fa_[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
    fa_[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
    fa_[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    fa_[1].value.oid = bpl;
    OK(fa->create_fdb_entry(&fe, 2, fa_));
    CHECK_EQ(m_l2_dest(m1, 200), lh);
    {
        std::lock_guard<std::mutex> g(g_mu);
        g_fdb_events.clear();
    }
    m_fire_l2_dest(0, m2, 200, lh);
    m_fire_l2_dest(0, m2, 300, sp3);
    CHECK(wait_for([&] { return g_fdb_events.size() >= 2; }));
    {
        std::lock_guard<std::mutex> g(g_mu);
        bool ok = g_fdb_events.size() >= 2;
        for (size_t i = 0; ok && i < 2; i++) {
            const sai_fdb_event_notification_data_t* none = nullptr;
            (void)none;
            ok = g_fdb_events[i].first == SAI_FDB_EVENT_LEARNED;
        }
        CHECK(ok && g_fdb_events[0].second.bv_id == v200 && g_fdb_events[1].second.bv_id == v300);
        g_fdb_events.clear();
    }
    sai_attribute_t fl[2];
    fl[0].id = SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID;
    fl[0].value.oid = bpl;
    fl[1].id = SAI_FDB_FLUSH_ATTR_ENTRY_TYPE;
    fl[1].value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_ALL;
    OK(fa->flush_fdb_entries(s, 2, fl));
    CHECK_EQ(m_l2_dest(m1, 200), 0u);

    // ---- members leave and join while the LAG is in VLANs: the LAG stays an L2VNI member until
    // the last member leaves; a joining member gets the LAG's untagged VLAN and PVID
    m_log_clear();
    OK(la->remove_lag_member(lm3));
    CHECK(m_l2vni_has(200, lh) && m_l2vni_has(300, lh));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200003 id=42 u32=0"));   // untagged egress off
    m_log_clear();
    lm3 = make_member(la, s, lag, p3);
    CHECK(lm3 != 0 && m_lag_members(lh, nullptr) == 2);
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200003 id=43 u32=300") &&
          has(m_log(), "ifcs_sysport_attr_set sp=0x3e200003 id=32 u32=300"));
    m_log_clear();
    OK(la->remove_lag_member(lm3));
    OK(la->remove_lag_member(lm1));
    CHECK(!m_l2vni_has(200, lh) && !m_l2vni_has(300, lh));                   // no member left behind it
    CHECK_EQ(m_lag_members(lh, nullptr), 0);
    lm1 = make_member(la, s, lag, p1);                                    // back: the LAG rejoins both VLANs
    CHECK(lm1 != 0 && m_l2vni_has(200, lh) && m_l2vni_has(300, lh) && !m_l2vni_has(200, sp1));

    // ---- teardown in orchagent's order: VLAN members, bridge port, VLANs, members, LAG
    OK(va->remove_vlan_member(vm200));
    OK(va->remove_vlan_member(vm300));
    CHECK(!m_l2vni_has(200, lh) && !m_l2vni_has(300, lh));
    OK(ba->remove_bridge_port(bpl));
    OK(va->remove_vlan(v200));
    OK(va->remove_vlan(v300));
    a.id = SAI_LAG_ATTR_PORT_VLAN_ID;
    a.value.u16 = 1;
    OK(la->set_lag_attribute(lag, &a));

    // ---- the LAG as port RIF: the frontend's internal VLAN holds the LAG (its members join as the LAG)
    sai_attribute_t ra_[4];
    a.id = SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID;
    OK(sw->get_switch_attribute(s, 1, &a));
    ra_[0].id = SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID;
    ra_[0].value.oid = a.value.oid;
    ra_[1].id = SAI_ROUTER_INTERFACE_ATTR_TYPE;
    ra_[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT;
    ra_[2].id = SAI_ROUTER_INTERFACE_ATTR_PORT_ID;
    ra_[2].value.oid = lag;
    ra_[3].id = SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS;
    const uint8_t rmac[6] = {0x02, 0, 0, 0, 0x0b, 0x01};
    std::memcpy(ra_[3].value.mac, rmac, 6);
    m_log_clear();
    sai_object_id_t rif = 0;
    OK(ra->create_router_interface(&rif, s, 4, ra_));
    std::snprintf(want, sizeof(want), "n=1 first=0x%x", lh);
    std::string log = m_log();
    size_t p = log.find("ifcs_l2vni_member_add vni=");
    bool lag_in_rif_vlan = false;
    while (p != std::string::npos) {
        size_t eol = log.find('\n', p);
        if (log.substr(p, eol - p).find(want) != std::string::npos) lag_in_rif_vlan = true;
        p = log.find("ifcs_l2vni_member_add vni=", p + 1);
    }
    CHECK(lag_in_rif_vlan);
    OK(ra->remove_router_interface(rif));

    m_log_clear();
    OK(la->remove_lag_member(lm1));
    OK(la->remove_lag(lag));
    CHECK(has(m_log(), "ifcs_lag_delete h=0x"));
    CHECK_EQ(m_lag_count(), 0);
}
