// SPDX-License-Identifier: GPL-2.0
/*
 * C side (GCC) of the ppc64 ABI conformance module: scalar calls in both
 * directions, register captures of GCC's own calls (compared in Rust with
 * the ELFv2 image and with rustc's captures), narrow return values,
 * varargs, and the kthreads that run the Rust atomics/lock hammer on every
 * CPU. The by-value struct part is generated (gen_abi.py).
 */
#include <linux/completion.h>
#include <linux/cpumask.h>
#include <linux/kthread.h>
#include <linux/printk.h>
#include <linux/slab.h>
#include <linux/stdarg.h>
#include "abi_common.h"

u64 abi_regs[32];
void abi_capture(void);

/* ---- scalars ---------------------------------------------------------- */

#define SC_PARAMS u8 a, s8 b, u16 c, s16 d, u32 e, s32 f, bool g, u64 h, \
	u8 i, s8 j, u16 k, s16 l, u32 m, s32 n, bool o, u64 p

static u64 hash_scalars(SC_PARAMS)
{
	u64 x = ABI_H0;

	x = abi_mix(x, a); x = abi_mix(x, (u64)(s64)b);
	x = abi_mix(x, c); x = abi_mix(x, (u64)(s64)d);
	x = abi_mix(x, e); x = abi_mix(x, (u64)(s64)f);
	x = abi_mix(x, g); x = abi_mix(x, h);
	x = abi_mix(x, i); x = abi_mix(x, (u64)(s64)j);
	x = abi_mix(x, k); x = abi_mix(x, (u64)(s64)l);
	x = abi_mix(x, m); x = abi_mix(x, (u64)(s64)n);
	x = abi_mix(x, o); x = abi_mix(x, p);
	return x;
}

u64 abi_c_ints(SC_PARAMS);
u64 abi_c_ints(SC_PARAMS)
{
	return hash_scalars(a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p);
}

u128 abi_c_u128(u64 a, u128 b, u64 c, u128 d);
u128 abi_c_u128(u64 a, u128 b, u64 c, u128 d)
{
	return b * 3 + (d ^ ((u128)a << 64)) + c;
}

/* Dirty sources: narrow values cut from 64-bit values with high bits set */
static volatile u64 dirty[8] = {
	0xdeadbeef000000f1ULL, 0xdeadbeef0000fff1ULL, 0xdeadbeef0000f2f3ULL,
	0xdeadbeef0000f448ULL, 0xdeadbeeff4f5f6f7ULL, 0xdeadbeeffffe7960ULL,
	0x8000000000000001ULL, 0xfedcba9876543210ULL,
};

u64 abi_rs_ints(SC_PARAMS);
u128 abi_rs_u128(u64 a, u128 b, u64 c, u128 d);

int abi_c_check_rust_scalars(void);
int abi_c_check_rust_scalars(void)
{
	int fails = 0;
	u64 got, want;
	u128 q1 = ((u128)dirty[7] << 64) | dirty[6], q2 = ((u128)dirty[6] << 64) | dirty[4];

	want = hash_scalars((u8)dirty[0], (s8)dirty[1], (u16)dirty[2], (s16)dirty[3],
			    (u32)dirty[4], (s32)dirty[5], dirty[6] & 1, dirty[7],
			    (u8)dirty[7], (s8)dirty[0], (u16)dirty[1], (s16)dirty[2],
			    (u32)dirty[5], (s32)dirty[4], !(dirty[6] & 1), dirty[0]);
	got = abi_rs_ints((u8)dirty[0], (s8)dirty[1], (u16)dirty[2], (s16)dirty[3],
			  (u32)dirty[4], (s32)dirty[5], dirty[6] & 1, dirty[7],
			  (u8)dirty[7], (s8)dirty[0], (u16)dirty[1], (s16)dirty[2],
			  (u32)dirty[5], (s32)dirty[4], !(dirty[6] & 1), dirty[0]);
	if (got != want) {
		fails++;
		pr_err("ppc64abi: FAIL C->Rust scalars: %llx != %llx\n", got, want);
	}
	if (abi_rs_u128(dirty[0], q1, dirty[1], q2) != abi_c_u128(dirty[0], q1, dirty[1], q2)) {
		fails++;
		pr_err("ppc64abi: FAIL C->Rust u128\n");
	}
	return fails;
}

/* ---- GCC's own calls, captured (Rust compares) ------------------------ */

