// SPDX-License-Identifier: Apache-2.0
// IFCS learn-ring read guard, part of libsai-teralynx-shim.so (see shim.c).
//
// libifcs.so 0.12.41's learn thread (_ii_pcie_rupt_thread_learn) polls three
// BAR0 registers about once a second through node_pci_reg_read32@plt:
// 0x4864 and 0x485c (read, then written back with its consumer state) and
// 0x4860, the learn ring's producer index B (bits 0..11 slot, bit 12 phase).
// It trusts B: n = size + B - A (same phase) or size - (B & 0xfff) + (A & 0xfff),
// im_malloc(n * 8) (malloc + memset), then "while (A != B)" over its ring.
// A read the ASIC does not answer (PCIe completion timeout: all-ones) gives
// n = 0xFFFFF801, a 32 GiB malloc + memset and a loop that never ends
// (ai_re/devices/cs8210-32x/learn-wedge.md).
//
// ifcs_lib.cpp points libifcs' GOT entries for node_pci_reg_read32,
// node_pci_reg_write32 and im_malloc at the functions below:
//  * reads of 0x4860 / 0x4864 / 0x485c are checked (0x4860: only bits 0..12
//    and slot < ring size; the others: not all-ones). An invalid value is
//    re-read up to 3 times, 3 ms apart; if it stays invalid the guard returns
//    "nothing new" (the consumer index the thread last wrote to 0x4864, else
//    the last valid value; 0 before any) so this poll is skipped, never a
//    garbage count. While a register is substituted it is not re-read again
//    (the thread's own 1 Hz read is the probe): unanswered reads stall the
//    SoC. Every invalid read is logged (stderr: syncd's journal) and counted,
//    by return code.
//  * every other register read of libifcs passes too: all-ones results are
//    counted (no retry, no change: some registers are legitimately all-ones),
//    the first 16 offsets each with its own counter and one log line.
//  * writes of 0x4864 are passed through and remembered (that consumer index).
//  * im_malloc(n) with n above a limit (default 1 GiB) is refused (NULL,
//    logged, counted) instead of malloc + memset of garbage sizes; else calloc.
// Every other register access goes straight to the real function.
// Counters: <run dir>/ifcs-guard.prom (Prometheus text, rewritten on every
// event and every 30 s while the thread polls), read by the CS8210 metrics.
#include <errno.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

typedef int (*tl_reg_read32_fn)(uint32_t node, int32_t off, uint32_t* val);
typedef int (*tl_reg_write32_fn)(uint32_t node, int32_t off, uint32_t val);

enum { R4864, R485C, R4860, NREG };
static const char* const reg_name[NREG] = {"0x4864", "0x485c", "0x4860"};

static struct {
    tl_reg_read32_fn rd;
    tl_reg_write32_fn wr;
    uint32_t ring_size;
    uint64_t malloc_limit;
    int retries;
    char stats[512];
    char inject[512];   // test hook: SAI_TERALYNX_IFCS_GUARD_INJECT (file "<reg> <count>")
} G = {NULL, NULL, 0x800, 1ull << 30, 3, "", ""};

struct reg_state {
    uint64_t reads, invalid, invalid_rc, recovered, substituted;
    uint32_t good;
    int have_good;
    int degraded;        // last poll was substituted: no re-reads until a valid read
    unsigned startup_bad; // invalid rc-0 values < 0x2000 before any valid one (ring size misconfigured?)
};
#define NALL 16
static struct { int32_t off; uint64_t n; } allones[NALL];
static uint64_t allones_other;
static struct reg_state S[NREG];
static uint32_t cidx_written;   // bits 0..12 of the last 0x4864 write (the thread's consumer index)
static int have_cidx;
static uint64_t malloc_refused, malloc_refused_max;
static double last_invalid;     // CLOCK_REALTIME of the last invalid read
static int installed;
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static double next_write_a;   // atomically read outside mu (bit copy of a double)
static double next_inject_check, log_window;
static int log_lines;
static unsigned log_suppressed;
static int inject_reg = -1;
static uint32_t inject_left;

static double now_mono(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + t.tv_nsec / 1e9;
}
static double now_real(void)
{
    struct timespec t;
    clock_gettime(CLOCK_REALTIME, &t);
    return (double)t.tv_sec + t.tv_nsec / 1e9;
}

