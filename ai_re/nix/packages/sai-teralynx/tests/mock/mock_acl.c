/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: ACL table / profiles / ACLs (6 inno blocks) / matches /
 * actions / meters / counters / ACEs (mock.h). Records one line per call. */
#include "mock.h"

#define MOCK_IBS 6
static int n_aces, n_meters, n_counters;
static uint64_t counter_packets = 0;   /* test hook: every counter reads this */

int mock_acl_aces(void) { return n_aces; }
int mock_acl_meters(void) { return n_meters; }
int mock_acl_counters(void) { return n_counters; }
void mock_acl_set_counter(uint64_t v) { counter_packets = v; }

static uint32_t find_u32(uint32_t c, const ifcs_attr_t* a, uint32_t id, uint32_t def)
{
    for (uint32_t i = 0; i < c; i++)
        if (a[i].id == id) return a[i].value.u32;
    return def;
}
static const ifcs_attr_t* find(uint32_t c, const ifcs_attr_t* a, uint32_t id)
{
    for (uint32_t i = 0; i < c; i++)
        if (a[i].id == id) return &a[i];
    return NULL;
}
static void hexs(char* b, size_t n, const uint8_t* p, uint32_t len)
{
    size_t o = 0;
    for (uint32_t i = 0; i < len && o + 3 < n; i++) o += (size_t)snprintf(b + o, n - o, "%02x", p[i]);
    if (!len && n) b[0] = 0;
}

