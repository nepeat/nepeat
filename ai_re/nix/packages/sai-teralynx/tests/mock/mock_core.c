/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: call log, init / node / attr helpers, log hook (mock.h). */
#include "mock.h"

static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static char* logbuf;
static size_t loglen, logcap;

void mock_logf(const char* fmt, ...)
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

ifcs_status_t mock_create_obj(const char* what, ifcs_handle_t* h, uint32_t c)
{
    if (*h == IFCS_NULL_HANDLE) *h = next_handle++;
    mock_logf("%s h=0x%x count=%u", what, *h, c);
    return IFCS_SUCCESS;
}

/* ------------------------------------------------------------- state */
int g_inited, g_node, g_enabled;
int g_mlr;   /* MULTICAST_LAG_RESOLUTION_ENABLE: only before ENABLE (mock_l2.c checks it for LAG mc_dests) */
uint32_t next_handle = 0x100;
static uint32_t g_aging;
static int temp_reads;
static float g_temp[5] = {55.5f, 54.0f, 53.5f, -300.0f, 52.0f};
static ifcs_log_notify_t log_cb;
static void* log_user;
static const char* names[] = {"SUCCESS", "UNINIT", "BUSY", "CONFIG", "MEMORY", "PARAM", "EXIST", "MULTI_EXIST",
                              "ENTRY_REPLACED", "LENGTH_MISMATCH", "NOTFOUND", "INVAL", "UNSUPPORTED",
                              "RESOURCE_FULL", "TIMEOUT", "DEVICE_ACCESS_ERROR", "DRIVER_ERROR"};

/* ---------------------------------------------------------- test hooks */
const char* mock_ifcs_log(void) { return logbuf ? logbuf : ""; }
void mock_ifcs_log_clear(void) { pthread_mutex_lock(&mu); loglen = 0; if (logbuf) logbuf[0] = 0; pthread_mutex_unlock(&mu); }
void mock_set_temp(int i, float c) { if (i >= 0 && i < 5) g_temp[i] = c; }
int mock_inited(void) { return g_inited; }
int mock_temp_reads(void) { return temp_reads; }
uint32_t mock_aging(void) { return g_aging; }
void mock_log_message(int level, const char* m) { if (log_cb) log_cb(level, log_user, m); }

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
    mock_logf("ifcs_init cfg=%s log=%s boot=%d first=%s", cfg ? cfg : "(null)", log_name ? log_name : "(null)", boot, first);
    if (!f) return IFCS_CONFIG;
    g_inited = 1;
    *num_nodes = 1;
    mock_port_reset();
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_deinit(int t) { mock_logf("ifcs_deinit type=%d", t); g_inited = 0; g_node = 0; g_enabled = 0; g_mlr = 0; return IFCS_SUCCESS; }
ifcs_status_t ifcs_node_create(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a)
{
    mock_logf("ifcs_node_create node=%u count=%u attrs=%s", n, c, a ? "ptr" : "null");
    if (!g_inited) return IFCS_UNINIT;
    g_node = 1;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_node_attr_set(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a)
{
    for (uint32_t i = 0; i < c; i++) {
        mock_logf("ifcs_node_attr_set node=%u count=%u id=%u u32=%u", n, c, a[i].id, a[i].value.u32);
        if (a[i].id == IFCS_NODE_ATTR_ENABLE) g_enabled = a[i].value.data;
        if (a[i].id == IFCS_NODE_ATTR_L2_ENTRY_AGING_INTERVAL) g_aging = a[i].value.u32;
        if (a[i].id == IFCS_NODE_ATTR_MULTICAST_LAG_RESOLUTION_ENABLE) {
            if (g_enabled) return IFCS_UNSUPPORTED;
            g_mlr = a[i].value.data;
        }
    }
    return g_node ? IFCS_SUCCESS : IFCS_UNINIT;
}
ifcs_status_t ifcs_node_attr_get(ifcs_node_id_t n, uint32_t c, ifcs_attr_t* a, uint32_t* got)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++) {
        uint32_t id = a[i].id;
        if (id >= IFCS_NODE_ATTR_MASTER_LAST_TEMP && id <= IFCS_NODE_ATTR_REMOTE3_LAST_TEMP) {
            a[i].value.flt = g_temp[id - IFCS_NODE_ATTR_MASTER_LAST_TEMP];
            temp_reads++;
        }
        else if (id == IFCS_NODE_ATTR_ACL_ENTRY_MINIMUM_PRIORITY) a[i].value.u32 = 1;
        else if (id == IFCS_NODE_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY) a[i].value.u32 = 4096;
        else if (id == IFCS_NODE_ATTR_NUMBER_OF_ECMP_GROUPS) a[i].value.u32 = 512;
        else return IFCS_UNSUPPORTED;
    }
    if (got) *got = c;
    return g_enabled ? IFCS_SUCCESS : IFCS_UNINIT;
}
ifcs_status_t ifcs_attr_t_init(ifcs_attr_t* a) { mock_logf("ifcs_attr_t_init"); memset(a, 0, sizeof(*a)); return IFCS_SUCCESS; }
ifcs_status_t ifcs_attr_t_id_set(ifcs_attr_t* a, uint32_t id) { mock_logf("ifcs_attr_t_id_set id=%u", id); a->id = id; return IFCS_SUCCESS; }
ifcs_status_t ifcs_attr_t_value_data_set(ifcs_attr_t* a, ifcs_bool_t v)
{
    mock_logf("ifcs_attr_t_value_data_set %d", v);
    a->value.data = v;
    a->bitmap |= 1ull << 21;
    return IFCS_SUCCESS;
}
const char* ifcs_status_to_string(ifcs_status_t rc)
{
    int r = IFCS_STATUS_REASON(rc);
    return r <= 16 ? names[r] : "?";
}
ifcs_status_t ifcs_log_function_register(ifcs_log_notify_t fn, void* u) { mock_logf("ifcs_log_function_register"); log_cb = fn; log_user = u; return IFCS_SUCCESS; }


/* Every symbol the backend loads is defined somewhere in the mock. */
#define MOCK_REF(n) (void*)&n,
void* const mock_all_symbols[] = {IFCS_SYMBOLS(MOCK_REF)};
