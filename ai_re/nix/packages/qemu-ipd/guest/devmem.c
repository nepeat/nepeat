// SPDX-License-Identifier: Apache-2.0
// libipd-devmem.so: LD_PRELOAD (amd64) for Marvell's SAI under qemu-ipd on
// arm64. glibc's x86 memcpy/memset/memmove/memcmp use unaligned 16-byte
// accesses; under qemu they become unaligned host accesses, which fault on
// arm64 Device memory, i.e. on the ipd mmaps (BAR0 and the DMA pool, both
// mapped non-cached by ipd on arm). Calls that touch an ipd mapping use only
// naturally aligned 8/4/2/1-byte accesses (the native arm64 build needed the
// same, sai-teralynx/src/shim.c); all other calls go to glibc unchanged.
// ipd mappings are learnt from mmap()/munmap() of fds whose path starts with
// /dev/ipd (IPD_DEVMEM_TEST_PATH adds one more path, for tests).
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <unistd.h>
#include <pthread.h>

// Device mappings: sorted, non-overlapping (libsai maps thousands of single
// DMA pages). Readers (every mem* call inside [lo, hi)) binary-search under
// the read lock. The table lives in raw mmap()ed memory and is copied by
// hand: nothing under the write lock may call memcpy & co (they take the
// read lock).
struct map { uintptr_t start, end; };
static struct map *maps;
static size_t nmaps, capmaps;
static uintptr_t lo, hi;
static pthread_rwlock_t rw = PTHREAD_RWLOCK_INITIALIZER;

static void *(*real_memcpy)(void *, const void *, size_t);
static void *(*real_memmove)(void *, const void *, size_t);
static void *(*real_memset)(void *, int, size_t);
static int (*real_memcmp)(const void *, const void *, size_t);

__attribute__((constructor)) static void init(void) {
    real_memcpy = dlsym(RTLD_NEXT, "memcpy");
    real_memmove = dlsym(RTLD_NEXT, "memmove");
    real_memset = dlsym(RTLD_NEXT, "memset");
    real_memcmp = dlsym(RTLD_NEXT, "memcmp");
}

static int hits(const void *p, size_t n) {
    uintptr_t a = (uintptr_t)p;
    if (a + n <= __atomic_load_n(&lo, __ATOMIC_ACQUIRE) || a >= __atomic_load_n(&hi, __ATOMIC_ACQUIRE))
        return 0;
    pthread_rwlock_rdlock(&rw);
    size_t l = 0, h = nmaps;
    while (l < h) {             // last map with start < a + n
        size_t m = (l + h) / 2;
        if (maps[m].start < a + n) l = m + 1; else h = m;
    }
    int hit = l > 0 && maps[l - 1].end > a;
    pthread_rwlock_unlock(&rw);
    return hit;
}

// the widest access size both addresses (and the length) allow
static size_t width(uintptr_t a, uintptr_t b, size_t n) {
    if (n >= 8 && !((a | b) & 7)) return 8;
    if (n >= 4 && !((a | b) & 3)) return 4;
    if (n >= 2 && !((a | b) & 1)) return 2;
    return 1;
}

#define ACC(w, d, s) do { switch (w) { \
    case 8: *(volatile uint64_t *)(d) = *(volatile const uint64_t *)(s); break; \
    case 4: *(volatile uint32_t *)(d) = *(volatile const uint32_t *)(s); break; \
    case 2: *(volatile uint16_t *)(d) = *(volatile const uint16_t *)(s); break; \
    default: *(volatile uint8_t *)(d) = *(volatile const uint8_t *)(s); } } while (0)

static void copy_fwd(uintptr_t d, uintptr_t s, size_t n) {
    while (n) { size_t w = width(d, s, n); ACC(w, d, s); d += w; s += w; n -= w; }
}
static void copy_bwd(uintptr_t d, uintptr_t s, size_t n) {
    while (n) { size_t w = width(d + n, s + n, n); n -= w; ACC(w, d + n, s + n); }
}

void *memcpy(void *d, const void *s, size_t n) {
    if (!real_memcpy || hits(d, n) || hits(s, n)) {
        copy_fwd((uintptr_t)d, (uintptr_t)s, n);
        return d;
    }
    return real_memcpy(d, s, n);
}

void *memmove(void *d, const void *s, size_t n) {
    if (!real_memmove || hits(d, n) || hits(s, n)) {
        if ((uintptr_t)d > (uintptr_t)s && (uintptr_t)d < (uintptr_t)s + n)
            copy_bwd((uintptr_t)d, (uintptr_t)s, n);
        else
            copy_fwd((uintptr_t)d, (uintptr_t)s, n);
        return d;
    }
    return real_memmove(d, s, n);
}

void *memset(void *d, int c, size_t n) {
    if (!real_memset || hits(d, n)) {
        uintptr_t a = (uintptr_t)d;
        uint64_t v = 0x0101010101010101ull * (uint8_t)c;
        while (n) {
            size_t w = width(a, 0, n);
            switch (w) {
            case 8: *(volatile uint64_t *)a = v; break;
            case 4: *(volatile uint32_t *)a = (uint32_t)v; break;
            case 2: *(volatile uint16_t *)a = (uint16_t)v; break;
            default: *(volatile uint8_t *)a = (uint8_t)v;
            }
            a += w; n -= w;
        }
        return d;
    }
    return real_memset(d, c, n);
}

