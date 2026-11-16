// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, QoS and buffers (tl_qos.cpp) on the mock libifcs:
// DSCP / DOT1P -> TC maps (IFCS map + shared map group + sysport attrs),
// map update, the TC -> queue identity rule, default TC, PFC, queue
// schedulers through scheduler groups, the port shaper, WRED (AQM curve
// groups in cells, queue flags), queue buffer profiles (reserve with the
// port flapped, thresholds), queue / PG / PFC counters (harness: tests/harness.h).
#include "harness.h"

// IFCS ids the checks name (values: src/ifcs_api_qos.h, src/ifcs_api.h)
enum : uint32_t { Q_WDRR_WEIGHT = 14 };
enum : int { QS_TOTAL_USE_WATERMARK = 1, QS_PACKET_DROP = 3, QS_PACKET_TX = 5, QS_BYTE_TX = 6, BAM_PACKET_DROP = 2,
             DP_RX_FRAMES_PFC0 = 21 };

static int (*m_maps)();
static int (*m_groups)();
static int (*m_curves)();
static void (*m_qstat)(uint32_t, uint32_t, int, uint64_t);
static void (*m_bstat)(uint32_t, uint32_t, int, uint64_t);
static uint32_t (*m_qattr)(uint32_t, uint32_t, uint32_t);

static size_t count(const std::string& log, const std::string& needle)
{
    size_t n = 0;
    for (size_t p = log.find(needle); p != std::string::npos; p = log.find(needle, p + 1)) n++;
    return n;
}

static sai_object_id_t qmap(sai_qos_map_api_t* qa, sai_object_id_t s, int32_t type, std::vector<sai_qos_map_t> l,
                            sai_status_t* st = nullptr)
{
    sai_attribute_t a[2];
    a[0].id = SAI_QOS_MAP_ATTR_TYPE;
    a[0].value.s32 = type;
    a[1].id = SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST;
    a[1].value.qosmap.count = (uint32_t)l.size();
    a[1].value.qosmap.list = l.data();
    sai_object_id_t m = 0;
    sai_status_t r = qa->create_qos_map(&m, s, 2, a);
    if (st) *st = r;
    else OK(r);
    return m;
}
static std::vector<sai_qos_map_t> entries(int n, int (*f)(int), bool dscp, bool tcq = false)
{
    std::vector<sai_qos_map_t> v;
    for (int i = 0; i < n; i++) {
        sai_qos_map_t m;
        std::memset(&m, 0, sizeof(m));
        if (tcq) { m.key.tc = (sai_cos_t)i; m.value.queue_index = (sai_queue_index_t)f(i); }
        else if (dscp) { m.key.dscp = (uint8_t)i; m.value.tc = (sai_cos_t)f(i); }
        else { m.key.dot1p = (uint8_t)i; m.value.tc = (sai_cos_t)f(i); }
        v.push_back(m);
    }
    return v;
}
static std::vector<sai_object_id_t> objlist(sai_port_api_t* pa, sai_object_id_t p, sai_attr_id_t id)
{
    sai_object_id_t l[32];
    sai_attribute_t a;
    a.id = id;
    a.value.objlist.count = 32;
    a.value.objlist.list = l;
    OK(pa->get_port_attribute(p, 1, &a));
    return std::vector<sai_object_id_t>(l, l + a.value.objlist.count);
}

