/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs.so for the sai-teralynx tests: exports every symbol the
 * backend dlsyms (IFCS_SYMBOLS in src/ifcs_api.h, signatures checked at
 * compile time), records the calls as text lines and keeps a little state.
 * Test hooks (mock_*) are reached with dlsym on the same handle. Nothing here
 * models the real SDK beyond what the tests assert. */
#include "ifcs_api.h"

#include <pthread.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_DP 260
#define MAX_OBJ 4096

static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static char* logbuf;
static size_t loglen, logcap;

static void logf_(const char* fmt, ...) __attribute__((format(printf, 1, 2)));
static void logf_(const char* fmt, ...)
{
    char b[2048];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof(b) - 1, fmt, ap);
    va_end(ap);
    if (n < 0) return;
    if ((size_t)n > sizeof(b) - 2) n = (int)sizeof(b) - 2;
    b[n++] = '\n';
    pthread_mutex_lock(&mu);
    if (loglen + (size_t)n + 1 > logcap) {
        logcap = (loglen + (size_t)n + 1) * 2;
        logbuf = realloc(logbuf, logcap);
    }
    memcpy(logbuf + loglen, b, (size_t)n);
    loglen += (size_t)n;
    logbuf[loglen] = 0;
    pthread_mutex_unlock(&mu);
}

/* ------------------------------------------------------------- state */
static int g_inited, g_node, g_enabled;
static uint32_t g_aging;
static float g_temp[5] = {55.5f, 54.0f, 53.5f, -300.0f, 52.0f};
static struct { uint32_t admin, link, speed, fec, lb, an; uint64_t st[IFCS_DEVPORT_STATS_ID_MAX_COUNT]; } dp[MAX_DP];
static ifcs_linkscan_event_notify_fn_t link_cb;
static void* link_user;
static ifcs_l2_event_notify_fn_t l2_cb;
static void* l2_user;
static ifcs_log_notify_t log_cb;
static void* log_user;
static uint32_t next_handle = 0x100;
static struct { int used; ifcs_l2_entry_key_t key; ifcs_handle_t dest; uint32_t type; } l2[MAX_OBJ];
static struct { int used; char key[96]; ifcs_handle_t nh; int fwd, ctc; ifcs_handle_t trap; } rt[MAX_OBJ];
static int l2vni_exists[4096];
static int fail_next_route; /* test hook */

static ifcs_handle_t sp_of(uint32_t d) { return d == 0 ? IFCS_HANDLE_SYSPORT(1000) : IFCS_HANDLE_SYSPORT(d); }
static int bad_dp(ifcs_devport_t d) { return d >= MAX_DP; }