static int reg_index(int32_t off)
{
    switch (off) {
    case 0x4864: return R4864;
    case 0x485c: return R485C;
    case 0x4860: return R4860;
    default: return -1;
    }
}

static int valid(int i, uint32_t v)
{
    if (i == R4860) return (v & ~0x1fffu) == 0 && (v & 0xfffu) < G.ring_size;
    return v != 0xffffffffu;
}

// Prometheus text of the counters (caller holds mu).
static int render(char* b, size_t cap)
{
    size_t n = 0;
#define P(...) do { int k = snprintf(b + n, n < cap ? cap - n : 0, __VA_ARGS__); if (k > 0) n += (size_t)k; } while (0)
    P("# HELP sai_teralynx_ifcs_guard_installed libifcs' learn-ring register reads go through the guard (1) or not (0)\n");
    P("# TYPE sai_teralynx_ifcs_guard_installed gauge\nsai_teralynx_ifcs_guard_installed %d\n", installed);
    static const char* const what[5] = {"reads", "invalid", "invalid_rc", "recovered", "substituted"};
    static const char* const help[5] = {
        "learn-ring register reads by libifcs",
        "reads that returned an invalid value (all-ones / out of range: a PCIe read the ASIC did not answer)",
        "invalid reads where node_pci_reg_read32 itself returned an error (an SDK path, not the link)",
        "invalid reads where a re-read gave a valid value",
        "invalid reads still invalid after the re-reads (or while degraded): that learn poll was skipped"};
    for (int w = 0; w < 5; w++) {
        P("# HELP sai_teralynx_ifcs_guard_%s_total %s\n# TYPE sai_teralynx_ifcs_guard_%s_total counter\n", what[w], help[w], what[w]);
        for (int i = 0; i < NREG; i++) {
            uint64_t v = w == 0 ? __atomic_load_n(&S[i].reads, __ATOMIC_RELAXED) : w == 1 ? S[i].invalid
                       : w == 2 ? S[i].invalid_rc : w == 3 ? S[i].recovered : S[i].substituted;
            P("sai_teralynx_ifcs_guard_%s_total{reg=\"%s\"} %llu\n", what[w], reg_name[i], (unsigned long long)v);
        }
    }
    P("# HELP sai_teralynx_ifcs_guard_allones_reads_total libifcs register reads (any offset) that returned 0xffffffff\n");
    P("# TYPE sai_teralynx_ifcs_guard_allones_reads_total counter\n");
    for (int i = 0; i < NALL && allones[i].n; i++)
        P("sai_teralynx_ifcs_guard_allones_reads_total{reg=\"0x%x\"} %llu\n", (unsigned)allones[i].off,
          (unsigned long long)allones[i].n);
    P("sai_teralynx_ifcs_guard_allones_reads_total{reg=\"other\"} %llu\n", (unsigned long long)allones_other);
    P("# HELP sai_teralynx_ifcs_guard_last_invalid_timestamp_seconds time of the last invalid read (0: none)\n");
    P("# TYPE sai_teralynx_ifcs_guard_last_invalid_timestamp_seconds gauge\nsai_teralynx_ifcs_guard_last_invalid_timestamp_seconds %.3f\n", last_invalid);
    P("# HELP sai_teralynx_ifcs_guard_im_malloc_refused_total im_malloc requests above the limit, refused\n");
    P("# TYPE sai_teralynx_ifcs_guard_im_malloc_refused_total counter\nsai_teralynx_ifcs_guard_im_malloc_refused_total %llu\n",
      (unsigned long long)malloc_refused);
    P("# HELP sai_teralynx_ifcs_guard_im_malloc_refused_max_bytes largest refused im_malloc request\n");
    P("# TYPE sai_teralynx_ifcs_guard_im_malloc_refused_max_bytes gauge\nsai_teralynx_ifcs_guard_im_malloc_refused_max_bytes %llu\n",
      (unsigned long long)malloc_refused_max);
    P("# HELP sai_teralynx_ifcs_guard_updated_timestamp_seconds when the guard last wrote this file\n");
    P("# TYPE sai_teralynx_ifcs_guard_updated_timestamp_seconds gauge\nsai_teralynx_ifcs_guard_updated_timestamp_seconds %.3f\n", now_real());
#undef P
    return n < cap ? (int)n : -1;
}

