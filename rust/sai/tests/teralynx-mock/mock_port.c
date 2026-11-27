/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: devports, counters, link scan, sysports (mock.h). */
#include "mock.h"

static struct { uint32_t admin, link, speed, fec, lb, an; uint64_t st[IFCS_DEVPORT_STATS_ID_MAX_COUNT]; } dp[MAX_DP];
static ifcs_linkscan_event_notify_fn_t link_cb;
static void* link_user;
static int link_reads;

/* ---------------------------------------------------------- test hooks */
void mock_set_link(uint32_t d, int up) { if (d < MAX_DP) dp[d].link = up ? 1 : 0; }
void mock_set_speed(uint32_t d, uint32_t e) { if (d < MAX_DP) dp[d].speed = e; }
void mock_set_stat(uint32_t d, uint32_t id, uint64_t v) { if (d < MAX_DP && id < IFCS_DEVPORT_STATS_ID_MAX_COUNT) dp[d].st[id] = v; }
int mock_link_reads(void) { return link_reads; }
uint32_t mock_admin(uint32_t d) { return d < MAX_DP ? dp[d].admin : 99; }
uint32_t mock_fec(uint32_t d) { return d < MAX_DP ? dp[d].fec : 99; }
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

void mock_port_reset(void)
{
    for (int i = 0; i < MAX_DP; i++) { dp[i].admin = 1; dp[i].speed = IFCS_DEVPORT_SPEED_400G; dp[i].fec = IFCS_DEVPORT_FEC_MODE_KP; }
}

static void attr_text(const ifcs_attr_t* a, char* b, size_t n)
{
    snprintf(b, n, "id=%u u32=%u", a->id, a->value.u32);
}

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
        mock_logf("ifcs_devport_attr_set dp=%u %s", d, b);
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
        case IFCS_DEVPORT_ATTR_LINK_STATUS: a[i].value.u32 = dp[d].link; link_reads++; break;
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
    mock_logf("ifcs_devport_stats_get dp=%u count=%u", d, c);
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
    mock_logf("ifcs_devport_stats_clear dp=%u count=%u", d, c);
    if (bad_dp(d)) return IFCS_PARAM;
    for (uint32_t i = 0; i < c; i++)
        if (ids[i] >= 0 && ids[i] < IFCS_DEVPORT_STATS_ID_MAX_COUNT) dp[d].st[ids[i]] = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_linkscan_event_register_notify(ifcs_node_id_t n, void* u, ifcs_linkscan_event_notify_fn_t f)
{
    mock_logf("ifcs_linkscan_event_register_notify node=%u", n);
    link_cb = f;
    link_user = u;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_linkscan_event_unregister_notify(ifcs_node_id_t n) { mock_logf("ifcs_linkscan_event_unregister_notify node=%u", n); link_cb = NULL; return IFCS_SUCCESS; }
ifcs_status_t ifcs_sysport_attr_set(ifcs_node_id_t n, ifcs_handle_t h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++) mock_logf("ifcs_sysport_attr_set sp=0x%x id=%u u32=%u", h, a[i].id, a[i].value.u32);
    return IFCS_SUCCESS;
}

