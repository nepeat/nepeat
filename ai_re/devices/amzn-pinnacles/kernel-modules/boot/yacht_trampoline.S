/* SPDX-License-Identifier: GPL-2.0 */
#include <linux/linkage.h>

/*
 * This range is copied into one physically contiguous control page.  It must
 * be position-independent and smaller than the marker slot at +0xff0.
 */
    .pushsection .text.yacht_control, "ax"
    .align 6
    .global yacht_control_start
yacht_control_start:

    .global yacht_rehearse_entry
yacht_rehearse_entry:
    movz    x2, #0xcafe
    movk    x2, #0x5941, lsl #16
    movk    x2, #0x4843, lsl #32
    movk    x2, #0x5454, lsl #48
    str     x2, [x0, #0xff0]
    dsb     sy
    br      x1

    .align 6
    .global yacht_copy_test_entry
yacht_copy_test_entry:
    /* x0=source PA, x1=destination PA, x2=high return VA, x3=old SCTLR. */
    bic     x4, x3, #1
    bic     x4, x4, #4
    bic     x4, x4, #0x1000
    msr     sctlr_el1, x4
    isb

    mrs     x5, ctr_el0
    ubfx    x5, x5, #16, #4
    mov     x6, #4
    lsl     x5, x6, x5
    mov     x6, x1
    add     x7, x1, #4096
1:
    dc      ivac, x6
    add     x6, x6, x5
    cmp     x6, x7
    b.lo    1b

    mov     x6, #256
2:
    ldp     x7, x8, [x0], #16
    stp     x7, x8, [x1], #16
    subs    x6, x6, #1
    b.ne    2b
    dsb     sy
    ic      iallu
    msr     sctlr_el1, x3
    isb
    br      x2

    .align 6
    .global yacht_list_copy_test_entry
yacht_list_copy_test_entry:
    /* x0=list PA, x1=high return VA, x2=old SCTLR, x3=invalidate flag. */
    mov     x19, x0
    mov     x17, x1
    mov     x18, x2
    mov     x16, x3
    bic     x9, x18, #1
    bic     x9, x9, #4
    bic     x9, x9, #0x1000
    msr     sctlr_el1, x9
    isb
    mrs     x25, ctr_el0
    ubfx    x25, x25, #16, #4
    mov     x26, #4
    lsl     x25, x26, x25
3:
    ldr     x20, [x19]
    ldr     x21, [x19, #8]
    add     x22, x19, #16
    cbz     x21, 7f
4:
    ldp     x23, x24, [x22], #16
    cbz     x16, 8f
    mov     x26, x24
    add     x27, x24, #4096
5:
    dc      ivac, x26
    add     x26, x26, x25
    cmp     x26, x27
    b.lo    5b
8:
    mov     x26, #256
6:
    ldp     x27, x28, [x23], #16
    stp     x27, x28, [x24], #16
    subs    x26, x26, #1
    b.ne    6b
    subs    x21, x21, #1
    b.ne    4b
7:
    mov     x19, x20
    cbnz    x19, 3b
    dsb     sy
    ic      iallu
    msr     sctlr_el1, x18
    isb
    br      x17

    .align 6
    .global yacht_jump_entry
yacht_jump_entry:
    /* x0=list PA, x1=Image PA, x2=FDT PA. */
    mov     x19, x0
    mov     x17, x1
    mov     x18, x2

    /* The branch here is identity mapped.  Turn off MMU, D-cache and I-cache. */
    mrs     x9, sctlr_el1
    bic     x9, x9, #1
    bic     x9, x9, #4
    bic     x9, x9, #0x1000
    msr     sctlr_el1, x9
    isb
    mrs     x25, ctr_el0
    ubfx    x25, x25, #16, #4
    mov     x26, #4
    lsl     x25, x26, x25

1:  /* One descriptor page: next PA, count, then {source PA,dest PA}. */
    ldr     x20, [x19]
    ldr     x21, [x19, #8]
    add     x22, x19, #16
    cbz     x21, 5f
2:
    ldp     x23, x24, [x22], #16

    /* Discard every old destination cache line before replacing the page. */
    mov     x26, x24
    add     x27, x24, #4096
3:
    dc      ivac, x26
    add     x26, x26, x25
    cmp     x26, x27
    b.lo    3b

    /* Copy exactly one 4 KiB page. */
    mov     x26, #256
4:
    ldp     x27, x28, [x23], #16
    stp     x27, x28, [x24], #16
    subs    x26, x26, #1
    b.ne    4b
    subs    x21, x21, #1
    b.ne    2b
5:
    mov     x19, x20
    cbnz    x19, 1b

    dsb     sy
    ic      iallu
    tlbi    vmalle1
    dsb     sy
    isb

    /* Linux arm64 boot protocol: x0=FDT and x1..x3 reserved zero. */
    mov     x0, x18
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    br      x17

    .global yacht_control_end
yacht_control_end:
    .popsection

/*
 * Switch TTBR0 to a one-page 39-bit identity map, visit the low control page,
 * and return with the original translation and interrupt state restored.
 */
ENTRY(yacht_run_identity_test)
    stp     x19, x20, [sp, #-32]!
    stp     x21, x30, [sp, #16]
    mrs     x19, ttbr0_el1
    mrs     x20, tcr_el1
    mrs     x21, daif
    msr     daifset, #0xf
    bic     x4, x20, #(1 << 7)       /* TCR_EL1.EPD0 */
    msr     tcr_el1, x4
    isb
    msr     ttbr0_el1, x0
    isb
    tlbi    vmalle1
    dsb     sy
    isb
    mov     x0, x2                   /* control-page PA for marker store */
    adr     x1, 1f                   /* high virtual return address */
    br      x3                       /* rehearsal-entry PA */
1:
    msr     ttbr0_el1, x19
    msr     tcr_el1, x20
    isb
    tlbi    vmalle1
    dsb     sy
    isb
    msr     daif, x21
    ldp     x21, x30, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret
ENDPROC(yacht_run_identity_test)

/* x0=PGD PA, x1=copy-test entry PA, x2=source PA, x3=destination PA. */
ENTRY(yacht_run_copy_test)
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    mrs     x19, ttbr0_el1
    mrs     x20, tcr_el1
    mrs     x21, daif
    mrs     x22, sctlr_el1
    mov     x23, x1
    msr     daifset, #0xf
    bic     x4, x20, #(1 << 7)
    msr     tcr_el1, x4
    isb
    msr     ttbr0_el1, x0
    isb
    tlbi    vmalle1
    dsb     sy
    isb
    mov     x0, x2
    mov     x1, x3
    adr     x2, 1f
    mov     x3, x22
    br      x23
1:
    msr     ttbr0_el1, x19
    msr     tcr_el1, x20
    isb
    tlbi    vmalle1
    dsb     sy
    isb
    msr     daif, x21
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret
ENDPROC(yacht_run_copy_test)

/* x0=PGD PA, x1=list-copy entry PA, x2=list PA, x3=invalidate flag. */
ENTRY(yacht_run_list_copy_test)
    /* The low entry uses x19-x28; preserve every AAPCS64 callee-save. */
    stp     x19, x20, [sp, #-96]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    stp     x25, x26, [sp, #48]
    stp     x27, x28, [sp, #64]
    stp     x29, x30, [sp, #80]
    /* x19-x28 belong to the low list walker; keep return state elsewhere. */
    mrs     x29, ttbr0_el1
    mrs     x30, tcr_el1
    mrs     x15, daif
    mrs     x22, sctlr_el1
    mov     x23, x1
    msr     daifset, #0xf
    bic     x4, x30, #(1 << 7)
    msr     tcr_el1, x4
    isb
    msr     ttbr0_el1, x0
    isb
    tlbi    vmalle1
    dsb     sy
    isb
    mov     x0, x2
    adr     x1, 1f
    mov     x2, x22
    br      x23
1:
    msr     ttbr0_el1, x29
    msr     tcr_el1, x30
    isb
    tlbi    vmalle1
    dsb     sy
    isb
    msr     daif, x15
    ldp     x29, x30, [sp, #80]
    ldp     x27, x28, [sp, #64]
    ldp     x25, x26, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #96
    ret
ENDPROC(yacht_run_list_copy_test)

/* x0=PGD PA, x1=jump-entry PA, x2=list PA, x3=Image PA, x4=FDT PA. */
ENTRY(yacht_enter_jump)
	/* Preserve the low branch target before x1 becomes the Image entry. */
	mov     x6, x1
    msr     daifset, #0xf
    mrs     x5, tcr_el1
    bic     x5, x5, #(1 << 7)
    msr     tcr_el1, x5
    isb
    msr     ttbr0_el1, x0
    isb
    tlbi    vmalle1
    dsb     sy
    isb
    mov     x0, x2
	mov     x1, x3
    mov     x2, x4
	br      x6
ENDPROC(yacht_enter_jump)

/* PSCI 1.0 CPU_OFF through yacht's DT-selected SMC conduit. */
ENTRY(yacht_psci_cpu_off)
    dsb     sy
    movz    x0, #0x0002
    movk    x0, #0x8400, lsl #16
    movz    x1, #0x4000, lsl #16
    smc     #0
    ret
ENDPROC(yacht_psci_cpu_off)