static void write_stats_locked(void)
{
    if (!G.stats[0]) return;
    char b[4096], tmp[600];
    int n = render(b, sizeof(b));
    if (n < 0) return;
    snprintf(tmp, sizeof(tmp), "%s.tmp", G.stats);
    FILE* f = fopen(tmp, "w");
    if (!f) return;
    int ok = fwrite(b, 1, (size_t)n, f) == (size_t)n;
    ok = (fclose(f) == 0) && ok;
    if (ok) rename(tmp, G.stats);
    else unlink(tmp);
    __atomic_store(&next_write_a, &(double){now_mono() + 30}, __ATOMIC_RELAXED);
}

// stderr, at most 20 lines per 10 minutes (a dead chip must not flood the journal).
// Formatted under mu into a pending buffer, written by flush_say() after unlock,
// so a stalled journal never blocks the learn thread while it holds mu.
static char pending[2048];
static size_t pending_len;
static void say_locked(const char* fmt, ...) __attribute__((format(printf, 1, 2)));
static void say_locked(const char* fmt, ...)
{
    double t = now_mono();
    if (t >= log_window) {
        if (log_suppressed && pending_len < sizeof(pending))
            pending_len += (size_t)snprintf(pending + pending_len, sizeof(pending) - pending_len,
                                            "sai-teralynx: ifcs guard: %u more events not logged\n", log_suppressed);
        log_window = t + 600;
        log_lines = 0;
        log_suppressed = 0;
    }
    if (++log_lines > 20) { log_suppressed++; return; }
    if (pending_len >= sizeof(pending) - 1) return;
    va_list ap;
    va_start(ap, fmt);
    int k = snprintf(pending + pending_len, sizeof(pending) - pending_len, "sai-teralynx: ifcs guard: ");
    if (k > 0) pending_len += (size_t)k;
    if (pending_len < sizeof(pending)) {
        k = vsnprintf(pending + pending_len, sizeof(pending) - pending_len, fmt, ap);
        if (k > 0) pending_len += (size_t)k;
    }
    if (pending_len > sizeof(pending) - 2) pending_len = sizeof(pending) - 2;
    pending[pending_len++] = '\n';
    pending[pending_len] = 0;
    va_end(ap);
}
// Caller holds mu: take the pending text out; print it after unlocking.
static void take_pending_locked(char* out, size_t cap)
{
    size_t n = pending_len < cap - 1 ? pending_len : cap - 1;
    memcpy(out, pending, n);
    out[n] = 0;
    pending_len = 0;
}
static void print_out(const char* s)
{
    if (s[0]) fputs(s, stderr);
}
static void sleep_ms(int ms)
{
    struct timespec t = {0, ms * 1000000L};
    while (nanosleep(&t, &t) != 0 && errno == EINTR) {}
}

// Test hook: with SAI_TERALYNX_IFCS_GUARD_INJECT=<file> set, the file
// "<reg> <count>" (checked at most once a second, then removed) makes the
// next <count> raw reads of that register return 0xffffffff.
static void check_inject_locked(void)
{
    double t = now_mono();
    if (t < next_inject_check) return;
    next_inject_check = t + 1;
    FILE* f = fopen(G.inject, "r");
    if (!f) return;
    unsigned off = 0, cnt = 0;
    int got = fscanf(f, "%x %u", &off, &cnt);
    fclose(f);
    unlink(G.inject);
    int i = reg_index((int32_t)off);
    if (got == 2 && i >= 0) {
        inject_left = cnt;
        __atomic_store_n(&inject_reg, i, __ATOMIC_RELAXED);
        say_locked("TEST: next %u reads of %s return 0xffffffff (injected)", cnt, reg_name[i]);
    }
}

static int raw_read(int i, uint32_t node, int32_t off, uint32_t* val)
{
    int rc = G.rd(node, off, val);
    if (__atomic_load_n(&inject_reg, __ATOMIC_RELAXED) == i) {
        pthread_mutex_lock(&mu);
        if (inject_reg == i && inject_left > 0) {
            inject_left--;
            *val = 0xffffffffu;
            rc = 0;
        }
        pthread_mutex_unlock(&mu);
    }
    return rc;
}

