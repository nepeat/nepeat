// SPDX-License-Identifier: Apache-2.0
// Included by test_sai.cpp (after test_acl.inc): QoS maps, schedulers and
// scheduler groups, WRED, buffers, queue / PG counters, PFC, mirror
// sessions, SAMPLEPACKET, genetlink hostifs, ACL mirror actions; first with
// a backend without them (the fake's default: objects kept, nothing
// programmed, as on the AS7760), then with the fake's QoS / mirror model.

#include <arpa/inet.h>

static sai_qos_map_api_t* qma;
static sai_scheduler_api_t* sca;
static sai_scheduler_group_api_t* sga;
static sai_wred_api_t* wra;
static sai_buffer_api_t* bfa;
static sai_mirror_api_t* mia;
static sai_samplepacket_api_t* spa;

static sai_object_id_t mk_qos_map(int32_t type, const std::vector<sai_qos_map_t>& l, sai_status_t* st = nullptr)
{
    sai_attribute_t a[2];
    a[0].id = SAI_QOS_MAP_ATTR_TYPE; a[0].value.s32 = type;
    a[1].id = SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST;
    a[1].value.qosmap.count = (uint32_t)l.size();
    a[1].value.qosmap.list = const_cast<sai_qos_map_t*>(l.data());
    sai_object_id_t m = 0;
    sai_status_t r = qma->create_qos_map(&m, g_sw, 2, a);
    if (st) *st = r;
    else OK(r);
    return m;
}
static std::vector<sai_qos_map_t> dscp_map(uint8_t dscp46_tc)
{
    std::vector<sai_qos_map_t> v;
    for (int d = 0; d < 64; d++) {
        sai_qos_map_t m;
        std::memset(&m, 0, sizeof(m));
        m.key.dscp = (uint8_t)d;
        m.value.tc = d == 46 ? dscp46_tc : (sai_cos_t)(d >> 3);
        v.push_back(m);
    }
    return v;
}
static sai_status_t port_set(sai_object_id_t p, sai_attr_id_t id, sai_object_id_t oid)
{
    sai_attribute_t a;
    a.id = id;
    a.value.oid = oid;
    return pa->set_port_attribute(p, &a);
}
static std::vector<sai_object_id_t> port_list_attr(sai_object_id_t p, sai_attr_id_t id)
{
    sai_object_id_t l[64];
    sai_attribute_t a;
    a.id = id;
    a.value.objlist.count = 64;
    a.value.objlist.list = l;
    OK(pa->get_port_attribute(p, 1, &a));
    return std::vector<sai_object_id_t>(l, l + a.value.objlist.count);
}
static sai_object_id_t mk_sched(int32_t type, uint8_t w, uint64_t max_rate = 0)
{
    sai_attribute_t a[4];
    uint32_t n = 0;
    a[n].id = SAI_SCHEDULER_ATTR_SCHEDULING_TYPE; a[n++].value.s32 = type;
    a[n].id = SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT; a[n++].value.u8 = w;
    if (max_rate) {
        a[n].id = SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE; a[n++].value.u64 = max_rate;
        a[n].id = SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_BURST_RATE; a[n++].value.u64 = 65536;
    }
    sai_object_id_t s = 0;
    OK(sca->create_scheduler(&s, g_sw, n, a));
    return s;
}
static sai_object_id_t mk_pool(int32_t type, uint64_t size)
{
    sai_attribute_t a[3];
    a[0].id = SAI_BUFFER_POOL_ATTR_TYPE; a[0].value.s32 = type;
    a[1].id = SAI_BUFFER_POOL_ATTR_SIZE; a[1].value.u64 = size;
    a[2].id = SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE; a[2].value.s32 = SAI_BUFFER_POOL_THRESHOLD_MODE_DYNAMIC;
    sai_object_id_t p = 0;
    OK(bfa->create_buffer_pool(&p, g_sw, 3, a));
    return p;
}
static sai_object_id_t mk_profile(sai_object_id_t pool, uint64_t reserved, int8_t th, uint64_t xoff = 0)
{
    sai_attribute_t a[4];
    uint32_t n = 0;
    a[n].id = SAI_BUFFER_PROFILE_ATTR_POOL_ID; a[n++].value.oid = pool;
    a[n].id = SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE; a[n++].value.u64 = reserved;
    a[n].id = SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH; a[n++].value.s8 = th;
    if (xoff) { a[n].id = SAI_BUFFER_PROFILE_ATTR_XOFF_TH; a[n++].value.u64 = xoff; }
    sai_object_id_t p = 0;
    OK(bfa->create_buffer_profile(&p, g_sw, n, a));
    return p;
}
static sai_object_id_t mk_span(sai_object_id_t monitor)
{
    sai_attribute_t a[2];
    a[0].id = SAI_MIRROR_SESSION_ATTR_TYPE; a[0].value.s32 = SAI_MIRROR_SESSION_TYPE_LOCAL;
    a[1].id = SAI_MIRROR_SESSION_ATTR_MONITOR_PORT; a[1].value.oid = monitor;
    sai_object_id_t m = 0;
    OK(mia->create_mirror_session(&m, g_sw, 2, a));
    return m;
}
static int fake_id_of_map(sai_object_id_t m) { return tp::backend_id(m); }
// The backend's QoS / mirror state by port (and queue / PG): -1 / nullptr when absent. The
// pointers are valid until the next call of the same helper.
static int pq(int hw, int32_t sai_map_type) { return tp::port_qos_map(hw, sai_map_type); }
static const tp::Sched* qsch(int hw, int q)
{
    static tp::Sched s;
    return tp::queue_scheduler(hw, q, s) ? &s : nullptr;
}
static const tp::Sched* psch(int hw)
{
    static tp::Sched s;
    return tp::port_scheduler(hw, s) ? &s : nullptr;
}
static const tp::Buffer* qbuf(int hw, int q)
{
    static tp::Buffer b;
    return tp::queue_buffer(hw, q, b) ? &b : nullptr;
}
static const tp::Buffer* pgbuf(int hw, int q)
{
    static tp::Buffer b;
    return tp::pg_buffer(hw, q, b) ? &b : nullptr;
}
static int qw(int hw, int q) { return tp::queue_wred(hw, q); }
static std::vector<int> pm(int hw, bool in) { return tp::port_mirrors(hw, in); }
static int psam(int hw, bool in) { return tp::port_sample(hw, in); }
static tp::QosMap qmap(int id)
{
    tp::QosMap m{};
    tp::qos_map(id, m);
    return m;
}
static std::tuple<bool, uint8_t, uint8_t> pfc_of(int hw)
{
    tp::Pfc p{};
    tp::port_pfc(hw, p);
    return std::make_tuple(p.separate, p.tx, p.rx);
}
static bool wred_has(int id)
{
    tp::Wred w;
    return tp::wred(id, w);
}
static tp::Wred wred_of(int id)
{
    tp::Wred w{};
    tp::wred(id, w);
    return w;
}
static bool mir_has(int id)
{
    tp::Mirror m;
    return tp::mirror(id, m);
}
static tp::Mirror mir_of(int id)
{
    tp::Mirror m{};
    tp::mirror(id, m);
    return m;
}
static bool genl_has(int id)
{
    tp::Genl g;
    return tp::genetlink(id, g);
}
static tp::Genl genl_of(int id)
{
    tp::Genl g{};
    tp::genetlink(id, g);
    return g;
}
static bool hash_has(bool lag)
{
    tp::Hash h;
    return tp::hash(lag, h);
}
static tp::Hash hash_of(bool lag)
{
    tp::Hash h{};
    tp::hash(lag, h);
    return h;
}
// The CPU port, read without counting a check.
static sai_object_id_t cpu_port_oid() { return switch_oid_attr(SAI_SWITCH_ATTR_CPU_PORT); }
static std::vector<int> ids(int a, int b = -1) { std::vector<int> v(1, a); if (b >= 0) v.push_back(b); return v; }

