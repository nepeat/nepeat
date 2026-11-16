// SPDX-License-Identifier: GPL-2.0-or-later
// qemu-ipd codegen probe (x86_64). Part 1, single-threaded: an mfence (plain
// qemu drops barriers without a second guest thread). Part 2, after a thread
// exists (qemu then translates for parallel execution and gives 16-byte
// accesses their atomicity): an aligned 16-byte SSE load and store (movdqa).
// tests/default.nix runs it with -d op / -d out_asm and QEMU_IPD_STRICT=0/1.
#include <emmintrin.h>
#include <pthread.h>
static __m128i buf[4] __attribute__((aligned(16)));
static void *nop(void *a) { return a; }
__attribute__((noinline)) static int sse(int x) {
    __m128i v = _mm_load_si128((__m128i *)&buf[1]);
    v = _mm_add_epi32(v, _mm_set1_epi32(x));
    _mm_store_si128((__m128i *)&buf[2], v);
    return _mm_cvtsi128_si32(v);
}
int main(void) {
    volatile int x = 1;
    _mm_mfence();
    pthread_t t;
    pthread_create(&t, 0, nop, 0);
    pthread_join(t, 0);
    x = sse(x);
    return x == 1 ? 0 : 1;
}
