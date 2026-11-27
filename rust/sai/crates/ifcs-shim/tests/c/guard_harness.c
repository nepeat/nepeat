// SPDX-License-Identifier: Apache-2.0
// Harness of libsai-teralynx-shim.so (tests/differential.rs): runs one script of
// register reads / writes / im_malloc calls / inject files / clock steps against the shim library
// (the Rust cdylib of crates/ifcs-shim) and prints a trace; the test compares the trace with the golden
// tests/golden/guard-traces-cpp.txt, captured with this harness from the C library (shim.c +
// ifcs_guard.c of the C++ sai-teralynx, deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0).
//
//   guard_harness <libsai-teralynx-shim.so> <script> <work dir>
//
// The process stands in for libifcs and the kernel: the original accessors are functions here
// (scripted answers, every call recorded); clock_gettime and nanosleep are defined here, so both
// libraries see the script's clock and their sleeps are recorded (and advance the clock) instead of
// slept; stderr (fd 2) is a file read back after every step; malloc / calloc / realloc / free /
// posix_memalign / aligned_alloc / memalign and __cxa_thread_atexit_impl are counted while a step
// runs (into <work dir>/counts.txt: the test checks that the Rust library registers no thread-local destructor).
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

// ---- interposed libc: the clock, sleeps, allocation and TLS-destructor counters
static double t_mono = 1000.0, t_real = 1764000000.25;
static int counting;                    // count only inside a step
static unsigned long n_alloc, n_tlsdtor;
static char sleeps[256];
static size_t sleeps_len;

static void set_ts(struct timespec* ts, double t)
{
    ts->tv_sec = (time_t)t;
    ts->tv_nsec = (long)((t - (double)ts->tv_sec) * 1e9 + 0.5);
    if (ts->tv_nsec >= 1000000000L) { ts->tv_sec++; ts->tv_nsec -= 1000000000L; }
}
int clock_gettime(clockid_t clk, struct timespec* ts)
{
    if (clk == CLOCK_MONOTONIC) { set_ts(ts, t_mono); return 0; }
    if (clk == CLOCK_REALTIME) { set_ts(ts, t_real); return 0; }
    return (int)syscall(SYS_clock_gettime, clk, ts);
}
int nanosleep(const struct timespec* req, struct timespec* rem)
{
    (void)rem;
    double s = (double)req->tv_sec + (double)req->tv_nsec / 1e9;
    if (sleeps_len < sizeof(sleeps) - 32)
        sleeps_len += (size_t)snprintf(sleeps + sleeps_len, sizeof(sleeps) - sleeps_len, "%s%ld.%09ld",
                                       sleeps_len ? "," : "", (long)req->tv_sec, req->tv_nsec);
    t_mono += s;
    t_real += s;
    return 0;
}
int clock_nanosleep(clockid_t clk, int flags, const struct timespec* req, struct timespec* rem)
{
    (void)clk; (void)flags;
    return nanosleep(req, rem);
}
extern void* __libc_malloc(size_t);
extern void* __libc_calloc(size_t, size_t);
extern void* __libc_realloc(void*, size_t);
extern void __libc_free(void*);
extern void* __libc_memalign(size_t, size_t);
void* malloc(size_t n) { if (counting) n_alloc++; return __libc_malloc(n); }
void* calloc(size_t a, size_t b) { if (counting) n_alloc++; return __libc_calloc(a, b); }
void* realloc(void* p, size_t n) { if (counting) n_alloc++; return __libc_realloc(p, n); }
void free(void* p) { __libc_free(p); }
void* memalign(size_t a, size_t n) { if (counting) n_alloc++; return __libc_memalign(a, n); }
void* aligned_alloc(size_t a, size_t n) { if (counting) n_alloc++; return __libc_memalign(a, n); }
int posix_memalign(void** p, size_t a, size_t n)
{
    if (counting) n_alloc++;
    *p = __libc_memalign(a, n);
    return *p ? 0 : ENOMEM;
}
int __cxa_thread_atexit_impl(void (*dtor)(void*), void* obj, void* dso)
{
    n_tlsdtor++;
    int (*real)(void (*)(void*), void*, void*) =
        (int (*)(void (*)(void*), void*, void*))dlsym(RTLD_NEXT, "__cxa_thread_atexit_impl");
    return real ? real(dtor, obj, dso) : 0;
}

