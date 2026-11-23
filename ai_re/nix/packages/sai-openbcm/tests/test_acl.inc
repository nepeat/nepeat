// SPDX-License-Identifier: Apache-2.0
// Included by test_sai.cpp: POLICER, ACL (tables, entries, counters, table
// groups, PORT/LAG binding) and CoPP policing on the fake's field processor.

#include <algorithm>

static sai_policer_api_t* poa;
static sai_acl_api_t* aa;

static sai_object_id_t mk_policer(uint64_t cir, uint64_t cbs, int32_t red = SAI_PACKET_ACTION_DROP)
{
    sai_attribute_t a[5];
    a[0].id = SAI_POLICER_ATTR_METER_TYPE; a[0].value.s32 = SAI_METER_TYPE_PACKETS;
    a[1].id = SAI_POLICER_ATTR_MODE; a[1].value.s32 = SAI_POLICER_MODE_SR_TCM;
    a[2].id = SAI_POLICER_ATTR_CIR; a[2].value.u64 = cir;
    a[3].id = SAI_POLICER_ATTR_CBS; a[3].value.u64 = cbs;
    a[4].id = SAI_POLICER_ATTR_RED_PACKET_ACTION; a[4].value.s32 = red;
    sai_object_id_t p = 0;
    OK(poa->create_policer(&p, g_sw, 5, a));
    return p;
}

static int fake_policer(sai_object_id_t p) { return tp::backend_id(p); }
static bool has(const tp::FpEntry& e, tp::Qual q) { return (e.present >> q) & 1u; }
static tp::Policer policer_of(int pid)
{
    tp::Policer p{};
    tp::policer(pid, p);
    return p;
}
static tp::FpGroup fp_group_of(int gid)
{
    tp::FpGroup g{};
    tp::fp_group(gid, g);
    return g;
}

// Backend FP entries in the CoPP group.
static std::vector<tp::FpEntry> copp_entries()
{
    std::vector<tp::FpEntry> v;
    int gid = tp::copp_group();
    for (auto& e : tp::fp_entries())
        if (e.gid == gid) v.push_back(e);
    return v;
}

static sai_object_id_t mk_trap(int32_t type, int32_t action, sai_object_id_t group)
{
    sai_attribute_t a[3];
    a[0].id = SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE; a[0].value.s32 = type;
    a[1].id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION; a[1].value.s32 = action;
    a[2].id = SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP; a[2].value.oid = group;
    sai_object_id_t t = 0;
    OK(ha->create_hostif_trap(&t, g_sw, 3, a));
    return t;
}