static void test_qos()
{
    OK(sai_api_query(SAI_API_QOS_MAP, (void**)&qma));
    OK(sai_api_query(SAI_API_SCHEDULER, (void**)&sca));
    OK(sai_api_query(SAI_API_SCHEDULER_GROUP, (void**)&sga));
    OK(sai_api_query(SAI_API_WRED, (void**)&wra));
    OK(sai_api_query(SAI_API_BUFFER, (void**)&bfa));
    OK(sai_api_query(SAI_API_MIRROR, (void**)&mia));
    OK(sai_api_query(SAI_API_SAMPLEPACKET, (void**)&spa));
    sai_object_id_t p1 = g_ports[1], p2 = g_ports[2];
    int hw1 = tp::backend_id(p1), hw2 = tp::backend_id(p2);
    sai_attribute_t a;

    // ---- scheduler groups: none shown without a QoS backend (as before); one per queue with one
    std::vector<sai_object_id_t> qs = port_list_attr(p1, SAI_PORT_ATTR_QOS_QUEUE_LIST);
    CHECK(port_list_attr(p1, SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST).empty());
    a.id = SAI_PORT_ATTR_QOS_NUMBER_OF_SCHEDULER_GROUPS;
    OK(pa->get_port_attribute(p1, 1, &a));
    CHECK_EQ(a.value.u32, 0u);
    tp::set_feature(tp::FEATURE_QOS, true);
    std::vector<sai_object_id_t> sgs = port_list_attr(p1, SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);
    OK(pa->get_port_attribute(p1, 1, &a));
    CHECK_EQ(a.value.u32, 8u);
    CHECK_EQ(sgs.size(), 8u);
    CHECK_EQ(qs.size(), 8u);
    for (size_t i = 0; i < sgs.size() && i < qs.size(); i++) {
        sai_object_id_t child[4];
        sai_attribute_t g[4];
        g[0].id = SAI_SCHEDULER_GROUP_ATTR_CHILD_COUNT;
        g[1].id = SAI_SCHEDULER_GROUP_ATTR_CHILD_LIST;
        g[1].value.objlist.count = 4;
        g[1].value.objlist.list = child;
        g[2].id = SAI_SCHEDULER_GROUP_ATTR_PORT_ID;
        g[3].id = SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE;
        OK(sga->get_scheduler_group_attribute(sgs[i], 4, g));
        CHECK_EQ(g[0].value.u32, 1u);
        CHECK(g[1].value.objlist.count == 1 && child[0] == qs[i]);
        CHECK(g[2].value.oid == p1 && g[3].value.oid == p1);
        sai_attribute_t qa1;
        qa1.id = SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE;
        OK(qa->get_queue_attribute(qs[i], 1, &qa1));
        CHECK(qa1.value.oid == sgs[i]);
    }
    sai_object_id_t sgx;
    CHECK_EQ(sga->create_scheduler_group(&sgx, g_sw, 0, nullptr), SAI_STATUS_NOT_SUPPORTED);
    tp::set_feature(tp::FEATURE_QOS, false);

    // ---- a backend without QoS / mirror: objects kept, nothing programmed
    sai_object_id_t m0 = mk_qos_map(SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(5));
    CHECK_EQ(fake_id_of_map(m0), -1);
    OK(port_set(p1, SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, m0));
    CHECK(tp::count(tp::T_PORT_QOS_MAPS) == 0 && tp::count(tp::T_QOS_MAPS) == 0);
    sai_object_id_t s0 = mk_sched(SAI_SCHEDULING_TYPE_DWRR, 20);
    a.id = SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = s0;
    OK(sga->set_scheduler_group_attribute(sgs[3], &a));
    CHECK(tp::count(tp::T_QUEUE_SCHEDULERS) == 0);
    uint64_t v[3];
    sai_stat_id_t qid[3] = {SAI_QUEUE_STAT_PACKETS, SAI_QUEUE_STAT_BYTES, SAI_QUEUE_STAT_DROPPED_PACKETS};
    CHECK_EQ(qa->get_queue_stats(qs[3], 3, qid, v), SAI_STATUS_NOT_SUPPORTED);
    sai_object_id_t ms0 = mk_span(p2);
    CHECK(tp::count(tp::T_MIRRORS) == 0 && tp::backend_id(ms0) == -1);
    CHECK_EQ(port_set(p1, SAI_PORT_ATTR_INGRESS_MIRROR_SESSION, 0), SAI_STATUS_ATTR_NOT_SUPPORTED_0);   // as before
    {
        sai_attribute_t h[3];
        h[0].id = SAI_HOSTIF_ATTR_TYPE; h[0].value.s32 = SAI_HOSTIF_TYPE_GENETLINK;
        h[1].id = SAI_HOSTIF_ATTR_NAME; std::snprintf(h[1].value.chardata, sizeof(h[1].value.chardata), "psample");
        h[2].id = SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME;
        std::snprintf(h[2].value.chardata, sizeof(h[2].value.chardata), "packets");
        sai_object_id_t gh;
        CHECK_EQ(ha->create_hostif(&gh, g_sw, 3, h), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    }
    a.id = SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = 0;
    OK(sga->set_scheduler_group_attribute(sgs[3], &a));
    OK(port_set(p1, SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, 0));
    OK(qma->remove_qos_map(m0));
    OK(sca->remove_scheduler(s0));
    OK(mia->remove_mirror_session(ms0));

    // ================= the fake's QoS / mirror model
    tp::set_feature(tp::FEATURE_QOS, true);
    tp::set_feature(tp::FEATURE_MIRROR, true);

    // ---- QoS maps: complete tables, port binding, switch default, update, in use
    sai_object_id_t m1 = mk_qos_map(SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(5));
    int id1 = fake_id_of_map(m1);
    CHECK(id1 > 0);
    {
        tp::QosMap q = qmap(id1);
        CHECK(q.type == SAI_QOS_MAP_TYPE_DSCP_TO_TC && q.entries.size() == 64);
        CHECK(q.entries[46].key == 46 && q.entries[46].value == 5 && q.entries[8].value == 1);
    }
    OK(port_set(p1, SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, m1));
    CHECK_EQ(pq(hw1, SAI_QOS_MAP_TYPE_DSCP_TO_TC), id1);
    std::vector<sai_qos_map_t> partial(1);
    std::memset(partial.data(), 0, sizeof(sai_qos_map_t));
    partial[0].key.dot1p = 5;
    partial[0].value.tc = 3;
    sai_object_id_t md = mk_qos_map(SAI_QOS_MAP_TYPE_DOT1P_TO_TC, partial);
    CHECK(qmap(fake_id_of_map(md)).entries.size() == 8);
    CHECK(qmap(fake_id_of_map(md)).entries[5].value == 3 && qmap(fake_id_of_map(md)).entries[4].value == 0);
    CHECK_EQ(port_set(p1, SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, md), SAI_STATUS_INVALID_ATTR_VALUE_0);   // wrong type
    // switch-level DSCP_TO_TC: ports without their own map get it
    sai_object_id_t m2 = mk_qos_map(SAI_QOS_MAP_TYPE_DSCP_TO_TC, dscp_map(6));
    a.id = SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP;
    a.value.oid = m2;
    OK(sw->set_switch_attribute(g_sw, &a));
    CHECK_EQ(pq(hw2, SAI_QOS_MAP_TYPE_DSCP_TO_TC), fake_id_of_map(m2));
    CHECK_EQ(pq(hw1, SAI_QOS_MAP_TYPE_DSCP_TO_TC), id1);      // its own wins
    a.value.oid = 0;
    OK(sw->set_switch_attribute(g_sw, &a));
    CHECK(pq(hw2, SAI_QOS_MAP_TYPE_DSCP_TO_TC) < 0);
    OK(qma->remove_qos_map(m2));
    // update in place; in use
    a.id = SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST;
    std::vector<sai_qos_map_t> upd = dscp_map(7);
    a.value.qosmap.count = (uint32_t)upd.size();
    a.value.qosmap.list = upd.data();
    OK(qma->set_qos_map_attribute(m1, &a));
    CHECK_EQ(qmap(id1).entries[46].value, 7u);
    CHECK_EQ(qma->remove_qos_map(m1), SAI_STATUS_OBJECT_IN_USE);
    std::vector<sai_qos_map_t> bad = dscp_map(5);
    bad[3].key.dscp = 64;
    sai_status_t st;
    mk_qos_map(SAI_QOS_MAP_TYPE_DSCP_TO_TC, bad, &st);
    CHECK_EQ(st, SAI_STATUS_INVALID_ATTR_VALUE_0);
    std::vector<sai_qos_map_t> tcc(1);
    std::memset(tcc.data(), 0, sizeof(sai_qos_map_t));
    tcc[0].key.tc = 2;
    tcc[0].key.color = SAI_PACKET_COLOR_YELLOW;
    tcc[0].value.dscp = 12;
    sai_object_id_t mr = mk_qos_map(SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP, tcc);
    {
        tp::QosMap q = qmap(fake_id_of_map(mr));
        CHECK(q.entries.size() == 24 && q.entries[7].value == 12 && q.entries[7].key_color == 1);
    }
    a.id = SAI_QOS_MAP_ATTR_TYPE;
    a.value.s32 = 0;
    sai_attribute_t g2[2];
    g2[0].id = SAI_QOS_MAP_ATTR_TYPE;
    sai_qos_map_t back[64];
    g2[1].id = SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST;
    g2[1].value.qosmap.count = 64;
    g2[1].value.qosmap.list = back;
    OK(qma->get_qos_map_attribute(m1, 2, g2));
    CHECK(g2[0].value.s32 == SAI_QOS_MAP_TYPE_DSCP_TO_TC && g2[1].value.qosmap.count == 64 && back[46].value.tc == 7);
    OK(qma->remove_qos_map(mr));
    OK(port_set(p1, SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP, md));
    CHECK_EQ(pq(hw1, SAI_QOS_MAP_TYPE_DOT1P_TO_TC), fake_id_of_map(md));

    // ---- default TC, PFC (combined, separate)
    a.id = SAI_PORT_ATTR_QOS_DEFAULT_TC;
    a.value.u8 = 3;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(tp::port_default_tc(hw1), 3);
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL;
    a.value.u8 = 0x18;
    OK(pa->set_port_attribute(p1, &a));
    CHECK((pfc_of(hw1) == std::make_tuple(false, (uint8_t)0x18, (uint8_t)0x18)));
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE;
    a.value.s32 = SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_RX;
    a.value.u8 = 0xff;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX;
    a.value.u8 = 0x08;
    OK(pa->set_port_attribute(p1, &a));
    CHECK((pfc_of(hw1) == std::make_tuple(true, (uint8_t)0x08, (uint8_t)0xff)));
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL;
    OK(pa->get_port_attribute(p1, 1, &a));
    CHECK_EQ(a.value.u8, 0x18);
    // PFC counters: a port stat through the QoS call
    tp::set_port_stat(hw1, SAI_PORT_STAT_PFC_3_RX_PKTS, 17);
    sai_stat_id_t pfcid = SAI_PORT_STAT_PFC_3_RX_PKTS;
    uint64_t pv = 0;
    OK(pa->get_port_stats(p1, 1, &pfcid, &pv));
    CHECK_EQ(pv, 17u);

    // ---- schedulers: on a queue's group, weight change, strict, own queue profile, port shaper
    sai_object_id_t sd = mk_sched(SAI_SCHEDULING_TYPE_DWRR, 20);
    sai_object_id_t ssp = mk_sched(SAI_SCHEDULING_TYPE_STRICT, 1);
    sai_object_id_t sh = mk_sched(SAI_SCHEDULING_TYPE_DWRR, 1, 125000000);
    a.id = SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = sd;
    OK(sga->set_scheduler_group_attribute(sgs[3], &a));
    CHECK(qsch(hw1, 3) && qsch(hw1, 3)->weight == 20 && !qsch(hw1, 3)->strict);
    a.value.oid = 0x123;   // not a scheduler
    CHECK_EQ(sga->set_scheduler_group_attribute(sgs[4], &a), SAI_STATUS_INVALID_ATTR_VALUE_0);
    a.value.oid = sd;
    OK(sga->set_scheduler_group_attribute(sgs[4], &a));
    a.id = SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT;
    a.value.u8 = 50;
    OK(sca->set_scheduler_attribute(sd, &a));
    CHECK(qsch(hw1, 3) && qsch(hw1, 3)->weight == 50 && qsch(hw1, 4) && qsch(hw1, 4)->weight == 50);
    a.value.u8 = 0;
    CHECK_EQ(sca->set_scheduler_attribute(sd, &a), SAI_STATUS_INVALID_ATTR_VALUE_0);
    a.id = SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = ssp;
    OK(qa->set_queue_attribute(qs[5], &a));
    CHECK(qsch(hw1, 5) && qsch(hw1, 5)->strict);
    OK(qa->set_queue_attribute(qs[3], &a));
    CHECK(qsch(hw1, 3) && !qsch(hw1, 3)->strict);                 // the group's profile wins
    OK(port_set(p1, SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID, sh));
    CHECK(psch(hw1) && psch(hw1)->max_rate == 125000000u && psch(hw1)->max_burst == 65536u);
    CHECK_EQ(sca->remove_scheduler(sd), SAI_STATUS_OBJECT_IN_USE);
    a.id = SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = 0;
    OK(sga->set_scheduler_group_attribute(sgs[3], &a));
    CHECK(qsch(hw1, 3) && qsch(hw1, 3)->strict);                  // falls back to the queue's own
    OK(sga->set_scheduler_group_attribute(sgs[4], &a));
    CHECK(!qsch(hw1, 4));                  // default
    a.id = SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = 0;
    OK(qa->set_queue_attribute(qs[3], &a));
    OK(qa->set_queue_attribute(qs[5], &a));
    OK(port_set(p1, SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID, 0));
    CHECK(tp::count(tp::T_QUEUE_SCHEDULERS) == 0 && tp::count(tp::T_PORT_SCHEDULERS) == 0);
    OK(sca->remove_scheduler(sd));
    OK(sca->remove_scheduler(ssp));
    OK(sca->remove_scheduler(sh));

    // ---- WRED
    sai_attribute_t w[5];
    w[0].id = SAI_WRED_ATTR_GREEN_ENABLE; w[0].value.booldata = true;
    w[1].id = SAI_WRED_ATTR_GREEN_MIN_THRESHOLD; w[1].value.u32 = 100000;
    w[2].id = SAI_WRED_ATTR_GREEN_MAX_THRESHOLD; w[2].value.u32 = 2000000;
    w[3].id = SAI_WRED_ATTR_GREEN_DROP_PROBABILITY; w[3].value.u32 = 5;
    w[4].id = SAI_WRED_ATTR_ECN_MARK_MODE; w[4].value.s32 = SAI_ECN_MARK_MODE_GREEN;
    sai_object_id_t wr = 0;
    OK(wra->create_wred(&wr, g_sw, 5, w));
    int wid = tp::backend_id(wr);
    CHECK(wred_has(wid) && wred_of(wid).enable[0] && wred_of(wid).max[0] == 2000000 &&
          wred_of(wid).ecn == SAI_ECN_MARK_MODE_GREEN && !wred_of(wid).enable[1]);
    a.id = SAI_QUEUE_ATTR_WRED_PROFILE_ID;
    a.value.oid = wr;
    OK(qa->set_queue_attribute(qs[3], &a));
    CHECK_EQ(qw(hw1, 3), wid);
    a.id = SAI_WRED_ATTR_GREEN_MIN_THRESHOLD;
    a.value.u32 = 3000000;   // above max
    CHECK_EQ(wra->set_wred_attribute(wr, &a), SAI_STATUS_INVALID_ATTR_VALUE_0);
    a.id = SAI_WRED_ATTR_GREEN_MAX_THRESHOLD;
    a.value.u32 = 3000000;
    OK(wra->set_wred_attribute(wr, &a));
    CHECK_EQ(wred_of(wid).max[0], 3000000u);
    CHECK_EQ(wra->remove_wred(wr), SAI_STATUS_OBJECT_IN_USE);
    a.id = SAI_QUEUE_ATTR_WRED_PROFILE_ID;
    a.value.oid = 0;
    OK(qa->set_queue_attribute(qs[3], &a));
    CHECK(qw(hw1, 3) < 0);
    OK(wra->remove_wred(wr));
    CHECK(!wred_has(wid));

    // ---- buffers: pools, profiles on queues (egress) and PGs (ingress), updates
    sai_object_id_t pin = mk_pool(SAI_BUFFER_POOL_TYPE_INGRESS, 20000000);
    sai_object_id_t peg = mk_pool(SAI_BUFFER_POOL_TYPE_EGRESS, 10000000);
    sai_object_id_t bq = mk_profile(peg, 1518, 3);
    sai_object_id_t bpg = mk_profile(pin, 1248, 0, 100000);
    a.id = SAI_QUEUE_ATTR_BUFFER_PROFILE_ID;
    a.value.oid = bpg;
    CHECK_EQ(qa->set_queue_attribute(qs[0], &a), SAI_STATUS_INVALID_ATTR_VALUE_0);   // an ingress pool's
    a.value.oid = bq;
    OK(qa->set_queue_attribute(qs[0], &a));
    {
        const tp::Buffer* bp = qbuf(hw1, 0);
        CHECK(bp);
        const tp::Buffer& b = bp ? *bp : tp::Buffer();
        CHECK(!b.ingress && b.dynamic && b.dynamic_th == 3 && b.reserved == 1518 && b.pool_size == 10000000);
    }
    sai_object_id_t pgs[8];
    a.id = SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST;
    a.value.objlist.count = 8;
    a.value.objlist.list = pgs;
    OK(pa->get_port_attribute(p1, 1, &a));
    a.id = SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE;
    a.value.oid = bpg;
    OK(bfa->set_ingress_priority_group_attribute(pgs[3], &a));
    CHECK(pgbuf(hw1, 3) && pgbuf(hw1, 3)->xoff == 100000 && pgbuf(hw1, 3)->ingress);
    a.id = SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH;
    a.value.s8 = 1;
    OK(bfa->set_buffer_profile_attribute(bq, &a));
    CHECK(qbuf(hw1, 0) && qbuf(hw1, 0)->dynamic_th == 1);
    a.id = SAI_BUFFER_POOL_ATTR_SIZE;
    a.value.u64 = 12000000;
    OK(bfa->set_buffer_pool_attribute(peg, &a));
    CHECK(qbuf(hw1, 0) && qbuf(hw1, 0)->pool_size == 12000000u);
    a.id = SAI_BUFFER_PROFILE_ATTR_POOL_ID;
    a.value.oid = pin;
    CHECK_EQ(bfa->set_buffer_profile_attribute(bq, &a), SAI_STATUS_INVALID_ATTR_VALUE_0);   // create only
    a.id = SAI_PORT_ATTR_QOS_INGRESS_BUFFER_PROFILE_LIST;
    a.value.objlist.count = 1;
    a.value.objlist.list = &bpg;
    OK(pa->set_port_attribute(p1, &a));
    a.value.objlist.list = &bq;   // an egress profile in the ingress list
    CHECK_EQ(pa->set_port_attribute(p1, &a), SAI_STATUS_INVALID_ATTR_VALUE_0);
    CHECK_EQ(bfa->remove_buffer_profile(bpg), SAI_STATUS_OBJECT_IN_USE);
    CHECK_EQ(bfa->remove_buffer_pool(pin), SAI_STATUS_OBJECT_IN_USE);
    a.value.objlist.count = 0;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE;
    a.value.oid = 0;
    OK(bfa->set_ingress_priority_group_attribute(pgs[3], &a));
    a.id = SAI_QUEUE_ATTR_BUFFER_PROFILE_ID;
    OK(qa->set_queue_attribute(qs[0], &a));
    CHECK(tp::count(tp::T_QUEUE_BUFFERS) == 0 && tp::count(tp::T_PG_BUFFERS) == 0);
    OK(bfa->remove_buffer_profile(bq));
    OK(bfa->remove_buffer_profile(bpg));
    OK(bfa->remove_buffer_pool(pin));
    OK(bfa->remove_buffer_pool(peg));

    // ---- queue / PG counters, read-and-clear, capability
    tp::set_queue_stat(hw1, 3, SAI_QUEUE_STAT_PACKETS, 42);
    tp::set_queue_stat(hw1, 3, SAI_QUEUE_STAT_BYTES, 4200);
    OK(qa->get_queue_stats(qs[3], 3, qid, v));
    CHECK(v[0] == 42 && v[1] == 4200 && v[2] == 0);
    OK(qa->get_queue_stats_ext(qs[3], 2, qid, SAI_STATS_MODE_READ_AND_CLEAR, v));
    OK(qa->get_queue_stats(qs[3], 2, qid, v));
    CHECK(v[0] == 0 && v[1] == 0);
    sai_stat_id_t wm = SAI_QUEUE_STAT_SHARED_WATERMARK_BYTES;
    CHECK_EQ(qa->get_queue_stats(qs[3], 1, &wm, v), SAI_STATUS_NOT_SUPPORTED);
    tp::set_pg_stat(hw1, 3, SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS, 9);
    sai_stat_id_t pgid = SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS;
    OK(bfa->get_ingress_priority_group_stats(pgs[3], 1, &pgid, v));
    CHECK_EQ(v[0], 9u);
    sai_stat_capability_t capl[64];
    sai_stat_capability_list_t cl;
    cl.count = 64;
    cl.list = capl;
    OK(sai_query_stats_capability(g_sw, SAI_OBJECT_TYPE_QUEUE, &cl));
    CHECK_EQ(cl.count, 3u);

    // ---- mirror sessions: SPAN, ERSPAN, port ingress / egress, update, in use
    sai_object_id_t span = mk_span(p2);
    int mid = tp::backend_id(span);
    CHECK(mir_has(mid) && mir_of(mid).monitor == hw2 && !mir_of(mid).erspan);
    a.id = SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    a.value.objlist.count = 1;
    a.value.objlist.list = &span;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(pm(hw1, true) == ids(mid));
    a.id = SAI_PORT_ATTR_EGRESS_MIRROR_SESSION;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(pm(hw1, false) == ids(mid));
    CHECK(port_list_attr(p1, SAI_PORT_ATTR_INGRESS_MIRROR_SESSION) == std::vector<sai_object_id_t>(1, span));
    CHECK_EQ(mia->remove_mirror_session(span), SAI_STATUS_OBJECT_IN_USE);
    a.id = SAI_MIRROR_SESSION_ATTR_MONITOR_PORT;
    a.value.oid = cpu_port_oid();
    CHECK_EQ(mia->set_mirror_session_attribute(span, &a), SAI_STATUS_INVALID_ATTR_VALUE_0);
    a.value.oid = g_ports[3];
    OK(mia->set_mirror_session_attribute(span, &a));
    CHECK_EQ(mir_of(mid).monitor, tp::backend_id(g_ports[3]));
    sai_attribute_t e[11];
    e[0].id = SAI_MIRROR_SESSION_ATTR_TYPE; e[0].value.s32 = SAI_MIRROR_SESSION_TYPE_ENHANCED_REMOTE;
    e[1].id = SAI_MIRROR_SESSION_ATTR_MONITOR_PORT; e[1].value.oid = p2;
    e[2].id = SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE; e[2].value.s32 = SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL;
    e[3].id = SAI_MIRROR_SESSION_ATTR_IPHDR_VERSION; e[3].value.u8 = 4;
    e[4].id = SAI_MIRROR_SESSION_ATTR_TOS; e[4].value.u8 = 32;
    e[5].id = SAI_MIRROR_SESSION_ATTR_TTL; e[5].value.u8 = 64;
    e[6].id = SAI_MIRROR_SESSION_ATTR_SRC_IP_ADDRESS;
    e[6].value.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    e[6].value.ipaddr.addr.ip4 = htonl(0x0a630101);
    e[7].id = SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS;
    e[7].value.ipaddr.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    e[7].value.ipaddr.addr.ip4 = htonl(0x0a630102);
    e[8].id = SAI_MIRROR_SESSION_ATTR_SRC_MAC_ADDRESS;
    e[9].id = SAI_MIRROR_SESSION_ATTR_DST_MAC_ADDRESS;
    for (int k = 0; k < 6; k++) { e[8].value.mac[k] = (uint8_t)(0x10 + k); e[9].value.mac[k] = (uint8_t)(0x20 + k); }
    e[10].id = SAI_MIRROR_SESSION_ATTR_GRE_PROTOCOL_TYPE; e[10].value.u16 = 0x88be;
    sai_object_id_t er = 0;
    OK(mia->create_mirror_session(&er, g_sw, 11, e));
    int eid2 = tp::backend_id(er);
    {
        tp::Mirror m = mir_of(eid2);
        CHECK(m.erspan && m.tos == 32 && m.ttl == 64 && m.gre_type == 0x88be && m.dst.addr[3] == 2 && m.dmac[5] == 0x25);
    }
    a.id = SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS;
    a.value.ipaddr = e[7].value.ipaddr;
    a.value.ipaddr.addr.ip4 = htonl(0x0a630103);
    OK(mia->set_mirror_session_attribute(er, &a));
    CHECK_EQ(mir_of(eid2).dst.addr[3], 3);
    sai_object_id_t both[2] = {span, er};
    a.id = SAI_PORT_ATTR_INGRESS_MIRROR_SESSION;
    a.value.objlist.count = 2;
    a.value.objlist.list = both;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(pm(hw1, true) == ids(mid, eid2));
    a.value.objlist.count = 0;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_EGRESS_MIRROR_SESSION;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(tp::count(tp::T_PORT_MIRRORS) == 0);

    // ---- ACL MIRROR_INGRESS: the rule carries the session's backend id
    sai_object_id_t tbl = mk_l3_table();
    OK(port_set(g_ports[4], SAI_PORT_ATTR_INGRESS_ACL, tbl));
    {
        std::vector<sai_attribute_t> ea;
        sai_attribute_t x;
        std::memset(&x, 0, sizeof(x));
        x.id = SAI_ACL_ENTRY_ATTR_TABLE_ID; x.value.oid = tbl; ea.push_back(x);
        x.id = SAI_ACL_ENTRY_ATTR_PRIORITY; x.value.u32 = 10; ea.push_back(x);
        std::memset(&x, 0, sizeof(x));
        x.id = SAI_ACL_ENTRY_ATTR_FIELD_DST_IP;
        x.value.aclfield.enable = true;
        uint8_t ip[4] = {10, 99, 0, 1}, mk[4] = {0xff, 0xff, 0xff, 0xff};
        std::memcpy(&x.value.aclfield.data.ip4, ip, 4);
        std::memcpy(&x.value.aclfield.mask.ip4, mk, 4);
        ea.push_back(x);
        std::memset(&x, 0, sizeof(x));
        x.id = SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS;
        x.value.aclaction.enable = true;
        x.value.aclaction.parameter.objlist.count = 1;
        x.value.aclaction.parameter.objlist.list = &er;
        ea.push_back(x);
        sai_object_id_t ent = 0;
        OK(aa->create_acl_entry(&ent, g_sw, (uint32_t)ea.size(), ea.data()));
        const tp::FpEntry* fe = fake_entry(ent);
        CHECK(fe && fe->mirror_ingress == ids(eid2) && fe->mirror_egress.empty());
        CHECK_EQ(mia->remove_mirror_session(er), SAI_STATUS_OBJECT_IN_USE);
        sai_object_id_t got[4];
        sai_attribute_t ga2;
        std::memset(&ga2, 0, sizeof(ga2));
        ga2.id = SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS;
        ga2.value.aclaction.parameter.objlist.count = 4;
        ga2.value.aclaction.parameter.objlist.list = got;
        OK(aa->get_acl_entry_attribute(ent, 1, &ga2));
        CHECK(ga2.value.aclaction.enable && ga2.value.aclaction.parameter.objlist.count == 1 && got[0] == er);
        OK(aa->remove_acl_entry(ent));
    }
    OK(port_set(g_ports[4], SAI_PORT_ATTR_INGRESS_ACL, 0));
    OK(aa->remove_acl_table(tbl));
    OK(mia->remove_mirror_session(er));
    OK(mia->remove_mirror_session(span));
    CHECK(tp::count(tp::T_MIRRORS) == 0);

    // ---- SAMPLEPACKET on a port, rate change, in use; genetlink hostif + SAMPLEPACKET trap entry
    sai_attribute_t sp1;
    sp1.id = SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE;
    sp1.value.u32 = 1000;
    sai_object_id_t smp = 0;
    OK(spa->create_samplepacket(&smp, g_sw, 1, &sp1));
    int sid = tp::backend_id(smp);
    CHECK_EQ(tp::sample_rate(sid), 1000u);
    OK(port_set(p1, SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE, smp));
    CHECK_EQ(psam(hw1, true), sid);
    sp1.value.u32 = 500;
    OK(spa->set_samplepacket_attribute(smp, &sp1));
    CHECK_EQ(tp::sample_rate(sid), 500u);
    CHECK_EQ(spa->remove_samplepacket(smp), SAI_STATUS_OBJECT_IN_USE);
    sai_attribute_t h[3];
    h[0].id = SAI_HOSTIF_ATTR_TYPE; h[0].value.s32 = SAI_HOSTIF_TYPE_GENETLINK;
    h[1].id = SAI_HOSTIF_ATTR_NAME; std::snprintf(h[1].value.chardata, sizeof(h[1].value.chardata), "psample");
    h[2].id = SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME;
    std::snprintf(h[2].value.chardata, sizeof(h[2].value.chardata), "packets");
    sai_object_id_t gh = 0;
    OK(ha->create_hostif(&gh, g_sw, 3, h));
    int gid = tp::backend_id(gh);
    CHECK(genl_has(gid) && genl_of(gid).name == "psample" && genl_of(gid).mcgrp == "packets");
    sai_object_id_t tg = get_oid(g_sw, SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP);
    sai_object_id_t trap = mk_trap(SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET, SAI_PACKET_ACTION_TRAP, tg);
    sai_attribute_t te[4];
    te[0].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE; te[0].value.s32 = SAI_HOSTIF_TABLE_ENTRY_TYPE_TRAP_ID;
    te[1].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID; te[1].value.oid = trap;
    te[2].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE; te[2].value.s32 = SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK;
    te[3].id = SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF; te[3].value.oid = gh;
    sai_object_id_t ten = 0;
    OK(ha->create_hostif_table_entry(&ten, g_sw, 4, te));
    {
        std::vector<int32_t> gt = genl_of(gid).traps;
        CHECK(std::find(gt.begin(), gt.end(), (int32_t)SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET) != gt.end());
    }
    CHECK_EQ(ha->remove_hostif(gh), SAI_STATUS_OBJECT_IN_USE);
    OK(ha->remove_hostif_table_entry(ten));
    CHECK(genl_of(gid).traps.empty());
    OK(ha->remove_hostif(gh));
    CHECK(!genl_has(gid));
    OK(ha->remove_hostif_trap(trap));
    OK(port_set(p1, SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE, 0));
    CHECK(tp::count(tp::T_PORT_SAMPLES) == 0);
    OK(spa->remove_samplepacket(smp));
    CHECK(tp::count(tp::T_SAMPLES) == 0);

    // ---- unbind the maps; back to the default (no QoS / mirror model) for what follows
    OK(port_set(p1, SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP, 0));
    OK(port_set(p1, SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP, 0));
    CHECK(tp::count(tp::T_PORT_QOS_MAPS) == 0);
    OK(qma->remove_qos_map(m1));
    OK(qma->remove_qos_map(md));
    CHECK(tp::count(tp::T_QOS_MAPS) == 0);
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE;
    a.value.s32 = SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_COMBINED;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL;
    a.value.u8 = 0;
    OK(pa->set_port_attribute(p1, &a));
    tp::set_feature(tp::FEATURE_QOS, false);
    tp::set_feature(tp::FEATURE_MIRROR, false);
}

// Hash objects, storm control, VLAN flood control (sai_hash.cpp): kept without
// the backend hooks (the fake's default), programmed with them (l2x_on).
static void test_l2_extras()
{
    sai_hash_api_t* hsa;
    OK(sai_api_query(SAI_API_HASH, (void**)&hsa));
    sai_attribute_t a;
    a.id = SAI_SWITCH_ATTR_LAG_HASH;                              // no hash backend: unsupported, as before
    CHECK_EQ(sw->get_switch_attribute(g_sw, 1, &a), SAI_STATUS_ATTR_NOT_SUPPORTED_0);
    tp::set_feature(tp::FEATURE_L2_EXTRAS, true);
    sai_object_id_t lh = get_oid(g_sw, SAI_SWITCH_ATTR_LAG_HASH), eh = get_oid(g_sw, SAI_SWITCH_ATTR_ECMP_HASH);
    CHECK(lh && eh && lh != eh && tp::oid_type(lh) == SAI_OBJECT_TYPE_HASH);
    int32_t f[3] = {SAI_NATIVE_HASH_FIELD_SRC_IP, SAI_NATIVE_HASH_FIELD_DST_IP, SAI_NATIVE_HASH_FIELD_L4_SRC_PORT};
    a.id = SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST;
    a.value.s32list.count = 3;
    a.value.s32list.list = f;
    OK(hsa->set_hash_attribute(eh, &a));
    CHECK(hash_has(false) && hash_of(false).fields.size() == 3);
    int32_t back[8];
    a.value.s32list.count = 8;
    a.value.s32list.list = back;
    OK(hsa->get_hash_attribute(eh, 1, &a));
    CHECK(a.value.s32list.count == 3 && back[2] == SAI_NATIVE_HASH_FIELD_L4_SRC_PORT);
    sai_object_id_t hx;
    CHECK_EQ(hsa->create_hash(&hx, g_sw, 0, nullptr), SAI_STATUS_NOT_SUPPORTED);
    tp::set_feature(tp::FEATURE_L2_EXTRAS, false);
    sai_object_id_t pol = mk_policer(1000, 1000);
    sai_attribute_t s;
    s.id = SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID;
    s.value.oid = pol;
    CHECK_EQ(pa->set_port_attribute(g_ports[1], &s), SAI_STATUS_ATTR_NOT_SUPPORTED_0);   // no hook: as before
    CHECK(tp::count(tp::T_STORM) == 0);
    sai_attribute_t va1;
    va1.id = SAI_VLAN_ATTR_VLAN_ID;
    va1.value.u16 = 300;
    sai_object_id_t v300;
    OK(va->create_vlan(&v300, g_sw, 1, &va1));
    sai_attribute_t fl;
    fl.id = SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE;
    fl.value.s32 = SAI_VLAN_FLOOD_CONTROL_TYPE_NONE;
    OK(va->set_vlan_attribute(v300, &fl));
    CHECK(tp::count(tp::T_FLOOD) == 0);

    tp::set_feature(tp::FEATURE_L2_EXTRAS, true);
    a.value.s32list.count = 3;
    a.value.s32list.list = f;
    OK(hsa->set_hash_attribute(lh, &a));
    CHECK(hash_has(true) && hash_of(true).fields.size() == 3);
    sai_attribute_t sd;
    sd.id = SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED;
    sd.value.u32 = 77;
    OK(sw->set_switch_attribute(g_sw, &sd));
    CHECK(hash_of(true).seed == 77u && hash_of(true).fields.size() == 3);
    sd.value.u32 = 0;
    OK(sw->get_switch_attribute(g_sw, 1, &sd));
    CHECK_EQ(sd.value.u32, 77u);
    s.id = SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID;
    s.value.oid = pol;
    OK(pa->set_port_attribute(g_ports[1], &s));
    CHECK_EQ(tp::port_storm(tp::backend_id(g_ports[1]), SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID), tp::backend_id(pol));
    CHECK_EQ(poa->remove_policer(pol), SAI_STATUS_OBJECT_IN_USE);
    s.value.oid = 0x1234;
    CHECK_EQ(pa->set_port_attribute(g_ports[1], &s), SAI_STATUS_INVALID_ATTR_VALUE_0);
    OK(va->set_vlan_attribute(v300, &fl));
    CHECK(tp::vlan_flood(300, SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE) >= 0 &&
          tp::vlan_flood(300, SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE) == 0);
    fl.value.s32 = SAI_VLAN_FLOOD_CONTROL_TYPE_ALL;
    OK(va->set_vlan_attribute(v300, &fl));
    CHECK(tp::vlan_flood(300, SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE) == 1);
    s.value.oid = 0;
    OK(pa->set_port_attribute(g_ports[1], &s));
    s.id = SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID;
    OK(pa->set_port_attribute(g_ports[1], &s));
    CHECK(tp::count(tp::T_STORM) == 0);
    OK(poa->remove_policer(pol));
    OK(va->remove_vlan(v300));
    tp::set_feature(tp::FEATURE_L2_EXTRAS, false);
}
