/* SPDX-License-Identifier: Apache-2.0
 * A recording stub of sdk_glue.h (all 75 functions), for sdk-glue's real-path test
 * (tests/real_stub.rs, through tests/c/real_stub_threads.c): Rust's real glue (feature "real",
 * src/real.rs) calls these instead of sdk_glue.c + the SDK. The code is sai-backend-openbcm's
 * tests/cpp/glue_stub.c as of commit 50a6aa7 (written for the C++ backend's glue differential,
 * deleted with the C++), unchanged. Every call prints one line, "  glue " + two blanks per nesting
 * level + the canonical line of sdk-glue's call log (src/record.rs, the reference of the format),
 * to stdout.
 *
 * Answers: GLUE_STUB_REPLAY names a file of a Rust fake chip's recorded lines (the same text,
 * without the "  glue " prefix). Each call takes the next unconsumed record of its nesting level
 * whose name and inputs are equal (looking at most WINDOW records of that level ahead, never past
 * a test injection), else the next unconsumed record of that level if it has the same name
 * (the inputs differ: the line shows it); it returns that record's return value
 * and outputs, and first fires the callbacks recorded inside it (link, L2, gate: into the
 * pointers registered with it; a gate's answer is the registered gate's). Without a record a call
 * returns 0 with zeroed outputs and its line ends in " (no record)". Records nobody asked
 * for are skipped. Test injections (glue_stub_inject) replay the callbacks recorded inside the
 * Rust injection of the same text.
 *
 * Not logged: ob_sdk_errmsg (a text lookup, "glue stub error <n>"). Without GLUE_STUB_REPLAY every
 * call answers 0: harmless. One thread at a time (a recursive lock). */
#define _GNU_SOURCE   /* PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP, getline */
#include "sdk_glue.h"

#include <pthread.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define WINDOW 16

/* ------------------------------------------------------------------ string builder */
typedef struct {
    char* s;
    size_t n, cap;
} sb_t;

static void sb_grow(sb_t* b, size_t more)
{
    if (b->n + more + 1 <= b->cap) return;
    size_t cap = b->cap ? b->cap : 256;
    while (cap < b->n + more + 1) cap *= 2;
    char* s = (char*)realloc(b->s, cap);
    if (!s) abort();
    b->s = s;
    b->cap = cap;
}
static void sb_printf(sb_t* b, const char* fmt, ...) __attribute__((format(printf, 2, 3)));
static void sb_printf(sb_t* b, const char* fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    va_list aq;
    va_copy(aq, ap);
    int n = vsnprintf(NULL, 0, fmt, aq);
    va_end(aq);
    if (n < 0) abort();
    sb_grow(b, (size_t)n);
    vsnprintf(b->s + b->n, b->cap - b->n, fmt, ap);
    b->n += (size_t)n;
    va_end(ap);
}
static void sb_quoted_n(sb_t* b, const char* s, size_t n)
{
    sb_printf(b, "\"");
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c == '"' || c == '\\') sb_printf(b, "\\%c", c);
        else if (c >= 0x20 && c <= 0x7e) sb_printf(b, "%c", c);
        else sb_printf(b, "\\x%02x", c);
    }
    sb_printf(b, "\"");
}
static void sb_quoted(sb_t* b, const char* s) { sb_quoted_n(b, s ? s : "", s ? strlen(s) : 0); }
/* a message buffer: up to the NUL, or all len bytes */
static void sb_text(sb_t* b, const char* buf, int len)
{
    size_t n = 0;
    while (len > 0 && n < (size_t)len && buf[n]) n++;
    sb_quoted_n(b, buf, n);
}
static void sb_mac(sb_t* b, const uint8_t* m)
{
    sb_printf(b, "%02x:%02x:%02x:%02x:%02x:%02x", m[0], m[1], m[2], m[3], m[4], m[5]);
}
static void sb_hex(sb_t* b, const uint8_t* p, int n)
{
    for (int i = 0; i < n; i++) sb_printf(b, "%02x", p[i]);
}
static void sb_ints(sb_t* b, const int* v, int n)
{
    sb_printf(b, "[");
    for (int i = 0; i < n; i++) sb_printf(b, "%s%d", i ? "," : "", v[i]);
    sb_printf(b, "]");
}
static void sb_u32s(sb_t* b, const uint32_t* v, int n)
{
    sb_printf(b, "[");
    for (int i = 0; i < n; i++) sb_printf(b, "%s%u", i ? "," : "", v[i]);
    sb_printf(b, "]");
}
static const char* set(const void* p) { return p ? "set" : "null"; }
static int clamp(int n, int max) { return n < 0 ? 0 : n > max ? max : n; }

/* ------------------------------------------------------------------ the records */
typedef struct {
    int depth;
    char kind;       /* 'c' call, 'b' callback, 'i' injection */
    char* text;      /* without the indentation */
    size_t keylen;   /* name and inputs: up to " -> " outside quotes */
    size_t namelen;  /* up to '(' */
    int used;
} rec_t;

static rec_t* g_recs;
static int g_nrecs, g_cursor, g_loaded;
static pthread_mutex_t g_mu = PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP;
static __thread int t_depth;   /* the level of the next call on this thread */

static ob_link_cb_t g_link;
static ob_l2_cb_t g_l2;
static void* g_ctx;
static ob_gate_cb_t g_gate;
static void* g_gate_ctx;

/* the position of " -> " outside double quotes, or the length */
static size_t arrow(const char* s)
{
    int q = 0;
    for (size_t i = 0; s[i]; i++) {
        if (q) {
            if (s[i] == '\\' && s[i + 1]) i++;
            else if (s[i] == '"') q = 0;
        } else if (s[i] == '"') {
            q = 1;
        } else if (strncmp(s + i, " -> ", 4) == 0) {
            return i;
        }
    }
    return strlen(s);
}