static void test_policer_copp()
{
    OK(sai_api_query(SAI_API_POLICER, (void**)&poa));
    OK(sai_api_query(SAI_API_ACL, (void**)&aa));
    // policer objects: CoppOrch's attributes
    sai_object_id_t p = mk_policer(600, 600);
    {
        tp::Policer f = policer_of(fake_policer(p));
        CHECK(f.packets && !f.tr_tcm && f.cir == 600 && f.cbs == 600 && !f.color_blind);
    }
    sai_attribute_t ga[3];
    ga[0].id = SAI_POLICER_ATTR_CIR;
    ga[1].id = SAI_POLICER_ATTR_RED_PACKET_ACTION;
    ga[2].id = SAI_POLICER_ATTR_COLOR_SOURCE;
    OK(poa->get_policer_attribute(p, 3, ga));
    CHECK_EQ(ga[0].value.u64, 600u);
    CHECK_EQ(ga[1].value.s32, SAI_PACKET_ACTION_DROP);
    CHECK_EQ(ga[2].value.s32, SAI_POLICER_COLOR_SOURCE_AWARE);
    sai_attribute_t s;
    s.id = SAI_POLICER_ATTR_CIR; s.value.u64 = 1000;
    OK(poa->set_policer_attribute(p, &s));
    CHECK_EQ(policer_of(fake_policer(p)).cir, 1000u);
    s.id = SAI_POLICER_ATTR_MODE; s.value.s32 = SAI_POLICER_MODE_TR_TCM;
    CHECK_EQ(poa->set_policer_attribute(p, &s), SAI_STATUS_INVALID_ATTR_VALUE_0);   // create-only
    {
        sai_attribute_t a[2];
        a[0].id = SAI_POLICER_ATTR_METER_TYPE; a[0].value.s32 = SAI_METER_TYPE_PACKETS;
        a[1].id = SAI_POLICER_ATTR_CIR; a[1].value.u64 = 5;
        sai_object_id_t x;
        CHECK_EQ(poa->create_policer(&x, g_sw, 2, a), SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING);
        a[1].id = SAI_POLICER_ATTR_MODE; a[1].value.s32 = SAI_POLICER_MODE_STORM_CONTROL;
        CHECK_EQ(poa->create_policer(&x, g_sw, 2, a), SAI_STATUS_ATTR_NOT_SUPPORTED_0 - 1);
        sai_attribute_t b[4] = {a[0], a[1], {}, {}};
        b[1].value.s32 = SAI_POLICER_MODE_TR_TCM;
        b[2].id = SAI_POLICER_ATTR_CIR; b[2].value.u64 = 100;
        b[3].id = SAI_POLICER_ATTR_PIR; b[3].value.u64 = 50;   // PIR < CIR: the fake (like RFC 2698) refuses
        CHECK(poa->create_policer(&x, g_sw, 4, b) != SAI_STATUS_SUCCESS);
    }
    uint64_t st;
    sai_stat_id_t sid = SAI_POLICER_STAT_PACKETS;
    CHECK_EQ(poa->get_policer_stats(p, 1, &sid, &st), SAI_STATUS_NOT_SUPPORTED);

    // CoPP: a trap group with the policer, traps in it -> FP entries
    sai_object_id_t grp = 0;
    sai_attribute_t q;
    q.id = SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE; q.value.u32 = 4;
    OK(ha->create_hostif_trap_group(&grp, g_sw, 1, &q));
    sai_attribute_t pa1;
    pa1.id = SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER; pa1.value.oid = p;
    OK(ha->set_hostif_trap_group_attribute(grp, &pa1));
    CHECK_EQ(tp::copp_group(), -1);   // no trap yet: no CoPP group
    sai_object_id_t arp = mk_trap(SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST, SAI_PACKET_ACTION_COPY, grp);
    auto v = copp_entries();
    CHECK(tp::copp_group() >= 0);
    CHECK_EQ(v.size(), 1u);
    if (v.size() == 1) {
        auto& r = v[0];
        CHECK(has(r, tp::Q_IP_TYPE) && r.ip_type == SAI_ACL_IP_TYPE_ARP_REQUEST);
        CHECK(r.policer == fake_policer(p) && r.red_drop && r.cpu_copy_only && !r.drop);
        CHECK(!has(r, tp::Q_IN_PORTS));   // every port
    }
    {
        tp::FpGroup fg = fp_group_of(tp::copp_group());
        CHECK(fg.qset & (1u << tp::Q_DST_CPU));
        CHECK(fg.priority > 999);
    }
    sai_object_id_t lacp = mk_trap(SAI_HOSTIF_TRAP_TYPE_LACP, SAI_PACKET_ACTION_TRAP, grp);
    sai_object_id_t bgp = mk_trap(SAI_HOSTIF_TRAP_TYPE_BGP, SAI_PACKET_ACTION_TRAP, grp);
    sai_object_id_t ip2me = mk_trap(SAI_HOSTIF_TRAP_TYPE_IP2ME, SAI_PACKET_ACTION_TRAP, grp);
    sai_object_id_t ttl = mk_trap(SAI_HOSTIF_TRAP_TYPE_TTL_ERROR, SAI_PACKET_ACTION_TRAP, grp);   // no match: unpoliced
    v = copp_entries();
    CHECK_EQ(v.size(), 1u + 1 + 2 + 1);
    int n_lacp = 0, n_bgp = 0, n_ip2me = 0;
    for (auto& e : v) {
        auto& r = e;
        if (has(r, tp::Q_ETHER_TYPE) && r.ether == 0x8809) { n_lacp++; CHECK(r.red_drop && !r.cpu_copy_only && r.priority == 200); }
        if (has(r, tp::Q_L4_DST_PORT) && r.l4_dst == 179) { n_bgp++; CHECK(has(r, tp::Q_DST_CPU) && r.priority == 150); }
        if (has(r, tp::Q_L4_SRC_PORT) && r.l4_src == 179) n_bgp++;
        if (r.present == (1u << tp::Q_DST_CPU)) { n_ip2me++; CHECK(r.priority == 100 && r.red_drop); }
    }
    CHECK_EQ(n_lacp, 1);
    CHECK_EQ(n_bgp, 2);
    CHECK_EQ(n_ip2me, 1);
    // the policer is shared (one meter for the group)
    for (auto& e : v) CHECK_EQ(e.policer, fake_policer(p));
    CHECK_EQ(poa->remove_policer(p), SAI_STATUS_OBJECT_IN_USE);
    // red action FORWARD: entries re-installed without the colour drop
    s.id = SAI_POLICER_ATTR_RED_PACKET_ACTION; s.value.s32 = SAI_PACKET_ACTION_FORWARD;
    OK(poa->set_policer_attribute(p, &s));
    v = copp_entries();
    CHECK_EQ(v.size(), 5u);
    for (auto& e : v) CHECK(!e.red_drop);
    s.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(poa->set_policer_attribute(p, &s));
    // a trap that stops going to the CPU loses its rule; another group without a policer: none
    sai_attribute_t act;
    act.id = SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION; act.value.s32 = SAI_PACKET_ACTION_DROP;
    OK(ha->set_hostif_trap_attribute(lacp, &act));
    CHECK_EQ(copp_entries().size(), 4u);
    sai_object_id_t grp2 = 0;
    OK(ha->create_hostif_trap_group(&grp2, g_sw, 1, &q));
    sai_attribute_t tg;
    tg.id = SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP; tg.value.oid = grp2;
    OK(ha->set_hostif_trap_attribute(bgp, &tg));
    CHECK_EQ(copp_entries().size(), 2u);
    // policer off the group: no CoPP entries, group gone
    pa1.value.oid = SAI_NULL_OBJECT_ID;
    OK(ha->set_hostif_trap_group_attribute(grp, &pa1));
    CHECK_EQ(tp::copp_rule_count(), 0u);
    CHECK_EQ(tp::copp_group(), -1);
    pa1.value.oid = tp::make_oid(SAI_OBJECT_TYPE_POLICER, 999);
    CHECK_EQ(ha->set_hostif_trap_group_attribute(grp, &pa1), SAI_STATUS_INVALID_ATTR_VALUE_0);
    // policer on the default trap group (CoppOrch's "default"): traps in it are policed
    sai_object_id_t def = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP);
    pa1.value.oid = p;
    OK(ha->set_hostif_trap_group_attribute(def, &pa1));
    sai_object_id_t lldp = mk_trap(SAI_HOSTIF_TRAP_TYPE_LLDP, SAI_PACKET_ACTION_TRAP, SAI_NULL_OBJECT_ID);
    CHECK_EQ(copp_entries().size(), 1u);
    for (auto t : {arp, lacp, bgp, ip2me, ttl, lldp}) OK(ha->remove_hostif_trap(t));
    CHECK_EQ(tp::copp_rule_count(), 0u);
    pa1.value.oid = SAI_NULL_OBJECT_ID;
    OK(ha->set_hostif_trap_group_attribute(def, &pa1));
    OK(ha->remove_hostif_trap_group(grp));
    OK(ha->remove_hostif_trap_group(grp2));
    OK(poa->remove_policer(p));
    CHECK(tp::count(tp::T_POLICERS) == 0 && tp::count(tp::T_FP_ENTRIES) == 0 && tp::count(tp::T_FP_GROUPS) == 0);
}

