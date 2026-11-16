// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, host interface traps + CoPP (Backend::trap_program):
// hostif traps with the group queue, the CoPP ACL (one per inno block; the
// mock has 6), trap entries with copy / drop / meter / counter, policer
// updates, system traps (TTL, MTU), IP2ME-based traps (harness: tests/harness.h).
#include "harness.h"

static int (*m_aces)();
static int (*m_meters)();

static size_t count(const std::string& log, const std::string& needle)
{
    size_t n = 0;
    for (size_t p = log.find(needle); p != std::string::npos; p = log.find(needle, p + 1)) n++;
    return n;
}

static sai_object_id_t make_trap(Env& e, int32_t type, int32_t action, sai_object_id_t group, sai_status_t* st = nullptr)
{
    sai_attribute_t t[3];
    t[0].id = SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE;
    t[0].value.s32 = type;
    t[1].id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION;
    t[1].value.s32 = action;
    t[2].id = SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP;
    t[2].value.oid = group;
    sai_object_id_t id = 0;
    sai_status_t r = e.ha->create_hostif_trap(&id, e.s, 3, t);
    if (st) *st = r;
    else OK(r);
    return id;
}

void test_copp(Env& e)
{
    USE_ENV(e);
    hook(g_mock, m_aces, "mock_acl_aces");
    hook(g_mock, m_meters, "mock_acl_meters");
    sai_policer_api_t* pol = e.pola;
    sai_attribute_t a;

    // policer 600 pps, red drop -> group on queue 4
    sai_attribute_t p[5];
    p[0].id = SAI_POLICER_ATTR_METER_TYPE; p[0].value.s32 = SAI_METER_TYPE_PACKETS;
    p[1].id = SAI_POLICER_ATTR_MODE; p[1].value.s32 = SAI_POLICER_MODE_SR_TCM;
    p[2].id = SAI_POLICER_ATTR_CIR; p[2].value.u64 = 600;
    p[3].id = SAI_POLICER_ATTR_CBS; p[3].value.u64 = 600;
    p[4].id = SAI_POLICER_ATTR_RED_PACKET_ACTION; p[4].value.s32 = SAI_PACKET_ACTION_DROP;
    sai_object_id_t pid = 0;
    OK(pol->create_policer(&pid, s, 5, p));
    sai_attribute_t g[2];
    g[0].id = SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE; g[0].value.u32 = 4;
    g[1].id = SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER; g[1].value.oid = pid;
    sai_object_id_t grp = 0;
    OK(ha->create_hostif_trap_group(&grp, s, 2, g));

    // ---- LLDP (TRAP): CoPP table on first use, 3 rules x 6 IBs
    m_log_clear();
    sai_object_id_t lldp = make_trap(e, SAI_HOSTIF_TRAP_TYPE_LLDP, SAI_PACKET_ACTION_TRAP, grp);
    std::string log = m_log();
    CHECK(has(log, "ifcs_acl_table_create") && has(log, "scope=1 type=1 size=128"));
    CHECK_EQ(count(log, "ifcs_acl_create h="), 6u);
    CHECK(has(log, "ifcs_acl_create ib=6 refused"));
    CHECK(has(log, "ifcs_acl_table_acl_add table=") && has(log, " n=6"));
    CHECK(has(log, "ifcs_hostif_trap_create h=") && has(log, "queue=4"));
    CHECK_EQ(count(log, "ifcs_acl_meter_create"), 6u);
    CHECK(has(log, "ib=0 mode=1 cir=403200 cbs=50400"));      // 600 pps at 84 B, as isai converts
    CHECK_EQ(count(log, "type=17 value=0180c200000e"), 6u);
    CHECK_EQ(count(log, "type=18 value=88cc"), 18u);
    CHECK(has(log, "type=ctc hostif=0x") && has(log, "green=1 yellow=1 red=0 dirty=f"));
    CHECK_EQ(count(log, "type=ctc_override green=0 yellow=0 red=1"), 18u);
    CHECK_EQ(count(log, "type=drop green=1 yellow=1 red=1"), 18u);
    CHECK_EQ(count(log, "matches=2 actions=5"), 18u);         // ctc, override, drop, meter, counter
    CHECK_EQ(m_aces(), 18);
    CHECK_EQ(m_meters(), 6);

    // policer CIR change: every meter of it updated, entries kept
    m_log_clear();
    a.id = SAI_POLICER_ATTR_CIR;
    a.value.u64 = 10;
    OK(pol->set_policer_attribute(pid, &a));
    CHECK_EQ(count(m_log(), "ifcs_acl_meter_attr_set") >= 6, true);
    CHECK(has(m_log(), "id=7 u64=6720"));
    CHECK_EQ(m_aces(), 18);

    // action DROP: entries re-installed without the copy
    m_log_clear();
    a.id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION;
    a.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(ha->set_hostif_trap_attribute(lldp, &a));
    CHECK_EQ(count(m_log(), "type=ctc "), 0u);
    CHECK_EQ(count(m_log(), "type=drop green=1"), 18u);
    CHECK_EQ(m_aces(), 18);
    a.value.s32 = SAI_PACKET_ACTION_TRAP;
    OK(ha->set_hostif_trap_attribute(lldp, &a));

    // ---- TTL error: system trap 17 (queue + enable) + one entry on it; MTU: no entry
    sai_attribute_t g2[1];
    g2[0].id = SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE;
    g2[0].value.u32 = 1;
    sai_object_id_t grp1 = 0;
    OK(ha->create_hostif_trap_group(&grp1, s, 1, g2));
    m_log_clear();
    int aces = m_aces();
    make_trap(e, SAI_HOSTIF_TRAP_TYPE_TTL_ERROR, SAI_PACKET_ACTION_TRAP, grp1);
    CHECK(has(m_log(), "ifcs_hostif_trap_attr_set h=0x18200011 id=0 u32=1"));
    CHECK(has(m_log(), "ifcs_hostif_trap_attr_set h=0x18200011 id=1 u32=1"));
    CHECK(has(m_log(), "type=98 value= object=0x18200011"));
    CHECK_EQ(m_aces(), aces + 6);
    CHECK_EQ(m_meters(), 6);                                    // no policer in this group
    m_log_clear();
    make_trap(e, SAI_HOSTIF_TRAP_TYPE_L3_MTU_ERROR, SAI_PACKET_ACTION_TRAP, grp1);
    CHECK(has(m_log(), "ifcs_hostif_trap_attr_set h=0x18200017 id=1 u32=1"));
    CHECK_EQ(m_aces(), aces + 6);

    // ---- IP2ME: the TRAP routes' hostif trap gets the group queue; BGP = ip2me + port 179
    m_log_clear();
    make_trap(e, SAI_HOSTIF_TRAP_TYPE_IP2ME, SAI_PACKET_ACTION_TRAP, grp1);
    std::string l2 = m_log();
    size_t q = l2.find("ifcs_hostif_trap_attr_set h=");
    CHECK(q != std::string::npos && l2.find("id=0 u32=1", q) != std::string::npos);
    std::string ip2me = q != std::string::npos ? l2.substr(q + 28, 10) : "";
    CHECK(has(l2, "type=98 value= object=" + ip2me.substr(0, ip2me.find(' '))));
    m_log_clear();
    make_trap(e, SAI_HOSTIF_TRAP_TYPE_BGP, SAI_PACKET_ACTION_TRAP, grp);
    CHECK_EQ(count(m_log(), "type=60 value=00b3"), 6u);         // dst port 179
    CHECK_EQ(count(m_log(), "type=59 value=00b3"), 6u);         // src port 179
    CHECK_EQ(count(m_log(), "type=98 value= object=" + ip2me.substr(0, ip2me.find(' '))), 12u);

    // ---- a trap type without a mapping is kept, not programmed
    m_log_clear();
    aces = m_aces();
    make_trap(e, SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY, SAI_PACKET_ACTION_TRAP, grp);
    CHECK_EQ(m_aces(), aces);
    CHECK(!has(m_log(), "ifcs_acl_"));

    // ---- group queue change: the trap follows (attr_set on its hostif trap)
    m_log_clear();
    a.id = SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE;
    a.value.u32 = 5;
    OK(ha->set_hostif_trap_group_attribute(grp, &a));
    CHECK(has(m_log(), "ifcs_hostif_trap_attr_set") && has(m_log(), "id=0 u32=5"));

    // ---- remove LLDP: its 18 entries and its hostif trap go
    aces = m_aces();
    m_log_clear();
    OK(ha->remove_hostif_trap(lldp));
    CHECK_EQ(m_aces(), aces - 18);
    CHECK(has(m_log(), "ifcs_acl_ace_remove") && has(m_log(), "ifcs_hostif_trap_delete"));
    // a used policer cannot go; the meters stay with it
    CHECK(pol->remove_policer(pid) != SAI_STATUS_SUCCESS);
}
