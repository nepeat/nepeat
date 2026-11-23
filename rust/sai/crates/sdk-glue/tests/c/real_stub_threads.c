/* SPDX-License-Identifier: Apache-2.0
 * The C side of sdk-glue's real-path test (tests/real_stub.rs, built by tests/real-stub.sh):
 * the recording + replaying stub of all 75 functions of sdk_glue.h (glue_stub.c, included, so its
 * static callback pointers are this file's too), plus calls into the registered link / L2 / gate pointers from threads this file creates,
 * the way the SDK's link scan and L2X threads make them: with the registered context, a stray
 * one, or pointers and context saved earlier (a late call after an unregistration), and any
 * argument values. The pointers are read under the stub's lock and called without it (as the
 * SDK does). Every entry point joins its threads before it returns. */
#include "glue_stub.c"

#include <stdint.h>

/* how: which pointers and context a call uses */
#define GLUE_STUB_CURRENT 0 /* the registered pointers and their context */
#define GLUE_STUB_STRAY 1   /* the registered pointers, a context that was never registered */
#define GLUE_STUB_SAVED 2   /* the pointers and context glue_stub_save() saw */
/* the result of a call that found no pointer to call */
#define GLUE_STUB_NONE (-100)
/* pthread_create failed */
#define GLUE_STUB_NO_THREAD (-101)

int glue_stub_fire_link(int how, int port, int up);
int glue_stub_fire_l2(int how, uint64_t mac, int null_mac, int vid, int port, int learned);
int glue_stub_fire_gate(int how, int port);
void glue_stub_save(void);
int glue_stub_fire_link_storm(int threads, int each, int port);
int glue_stub_inject_from_thread(const char* line);
void glue_stub_line_buffered(void);

/* the stub's stdout line by line (before its first line): its lines are whole between the test
 * harness's own output in the same pipe */
void glue_stub_line_buffered(void) { setvbuf(stdout, NULL, _IOLBF, 0); }

static ob_link_cb_t s_link;
static ob_l2_cb_t s_l2;
static void* s_ctx;
static ob_gate_cb_t s_gate;
static void* s_gate_ctx;

void glue_stub_save(void)
{
    pthread_mutex_lock(&g_mu);
    s_link = g_link;
    s_l2 = g_l2;
    s_ctx = g_ctx;
    s_gate = g_gate;
    s_gate_ctx = g_gate_ctx;
    pthread_mutex_unlock(&g_mu);
}

typedef struct {
    char kind; /* 'k' link, '2' L2, 'g' gate */
    int how, port, up, vid, learned, null_mac;
    uint64_t mac;
    int result;
} shot_t;

static void* shoot(void* arg)
{
    shot_t* s = (shot_t*)arg;
    ob_link_cb_t link;
    ob_l2_cb_t l2;
    ob_gate_cb_t gate;
    void *ctx, *gate_ctx;
    pthread_mutex_lock(&g_mu);
    if (s->how == GLUE_STUB_SAVED) {
        link = s_link;
        l2 = s_l2;
        ctx = s_ctx;
        gate = s_gate;
        gate_ctx = s_gate_ctx;
    } else {
        link = g_link;
        l2 = g_l2;
        ctx = g_ctx;
        gate = g_gate;
        gate_ctx = g_gate_ctx;
    }
    pthread_mutex_unlock(&g_mu);
    if (s->how == GLUE_STUB_STRAY) {
        /* an address-sized number no registration has (the keys count up from 1) */
        ctx = (void*)(~(uintptr_t)0 - 7);
        gate_ctx = ctx;
    }
    s->result = GLUE_STUB_NONE;
    if (s->kind == 'k' && link) {
        link(ctx, s->port, s->up);
        s->result = 0;
    } else if (s->kind == '2' && l2) {
        uint8_t mac[6];
        for (int i = 0; i < 6; i++) mac[i] = (uint8_t)(s->mac >> (8 * (5 - i)));
        l2(ctx, s->null_mac ? NULL : mac, s->vid, s->port, s->learned);
        s->result = 0;
    } else if (s->kind == 'g' && gate) {
        s->result = gate(gate_ctx, s->port);
    }
    return NULL;
}

static int fire_on_thread(shot_t* s)
{
    pthread_t t;
    if (pthread_create(&t, NULL, shoot, s) != 0) return GLUE_STUB_NO_THREAD;
    pthread_join(t, NULL);
    return s->result;
}

/* the link callback on a new thread: 0 when a pointer was called */
int glue_stub_fire_link(int how, int port, int up)
{
    shot_t s = {.kind = 'k', .how = how, .port = port, .up = up};
    return fire_on_thread(&s);
}

/* the L2 callback on a new thread; mac: the six bytes, most significant first (null_mac: NULL) */
int glue_stub_fire_l2(int how, uint64_t mac, int null_mac, int vid, int port, int learned)
{
    shot_t s = {.kind = '2', .how = how, .mac = mac, .null_mac = null_mac, .vid = vid, .port = port, .learned = learned};
    return fire_on_thread(&s);
}

/* the gate on a new thread: its answer */
int glue_stub_fire_gate(int how, int port)
{
    shot_t s = {.kind = 'g', .how = how, .port = port};
    return fire_on_thread(&s);
}

typedef struct {
    int each, port;
    long found;
} storm_t;

static void* storm_run(void* arg)
{
    storm_t* s = (storm_t*)arg;
    for (int i = 0; i < s->each; i++) {
        pthread_mutex_lock(&g_mu);
        ob_link_cb_t link = g_link;
        void* ctx = g_ctx;
        pthread_mutex_unlock(&g_mu);
        if (link) {
            link(ctx, s->port, i & 1);
            s->found++;
        }
    }
    return NULL;
}

/* threads x each link calls with the registered pointer, read anew for every call, from
 * threads running at the same time: how many calls found a pointer (negative: no thread) */
int glue_stub_fire_link_storm(int threads, int each, int port)
{
    enum { MAX = 32 };
    pthread_t t[MAX];
    storm_t s[MAX];
    if (threads < 1 || threads > MAX) return GLUE_STUB_NO_THREAD;
    int started = 0;
    for (int i = 0; i < threads; i++) {
        s[i] = (storm_t){.each = each, .port = port, .found = 0};
        if (pthread_create(&t[i], NULL, storm_run, &s[i]) != 0) break;
        started++;
    }
    long found = 0;
    for (int i = 0; i < started; i++) {
        pthread_join(t[i], NULL);
        found += s[i].found;
    }
    return started == threads ? (int)found : GLUE_STUB_NO_THREAD;
}

static void* inject_run(void* arg)
{
    glue_stub_inject((const char*)arg);
    return NULL;
}

/* glue_stub_inject(line) on a new thread: the callbacks the injection replays run there */
int glue_stub_inject_from_thread(const char* line)
{
    pthread_t t;
    if (pthread_create(&t, NULL, inject_run, (void*)line) != 0) return GLUE_STUB_NO_THREAD;
    pthread_join(t, NULL);
    return 0;
}