static sai_attribute_t field_bool(sai_attr_id_t id)
{
    sai_attribute_t a;
    std::memset(&a, 0, sizeof(a));
    a.id = id;
    a.value.booldata = true;
    return a;
}

// AclOrch's L3 table: ingress, PORT + LAG, its match fields.
static sai_object_id_t mk_l3_table(sai_status_t* status = nullptr, int32_t stage = SAI_ACL_STAGE_INGRESS,
                                   sai_attr_id_t extra = 0)
{
    static int32_t bp[2] = {SAI_ACL_BIND_POINT_TYPE_PORT, SAI_ACL_BIND_POINT_TYPE_LAG};
    std::vector<sai_attribute_t> a;
    sai_attribute_t x;
    x.id = SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST;
    x.value.s32list.count = 2;
    x.value.s32list.list = bp;
    a.push_back(x);
    for (sai_attr_id_t f : {SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, SAI_ACL_TABLE_ATTR_FIELD_ACL_IP_TYPE,
                            SAI_ACL_TABLE_ATTR_FIELD_SRC_IP, SAI_ACL_TABLE_ATTR_FIELD_DST_IP,
                            SAI_ACL_TABLE_ATTR_FIELD_ICMP_TYPE, SAI_ACL_TABLE_ATTR_FIELD_ICMP_CODE,
                            SAI_ACL_TABLE_ATTR_FIELD_IP_PROTOCOL, SAI_ACL_TABLE_ATTR_FIELD_L4_SRC_PORT,
                            SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT, SAI_ACL_TABLE_ATTR_FIELD_TCP_FLAGS})
        a.push_back(field_bool(f));
    if (extra) a.push_back(field_bool(extra));
    static int32_t rt[2] = {SAI_ACL_RANGE_TYPE_L4_SRC_PORT_RANGE, SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE};
    x.id = SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE;   // AclOrch's mandatory range field
    x.value.s32list.count = 2;
    x.value.s32list.list = rt;
    a.push_back(x);
    x.id = SAI_ACL_TABLE_ATTR_ACL_STAGE;
    x.value.s32 = stage;
    a.push_back(x);
    sai_object_id_t t = 0;
    sai_status_t st = aa->create_acl_table(&t, g_sw, (uint32_t)a.size(), a.data());
    if (status) *status = st;
    else OK(st);
    return t;
}

