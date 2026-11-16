// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, SAI ACL tables (Backend::fp_*, tl_acl.cpp): tables as
// SONiC's aclorch makes them (L3, L3V6), port binding through a table
// group, entries with every match field and action the frontend passes,
// counters, IN_PORTS on part of the bound ports, policers, removal
// (harness: tests/harness.h).
#include "harness.h"

static int (*m_aces)();
static int (*m_bound)();
static void (*m_set_counter)(uint64_t);
static void (*m_fail_next_ace)();

static size_t count(const std::string& log, const std::string& needle)
{
    size_t n = 0;
    for (size_t p = log.find(needle); p != std::string::npos; p = log.find(needle, p + 1)) n++;
    return n;
}

static sai_attribute_t battr(sai_attr_id_t id) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.booldata = true; return a; }
static sai_attribute_t s32attr(sai_attr_id_t id, int32_t v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.s32 = v; return a; }
static sai_attribute_t u32attr(sai_attr_id_t id, uint32_t v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.u32 = v; return a; }
static sai_attribute_t oidattr(sai_attr_id_t id, sai_object_id_t v) { sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.oid = v; return a; }
static sai_attribute_t f_u8(sai_attr_id_t id, uint8_t v, uint8_t m)
{
    sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.aclfield.enable = true;
    a.value.aclfield.data.u8 = v; a.value.aclfield.mask.u8 = m; return a;
}
static sai_attribute_t f_u16(sai_attr_id_t id, uint16_t v, uint16_t m)
{
    sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.aclfield.enable = true;
    a.value.aclfield.data.u16 = v; a.value.aclfield.mask.u16 = m; return a;
}
static sai_attribute_t f_ip4(sai_attr_id_t id, const char* ip, const char* mask)
{
    sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.aclfield.enable = true;
    inet_pton(AF_INET, ip, &a.value.aclfield.data.ip4); inet_pton(AF_INET, mask, &a.value.aclfield.mask.ip4); return a;
}
static sai_attribute_t f_ip6(sai_attr_id_t id, const char* ip, const char* mask)
{
    sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.aclfield.enable = true;
    inet_pton(AF_INET6, ip, a.value.aclfield.data.ip6); inet_pton(AF_INET6, mask, a.value.aclfield.mask.ip6); return a;
}
static sai_attribute_t f_s32(sai_attr_id_t id, int32_t v)
{
    sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.aclfield.enable = true; a.value.aclfield.data.s32 = v; return a;
}
static sai_attribute_t act_s32(sai_attr_id_t id, int32_t v)
{
    sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.aclaction.enable = true; a.value.aclaction.parameter.s32 = v; return a;
}
static sai_attribute_t act_oid(sai_attr_id_t id, sai_object_id_t v)
{
    sai_attribute_t a; std::memset(&a, 0, sizeof(a)); a.id = id; a.value.aclaction.enable = true; a.value.aclaction.parameter.oid = v; return a;
}

