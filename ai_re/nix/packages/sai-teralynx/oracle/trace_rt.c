/* SPDX-License-Identifier: Apache-2.0
 * libifcs call tracer runtime (trace_rt.h). Environment:
 *   IFCS_TRACE_FILE    output (appended; default stderr)
 *   IFCS_TRACE_NESTED  1: also log IFCS calls made from inside IFCS
 * Lines are written with one write(2) each (O_APPEND), so the SAI script
 * runner's markers interleave correctly. Pointers are only read after a
 * probe write(2) of the range to a pipe succeeds (unmapped: shown as "?"). */
#define _GNU_SOURCE
#include "trace_rt.h"

#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

struct tr_line { char buf[16384]; size_t n; int nargs; };

static __thread int depth;
static __thread struct tr_line tl_line;
static void* bound;
static int fd = -2, nested = -1, probe[2] = {-1, -1};
static unsigned long seq;
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;

static void init_once(void)
{
    if (fd != -2) return;
    const char* f = getenv("IFCS_TRACE_FILE");
    fd = f && *f ? open(f, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644) : 2;
    if (fd < 0) fd = 2;
    const char* n = getenv("IFCS_TRACE_NESTED");
    nested = n && *n == '1';
    if (pipe2(probe, O_CLOEXEC | O_NONBLOCK) != 0) probe[0] = probe[1] = -1;
    char hdr[256];
    int k = snprintf(hdr, sizeof(hdr), "# ifcs-trace v1 abi=%s pid=%d\n", tr_abi.name, (int)getpid());
    if (k > 0) (void)!write(fd, hdr, (size_t)k);
}

void ifcs_trace_bind(void* h)
{
    pthread_mutex_lock(&mu);
    init_once();
    bound = h;
    pthread_mutex_unlock(&mu);
}

void ifcs_trace_mark(const char* text)
{
    pthread_mutex_lock(&mu);
    init_once();
    pthread_mutex_unlock(&mu);
    char b[1024];
    int k = snprintf(b, sizeof(b), "M %s\n", text ? text : "");
    if (k > 0) (void)!write(fd, b, (size_t)k < sizeof(b) ? (size_t)k : sizeof(b) - 1);
}

int tr_enter(void)
{
    int top = depth++ == 0;
    if (fd == -2) {
        pthread_mutex_lock(&mu);
        init_once();
        pthread_mutex_unlock(&mu);
    }
    return top || nested;
}
void tr_leave(void) { depth--; }

void* tr_resolve(const char* name)
{
    void* p = bound ? dlsym(bound, name) : dlsym(RTLD_NEXT, name);
    if (!p) {
        char b[256];
        int k = snprintf(b, sizeof(b), "ifcs-trace: no real %s\n", name);
        if (k > 0) (void)!write(2, b, (size_t)k);
        abort();
    }
    return p;
}

uint64_t tr_now_us(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000000u + (uint64_t)t.tv_nsec / 1000u;
}

/* readable: probe-write the range into a nonblocking pipe (drained after). */
static int readable(const void* p, size_t n)
{
    if (!p || n == 0) return 0;
    if ((uintptr_t)p < 4096) return 0;
    if (probe[1] < 0) return 1;
    const char* c = (const char*)p;
    char sink[4096];
    while (n) {
        size_t k = n > 4096 ? 4096 : n;
        ssize_t w = write(probe[1], c, k);
        if (w < 0) return 0;
        while (read(probe[0], sink, sizeof(sink)) > 0) {}
        if ((size_t)w == 0) return 0;
        c += w;
        n -= (size_t)w;
    }
    return 1;
}

static void put(struct tr_line* l, const char* fmt, ...) __attribute__((format(printf, 2, 3)));
static void put(struct tr_line* l, const char* fmt, ...)
{
    if (l->n >= sizeof(l->buf) - 64) return;
    va_list ap;
    va_start(ap, fmt);
    int k = vsnprintf(l->buf + l->n, sizeof(l->buf) - 64 - l->n, fmt, ap);
    va_end(ap);
    if (k > 0) l->n += (size_t)k < sizeof(l->buf) - 64 - l->n ? (size_t)k : sizeof(l->buf) - 64 - l->n;
}

