/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: QoS / buffers (tl_qos.cpp): qos maps + groups, sysport
 * get, queues (attributes recorded, a few read back, counters settable),
 * BAM queue counters, AQM curve groups (mock.h). */
#include "mock.h"

#define MAX_Q (MAX_DP * 8)
static uint64_t qstat[MAX_Q][16], bstat[MAX_Q][8];
static uint32_t qattr[MAX_Q][48];
static int qmaps, qgroups, curves;

static int qix(const ifcs_queue_t* k) { return k && k->devport < MAX_DP && k->queue_id < 8 ? (int)(k->devport * 8 + k->queue_id) : -1; }

/* test hooks */
int mock_qos_maps(void) { return qmaps; }
int mock_qos_groups(void) { return qgroups; }
int mock_qos_curves(void) { return curves; }
void mock_queue_stat(uint32_t dp, uint32_t q, int id, uint64_t v) { if (dp < MAX_DP && q < 8 && id >= 0 && id < 16) qstat[dp * 8 + q][id] = v; }
void mock_bam_stat(uint32_t dp, uint32_t q, int id, uint64_t v) { if (dp < MAX_DP && q < 8 && id >= 0 && id < 8) bstat[dp * 8 + q][id] = v; }
uint32_t mock_queue_attr(uint32_t dp, uint32_t q, uint32_t id) { return dp < MAX_DP && q < 8 && id < 48 ? qattr[dp * 8 + q][id] : 0xdead; }

static void log_map_info(const char* fn, ifcs_handle_t h, const ifcs_attr_t* a)
{
    ifcs_qos_map_info_t info;
    memcpy(&info, &a->value, sizeof(info));
    char b[1024];
    int n = snprintf(b, sizeof(b), "%s h=0x%x type=%d count=%u", fn, h, info.type, info.list.count);
    for (uint32_t i = 0; i < info.list.count && n < (int)sizeof(b) - 32; i++) {
        const ifcs_qos_map_t* m = &info.list.arr[i];
        uint32_t src = info.type == IFCS_QOS_MAP_TYPE_DSCP_TO_TC_DP ? m->map_src.dscp : m->map_src.dot1p_cfi.dot1p;
        n += snprintf(b + n, sizeof(b) - (size_t)n, " %u:%u/%u", src, m->map_value.tc_dp.tc, m->map_value.tc_dp.dp);
    }
    mock_logf("%s", b);
}

ifcs_status_t ifcs_qos_map_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (c != 1 || a[0].id != IFCS_QOS_MAP_ATTR_QOS_MAP_INFO) return IFCS_PARAM;
    *h = next_handle++;
    log_map_info("ifcs_qos_map_create", *h, &a[0]);
    qmaps++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_qos_map_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (c != 1) return IFCS_PARAM;
    log_map_info("ifcs_qos_map_attr_set", h, &a[0]);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_qos_map_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_qos_map_delete h=0x%x", h); qmaps--; return IFCS_SUCCESS; }
ifcs_status_t ifcs_qos_map_info_t_init(ifcs_qos_map_info_t* i) { memset(i, 0, sizeof(*i)); return IFCS_SUCCESS; }
ifcs_status_t ifcs_qos_map_group_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    char b[256];
    int k = snprintf(b, sizeof(b), "ifcs_qos_map_group_create h=0x%x", *h);
    for (uint32_t i = 0; i < c; i++) k += snprintf(b + k, sizeof(b) - (size_t)k, " %u=0x%x", a[i].id, a[i].value.u32);
    mock_logf("%s", b);
    qgroups++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_qos_map_group_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++) mock_logf("ifcs_qos_map_group_attr_set h=0x%x id=%u v=0x%x", h, a[i].id, a[i].value.u32);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_qos_map_group_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_qos_map_group_delete h=0x%x", h); qgroups--; return IFCS_SUCCESS; }
ifcs_status_t ifcs_sysport_attr_get(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a, uint32_t* got)
{
    (void)n; (void)h; (void)a;
    if (got) *got = c;
    return IFCS_SUCCESS;
}