// ---- the original accessors: registers with scripted answers
#define NREG 64
#define NANS 64
static struct reg {
    int32_t off;
    uint32_t val;
    int nans, head;
    int rc[NANS];
    uint32_t v[NANS];
} regs[NREG];
static int nregs;
static char calls[4096];
static size_t calls_len;
static int wr_rc;

static struct reg* reg(int32_t off)
{
    for (int i = 0; i < nregs; i++)
        if (regs[i].off == off) return &regs[i];
    if (nregs == NREG) { fprintf(stdout, "harness: too many registers\n"); exit(3); }
    regs[nregs].off = off;
    return &regs[nregs++];
}
static void note(const char* fmt, ...) __attribute__((format(printf, 1, 2)));
static void note(const char* fmt, ...)
{
    if (calls_len >= sizeof(calls) - 64) return;
    va_list ap;
    va_start(ap, fmt);
    calls_len += (size_t)vsnprintf(calls + calls_len, sizeof(calls) - calls_len, fmt, ap);
    va_end(ap);
}
static int h_read(uint32_t node, int32_t off, uint32_t* val)
{
    struct reg* r = reg(off);
    int rc = 0;
    uint32_t v = r->val;
    if (r->head < r->nans) { rc = r->rc[r->head]; v = r->v[r->head]; r->head++; }
    note("R%x%s:%d ", (unsigned)off, val ? "" : "(null)", rc);
    (void)node;
    if (rc == 0 && val) *val = v;
    return rc;
}
static int h_write(uint32_t node, int32_t off, uint32_t val)
{
    (void)node;
    note("W%x=%x ", (unsigned)off, val);
    reg(off)->val = val;
    return wr_rc;
}

// ---- the library under test
static int (*g_init)(void*, void*, const char*, unsigned, unsigned long long);
static void (*g_installed)(int);
static int (*g_rd)(uint32_t, int32_t, uint32_t*);
static int (*g_wr)(uint32_t, int32_t, uint32_t);
static void* (*g_malloc)(size_t);
static const char* (*g_text)(void);
static void* (*g_memcpy)(void*, const void*, size_t);

// the im_malloc call site: the library logs its caller, which must be in here
__attribute__((noinline)) static void* do_malloc(size_t n)
{
    void* p = g_malloc(n);
    __asm__ volatile("" ::: "memory");
    return p;
}

static char work[512], stats_path[600], inject_path[600];
static int errfd;
static off_t err_seen;
static char last_prom[8192];
static int have_last_prom;

static void print_escaped(const char* tag, const char* s, size_t n)
{
    fputs(tag, stdout);
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c == '\n') fputs("\\n", stdout);
        else if (c == '\\') fputs("\\\\", stdout);
        else if (c < 0x20 || c >= 0x7f) fprintf(stdout, "\\x%02x", c);
        else fputc(c, stdout);
    }
    fputc('\n', stdout);
}

// "caller 0x<hex>)" -> "caller <in do_malloc>)" when the address is inside do_malloc
static void mask_caller(char* s, size_t* n)
{
    char* p = strstr(s, "caller 0x");
    while (p) {
        char* h = p + 7;
        char* e;
        uintptr_t a = (uintptr_t)strtoull(h, &e, 16);
        uintptr_t f = (uintptr_t)(void*)do_malloc;
        const char* rep = (a > f && a < f + 0x400) ? "<in do_malloc>" : "<NOT in do_malloc>";
        size_t rl = strlen(rep), tail = strlen(e);
        memmove(h + rl, e, tail + 1);
        memcpy(h, rep, rl);
        *n = strlen(s);
        p = strstr(h, "caller 0x");
    }
}