/* ---------------------------------------------------------- test hooks */
const char* mock_ifcs_log(void) { return logbuf ? logbuf : ""; }
void mock_ifcs_log_clear(void) { pthread_mutex_lock(&mu); loglen = 0; if (logbuf) logbuf[0] = 0; pthread_mutex_unlock(&mu); }
void mock_set_link(uint32_t d, int up) { if (d < MAX_DP) dp[d].link = up ? 1 : 0; }
void mock_set_speed(uint32_t d, uint32_t e) { if (d < MAX_DP) dp[d].speed = e; }
void mock_set_stat(uint32_t d, uint32_t id, uint64_t v) { if (d < MAX_DP && id < IFCS_DEVPORT_STATS_ID_MAX_COUNT) dp[d].st[id] = v; }
void mock_set_temp(int i, float c) { if (i >= 0 && i < 5) g_temp[i] = c; }
uint32_t mock_admin(uint32_t d) { return d < MAX_DP ? dp[d].admin : 99; }
uint32_t mock_fec(uint32_t d) { return d < MAX_DP ? dp[d].fec : 99; }
int mock_inited(void) { return g_inited; }
uint32_t mock_aging(void) { return g_aging; }
void mock_fail_next_route(void) { fail_next_route = 1; }
int mock_fire_link(uint32_t d, int up)
{
    if (!link_cb) return -1;
    mock_set_link(d, up);
    ifcs_linkscan_event_info_t e;
    memset(&e, 0, sizeof(e));
    e.devport_id = d;
    e.port_state = up ? IFCS_DEVPORT_LINK_STATUS_UP : IFCS_DEVPORT_LINK_STATUS_DOWN;
    link_cb(0, link_user, &e);
    return 0;
}
int mock_fire_l2(int type, const uint8_t mac[6], uint16_t vid, uint32_t d)
{
    if (!l2_cb) return -1;
    ifcs_l2_event_info_t e;
    ifcs_attr_t a;
    memset(&e, 0, sizeof(e));
    memset(&a, 0, sizeof(a));
    e.event_type = type;
    e.count = 1;
    e.l2_entry.key_type = IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI;
    memcpy(e.l2_entry.key.mac_l2vni.mac_addr, mac, 6);
    e.l2_entry.key.mac_l2vni.l2vni = IFCS_HANDLE_L2VNI(vid);
    a.id = IFCS_L2_ENTRY_ATTR_ENTRY_DEST;
    a.value.handle = sp_of(d);
    e.attr_count = 1;
    e.attr_p = &a;
    l2_cb(0, l2_user, &e);
    return 0;
}
int mock_l2_count(void) { int n = 0; for (int i = 0; i < MAX_OBJ; i++) n += l2[i].used; return n; }
int mock_route_count(void) { int n = 0; for (int i = 0; i < MAX_OBJ; i++) n += rt[i].used; return n; }
/* Route by key text "<l3vni hex>/<family>/<addr hex>/<mask hex>": fwd, ctc, nexthop. */
int mock_route_find(const char* key, int* fwd, int* ctc, uint32_t* nh)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (rt[i].used && strcmp(rt[i].key, key) == 0) {
            *fwd = rt[i].fwd;
            *ctc = rt[i].ctc;
            *nh = rt[i].nh;
            return 1;
        }
    return 0;
}
void mock_log_message(int level, const char* m) { if (log_cb) log_cb(level, log_user, m); }

/* ------------------------------------------------------------- helpers */
static void attr_text(const ifcs_attr_t* a, char* b, size_t n)
{
    snprintf(b, n, "id=%u u32=%u", a->id, a->value.u32);
}
static const char* names[] = {"SUCCESS", "UNINIT", "BUSY", "CONFIG", "MEMORY", "PARAM", "EXIST", "MULTI_EXIST",
                              "ENTRY_REPLACED", "LENGTH_MISMATCH", "NOTFOUND", "INVAL", "UNSUPPORTED",
                              "RESOURCE_FULL", "TIMEOUT", "DEVICE_ACCESS_ERROR", "DRIVER_ERROR"};
static void route_key_text(const ifcs_route_entry_key_t* k, char* b, size_t n)
{
    const ifcs_route_entry_key_ip_dest_l3vni_t* r = &k->key.ip_dest_l3vni;
    if (r->ip_dest.addr_family == IFCS_IP_ADDR_FAMILY_IPV4)
        snprintf(b, n, "%x/4/%08x/%08x", r->l3vni, r->ip_dest.addr.ipv4, r->ip_dest.mask.ipv4);
    else {
        char a[33], m[33];
        for (int i = 0; i < 16; i++) { sprintf(a + 2 * i, "%02x", r->ip_dest.addr.ipv6[i]); sprintf(m + 2 * i, "%02x", r->ip_dest.mask.ipv6[i]); }
        snprintf(b, n, "%x/6/%s/%s", r->l3vni, a, m);
    }
}

