/* SPDX-License-Identifier: GPL-2.0 */
/*
 * Register-level probes for the ABI conformance module (ELFv2 only).
 *
 * abi_capture: called with any prototype; stores the argument registers
 * r3-r10 and parameter doublewords 8-15 (ELFv2: 96(r1) onward in the
 * caller's frame) into abi_regs[0..15], so the caller's placement and
 * extension of every argument can be compared with the ABI and between
 * GCC and rustc callers.
 *
 * abi_call_ret: u64 abi_call_ret(void *fn) calls fn() and returns with r3
 * as fn left it, so the extension of a narrow return value is visible.
 */
#include <linux/linkage.h>
#include <asm/ppc_asm.h>

#ifndef CONFIG_PPC64_ELF_ABI_V2
#error "abi_regs.S is written for the ELFv2 ABI"
#endif

	.section ".toc","aw"
.Labi_regs_toc:
	.tc abi_regs[TC],abi_regs

	.text
_GLOBAL_TOC(abi_capture)
	ld	r11,.Labi_regs_toc@toc(r2)
	std	r3,0(r11)
	std	r4,8(r11)
	std	r5,16(r11)
	std	r6,24(r11)
	std	r7,32(r11)
	std	r8,40(r11)
	std	r9,48(r11)
	std	r10,56(r11)
	ld	r12,96(r1)
	std	r12,64(r11)
	ld	r12,104(r1)
	std	r12,72(r11)
	ld	r12,112(r1)
	std	r12,80(r11)
	ld	r12,120(r1)
	std	r12,88(r11)
	ld	r12,128(r1)
	std	r12,96(r11)
	ld	r12,136(r1)
	std	r12,104(r11)
	ld	r12,144(r1)
	std	r12,112(r11)
	ld	r12,152(r1)
	std	r12,120(r11)
	blr

_GLOBAL_TOC(abi_call_ret)
	mflr	r0
	std	r0,16(r1)
	stdu	r1,-48(r1)
	std	r2,24(r1)
	mr	r12,r3
	mtctr	r12
	bctrl
	ld	r2,24(r1)
	addi	r1,r1,48
	ld	r0,16(r1)
	mtlr	r0
	blr
