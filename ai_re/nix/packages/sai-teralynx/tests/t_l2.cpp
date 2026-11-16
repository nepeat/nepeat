// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, L2: VLANs, members, FDB entries and events, flush, aging (harness: tests/harness.h).
#include "harness.h"

void test_l2(Env& e)
{
    USE_ENV(e);
    sai_attribute_t a;
    // ---- VLAN 100 with port 1 untagged
    vlan = 0;
    a.id = SAI_VLAN_ATTR_VLAN_ID;
    a.value.u16 = 100;
    OK(va->create_vlan(&vlan, s, 1, &a));
    CHECK(has(m_log(), "ifcs_l2vni_create vni=100 count=2"));
    CHECK(has(m_log(), "ifcs_l2vni_member_add vni=100 n=1 first=0x3e2003e8"));
    sai_object_id_t bridge = 0, bps[8];
    a.id = SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID;
    OK(sw->get_switch_attribute(s, 1, &a));
    bridge = a.value.oid;
    a.id = SAI_BRIDGE_ATTR_PORT_LIST;
    a.value.objlist.count = 8;
    a.value.objlist.list = bps;
    OK(ba->get_bridge_attribute(bridge, 1, &a));
    bp1 = 0;
    for (uint32_t i = 0; i < a.value.objlist.count; i++) {
        sai_attribute_t q;
        q.id = SAI_BRIDGE_PORT_ATTR_PORT_ID;
        if (ba->get_bridge_port_attribute(bps[i], 1, &q) == SAI_STATUS_SUCCESS && q.value.oid == p1) bp1 = bps[i];
    }
    CHECK(bp1 != 0);
    sai_attribute_t vm[3];
    vm[0].id = SAI_VLAN_MEMBER_ATTR_VLAN_ID;
    vm[0].value.oid = vlan;
    vm[1].id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
    vm[1].value.oid = bp1;
    vm[2].id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
    vm[2].value.s32 = SAI_VLAN_TAGGING_MODE_UNTAGGED;
    sai_object_id_t member = 0;
    OK(va->create_vlan_member(&member, s, 3, vm));
    CHECK(has(m_log(), "ifcs_l2vni_member_add vni=100 n=1 first=0x3e200001"));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=43 u32=100"));

    // FDB: learned event from the SDK -> SAI notification; static entry + flush
    const uint8_t mac[6] = {0x02, 0x11, 0x22, 0x33, 0x44, 0x55};
    m_fire_l2(0, mac, 100, 1);
    CHECK(wait_for([&] { return !g_fdb_events.empty(); }));
    {
        std::lock_guard<std::mutex> g(g_mu);
        CHECK(!g_fdb_events.empty() && g_fdb_events[0].first == SAI_FDB_EVENT_LEARNED &&
              g_fdb_events[0].second.bv_id == vlan);
    }
    sai_fdb_entry_t fe;
    std::memset(&fe, 0, sizeof(fe));
    fe.switch_id = s;
    fe.bv_id = vlan;
    const uint8_t mac2[6] = {0x02, 0, 0, 0, 0, 0x99};
    std::memcpy(fe.mac_address, mac2, 6);
    sai_attribute_t fa_[2];
    fa_[0].id = SAI_FDB_ENTRY_ATTR_TYPE;
    fa_[0].value.s32 = SAI_FDB_ENTRY_TYPE_STATIC;
    fa_[1].id = SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID;
    fa_[1].value.oid = bp1;
    OK(fa->create_fdb_entry(&fe, 2, fa_));
    CHECK_EQ(m_l2_count(), 1);
    a.id = SAI_FDB_FLUSH_ATTR_ENTRY_TYPE;
    a.value.s32 = SAI_FDB_FLUSH_ENTRY_TYPE_ALL;
    OK(fa->flush_fdb_entries(s, 1, &a));
    CHECK_EQ(m_l2_count(), 0);
    a.id = SAI_SWITCH_ATTR_FDB_AGING_TIME;
    a.value.u32 = 600;
    OK(sw->set_switch_attribute(s, &a));
    CHECK_EQ(m_aging(), 600u);

}