static sai_object_id_t mk_counter(sai_object_id_t table)
{
    sai_attribute_t a[3];
    a[0].id = SAI_ACL_COUNTER_ATTR_TABLE_ID; a[0].value.oid = table;
    a[1].id = SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT; a[1].value.booldata = true;
    a[2].id = SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT; a[2].value.booldata = true;
    sai_object_id_t k = 0;
    OK(aa->create_acl_counter(&k, g_sw, 3, a));
    return k;
}

// AclRule::createRule: TABLE_ID, PRIORITY, ADMIN_STATE, COUNTER, matches, actions.
static sai_status_t mk_rule(sai_object_id_t* e, sai_object_id_t table, uint32_t prio, sai_object_id_t counter,
                            uint8_t d0, uint8_t d1, uint8_t d2, uint8_t d3, int32_t action,
                            sai_attr_id_t extra_field = 0)
{
    std::vector<sai_attribute_t> a;
    sai_attribute_t x;
    std::memset(&x, 0, sizeof(x));
    x.id = SAI_ACL_ENTRY_ATTR_TABLE_ID; x.value.oid = table; a.push_back(x);
    x.id = SAI_ACL_ENTRY_ATTR_PRIORITY; x.value.u32 = prio; a.push_back(x);
    x.id = SAI_ACL_ENTRY_ATTR_ADMIN_STATE; x.value.booldata = true; a.push_back(x);
    if (counter) {
        std::memset(&x, 0, sizeof(x));
        x.id = SAI_ACL_ENTRY_ATTR_ACTION_COUNTER;
        x.value.aclaction.enable = true;
        x.value.aclaction.parameter.oid = counter;
        a.push_back(x);
    }
    std::memset(&x, 0, sizeof(x));
    x.id = SAI_ACL_ENTRY_ATTR_FIELD_DST_IP;
    x.value.aclfield.enable = true;
    uint8_t ip[4] = {d0, d1, d2, d3}, m[4] = {0xff, 0xff, 0xff, 0xff};
    std::memcpy(&x.value.aclfield.data.ip4, ip, 4);
    std::memcpy(&x.value.aclfield.mask.ip4, m, 4);
    a.push_back(x);
    if (extra_field) {
        std::memset(&x, 0, sizeof(x));
        x.id = extra_field;
        x.value.aclfield.enable = true;
        x.value.aclfield.data.u16 = 0x0806;
        x.value.aclfield.mask.u16 = 0xffff;
        a.push_back(x);
    }
    std::memset(&x, 0, sizeof(x));
    x.id = SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION;
    x.value.aclaction.enable = true;
    x.value.aclaction.parameter.s32 = action;
    a.push_back(x);
    return aa->create_acl_entry(e, g_sw, (uint32_t)a.size(), a.data());
}

// The backend entry an ACL entry is installed as (nullptr: not installed). The pointer is
// valid until the next call.
static const tp::FpEntry* fake_entry(sai_object_id_t e)
{
    static tp::FpEntry last;
    return tp::fp_entry(tp::backend_id(e), last) ? &last : nullptr;
}

static uint64_t counter_packets(sai_object_id_t k)
{
    sai_attribute_t a[2];
    a[0].id = SAI_ACL_COUNTER_ATTR_PACKETS;
    a[1].id = SAI_ACL_COUNTER_ATTR_BYTES;
    OK(aa->get_acl_counter_attribute(k, 2, a));
    return a[0].value.u64;
}

