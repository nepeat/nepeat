/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: the BAR register accessors and im_malloc that libifcs'
 * learn thread calls through its PLT (node_pci_reg_read32 / _write32,
 * im_malloc), so ifcs_lib.cpp's GOT redirect to the shim's learn-ring guard
 * (src/ifcs_guard.c) is exercised for real. A few registers, failure
 * injection (all-ones reads) and the vendor im_malloc (malloc + memset),
 * capped at 64 MiB so a garbage size is recorded instead of allocated.
 * The learn pass itself is in mock_learn_thread.c (a separate file, so its
 * calls go through the PLT). Test: tests/t_learn.cpp. */
#include "mock.h"

#include <stdint.h>

#define NREGS 8
static struct { int32_t off; uint32_t val; unsigned fail; } regs[NREGS] = {
    {0x4864, 0xa5a00000u, 0}, {0x485c, 0x00300000u, 0}, {0x4860, 0, 0}};
static uint64_t im_malloc_calls, im_malloc_max;
static pthread_mutex_t rmu = PTHREAD_MUTEX_INITIALIZER;

static int find(int32_t off)
{
    for (int i = 0; i < NREGS; i++)
        if (regs[i].off == off) return i;
    for (int i = 0; i < NREGS; i++)
        if (regs[i].off == 0) { regs[i].off = off; return i; }
    return -1;
}

/* the real accessors' contract: 0 = ok; on error *val is not written */
int mock_reg_read_impl(uint32_t node, int32_t off, uint32_t* val)
{
    (void)node;
    pthread_mutex_lock(&rmu);
    int i = find(off);
    int rc = 0;
    if (i < 0) rc = 11;
    else if (regs[i].fail) { regs[i].fail--; *val = 0xffffffffu; }   /* a completion timeout: all-ones */
    else *val = regs[i].val;
    pthread_mutex_unlock(&rmu);
    return rc;
}
int mock_reg_write_impl(uint32_t node, int32_t off, uint32_t val)
{
    (void)node;
    pthread_mutex_lock(&rmu);
    int i = find(off);
    if (i >= 0) regs[i].val = val;
    pthread_mutex_unlock(&rmu);
    return i < 0 ? 11 : 0;
}
void* mock_im_malloc_impl(size_t n)
{
    pthread_mutex_lock(&rmu);
    im_malloc_calls++;
    if (n > im_malloc_max) im_malloc_max = n;
    pthread_mutex_unlock(&rmu);
    if (n > (64u << 20)) return NULL;   /* the real one would malloc + memset it */
    void* p = malloc(n);
    if (p) memset(p, 0, n);
    return p;
}

/* exported like libifcs' (the PLT targets the guard replaces) */
int node_pci_reg_read32(uint32_t node, int32_t off, uint32_t* val) { return mock_reg_read_impl(node, off, val); }
int node_pci_reg_write32(uint32_t node, int32_t off, uint32_t val) { return mock_reg_write_impl(node, off, val); }
void* im_malloc(size_t n) { return mock_im_malloc_impl(n); }

/* test hooks */
void mock_reg_set(int32_t off, uint32_t v) { mock_reg_write_impl(0, off, v); }
uint32_t mock_reg_get(int32_t off) { uint32_t v = 0; pthread_mutex_lock(&rmu); int i = find(off); if (i >= 0) v = regs[i].val; pthread_mutex_unlock(&rmu); return v; }
void mock_reg_fail(int32_t off, unsigned n) { pthread_mutex_lock(&rmu); int i = find(off); if (i >= 0) regs[i].fail = n; pthread_mutex_unlock(&rmu); }
unsigned mock_reg_fail_left(int32_t off) { unsigned v = 0; pthread_mutex_lock(&rmu); int i = find(off); if (i >= 0) v = regs[i].fail; pthread_mutex_unlock(&rmu); return v; }
uint64_t mock_im_malloc_calls(void) { return im_malloc_calls; }
uint64_t mock_im_malloc_max(void) { return im_malloc_max; }