/* abi_capture under the prototypes used here (asm labels: no casts) */
void cap_ints(SC_PARAMS) __asm__("abi_capture");
void cap_u128(u64, u128, u64, u128) __asm__("abi_capture");
void cap_ptrs(const void *, unsigned long, long, void (*)(void), u8) __asm__("abi_capture");

void abi_c_capture_ints(u64 *out);
void abi_c_capture_ints(u64 *out)
{
	cap_ints((u8)dirty[0], (s8)dirty[1], (u16)dirty[2], (s16)dirty[3],
			(u32)dirty[4], (s32)dirty[5], dirty[6] & 1, dirty[7],
			(u8)dirty[7], (s8)dirty[0], (u16)dirty[1], (s16)dirty[2],
			(u32)dirty[5], (s32)dirty[4], !(dirty[6] & 1), dirty[0]);
	memcpy(out, abi_regs, 16 * sizeof(u64));
}

void abi_c_capture_u128(u64 *out);
void abi_c_capture_u128(u64 *out)
{
	cap_u128(dirty[0], ((u128)dirty[7] << 64) | dirty[6], dirty[1],
					   ((u128)dirty[6] << 64) | dirty[4]);
	memcpy(out, abi_regs, 16 * sizeof(u64));
}

void abi_c_capture_ptrs(u64 *out);
void abi_c_capture_ptrs(u64 *out)
{
	cap_ptrs((const void *)(uintptr_t)dirty[7], dirty[6],
					   (long)dirty[5], (void (*)(void))abi_capture, (u8)dirty[3]);
	memcpy(out, abi_regs, 16 * sizeof(u64));
}

/* ---- narrow return values (abi_call_ret shows the raw r3) -------------- */

u8 abi_c_r_u8(void);
u8 abi_c_r_u8(void) { return (u8)dirty[0]; }
s8 abi_c_r_s8(void);
s8 abi_c_r_s8(void) { return (s8)dirty[1]; }
u16 abi_c_r_u16(void);
u16 abi_c_r_u16(void) { return (u16)dirty[2]; }
s16 abi_c_r_s16(void);
s16 abi_c_r_s16(void) { return (s16)dirty[3]; }
u32 abi_c_r_u32(void);
u32 abi_c_r_u32(void) { return (u32)dirty[4]; }
s32 abi_c_r_s32(void);
s32 abi_c_r_s32(void) { return (s32)dirty[5]; }
bool abi_c_r_bool(void);
bool abi_c_r_bool(void) { return dirty[6] & 1; }

/* ---- varargs ----------------------------------------------------------- */

u64 abi_c_va(int n, ...);
u64 abi_c_va(int n, ...)
{
	va_list ap;
	u64 h = ABI_H0;
	int i;

	va_start(ap, n);
	for (i = 0; i < n; i++)
		h = abi_mix(h, va_arg(ap, u64));
	va_end(ap);
	return h;
}

/* ---- one kthread per online CPU running a Rust function ---------------- */

struct abi_thr {
	void *ctx;
	u64 (*fn)(void *ctx, int idx, u64 iters);
	int idx;
	u64 iters;
	u64 ret;
	struct completion done;
};

static int abi_thread(void *p)
{
	struct abi_thr *t = p;

	t->ret = t->fn(t->ctx, t->idx, t->iters);
	complete(&t->done);
	return 0;
}

/* Returns the number of threads that ran (one per online CPU), or <0. */
int abi_c_run_threads(void *ctx, u64 (*fn)(void *, int, u64), u64 iters);
int abi_c_run_threads(void *ctx, u64 (*fn)(void *, int, u64), u64 iters)
{
	int n = num_online_cpus(), started = 0, err = 0, i, cpu;
	struct abi_thr *t = kcalloc(n, sizeof(*t), GFP_KERNEL);

	if (!t)
		return -ENOMEM;
	for_each_online_cpu(cpu) {
		struct task_struct *task;

		if (started == n)
			break;
		i = started;
		t[i] = (struct abi_thr){ .ctx = ctx, .fn = fn, .idx = i, .iters = iters };
		init_completion(&t[i].done);
		task = kthread_create(abi_thread, &t[i], "ppc64abi/%d", i);
		if (IS_ERR(task)) {
			err = PTR_ERR(task);
			break;
		}
		kthread_bind(task, cpu);
		wake_up_process(task);
		started++;
	}
	for (i = 0; i < started; i++)
		wait_for_completion(&t[i].done);
	kfree(t);
	return err ? err : started;
}
