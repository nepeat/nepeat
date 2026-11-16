// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, L2: VLANs, members (per-member mc_dests, the CPU in no
// VLAN), FDB entries and events, flush by port / VLAN / all, aging (harness: tests/harness.h).
#include "harness.h"

static sai_fdb_entry_t fdb(sai_object_id_t s, sai_object_id_t vlan, const uint8_t mac[6])
{
    sai_fdb_entry_t fe;
    std::memset(&fe, 0, sizeof(fe));
    fe.switch_id = s;
    fe.bv_id = vlan;
    std::memcpy(fe.mac_address, mac, 6);
    return fe;
}

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

void test_l2(Env& e)
{
    USE_ENV(e);
    int (*m_l2vni_has)(uint16_t, uint32_t) = nullptr;
    uint32_t (*m_l2_dest)(const uint8_t*, uint16_t) = nullptr;
    hook(g_mock, m_l2vni_has, "mock_l2vni_has");
    hook(g_mock, m_l2_dest, "mock_l2_dest");
    sai_attribute_t a;
    const uint32_t cpu_sp = 0x3e2003e8, sp1 = 0x3e200001, sp2 = 0x3e200002;

    // ---- VLAN 100: L2VNI with STP, flood group, learning on (isai's attributes); the CPU is in no VLAN
    m_log_clear();
    vlan = 0;
    a.id = SAI_VLAN_ATTR_VLAN_ID;
    a.value.u16 = 100;
    OK(va->create_vlan(&vlan, s, 1, &a));
    CHECK(has(m_log(), "ifcs_l2vni_create vni=100 count=4 stp=0x100"));
    CHECK(!m_l2vni_has(100, cpu_sp));
    CHECK(!has(m_log(), "ifcs_mc_dest_create"));

    // ---- port 1 untagged: L2VNI member + its own mc_dest in the flood group + untagged egress
    bp1 = find_bridge_port(ba, s, sw, p1);
    CHECK(bp1 != 0);
    sai_object_id_t member = make_vlan_member(va, s, vlan, bp1, false);
    CHECK(member != 0);
    CHECK(m_l2vni_has(100, sp1));
    CHECK(has(m_log(), "ifcs_mc_dest_create h=0x") && has(m_log(), "dest=0x3e200001"));
    CHECK(has(m_log(), "ifcs_mdg_mc_dest_add mdg="));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=43 u32=100"));

    // ---- port 2 tagged in VLAN 100 and in VLAN 101; untagged in a second VLAN is refused (one per port)
    sai_object_id_t bp2 = find_bridge_port(ba, s, sw, p2);
    CHECK(bp2 != 0);
    sai_object_id_t v101 = 0;
    a.value.u16 = 101;
    OK(va->create_vlan(&v101, s, 1, &a));
    sai_object_id_t m2 = make_vlan_member(va, s, vlan, bp2, true);
    sai_object_id_t m2b = make_vlan_member(va, s, v101, bp2, true);
    CHECK(m2 && m2b && m_l2vni_has(100, sp2) && m_l2vni_has(101, sp2));
    CHECK(make_vlan_member(va, s, v101, bp1, false) == 0);   // port 1 is untagged in 100
    m_log_clear();
    OK(va->remove_vlan_member(m2b));
    CHECK(!m_l2vni_has(101, sp2) && m_l2vni_has(100, sp2));
    CHECK(has(m_log(), "ifcs_mdg_mc_dest_remove") && has(m_log(), "ifcs_mc_dest_delete") &&
          has(m_log(), "ifcs_l2vni_member_remove vni=101 n=1 first=0x3e200002"));
    OK(va->remove_vlan(v101));
    CHECK(has(m_log(), "ifcs_l2vni_delete vni=101"));

    // ---- FDB events from the SDK -> SAI notifications (learned, aged)
    const uint8_t mac[6] = {0x02, 0x11, 0x22, 0x33, 0x44, 0x55};
    {
        std::lock_guard<std::mutex> g(g_mu);
        g_fdb_events.clear();
    }
    m_fire_l2(0, mac, 100, 1);
    m_fire_l2(1, mac, 100, 1);
    CHECK(wait_for([&] { return g_fdb_events.size() >= 2; }));
    {
        std::lock_guard<std::mutex> g(g_mu);
        CHECK(g_fdb_events.size() >= 2 && g_fdb_events[0].first == SAI_FDB_EVENT_LEARNED &&
              g_fdb_events[0].second.bv_id == vlan && g_fdb_events[1].first == SAI_FDB_EVENT_AGED);
        g_fdb_events.clear();
    }

    // ---- static / dynamic / drop entries; flush by bridge port, by VLAN, all
    const uint8_t ma[6] = {0x02, 0, 0, 0, 0, 0x91}, mb[6] = {0x02, 0, 0, 0, 0, 0x92}, mc[6] = {0x02, 0, 0, 0, 0, 0x93};
    sai_attribute_t fa_[3];
    fa_[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
    fa_[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
    fa_[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    fa_[1].value.oid = bp1;
    fa_[2].id = SAI_FDB_ENTRY_ATTR_PACKET_ACTION;
    fa_[2].value.s32 = SAI_PACKET_ACTION_DROP;
    sai_fdb_entry_t fea = fdb(s, vlan, ma), feb = fdb(s, vlan, mb), fec = fdb(s, vlan, mc);
    int l2_0 = m_l2_count();
    OK(fa->create_fdb_entry(&fea, 2, fa_));
    CHECK_EQ(m_l2_dest(ma, 100), sp1);
    m_log_clear();
    OK(fa->create_fdb_entry(&fec, 3, fa_));                               // drop: FWD_POLICY added
    CHECK(has(m_log(), "vni=100 count=3 dest=0x3e200001 etype=0"));
    fa_[0].value.s32 = SAI_FDB_ENTRY_TYPE_DYNAMIC;
    fa_[1].value.oid = bp2;
    OK(fa->create_fdb_entry(&feb, 2, fa_));
    CHECK_EQ(m_l2_dest(mb, 100), sp2);
    CHECK_EQ(m_l2_count(), l2_0 + 3);
    sai_attribute_t fl[2];
    fl[0].id = SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID;
    fl[0].value.oid = bp2;
    fl[1].id = SAI_FDB_FLUSH_ATTR_ENTRY_TYPE;
    fl[1].value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_STATIC;
    OK(fa->flush_fdb_entries(s, 2, fl));                                  // port 2 has only a dynamic one
    CHECK_EQ(m_l2_count(), l2_0 + 3);
    fl[1].value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC;
    OK(fa->flush_fdb_entries(s, 2, fl));
    CHECK_EQ(m_l2_count(), l2_0 + 2);
    CHECK_EQ(m_l2_dest(mb, 100), 0u);
    fl[0].id = SAI_FDB_FLUSH_ATTR_BV_ID;
    fl[0].value.oid = vlan;
    fl[1].value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_ALL;
    OK(fa->flush_fdb_entries(s, 2, fl));
    CHECK_EQ(m_l2_count(), l2_0);
    OK(fa->create_fdb_entry(&fea, 2, fa_));
    a.id = SAI_FDB_FLUSH_ATTR_ENTRY_TYPE;
    a.value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_ALL;
    OK(fa->flush_fdb_entries(s, 1, &a));
    CHECK_EQ(m_l2_count(), 0);
    OK(va->remove_vlan_member(m2));
    CHECK(!m_l2vni_has(100, sp2));

    // ---- aging (seconds)
    a.id = SAI_SWITCH_ATTR_FDB_AGING_TIME;
    a.value.u32 = 600;
    OK(sw->set_switch_attribute(s, &a));
    CHECK_EQ(m_aging(), 600u);
}
