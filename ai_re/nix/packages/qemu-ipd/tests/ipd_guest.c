// SPDX-License-Identifier: GPL-2.0-or-later
// qemu-ipd test guest (x86_64), run under qemu-x86_64 + ./supervisor.c.
// $IPD_FAKE is both QEMU_IPD_TEST_PATH (qemu treats it as /dev/ipd*) and the
// supervisor's fake driver. Prints "ALL OK" and exits 0 when every check holds.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

static int fails;
#define CHECK(what, c) do { int ok_ = (c); printf("%-44s %s\n", what, ok_ ? "ok" : "FAIL"); if (!ok_) fails++; } while (0)

static sigjmp_buf jb;
static void *bus_addr;
static void on_bus(int s, siginfo_t *si, void *u) { (void)s; (void)u; bus_addr = si->si_addr; siglongjmp(jb, 1); }

// 1 if a locked op at p faults (qemu-ipd: SIGSEGV SI_KERNEL for x86 guests)
static int atomic_bus(void *p) {
    bus_addr = NULL;
    if (sigsetjmp(jb, 1) == 0) {
        __atomic_fetch_add((int *)p, 1, __ATOMIC_SEQ_CST);
        return 0;
    }
    return 1;
}
static int cmpxchg_bus(void *p) {
    long e = 0;
    bus_addr = NULL;
    if (sigsetjmp(jb, 1) == 0) {
        __atomic_compare_exchange_n((long *)p, &e, 1, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST);
        return 0;
    }
    return 1;
}
static void *idle(void *a) { pause(); return a; }

int main(void) {
    const char *fake = getenv("IPD_FAKE");
    int fd = open(fake, O_RDWR), nul = open("/dev/null", O_RDWR);
    char q[64] = "IPDTEST-QUERY-IN";
    long r;

    printf("QUERY_ARG %p\n", (void *)q);
    r = ioctl(fd, 0xc0084902, q);
    CHECK("query: struct passed and written back", r == 0 && !memcmp(q, "IPDTEST-QUERY-OK", 16));
    r = ioctl(fd, 0x40044923, 7);
    CHECK("integer argument + positive return", r == 3);
    errno = 0;
    r = ioctl(fd, 0xc0084904, q);
    CHECK("driver error comes back as errno", r == -1 && errno == EINVAL);
    errno = 0;
    r = ioctl(fd, 0xc0084963, q);
    CHECK("unknown ipd nr refused (ENOTTY)", r == -1 && errno == ENOTTY);
    errno = 0;
    r = ioctl(nul, 0xc0084902, q);
    CHECK("ipd cmd on another fd: ENOTTY", r == -1 && errno == ENOTTY);
    errno = 0;
    r = ioctl(nul, 0x703, 0x50);
    CHECK("I2C_SLAVE on another fd: ENOTTY", r == -1 && errno == ENOTTY);

    struct sigaction sa = { 0 };
    sa.sa_sigaction = on_bus;
    sa.sa_flags = SA_SIGINFO;
    sigaction(SIGBUS, &sa, NULL);
    sigaction(SIGSEGV, &sa, NULL);
    pthread_t t;
    pthread_create(&t, NULL, idle, NULL);   // parallel TBs: atomics via helpers

    char *m = mmap(NULL, 3 * 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    CHECK("mmap of the device", m != MAP_FAILED);
    m[0] = 1;
    CHECK("plain store/load on the device map", ((volatile char *)m)[0] == 1);
    CHECK("atomic add on the device map: faults", atomic_bus(m + 8));
    CHECK("cmpxchg on the device map: faults", cmpxchg_bus(m + 4096 + 16));
    char anon[64] __attribute__((aligned(8))) = { 0 };
    CHECK("atomic add on normal memory works", !atomic_bus(anon) && *(int *)anon == 1);

    munmap(m + 4096, 4096);
    CHECK("remap anon in the hole",
          mmap(m + 4096, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) == m + 4096);
    CHECK("partial unmap: hole is normal memory", !atomic_bus(m + 4096 + 8));
    CHECK("partial unmap: head still device", atomic_bus(m + 16));
    CHECK("partial unmap: tail still device", atomic_bus(m + 2 * 4096 + 8));
    CHECK("MAP_FIXED anon over the device",
          mmap(m, 3 * 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) == m);
    CHECK("after replacement: normal memory", !atomic_bus(m + 8) && !atomic_bus(m + 2 * 4096 + 8));

    // many single-page device maps, each followed by an anonymous page (libsai
    // maps its DMA pages one by one: thousands of scattered ranges)
    enum { NP = 600 };
    static char *dp[NP], *ap[NP];
    int okmap = 1;
    for (int i = 0; i < NP; i++) {
        dp[i] = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        ap[i] = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (dp[i] == MAP_FAILED || ap[i] == MAP_FAILED) okmap = 0;
    }
    CHECK("600 device pages + 600 anon pages mapped", okmap);
    CHECK("many maps: device page 0 faults", atomic_bus(dp[0] + 8));
    CHECK("many maps: device page 450 faults", atomic_bus(dp[450] + 64));
    CHECK("many maps: anon pages work", !atomic_bus(ap[0] + 8) && !atomic_bus(ap[450] + 8) && !atomic_bus(ap[NP - 1]));
    for (int i = 0; i < NP; i++) munmap(dp[i], 4096);
    CHECK("many maps: unmapped, nothing faults", !atomic_bus(ap[300] + 16));

    if (fails == 0) puts("ALL OK");
    fflush(stdout);
    _exit(fails ? 1 : 0);
}