static void hex(struct tr_line* l, const uint8_t* p, size_t n)
{
    while (n && p[n - 1] == 0) n--;   /* trailing zeros dropped */
    for (size_t i = 0; i < n; i++) put(l, "%02x", p[i]);
    if (!n) put(l, "0");
}

tr_line* tr_begin(const char* fn)
{
    struct tr_line* l = &tl_line;
    l->n = 0;
    l->nargs = 0;
    put(l, "%s(", fn);
    return l;
}

static uint64_t rd(const uint8_t* p, unsigned size)
{
    uint64_t v = 0;
    memcpy(&v, p, size > 8 ? 8 : size);
    return v;
}

static void elems(struct tr_line* l, const uint8_t* arr, uint32_t count, unsigned esz, int handles)
{
    uint32_t shown = count > 32 ? 32 : count;
    if (!readable(arr, (size_t)shown * esz)) { put(l, "[%u:?]", count); return; }
    put(l, "[");
    for (uint32_t i = 0; i < shown; i++) {
        uint64_t v = rd(arr + (size_t)i * esz, esz);
        if (handles) put(l, "%s0x%llx", i ? "," : "", (unsigned long long)v);
        else if (esz == 1) put(l, "%s%d", i ? "," : "", (int)(int8_t)v);
        else put(l, "%s%llu", i ? "," : "", (unsigned long long)v);
    }
    if (count > shown) put(l, ",..%u", count);
    put(l, "]");
}

/* One attribute: id, the first 16 bytes of the value (raw, both ABIs), and
 * when the bitmap names the member, its typed value. */
static void attr(struct tr_line* l, const uint8_t* a)
{
    const struct tr_abi* A = &tr_abi;
    uint32_t id;
    memcpy(&id, a, 4);
    const uint8_t* v = a + A->value_off;
    uint64_t v0 = rd(v, 8), v1 = rd(v + 8, 8);
    put(l, "{%u v=%llx", id, (unsigned long long)v0);
    if (v1) put(l, ":%llx", (unsigned long long)v1);
    int mem = -1;
    for (uint32_t i = 0; i < A->bitmap_size * 8 && i < A->n_members; i++)
        if (a[A->bitmap_off + i / 8] & (1u << (i % 8))) { mem = (int)i; break; }
    const struct tr_member* m = mem >= 0 ? &A->members[mem] : NULL;
    uint32_t cnt = (uint32_t)v0;
    const uint8_t* ptr = (const uint8_t*)(uintptr_t)v1;
    if (m) {
        put(l, " %s=", m->name);
        switch (m->kind) {
        case TRK_U: put(l, "%llu", (unsigned long long)rd(v, m->size)); break;
        case TRK_I: put(l, "%lld", (long long)(m->size == 1 ? (int8_t)v0 : m->size == 2 ? (int16_t)v0 : m->size == 4 ? (int32_t)v0 : (int64_t)v0)); break;
        case TRK_FLT: { float f; memcpy(&f, v, 4); put(l, "%g", (double)f); break; }
        case TRK_HANDLE: put(l, "0x%llx", (unsigned long long)rd(v, 4)); break;
        case TRK_MAC: put(l, "%02x:%02x:%02x:%02x:%02x:%02x", v[0], v[1], v[2], v[3], v[4], v[5]); break;
        case TRK_LIST: case TRK_HLIST: elems(l, ptr, cnt, m->elem ? m->elem : 1, m->kind == TRK_HLIST); break;
        default: hex(l, v, m->size ? m->size : 16); break;
        }
    } else if ((v0 >> 32) == 0 && cnt >= 1 && cnt <= 256 && (uintptr_t)ptr >= 0x10000 && readable(ptr, cnt)) {
        /* no bitmap (the vendor CLI and our backend leave it 0): a {count, ptr} shape is shown as bytes */
        put(l, " list?=");
        size_t n = readable(ptr, (size_t)cnt * 4) ? (size_t)cnt * 4 : cnt;
        if (n > 64) n = 64;
        hex(l, ptr, n);
    }
    put(l, "}");
}