/* ---------------------------------------------------------------- core */
ifcs_status_t ifcs_init(const char* cfg, const char* log_name, int boot, uint32_t* num_nodes)
{
    FILE* f = fopen(cfg, "r");
    char first[128] = "";
    if (f) {
        if (!fgets(first, sizeof(first), f)) first[0] = 0;
        fclose(f);
    }
    first[strcspn(first, "\n")] = 0;
    logf_("ifcs_init cfg=%s log=%s boot=%d first=%s", cfg ? cfg : "(null)", log_name ? log_name : "(null)", boot, first);
    if (!f) return IFCS_CONFIG;
    g_inited = 1;
    *num_nodes = 1;
    for (int i = 0; i < MAX_DP; i++) { dp[i].admin = 1; dp[i].speed = IFCS_DEVPORT_SPEED_400G; dp[i].fec = IFCS_DEVPORT_FEC_MODE_KP; }
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_deinit(int t) { logf_("ifcs_deinit type=%d", t); g_inited = 0; g_node = 0; g_enabled = 0; return IFCS_SUCCESS; }
ifcs_status_t ifcs_node_create(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a)
{
    logf_("ifcs_node_create node=%u count=%u attrs=%s", n, c, a ? "ptr" : "null");
    if (!g_inited) return IFCS_UNINIT;
    g_node = 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_node_attr_set(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a)
{
    for (uint32_t i = 0; i < c; i++) {
        logf_("ifcs_node_attr_set node=%u count=%u id=%u u32=%u", n, c, a[i].id, a[i].value.u32);
        if (a[i].id == IFCS_NODE_ATTR_ENABLE) g_enabled = a[i].value.data;
        if (a[i].id == IFCS_NODE_ATTR_L2_ENTRY_AGING_INTERVAL) g_aging = a[i].value.u32;
    }
    return g_node ? IFCS_SUCCESS : IFCS_UNINIT;
}
ifcs_status_t ifcs_node_attr_get(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a, uint32_t* got)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++) {
        uint32_t id = a[i].id;
        if (id >= IFCS_NODE_ATTR_MASTER_LAST_TEMP && id <= IFCS_NODE_ATTR_REMOTE3_LAST_TEMP)
            a[i].value.flt = g_temp[id - IFCS_NODE_ATTR_MASTER_LAST_TEMP];
        else if (id == IFCS_NODE_ATTR_ACL_ENTRY_MINIMUM_PRIORITY) a[i].value.u32 = 1;
        else if (id == IFCS_NODE_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY) a[i].value.u32 = 4096;
        else if (id == IFCS_NODE_ATTR_NUMBER_OF_ECMP_GROUPS) a[i].value.u32 = 512;
        else return IFCS_UNSUPPORTED;
    }
    if (got) *got = c;
    return g_enabled ? IFCS_SUCCESS : IFCS_UNINIT;
}
ifcs_status_t ifcs_attr_t_init(ifcs_attr_t* a) { logf_("ifcs_attr_t_init"); memset(a, 0, sizeof(*a)); return IFCS_SUCCESS; }
ifcs_status_t ifcs_attr_t_id_set(ifcs_attr_t* a, uint32_t id) { logf_("ifcs_attr_t_id_set id=%u", id); a->id = id; return IFCS_SUCCESS; }
ifcs_status_t ifcs_attr_t_value_data_set(ifcs_attr_t* a, ifcs_bool_t v)
{
    logf_("ifcs_attr_t_value_data_set %d", v);
    a->value.data = v;
    a->bitmap |= 1ull << 21;
    return IFCS_SUCCESS;
}
const char* ifcs_status_to_string(ifcs_status_t rc)
{
    int r = IFCS_STATUS_REASON(rc);
    return r <= 16 ? names[r] : "?";
}
ifcs_status_t ifcs_log_function_register(ifcs_log_notify_t fn, void* u) { logf_("ifcs_log_function_register"); log_cb = fn; log_user = u; return IFCS_SUCCESS; }