void test_qos(Env& e)
{
    USE_ENV(e);
    hook(g_mock, m_maps, "mock_qos_maps");
    hook(g_mock, m_groups, "mock_qos_groups");
    hook(g_mock, m_curves, "mock_qos_curves");
    hook(g_mock, m_qstat, "mock_queue_stat");
    hook(g_mock, m_bstat, "mock_bam_stat");
    hook(g_mock, m_qattr, "mock_queue_attr");
    sai_qos_map_api_t* qma;
    sai_scheduler_api_t* sca;
    sai_scheduler_group_api_t* sga;
    sai_wred_api_t* wra;
    sai_buffer_api_t* bfa;
    sai_queue_api_t* qa;
    OK(sai_api_query(SAI_API_QOS_MAP, (void**)&qma));
    OK(sai_api_query(SAI_API_SCHEDULER, (void**)&sca));
    OK(sai_api_query(SAI_API_SCHEDULER_GROUP, (void**)&sga));
    OK(sai_api_query(SAI_API_WRED, (void**)&wra));
    OK(sai_api_query(SAI_API_BUFFER, (void**)&bfa));
    OK(sai_api_query(SAI_API_QUEUE, (void**)&qa));
    sai_attribute_t a;

    // ---- DSCP -> TC: an IFCS map at create (64 entries, dp 0), a group on first use, both sysport attrs
    m_log_clear();
    sai_object_id_t md = qmap(qma, s, SAI_QOS_MAP_TYPE_DSCP_TO_TC, entries(64, [](int d) { return d >> 3; }, true));
    CHECK(has(m_log(), "ifcs_qos_map_create h=") && has(m_log(), "type=1 count=64 0:0/0") && has(m_log(), " 46:5/0 "));
    CHECK_EQ(m_maps(), 1);
    m_log_clear();
    a.id = SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP;
    a.value.oid = md;
    OK(pa->set_port_attribute(p1, &a));
    std::string log = m_log();
    CHECK(has(log, "ifcs_qos_map_group_create") && has(log, " 0=0x0 2=0x"));      // ACTION_TYPE MAP, DSCP_TO_TC_DP
    CHECK(has(log, "ifcs_sysport_attr_set sp=0x3e200001 id=1 u32=") && has(log, "ifcs_sysport_attr_set sp=0x3e200001 id=0 u32=0"));
    CHECK_EQ(m_groups(), 1);
    OK(pa->set_port_attribute(p2, &a));                                          // same map: the group is shared
    CHECK_EQ(m_groups(), 1);
    // DOT1P -> TC on p1: a second group (DSCP + DOT1P), the old one stays for p2
    sai_object_id_t m1p = qmap(qma, s, SAI_QOS_MAP_TYPE_DOT1P_TO_TC, entries(8, [](int d) { return 7 - d; }, false));
    CHECK(has(m_log(), "type=0 count=8 0:7/0"));
    m_log_clear();
    a.id = SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP;
    a.value.oid = m1p;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), " 2=0x") && has(m_log(), " 1=0x"));
    CHECK_EQ(m_groups(), 2);
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=0 u32=") && !has(m_log(), "id=0 u32=0\n"));
    // map update: one attr_set with the new table
    m_log_clear();
    std::vector<sai_qos_map_t> upd = entries(64, [](int d) { return d == 46 ? 6 : d >> 3; }, true);
    a.id = SAI_QOS_MAP_ATTR_MAP_TO_VALUE_LIST;
    a.value.qosmap.count = 64;
    a.value.qosmap.list = upd.data();
    OK(qma->set_qos_map_attribute(md, &a));
    CHECK(has(m_log(), "ifcs_qos_map_attr_set") && has(m_log(), " 46:6/0 "));
    // unbinding: p2 off -> its group goes once nobody uses it
    a.id = SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP;
    a.value.oid = 0;
    OK(pa->set_port_attribute(p2, &a));
    CHECK_EQ(m_groups(), 1);
    CHECK_EQ(qma->remove_qos_map(md), SAI_STATUS_OBJECT_IN_USE);
    // TC -> queue: identity accepted (nothing programmed), anything else NOT_SUPPORTED; TC -> PG kept
    m_log_clear();
    sai_object_id_t mq = qmap(qma, s, SAI_QOS_MAP_TYPE_TC_TO_QUEUE, entries(8, [](int d) { return d; }, false, true));
    a.id = SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP;
    a.value.oid = mq;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(!has(m_log(), "ifcs_"));
    sai_status_t st;
    qmap(qma, s, SAI_QOS_MAP_TYPE_TC_TO_QUEUE, entries(8, [](int d) { return d == 1 ? 2 : d == 2 ? 1 : d; }, false, true), &st);
    CHECK_EQ(st, SAI_STATUS_NOT_SUPPORTED);
    std::vector<sai_qos_map_t> tcpg(8);
    std::memset(tcpg.data(), 0, 8 * sizeof(sai_qos_map_t));
    for (int i = 0; i < 8; i++) { tcpg[i].key.tc = (sai_cos_t)i; tcpg[i].value.pg = (uint8_t)(i == 3 || i == 4 ? i : 0); }
    sai_object_id_t mpg = qmap(qma, s, SAI_QOS_MAP_TYPE_TC_TO_PRIORITY_GROUP, tcpg);
    a.id = SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP;
    a.value.oid = mpg;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(!has(m_log(), "ifcs_"));
    std::vector<sai_qos_map_t> rem(1);
    std::memset(rem.data(), 0, sizeof(sai_qos_map_t));
    qmap(qma, s, SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP, rem, &st);
    CHECK_EQ(st, SAI_STATUS_NOT_SUPPORTED);

    // ---- default TC, PFC (combined then separate)
    m_log_clear();
    a.id = SAI_PORT_ATTR_QOS_DEFAULT_TC;
    a.value.u8 = 1;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "ifcs_sysport_attr_set sp=0x3e200001 id=49 u32=1"));
    m_log_clear();
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL;
    a.value.u8 = 0x18;
    OK(pa->set_port_attribute(p1, &a));
    log = m_log();
    CHECK(has(log, "dp=1 id=82 u32=1") && has(log, "dp=1 id=83 u32=1") && has(log, "dp=1 id=79 u32=0") && has(log, "dp=1 id=86 u32=0"));
    m_log_clear();
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE;
    a.value.s32 = SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_SEPARATE;
    OK(pa->set_port_attribute(p1, &a));
    log = m_log();
    CHECK(has(log, "dp=1 id=78 u32=1"));                                         // PFC_MODE separate
    m_log_clear();
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX;
    a.value.u8 = 0x08;
    OK(pa->set_port_attribute(p1, &a));
    log = m_log();
    CHECK(has(log, "dp=1 id=90 u32=1") && has(log, "dp=1 id=98 u32=0") && !has(log, "id=78"));   // PFC3 TX on, RX off
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_TX;
    a.value.u8 = 0;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE;
    a.value.s32 = SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_COMBINED;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL;
    a.value.u8 = 0;
    OK(pa->set_port_attribute(p1, &a));

    // ---- schedulers via the queue's scheduler group; weight change follows; strict; port shaper
    std::vector<sai_object_id_t> qs = objlist(pa, p1, SAI_PORT_ATTR_QOS_QUEUE_LIST);
    std::vector<sai_object_id_t> sgs = objlist(pa, p1, SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST);
    CHECK(qs.size() == 8 && sgs.size() == 8);
    sai_attribute_t sc[2];
    sc[0].id = SAI_SCHEDULER_ATTR_SCHEDULING_TYPE;
    sc[0].value.s32 = SAI_SCHEDULING_TYPE_DWRR;
    sc[1].id = SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT;
    sc[1].value.u8 = 20;
    sai_object_id_t sd = 0, ssp = 0, sh = 0;
    OK(sca->create_scheduler(&sd, s, 2, sc));
    sc[0].value.s32 = SAI_SCHEDULING_TYPE_STRICT;
    OK(sca->create_scheduler(&ssp, s, 1, sc));
    m_log_clear();
    a.id = SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = sd;
    OK(sga->set_scheduler_group_attribute(sgs[3], &a));
    CHECK(has(m_log(), "ifcs_queue_attr_set dp=1 q=3 n=4 13=1 14=20 16=0 21=0"));
    CHECK_EQ(m_qattr(1, 3, Q_WDRR_WEIGHT), 20u);
    m_log_clear();
    a.id = SAI_SCHEDULER_ATTR_SCHEDULING_WEIGHT;
    a.value.u8 = 50;
    OK(sca->set_scheduler_attribute(sd, &a));
    CHECK(has(m_log(), "ifcs_queue_attr_set dp=1 q=3 n=4 13=1 14=50 16=0 21=0"));
    m_log_clear();
    a.id = SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = ssp;
    OK(sga->set_scheduler_group_attribute(sgs[5], &a));
    CHECK(has(m_log(), "ifcs_queue_attr_set dp=1 q=5 n=3 13=0 16=0 21=0"));
    m_log_clear();
    a.value.oid = 0;
    OK(sga->set_scheduler_group_attribute(sgs[5], &a));
    CHECK(has(m_log(), "ifcs_queue_attr_set dp=1 q=5 n=4 13=1 14=1 16=0 21=0"));   // default: WDRR 1
    sai_attribute_t shp[3];
    shp[0].id = SAI_SCHEDULER_ATTR_METER_TYPE; shp[0].value.s32 = SAI_METER_TYPE_BYTES;
    shp[1].id = SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_RATE; shp[1].value.u64 = 125000000;
    shp[2].id = SAI_SCHEDULER_ATTR_MAX_BANDWIDTH_BURST_RATE; shp[2].value.u64 = 65536;
    OK(sca->create_scheduler(&sh, s, 3, shp));
    m_log_clear();
    a.id = SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID;
    a.value.oid = sh;
    OK(pa->set_port_attribute(p1, &a));
    log = m_log();
    CHECK(has(log, "ifcs_devport_attr_set dp=1 id=104 u32=1") && has(log, "dp=1 id=105") && has(log, "dp=1 id=106 u32=65536"));
    CHECK(has(log, "dp=1 id=105 u32=1000000000"));                                // 125000000 B/s as bits/s
    m_log_clear();
    a.value.oid = 0;
    OK(pa->set_port_attribute(p1, &a));
    CHECK(has(m_log(), "ifcs_devport_attr_set dp=1 id=104 u32=0"));
    // queue shaper: a scheduler with a max rate on a queue
    m_log_clear();
    a.id = SAI_SCHEDULER_GROUP_ATTR_SCHEDULER_PROFILE_ID;
    a.value.oid = sh;
    OK(sga->set_scheduler_group_attribute(sgs[1], &a));
    CHECK(has(m_log(), "ifcs_queue_attr_set dp=1 q=1 n=6 13=1 14=1 16=0 21=1 22=1000000000 23=65536"));   // bits/s
    a.value.oid = 0;
    OK(sga->set_scheduler_group_attribute(sgs[1], &a));
    OK(sga->set_scheduler_group_attribute(sgs[3], &a));
    OK(sca->remove_scheduler(sd));
    OK(sca->remove_scheduler(ssp));
    OK(sca->remove_scheduler(sh));

    // ---- WRED: curve group in cells (isai: 100000 B -> 440, 2000000 B -> 8928), queue flags
    sai_attribute_t w[5];
    w[0].id = SAI_WRED_ATTR_GREEN_ENABLE; w[0].value.booldata = true;
    w[1].id = SAI_WRED_ATTR_GREEN_MIN_THRESHOLD; w[1].value.u32 = 100000;
    w[2].id = SAI_WRED_ATTR_GREEN_MAX_THRESHOLD; w[2].value.u32 = 2000000;
    w[3].id = SAI_WRED_ATTR_GREEN_DROP_PROBABILITY; w[3].value.u32 = 5;
    w[4].id = SAI_WRED_ATTR_ECN_MARK_MODE; w[4].value.s32 = SAI_ECN_MARK_MODE_GREEN;
    sai_object_id_t wr = 0;
    m_log_clear();
    OK(wra->create_wred(&wr, s, 5, w));
    CHECK(has(m_log(), "ifcs_aqm_curve_group_create") && has(m_log(), " 0=440 1=8928 2=5 6=45728 7=45728 8=100 12=45728 13=45728 14=100"));
    CHECK_EQ(m_curves(), 1);
    m_log_clear();
    a.id = SAI_QUEUE_ATTR_WRED_PROFILE_ID;
    a.value.oid = wr;
    OK(qa->set_queue_attribute(qs[3], &a));
    log = m_log();
    CHECK(has(log, "ifcs_queue_attr_set dp=1 q=3 n=1 26=") && has(log, "ifcs_queue_attr_set dp=1 q=3 n=1 28=1") &&
          has(log, "ifcs_queue_attr_set dp=1 q=3 n=1 27=1") && !has(log, " 32="));   // no averaging weight: the default
    m_log_clear();
    a.id = SAI_WRED_ATTR_GREEN_MAX_THRESHOLD;
    a.value.u32 = 3000000;
    OK(wra->set_wred_attribute(wr, &a));
    CHECK(has(m_log(), "ifcs_aqm_curve_group_attr_set") && has(m_log(), " 1=13392 "));
    CHECK_EQ(wra->remove_wred(wr), SAI_STATUS_OBJECT_IN_USE);
    m_log_clear();
    a.id = SAI_QUEUE_ATTR_WRED_PROFILE_ID;
    a.value.oid = 0;
    OK(qa->set_queue_attribute(qs[3], &a));
    CHECK(has(m_log(), "ifcs_queue_attr_set dp=1 q=3 n=1 28=0") && has(m_log(), "ifcs_queue_attr_set dp=1 q=3 n=1 27=0") &&
          has(m_log(), "ifcs_queue_attr_set dp=1 q=3 n=1 26=0"));
    // a profile without ECN never writes ECN_EXTENSION_ENABLE (0.12.41 refuses it on this queue type)
    sai_attribute_t w2[3] = {w[0], w[1], w[2]};
    sai_object_id_t wr2 = 0;
    OK(wra->create_wred(&wr2, s, 3, w2));
    m_log_clear();
    a.value.oid = wr2;
    OK(qa->set_queue_attribute(qs[3], &a));
    CHECK(has(m_log(), "n=1 27=1") && !has(m_log(), " 28="));
    a.value.oid = 0;
    OK(qa->set_queue_attribute(qs[3], &a));
    CHECK(!has(m_log(), " 28="));
    OK(wra->remove_wred(wr2));
    OK(wra->remove_wred(wr));
    CHECK_EQ(m_curves(), 0);

    // ---- buffer profile on a queue: reserve 1518 B = 7 cells with the port flapped, dynamic th 3 -> 8 / 4
    sai_attribute_t pl[3];
    pl[0].id = SAI_BUFFER_POOL_ATTR_TYPE; pl[0].value.s32 = SAI_BUFFER_POOL_TYPE_EGRESS;
    pl[1].id = SAI_BUFFER_POOL_ATTR_SIZE; pl[1].value.u64 = 10000000;
    pl[2].id = SAI_BUFFER_POOL_ATTR_THRESHOLD_MODE; pl[2].value.s32 = SAI_BUFFER_POOL_THRESHOLD_MODE_DYNAMIC;
    sai_object_id_t pool = 0, pin = 0, prof = 0, ppg = 0;
    OK(bfa->create_buffer_pool(&pool, s, 3, pl));
    pl[0].value.s32 = SAI_BUFFER_POOL_TYPE_INGRESS;
    OK(bfa->create_buffer_pool(&pin, s, 3, pl));
    sai_attribute_t bp[3];
    bp[0].id = SAI_BUFFER_PROFILE_ATTR_POOL_ID; bp[0].value.oid = pool;
    bp[1].id = SAI_BUFFER_PROFILE_ATTR_RESERVED_BUFFER_SIZE; bp[1].value.u64 = 1518;
    bp[2].id = SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH; bp[2].value.s8 = 3;
    OK(bfa->create_buffer_profile(&prof, s, 3, bp));
    bp[0].value.oid = pin;
    OK(bfa->create_buffer_profile(&ppg, s, 3, bp));
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = true;
    OK(pa->set_port_attribute(p1, &a));
    m_log_clear();
    a.id = SAI_QUEUE_ATTR_BUFFER_PROFILE_ID;
    a.value.oid = prof;
    OK(qa->set_queue_attribute(qs[0], &a));
    log = m_log();
    size_t down = log.find("ifcs_devport_attr_set dp=1 id=23 u32=0"), set = log.find("ifcs_queue_attr_set dp=1 q=0 n=4 2=7 3=1 6=8 9=4"),
           up = log.find("ifcs_devport_attr_set dp=1 id=23 u32=1"), adm = log.find("ifcs_queue_attr_set dp=1 q=0 n=2 35=0 34=0");
    CHECK(down != std::string::npos && set != std::string::npos && up != std::string::npos && adm != std::string::npos);
    CHECK(down < set && set < up && up < adm);
    m_log_clear();   // threshold-only change: no flap
    a.id = SAI_BUFFER_PROFILE_ATTR_SHARED_DYNAMIC_TH;
    a.value.s8 = 1;
    OK(bfa->set_buffer_profile_attribute(prof, &a));
    CHECK(has(m_log(), "ifcs_queue_attr_set dp=1 q=0 n=3 3=1 6=6 9=2") && !has(m_log(), "id=23"));
    m_log_clear();   // unset: the SDK's values back (reserve 14 cells read at first use)
    a.id = SAI_QUEUE_ATTR_BUFFER_PROFILE_ID;
    a.value.oid = 0;
    OK(qa->set_queue_attribute(qs[0], &a));
    CHECK(has(m_log(), "ifcs_queue_attr_set dp=1 q=0 n=6 2=14 "));
    // PG profile: kept, nothing programmed
    sai_object_id_t pgs[8];
    a.id = SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST;
    a.value.objlist.count = 8;
    a.value.objlist.list = pgs;
    OK(pa->get_port_attribute(p1, 1, &a));
    m_log_clear();
    a.id = SAI_INGRESS_PRIORITY_GROUP_ATTR_BUFFER_PROFILE;
    a.value.oid = ppg;
    OK(bfa->set_ingress_priority_group_attribute(pgs[3], &a));
    CHECK(!has(m_log(), "ifcs_"));
    a.value.oid = 0;
    OK(bfa->set_ingress_priority_group_attribute(pgs[3], &a));
    OK(bfa->remove_buffer_profile(prof));
    OK(bfa->remove_buffer_profile(ppg));
    OK(bfa->remove_buffer_pool(pool));
    OK(bfa->remove_buffer_pool(pin));
    a.id = SAI_PORT_ATTR_ADMIN_STATE;
    a.value.booldata = false;
    OK(pa->set_port_attribute(p1, &a));

    // ---- counters: queue (one batched call; watermarks in cells x 224), PG (BAM), PFC frames
    m_qstat(1, 3, QS_PACKET_TX, 50);
    m_qstat(1, 3, QS_BYTE_TX, 5000);
    m_qstat(1, 3, QS_PACKET_DROP, 2);
    m_qstat(1, 3, QS_TOTAL_USE_WATERMARK, 10);
    sai_stat_id_t ids[4] = {SAI_QUEUE_STAT_PACKETS, SAI_QUEUE_STAT_BYTES, SAI_QUEUE_STAT_DROPPED_PACKETS,
                            SAI_QUEUE_STAT_SHARED_WATERMARK_BYTES};
    uint64_t v[4] = {0, 0, 0, 0};
    m_log_clear();
    OK(qa->get_queue_stats(qs[3], 4, ids, v));
    CHECK(v[0] == 50 && v[1] == 5000 && v[2] == 2 && v[3] == 2240);
    CHECK_EQ(count(m_log(), "ifcs_queue_stats_get dp=1 q=3 ids=5,6,3,1,"), 1u);
    OK(qa->get_queue_stats_ext(qs[3], 4, ids, SAI_STATS_MODE_READ_AND_CLEAR, v));
    OK(qa->get_queue_stats(qs[3], 4, ids, v));
    CHECK(v[0] == 0 && v[1] == 0 && v[3] == 0);
    sai_stat_id_t db = SAI_QUEUE_STAT_DROPPED_BYTES;
    CHECK_EQ(qa->get_queue_stats(qs[3], 1, &db, v), SAI_STATUS_NOT_SUPPORTED);
    m_bstat(1, 3, BAM_PACKET_DROP, 7);
    sai_stat_id_t pid = SAI_INGRESS_PRIORITY_GROUP_STAT_DROPPED_PACKETS;
    OK(bfa->get_ingress_priority_group_stats(pgs[3], 1, &pid, v));
    CHECK_EQ(v[0], 7u);
    sai_stat_id_t pp = SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS;
    CHECK_EQ(bfa->get_ingress_priority_group_stats(pgs[3], 1, &pp, v), SAI_STATUS_NOT_SUPPORTED);
    m_set_stat(1, DP_RX_FRAMES_PFC0 + 3, 11);
    sai_stat_id_t pf = SAI_PORT_STAT_PFC_3_RX_PKTS;
    OK(pa->get_port_stats(p1, 1, &pf, v));
    CHECK_EQ(v[0], 11u);
    sai_stat_capability_t capl[64];
    sai_stat_capability_list_t cl;
    cl.count = 64;
    cl.list = capl;
    OK(sai_query_stats_capability(s, SAI_OBJECT_TYPE_QUEUE, &cl));
    CHECK_EQ(cl.count, 8u);                                                      // the 8 mapped queue counters

    // ---- teardown of the maps
    a.id = SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP;
    a.value.oid = 0;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP;
    OK(pa->set_port_attribute(p1, &a));
    a.id = SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP;
    OK(pa->set_port_attribute(p1, &a));
    CHECK_EQ(m_groups(), 0);
    OK(qma->remove_qos_map(md));
    OK(qma->remove_qos_map(m1p));
    OK(qma->remove_qos_map(mq));
    OK(qma->remove_qos_map(mpg));
    CHECK_EQ(m_maps(), 0);
}