ifcs_status_t ifcs_queue_t_init(ifcs_queue_t* k) { memset(k, 0, sizeof(*k)); k->_base_key = 0x38200000u; return IFCS_SUCCESS; }
ifcs_status_t ifcs_queue_t_devport_set(ifcs_queue_t* k, ifcs_devport_t d) { k->devport = d; k->dirty |= 1; return IFCS_SUCCESS; }
ifcs_status_t ifcs_queue_t_queue_id_set(ifcs_queue_t* k, uint32_t q) { k->queue_id = q; k->dirty |= 2; return IFCS_SUCCESS; }
ifcs_status_t ifcs_queue_attr_set(ifcs_node_id_t n, ifcs_queue_t* k, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    int q = qix(k);
    if (q < 0 || k->_base_key != 0x38200000u || k->dirty != 3) return IFCS_PARAM;
    char b[512];
    int m = snprintf(b, sizeof(b), "ifcs_queue_attr_set dp=%u q=%u n=%u", k->devport, k->queue_id, c);
    for (uint32_t i = 0; i < c; i++) {
        uint64_t v = (a[i].id == IFCS_QUEUE_ATTR_MAX_RATE || a[i].id == IFCS_QUEUE_ATTR_GUARANTEED_RATE) ? a[i].value.u64 : a[i].value.u32;
        m += snprintf(b + m, sizeof(b) - (size_t)m, " %u=%llu", a[i].id, (unsigned long long)v);
        if (a[i].id < 48) qattr[q][a[i].id] = a[i].value.u32;
    }
    mock_logf("%s", b);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_queue_attr_get(ifcs_node_id_t n, ifcs_queue_t* k, uint32_t c, ifcs_attr_t* a, uint32_t* got)
{
    (void)n;
    int q = qix(k);
    if (q < 0) return IFCS_PARAM;
    for (uint32_t i = 0; i < c; i++) {
        uint32_t v = a[i].id < 48 ? qattr[q][a[i].id] : 0;
        if (a[i].id == IFCS_QUEUE_ATTR_RESERVED_SIZE && !v) v = 14;   /* the SDK's default (isai read 14) */
        a[i].value.u32 = v;
    }
    if (got) *got = c;
    mock_logf("ifcs_queue_attr_get dp=%u q=%u n=%u", k->devport, k->queue_id, c);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_queue_stats_get(ifcs_node_id_t n, ifcs_queue_t* k, uint32_t c, ifcs_queue_stats_id_t* ids, uint64_t* v, uint32_t* got)
{
    (void)n;
    int q = qix(k);
    if (q < 0) return IFCS_PARAM;
    char b[256];
    int m = snprintf(b, sizeof(b), "ifcs_queue_stats_get dp=%u q=%u ids=", k->devport, k->queue_id);
    for (uint32_t i = 0; i < c; i++) {
        v[i] = ids[i] >= 0 && ids[i] < 16 ? qstat[q][ids[i]] : 0;
        m += snprintf(b + m, sizeof(b) - (size_t)m, "%d,", ids[i]);
    }
    if (got) *got = c;
    mock_logf("%s", b);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_queue_stats_clear(ifcs_node_id_t n, ifcs_queue_t* k, uint32_t c, ifcs_queue_stats_id_t* ids)
{
    (void)n;
    int q = qix(k);
    if (q < 0) return IFCS_PARAM;
    for (uint32_t i = 0; i < c; i++) if (ids[i] >= 0 && ids[i] < 16) qstat[q][ids[i]] = 0;
    mock_logf("ifcs_queue_stats_clear dp=%u q=%u n=%u", k->devport, k->queue_id, c);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_bam_queue_t_init(ifcs_bam_queue_t* k) { memset(k, 0, sizeof(*k)); k->_base_key = 0x38400000u; return IFCS_SUCCESS; }
ifcs_status_t ifcs_bam_queue_stats_get(ifcs_node_id_t n, ifcs_bam_queue_t* k, uint32_t c, ifcs_bam_queue_stats_id_t* ids, uint64_t* v, uint32_t* got)
{
    (void)n;
    int q = qix(k);
    if (q < 0 || k->dirty != 3) return IFCS_PARAM;
    for (uint32_t i = 0; i < c; i++) v[i] = ids[i] >= 0 && ids[i] < 8 ? bstat[q][ids[i]] : 0;
    if (got) *got = c;
    mock_logf("ifcs_bam_queue_stats_get dp=%u q=%u n=%u", k->devport, k->queue_id, c);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_bam_queue_stats_clear(ifcs_node_id_t n, ifcs_bam_queue_t* k, uint32_t c, ifcs_bam_queue_stats_id_t* ids)
{
    (void)n;
    int q = qix(k);
    if (q < 0) return IFCS_PARAM;
    for (uint32_t i = 0; i < c; i++) if (ids[i] >= 0 && ids[i] < 8) bstat[q][ids[i]] = 0;
    mock_logf("ifcs_bam_queue_stats_clear dp=%u q=%u n=%u", k->devport, k->queue_id, c);
    return IFCS_SUCCESS;
}

static void log_curve(const char* fn, ifcs_handle_t h, uint32_t c, const ifcs_attr_t* a)
{
    char b[512];
    int m = snprintf(b, sizeof(b), "%s h=0x%x", fn, h);
    for (uint32_t i = 0; i < c; i++) m += snprintf(b + m, sizeof(b) - (size_t)m, " %u=%u", a[i].id, a[i].value.u32);
    mock_logf("%s", b);
}
ifcs_status_t ifcs_aqm_curve_group_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (c != 9) return IFCS_PARAM;
    *h = next_handle++;
    log_curve("ifcs_aqm_curve_group_create", *h, c, a);
    curves++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_aqm_curve_group_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    log_curve("ifcs_aqm_curve_group_attr_set", h, c, a);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_aqm_curve_group_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_aqm_curve_group_delete h=0x%x", h); curves--; return IFCS_SUCCESS; }