/* -------------------------------------------------------------- devport */
ifcs_status_t ifcs_devport_attr_set(ifcs_node_id_t n, ifcs_devport_t d, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (bad_dp(d)) return IFCS_PARAM;
    for (uint32_t i = 0; i < c; i++) {
        char b[512] = "";
        switch (a[i].id) {
        case IFCS_DEVPORT_ATTR_TX_EQ_PRE1: case IFCS_DEVPORT_ATTR_TX_EQ_PRE2: case IFCS_DEVPORT_ATTR_TX_EQ_PRE3:
        case IFCS_DEVPORT_ATTR_TX_EQ_ATTN: case IFCS_DEVPORT_ATTR_TX_EQ_POST: {
            size_t o = (size_t)snprintf(b, sizeof(b), "id=%u i8=[", a[i].id);
            for (uint32_t j = 0; j < a[i].value.i8_list.count && o < sizeof(b) - 8; j++)
                o += (size_t)snprintf(b + o, sizeof(b) - o, "%s%d", j ? "," : "", a[i].value.i8_list.arr[j]);
            snprintf(b + o, sizeof(b) - o, "]");
            break;
        }
        case IFCS_DEVPORT_ATTR_RX_CTLE_LF: case IFCS_DEVPORT_ATTR_RX_CTLE_HF: case IFCS_DEVPORT_ATTR_RX_CTLE_BW: {
            size_t o = (size_t)snprintf(b, sizeof(b), "id=%u u8=[", a[i].id);
            for (uint32_t j = 0; j < a[i].value.u8_list.count && o < sizeof(b) - 8; j++)
                o += (size_t)snprintf(b + o, sizeof(b) - o, "%s%u", j ? "," : "", a[i].value.u8_list.arr[j]);
            snprintf(b + o, sizeof(b) - o, "]");
            break;
        }
        case IFCS_DEVPORT_ATTR_RX_GAINSHAPE1: case IFCS_DEVPORT_ATTR_RX_GAINSHAPE2: case IFCS_DEVPORT_ATTR_RX_AGC_TARGET:
        case IFCS_DEVPORT_ATTR_RX_EYE_DISQUALIFY_THRESHOLD_25G: {
            size_t o = (size_t)snprintf(b, sizeof(b), "id=%u u32l=[", a[i].id);
            for (uint32_t j = 0; j < a[i].value.u32_list.count && o < sizeof(b) - 12; j++)
                o += (size_t)snprintf(b + o, sizeof(b) - o, "%s%u", j ? "," : "", a[i].value.u32_list.arr[j]);
            snprintf(b + o, sizeof(b) - o, "]");
            break;
        }
        default:
            attr_text(&a[i], b, sizeof(b));
        }
        logf_("ifcs_devport_attr_set dp=%u %s", d, b);
        switch (a[i].id) {
        case IFCS_DEVPORT_ATTR_ADMIN_STATE: dp[d].admin = a[i].value.u32; if (!dp[d].admin) dp[d].link = 0; break;
        case IFCS_DEVPORT_ATTR_FEC_MODE: dp[d].fec = a[i].value.u32; break;
        case IFCS_DEVPORT_ATTR_LOOPBACK: dp[d].lb = a[i].value.u32; break;
        case IFCS_DEVPORT_ATTR_AUTO_NEG: dp[d].an = a[i].value.u32; break;
        default: break;
        }
    }
    return g_enabled ? IFCS_SUCCESS : IFCS_UNINIT;
}
ifcs_status_t ifcs_devport_attr_get(ifcs_node_id_t n, ifcs_devport_t d, uint32_t c, ifcs_attr_t* a, uint32_t* got)
{
    (void)n;
    if (bad_dp(d) || d > 256) return IFCS_NOTFOUND;
    for (uint32_t i = 0; i < c; i++) {
        switch (a[i].id) {
        case IFCS_DEVPORT_ATTR_SYSPORT: a[i].value.handle = sp_of(d); break;
        case IFCS_DEVPORT_ATTR_LINK_STATUS: a[i].value.u32 = dp[d].link; break;
        case IFCS_DEVPORT_ATTR_SPEED: a[i].value.u32 = dp[d].speed; break;
        case IFCS_DEVPORT_ATTR_FEC_MODE: a[i].value.u32 = dp[d].fec; break;
        case IFCS_DEVPORT_ATTR_ADMIN_STATE: a[i].value.u32 = dp[d].admin; break;
        case IFCS_DEVPORT_ATTR_AUTO_NEG: a[i].value.u32 = dp[d].an; break;
        case IFCS_DEVPORT_ATTR_LOOPBACK: a[i].value.u32 = dp[d].lb; break;
        default: return IFCS_UNSUPPORTED;
        }
    }
    if (got) *got = c;
    return g_enabled ? IFCS_SUCCESS : IFCS_UNINIT;
}
ifcs_status_t ifcs_devport_stats_get(ifcs_node_id_t n, ifcs_devport_t d, uint32_t c, int* ids, uint64_t* v, uint32_t* got)
{
    (void)n;
    logf_("ifcs_devport_stats_get dp=%u count=%u", d, c);
    if (bad_dp(d)) return IFCS_PARAM;
    if (c == IFCS_DEVPORT_STATS_ID_ALL) {       /* the vendor CLI's form: all counters, by id */
        for (int i = 0; i < IFCS_DEVPORT_STATS_ID_MAX_COUNT; i++) v[i] = dp[d].st[i];
        if (got) *got = IFCS_DEVPORT_STATS_ID_MAX_COUNT;
    } else {
        for (uint32_t i = 0; i < c; i++) v[i] = ids[i] >= 0 && ids[i] < IFCS_DEVPORT_STATS_ID_MAX_COUNT ? dp[d].st[ids[i]] : 0;
        if (got) *got = c;
    }
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_devport_stats_clear(ifcs_node_id_t n, ifcs_devport_t d, uint32_t c, int* ids)
{
    (void)n;
    logf_("ifcs_devport_stats_clear dp=%u count=%u", d, c);
    if (bad_dp(d)) return IFCS_PARAM;
    for (uint32_t i = 0; i < c; i++)
        if (ids[i] >= 0 && ids[i] < IFCS_DEVPORT_STATS_ID_MAX_COUNT) dp[d].st[ids[i]] = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_linkscan_event_register_notify(ifcs_node_id_t n, void* u, ifcs_linkscan_event_notify_fn_t f)
{
    logf_("ifcs_linkscan_event_register_notify node=%u", n);
    link_cb = f;
    link_user = u;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_linkscan_event_unregister_notify(ifcs_node_id_t n) { logf_("ifcs_linkscan_event_unregister_notify node=%u", n); link_cb = NULL; return IFCS_SUCCESS; }
ifcs_status_t ifcs_sysport_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++) logf_("ifcs_sysport_attr_set sp=0x%x id=%u u32=%u", h, a[i].id, a[i].value.u32);
    return IFCS_SUCCESS;
}

/* ------------------------------------------------------------------- L2 */
static ifcs_status_t create_obj(const char* what, ifcs_handle_t* h, uint32_t c)
{
    if (*h == IFCS_NULL_HANDLE) *h = next_handle++;
    logf_("%s h=0x%x count=%u", what, *h, c);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_stp_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a) { (void)n; (void)a; return create_obj("ifcs_stp_create", h, c); }
ifcs_status_t ifcs_stp_port_add(ifcs_node_id_t n, ifcs_handle_t s, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)l;
    logf_("ifcs_stp_port_add stp=0x%x members=%u attrs=%u state=%u", s, m, c, c ? a[0].value.u32 : 99);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_mc_dest_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (*h == IFCS_NULL_HANDLE) *h = next_handle++;
    logf_("ifcs_mc_dest_create h=0x%x count=%u dest=0x%x", *h, c, c ? a[0].value.handle : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_mc_dest_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; logf_("ifcs_mc_dest_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_mdg_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a) { (void)n; (void)a; return create_obj("ifcs_mdg_create", h, c); }
ifcs_status_t ifcs_mdg_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; logf_("ifcs_mdg_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_mdg_mc_dest_add(ifcs_node_id_t n, ifcs_handle_t g, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)c; (void)a;
    logf_("ifcs_mdg_mc_dest_add mdg=0x%x n=%u first=0x%x", g, m, m ? l[0] : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_mdg_mc_dest_remove(ifcs_node_id_t n, ifcs_handle_t g, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    logf_("ifcs_mdg_mc_dest_remove mdg=0x%x n=%u first=0x%x", g, m, m ? l[0] : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    uint32_t v = IFCS_HANDLE_VALUE(*h);
    logf_("ifcs_l2vni_create vni=%u count=%u stp=0x%x flood=0x%x", v, c, c > 0 ? a[0].value.handle : 0, c > 1 ? a[1].value.handle : 0);
    if (v >= 4096) return IFCS_PARAM;
    if (l2vni_exists[v]) return IFCS_EXIST;
    l2vni_exists[v] = 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_delete(ifcs_node_id_t n, ifcs_handle_t h)
{
    (void)n;
    uint32_t v = IFCS_HANDLE_VALUE(h);
    logf_("ifcs_l2vni_delete vni=%u", v);
    if (v >= 4096 || !l2vni_exists[v]) return IFCS_NOTFOUND;
    l2vni_exists[v] = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)a;
    logf_("ifcs_l2vni_attr_set vni=%u count=%u", IFCS_HANDLE_VALUE(h), c);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2vni_member_add(ifcs_node_id_t n, ifcs_handle_t h, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)c; (void)a;
    logf_("ifcs_l2vni_member_add vni=%u n=%u first=0x%x", IFCS_HANDLE_VALUE(h), m, m ? l[0] : 0);
    return l2vni_exists[IFCS_HANDLE_VALUE(h) & 4095] ? IFCS_SUCCESS : IFCS_NOTFOUND;
}
ifcs_status_t ifcs_l2vni_member_remove(ifcs_node_id_t n, ifcs_handle_t h, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    logf_("ifcs_l2vni_member_remove vni=%u n=%u first=0x%x", IFCS_HANDLE_VALUE(h), m, m ? l[0] : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_key_t_init(ifcs_l2_entry_key_t* k) { memset(k, 0, sizeof(*k)); k->_base_key = 0xb45e; return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_t_key_type_set(ifcs_l2_entry_key_t* k, int t) { k->key_type = t; k->dirty |= 1; return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_t_mac_l2vni_set(ifcs_l2_entry_key_t* k, ifcs_l2_entry_key_mac_l2vni_t v) { k->key.mac_l2vni = v; k->dirty |= 4; return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(ifcs_l2_entry_key_mac_l2vni_t* k, ifcs_mac_t* m) { memcpy(k->mac_addr, *m, 6); return IFCS_SUCCESS; }
ifcs_status_t ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(ifcs_l2_entry_key_mac_l2vni_t* k, ifcs_handle_t h) { k->l2vni = h; return IFCS_SUCCESS; }
static int l2_find(const ifcs_l2_entry_key_t* k)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (l2[i].used && memcmp(l2[i].key.key.mac_l2vni.mac_addr, k->key.mac_l2vni.mac_addr, 6) == 0 &&
            l2[i].key.key.mac_l2vni.l2vni == k->key.mac_l2vni.l2vni)
            return i;
    return -1;
}
ifcs_status_t ifcs_l2_entry_create(ifcs_node_id_t n, ifcs_l2_entry_key_t* k, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    const uint8_t* m = k->key.mac_l2vni.mac_addr;
    logf_("ifcs_l2_entry_create base=%x type=%d mac=%02x:%02x:%02x:%02x:%02x:%02x vni=%u count=%u dest=0x%x etype=%u",
          k->_base_key, k->key_type, m[0], m[1], m[2], m[3], m[4], m[5], IFCS_HANDLE_VALUE(k->key.mac_l2vni.l2vni), c,
          c > 0 ? a[0].value.handle : 0, c > 1 ? a[1].value.u32 : 99);
    if (k->_base_key != 0xb45e) return IFCS_PARAM;   /* key not built through the accessors */
    if (l2_find(k) >= 0) return IFCS_EXIST;
    for (int i = 0; i < MAX_OBJ; i++)
        if (!l2[i].used) {
            l2[i].used = 1;
            l2[i].key = *k;
            l2[i].dest = c > 0 ? a[0].value.handle : 0;
            l2[i].type = c > 1 ? a[1].value.u32 : 0;
            return IFCS_SUCCESS;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_l2_entry_delete(ifcs_node_id_t n, ifcs_l2_entry_key_t* k)
{
    (void)n;
    int i = l2_find(k);
    logf_("ifcs_l2_entry_delete vni=%u found=%d", IFCS_HANDLE_VALUE(k->key.mac_l2vni.l2vni), i >= 0);
    if (i < 0) return IFCS_NOTFOUND;
    l2[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_get_all(ifcs_node_id_t n, ifcs_l2_entry_key_t* k, uint32_t c, ifcs_attr_t* a,
                                    ifcs_l2_entry_user_cb_t cb, void* u, uint32_t* cnt)
{
    (void)k; (void)c; (void)a;
    logf_("ifcs_l2_entry_get_all");
    uint32_t x = 0;
    for (int i = 0; i < MAX_OBJ; i++)
        if (l2[i].used) {
            ifcs_l2_entry_key_t key = l2[i].key;
            cb(n, &key, 0, NULL, u);
            x++;
        }
    if (cnt) *cnt = x;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_attr_get(ifcs_node_id_t n, ifcs_l2_entry_key_t* k, uint32_t c, ifcs_attr_t* a, uint32_t* got)
{
    (void)n;
    int i = l2_find(k);
    if (i < 0) return IFCS_NOTFOUND;
    for (uint32_t j = 0; j < c; j++) {
        if (a[j].id == IFCS_L2_ENTRY_ATTR_ENTRY_TYPE) a[j].value.u32 = l2[i].type;
        else if (a[j].id == IFCS_L2_ENTRY_ATTR_ENTRY_DEST) a[j].value.handle = l2[i].dest;
    }
    if (got) *got = c;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_entry_usage_get(ifcs_node_id_t n, ifcs_l2_entry_key_t* k, uint32_t c, ifcs_attr_t* a, ifcs_usage_t* us)
{
    (void)n; (void)k; (void)c; (void)a;
    us->max = 45056;
    us->current = (uint32_t)mock_l2_count();
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_event_register_notify(ifcs_node_id_t n, void* u, ifcs_l2_event_notify_fn_t f)
{
    logf_("ifcs_l2_event_register_notify node=%u", n);
    l2_cb = f;
    l2_user = u;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_l2_event_unregister_notify(ifcs_node_id_t n) { logf_("ifcs_l2_event_unregister_notify node=%u", n); l2_cb = NULL; return IFCS_SUCCESS; }

/* --------------------------------------------------------------- netdev */
ifcs_status_t ifcs_netdev_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    char name[32] = "";
    uint32_t devport = 0, tag = 99;
    const uint8_t* mac = NULL;
    for (uint32_t i = 0; i < c; i++) {
        if (a[i].id == IFCS_NETDEV_ATTR_INTF_NAME && a[i].value.string.count < sizeof(name)) {
            memcpy(name, a[i].value.string.arr, a[i].value.string.count);
            name[a[i].value.string.count] = 0;
        }
        if (a[i].id == IFCS_NETDEV_ATTR_DEVPORTNUM) devport = a[i].value.devport;
        if (a[i].id == IFCS_NETDEV_ATTR_MACADDR) mac = a[i].value.mac;
        if (a[i].id == IFCS_NETDEV_ATTR_VLAN_TAG) tag = a[i].value.u32;
    }
    *h = next_handle++;
    logf_("ifcs_netdev_create h=0x%x name=%s devport=%u mac=%02x:..:%02x tag=%u", *h, name, devport, mac ? mac[0] : 0,
          mac ? mac[5] : 0, tag);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_netdev_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; logf_("ifcs_netdev_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_netdev_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    logf_("ifcs_netdev_attr_set h=0x%x count=%u id=%u u32=%u", h, c, a[0].id, a[0].value.u32);
    return IFCS_SUCCESS;
}

/* ------------------------------------------------------------------- L3 */
ifcs_status_t ifcs_l3vni_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)a;
    logf_("ifcs_l3vni_create h=0x%x count=%u", *h, c);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_intf_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    logf_("ifcs_intf_create h=0x%x count=%u type=%u fwd=0x%x transport=0x%x", *h, c, a[0].value.u32, a[1].value.handle,
          a[2].value.handle);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_intf_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; logf_("ifcs_intf_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_intf_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    logf_("ifcs_intf_attr_set h=0x%x count=%u id=%u", h, c, a[0].id);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_nexthop_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    logf_("ifcs_nexthop_create h=0x%x count=%u intf=0x%x dest=0x%x", *h, c, a[0].value.handle, a[2].value.handle);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_nexthop_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; logf_("ifcs_nexthop_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_nexthop_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    logf_("ifcs_nexthop_attr_set h=0x%x count=%u id=%u", h, c, a[0].id);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_nexthop_usage_get(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a, ifcs_usage_t* u)
{
    (void)n; (void)c; (void)a;
    u->max = 32768;
    u->current = 3;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_route_entry_key_t_init(ifcs_route_entry_key_t* k) { memset(k, 0, sizeof(*k)); k->_base_key = 0x5e7; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_t_key_type_set(ifcs_route_entry_key_t* k, int t) { k->key_type = t; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_t_ip_dest_l3vni_set(ifcs_route_entry_key_t* k, ifcs_route_entry_key_ip_dest_l3vni_t v) { k->key.ip_dest_l3vni = v; k->dirty |= 4; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(ifcs_route_entry_key_ip_dest_l3vni_t* k, ifcs_handle_t h) { k->l3vni = h; return IFCS_SUCCESS; }
ifcs_status_t ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(ifcs_route_entry_key_ip_dest_l3vni_t* k, ifcs_ip_prefix_t* p) { k->ip_dest = *p; return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_init(ifcs_ip_prefix_t* p) { memset(p, 0, sizeof(*p)); return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_addr_family_set(ifcs_ip_prefix_t* p, int f) { p->addr_family = f; return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_addr_set(ifcs_ip_prefix_t* p, ifcs_ip_addr_t* a) { p->addr = *a; return IFCS_SUCCESS; }
ifcs_status_t ifcs_ip_prefix_t_mask_set(ifcs_ip_prefix_t* p, ifcs_ip_addr_t* m) { p->mask = *m; return IFCS_SUCCESS; }
static int rt_find(const char* key)
{
    for (int i = 0; i < MAX_OBJ; i++)
        if (rt[i].used && strcmp(rt[i].key, key) == 0) return i;
    return -1;
}
static void rt_apply(int i, uint32_t c, const ifcs_attr_t* a)
{
    for (uint32_t j = 0; j < c; j++) {
        if (a[j].id == IFCS_ROUTE_ENTRY_ATTR_NEXTHOP) rt[i].nh = a[j].value.handle;
        if (a[j].id == IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY) rt[i].fwd = a[j].value.fwd_policy.fwd_action;
        if (a[j].id == IFCS_ROUTE_ENTRY_ATTR_CTC_POLICY) { rt[i].ctc = a[j].value.ctc_policy.ctc_action; rt[i].trap = a[j].value.ctc_policy.trap_handle; }
    }
}
ifcs_status_t ifcs_route_entry_create(ifcs_node_id_t n, ifcs_route_entry_key_t* k, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    char key[96];
    route_key_text(k, key, sizeof(key));
    logf_("ifcs_route_entry_create key=%s count=%u", key, c);
    if (fail_next_route) { fail_next_route = 0; return IFCS_RESOURCE_FULL; }
    if (k->_base_key != 0x5e7) return IFCS_PARAM;
    if (rt_find(key) >= 0) return IFCS_EXIST;
    for (int i = 0; i < MAX_OBJ; i++)
        if (!rt[i].used) {
            rt[i].used = 1;
            snprintf(rt[i].key, sizeof(rt[i].key), "%s", key);
            rt[i].nh = 0;
            rt[i].fwd = 0;
            rt[i].ctc = 0;
            rt_apply(i, c, a);
            return IFCS_SUCCESS;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_route_entry_delete(ifcs_node_id_t n, ifcs_route_entry_key_t* k)
{
    (void)n;
    char key[96];
    route_key_text(k, key, sizeof(key));
    int i = rt_find(key);
    logf_("ifcs_route_entry_delete key=%s found=%d", key, i >= 0);
    if (i < 0) return IFCS_NOTFOUND;
    rt[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_route_entry_attr_set(ifcs_node_id_t n, ifcs_route_entry_key_t* k, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    char key[96];
    route_key_text(k, key, sizeof(key));
    int i = rt_find(key);
    logf_("ifcs_route_entry_attr_set key=%s count=%u found=%d", key, c, i >= 0);
    if (i < 0) return IFCS_NOTFOUND;
    rt_apply(i, c, a);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_hostif_trap_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    *h = next_handle++;
    logf_("ifcs_hostif_trap_create h=0x%x count=%u queue=%u", *h, c, c ? a[0].value.u32 : 99);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a) { (void)n; (void)a; *h = IFCS_NULL_HANDLE; return create_obj("ifcs_ecmp_create", h, c); }
ifcs_status_t ifcs_ecmp_delete(ifcs_node_id_t n, ifcs_handle_t h) { (void)n; logf_("ifcs_ecmp_delete h=0x%x", h); return IFCS_SUCCESS; }
ifcs_status_t ifcs_ecmp_member_add(ifcs_node_id_t n, ifcs_handle_t e, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    logf_("ifcs_ecmp_member_add ecmp=0x%x n=%u first=0x%x weight=%u", e, m, m ? l[0] : 0, c ? a[0].value.u32 : 1);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_member_remove(ifcs_node_id_t n, ifcs_handle_t e, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    logf_("ifcs_ecmp_member_remove ecmp=0x%x n=%u first=0x%x", e, m, m ? l[0] : 0);
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_ecmp_usage_get(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a, ifcs_usage_t* u)
{
    (void)n; (void)c; (void)a;
    u->max = 512;
    u->current = 0;
    return IFCS_SUCCESS;
}

/* Every symbol the backend loads has exactly the declared type. */
#define MOCK_CHECK(n) static n##_fn const check_##n __attribute__((unused)) = n;
IFCS_SYMBOLS(MOCK_CHECK)