// All-ones from any other offset: counted (first NALL offsets by name), logged once per new offset.
static void count_allones(int32_t off)
{
    char out[512];
    pthread_mutex_lock(&mu);
    int i;
    for (i = 0; i < NALL && allones[i].n; i++)
        if (allones[i].off == off) break;
    if (i < NALL) {
        if (!allones[i].n) {
            allones[i].off = off;
            say_locked("register 0x%x read 0xffffffff (first time; counted, not changed)", (unsigned)off);
        }
        allones[i].n++;
    } else {
        allones_other++;
    }
    take_pending_locked(out, sizeof(out));
    pthread_mutex_unlock(&mu);
    print_out(out);
}

int tl_ifcs_reg_read32(uint32_t node, int32_t off, uint32_t* val)
{
    int i = reg_index(off);
    if (i < 0 || !val) {
        int rc = G.rd(node, off, val);
        if (rc == 0 && val && *val == 0xffffffffu) count_allones(off);
        return rc;
    }
    char out[2048];
    if (G.inject[0]) {
        pthread_mutex_lock(&mu);
        check_inject_locked();
        take_pending_locked(out, sizeof(out));
        pthread_mutex_unlock(&mu);
        print_out(out);
    }
    *val = 0xffffffffu;   // node_pci_reg_read32 leaves it untouched on failure; the thread ignores rc
    int rc = raw_read(i, node, off, val);
    __atomic_add_fetch(&S[i].reads, 1, __ATOMIC_RELAXED);
    if (rc == 0 && valid(i, *val)) {
        __atomic_store_n(&S[i].good, *val, __ATOMIC_RELAXED);
        __atomic_store_n(&S[i].have_good, 1, __ATOMIC_RELAXED);
        if (__atomic_load_n(&S[i].degraded, __ATOMIC_RELAXED)) {
            pthread_mutex_lock(&mu);
            S[i].degraded = 0;
            say_locked("%s valid again (0x%08x) after %llu substituted polls", reg_name[i], *val,
                       (unsigned long long)S[i].substituted);
            write_stats_locked();
            take_pending_locked(out, sizeof(out));
            pthread_mutex_unlock(&mu);
            print_out(out);
            return 0;
        }
        double nw;
        __atomic_load(&next_write_a, &nw, __ATOMIC_RELAXED);
        if (G.stats[0] && now_mono() >= nw) {
            pthread_mutex_lock(&mu);
            __atomic_load(&next_write_a, &nw, __ATOMIC_RELAXED);
            if (now_mono() >= nw) write_stats_locked();
            pthread_mutex_unlock(&mu);
        }
        return 0;
    }
    uint32_t first = *val;
    int first_rc = rc, tries = 0, ok = 0;
    // Re-read only while not degraded: a register that stayed bad last poll is probed by
    // the thread's own next read, not hammered (each unanswered read can stall the SoC).
    if (!__atomic_load_n(&S[i].degraded, __ATOMIC_RELAXED)) {
        for (tries = 1; tries <= G.retries; tries++) {
            sleep_ms(3);   // past a short link recovery
            *val = 0xffffffffu;
            rc = raw_read(i, node, off, val);
            if (rc == 0 && valid(i, *val)) { ok = 1; break; }
        }
        if (!ok) tries = G.retries;
    }
    pthread_mutex_lock(&mu);
    S[i].invalid++;
    if (first_rc != 0) S[i].invalid_rc++;
    last_invalid = now_real();
    if (ok) {
        S[i].recovered++;
        __atomic_store_n(&S[i].good, *val, __ATOMIC_RELAXED);
        __atomic_store_n(&S[i].have_good, 1, __ATOMIC_RELAXED);
        say_locked("%s read 0x%08x (rc %d), re-read %d gave 0x%08x: recovered (invalid %llu, recovered %llu, substituted %llu)",
                   reg_name[i], first, first_rc, tries, *val, (unsigned long long)S[i].invalid,
                   (unsigned long long)S[i].recovered, (unsigned long long)S[i].substituted);
    } else {
        int was_degraded = S[i].degraded;
        S[i].substituted++;
        S[i].degraded = 1;
        const char* from;
        if (i == R4860 && have_cidx) { *val = cidx_written; from = "the consumer index last written to 0x4864"; }
        else if (S[i].have_good) { *val = S[i].good; from = "the last valid value"; }
        else { *val = 0; from = "0 (no valid value yet)"; }
        rc = 0;
        if (i == R4860 && !S[i].have_good && first_rc == 0 && first < 0x2000 && ++S[i].startup_bad == 10)
            say_locked("0x4860 invalid on the first 10 reads with indexes below 0x2000 (e.g. 0x%08x): is the learn "
                       "ring size (SAI_TERALYNX_LEARN_RING_SIZE = %u) wrong? learning is stopped", first, G.ring_size);
        if (!was_degraded)
            say_locked("%s read 0x%08x (rc %d), still invalid after %d re-reads: %s, returned 0x%08x = %s; "
                       "no re-reads until it is valid again (invalid %llu, recovered %llu, substituted %llu)",
                       reg_name[i], first, first_rc, tries, i == R4860 ? "learn poll skipped" : "write-back keeps its bits",
                       *val, from, (unsigned long long)S[i].invalid, (unsigned long long)S[i].recovered,
                       (unsigned long long)S[i].substituted);
    }
    write_stats_locked();
    take_pending_locked(out, sizeof(out));
    pthread_mutex_unlock(&mu);
    print_out(out);
    return rc;
}