int memcmp(const void *x, const void *y, size_t n) {
    if (!real_memcmp || hits(x, n) || hits(y, n)) {
        const volatile uint8_t *a = x, *b = y;
        for (size_t i = 0; i < n; i++)
            if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
        return 0;
    }
    return real_memcmp(x, y, n);
}

static int is_ipd(int fd) {
    char link[64], path[4096];
    const char *t = getenv("IPD_DEVMEM_TEST_PATH");
    ssize_t n;
    snprintf(link, sizeof link, "/proc/self/fd/%d", fd);
    n = readlink(link, path, sizeof path - 1);
    if (n <= 0) return 0;
    path[n] = 0;
    return !strncmp(path, "/dev/ipd", 8) || (t && *t && !strcmp(path, t));
}

static void grow(size_t need) {           // under the write lock
    if (need <= capmaps) return;
    size_t cap = capmaps * 2 + 256;
    if (cap < need) cap = need;
    struct map *n = (struct map *)syscall(SYS_mmap, NULL, cap * sizeof *n, PROT_READ | PROT_WRITE,
                                          MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (n == MAP_FAILED) abort();
    for (size_t i = 0; i < nmaps; i++) { n[i].start = maps[i].start; n[i].end = maps[i].end; }
    if (maps) syscall(SYS_munmap, maps, capmaps * sizeof *maps);
    maps = n;
    capmaps = cap;
}

static void bounds(void) {
    __atomic_store_n(&lo, nmaps ? maps[0].start : 0, __ATOMIC_RELEASE);
    __atomic_store_n(&hi, nmaps ? maps[nmaps - 1].end : 0, __ATOMIC_RELEASE);
}

static void cut(uintptr_t s, uintptr_t e) {   // under the write lock; drop [s, e)
    size_t i, j, split = 0;
    for (i = 0; i < nmaps; i++)
        if (maps[i].start < s && maps[i].end > e) split++;
    grow(nmaps + split + 1);
    // walk backwards so a split (one entry -> two) can shift the tail right
    size_t out = nmaps + split;
    for (i = nmaps, j = out; i-- > 0;) {
        uintptr_t ms = maps[i].start, me = maps[i].end;
        if (e <= ms || s >= me) { j--; maps[j].start = ms; maps[j].end = me; continue; }
        if (me > e) { j--; maps[j].start = e; maps[j].end = me; }
        if (ms < s) { j--; maps[j].start = ms; maps[j].end = s; }
    }
    // entries now occupy [j, out); move them to the front
    size_t k = 0;
    for (i = j; i < out; i++, k++) { maps[k].start = maps[i].start; maps[k].end = maps[i].end; }
    nmaps = k;
}

static void insert(uintptr_t s, uintptr_t e) {   // under the write lock
    grow(nmaps + 1);
    size_t l = 0, h = nmaps;
    while (l < h) { size_t m = (l + h) / 2; if (maps[m].start < s) l = m + 1; else h = m; }
    for (size_t i = nmaps; i > l; i--) { maps[i].start = maps[i - 1].start; maps[i].end = maps[i - 1].end; }
    maps[l].start = s; maps[l].end = e;
    nmaps++;
}

void *mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off) {
    void *r = (void *)syscall(SYS_mmap, addr, len, prot, flags, fd, off);
    if (r == MAP_FAILED) return r;
    uintptr_t s = (uintptr_t)r, e = s + len;
    int dev = fd >= 0 && !(flags & MAP_ANONYMOUS) && is_ipd(fd);
    if (!dev && (s >= __atomic_load_n(&hi, __ATOMIC_ACQUIRE) || e <= __atomic_load_n(&lo, __ATOMIC_ACQUIRE)))
        return r;
    pthread_rwlock_wrlock(&rw);
    if (nmaps) cut(s, e);        // MAP_FIXED over an old mapping replaces it
    if (dev) insert(s, e);
    bounds();
    pthread_rwlock_unlock(&rw);
    if (dev && getenv("IPD_DEVMEM_TRACE"))
        fprintf(stderr, "ipd-devmem: device map %p +0x%zx (fd %d off 0x%llx)\n",
                r, len, fd, (unsigned long long)off);
    return r;
}
void *mmap64(void *addr, size_t len, int prot, int flags, int fd, off_t off)
    __attribute__((alias("mmap")));

int munmap(void *addr, size_t len) {
    int r = (int)syscall(SYS_munmap, addr, len);
    uintptr_t s = (uintptr_t)addr, e = s + len;
    if (r == 0 && s < __atomic_load_n(&hi, __ATOMIC_ACQUIRE) && e > __atomic_load_n(&lo, __ATOMIC_ACQUIRE)) {
        pthread_rwlock_wrlock(&rw);
        cut(s, e);
        bounds();
        pthread_rwlock_unlock(&rw);
    }
    return r;
}

// tests: 1 if p..p+n touches a known device mapping
int ipd_devmem_hits(const void *p, size_t n) { return hits(p, n); }
