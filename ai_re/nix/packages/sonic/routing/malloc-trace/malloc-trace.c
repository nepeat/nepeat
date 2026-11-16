/*
 * malloc-trace: an LD_PRELOAD heap profiler small enough to cross-build and
 * run on the switch. Every malloc/calloc/realloc/memalign is charged to its
 * call stack (glibc backtrace(), depth MT_DEPTH); live bytes per stack are
 * kept until freed. Every MALLOC_TRACE_INTERVAL seconds (default 60) the
 * stacks are written to MALLOC_TRACE_OUT (default /tmp/malloc-trace.<pid>),
 * sorted by live bytes, frames as "object+0xoffset (symbol+0xoff)" so they
 * can be resolved off the box (addr2line -e <object or its .debug>).
 *   LD_PRELOAD=/path/libmalloc-trace.so MALLOC_TRACE_OUT=/var/tmp/bfdd.mt <daemon>
 * Overhead: one backtrace per allocation, a global spin lock.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <execinfo.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

extern void *__libc_malloc(size_t);
extern void *__libc_calloc(size_t, size_t);
extern void *__libc_realloc(void *, size_t);
extern void *__libc_memalign(size_t, size_t);
extern void __libc_free(void *);

#define MT_DEPTH 8
#define MT_SKIP 2                 /* our own frames */
#define MT_SITES 65536            /* power of two */
#define MT_PTRS (1u << 22)        /* live pointer slots, power of two */

struct site {
	uintptr_t pc[MT_DEPTH];
	int n;
	int used;
	uint64_t live_bytes, live_count, allocs, frees;
};
struct ptr {
	uintptr_t p;              /* 0 = empty, 1 = deleted */
	uint32_t size;
	uint32_t site;
};

static struct site *sites;
static uint64_t sites_full;   /* allocations not charged: site table full */
static struct ptr *ptrs;
static volatile int lock_;
static __thread int busy;
static int ready;
static uint64_t dropped;
static int out_fd = -1;       /* opened at load: the daemon may drop privileges later */

static void lock(void) { while (__atomic_exchange_n(&lock_, 1, __ATOMIC_ACQUIRE)) ; }
static void unlock(void) { __atomic_store_n(&lock_, 0, __ATOMIC_RELEASE); }

static uint64_t mix(uint64_t x) { x ^= x >> 33; x *= 0xff51afd7ed558ccdULL; x ^= x >> 33; return x; }

static uint32_t site_get(uintptr_t *pc, int n)
{
	uint64_t h = n;
	for (int i = 0; i < n; i++) h = mix(h ^ pc[i]);
	for (uint32_t i = 0; i < MT_SITES; i++) {
		uint32_t s = (h + i) & (MT_SITES - 1);
		if (!sites[s].used) {
			sites[s].used = 1; sites[s].n = n;
			memcpy(sites[s].pc, pc, n * sizeof(*pc));
			return s;
		}
		if (sites[s].n == n && !memcmp(sites[s].pc, pc, n * sizeof(*pc)))
			return s;
	}
	return UINT32_MAX;
}

static void ptr_add(void *p, size_t size, uint32_t s)
{
	uint64_t h = mix((uintptr_t)p);
	for (uint32_t i = 0; i < 64; i++) {
		struct ptr *e = &ptrs[(h + i) & (MT_PTRS - 1)];
		if (e->p <= 1) {
			e->p = (uintptr_t)p; e->size = size > UINT32_MAX ? UINT32_MAX : size; e->site = s;
			sites[s].live_bytes += size; sites[s].live_count++; sites[s].allocs++;
			return;
		}
	}
	dropped++;
}

static void ptr_del(void *p)
{
	uint64_t h = mix((uintptr_t)p);
	for (uint32_t i = 0; i < 64; i++) {
		struct ptr *e = &ptrs[(h + i) & (MT_PTRS - 1)];
		if (e->p == (uintptr_t)p) {
			struct site *s = &sites[e->site];
			s->live_bytes -= e->size; s->live_count--; s->frees++;
			e->p = 1;
			return;
		}
		if (e->p == 0)
			return;   /* allocated before we started, or dropped */
	}
}

static void record(void *p, size_t size)
{
	void *bt[MT_DEPTH + MT_SKIP];
	uintptr_t pc[MT_DEPTH];
	int n;

	if (!p || !ready || busy) return;
	busy = 1;
	n = backtrace(bt, MT_DEPTH + MT_SKIP) - MT_SKIP;
	if (n < 0) n = 0;
	for (int i = 0; i < n; i++) pc[i] = (uintptr_t)bt[i + MT_SKIP];
	lock();
	{
		uint32_t s = site_get(pc, n);
		if (s == UINT32_MAX) sites_full++;
		else ptr_add(p, size, s);
	}
	unlock();
	busy = 0;
}