static void load(void)
{
    if (g_loaded) return;
    g_loaded = 1;
    const char* path = getenv("GLUE_STUB_REPLAY");
    if (!path || !*path) return;
    FILE* f = fopen(path, "r");
    if (!f) {
        fprintf(stderr, "glue stub: cannot read %s\n", path);
        exit(3);
    }
    char* line = NULL;
    size_t cap = 0;
    ssize_t n;
    int room = 0;
    while ((n = getline(&line, &cap, f)) >= 0) {
        while (n > 0 && (line[n - 1] == '\n' || line[n - 1] == '\r')) line[--n] = 0;
        if (n == 0) continue;
        int sp = 0;
        while (line[sp] == ' ') sp++;
        if (g_nrecs == room) {
            room = room ? room * 2 : 1024;
            g_recs = (rec_t*)realloc(g_recs, (size_t)room * sizeof(rec_t));
            if (!g_recs) abort();
        }
        rec_t* r = &g_recs[g_nrecs++];
        r->depth = sp / 2;
        r->text = strdup(line + sp);
        if (!r->text) abort();
        r->kind = strncmp(r->text, "cb ", 3) == 0 ? 'b' : strncmp(r->text, "inject ", 7) == 0 ? 'i' : 'c';
        r->keylen = arrow(r->text);
        const char* paren = strchr(r->text, '(');
        r->namelen = paren ? (size_t)(paren - r->text) : r->keylen;
        r->used = 0;
    }
    free(line);
    fclose(f);
}

static size_t name_len(const char* key)
{
    const char* p = strchr(key, '(');
    return p ? (size_t)(p - key) : strlen(key);
}

/* The record of a call / injection with this key at level t_depth, or -1. */
static int find(char kind, const char* key)
{
    size_t klen = strlen(key), nlen = name_len(key);
    int fallback = -1, seen = 0;
    for (int i = g_cursor; i < g_nrecs; i++) {
        rec_t* r = &g_recs[i];
        if (r->depth < t_depth) break;   /* past the end of the enclosing step */
        if (r->depth > t_depth || r->used) continue;
        if (kind == 'c' && r->kind == 'i') break;   /* never past an injection */
        if (r->kind == kind && r->keylen == klen && memcmp(r->text, key, klen) == 0) return i;
        /* the first record of the level only: same function, other inputs */
        if (seen == 0 && r->kind == kind && r->namelen == nlen && memcmp(r->text, key, nlen) == 0) fallback = i;
        if (++seen >= WINDOW) break;
    }
    return fallback;
}

/* The C++ side of a recorded callback; prints its line at level t_depth - 1. */
static void fire(int k)
{
    const char* t = g_recs[k].text;
    sb_t b = {0};
    int port, up, vid, learned;
    unsigned m[6];
    char macw[32];
    if (sscanf(t, "cb link(port=%d, up=%d)", &port, &up) == 2) {
        if (g_link) g_link(g_ctx, port, up);
        sb_printf(&b, "cb link(port=%d, up=%d)%s", port, up, g_link ? "" : " (not registered)");
    } else if (sscanf(t, "cb gate(port=%d)", &port) == 1) {
        int answer = g_gate ? g_gate(g_gate_ctx, port) : -1;
        sb_printf(&b, "cb gate(port=%d) -> %d%s", port, answer, g_gate ? "" : " (not registered)");
    } else if (sscanf(t, "cb l2(mac=%31[^,], vid=%d, port=%d, learned=%d)", macw, &vid, &port, &learned) == 4) {
        uint8_t mac[6];
        int have = sscanf(macw, "%x:%x:%x:%x:%x:%x", &m[0], &m[1], &m[2], &m[3], &m[4], &m[5]) == 6;
        for (int i = 0; i < 6; i++) mac[i] = have ? (uint8_t)m[i] : 0;
        if (g_l2) g_l2(g_ctx, have ? mac : NULL, vid, port, learned);
        sb_printf(&b, "cb l2(mac=%s, vid=%d, port=%d, learned=%d)%s", macw, vid, port, learned,
                  g_l2 ? "" : " (not registered)");
    } else {
        sb_printf(&b, "%s (not understood)", t);
    }
    printf("  glue %*s%s\n", 2 * (t_depth - 1), "", b.s);
    free(b.s);
}

/* Fires the callbacks recorded inside record j (the deeper records just before it). */
static void replay(int j)
{
    int level = t_depth;
    int start = j;
    while (start > g_cursor && g_recs[start - 1].depth > level) start--;
    for (int k = g_cursor; k < start; k++)
        if (g_recs[k].depth >= level) g_recs[k].used = 1;   /* the Rust stack's, not asked for */
    int pos = start;
    while (pos < j) {
        int k = pos;
        while (k < j && g_recs[k].depth != level + 1) k++;
        if (k >= j) break;
        g_cursor = pos;
        t_depth = level + 2;
        fire(k);
        t_depth = level;
        for (int x = pos; x <= k; x++) g_recs[x].used = 1;
        pos = k + 1;
    }
    for (int x = start; x < j; x++) g_recs[x].used = 1;
    g_recs[j].used = 1;
    g_cursor = j + 1;
}

/* ------------------------------------------------------------------ one call */
typedef struct {
    sb_t key;
    int j;        /* its record, -1 none */
    int level;
} call_t;

static void begin(call_t* c, const char* fmt, ...) __attribute__((format(printf, 2, 3)));
static void begin(call_t* c, const char* fmt, ...)
{
    pthread_mutex_lock(&g_mu);
    load();
    memset(c, 0, sizeof(*c));
    va_list ap;
    va_start(ap, fmt);
    va_list aq;
    va_copy(aq, ap);
    int n = vsnprintf(NULL, 0, fmt, aq);
    va_end(aq);
    sb_grow(&c->key, (size_t)n);
    vsnprintf(c->key.s, c->key.cap, fmt, ap);
    c->key.n = (size_t)n;
    va_end(ap);
    c->level = t_depth;
    c->j = find('c', c->key.s);
    if (c->j >= 0) replay(c->j);
}