void test_acl(Env& e)
{
    USE_ENV(e);
    sai_acl_api_t* acl = e.acla;
    hook(g_mock, m_aces, "mock_acl_aces");
    hook(g_mock, m_bound, "mock_acl_bound");
    hook(g_mock, m_set_counter, "mock_acl_set_counter");
    hook(g_mock, m_fail_next_ace, "mock_acl_fail_next_ace");
    sai_attribute_t a;
    int aces0 = m_aces();

    // ---- L3 table as aclorch: node-scope IFCS table, PORT_SHARING profile
    int32_t bind[2] = {SAI_ACL_BIND_POINT_TYPE_PORT, SAI_ACL_BIND_POINT_TYPE_LAG};
    std::vector<sai_attribute_t> t;
    t.push_back(s32attr(SAI_ACL_TABLE_ATTR_ACL_STAGE, SAI_ACL_STAGE_INGRESS));
    a = s32attr(SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST, 0);
    a.value.s32list.count = 2;
    a.value.s32list.list = bind;
    t.push_back(a);
    for (sai_attr_id_t f : {SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_ID,
                            SAI_ACL_TABLE_ATTR_FIELD_ACL_IP_TYPE, SAI_ACL_TABLE_ATTR_FIELD_SRC_IP, SAI_ACL_TABLE_ATTR_FIELD_DST_IP,
                            SAI_ACL_TABLE_ATTR_FIELD_IP_PROTOCOL, SAI_ACL_TABLE_ATTR_FIELD_L4_SRC_PORT,
                            SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT, SAI_ACL_TABLE_ATTR_FIELD_TCP_FLAGS,
                            SAI_ACL_TABLE_ATTR_FIELD_ICMP_TYPE, SAI_ACL_TABLE_ATTR_FIELD_ICMP_CODE, SAI_ACL_TABLE_ATTR_FIELD_DSCP,
                            SAI_ACL_TABLE_ATTR_FIELD_IN_PORTS})
        t.push_back(battr(f));
    m_log_clear();
    sai_object_id_t t4 = 0;
    OK(acl->create_acl_table(&t4, s, (uint32_t)t.size(), t.data()));
    std::string log = m_log();
    CHECK(has(log, "ifcs_acl_match_profile_create") && has(log, "sharing=1"));
    CHECK(has(log, "list=4,18,27,25,56,28,29,59,60,68"));               // in-port, ether, L3 type, ARP op, proto, IPs, ports, flags
    CHECK(has(log, "ifcs_acl_table_create") && has(log, "scope=0 type=9 size=256 prio=15 meters=256"));
    CHECK(has(log, "ifcs_acl_create h=") && has(log, "scope=0 ib=0"));
    CHECK(has(log, "ifcs_acl_table_acl_add table=") && has(log, " n=1"));

    // ---- bound to Ethernet0 (p1) through a table group
    sai_object_id_t grp = 0;
    int32_t pb[1] = {SAI_ACL_BIND_POINT_TYPE_PORT};
    sai_attribute_t ga[3] = {s32attr(SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE, SAI_ACL_STAGE_INGRESS),
                             s32attr(SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST, 0),
                             s32attr(SAI_ACL_TABLE_GROUP_ATTR_TYPE, SAI_ACL_TABLE_GROUP_TYPE_PARALLEL)};
    ga[1].value.s32list.count = 1;
    ga[1].value.s32list.list = pb;
    OK(acl->create_acl_table_group(&grp, s, 3, ga));
    sai_attribute_t gm[3] = {oidattr(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, grp),
                             oidattr(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, t4),
                             u32attr(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY, 100)};
    sai_object_id_t mem = 0;
    OK(acl->create_acl_table_group_member(&mem, s, 3, gm));
    a = oidattr(SAI_PORT_ATTR_INGRESS_ACL, grp);
    OK(pa->set_port_attribute(p1, &a));

    // ---- drop dst 10.99.0.1 tcp/179, counted
    sai_attribute_t ca[3] = {oidattr(SAI_ACL_COUNTER_ATTR_TABLE_ID, t4), battr(SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT),
                             battr(SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT)};
    sai_object_id_t c1 = 0;
    OK(acl->create_acl_counter(&c1, s, 3, ca));
    m_log_clear();
    sai_attribute_t e1a[7] = {oidattr(SAI_ACL_ENTRY_ATTR_TABLE_ID, t4), u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 4000),
                              f_ip4(SAI_ACL_ENTRY_ATTR_FIELD_DST_IP, "10.99.0.1", "255.255.255.255"),
                              f_u8(SAI_ACL_ENTRY_ATTR_FIELD_IP_PROTOCOL, 6, 0xff),
                              f_u16(SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, 179, 0xffff),
                              act_s32(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, SAI_PACKET_ACTION_DROP),
                              act_oid(SAI_ACL_ENTRY_ATTR_ACTION_COUNTER, c1)};
    sai_object_id_t e1 = 0;
    OK(acl->create_acl_entry(&e1, s, 7, e1a));
    log = m_log();
    CHECK(has(log, "ifcs_acl_port_add acl=") && has(log, "sysport=0x3e200001"));
    CHECK(has(log, "type=29 value=0a630001 object=0x0 mask=ffffffff"));
    CHECK(has(log, "type=56 value=06 object=0x0 mask=ff"));
    CHECK(has(log, "type=60 value=00b3 object=0x0 mask=ffff"));
    CHECK(has(log, "type=drop green=1 yellow=1 red=1"));
    CHECK(has(log, "ifcs_acl_counter_create") && has(log, "mode=0"));
    char pr[64];
    std::snprintf(pr, sizeof(pr), "ib=99 prio=%u matches=3 actions=2", 0xffffffffu - 2 * 4000u);
    CHECK(has(log, pr));                                                  // node scope, isai's priority mapping
    CHECK_EQ(m_aces(), aces0 + 1);
    CHECK_EQ(m_bound(), 1);
    m_set_counter(42);
    sai_attribute_t cg[2];
    cg[0].id = SAI_ACL_COUNTER_ATTR_PACKETS;
    cg[1].id = SAI_ACL_COUNTER_ATTR_BYTES;
    OK(acl->get_acl_counter_attribute(c1, 2, cg));
    CHECK_EQ(cg[0].value.u64, 42u);
    CHECK_EQ(cg[1].value.u64, 42u * 64);
    m_set_counter(0);

    // ---- FORWARD (a zero drop action), TRAP (copy with the ACL trap + drop), COPY
    m_log_clear();
    sai_attribute_t e2a[5] = {oidattr(SAI_ACL_ENTRY_ATTR_TABLE_ID, t4), u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 3900),
                              f_ip4(SAI_ACL_ENTRY_ATTR_FIELD_SRC_IP, "10.0.0.0", "255.0.0.0"),
                              f_u8(SAI_ACL_ENTRY_ATTR_FIELD_TCP_FLAGS, 0x02, 0x3f),
                              act_s32(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, SAI_PACKET_ACTION_FORWARD)};
    sai_object_id_t e2 = 0;
    OK(acl->create_acl_entry(&e2, s, 5, e2a));
    log = m_log();
    CHECK(has(log, "type=28 value=0a000000 object=0x0 mask=ff000000"));
    CHECK(has(log, "type=68 value=02 object=0x0 mask=3f"));
    CHECK(has(log, "type=drop green=0 yellow=0 red=0"));
    m_log_clear();
    sai_attribute_t e3a[4] = {oidattr(SAI_ACL_ENTRY_ATTR_TABLE_ID, t4), u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 3800),
                              f_u16(SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, 9999, 0xffff),
                              act_s32(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, SAI_PACKET_ACTION_TRAP)};
    sai_object_id_t e3 = 0;
    OK(acl->create_acl_entry(&e3, s, 4, e3a));
    log = m_log();
    CHECK(has(log, "ifcs_hostif_trap_create h=") && has(log, "queue=0"));   // the ACL trap, first use
    CHECK(has(log, "type=ctc hostif=0x") && has(log, "green=1 yellow=1 red=1 dirty=f"));
    CHECK(has(log, "type=drop green=1 yellow=1 red=1"));
    m_log_clear();
    e3a[1] = u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 3790);
    e3a[2] = f_u16(SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, 9998, 0xffff);
    e3a[3] = act_s32(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, SAI_PACKET_ACTION_COPY);
    sai_object_id_t e3c = 0;
    OK(acl->create_acl_entry(&e3c, s, 4, e3a));
    log = m_log();
    CHECK(!has(log, "ifcs_hostif_trap_create"));                        // shared
    CHECK(has(log, "type=ctc hostif=0x") && !has(log, "type=drop"));

    // ---- ICMP echo (v4: type/code in the L4 source port field + protocol 1), DSCP, IPV4ANY, VLAN
    m_log_clear();
    sai_attribute_t e4a[7] = {oidattr(SAI_ACL_ENTRY_ATTR_TABLE_ID, t4), u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 3700),
                              f_u8(SAI_ACL_ENTRY_ATTR_FIELD_ICMP_TYPE, 8, 0xff), f_u8(SAI_ACL_ENTRY_ATTR_FIELD_ICMP_CODE, 0, 0xff),
                              f_u8(SAI_ACL_ENTRY_ATTR_FIELD_DSCP, 46, 0x3f), f_s32(SAI_ACL_ENTRY_ATTR_FIELD_ACL_IP_TYPE, SAI_ACL_IP_TYPE_IPV4ANY),
                              act_s32(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, SAI_PACKET_ACTION_DROP)};
    sai_object_id_t e4 = 0;
    OK(acl->create_acl_entry(&e4, s, 7, e4a));
    log = m_log();
    CHECK(has(log, "type=59 value=0800 object=0x0 mask=ffff"));
    CHECK(has(log, "type=56 value=01 object=0x0 mask=ff"));
    CHECK(has(log, "type=54 value=2e object=0x0 mask=3f"));
    CHECK(has(log, "type=27 value=04 object=0x0 mask=06"));
    m_log_clear();
    sai_attribute_t e5a[5] = {oidattr(SAI_ACL_ENTRY_ATTR_TABLE_ID, t4), u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 3600),
                              f_u16(SAI_ACL_ENTRY_ATTR_FIELD_OUTER_VLAN_ID, 100, 0xfff), f_u16(SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE, 0x0800, 0xffff),
                              act_s32(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, SAI_PACKET_ACTION_DROP)};
    sai_object_id_t e5 = 0;
    OK(acl->create_acl_entry(&e5, s, 5, e5a));
    log = m_log();
    CHECK(has(log, "type=21 value=0064 object=0x0 mask=0fff"));
    CHECK(has(log, "type=18 value=0800 object=0x0 mask=ffff"));
    // ARP request: ether type 0x0806 + ARP operation 1; NON_IPV4 has no IFCS form
    m_log_clear();
    e5a[2] = f_s32(SAI_ACL_ENTRY_ATTR_FIELD_ACL_IP_TYPE, SAI_ACL_IP_TYPE_ARP_REQUEST);
    sai_object_id_t e5b = 0;
    OK(acl->create_acl_entry(&e5b, s, 3, e5a));
    // (3 attrs: table, priority, ip type)
    log = m_log();
    CHECK(has(log, "type=18 value=0806 object=0x0 mask=ffff") && has(log, "type=25 value=0001 object=0x0 mask=ffff"));
    if (!has(log, "type=25")) std::fprintf(stderr, "--- ARP entry log ---\n%s", log.c_str());
    e5a[2] = f_s32(SAI_ACL_ENTRY_ATTR_FIELD_ACL_IP_TYPE, SAI_ACL_IP_TYPE_NON_IPV4);
    sai_object_id_t bad = 0;
    CHECK(acl->create_acl_entry(&bad, s, 3, e5a) != SAI_STATUS_SUCCESS);

    // ---- bind Ethernet120-like p2 too: every entry now spans 2 ports, one ACE each;
    // an IN_PORTS [p1] entry covers part of them: one ACE with the port match
    m_log_clear();
    a = oidattr(SAI_PORT_ATTR_INGRESS_ACL, grp);
    OK(pa->set_port_attribute(p2, &a));
    CHECK_EQ(m_bound(), 2);
    CHECK(has(m_log(), "sysport=0x3e200002"));
    int aces1 = m_aces();
    m_log_clear();
    sai_attribute_t e6a[4] = {oidattr(SAI_ACL_ENTRY_ATTR_TABLE_ID, t4), u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 3500), {},
                              act_s32(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, SAI_PACKET_ACTION_DROP)};
    sai_object_id_t inp[1] = {p1};
    e6a[2].id = SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS;
    e6a[2].value.aclfield.enable = true;
    e6a[2].value.aclfield.data.objlist.count = 1;
    e6a[2].value.aclfield.data.objlist.list = inp;
    sai_object_id_t e6 = 0;
    OK(acl->create_acl_entry(&e6, s, 4, e6a));
    log = m_log();
    CHECK(has(log, "type=4 value= object=0x3e200001"));
    CHECK_EQ(m_aces(), aces1 + 1);

    // ---- failed ACE: the entry is refused and leaves nothing behind
    int aces2 = m_aces();
    m_fail_next_ace();
    e6a[1] = u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 3400);
    sai_object_id_t e7 = 0;
    CHECK(acl->create_acl_entry(&e7, s, 4, e6a) != SAI_STATUS_SUCCESS);
    CHECK_EQ(m_aces(), aces2 + 1);                                       // the mock counts the refused one
    CHECK_EQ(m_bound(), 2);

    // ---- policer on an entry: a meter in this table (node scope), METER action, red dropped
    sai_attribute_t p[5];
    p[0].id = SAI_POLICER_ATTR_METER_TYPE; p[0].value.s32 = SAI_METER_TYPE_PACKETS;
    p[1].id = SAI_POLICER_ATTR_MODE; p[1].value.s32 = SAI_POLICER_MODE_SR_TCM;
    p[2].id = SAI_POLICER_ATTR_CIR; p[2].value.u64 = 1000;
    p[3].id = SAI_POLICER_ATTR_CBS; p[3].value.u64 = 1000;
    p[4].id = SAI_POLICER_ATTR_RED_PACKET_ACTION; p[4].value.s32 = SAI_PACKET_ACTION_DROP;
    sai_object_id_t pol = 0;
    OK(e.pola->create_policer(&pol, s, 5, p));
    m_log_clear();
    sai_attribute_t e8a[4] = {oidattr(SAI_ACL_ENTRY_ATTR_TABLE_ID, t4), u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 3300),
                              f_u16(SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, 5001, 0xffff), act_oid(SAI_ACL_ENTRY_ATTR_ACTION_SET_POLICER, pol)};
    sai_object_id_t e8 = 0;
    OK(acl->create_acl_entry(&e8, s, 4, e8a));
    log = m_log();
    CHECK(has(log, "ifcs_acl_meter_create") && has(log, "ib=99 mode=1 cir=672000 cbs=84000"));
    CHECK(has(log, "type=drop green=0 yellow=0 red=1"));
    CHECK(has(log, "type=0 object=0x"));                                 // METER action
    CHECK(e.pola->remove_policer(pol) != SAI_STATUS_SUCCESS);           // in use

    // ---- L3V6 table: v6 addresses (16 bytes), next header, ICMPv6 type in L4 byte 1
    std::vector<sai_attribute_t> t6;
    t6.push_back(s32attr(SAI_ACL_TABLE_ATTR_ACL_STAGE, SAI_ACL_STAGE_INGRESS));
    for (sai_attr_id_t f : {SAI_ACL_TABLE_ATTR_FIELD_SRC_IPV6, SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6,
                            SAI_ACL_TABLE_ATTR_FIELD_IPV6_NEXT_HEADER, SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT,
                            SAI_ACL_TABLE_ATTR_FIELD_ICMPV6_TYPE, SAI_ACL_TABLE_ATTR_FIELD_IN_PORTS})
        t6.push_back(battr(f));
    sai_object_id_t tv6 = 0;
    m_log_clear();
    OK(acl->create_acl_table(&tv6, s, (uint32_t)t6.size(), t6.data()));
    CHECK(has(m_log(), "prio=14"));                                       // the second table
    sai_attribute_t gm6[3] = {oidattr(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, grp),
                              oidattr(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, tv6),
                              u32attr(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY, 200)};
    sai_object_id_t mem6 = 0;
    OK(acl->create_acl_table_group_member(&mem6, s, 3, gm6));
    m_log_clear();
    sai_attribute_t e9a[5] = {oidattr(SAI_ACL_ENTRY_ATTR_TABLE_ID, tv6), u32attr(SAI_ACL_ENTRY_ATTR_PRIORITY, 4000),
                              f_ip6(SAI_ACL_ENTRY_ATTR_FIELD_DST_IPV6, "2001:db8::1", "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"),
                              f_u8(SAI_ACL_ENTRY_ATTR_FIELD_ICMPV6_TYPE, 128, 0xff),
                              act_s32(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, SAI_PACKET_ACTION_DROP)};
    sai_object_id_t e9 = 0;
    OK(acl->create_acl_entry(&e9, s, 5, e9a));
    log = m_log();
    CHECK(has(log, "type=31 value=20010db8000000000000000000000001 object=0x0 mask=ffffffffffffffffffffffffffffffff"));
    CHECK(has(log, "type=61 value=80 object=0x0 mask=ff") && has(log, "type=56 value=3a object=0x0 mask=ff"));
    CHECK_EQ(count(log, "ifcs_ace_create"), 1u);                        // both ports bound: one ACE

    // ---- removal: entries, unbinding, tables
    for (sai_object_id_t x : {e1, e2, e3, e3c, e4, e5, e5b, e6, e8, e9}) OK(acl->remove_acl_entry(x));
    CHECK_EQ(m_aces(), aces0 + 1);                                       // + the refused one the mock counted
    CHECK_EQ(m_bound(), 0);                                              // no entry: no port bound
    m_log_clear();
    OK(e.pola->remove_policer(pol));
    CHECK(has(m_log(), "ifcs_acl_meter_delete"));
    a = oidattr(SAI_PORT_ATTR_INGRESS_ACL, SAI_NULL_OBJECT_ID);
    OK(pa->set_port_attribute(p1, &a));
    OK(pa->set_port_attribute(p2, &a));
    OK(acl->remove_acl_table_group_member(mem));
    OK(acl->remove_acl_table_group_member(mem6));
    OK(acl->remove_acl_table_group(grp));
    OK(acl->remove_acl_counter(c1));
    m_log_clear();
    OK(acl->remove_acl_table(t4));
    OK(acl->remove_acl_table(tv6));
    log = m_log();
    CHECK_EQ(count(log, "ifcs_acl_table_delete"), 2u);
    CHECK(has(log, "ifcs_acl_table_acl_remove") && has(log, "ifcs_acl_delete"));
}
