/* SPDX-License-Identifier: Apache-2.0
 * The parity harness's byte tap (crate sai-teralynx-parity, README "Parity"): the runtime of
 * libparity-bytes.so, whose wrappers are sai-teralynx's oracle tracer wrappers (oracle/gen_tracer.py
 * --header over the generated ifcs_api.h: one typed wrapper per IFCS_SYMBOLS entry, bound to the
 * real libifcs by ifcs_trace_bind; both libsais load it as SAI_TERALYNX_IFCS_TRACE and take every
 * IFCS function from it). It implements oracle/trace_rt.h like trace_rt.c, but for comparison
 * rather than reading: every top-level IFCS call becomes one line
 *     @B <function>(<arguments>) = <status>
 * appended to the tap's stream (PARITY_TAP_FILE, one write(2) per line, O_APPEND, as tap.c), in which
 *   - a struct (passed by value, or the struct a pointer argument points at, after the call) is
 *     ALL its bytes in hex: padding and union bytes included, nothing trimmed;
 *   - an attribute list is each attribute's bytes in hex (id, the padding after it, the value
 *     union, the bitmap), each 8-byte-aligned word of the value that holds an address of this
 *     process (a list member's buffer) shown as <p> (addresses differ between processes; what they
 *     point at is the mock's call log's business);
 *   - scalars, handles, out-values and counted arrays as trace_rt.c prints them; pointers as
 *     ptr / fn / null; no thread ids, sequence numbers or times.
 * So a byte libifcs receives that differs between the C++ and the Rust libsai (a padding byte the
 * mock ignores because it compares fields) makes the two streams differ.
 * Environment: PARITY_TAP_FILE (the stream; unset: nothing written). Nothing here allocates. */
#define _GNU_SOURCE
#include "trace_rt.h"

#include <dlfcn.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

struct tr_line { char buf[65536]; size_t n; int nargs; };

static __thread int depth;
static __thread struct tr_line tl_line;
static void* bound;
static int fd = -2, probe[2] = {-1, -1};
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;

static void init_locked(void)
{
    if (fd != -2) return;
    const char* f = getenv("PARITY_TAP_FILE");
    fd = f && *f ? open(f, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644) : -1;
    if (pipe2(probe, O_CLOEXEC | O_NONBLOCK) != 0) probe[0] = probe[1] = -1;
}

static void init(void)
{
    pthread_mutex_lock(&mu);
    init_locked();
    pthread_mutex_unlock(&mu);
}

void ifcs_trace_bind(void* h)
{
    pthread_mutex_lock(&mu);
    init_locked();
    bound = h;
    pthread_mutex_unlock(&mu);
}

void ifcs_trace_mark(const char* text) { (void)text; init(); }   /* the markers are tap.c's */

int tr_enter(void)
{
    if (fd == -2) init();
    return depth++ == 0;
}
void tr_leave(void) { depth--; }

void* tr_resolve(const char* name)
{
    void* p = bound ? dlsym(bound, name) : dlsym(RTLD_NEXT, name);
    if (!p) {
        char b[256];
        int k = snprintf(b, sizeof(b), "parity-bytes: no real %s\n", name);
        if (k > 0) (void)!write(2, b, (size_t)k);
        abort();
    }
    return p;
}

uint64_t tr_now_us(void) { return 0; }

/* readable: probe-write the range into a nonblocking pipe (drained after), as trace_rt.c */
static int readable(const void* p, size_t n)
{
    if (!p || n == 0 || (uintptr_t)p < 4096) return 0;
    if (probe[1] < 0) return 1;
    const char* c = (const char*)p;
    char sink[4096];
    while (n) {
        size_t k = n > 4096 ? 4096 : n;
        ssize_t w = write(probe[1], c, k);
        if (w <= 0) return 0;
        while (read(probe[0], sink, sizeof(sink)) > 0) {}
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
    for (size_t i = 0; i < n; i++) put(l, "%02x", p[i]);
}

tr_line* tr_begin(const char* fn)
{
    struct tr_line* l = &tl_line;
    l->n = 0;
    l->nargs = 0;
    put(l, "@B %s(", fn);
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
    uint32_t shown = count > 64 ? 64 : count;
    if (!readable(arr, (size_t)shown * esz)) { put(l, "[%u:?]", count); return; }
    put(l, "[");
    for (uint32_t i = 0; i < shown; i++) {
        uint64_t v = rd(arr + (size_t)i * esz, esz);
        if (handles) put(l, "%s0x%llx", i ? "," : "", (unsigned long long)v);
        else put(l, "%s%llu", i ? "," : "", (unsigned long long)v);
    }
    if (count > shown) put(l, ",..%u", count);
    put(l, "]");
}

/* an address of this process: a user-space value that is mapped */
static int address(uint64_t v)
{
    return v >= 0x10000 && v < 0x0001000000000000ull && readable((const void*)(uintptr_t)v, 1);
}

static void attr(struct tr_line* l, const uint8_t* a)
{
    const struct tr_abi* A = &tr_abi;
    put(l, "{");
    hex(l, a, A->value_off);
    put(l, "|");
    for (uint32_t o = 0; o < A->value_size; o += 8) {
        uint32_t n = A->value_size - o < 8 ? A->value_size - o : 8;
        if (n == 8 && address(rd(a + A->value_off + o, 8))) put(l, "<p>");
        else hex(l, a + A->value_off + o, n);
    }
    put(l, "|");
    hex(l, a + A->value_off + A->value_size, A->attr_size - A->value_off - A->value_size);
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
        if (n > 256) n = 256;
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
    default: put(l, "?%d", kind);
    }
}

static const char* reasons[] = {"SUCCESS", "UNINIT", "BUSY", "CONFIG", "MEMORY", "PARAM", "EXIST", "MULTI_EXIST",
                                "ENTRY_REPLACED", "LENGTH_MISMATCH", "NOTFOUND", "INVAL", "UNSUPPORTED",
                                "RESOURCE_FULL", "TIMEOUT", "DEVICE_ACCESS_ERROR", "DRIVER_ERROR"};

void tr_end(tr_line* l, int status_ret, uint64_t ret, uint64_t dt_us)
{
    (void)dt_us;
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
    l->buf[l->n++] = '\n';
    if (fd >= 0) (void)!write(fd, l->buf, l->n);
}