/* An output of the call's record: the value after " name=" at the top level of its outputs. */
static const char* out_of(const call_t* c, const char* name)
{
    if (c->j < 0) return NULL;
    const char* s = g_recs[c->j].text + g_recs[c->j].keylen;
    size_t nl = strlen(name);
    int q = 0, deep = 0;
    for (size_t i = 0; s[i]; i++) {
        if (q) {
            if (s[i] == '\\' && s[i + 1]) i++;
            else if (s[i] == '"') q = 0;
            continue;
        }
        if (s[i] == '"') q = 1;
        else if (s[i] == '[' || s[i] == '{') deep++;
        else if (s[i] == ']' || s[i] == '}') deep--;
        else if (deep == 0 && s[i] == ' ' && strncmp(s + i + 1, name, nl) == 0 && s[i + 1 + nl] == '=')
            return s + i + 2 + nl;
    }
    return NULL;
}
static long long out_ll(const call_t* c, const char* name)
{
    const char* p = out_of(c, name);
    return p ? strtoll(p, NULL, 0) : 0;
}
static unsigned long long out_ull(const call_t* c, const char* name)
{
    const char* p = out_of(c, name);
    return p ? strtoull(p, NULL, 0) : 0;
}
/* a quoted string value at p into buf (len bytes, NUL-terminated) */
static void unquote(const char* p, char* buf, int len)
{
    if (len <= 0) return;
    int n = 0;
    if (p && *p == '"') {
        for (p++; *p && *p != '"'; p++) {
            int ch = (unsigned char)*p;
            if (*p == '\\' && p[1] == 'x' && p[2] && p[3]) {
                char h[3] = {p[2], p[3], 0};
                ch = (int)strtol(h, NULL, 16);
                p += 3;
            } else if (*p == '\\' && p[1]) {
                ch = (unsigned char)*++p;
            }
            if (n < len - 1) buf[n++] = (char)ch;
        }
    }
    buf[n] = 0;
}
static void out_text(const call_t* c, const char* name, char* buf, int len) { unquote(out_of(c, name), buf, len); }
/* a list [a,b,..] of numbers; how many were read (at most max) */
static int out_list(const call_t* c, const char* name, long long* v, int max)
{
    const char* p = out_of(c, name);
    int n = 0;
    if (!p || *p != '[') return 0;
    p++;
    while (*p && *p != ']' && n < max) {
        char* end;
        long long x = strtoll(p, &end, 0);
        if (end == p) break;
        v[n++] = x;
        p = end;
        if (*p == ',') p++;
    }
    return n;
}
/* field "name=" inside a struct starting at p ("{...}") */
static const char* field_of(const char* p, const char* name)
{
    if (!p || *p != '{') return NULL;
    size_t nl = strlen(name);
    for (const char* s = p + 1; *s && *s != '}'; s++)
        if ((s == p + 1 || s[-1] == ' ') && strncmp(s, name, nl) == 0 && s[nl] == '=') return s + nl + 1;
    return NULL;
}
static long long field_ll(const char* p, const char* name)
{
    const char* v = field_of(p, name);
    return v ? strtoll(v, NULL, 0) : 0;
}
static unsigned long long field_ull(const char* p, const char* name)
{
    const char* v = field_of(p, name);
    return v ? strtoull(v, NULL, 0) : 0;
}

static int rv_of(const call_t* c)
{
    if (c->j < 0) return 0;
    const char* s = g_recs[c->j].text + g_recs[c->j].keylen;
    return strncmp(s, " -> ", 4) == 0 ? (int)strtol(s + 4, NULL, 0) : 0;
}

/* Prints the call's line (key, " -> ", rv, the outputs fmt gives) and returns rv. */
static int end(call_t* c, int rv, const char* fmt, ...) __attribute__((format(printf, 3, 4)));
static int end(call_t* c, int rv, const char* fmt, ...)
{
    sb_t b = {0};
    sb_printf(&b, "%s -> %d", c->key.s, rv);
    va_list ap;
    va_start(ap, fmt);
    va_list aq;
    va_copy(aq, ap);
    int n = vsnprintf(NULL, 0, fmt, aq);
    va_end(aq);
    sb_grow(&b, (size_t)n);
    vsnprintf(b.s + b.n, b.cap - b.n, fmt, ap);
    b.n += (size_t)n;
    va_end(ap);
    printf("  glue %*s%s%s\n", 2 * c->level, "", b.s, c->j < 0 ? " (no record)" : "");
    free(b.s);
    free(c->key.s);
    pthread_mutex_unlock(&g_mu);
    return rv;
}
/* the same with outputs already formatted */
static int end_sb(call_t* c, int rv, sb_t* outs)
{
    int r = end(c, rv, "%s", outs->s ? outs->s : "");
    free(outs->s);
    return r;
}
static void end_void(call_t* c)
{
    printf("  glue %*s%s -> void%s\n", 2 * c->level, "", c->key.s, c->j < 0 ? " (no record)" : "");
    free(c->key.s);
    pthread_mutex_unlock(&g_mu);
}

/* ------------------------------------------------------------------ injections */
void glue_stub_inject(const char* line)
{
    pthread_mutex_lock(&g_mu);
    load();
    int level = t_depth;
    int j = find('i', line);
    if (j >= 0) replay(j);
    printf("  glue %*s%s%s\n", 2 * level, "", line, j < 0 ? " (no record)" : "");
    pthread_mutex_unlock(&g_mu);
}

/* ------------------------------------------------------------------ the glue */
static void err_out(const call_t* c, char* err, int errlen) { out_text(c, "err", err, errlen); }

static int bring_up(call_t* c, char* err, int errlen)
{
    int rv = rv_of(c);
    err_out(c, err, errlen);
    sb_t o = {0};
    sb_printf(&o, " err=");
    sb_text(&o, err, errlen);
    return end_sb(c, rv, &o);
}

