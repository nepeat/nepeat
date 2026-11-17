/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: one pass of the vendor learn thread's ring poll, as
 * disassembled from libifcs 0.12.41 (_ii_pcie_rupt_thread_learn,
 * 0x166b97c..0x166c37c; learn-wedge.md): read 0x4864 and 0x485c, then the
 * producer index B = 0x4860; n from A (software index) and B; im_malloc(n * 8);
 * walk while (A != B), B not re-read; write A back into 0x4864 bits 0..12
 * and the 0x485c field. Calls go through the PLT (mock_learn.c defines them),
 * so the shim's guard sees them once ifcs_lib.cpp redirected the GOT. The
 * walk is bounded here (the vendor's is not): over 2 x 4096 steps = wedged.
 * mock_learn_pass_unguarded() calls the implementations directly (the
 * control: what libifcs does without the guard). */
#include "mock.h"

#include <stdint.h>

int mock_reg_read_impl(uint32_t node, int32_t off, uint32_t* val);
int mock_reg_write_impl(uint32_t node, int32_t off, uint32_t val);
void* mock_im_malloc_impl(size_t n);
int node_pci_reg_read32(uint32_t node, int32_t off, uint32_t* val);
int node_pci_reg_write32(uint32_t node, int32_t off, uint32_t val);
void* im_malloc(size_t n);

#define RING 0x800u
static uint32_t A;            /* software index: 0..0x7ff, 0x1000..0x17ff */
static uint64_t last_request; /* bytes asked of im_malloc in the last pass */
static int wedged;

static int pass(int guarded)
{
    uint32_t r4864 = 0x5a5a5a5au, r485c = 0x5a5a5a5au, B = 0x5a5a5a5au;   /* the thread's stack: not initialised */
    /* direct calls (bl ...@plt), as libifcs makes them; return codes ignored, as in libifcs */
#define RD(o, v) (guarded ? node_pci_reg_read32(0, o, v) : mock_reg_read_impl(0, o, v))
#define WR(o, v) (guarded ? node_pci_reg_write32(0, o, v) : mock_reg_write_impl(0, o, v))
    RD(0x4864, &r4864);
    RD(0x485c, &r485c);
    RD(0x4860, &B);
    uint32_t n = ((A >> 12) == (B >> 12)) ? RING + B - A : RING - (B & 0xfff) + (A & 0xfff);
    void* buf = NULL;
    last_request = 0;
    if (n) {
        last_request = (uint64_t)n << 3;
        buf = guarded ? im_malloc(last_request) : mock_im_malloc_impl(last_request);
    }
    int steps = 0;
    while (A != B) {
        if (++steps > 2 * 4096) { wedged = 1; steps = -1; break; }
        A++;
        if ((A & 0xfff) == RING) A = ~A & 0x1000;
    }
    free(buf);
    WR(0x4864, (r4864 & ~0x1fffu) | (A & 0x1fffu));
    WR(0x485c, (r485c & ~0x1fffffu) | 0x42u);
#undef RD
#undef WR
    return steps;
}

/* entries consumed by this pass, -1 = wedged (the vendor thread would spin forever) */
int mock_learn_pass(void) { return pass(1); }
int mock_learn_pass_unguarded(void) { return pass(0); }
uint32_t mock_learn_sw_index(void) { return A; }
void mock_learn_reset(uint32_t a) { A = a; wedged = 0; }
uint64_t mock_learn_last_request(void) { return last_request; }
int mock_learn_wedged(void) { return wedged; }
void* mock_learn_im_malloc(size_t n) { return im_malloc(n); }   /* another libifcs caller of im_malloc (PLT) */
uint32_t mock_learn_read(int32_t off) { uint32_t v = 0; node_pci_reg_read32(0, off, &v); return v; }   /* any other register (PLT) */