static void flush_stderr(void)
{
    char buf[16384];
    off_t end = lseek(errfd, 0, SEEK_END);
    size_t n = 0;
    if (end > err_seen) {
        size_t want = (size_t)(end - err_seen);
        if (want > sizeof(buf) - 1) want = sizeof(buf) - 1;
        ssize_t k = pread(errfd, buf, want, err_seen);
        if (k > 0) n = (size_t)k;
        err_seen = end;
    }
    buf[n] = 0;
    if (n) {
        mask_caller(buf, &n);
        print_escaped("  log ", buf, n);
    }
}

static void show_prom(void)
{
    char buf[8192];
    int fd = open(stats_path, O_RDONLY);
    if (fd < 0) {
        if (have_last_prom) fputs("  prom gone\n", stdout);
        have_last_prom = 0;
        return;
    }
    ssize_t k = read(fd, buf, sizeof(buf) - 1);
    close(fd);
    if (k < 0) k = 0;
    buf[k] = 0;
    if (!have_last_prom || strcmp(buf, last_prom) != 0) print_escaped("  prom ", buf, (size_t)k);
    memcpy(last_prom, buf, (size_t)k + 1);
    have_last_prom = 1;
}

// the step's common trailer: calls made, sleeps, stderr, the stats file, the inject file
static void end_step(void)
{
    counting = 0;
    if (calls_len) fprintf(stdout, "  calls %s\n", calls);
    if (sleeps_len) fprintf(stdout, "  sleeps %s\n", sleeps);
    calls_len = 0; calls[0] = 0;
    sleeps_len = 0; sleeps[0] = 0;
    flush_stderr();
    show_prom();
    if (inject_path[0] && access(inject_path, F_OK) == 0) fputs("  inject file present\n", stdout);
}
static void begin_step(void)
{
    n_alloc = 0;
    counting = 1;
}
static unsigned long alloc_total, tls_total;
static unsigned long cur_line;   // of the script
static FILE* counts;   // <work dir>/counts.txt: the libraries may differ there, so not in the trace
static void count_step(const char* what)
{
    fprintf(counts, "%s alloc %lu tlsdtor %lu line %lu\n", what, n_alloc, n_tlsdtor, cur_line);
    alloc_total += n_alloc;
    tls_total += n_tlsdtor;
    n_tlsdtor = 0;
}

static void unescape(char* s)
{
    char* o = s;
    for (char* p = s; *p; p++) {
        if (*p == '\\' && p[1] == 'n') { *o++ = '\n'; p++; }
        else if (*p == '\\' && p[1] == 't') { *o++ = '\t'; p++; }
        else if (*p == '\\' && p[1] == 'x' && p[2] && p[3]) {
            char h[3] = {p[2], p[3], 0};
            *o++ = (char)strtol(h, NULL, 16);
            p += 3;
        } else *o++ = *p;
    }
    *o = 0;
}

// ---- a worker thread for steps marked to run off the main thread (as libifcs' learn thread)
static pthread_t worker;
static pthread_mutex_t wmu = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t wcv = PTHREAD_COND_INITIALIZER;
static void (*wjob)(void*);
static void* warg;
static int wbusy, use_thread;
static void* worker_main(void* a)
{
    (void)a;
    pthread_mutex_lock(&wmu);
    for (;;) {
        while (!wbusy) pthread_cond_wait(&wcv, &wmu);
        pthread_mutex_unlock(&wmu);
        wjob(warg);
        pthread_mutex_lock(&wmu);
        wbusy = 0;
        pthread_cond_broadcast(&wcv);
    }
    return NULL;
}
static void run(void (*job)(void*), void* arg)
{
    if (!use_thread) { job(arg); return; }
    pthread_mutex_lock(&wmu);
    wjob = job; warg = arg; wbusy = 1;
    pthread_cond_broadcast(&wcv);
    while (wbusy) pthread_cond_wait(&wcv, &wmu);
    pthread_mutex_unlock(&wmu);
}