int ob_sdk_init(int unit, const char* config_file, const char* rc_file, char* err, int errlen)
{
    call_t c;
    sb_t k = {0};
    sb_quoted(&k, config_file);
    sb_t r = {0};
    sb_quoted(&r, rc_file);
    begin(&c, "init(unit=%d, config=%s, rc=%s, errlen=%d)", unit, k.s, r.s, errlen);
    free(k.s);
    free(r.s);
    return bring_up(&c, err, errlen);
}

int ob_sdk_attach(int unit, char* err, int errlen)
{
    call_t c;
    begin(&c, "attach(unit=%d, errlen=%d)", unit, errlen);
    return bring_up(&c, err, errlen);
}

int ob_sdk_init_mdio(int unit, const char* config_file, char* err, int errlen)
{
    call_t c;
    sb_t k = {0};
    sb_quoted(&k, config_file);
    begin(&c, "init_mdio(unit=%d, config=%s, errlen=%d)", unit, k.s, errlen);
    free(k.s);
    return bring_up(&c, err, errlen);
}

int ob_sdk_temperatures(int unit, int* cur, int* peak, int max, int* count)
{
    call_t c;
    begin(&c, "temperatures(unit=%d, max=%d)", unit, max);
    long long v[64];
    int n = (int)out_ll(&c, "count");
    *count = n;
    int k = out_list(&c, "cur", v, 64);
    for (int i = 0; i < k && i < max; i++) cur[i] = (int)v[i];
    k = out_list(&c, "peak", v, 64);
    for (int i = 0; i < k && i < max; i++) peak[i] = (int)v[i];
    int shown = clamp(*count < max ? *count : max, max);
    sb_t o = {0};
    sb_printf(&o, " count=%d cur=", *count);
    sb_ints(&o, cur, shown);
    sb_printf(&o, " peak=");
    sb_ints(&o, peak, shown);
    return end_sb(&c, rv_of(&c), &o);
}

