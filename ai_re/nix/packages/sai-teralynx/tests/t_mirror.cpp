// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, mirror sessions and sampling (tl_mirror.cpp) on the
// mock libifcs: SPAN (DCDP on the monitor port, collector + set per
// direction on first use, sysport MIRROR_* policy), ERSPAN collector
// attributes, session updates inside the same set, one session per port,
// SAMPLEPACKET (the CPU DCDP, one sFlow collector / set, SFLOW_INGRESS),
// egress sampling refused (genetlink host interfaces: t_sflow.cpp; harness:
// tests/harness.h).
#include "harness.h"

#include <algorithm>

static int (*m_dcdps)();
static int (*m_collectors)();
static int (*m_sets)();
static int (*m_members)();

void test_mirror(Env& e)
{
    USE_ENV(e);
    hook(g_mock, m_dcdps, "mock_mirror_dcdps");
    hook(g_mock, m_collectors, "mock_mirror_collectors");
    hook(g_mock, m_sets, "mock_mirror_sets");
    hook(g_mock, m_members, "mock_mirror_members");
    sai_mirror_api_t* mia;
    sai_samplepacket_api_t* spa;
    OK(sai_api_query(SAI_API_MIRROR, (void**)&mia));
    OK(sai_api_query(SAI_API_SAMPLEPACKET, (void**)&spa));
    sai_attribute_t a;

    // ---- SPAN to p2: a DCDP on p2's sysport at create, nothing else yet
    m_log_clear();
    sai_attribute_t ms[2];
    ms[0].id = SAI_MIRROR_SESSION_ATTR_TYPE; ms[0].value.s32 = SAI_MIRROR_SESSION_TYPE_LOCAL;
    ms[1].id = SAI_MIRROR_SESSION_ATTR_MONITOR_PORT; ms[1].value.oid = p2;
    sai_object_id_t span = 0;
    OK(mia->create_mirror_session(&span, s, 2, ms));
    CHECK(has(m_log(), "ifcs_dup_copy_dest_port_create h=") && has(m_log(), "sysport=0x3e200002"));
    CHECK(m_dcdps() == 1 && m_collectors() == 0 && m_sets() == 0);
    // ingress on p1: collector (SPAN, ingress, dcdp, truncate) + set + member, sysport policy enable
    m_log_clear();
    a.id = SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    a.value.objlist.count = 1;
    a.value.objlist.list = &span;
    OK(pa->set_port_attribute(p1, &a));
    std::string log = m_log();
    CHECK(has(log, "ifcs_collector_create") && has(log, " n=4 0=0x0 1=0x0 2=0x") && has(log, " 18=0x0"));
    CHECK(has(log, "ifcs_collector_set_create") && has(log, "type=0 dir=0") && has(log, "ifcs_collector_set_member_add"));
    CHECK(has(log, "ifcs_sysport_attr_set sp=0x3e200001 id=22 u32=0"));         // MIRROR_INGRESS, action ENABLE
    CHECK(m_collectors() == 1 && m_sets() == 1 && m_members() == 1);
    // egress on p1: its own collector (no truncate attr) and set
    m_log_clear();
    a.id = SAI_PORT_ATTR_EGRESS_MIRROR_SESSION;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), " n=3 0=0x0 1=0x1 2=0x") && !has(m_log(), " 18=") && has(m_log(), "type=0 dir=1") && has(m_log(), "id=24 u32=0"));   // no TRUNCATE on egress (refused)
    CHECK(m_collectors() == 2 && m_sets() == 2);
    // a second port on the same session and direction shares the set
    a.id = SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    OK(pa->set_port_attribute(p3, &a));
    CHECK(m_sets() == 2);
    // two sessions on one port and direction: refused
    sai_object_id_t span2 = 0;
    OK(mia->create_mirror_session(&span2, s, 2, ms));
    CHECK_EQ(m_dcdps(), 1);                                                       // same monitor port: one DCDP
    sai_object_id_t two[2] = {span, span2};
    a.value.objlist.count = 2;
    a.value.objlist.list = two;
    CHECK_EQ(pa->set_port_attribute(p1, &a), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    OK(mia->remove_mirror_session(span2));
    // monitor port change: new DCDP, collectors re-made inside the same sets
    m_log_clear();
    a.id = SAI_MIRROR_SESSION_ATTR_MONITOR_PORT;
    a.value.oid = p3;
    OK(mia->set_mirror_session_attribute(span, &a));
    log = m_log();
    CHECK(has(log, "sysport=0x3e200003") && has(log, "ifcs_collector_set_member_remove") && has(log, "ifcs_dup_copy_dest_port_delete"));
    CHECK(!has(log, "ifcs_collector_set_create") && !has(log, "ifcs_collector_set_delete"));
    CHECK(m_dcdps() == 1 && m_collectors() == 2 && m_sets() == 2 && m_members() == 2);
    CHECK_EQ(mia->remove_mirror_session(span), SAI_STATUS_OBJECT_IN_USE);
    // off: policy DISABLE (action 1) on the same set; the set goes with its last user
    m_log_clear();
    a.id = SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    a.value.objlist.count = 0;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=22 u32=1"));
    CHECK_EQ(m_sets(), 2);                                                        // p3 still uses it
    OK(pa->set_port_attribute(p3, &a));
    a.id = SAI_PORT_ATTR_EGRESS_MIRROR_SESSION;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(m_collectors() == 0 && m_sets() == 0 && m_members() == 0);
    OK(mia->remove_mirror_session(span));
    CHECK_EQ(m_dcdps(), 0);

    // ---- ERSPAN: type II collector with MACs, IPs, DSCP = TOS >> 2, TTL, session 0
    sai_attribute_t er[10];
    er[0].id = SAI_MIRROR_SESSION_ATTR_TYPE; er[0].value.s32 = SAI_MIRROR_SESSION_TYPE_ENHANCED_REMOTE;
    er[1].id = SAI_MIRROR_SESSION_ATTR_MONITOR_PORT; er[1].value.oid = p2;
    er[2].id = SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE; er[2].value.s32 = SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL;
    er[3].id = SAI_MIRROR_SESSION_ATTR_TOS; er[3].value.u8 = 32;
    er[4].id = SAI_MIRROR_SESSION_ATTR_TTL; er[4].value.u8 = 64;
    er[5].id = SAI_MIRROR_SESSION_ATTR_SRC_IP_ADDRESS;
    er[5].value.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    er[5].value.ipaddr.addr.ip4 = htonl(0x0a630101);
    er[6].id = SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS;
    er[6].value.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    er[6].value.ipaddr.addr.ip4 = htonl(0x0a630102);
    er[7].id = SAI_MIRROR_SESSION_ATTR_SRC_MAC_ADDRESS;
    er[8].id = SAI_MIRROR_SESSION_ATTR_DST_MAC_ADDRESS;
    for (int k = 0; k < 6; k++) { er[7].value.mac[k] = (uint8_t)(0x10 + k); er[8].value.mac[k] = (uint8_t)(0x20 + k); }
    er[9].id = SAI_MIRROR_SESSION_ATTR_GRE_PROTOCOL_TYPE; er[9].value.u16 = 0x88be;
    sai_object_id_t ers = 0;
    OK(mia->create_mirror_session(&ers, s, 10, er));
    m_log_clear();
    a.id = SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    a.value.objlist.count = 1;
    a.value.objlist.list = &ers;
    OK(pa->set_port_attribute(p1, &a));
    log = m_log();
    CHECK(has(log, " 0=0x3 1=0x0 2=0x") && has(log, " 9=202122232425 10=101112131415 11=0a630101 12=0a630102 13=8 14=64 17=0"));
    CHECK(!has(log, " 18="));                                                      // no TRUNCATE on ERSPAN (0.12.41 refuses it)
    CHECK(has(log, "type=3 dir=0"));
    m_log_clear();
    a.id = SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS;
    a.value.ipaddr = er[6].value.ipaddr;
    a.value.ipaddr.addr.ip4 = htonl(0x0a630103);
    OK(mia->set_mirror_session_attribute(ers, &a));
    CHECK(has(m_log(), " 12=0a630103 ") && !has(m_log(), "ifcs_dup_copy_dest_port_create"));
    a.id = SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    a.value.objlist.count = 0;
    OK(pa->set_port_attribute(p1, &a));
    OK(mia->remove_mirror_session(ers));
    CHECK(m_dcdps() == 0 && m_collectors() == 0 && m_sets() == 0);

    // ---- SAMPLEPACKET: CPU DCDP + sFlow collector (isai's tags) + set, SFLOW_INGRESS on the port
    sai_attribute_t sp;
    sp.id = SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE;
    sp.value.u32 = 1000;
    sai_object_id_t smp = 0;
    m_log_clear();
    OK(spa->create_samplepacket(&smp, s, 1, &sp));
    CHECK(!has(m_log(), "ifcs_"));
    a.id = SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
    a.value.oid = smp;
    OK(pa->set_port_attribute(p1, &a));
    log = m_log();
    CHECK(has(log, "sysport=0x3e2003e8") && has(log, " 0=0x5 1=0x0 2=0x") && has(log, " 13=2 14=100 17=111"));
    CHECK(has(log, "type=5 dir=0") && has(log, "ifcs_sysport_attr_set sp=0x3e200001 id=25 u32=0"));
    OK(pa->set_port_attribute(p2, &a));                                           // shared collector / set
    CHECK(m_collectors() == 1 && m_sets() == 1 && m_dcdps() == 1);
    m_log_clear();
    sp.value.u32 = 500;
    OK(spa->set_samplepacket_attribute(smp, &sp));
    CHECK_EQ((int)std::count(m_log(), m_log() + std::strlen(m_log()), '\n'), 2);   // both ports' policy re-set
    a.id = SAI_PORT_ATTR_EGRESS_SAMPLEPACKET_ENABLE;
    CHECK_EQ(pa->set_port_attribute(p1, &a), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    CHECK_EQ(spa->remove_samplepacket(smp), SAI_STATUS_OBJECT_IN_USE);
    m_log_clear();
    a.id = SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE;
    a.value.oid = 0;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=25 u32=1"));
    OK(pa->set_port_attribute(p2, &a));
    CHECK(m_collectors() == 0 && m_sets() == 0 && m_dcdps() == 0);
    OK(spa->remove_samplepacket(smp));
}
