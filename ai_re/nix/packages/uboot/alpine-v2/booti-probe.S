// SPDX-License-Identifier: GPL-2.0+
/*
 * A fake arm64 "kernel" for the QEMU test: an Image header, then report
 * what the bootloader handed over on the virt PL011 and stop:
 *   PROBE: EL<n> x0=<dtb> magic=<ok|bad>
 *   PROBE: rsv <addr> <size>      (one line per memory reserve entry)
 *   PROBE: done
 * Entered with MMU off, so only aligned loads.
 */
	.equ	UART, 0x09000000

	.text
	.globl	_start
_start:
	b	start			/* code0 */
	.long	0			/* code1 */
	.quad	0			/* text_offset */
	.quad	0x10000			/* image_size */
	.quad	0xa			/* flags: LE, 4K pages, place anywhere */
	.quad	0, 0, 0
	.ascii	"ARM\x64"
	.long	0

start:
	mov	x20, x0
	adr	x0, s_el
	bl	puts
	mrs	x0, CurrentEL
	lsr	x0, x0, #2
	add	x0, x0, #'0'
	bl	putc
	adr	x0, s_x0
	bl	puts
	mov	x0, x20
	bl	puthex
	ldr	w1, [x20]
	rev	w1, w1
	ldr	w2, =0xd00dfeed
	adr	x0, s_bad
	cmp	w1, w2
	b.ne	1f
	adr	x0, s_ok
1:	bl	puts
	b.ne	done
	ldr	w1, [x20, #16]		/* off_mem_rsvmap */
	rev	w1, w1
	add	x21, x20, x1
2:	ldp	x22, x23, [x21], #16
	rev	x22, x22
	rev	x23, x23
	orr	x1, x22, x23
	cbz	x1, done
	adr	x0, s_rsv
	bl	puts
	mov	x0, x22
	bl	puthex
	mov	x0, #' '
	bl	putc
	mov	x0, x23
	bl	puthex
	mov	x0, #'\n'
	bl	putc
	b	2b
done:
	adr	x0, s_done
	bl	puts
3:	wfi
	b	3b

putc:					/* w0; clobbers x9, x10 */
	ldr	x9, =UART
4:	ldr	w10, [x9, #0x18]
	tbnz	w10, #5, 4b		/* TXFF */
	str	w0, [x9]
	ret

puts:					/* x0 = NUL-terminated string */
	mov	x11, x30
	mov	x12, x0
5:	ldrb	w0, [x12], #1
	cbz	w0, 6f
	bl	putc
	b	5b
6:	ret	x11

puthex:					/* x0, as 0x%016x */
	mov	x13, x30
	mov	x14, x0
	mov	x0, #'0'
	bl	putc
	mov	x0, #'x'
	bl	putc
	mov	x15, #60
7:	lsr	x0, x14, x15
	and	x0, x0, #0xf
	cmp	x0, #10
	add	x1, x0, #'0'
	add	x2, x0, #('a' - 10)
	csel	x0, x1, x2, lo
	bl	putc
	subs	x15, x15, #4
	b.ge	7b
	ret	x13

s_el:	.asciz	"PROBE: EL"
s_x0:	.asciz	" x0="
s_ok:	.asciz	" magic=ok\n"
s_bad:	.asciz	" magic=bad\n"
s_rsv:	.asciz	"PROBE: rsv "
s_done:	.asciz	"PROBE: done\n"
	.align	3
	.ltorg