ifcs_status_t ifcs_acl_match_profile_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    const ifcs_attr_t* t = find(c, a, IFCS_ACL_MATCH_PROFILE_ATTR_MATCH_TYPES);
    mock_logf("ifcs_acl_match_profile_create h=0x%x types=%u width=%u", *h, t ? t->value.u32_list.count : 0,
              find_u32(c, a, IFCS_ACL_MATCH_PROFILE_ATTR_MAX_WIDTH, 0));
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_match_profile_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_acl_match_profile_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_action_profile_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    const ifcs_attr_t* t = find(c, a, IFCS_ACL_ACTION_PROFILE_ATTR_ACTION_TYPES);
    mock_logf("ifcs_acl_action_profile_create h=0x%x types=%u", *h, t ? t->value.u32_list.count : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_action_profile_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_acl_action_profile_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_table_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    mock_logf("ifcs_acl_table_create h=0x%x scope=%u type=%u size=%u", *h, find_u32(c, a, IFCS_ACL_TABLE_ATTR_SCOPE, 9),
              find_u32(c, a, IFCS_ACL_TABLE_ATTR_TYPE, 9), find_u32(c, a, IFCS_ACL_TABLE_ATTR_SIZE, 0));
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_table_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_acl_table_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_table_acl_add(ifcs_node_id_t n, ifcs_handle_t t, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)l; (void)c; (void)a;
    mock_logf("ifcs_acl_table_acl_add table=0x%x n=%u", t, m);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_table_acl_remove(ifcs_node_id_t n, ifcs_handle_t t, uint32_t m, ifcs_handle_t* l)
{
    (void)n; (void)l;
    mock_logf("ifcs_acl_table_acl_remove table=0x%x n=%u", t, m);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    uint32_t ib = find_u32(c, a, IFCS_ACL_ATTR_IB, 0);
    if (ib >= MOCK_IBS) { mock_logf("ifcs_acl_create ib=%u refused", ib); return IFCS_PARAM; }
    *h = next_handle++;
    mock_logf("ifcs_acl_create h=0x%x scope=%u ib=%u", *h, find_u32(c, a, IFCS_ACL_ATTR_SCOPE, 9), ib);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_acl_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_match_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    char v[80] = "";
    const ifcs_attr_t* val = find(c, a, IFCS_ACL_MATCH_ATTR_VALUE);
    if (val) hexs(v, sizeof(v), val->value.u8_list.arr, val->value.u8_list.count);
    const ifcs_attr_t* obj = find(c, a, IFCS_ACL_MATCH_ATTR_OBJECT);
    mock_logf("ifcs_acl_match_create h=0x%x type=%u value=%s object=0x%x", *h, find_u32(c, a, IFCS_ACL_MATCH_ATTR_TYPE, 999), v,
              obj ? obj->value.handle : 0);
    return find(c, a, IFCS_ACL_MATCH_ATTR_MATCH_PROFILE) ? IFCS_SUCCESS : IFCS_PARAM;
}
ifcs_status_t ifcs_acl_match_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_acl_match_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_action_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    uint32_t type = find_u32(c, a, IFCS_ACL_ACTION_ATTR_TYPE, 999);
    const ifcs_attr_t* v = find(c, a, IFCS_ACL_ACTION_ATTR_VALUE);
    const ifcs_attr_t* o = find(c, a, IFCS_ACL_ACTION_ATTR_OBJECT);
    if (type == IFCS_ACL_ACTION_TYPE_COPY_TO_CPU && v) {
        ifcs_acl_ctc_action_t x;
        memcpy(&x, &v->value, sizeof(x));
        mock_logf("ifcs_acl_action_create h=0x%x type=ctc hostif=0x%x green=%u yellow=%u red=%u dirty=%x", *h, x.hostif, x.green,
                  x.yellow, x.red, x.dirty);
    } else if (type == IFCS_ACL_ACTION_TYPE_COPY_TO_CPU_OVERRIDE && v) {
        ifcs_acl_ctc_override_action_t x;
        memcpy(&x, &v->value, sizeof(x));
        mock_logf("ifcs_acl_action_create h=0x%x type=ctc_override green=%u yellow=%u red=%u", *h, x.green, x.yellow, x.red);
    } else if (type == IFCS_ACL_ACTION_TYPE_DROP && v) {
        ifcs_acl_drop_action_t x;
        memcpy(&x, &v->value, sizeof(x));
        mock_logf("ifcs_acl_action_create h=0x%x type=drop green=%u yellow=%u red=%u", *h, x.green, x.yellow, x.red);
    } else {
        mock_logf("ifcs_acl_action_create h=0x%x type=%u object=0x%x", *h, type, o ? o->value.handle : 0);
    }
    return find(c, a, IFCS_ACL_ACTION_ATTR_ACTION_PROFILE) ? IFCS_SUCCESS : IFCS_PARAM;
}
ifcs_status_t ifcs_acl_action_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_acl_action_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_meter_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    const ifcs_attr_t* cir = find(c, a, IFCS_ACL_METER_ATTR_COMMITTED_INFORMATION_RATE);
    const ifcs_attr_t* cbs = find(c, a, IFCS_ACL_METER_ATTR_COMMITTED_BURST_SIZE);
    mock_logf("ifcs_acl_meter_create h=0x%x ib=%u mode=%u cir=%llu cbs=%llu", *h, find_u32(c, a, IFCS_ACL_METER_ATTR_IB, 99),
              find_u32(c, a, IFCS_ACL_METER_ATTR_MODE, 99), cir ? (unsigned long long)cir->value.u64 : 0,
              cbs ? (unsigned long long)cbs->value.u64 : 0);
    n_meters++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_meter_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_acl_meter_delete h=0x%x", h); n_meters--; return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_meter_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++)
        mock_logf("ifcs_acl_meter_attr_set h=0x%x id=%u u64=%llu", h, a[i].id, (unsigned long long)a[i].value.u64);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_counter_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    mock_logf("ifcs_acl_counter_create h=0x%x ib=%u mode=%u", *h, find_u32(c, a, IFCS_ACL_COUNTER_ATTR_IB, 99),
              find_u32(c, a, IFCS_ACL_COUNTER_ATTR_MODE, 99));
    n_counters++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_counter_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_acl_counter_delete h=0x%x", h); n_counters--; return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_counter_stats_get(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_acl_counter_stats_id_t* ids,
                                         uint64_t* v, uint32_t* got)
{
    (void)n; (void)h;
    for (uint32_t i = 0; i < c; i++) v[i] = ids[i] == IFCS_ACL_COUNTER_STATS_ID_RED_PACKETS ? counter_packets / 2 : counter_packets;
    if (got) *got = c;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ace_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    const ifcs_attr_t* m = find(c, a, IFCS_ACE_ATTR_MATCH_SET);
    const ifcs_attr_t* x = find(c, a, IFCS_ACE_ATTR_ACTION_SET);
    mock_logf("ifcs_ace_create h=0x%x ib=%u prio=%u matches=%u actions=%u", *h, find_u32(c, a, IFCS_ACE_ATTR_IB, 99),
              find_u32(c, a, IFCS_ACE_ATTR_PRIORITY, 0), m ? m->value.handle_list.count : 0, x ? x->value.handle_list.count : 0);
    n_aces++;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ace_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; mock_logf("ifcs_ace_delete h=0x%x", h); n_aces--; return IFCS_SUCCESS; }
ifcs_status_t ifcs_acl_ace_add(ifcs_node_id_t n, ifcs_handle_t acl, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)l; (void)c; (void)a;
    mock_logf("ifcs_acl_ace_add acl=0x%x n=%u", acl, m);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_acl_ace_remove(ifcs_node_id_t n, ifcs_handle_t acl, uint32_t m, ifcs_handle_t* l)
{
    (void)n; (void)l;
    mock_logf("ifcs_acl_ace_remove acl=0x%x n=%u", acl, m);
    return IFCS_SUCCESS;
}