static void test_acl()
{
    // capabilities AclOrch reads
    sai_attribute_t c;
    c.id = SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT;
    OK(sw->get_switch_attribute(g_sw, 1, &c));
    CHECK_EQ(c.value.u32, 3u);
    int32_t acts[8];
    c.id = SAI_SWITCH_ATTR_ACL_STAGE_INGRESS;
    c.value.aclcapability.action_list.count = 8;
    c.value.aclcapability.action_list.list = acts;
    OK(sw->get_switch_attribute(g_sw, 1, &c));
    CHECK_EQ(c.value.aclcapability.action_list.count, 3u);
    CHECK(!c.value.aclcapability.is_action_list_mandatory);
    CHECK(std::find(acts, acts + 3, SAI_ACL_ACTION_TYPE_PACKET_ACTION) != acts + 3);
    c.id = SAI_SWITCH_ATTR_ACL_STAGE_EGRESS;
    c.value.aclcapability.action_list.count = 8;
    OK(sw->get_switch_attribute(g_sw, 1, &c));
    CHECK_EQ(c.value.aclcapability.action_list.count, 0u);
    sai_attr_capability_t cap;
    OK(sai_query_attribute_capability(g_sw, SAI_OBJECT_TYPE_ACL_TABLE, SAI_ACL_TABLE_ATTR_FIELD_DST_IP, &cap));
    CHECK(cap.create_implemented);
    OK(sai_query_attribute_capability(g_sw, SAI_OBJECT_TYPE_ACL_TABLE, SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_PRI, &cap));
    CHECK(!cap.create_implemented);   // AclOrch then leaves it out of its table types
    OK(sai_query_attribute_capability(g_sw, SAI_OBJECT_TYPE_ACL_ENTRY, SAI_ACL_ENTRY_ATTR_FIELD_ACL_USER_META, &cap));
    CHECK(!cap.set_implemented);
    int32_t ev[8];
    sai_s32_list_t el = {8, ev};
    OK(sai_query_attribute_enum_values_capability(g_sw, SAI_OBJECT_TYPE_ACL_TABLE_GROUP, SAI_ACL_TABLE_GROUP_ATTR_TYPE, &el));
    CHECK(el.count == 1 && ev[0] == SAI_ACL_TABLE_GROUP_TYPE_PARALLEL);
    el.count = 8;
    OK(sai_query_attribute_enum_values_capability(g_sw, SAI_OBJECT_TYPE_ACL_ENTRY, SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, &el));
    CHECK_EQ(el.count, 4u);

    // unsupported: egress, a match field we do not program, ranges
    sai_status_t st;
    mk_l3_table(&st, SAI_ACL_STAGE_EGRESS);
    CHECK(st != SAI_STATUS_SUCCESS);
    mk_l3_table(&st, SAI_ACL_STAGE_INGRESS, SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_PRI);
    CHECK(st != SAI_STATUS_SUCCESS);
    sai_object_id_t rng;
    CHECK_EQ(aa->create_acl_range(&rng, g_sw, 0, nullptr), SAI_STATUS_NOT_SUPPORTED);

    // table -> SDK field group with the table's qualifiers + in-ports
    sai_object_id_t t = mk_l3_table();
    {
        tp::FpGroup fg = fp_group_of(tp::backend_id(t));
        CHECK(fg.qset & (1u << tp::Q_IN_PORTS));
        CHECK(fg.qset & (1u << tp::Q_DST_IP));
        CHECK(fg.qset & (1u << tp::Q_ICMP_TYPE_CODE));
        CHECK(!(fg.qset & (1u << tp::Q_DST_IP6)));
    }
    sai_attribute_t tg;
    tg.id = SAI_ACL_TABLE_ATTR_FIELD_DST_IP;
    OK(aa->get_acl_table_attribute(t, 1, &tg));
    CHECK(tg.value.booldata);
    tg.id = SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6;
    OK(aa->get_acl_table_attribute(t, 1, &tg));
    CHECK(!tg.value.booldata);
    tg.id = SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY;   // CRM: unsupported, cleanly
    CHECK_EQ(aa->get_acl_table_attribute(t, 1, &tg), SAI_STATUS_ATTR_NOT_SUPPORTED_0);

    // rule: drop to 10.0.0.1, with a counter; table not bound -> not installed
    sai_object_id_t k = mk_counter(t), e = 0;
    OK(mk_rule(&e, t, 9000, k, 10, 0, 0, 1, SAI_PACKET_ACTION_DROP));
    CHECK_EQ(tp::backend_id(e), -1);
    sai_object_id_t bad;
    CHECK(mk_rule(&bad, t, 10, 0, 10, 0, 0, 2, SAI_PACKET_ACTION_DROP, SAI_ACL_ENTRY_ATTR_FIELD_OUTER_VLAN_ID) != SAI_STATUS_SUCCESS);
    CHECK(mk_rule(&bad, t, 11, k, 10, 0, 0, 3, SAI_PACKET_ACTION_DROP) == SAI_STATUS_OBJECT_IN_USE);   // counter taken

    // PortsOrch: table group, bind it to the port, member = the table
    sai_object_id_t port_a = g_ports[20], port_b = g_ports[21];
    int hw_a = tp::backend_id(port_a), hw_b = tp::backend_id(port_b);
    sai_object_id_t grp = 0, mem = 0;
    sai_object_id_t bpl[1] = {SAI_ACL_BIND_POINT_TYPE_PORT};   // portsorch passes an object list
    sai_attribute_t g3[3];
    g3[0].id = SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE; g3[0].value.s32 = SAI_ACL_STAGE_INGRESS;
    g3[1].id = SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST; g3[1].value.objlist.count = 1; g3[1].value.objlist.list = bpl;
    g3[2].id = SAI_ACL_TABLE_GROUP_ATTR_TYPE; g3[2].value.s32 = SAI_ACL_TABLE_GROUP_TYPE_PARALLEL;
    OK(aa->create_acl_table_group(&grp, g_sw, 3, g3));
    sai_attribute_t bind;
    bind.id = SAI_PORT_ATTR_INGRESS_ACL; bind.value.oid = grp;
    OK(pa->set_port_attribute(port_a, &bind));
    CHECK_EQ(tp::backend_id(e), -1);   // the table is not a member yet
    sai_attribute_t m3[3];
    m3[0].id = SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID; m3[0].value.oid = grp;
    m3[1].id = SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID; m3[1].value.oid = t;
    m3[2].id = SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY; m3[2].value.u32 = 100;
    OK(aa->create_acl_table_group_member(&mem, g_sw, 3, m3));
    {
        auto* fe = fake_entry(e);
        CHECK(fe != nullptr);
        if (fe) {
            CHECK(fe->drop && fe->counter && fe->priority == 9000);
            CHECK(fe->in_ports == std::vector<int>{hw_a});
            CHECK(has(*fe, tp::Q_DST_IP) && fe->dst.addr[0] == 10 && fe->dst.addr[3] == 1);
            CHECK(fe->dst_mask.addr[3] == 0xff);
        }
    }
    sai_attribute_t gb;
    gb.id = SAI_PORT_ATTR_INGRESS_ACL;
    OK(pa->get_port_attribute(port_a, 1, &gb));
    CHECK_EQ(gb.value.oid, grp);
    bind.id = SAI_PORT_ATTR_EGRESS_ACL;
    CHECK_EQ(pa->set_port_attribute(port_b, &bind), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    bind.id = SAI_PORT_ATTR_INGRESS_ACL;

    // counter: hits on the SDK entry; kept when the entry is re-created
    tp::fp_hit(tp::backend_id(e), 5, 500);
    CHECK_EQ(counter_packets(k), 5u);
    OK(pa->set_port_attribute(port_b, &bind));   // second port in the same group
    {
        auto* fe = fake_entry(e);
        CHECK(fe && fe->in_ports == (std::vector<int>{std::min(hw_a, hw_b), std::max(hw_a, hw_b)}));
    }
    CHECK_EQ(counter_packets(k), 5u);
    tp::fp_hit(tp::backend_id(e), 2, 200);
    CHECK_EQ(counter_packets(k), 7u);
    sai_attribute_t kz;
    kz.id = SAI_ACL_COUNTER_ATTR_PACKETS; kz.value.u64 = 0;
    OK(aa->set_acl_counter_attribute(k, &kz));
    CHECK_EQ(counter_packets(k), 0u);

    // entry attributes: action, admin state, priority
    sai_attribute_t ea;
    std::memset(&ea, 0, sizeof(ea));
    ea.id = SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION;
    ea.value.aclaction.enable = true;
    ea.value.aclaction.parameter.s32 = SAI_PACKET_ACTION_FORWARD;
    OK(aa->set_acl_entry_attribute(e, &ea));
    {
        auto* fe = fake_entry(e);
        CHECK(fe && !fe->drop && fe->drop_cancel);
    }
    ea.id = SAI_ACL_ENTRY_ATTR_ADMIN_STATE;
    ea.value.booldata = false;
    OK(aa->set_acl_entry_attribute(e, &ea));
    CHECK_EQ(tp::backend_id(e), -1);
    ea.value.booldata = true;
    OK(aa->set_acl_entry_attribute(e, &ea));
    CHECK(tp::backend_id(e) >= 0);
    std::memset(&ea, 0, sizeof(ea));
    ea.id = SAI_ACL_ENTRY_ATTR_ACTION_COUNTER;   // the same counter again: no conflict with itself
    ea.value.aclaction.enable = true;
    ea.value.aclaction.parameter.oid = k;
    OK(aa->set_acl_entry_attribute(e, &ea));
    sai_attribute_t eg[2];
    eg[0].id = SAI_ACL_ENTRY_ATTR_PRIORITY;
    eg[1].id = SAI_ACL_ENTRY_ATTR_FIELD_DST_IP;
    OK(aa->get_acl_entry_attribute(e, 2, eg));
    CHECK_EQ(eg[0].value.u32, 9000u);
    CHECK(eg[1].value.aclfield.enable);

    // LAG binding: the LAG's member ports, following membership
    free_port(g_ports[22]);
    free_port(g_ports[23]);
    sai_object_id_t lag = 0, lm1 = 0, lm2 = 0;
    OK(la->create_lag(&lag, g_sw, 0, nullptr));
    sai_attribute_t ma[2];
    ma[0].id = SAI_LAG_MEMBER_ATTR_LAG_ID; ma[0].value.oid = lag;
    ma[1].id = SAI_LAG_MEMBER_ATTR_PORT_ID; ma[1].value.oid = g_ports[22];
    OK(la->create_lag_member(&lm1, g_sw, 2, ma));
    sai_attribute_t lb;
    lb.id = SAI_LAG_ATTR_INGRESS_ACL; lb.value.oid = grp;
    OK(la->set_lag_attribute(lag, &lb));
    {
        auto* fe = fake_entry(e);
        CHECK(fe && fe->in_ports.size() == 3);
    }
    ma[1].value.oid = g_ports[23];
    OK(la->create_lag_member(&lm2, g_sw, 2, ma));
    {
        auto* fe = fake_entry(e);
        CHECK(fe && fe->in_ports.size() == 4);
    }
    OK(la->remove_lag_member(lm2));
    lb.value.oid = SAI_NULL_OBJECT_ID;
    OK(la->set_lag_attribute(lag, &lb));
    OK(la->remove_lag_member(lm1));
    OK(la->remove_lag(lag));

    // every advertised get on the ACL / POLICER objects works on a live one
    sai_object_id_t pol = mk_policer(100, 100);
    for (auto& cp : tp::attr_caps()) {
        if (!cp.get) continue;
        sai_object_id_t oid = cp.ot == SAI_OBJECT_TYPE_ACL_TABLE ? t : cp.ot == SAI_OBJECT_TYPE_ACL_ENTRY ? e
                            : cp.ot == SAI_OBJECT_TYPE_ACL_COUNTER ? k : cp.ot == SAI_OBJECT_TYPE_ACL_TABLE_GROUP ? grp
                            : cp.ot == SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER ? mem
                            : cp.ot == SAI_OBJECT_TYPE_POLICER ? pol : 0;
        if (!oid) continue;
        sai_attribute_t a;
        std::memset(&a, 0, sizeof(a));
        a.id = cp.id;
        uint64_t buf[64];
        a.value.objlist.count = 64;
        a.value.objlist.list = (sai_object_id_t*)buf;
        if (cp.ot == SAI_OBJECT_TYPE_ACL_ENTRY && cp.id == SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS) {
            a.value.aclfield.data.objlist.count = 64;
            a.value.aclfield.data.objlist.list = (sai_object_id_t*)buf;
        }
        sai_status_t gs = cp.ot == SAI_OBJECT_TYPE_ACL_TABLE ? aa->get_acl_table_attribute(oid, 1, &a)
                        : cp.ot == SAI_OBJECT_TYPE_ACL_ENTRY ? aa->get_acl_entry_attribute(oid, 1, &a)
                        : cp.ot == SAI_OBJECT_TYPE_ACL_COUNTER ? aa->get_acl_counter_attribute(oid, 1, &a)
                        : cp.ot == SAI_OBJECT_TYPE_ACL_TABLE_GROUP ? aa->get_acl_table_group_attribute(oid, 1, &a)
                        : cp.ot == SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER ? aa->get_acl_table_group_member_attribute(oid, 1, &a)
                                                                       : poa->get_policer_attribute(oid, 1, &a);
        if (gs != SAI_STATUS_SUCCESS) std::fprintf(stderr, "  capability get ot=%d attr=%d -> %d\n", cp.ot, cp.id, gs);
        CHECK_EQ(gs, SAI_STATUS_SUCCESS);
    }
    OK(poa->remove_policer(pol));

    // in use: table with entries / members, group bound, counter on an entry
    CHECK_EQ(aa->remove_acl_table(t), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(aa->remove_acl_table_group(grp), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(aa->remove_acl_counter(k), SAI_STATUS_OBJECT_IN_USE);

    // AclOrch / PortsOrch teardown order
    OK(aa->remove_acl_entry(e));
    OK(aa->remove_acl_counter(k));
    OK(aa->remove_acl_table_group_member(mem));
    bind.value.oid = SAI_NULL_OBJECT_ID;
    OK(pa->set_port_attribute(port_a, &bind));
    OK(pa->set_port_attribute(port_b, &bind));
    OK(aa->remove_acl_table_group(grp));
    OK(aa->remove_acl_table(t));
    {
        CHECK(tp::count(tp::T_FP_ENTRIES) == 0 && tp::count(tp::T_FP_GROUPS) == 0);
    }
    CHECK(tp::object_count(SAI_OBJECT_TYPE_ACL_TABLE) == 0 && tp::object_count(SAI_OBJECT_TYPE_ACL_ENTRY) == 0 &&
          tp::acl_binding_count() == 0);

    // L3V6: an IPv6 match keeps all 16 bytes
    {
        static int32_t bp6[1] = {SAI_ACL_BIND_POINT_TYPE_PORT};
        sai_attribute_t t6[3];
        t6[0].id = SAI_ACL_TABLE_ATTR_ACL_STAGE; t6[0].value.s32 = SAI_ACL_STAGE_INGRESS;
        t6[1].id = SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST; t6[1].value.s32list.count = 1; t6[1].value.s32list.list = bp6;
        t6[2] = field_bool(SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6);
        sai_object_id_t tv6 = 0, ev6 = 0;
        OK(aa->create_acl_table(&tv6, g_sw, 3, t6));
        sai_attribute_t x[3];
        std::memset(x, 0, sizeof(x));
        x[0].id = SAI_ACL_ENTRY_ATTR_TABLE_ID; x[0].value.oid = tv6;
        x[1].id = SAI_ACL_ENTRY_ATTR_FIELD_DST_IPV6;
        x[1].value.aclfield.enable = true;
        for (int b = 0; b < 16; b++) { x[1].value.aclfield.data.ip6[b] = (uint8_t)(0x20 + b); x[1].value.aclfield.mask.ip6[b] = 0xff; }
        x[2].id = SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION;
        x[2].value.aclaction.enable = true;
        x[2].value.aclaction.parameter.s32 = SAI_PACKET_ACTION_DROP;
        OK(aa->create_acl_entry(&ev6, g_sw, 3, x));
        sai_attribute_t g6;
        g6.id = SAI_ACL_ENTRY_ATTR_FIELD_DST_IPV6;
        OK(aa->get_acl_entry_attribute(ev6, 1, &g6));
        CHECK(std::memcmp(g6.value.aclfield.data.ip6, x[1].value.aclfield.data.ip6, 16) == 0);
        bind.value.oid = tv6;
        OK(pa->set_port_attribute(port_b, &bind));
        {
                auto* fe = fake_entry(ev6);
            CHECK(fe && has(*fe, tp::Q_DST_IP6) && fe->dst.v6 && fe->dst.addr[15] == 0x2f);
        }
        bind.value.oid = SAI_NULL_OBJECT_ID;
        OK(pa->set_port_attribute(port_b, &bind));
        OK(aa->remove_acl_entry(ev6));
        OK(aa->remove_acl_table(tv6));
    }

    // a table bound directly (no group) works too
    t = mk_l3_table();
    OK(mk_rule(&e, t, 5, 0, 192, 0, 2, 1, SAI_PACKET_ACTION_TRAP));
    bind.value.oid = t;
    OK(pa->set_port_attribute(port_a, &bind));
    {
        auto* fe = fake_entry(e);
        CHECK(fe && fe->drop && fe->copy_cpu && !fe->counter);
    }
    bind.value.oid = SAI_NULL_OBJECT_ID;
    OK(pa->set_port_attribute(port_a, &bind));
    OK(aa->remove_acl_entry(e));
    OK(aa->remove_acl_table(t));
}