void tr_arg(tr_line* l, int kind, uint64_t v, uint64_t count, uint32_t size)
{
    if (l->nargs++) put(l, ", ");
    const void* p = (const void*)(uintptr_t)v;
    switch (kind) {
    case TRA_U: put(l, "%u", (uint32_t)v); break;
    case TRA_I: put(l, "%d", (int32_t)v); break;
    case TRA_U64: put(l, "%llu", (unsigned long long)v); break;
    case TRA_HANDLE: put(l, "0x%x", (uint32_t)v); break;
    case TRA_PTR: put(l, p ? "ptr" : "null"); break;
    case TRA_FN: put(l, p ? "fn" : "null"); break;
    case TRA_STR:
        if (!p) put(l, "null");
        else if (readable(p, 1)) put(l, "\"%.200s\"", (const char*)p);
        else put(l, "?");
        break;
    case TRA_ATTRS: {
        uint32_t n = count == TR_SINGLE ? 1 : (uint32_t)count;
        if (!p) { put(l, "null"); break; }
        if (n == 0) { put(l, "[]"); break; }
        if (n > 64) n = 64;
        if (!readable(p, (size_t)n * tr_abi.attr_size)) { put(l, "attrs?"); break; }
        put(l, "[");
        for (uint32_t i = 0; i < n; i++) attr(l, (const uint8_t*)p + (size_t)i * tr_abi.attr_size);
        put(l, "]");
        break;
    }
    case TRA_PU: case TRA_PI: case TRA_PU64: case TRA_PHANDLE: {
        if (!p) { put(l, "null"); break; }
        unsigned esz = kind == TRA_PU64 ? 8 : 4;
        uint32_t n = count == TR_SINGLE ? 1 : (uint32_t)count;
        if (n == 0) { put(l, "[]"); break; }
        if (n > 1024) n = 1024;
        if (count == TR_SINGLE) {
            if (!readable(p, esz)) { put(l, "*?"); break; }
            uint64_t x = rd((const uint8_t*)p, esz);
            if (kind == TRA_PHANDLE) put(l, "*0x%llx", (unsigned long long)x);
            else if (kind == TRA_PI) put(l, "*%d", (int32_t)x);
            else put(l, "*%llu", (unsigned long long)x);
        } else {
            elems(l, (const uint8_t*)p, n, esz, kind == TRA_PHANDLE);
        }
        break;
    }
    case TRA_STRUCT:
        if (!p) { put(l, "null"); break; }
        if (!readable(p, size)) { put(l, "{?}"); break; }
        put(l, "{");
        hex(l, (const uint8_t*)p, size);
        put(l, "}");
        break;
    default: put(l, "%llx", (unsigned long long)v);
    }
}

static const char* reasons[] = {"SUCCESS", "UNINIT", "BUSY", "CONFIG", "MEMORY", "PARAM", "EXIST", "MULTI_EXIST",
                                "ENTRY_REPLACED", "LENGTH_MISMATCH", "NOTFOUND", "INVAL", "UNSUPPORTED",
                                "RESOURCE_FULL", "TIMEOUT", "DEVICE_ACCESS_ERROR", "DRIVER_ERROR"};

void tr_end(tr_line* l, int status_ret, uint64_t ret, uint64_t dt_us)
{
    if (status_ret == TRR_STATUS) {
        unsigned r = (unsigned)ret & 31u;
        put(l, ") = %s", r <= 16 ? reasons[r] : "?");
    } else if (status_ret == TRR_PTR) {
        put(l, ") = %s", ret ? "ptr" : "null");
    } else if (status_ret == TRR_VOID) {
        put(l, ")");
    } else {
        put(l, ") = %llu", (unsigned long long)(uint32_t)ret);
    }
    char head[96];
    pthread_mutex_lock(&mu);
    unsigned long s = ++seq;
    pthread_mutex_unlock(&mu);
    int k = snprintf(head, sizeof(head), "T %lu %ld %llu %llu %d ", s, (long)syscall(SYS_gettid),
                     (unsigned long long)tr_now_us(), (unsigned long long)dt_us, depth - 1);
    char out[sizeof(l->buf) + 128];
    size_t n = 0;
    if (k > 0) { memcpy(out, head, (size_t)k); n = (size_t)k; }
    memcpy(out + n, l->buf, l->n);
    n += l->n;
    out[n++] = '\n';
    (void)!write(fd, out, n);
}