static void forget(void *p)
{
	if (!p || !ready) return;
	lock();
	ptr_del(p);
	unlock();
}

void *malloc(size_t n) { void *p = __libc_malloc(n); record(p, n); return p; }
void *calloc(size_t a, size_t b) { void *p = __libc_calloc(a, b); record(p, a * b); return p; }
void free(void *p) { forget(p); __libc_free(p); }
void *realloc(void *o, size_t n)
{
	void *p;
	forget(o);
	p = __libc_realloc(o, n);
	if (p) record(p, n);
	else if (o && n) record(o, n);   /* failed: old block still live, size unknown */
	return p;
}
void *memalign(size_t a, size_t n) { void *p = __libc_memalign(a, n); record(p, n); return p; }
void *aligned_alloc(size_t a, size_t n) { return memalign(a, n); }
int posix_memalign(void **r, size_t a, size_t n)
{
	void *p = __libc_memalign(a, n);
	if (!p) return ENOMEM;
	record(p, n);
	*r = p;
	return 0;
}
void *valloc(size_t n) { return memalign(sysconf(_SC_PAGESIZE), n); }

static int cmp(const void *a, const void *b)
{
	const struct site *x = *(struct site *const *)a, *y = *(struct site *const *)b;
	return x->live_bytes < y->live_bytes ? 1 : x->live_bytes > y->live_bytes ? -1 : 0;
}

static void dump(int fd, int top)
{
	static struct site *order[MT_SITES], copy[MT_SITES];
	char line[1024];
	int m = 0;
	uint64_t total = 0;

	lock();
	memcpy(copy, sites, sizeof(copy));
	unlock();
	for (int i = 0; i < MT_SITES; i++)
		if (copy[i].used) { order[m++] = &copy[i]; total += copy[i].live_bytes; }
	qsort(order, m, sizeof(*order), cmp);

	dprintf(fd, "=== t=%ld pid=%d live_bytes=%llu sites=%d untracked_drops=%llu sites_full=%llu\n",
		(long)time(NULL), getpid(), (unsigned long long)total, m, (unsigned long long)dropped,
		(unsigned long long)sites_full);
	for (int i = 0; i < m && i < top; i++) {
		struct site *s = order[i];
		dprintf(fd, "#%d live_bytes=%llu live=%llu allocs=%llu frees=%llu\n", i,
			(unsigned long long)s->live_bytes, (unsigned long long)s->live_count,
			(unsigned long long)s->allocs, (unsigned long long)s->frees);
		for (int j = 0; j < s->n; j++) {
			Dl_info di;
			uintptr_t pc = s->pc[j];
			if (dladdr((void *)pc, &di) && di.dli_fname) {
				snprintf(line, sizeof(line), "   %s+0x%lx (%s+0x%lx)\n", di.dli_fname,
					 (unsigned long)(pc - (uintptr_t)di.dli_fbase),
					 di.dli_sname ? di.dli_sname : "?",
					 di.dli_saddr ? (unsigned long)(pc - (uintptr_t)di.dli_saddr) : 0UL);
			} else
				snprintf(line, sizeof(line), "   0x%lx\n", (unsigned long)pc);
			if (write(fd, line, strlen(line)) < 0)
				return;
		}
	}
	fsync(fd);
}

static void *dumper(void *arg)
{
	const char *iv = getenv("MALLOC_TRACE_INTERVAL");
	const char *tp = getenv("MALLOC_TRACE_TOP");
	int interval = iv ? atoi(iv) : 60, top = tp ? atoi(tp) : 25;

	(void)arg;
	busy = 1;   /* this thread's own allocations are not traced */
	if (interval < 1) interval = 60;
	for (;;) {
		sleep(interval);
		dump(out_fd, top);
	}
	return NULL;
}

__attribute__((constructor)) static void init(void)
{
	pthread_t t;
	void *bt[2];

	busy = 1;
	backtrace(bt, 2);   /* load the unwinder (it may allocate) before tracing */
	sites = mmap(NULL, sizeof(*sites) * MT_SITES, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	ptrs = mmap(NULL, sizeof(*ptrs) * MT_PTRS, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
	if (sites == MAP_FAILED || ptrs == MAP_FAILED) { busy = 0; return; }
	{
		const char *out = getenv("MALLOC_TRACE_OUT");
		char def[64];

		if (!out) { snprintf(def, sizeof(def), "/tmp/malloc-trace.%d", getpid()); out = def; }
		out_fd = open(out, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
		if (out_fd < 0) { busy = 0; return; }
		dprintf(out_fd, "=== start pid=%d\n", getpid());
	}
	pthread_create(&t, NULL, dumper, NULL);
	pthread_detach(t);
	ready = 1;
	busy = 0;
}