int ob_sdk_th_overtemp_shutdown(int unit)
{
    call_t c;
    begin(&c, "th_overtemp_shutdown(unit=%d)", unit);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_mdio_c45_read(int unit, unsigned phy, unsigned dev, unsigned reg, unsigned* val)
{
    call_t c;
    begin(&c, "mdio_c45_read(unit=%d, phy=0x%x, dev=%u, reg=0x%x)", unit, phy, dev, reg);
    *val = (unsigned)out_ull(&c, "val");
    return end(&c, rv_of(&c), " val=0x%x", *val);
}

int ob_sdk_mdio_c45_write(int unit, unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    call_t c;
    begin(&c, "mdio_c45_write(unit=%d, phy=0x%x, dev=%u, reg=0x%x, val=0x%x)", unit, phy, dev, reg, val);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_linkscan_port_gate(int unit, int port, ob_gate_cb_t gate, void* ctx)
{
    call_t c;
    begin(&c, "linkscan_port_gate(unit=%d, port=%d, gate=%s, ctx=%s)", unit, port, set((const void*)gate), set(ctx));
    int rv = rv_of(&c);
    if (gate && rv == 0) {
        g_gate = gate;
        g_gate_ctx = ctx;
    }
    return end(&c, rv, "%s", "");
}

void ob_sdk_shutdown(int unit)
{
    call_t c;
    begin(&c, "shutdown(unit=%d)", unit);
    g_link = NULL;
    g_l2 = NULL;
    g_ctx = NULL;
    end_void(&c);
}

int ob_sdk_shell(int unit, const char* cmd)
{
    call_t c;
    sb_t q = {0};
    sb_quoted(&q, cmd);
    begin(&c, "shell(unit=%d, cmd=%s)", unit, q.s);
    free(q.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_shell_to(int unit, const char* cmd, int fd)
{
    (void)fd;
    call_t c;
    sb_t q = {0};
    sb_quoted(&q, cmd);
    begin(&c, "shell_to(unit=%d, cmd=%s, fd)", unit, q.s);
    free(q.s);
    return end(&c, rv_of(&c), "%s", "");
}

const char* ob_sdk_errmsg(int rv)
{
    static __thread char b[48];
    snprintf(b, sizeof(b), "glue stub error %d", rv);
    return b;
}

int ob_sdk_register_callbacks(int unit, ob_link_cb_t link, ob_l2_cb_t l2, void* ctx)
{
    call_t c;
    begin(&c, "register_callbacks(unit=%d, link=%s, l2=%s, ctx=%s)", unit, set((const void*)link), set((const void*)l2),
          set(ctx));
    /* as the glue: stored first, then the SDK is asked */
    g_link = link;
    g_l2 = l2;
    g_ctx = ctx;
    return end(&c, rv_of(&c), "%s", "");
}

void ob_sdk_unregister_callbacks(int unit)
{
    call_t c;
    begin(&c, "unregister_callbacks(unit=%d)", unit);
    g_link = NULL;
    g_l2 = NULL;
    end_void(&c);
}

int ob_sdk_hw_info(int unit, char* buf, int len)
{
    call_t c;
    begin(&c, "hw_info(unit=%d, len=%d)", unit, len);
    out_text(&c, "buf", buf, len);
    sb_t o = {0};
    sb_printf(&o, " buf=");
    sb_text(&o, buf, len);
    return end_sb(&c, rv_of(&c), &o);
}

int ob_sdk_knet_cleanup(int unit, int* netifs, int* filters)
{
    call_t c;
    begin(&c, "knet_cleanup(unit=%d)", unit);
    *netifs = (int)out_ll(&c, "netifs");
    *filters = (int)out_ll(&c, "filters");
    return end(&c, rv_of(&c), " netifs=%d filters=%d", *netifs, *filters);
}

int ob_sdk_table_usage(int unit, ob_table_usage_t* u)
{
    call_t c;
    begin(&c, "table_usage(unit=%d)", unit);
    const char* p = out_of(&c, "u");
    u->v4_routes_free = (uint32_t)field_ull(p, "v4_routes_free");
    u->v6_routes_free = (uint32_t)field_ull(p, "v6_routes_free");
    u->hosts_free = (uint32_t)field_ull(p, "hosts_free");
    u->egress_free = (uint32_t)field_ull(p, "egress_free");
    u->fdb_free = (uint32_t)field_ull(p, "fdb_free");
    u->ecmp_groups_free = (uint32_t)field_ull(p, "ecmp_groups_free");
    u->ecmp_members_free = (uint32_t)field_ull(p, "ecmp_members_free");
    return end(&c, rv_of(&c),
               " u={v4_routes_free=%u, v6_routes_free=%u, hosts_free=%u, egress_free=%u, fdb_free=%u, "
               "ecmp_groups_free=%u, ecmp_members_free=%u}",
               u->v4_routes_free, u->v6_routes_free, u->hosts_free, u->egress_free, u->fdb_free, u->ecmp_groups_free,
               u->ecmp_members_free);
}

int ob_sdk_chip_limits(int unit, ob_chip_limits_t* l)
{
    call_t c;
    begin(&c, "chip_limits(unit=%d)", unit);
    const char* p = out_of(&c, "l");
    l->buffer_bytes = field_ull(p, "buffer_bytes");
    l->acl_prio_min = (uint32_t)field_ull(p, "acl_prio_min");
    l->acl_prio_max = (uint32_t)field_ull(p, "acl_prio_max");
    l->max_host = (int)field_ll(p, "max_host");
    l->max_route = (int)field_ll(p, "max_route");
    l->max_nexthop = (int)field_ll(p, "max_nexthop");
    l->max_l2 = (int)field_ll(p, "max_l2");
    l->ecmp_groups = (int)field_ll(p, "ecmp_groups");
    l->ecmp_paths = (int)field_ll(p, "ecmp_paths");
    l->ecmp_members = (int)field_ll(p, "ecmp_members");
    l->vrf_max = (int)field_ll(p, "vrf_max");
    return end(&c, rv_of(&c),
               " l={buffer_bytes=%llu, acl_prio_min=%u, acl_prio_max=%u, max_host=%d, max_route=%d, max_nexthop=%d, "
               "max_l2=%d, ecmp_groups=%d, ecmp_paths=%d, ecmp_members=%d, vrf_max=%d}",
               (unsigned long long)l->buffer_bytes, l->acl_prio_min, l->acl_prio_max, l->max_host, l->max_route,
               l->max_nexthop, l->max_l2, l->ecmp_groups, l->ecmp_paths, l->ecmp_members, l->vrf_max);
}

int ob_sdk_cpu_port(int unit)
{
    call_t c;
    begin(&c, "cpu_port(unit=%d)", unit);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_ports(int unit, ob_port_info_t* out, int max)
{
    call_t c;
    begin(&c, "ports(unit=%d, max=%d)", unit, max);
    int rv = rv_of(&c);
    const char* p = out_of(&c, "out");
    int n = 0;
    if (p && *p == '[') {
        p++;
        while (*p == '{' && n < max) {
            ob_port_info_t* o = &out[n];
            memset(o, 0, sizeof(*o));
            o->port = (int)field_ll(p, "port");
            o->phys = (int)field_ll(p, "phys");
            o->lanes = (int)field_ll(p, "lanes");
            o->speed = (int)field_ll(p, "speed");
            unquote(field_of(p, "name"), o->name, (int)sizeof(o->name));
            n++;
            /* to the next entry: past this struct's closing brace (names hold no brace) */
            const char* e = strchr(p, '}');
            if (!e) break;
            p = e + 1;
            if (*p == ',') p++;
        }
    }
    sb_t o = {0};
    sb_printf(&o, " out=[");
    int shown = clamp(rv, max);
    for (int i = 0; i < shown; i++) {
        sb_printf(&o, "%s{port=%d, phys=%d, lanes=%d, speed=%d, name=", i ? "," : "", out[i].port, out[i].phys,
                  out[i].lanes, out[i].speed);
        sb_text(&o, out[i].name, (int)sizeof(out[i].name));
        sb_printf(&o, "}");
    }
    sb_printf(&o, "]");
    return end_sb(&c, rv, &o);
}

#define PORT_INT(fn, arg)                                                       \
    int ob_sdk_##fn(int unit, int port, int arg)                                \
    {                                                                           \
        call_t c;                                                               \
        begin(&c, #fn "(unit=%d, port=%d, " #arg "=%d)", unit, port, arg);      \
        return end(&c, rv_of(&c), "%s", "");                                    \
    }
PORT_INT(port_enable_set, enable)
PORT_INT(port_speed_set, mbps)
PORT_INT(port_frame_max_set, bytes)
PORT_INT(port_fec_set, mode)
PORT_INT(port_autoneg_set, on)
PORT_INT(port_ifg_set, bits)
PORT_INT(port_loopback_set, mode)
PORT_INT(port_pvid_set, vid)
PORT_INT(port_learn_set, mode)

#define PORT_GET(fn, arg)                                                       \
    int ob_sdk_##fn(int unit, int port, int* arg)                               \
    {                                                                           \
        call_t c;                                                               \
        begin(&c, #fn "(unit=%d, port=%d)", unit, port);                        \
        *arg = (int)out_ll(&c, #arg);                                           \
        return end(&c, rv_of(&c), " " #arg "=%d", *arg);                        \
    }
PORT_GET(port_link_get, up)
PORT_GET(port_speed_get, mbps)
PORT_GET(port_autoneg_get, on)

int ob_sdk_port_speeds(int unit, int port, uint32_t* out, int max)
{
    call_t c;
    begin(&c, "port_speeds(unit=%d, port=%d, max=%d)", unit, port, max);
    long long v[64];
    int k = out_list(&c, "out", v, 64);
    for (int i = 0; i < k && i < max; i++) out[i] = (uint32_t)v[i];
    int rv = rv_of(&c);
    sb_t o = {0};
    sb_printf(&o, " out=");
    sb_u32s(&o, out, clamp(rv, max));
    return end_sb(&c, rv, &o);
}

int ob_sdk_port_interface_set(int unit, int port, const char* name)
{
    call_t c;
    sb_t q = {0};
    sb_quoted(&q, name);
    begin(&c, "port_interface_set(unit=%d, port=%d, name=%s)", unit, port, q.s);
    free(q.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_port_stat_get(int unit, int port, int which, uint64_t* v)
{
    call_t c;
    begin(&c, "port_stat_get(unit=%d, port=%d, which=%d)", unit, port, which);
    *v = out_ull(&c, "v");
    return end(&c, rv_of(&c), " v=%llu", (unsigned long long)*v);
}

int ob_sdk_port_stat_clear(int unit, int port)
{
    call_t c;
    begin(&c, "port_stat_clear(unit=%d, port=%d)", unit, port);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_vlan_create(int unit, int vid)
{
    call_t c;
    begin(&c, "vlan_create(unit=%d, vid=%d)", unit, vid);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_vlan_destroy(int unit, int vid)
{
    call_t c;
    begin(&c, "vlan_destroy(unit=%d, vid=%d)", unit, vid);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_vlan_port_add(int unit, int vid, int port, int untagged)
{
    call_t c;
    begin(&c, "vlan_port_add(unit=%d, vid=%d, port=%d, untagged=%d)", unit, vid, port, untagged);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_vlan_port_remove(int unit, int vid, int port)
{
    call_t c;
    begin(&c, "vlan_port_remove(unit=%d, vid=%d, port=%d)", unit, vid, port);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_vlan_members(int unit, int vid, int* ports, int* untagged, int max)
{
    call_t c;
    begin(&c, "vlan_members(unit=%d, vid=%d, max=%d)", unit, vid, max);
    long long v[1024];
    int k = out_list(&c, "ports", v, 1024);
    for (int i = 0; i < k && i < max; i++) ports[i] = (int)v[i];
    k = out_list(&c, "untagged", v, 1024);
    for (int i = 0; i < k && i < max; i++) untagged[i] = (int)v[i];
    int rv = rv_of(&c);
    sb_t o = {0};
    sb_printf(&o, " ports=");
    sb_ints(&o, ports, clamp(rv, max));
    sb_printf(&o, " untagged=");
    sb_ints(&o, untagged, clamp(rv, max));
    return end_sb(&c, rv, &o);
}

/* a MAC argument as text */
#define MAC_ARG(m) sb_t m##_s = {0}; sb_mac(&m##_s, m)

int ob_sdk_l2_add(int unit, const uint8_t mac[6], int vid, int port, int is_static, int discard)
{
    call_t c;
    MAC_ARG(mac);
    begin(&c, "l2_add(unit=%d, mac=%s, vid=%d, port=%d, is_static=%d, discard=%d)", unit, mac_s.s, vid, port, is_static,
          discard);
    free(mac_s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l2_delete(int unit, const uint8_t mac[6], int vid)
{
    call_t c;
    MAC_ARG(mac);
    begin(&c, "l2_delete(unit=%d, mac=%s, vid=%d)", unit, mac_s.s, vid);
    free(mac_s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l2_flush(int unit, int port, int vid, int statics, int dynamics)
{
    call_t c;
    begin(&c, "l2_flush(unit=%d, port=%d, vid=%d, statics=%d, dynamics=%d)", unit, port, vid, statics, dynamics);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l2_age_set(int unit, int seconds)
{
    call_t c;
    begin(&c, "l2_age_set(unit=%d, seconds=%d)", unit, seconds);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_netif_create(int unit, const char* name, int port, int vid, const uint8_t mac[6], int keep_tag, int* netif_id,
                        int* filter_id)
{
    call_t c;
    sb_t q = {0};
    sb_quoted(&q, name);
    MAC_ARG(mac);
    begin(&c, "netif_create(unit=%d, name=%s, port=%d, vid=%d, mac=%s, keep_tag=%d)", unit, q.s, port, vid, mac_s.s,
          keep_tag);
    free(q.s);
    free(mac_s.s);
    *netif_id = (int)out_ll(&c, "netif_id");
    *filter_id = (int)out_ll(&c, "filter_id");
    return end(&c, rv_of(&c), " netif_id=%d filter_id=%d", *netif_id, *filter_id);
}

int ob_sdk_netif_destroy(int unit, int netif_id, int filter_id)
{
    call_t c;
    begin(&c, "netif_destroy(unit=%d, netif_id=%d, filter_id=%d)", unit, netif_id, filter_id);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_netif_filter_set(int unit, int netif_id, int port, int keep_tag, int* filter_id)
{
    call_t c;
    begin(&c, "netif_filter_set(unit=%d, netif_id=%d, port=%d, keep_tag=%d, filter_id=%d)", unit, netif_id, port,
          keep_tag, *filter_id);
    *filter_id = (int)out_ll(&c, "filter_id");
    return end(&c, rv_of(&c), " filter_id=%d", *filter_id);
}

int ob_sdk_trap_set(int unit, int kind, int to_cpu)
{
    call_t c;
    begin(&c, "trap_set(unit=%d, kind=%d, to_cpu=%d)", unit, kind, to_cpu);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_trunk_create(int unit, int* tid)
{
    call_t c;
    begin(&c, "trunk_create(unit=%d)", unit);
    *tid = (int)out_ll(&c, "tid");
    return end(&c, rv_of(&c), " tid=%d", *tid);
}

int ob_sdk_trunk_destroy(int unit, int tid)
{
    call_t c;
    begin(&c, "trunk_destroy(unit=%d, tid=%d)", unit, tid);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_trunk_set(int unit, int tid, const int* ports, const int* flags, int n)
{
    call_t c;
    sb_t p = {0}, f = {0};
    sb_ints(&p, ports, n > 0 && ports ? n : 0);
    sb_ints(&f, flags, n > 0 && flags ? n : 0);
    begin(&c, "trunk_set(unit=%d, tid=%d, ports=%s, flags=%s, n=%d)", unit, tid, p.s, f.s, n);
    free(p.s);
    free(f.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_intf_create(int unit, const uint8_t mac[6], int vid, int mtu, int vrf, int* intf_id)
{
    call_t c;
    MAC_ARG(mac);
    begin(&c, "l3_intf_create(unit=%d, mac=%s, vid=%d, mtu=%d, vrf=%d)", unit, mac_s.s, vid, mtu, vrf);
    free(mac_s.s);
    *intf_id = (int)out_ll(&c, "intf_id");
    return end(&c, rv_of(&c), " intf_id=%d", *intf_id);
}

int ob_sdk_l3_intf_update(int unit, int intf_id, const uint8_t mac[6], int vid, int mtu, int vrf)
{
    call_t c;
    MAC_ARG(mac);
    begin(&c, "l3_intf_update(unit=%d, intf_id=%d, mac=%s, vid=%d, mtu=%d, vrf=%d)", unit, intf_id, mac_s.s, vid, mtu,
          vrf);
    free(mac_s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_intf_destroy(int unit, int intf_id)
{
    call_t c;
    begin(&c, "l3_intf_destroy(unit=%d, intf_id=%d)", unit, intf_id);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_station_add(int unit, const uint8_t mac[6], int* station_id)
{
    call_t c;
    MAC_ARG(mac);
    begin(&c, "l3_station_add(unit=%d, mac=%s)", unit, mac_s.s);
    free(mac_s.s);
    *station_id = (int)out_ll(&c, "station_id");
    return end(&c, rv_of(&c), " station_id=%d", *station_id);
}

int ob_sdk_l3_station_delete(int unit, int station_id)
{
    call_t c;
    begin(&c, "l3_station_delete(unit=%d, station_id=%d)", unit, station_id);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_egress_create(int unit, int intf, const uint8_t mac[6], int port, int vid, int* egress_id)
{
    call_t c;
    MAC_ARG(mac);
    begin(&c, "l3_egress_create(unit=%d, intf=%d, mac=%s, port=%d, vid=%d)", unit, intf, mac_s.s, port, vid);
    free(mac_s.s);
    *egress_id = (int)out_ll(&c, "egress_id");
    return end(&c, rv_of(&c), " egress_id=%d", *egress_id);
}

int ob_sdk_l3_egress_update(int unit, int egress_id, int intf, const uint8_t mac[6], int port, int vid)
{
    call_t c;
    MAC_ARG(mac);
    begin(&c, "l3_egress_update(unit=%d, egress_id=%d, intf=%d, mac=%s, port=%d, vid=%d)", unit, egress_id, intf, mac_s.s,
          port, vid);
    free(mac_s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_egress_special(int unit, int drop, int* egress_id)
{
    call_t c;
    begin(&c, "l3_egress_special(unit=%d, drop=%d)", unit, drop);
    *egress_id = (int)out_ll(&c, "egress_id");
    return end(&c, rv_of(&c), " egress_id=%d", *egress_id);
}

int ob_sdk_l3_egress_destroy(int unit, int egress_id)
{
    call_t c;
    begin(&c, "l3_egress_destroy(unit=%d, egress_id=%d)", unit, egress_id);
    return end(&c, rv_of(&c), "%s", "");
}

/* an address as the glue reads it: 16 bytes with v6, else 4 */
#define ADDR_ARG(a) sb_t a##_s = {0}; sb_hex(&a##_s, a, v6 ? 16 : 4)

int ob_sdk_l3_host_add(int unit, int vrf, int v6, const uint8_t* addr, int egress_id)
{
    call_t c;
    ADDR_ARG(addr);
    begin(&c, "l3_host_add(unit=%d, vrf=%d, v6=%d, addr=%s, egress_id=%d)", unit, vrf, v6, addr_s.s, egress_id);
    free(addr_s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_host_delete(int unit, int vrf, int v6, const uint8_t* addr)
{
    call_t c;
    ADDR_ARG(addr);
    begin(&c, "l3_host_delete(unit=%d, vrf=%d, v6=%d, addr=%s)", unit, vrf, v6, addr_s.s);
    free(addr_s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_route_add(int unit, int vrf, int v6, const uint8_t* addr, const uint8_t* mask, int egress_id, int ecmp,
                        int drop, int replace)
{
    call_t c;
    ADDR_ARG(addr);
    ADDR_ARG(mask);
    begin(&c,
          "l3_route_add(unit=%d, vrf=%d, v6=%d, addr=%s, mask=%s, egress_id=%d, ecmp=%d, drop=%d, replace=%d)",
          unit, vrf, v6, addr_s.s, mask_s.s, egress_id, ecmp, drop, replace);
    free(addr_s.s);
    free(mask_s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_route_delete(int unit, int vrf, int v6, const uint8_t* addr, const uint8_t* mask)
{
    call_t c;
    ADDR_ARG(addr);
    ADDR_ARG(mask);
    begin(&c, "l3_route_delete(unit=%d, vrf=%d, v6=%d, addr=%s, mask=%s)", unit, vrf, v6, addr_s.s, mask_s.s);
    free(addr_s.s);
    free(mask_s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_l3_ecmp_set(int unit, int* ecmp_id, int replace, const int* egress, int n)
{
    call_t c;
    sb_t cur = {0}, e = {0};
    if (replace) sb_printf(&cur, "ecmp_id=%d, ", *ecmp_id);
    else sb_printf(&cur, "%s", "");
    sb_ints(&e, egress, n > 0 && egress ? n : 0);
    begin(&c, "l3_ecmp_set(unit=%d, %sreplace=%d, egress=%s, n=%d)", unit, cur.s, replace, e.s, n);
    free(cur.s);
    free(e.s);
    *ecmp_id = (int)out_ll(&c, "ecmp_id");
    return end(&c, rv_of(&c), " ecmp_id=%d", *ecmp_id);
}

int ob_sdk_l3_ecmp_destroy(int unit, int ecmp_id)
{
    call_t c;
    begin(&c, "l3_ecmp_destroy(unit=%d, ecmp_id=%d)", unit, ecmp_id);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_fp_group_create(int unit, uint32_t qset, int priority, int* gid)
{
    call_t c;
    begin(&c, "fp_group_create(unit=%d, qset=0x%x, priority=%d)", unit, qset, priority);
    *gid = (int)out_ll(&c, "gid");
    return end(&c, rv_of(&c), " gid=%d", *gid);
}

int ob_sdk_fp_group_destroy(int unit, int gid)
{
    call_t c;
    begin(&c, "fp_group_destroy(unit=%d, gid=%d)", unit, gid);
    return end(&c, rv_of(&c), "%s", "");
}

static void sb_rule(sb_t* b, const ob_fp_rule_t* r)
{
    sb_printf(b, "{present=0x%x, ether=%u, ether_mask=%u, ip_type=%d, proto=%u, proto_mask=%u, src=", r->present,
              r->ether, r->ether_mask, r->ip_type, r->proto, r->proto_mask);
    sb_hex(b, r->src, 16);
    sb_printf(b, ", src_mask=");
    sb_hex(b, r->src_mask, 16);
    sb_printf(b, ", dst=");
    sb_hex(b, r->dst, 16);
    sb_printf(b, ", dst_mask=");
    sb_hex(b, r->dst_mask, 16);
    sb_printf(b,
              ", l4_src=%u, l4_src_mask=%u, l4_dst=%u, l4_dst_mask=%u, tcp=%u, tcp_mask=%u, icmp=%u, icmp_mask=%u, "
              "tos=%u, tos_mask=%u, dmac=",
              r->l4_src, r->l4_src_mask, r->l4_dst, r->l4_dst_mask, r->tcp, r->tcp_mask, r->icmp, r->icmp_mask, r->tos,
              r->tos_mask);
    sb_mac(b, r->dmac);
    sb_printf(b, ", dmac_mask=");
    sb_mac(b, r->dmac_mask);
    sb_printf(b, ", smac=");
    sb_mac(b, r->smac);
    sb_printf(b, ", smac_mask=");
    sb_mac(b, r->smac_mask);
    sb_printf(b, ", vlan=%u, vlan_mask=%u, ttl=%u, ttl_mask=%u, dst_egress=%d, ports=", r->vlan, r->vlan_mask, r->ttl,
              r->ttl_mask, r->dst_egress);
    sb_ints(b, r->ports, r->ports && r->nports > 0 ? r->nports : 0);
    sb_printf(b,
              ", nports=%d, priority=%d, drop=%d, drop_cancel=%d, copy_cpu=%d, counter=%d, policer=%d, green_drop=%d, "
              "yellow_drop=%d, red_drop=%d, cpu_copy_only=%d}",
              r->nports, r->priority, r->drop, r->drop_cancel, r->copy_cpu, r->counter, r->policer, r->green_drop,
              r->yellow_drop, r->red_drop, r->cpu_copy_only);
}

int ob_sdk_fp_entry_create(int unit, int gid, const ob_fp_rule_t* r, int* eid, int* stat_id)
{
    call_t c;
    sb_t s = {0};
    sb_rule(&s, r);
    begin(&c, "fp_entry_create(unit=%d, gid=%d, r=%s)", unit, gid, s.s);
    free(s.s);
    *eid = (int)out_ll(&c, "eid");
    *stat_id = (int)out_ll(&c, "stat_id");
    return end(&c, rv_of(&c), " eid=%d stat_id=%d", *eid, *stat_id);
}

int ob_sdk_fp_entry_destroy(int unit, int eid, int stat_id)
{
    call_t c;
    begin(&c, "fp_entry_destroy(unit=%d, eid=%d, stat_id=%d)", unit, eid, stat_id);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_fp_stat_get(int unit, int stat_id, uint64_t* packets, uint64_t* bytes)
{
    call_t c;
    begin(&c, "fp_stat_get(unit=%d, stat_id=%d)", unit, stat_id);
    *packets = out_ull(&c, "packets");
    *bytes = out_ull(&c, "bytes");
    return end(&c, rv_of(&c), " packets=%llu bytes=%llu", (unsigned long long)*packets, (unsigned long long)*bytes);
}

static void sb_policer(sb_t* b, const ob_policer_cfg_t* p)
{
    sb_printf(b, "{tr_tcm=%d, packets=%d, color_blind=%d, cir=%llu, cbs=%llu, pir=%llu, pbs=%llu}", p->tr_tcm,
              p->packets, p->color_blind, (unsigned long long)p->cir, (unsigned long long)p->cbs,
              (unsigned long long)p->pir, (unsigned long long)p->pbs);
}

int ob_sdk_policer_create(int unit, const ob_policer_cfg_t* cfg, int* pid)
{
    call_t c;
    sb_t s = {0};
    sb_policer(&s, cfg);
    begin(&c, "policer_create(unit=%d, c=%s)", unit, s.s);
    free(s.s);
    *pid = (int)out_ll(&c, "pid");
    return end(&c, rv_of(&c), " pid=%d", *pid);
}

int ob_sdk_policer_set(int unit, int pid, const ob_policer_cfg_t* cfg)
{
    call_t c;
    sb_t s = {0};
    sb_policer(&s, cfg);
    begin(&c, "policer_set(unit=%d, pid=%d, c=%s)", unit, pid, s.s);
    free(s.s);
    return end(&c, rv_of(&c), "%s", "");
}

int ob_sdk_policer_destroy(int unit, int pid)
{
    call_t c;
    begin(&c, "policer_destroy(unit=%d, pid=%d)", unit, pid);
    return end(&c, rv_of(&c), "%s", "");
}