int tl_ifcs_reg_write32(uint32_t node, int32_t off, uint32_t val)
{
    if (off == 0x4864) {
        pthread_mutex_lock(&mu);
        cidx_written = val & 0x1fffu;
        have_cidx = 1;
        pthread_mutex_unlock(&mu);
    }
    return G.wr(node, off, val);
}

void* tl_ifcs_im_malloc(size_t n)
{
    if (G.malloc_limit && n > G.malloc_limit) {
        char out[1024];
        pthread_mutex_lock(&mu);
        malloc_refused++;
        if (n > malloc_refused_max) malloc_refused_max = n;
        say_locked("im_malloc(%zu) refused (limit %llu bytes; caller %p): returning NULL", n,
                   (unsigned long long)G.malloc_limit, __builtin_return_address(0));
        write_stats_locked();
        take_pending_locked(out, sizeof(out));
        pthread_mutex_unlock(&mu);
        print_out(out);
        errno = ENOMEM;
        return NULL;
    }
    return calloc(1, n);   // im_malloc is malloc + memset(0); calloc is the same contract
}

// ifcs_lib.cpp: before it redirects the GOT entries. ring_size 0: 0x800.
int tl_ifcs_guard_init(void* rd, void* wr, const char* stats, unsigned ring_size, unsigned long long malloc_limit)
{
    if (!rd || !wr) return -1;
    pthread_mutex_lock(&mu);
    G.rd = (tl_reg_read32_fn)rd;
    G.wr = (tl_reg_write32_fn)wr;
    G.ring_size = ring_size ? ring_size : 0x800;
    G.malloc_limit = malloc_limit;
    snprintf(G.stats, sizeof(G.stats), "%s", stats ? stats : "");
    const char* inj = getenv("SAI_TERALYNX_IFCS_GUARD_INJECT");
    snprintf(G.inject, sizeof(G.inject), "%s", inj ? inj : "");
    memset(S, 0, sizeof(S));
    memset(allones, 0, sizeof(allones));
    allones_other = 0;
    have_cidx = 0;
    malloc_refused = malloc_refused_max = 0;
    last_invalid = 0;
    __atomic_store_n(&inject_reg, -1, __ATOMIC_RELAXED);
    inject_left = 0;
    pthread_mutex_unlock(&mu);
    return 0;
}

// ifcs_lib.cpp: after the redirect (1 = read32 redirected, the guard is live).
void tl_ifcs_guard_installed(int on)
{
    pthread_mutex_lock(&mu);
    installed = on;
    write_stats_locked();
    pthread_mutex_unlock(&mu);
}

// The counters as Prometheus text (tests; static buffer, not thread-safe across callers).
const char* tl_ifcs_guard_text(void)
{
    static char b[4096];
    pthread_mutex_lock(&mu);
    if (render(b, sizeof(b)) < 0) b[0] = 0;
    pthread_mutex_unlock(&mu);
    return b;
}