struct rdjob { int32_t off; int null; uint32_t val; int rc; };
static void rd_job(void* a)
{
    struct rdjob* j = a;
    j->rc = g_rd(0, j->off, j->null ? NULL : &j->val);
}
struct wrjob { int32_t off; uint32_t val; int rc; };
static void wr_job(void* a)
{
    struct wrjob* j = a;
    j->rc = g_wr(0, j->off, j->val);
}
struct mjob { size_t n; void* p; int err; };
static void m_job(void* a)
{
    struct mjob* j = a;
    errno = 0;
    j->p = do_malloc(j->n);
    j->err = errno;
}

static void* need(void* h, const char* name)
{
    void* p = dlsym(h, name);
    if (!p) { fprintf(stdout, "harness: %s missing\n", name); exit(3); }
    return p;
}

int main(int argc, char** argv)
{
    if (argc != 4) { fprintf(stdout, "usage: guard_harness <shim.so> <script> <work dir>\n"); return 2; }
    snprintf(work, sizeof(work), "%s", argv[3]);
    snprintf(stats_path, sizeof(stats_path), "%s/ifcs-guard.prom", work);
    char errpath[600];
    snprintf(errpath, sizeof(errpath), "%s/counts.txt", work);
    counts = fopen(errpath, "w");
    if (!counts) { fprintf(stdout, "harness: counts\n"); return 3; }
    snprintf(errpath, sizeof(errpath), "%s/stderr.txt", work);
    errfd = open(errpath, O_RDWR | O_CREAT | O_TRUNC, 0644);
    if (errfd < 0 || dup2(errfd, 2) < 0) { fprintf(stdout, "harness: stderr\n"); return 3; }
    setvbuf(stdout, NULL, _IOLBF, 0);
    void* h = dlopen(argv[1], RTLD_NOW | RTLD_GLOBAL);
    if (!h) { fprintf(stdout, "harness: dlopen: %s\n", dlerror()); return 3; }
    g_init = (int (*)(void*, void*, const char*, unsigned, unsigned long long))need(h, "tl_ifcs_guard_init");
    g_installed = (void (*)(int))need(h, "tl_ifcs_guard_installed");
    g_rd = (int (*)(uint32_t, int32_t, uint32_t*))need(h, "tl_ifcs_reg_read32");
    g_wr = (int (*)(uint32_t, int32_t, uint32_t))need(h, "tl_ifcs_reg_write32");
    g_malloc = (void* (*)(size_t))need(h, "tl_ifcs_im_malloc");
    g_text = (const char* (*)(void))need(h, "tl_ifcs_guard_text");
    g_memcpy = (void* (*)(void*, const void*, size_t))need(h, "tl_devmem_memcpy");
    pthread_create(&worker, NULL, worker_main, NULL);

    FILE* sc = fopen(argv[2], "r");
    if (!sc) { fprintf(stdout, "harness: script\n"); return 3; }
    char line[8192];
    unsigned long steps = 0;
    while (fgets(line, sizeof(line), sc)) {
        cur_line++;
        line[strcspn(line, "\n")] = 0;
        if (!line[0] || line[0] == '#') continue;
        char op[32] = "";
        int pos = 0;
        sscanf(line, "%31s %n", op, &pos);
        char* rest = line + pos;
        steps++;
        if (!strcmp(op, "t")) {   // t <mono> <real>: the clock
            sscanf(rest, "%lf %lf", &t_mono, &t_real);
            continue;
        } else if (!strcmp(op, "adv")) {   // adv <seconds>
            double s = 0;
            sscanf(rest, "%lf", &s);
            t_mono += s;
            t_real += s;
            continue;
        } else if (!strcmp(op, "set")) {   // set <off> <val>
            unsigned off = 0, v = 0;
            sscanf(rest, "%x %x", &off, &v);
            reg((int32_t)off)->val = v;
            continue;
        } else if (!strcmp(op, "q")) {   // q <off> <rc>:<val> ...: the next answers of <off>
            unsigned off = 0;
            int n = 0;
            sscanf(rest, "%x %n", &off, &n);
            struct reg* r = reg((int32_t)off);
            if (r->head == r->nans) r->head = r->nans = 0;
            char* p = rest + n;
            int rc;
            unsigned v;
            int k;
            while (sscanf(p, "%d:%x %n", &rc, &v, &k) == 2 && r->nans < NANS) {
                r->rc[r->nans] = rc;
                r->v[r->nans++] = v;
                p += k;
            }
            continue;
        } else if (!strcmp(op, "wrrc")) {
            sscanf(rest, "%d", &wr_rc);
            continue;
        } else if (!strcmp(op, "thread")) {
            use_thread = atoi(rest);
            continue;
        } else if (!strcmp(op, "injectfile")) {   // injectfile <escaped text>: write the hook's file
            char text[4096];
            snprintf(text, sizeof(text), "%s", rest);
            unescape(text);
            snprintf(inject_path, sizeof(inject_path), "%s/inject", work);
            FILE* f = fopen(inject_path, "w");
            if (f) { fputs(text, f); fclose(f); }
            continue;
        }
        fprintf(stdout, "%s\n", line);
        if (!strcmp(op, "init")) {   // init <ring> <limit> <stats: none|empty|file|baddir> <inject 0|1> [nullrd]
            unsigned ring = 0;
            unsigned long long limit = 0;
            char st[16] = "", nr[16] = "";
            int inj = 0;
            sscanf(rest, "%x %llu %15s %d %15s", &ring, &limit, st, &inj, nr);
            snprintf(inject_path, sizeof(inject_path), "%s/inject", work);
            if (inj) setenv("SAI_TERALYNX_IFCS_GUARD_INJECT", inject_path, 1);
            else unsetenv("SAI_TERALYNX_IFCS_GUARD_INJECT");
            if (!inj) inject_path[0] = 0;
            const char* sp = NULL;
            char bad[700];
            if (!strcmp(st, "empty")) sp = "";
            else if (!strcmp(st, "file")) sp = stats_path;
            else if (!strcmp(st, "baddir")) { snprintf(bad, sizeof(bad), "%s/no-such-dir/ifcs-guard.prom", work); sp = bad; }
            begin_step();
            int rc = g_init(!strcmp(nr, "nullrd") ? NULL : (void*)h_read, !strcmp(nr, "nullwr") ? NULL : (void*)h_write,
                            sp, ring, limit);
            end_step();
            count_step("init");
            fprintf(stdout, "  = %d\n", rc);
        } else if (!strcmp(op, "installed")) {
            begin_step();
            g_installed(atoi(rest));
            end_step();
            count_step("installed");
        } else if (!strcmp(op, "rd") || !strcmp(op, "rdnull")) {   // rd <off> [repeat]
            unsigned off = 0, rep = 1;
            sscanf(rest, "%x %u", &off, &rep);
            // a repeated read: every call but the last is folded into one digest (rc, value, calls,
            // sleeps); stderr and the file are shown after the last
            uint64_t dig = 1469598103934665603ull;
            for (unsigned k = 0; k < rep; k++) {
                struct rdjob j = {(int32_t)off, !strcmp(op, "rdnull"), 0x5a5a5a5au, 0};
                begin_step();
                run(rd_job, &j);
                if (k + 1 < rep) {
                    counting = 0;
                    count_step("rd");
                    char b[64];
                    int bn = snprintf(b, sizeof(b), "%d %08x|", j.rc, j.val);
                    for (int i = 0; i < bn; i++) dig = (dig ^ (unsigned char)b[i]) * 1099511628211ull;
                    for (size_t i = 0; i < calls_len; i++) dig = (dig ^ (unsigned char)calls[i]) * 1099511628211ull;
                    for (size_t i = 0; i < sleeps_len; i++) dig = (dig ^ (unsigned char)sleeps[i]) * 1099511628211ull;
                    calls_len = 0; calls[0] = 0;
                    sleeps_len = 0; sleeps[0] = 0;
                    continue;
                }
                if (rep > 1) fprintf(stdout, "  first %u: %016" PRIx64 "\n", rep - 1, dig);
                end_step();
                count_step("rd");
                fprintf(stdout, "  = %d val %08x\n", j.rc, j.val);
            }
        } else if (!strcmp(op, "wr")) {
            unsigned off = 0, v = 0;
            sscanf(rest, "%x %x", &off, &v);
            struct wrjob j = {(int32_t)off, v, 0};
            begin_step();
            run(wr_job, &j);
            end_step();
            count_step("wr");
            fprintf(stdout, "  = %d\n", j.rc);
        } else if (!strcmp(op, "malloc")) {
            unsigned long long n = 0;
            sscanf(rest, "%llu", &n);
            struct mjob j = {(size_t)n, NULL, 0};
            begin_step();
            run(m_job, &j);
            end_step();
            count_step("malloc");
            int zero = 1;
            if (j.p && n <= 65536)
                for (size_t i = 0; i < n; i++) if (((unsigned char*)j.p)[i]) zero = 0;
            fprintf(stdout, "  = %s errno %d%s\n", j.p ? "ptr" : "NULL", j.p ? 0 : j.err, j.p && !zero ? " NOT ZEROED" : "");
            free(j.p);
        } else if (!strcmp(op, "text")) {
            begin_step();
            const char* t = g_text();
            end_step();
            count_step("text");
            print_escaped("  text ", t, strlen(t));
        } else if (!strcmp(op, "memcpy")) {   // memcpy <dst offset> <src offset> <n>
            unsigned dofs = 0, sofs = 0;
            size_t n = 0;
            sscanf(rest, "%u %u %zu", &dofs, &sofs, &n);
            static unsigned char src[4096 + 64], dst[4096 + 64];
            for (size_t i = 0; i < sizeof(src); i++) { src[i] = (unsigned char)(i * 37 + 11); dst[i] = 0xee; }
            begin_step();
            void* r = g_memcpy(dst + dofs, src + sofs, n);
            end_step();
            count_step("memcpy");
            uint64_t hsh = 1469598103934665603ull;
            for (size_t i = 0; i < sizeof(dst); i++) hsh = (hsh ^ dst[i]) * 1099511628211ull;
            fprintf(stdout, "  = %s dst %016" PRIx64 " copied %s\n", r == dst + dofs ? "dst" : "OTHER", hsh,
                    memcmp(dst + dofs, src + sofs, n) == 0 ? "ok" : "WRONG");
        } else if (!strcmp(op, "py")) {   // py <stub>: call one CPython stub with arguments
            char name[64] = "";
            sscanf(rest, "%63s", name);
            void* f = dlsym(h, name);
            begin_step();
            long rv = -7;
            if (!f) fprintf(stdout, "  missing\n");
            else if (!strcmp(name, "Py_BuildValue")) rv = (long)(intptr_t)((void* (*)(const char*, ...))f)("(iiiiiiiiii)", 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
            else if (!strcmp(name, "PyObject_CallFunctionObjArgs")) rv = (long)(intptr_t)((void* (*)(void*, ...))f)((void*)1, (void*)2, (void*)3, 1.5, (void*)0);
            else if (!strcmp(name, "PyErr_Print") || !strcmp(name, "Py_Finalize") || !strcmp(name, "Py_Initialize")) { ((void (*)(void))f)(); rv = 0; }
            else if (!strcmp(name, "PyGILState_Release")) { ((void (*)(long))f)(5); rv = 0; }
            else if (!strcmp(name, "PyCallable_Check") || !strcmp(name, "PyGILState_Ensure") || !strcmp(name, "PyInt_AsLong")
                     || !strcmp(name, "PyLong_AsLong") || !strcmp(name, "PyList_SetItem"))
                rv = ((long (*)(void*, long, void*))f)((void*)1, 2, (void*)3);
            else rv = (long)(intptr_t)((void* (*)(void*, long, void*))f)((void*)1, 2, (void*)3);
            end_step();
            count_step("py");
            fprintf(stdout, "  = %ld\n", rv);
        } else {
            fprintf(stdout, "harness: unknown op %s\n", op);
            return 3;
        }
    }
    fprintf(stdout, "end: %lu steps\n", steps);
    fprintf(counts, "total alloc %lu tlsdtor %lu\n", alloc_total, tls_total);
    fclose(counts);
    return 0;
}
