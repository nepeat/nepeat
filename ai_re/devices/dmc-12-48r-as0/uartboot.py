#!/usr/bin/env python3
"""DMC-12: run production stage3 from a RAM flash image (Route A), and the boot ROM's XMODEM.

Background and evidence: uart-boot.md. Offline subcommands never touch hardware.

  uartboot.py image --app 0x110 --uboot <u-boot.bin> out.bin   RAM flash image: our U-Boot in IPXE 0
  uartboot.py image --app 0x10000005 out.bin                   `u` test: factory UBOOT 1 vs 0, no new code
  uartboot.py image --app none out.bin                         mtd0 as is (BOOT_MODE blank, app 5)
  uartboot.py tramp out.bin                                    the AArch64 trampoline (cache clean + reset)
  uartboot.py route-a --image out.bin [--key u] [--run]        dry run unless --run
  uartboot.py s2 dumps/u01/mtd0.bin.zst stg2.s2                boot-ROM image (S2 header + payload + sum)
  sudo uartboot.py xmodem --tty /dev/serial/by-id/... stg2.s2  needs the tty to itself (stop tio first)

Every image blanks UBOOT_SCRIPT 0/1: in RAM-load ("UART recovery") mode the factory U-Boot
would otherwise run it, and with a default env the script ends in `saveenv` (an SPI write).

route-a, at the factory `ALPINE_DB>` prompt, through console.py and the tmux pane `serial-dmc12`:
TFTP the image to 0x01800000 and the trampoline to 0x01700000, copy a clean stage3 out of the
image to 0x01000000 and zero its .bss, set the stage2 shared-data flags to RAM load
(0xfbff41c1: bit0 on, bit1 off), invalidate the SRAM cpu-resume watermark (0xfbff4120 = 0) so
stage3 takes its cold-boot path (it enables VFP/NEON there; v2 hung on the first NEON instruction),
optionally enable the boot ROM's debug print (0xfbff4020 = 1: a stage3 exception then prints
"Exception! LR=..., CPSR=..." instead of a silent hang), then `go 0x01700000` (trampoline v3,
uart-boot.md section 5):
- EL2: copy a 48-byte EL3 stub over the agent's entry at 0x1000, clean D-cache (0x1000 line,
  0x01000000-0x02000000, the SRAM lines 0xfbff4000/4100/41c0), set the boot-ROM resume address 0xf0072028 =
  0x01000000, CPU0 power-on reset (0xf0070024 = 0x13). config_aarch64 (0xf0072004) and RVBAR are
  write-once and stage3 already set them to AArch64 / 0x1000, so CPU0 comes back at the stub, at EL3.
- EL3 stub: print '#' on UART0, then RMR_EL3 = AA64 0 | RR 1 and WFI: a warm reset into AArch32.
- AArch32 reset -> ROM vectors (VINITHI) -> resume address -> stage3 in secure SVC. Stage3
  re-copies the agent to 0x1000 itself, which removes the stub.
Afterwards **power-cycle** (AC off >= 10 s). Stage3's cold-boot path re-installs the SRAM agent, which
sets 0xf0072028 back to 0xfbff423c; if stage3 dies before that, 0x01000000 stays set until power-off.
"""
import argparse, os, pathlib, struct, subprocess, sys, termios, time, zlib

ROOT = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
import alboot  # noqa: E402  (offline helpers; symlink into ai_re/nix/packages/alboot)

MTD0 = ROOT / "dumps/u01/mtd0.bin.zst"
IMG_SIZE = 0x800000            # stage3 clears DRAM from 0x02000000 = RAM image + 8 MiB
IMG_ADDR, TRAMP_ADDR, STG3_ADDR = 0x01800000, 0x01700000, 0x01000000
STG3_BSS_END = 0x0105E000      # stage3 .bss ends ~0x0105d322 (highest reference)
FLAGS = 0xFBFF41C1             # al_stage2_shared_data.flags (magic at 0xfbff41c0)
SESSION = "serial-dmc12"

# AArch64, position independent; disassembly checked in Ghidra (uart-boot.md section 5).
# v1 also wrote config_aarch64 = 0: ignored (write-once), CPU0 came back at the agent (exp. 2).
# v2 reached stage3 via RMR_EL3 (exp. 2b); v3 adds the cache clean of the SRAM 0xfbff4000/4100 lines.
TRAMP = [
    0x10000505,  # adr  x5, stub (+0xa0)
    0xD2820006,  # mov  x6, #0x1000        agent entry = CPU0 RVBAR (write-once)
    0xA94020A7,  # ldp  x7, x8, [x5]
    0xA90020C7,  # stp  x7, x8, [x6]
    0xA94120A7,  # ldp  x7, x8, [x5, #16]
    0xA90120C7,  # stp  x7, x8, [x6, #16]
    0xA94220A7,  # ldp  x7, x8, [x5, #32]
    0xA90220C7,  # stp  x7, x8, [x6, #32]
    0xD50B7E26,  # dc civac, x6            (one 64-byte line: the whole stub)
    0xD2A02000,  # mov  x0, #0x01000000
    0xD2A04001,  # mov  x1, #0x02000000
    0xD50B7E20,  # 1: dc civac, x0
    0x91010000,  # add  x0, x0, #64
    0xEB01001F,  # cmp  x0, x1
    0x54FFFFA3,  # b.lo 1b
    0xD2880004,  # mov  x4, #0x4000
    0xF2BF7FE4,  # movk x4, #0xfbff, lsl #16
    0xD50B7E24,  # dc civac, x4            0xfbff4000 line (ROM state, debug flag)
    0x91040084,  # add  x4, x4, #0x100
    0xD50B7E24,  # dc civac, x4            0xfbff4100 line (cpu-resume watermark 0xfbff4120)
    0x91030084,  # add  x4, x4, #0xc0
    0xD50B7E24,  # dc civac, x4            0xfbff41c0 line (shared-data flags)
    0xD5033F9F,  # dsb  sy
    0xD2BE00E2,  # mov  x2, #0xf0070000    (NB service)
    0x52A02003,  # mov  w3, #0x01000000
    0xB9202843,  # str  w3, [x2, #0x2028]  CPU0 resume_addr_l: the ROM jumps here on AArch32 reset
    0xD5033F9F,  # dsb  sy
    0x52800263,  # mov  w3, #0x13
    0xB9002443,  # str  w3, [x2, #0x24]    cpus_software_reset: CPU0, level cpu_poreset (as stage3)
    0xD5033F9F,  # dsb  sy
    0xD5033FDF,  # isb
    0xD503207F,  # 2: wfi
    0x17FFFFFF,  # b    2b
] + [0xD503201F] * 7  # nop padding to +0xa0
STUB = [                # runs at 0x1000, EL3 AArch64, MMU off
    0xD2860001,  # mov  x1, #0x3000
    0xF2BFB101,  # movk x1, #0xfd88, lsl #16   UART0 THR
    0x52800462,  # mov  w2, #'#'
    0xB9000022,  # str  w2, [x1]              marker: "reached EL3 at the stub"
    0xD2800040,  # mov  x0, #2                 RMR_EL3: AA64 = 0, RR = 1
    0xD5033F9F,  # dsb  sy
    0xD5033FDF,  # isb
    0xD51EC040,  # msr  rmr_el3, x0
    0xD5033FDF,  # isb
    0xD503207F,  # 3: wfi                      warm reset is applied in WFI
    0x17FFFFFF,  # b    3b
    0xD503201F,  # nop (pad to 48 bytes)
]
TRAMP_BYTES = b"".join(struct.pack("<I", w) for w in TRAMP + STUB)
AGENT_HEAD = "58000904 f9000080 58000904 f9000080"   # stage3's agent at 0x1000 (stg3 +0x48ccc)

# --- agent trace (uart-boot.md section 5, "v4"): print one char at each step of the AArch64
# agent's CPU0 path, so a hang between stage3's "Executing application..." and the app is located.
# Stage3 copies the agent from its own image (0x01048ccc, 0x6dc9 B) to 0x1000, so the call sites
# are patched in the stage3 copy at 0x01000000; the hooks live at HOOK_ADDR (inside the range the
# trampoline cleans; nothing in stage3 uses it). Hooks use only x16/x17 (no stack) and poll LSR.THRE.
AGENT_SRC, AGENT_DST = 0x01048CCC, 0x1000
HOOK_ADDR = 0x01060000
AGENT_TRACE = [   # (agent address, char, kind): 'entry' = first instruction, else a `bl` site
    (0x1000, "A", "entry"),     # ldr x4, =0x7dc0 (displaced: mov x4, #0x7dc0)
    (0x1068, "M", 0x5930),      # agent main
    (0x611c, "Z", 0x5044),      # CPU0 parks: app entry 0xfbff40f8 read as 0
    (0x6188, "g", 0x53d8),      # GIC distributor init
    (0x61ac, "r", 0x5404),      # GIC redistributor SGI/PPI init
    (0x61fc, "D", 0x4018),      # DFX handshake (0xfd8e0000 = 1, wait 0xfd8e0080 == 2) + L2 regs
    (0x623c, "W", 0x54ac),      # GICR_WAKER wake + wait
    (0x6244, "S", 0x569c),      # SCR_EL3 / VBAR_EL3
    (0x6278, "E", 0x5720),      # ERET towards the app (EL2)
]


def _b(src, dst, link=False):
    off = (dst - src) // 4
    return (0x94000000 if link else 0x14000000) | (off & 0x3FFFFFF)


def _arm_b(src, dst, link=False, cond=0xE):
    return (cond << 28) | (0x0B000000 if link else 0x0A000000) | (((dst - src - 8) >> 2) & 0xFFFFFF)


def _arm_movw(rd, imm):
    return 0xE3000000 | ((imm >> 12) & 0xF) << 16 | rd << 12 | (imm & 0xFFF)


def _arm_movt(rd, imm):
    return 0xE3400000 | ((imm >> 12) & 0xF) << 16 | rd << 12 | (imm & 0xFFF)


def _arm_putc(base, tmp, val_insns, at):
    """ARM: wait LSR.THRE on UART0 (base reg already set), then store the char made by val_insns."""
    w = [0xE5900014 | base << 16 | tmp << 12,          # 1: ldr tmp, [base, #0x14]
         0xE3100020 | tmp << 16,                        # tst tmp, #0x20
         0]                                             # beq 1b (filled below)
    w[2] = _arm_b(at + 8, at, cond=0x0)
    return w + val_insns + [0xE5800000 | base << 16 | (val_reg(val_insns)) << 12]


def val_reg(insns):
    return (insns[-1] >> 12) & 0xF


# AArch32 hooks. R = the boot ROM's AArch32 resume path (via the SRAM agent's resume address).
# w = stage3 right before its CPU0 reset request, then config_aarch64 as '0'/'1'. stage3 runs there
# in Non-secure SVC (the SRAM agent's "exec via agent" switches to NS), so RMR is UNDEFINED there:
# v5 read RMR in this hook and the UNDEF flooded the console (2f). v5b reads/sets RMR in P instead.
# P = stage3's entry (0x01000000) right after our stub's RMR warm reset, Secure SVC: prints RMR.AA64,
# sets RMR = AA64 1 / RR 0 (unless --no-rmr-fix), prints it again, then runs stage3's first insn.
HOOK32_R, HOOK32_W, HOOK32_P = HOOK_ADDR + 0x200, HOOK_ADDR + 0x300, HOOK_ADDR + 0x380
STG3_ENTRY_INSN = 0x01000000      # ldr r3, =0x01059020 (displaced: movw/movt r3), then b 0x01000004
RESUME_LITERAL = 0x01047DC0       # SRAM agent init: resume address it gives all 4 CPUs (0xfbff423c)
STG3_RESET_CALL = 0x010294B4      # stage3 exec: bl 0x01023da0 (core warm reset), then wfi


def hooks32(rmr_fix):
    r = [_arm_movw(0, 0x3000), _arm_movt(0, 0xFD88)]  # r0/r1 are dead at 0xfbff423c's entry
    r += _arm_putc(0, 1, [0xE3A01000 | ord("R")], HOOK32_R + 4 * len(r))
    r += [_arm_movw(0, 0x423C), _arm_movt(0, 0xFBFF), 0xE12FFF10]   # bx r0 -> 0xfbff423c (ARM)
    w = [_arm_movw(2, 0x3000), _arm_movt(2, 0xFD88)]  # r2/r3/r12 are caller-saved at the bl
    w += _arm_putc(2, 12, [0xE3A03000 | ord("w")], HOOK32_W + 4 * len(w))
    w += _arm_putc(2, 12, [_arm_movw(3, 0x2004), _arm_movt(3, 0xF007), 0xE5933000,  # ldr r3, [r3]
                           0xE2033001, 0xE2833030],     # config_aarch64 & 1 -> '0'/'1'
                   HOOK32_W + 4 * len(w))
    w.append(_arm_b(HOOK32_W + 4 * len(w), 0x01023DA0))  # b 0x01023da0 (lr = site + 4)
    rmr = [0xEE1C3F50, 0xE2033001, 0xE2833030]          # mrc p15,0,r3,c12,c0,2 (RMR); and #1; add #'0'
    p = [_arm_movw(1, 0x3000), _arm_movt(1, 0xFD88)]  # r1/r2/r3 are dead at stage3's entry (r0, lr kept)
    p += _arm_putc(1, 2, [0xE3A03000 | ord("P")], HOOK32_P + 4 * len(p))
    p += _arm_putc(1, 2, list(rmr), HOOK32_P + 4 * len(p))
    if rmr_fix:
        p += [0xE3A03001, 0xEE0C3F50, 0xF57FF06F]       # mov r3, #1; mcr RMR (AA64=1, RR=0); isb
    p += _arm_putc(1, 2, list(rmr), HOOK32_P + 4 * len(p))
    p += [_arm_movw(3, 0x9020), _arm_movt(3, 0x0105)]   # the displaced ldr r3, =0x01059020
    p.append(_arm_b(HOOK32_P + 4 * len(p), STG3_ENTRY_INSN + 4))
    pack = lambda l: b"".join(struct.pack("<I", x) for x in l)
    return pack(r), pack(w), pack(p)


def agent_trace():
    """-> (hook blob for HOOK_ADDR, [(stage3 address, original word or None, new word)])."""
    blob, patches = [], []
    for i, (site, ch, kind) in enumerate(AGENT_TRACE):
        h = HOOK_ADDR + 0x20 * i
        w = [0xD2860010,                       # mov  x16, #0x3000
             0xF2BFB110,                       # movk x16, #0xfd88, lsl #16   (UART0)
             0xB9401611,                       # 1: ldr w17, [x16, #0x14]     (LSR)
             0x362FFFF1,                       # tbz  w17, #5, 1b             (THRE)
             0x52800011 | (ord(ch) << 5),      # mov  w17, #ch
             0xB9000211]                       # str  w17, [x16]
        if kind == "entry":
            w += [0xD28FB804,                  # mov  x4, #0x7dc0  (the displaced literal load)
                  _b(h + 0x1C, site + 4)]      # b    site + 4
            new = _b(site, h)                  # b    hook
        else:
            w += [_b(h + 0x18, kind), 0xD503201F]   # b F (x30 = site + 4 from the patched bl); nop
            new = _b(site, h, link=True)       # bl   hook
        blob += w
        patches.append((AGENT_SRC + site - AGENT_DST, new))
    return b"".join(struct.pack("<I", x) for x in blob), patches


HOOK32_X = HOOK_ADDR + 0x400      # v6: replaces w at stage3's reset call (never returns to stage3)


def hook_x():
    """v6 'w' replacement, ARM, at HOOK32_X; entered by the patched `bl 0x01023da0` (r0 = handle, r1 = 0).
    Prints: w<config_aarch64> <word at 0x1000> then calls stage3's reset request itself. If that
    returns: q<cpus_software_reset> <cpu0 power_status> <cpus_hold_reset>, 16 dots (~a delay loop each),
    then WFI up to 8 times, '!' after each WFI that returns. Never returns to stage3."""
    code, labels, fix = [], {}, []
    here = lambda: HOOK32_X + 4 * len(code)
    def putc_r3():
        at = here()
        code.extend([0xE592C014, 0xE31C0020, _arm_b(at + 8, at, cond=0x0), 0xE5823000])
    def putc(ch):
        code.append(0xE3A03000 | ord(ch)); putc_r3()
    def hexw(addr=None):            # print r5 (or [addr]) as 8 hex digits via the `hex` routine
        if addr is not None:
            code.extend([_arm_movw(4, addr & 0xFFFF), _arm_movt(4, addr >> 16), 0xE5945000])
        fix.append((len(code), "hex", True)); code.append(0)
    code.extend([_arm_movw(2, 0x3000), _arm_movt(2, 0xFD88)])          # r2 = UART0
    putc("w")
    code.extend([_arm_movw(3, 0x2004), _arm_movt(3, 0xF007), 0xE5933000, 0xE2033001, 0xE2833030])
    putc_r3()
    putc(" "); hexw(0x1000)
    code.append(_arm_b(here(), 0x01023DA0, link=True))                 # stage3's reset request
    code.extend([_arm_movw(2, 0x3000), _arm_movt(2, 0xFD88)])          # r2 is caller-saved: reload
    putc("q"); hexw(0xF0070024); putc(" "); hexw(0xF0072024); putc(" "); hexw(0xF0070020)
    code.append(0xE3A07010)                                            # mov r7, #16
    labels["dots"] = len(code)
    code.extend([_arm_movw(8, 0), _arm_movt(8, 0x80)])                 # r8 = 0x800000
    labels["d1"] = len(code)
    code.append(0xE2588001)                                            # subs r8, r8, #1
    fix.append((len(code), "d1", 0x1)); code.append(0)                 # bne d1
    putc(".")
    code.append(0xE2577001)                                            # subs r7, r7, #1
    fix.append((len(code), "dots", 0x1)); code.append(0)               # bne dots
    code.append(0xE3A07008)                                            # mov r7, #8
    labels["wl"] = len(code)
    code.extend([0xF57FF04F, 0xF57FF06F, 0xE320F003])                  # dsb; isb; wfi
    putc("!")
    code.append(0xE2577001)
    fix.append((len(code), "wl", 0x1)); code.append(0)                 # bne wl
    code.append(_arm_b(here(), here()))                                # b .
    labels["hex"] = len(code)                                          # hex: r5 -> 8 digits, r6 count
    code.append(0xE3A06008)                                            # mov r6, #8
    labels["h1"] = len(code)
    code.extend([0xE1A03E25, 0xE353000A, 0xB2833030, 0xA2833057])      # r3 = r5>>28; <10 ? +'0' : +'a'-10
    putc_r3()
    code.extend([0xE1A05205, 0xE2566001])                              # r5 <<= 4; subs r6, #1
    fix.append((len(code), "h1", 0x1)); code.append(0)                 # bne h1
    code.append(0xE12FFF1E)                                            # bx lr
    for i, lab, kind in fix:
        src, dst = HOOK32_X + 4 * i, HOOK32_X + 4 * labels[lab]
        code[i] = _arm_b(src, dst, link=True) if kind is True else _arm_b(src, dst, cond=kind)
    return b"".join(struct.pack("<I", x) for x in code)


def trace_blob(rmr_fix=True):
    """The whole 0x400-byte blob at HOOK_ADDR and every patch in the stage3 copy."""
    a64, patches = agent_trace()
    r, w, p = hooks32(rmr_fix)
    assert len(a64) <= 0x200 and len(r) <= 0x100 and len(w) <= 0x80 and len(p) <= 0x80
    x = hook_x()
    assert len(x) <= 0x200
    blob = bytearray(0x600)
    blob[0x400:0x400 + len(x)] = x
    blob[:len(a64)] = a64
    blob[0x200:0x200 + len(r)] = r
    blob[0x300:0x300 + len(w)] = w
    blob[0x380:0x380 + len(p)] = p
    patches = patches + [(STG3_ENTRY_INSN, _arm_b(STG3_ENTRY_INSN, HOOK32_P)),
                         (RESUME_LITERAL, HOOK32_R),
                         (STG3_RESET_CALL, _arm_b(STG3_RESET_CALL, HOOK32_X, link=True))]
    return bytes(blob), patches


def toc_entries(d):
    o, n, ok = alboot.find_toc(d)
    if o is None or not ok:
        raise SystemExit("no valid TOC")
    for i in range(n):
        e = o + 0x14 + i * 0x20
        idi, = struct.unpack_from("<I", d, e)
        off, mx = struct.unpack_from("<II", d, e + 0x10)
        yield idi & 0x0FFFFFFF, idi >> 28, off, mx


def slot(d, oid, inst):
    for i, n, off, mx in toc_entries(d):
        if i == oid and n == inst:
            return off, mx
    raise SystemExit(f"object {oid:#x}/{inst} not in TOC")


def obj(oid, load, data):
    h = struct.pack("<6I16s3QI", alboot.OBJ_MAGIC, 0, oid, 2, 60, 4,
                    b"N/A".ljust(16, b"\0"), len(data), load, load, 0)
    h += struct.pack("<I", alboot.bsum(h))
    return h + data + struct.pack("<I", alboot.bsum(data))


def put(img, d, oid, inst, blob):
    off, mx = slot(d, oid, inst)
    if len(blob) > mx or off + mx > IMG_SIZE:
        raise SystemExit(f"{oid:#x}/{inst}: {len(blob):#x} B at {off:#x} does not fit")
    img[off:off + mx] = blob.ljust(mx, b"\xff")


def cmd_image(a):
    d = alboot.load(str(a.mtd))
    img = bytearray(d[:IMG_SIZE])
    for inst in (0, 1):
        put(img, d, 6, inst, b"")                       # UBOOT_SCRIPT blank: no saveenv path
    if a.app != "none":
        app = int(a.app, 0)
        if app & 0x0FFFFFFF == 0x110:
            if not a.uboot:
                raise SystemExit("--app 0x110 needs --uboot u-boot.bin (linked at 0x08000000)")
            put(img, d, 0x110, app >> 28, obj(0x110, 0x08000000, a.uboot.read_bytes()))
        put(img, d, 0, 0, obj(0, 0, struct.pack("<II", 0, app)))   # stage3 reads word +4 only
    so, _ = slot(d, 4, 0)
    st, f = alboot.check_object(img, so, 0)
    if st != "OK":
        raise SystemExit(f"STG3 0 in image: {st}")
    stg3 = bytes(img[so + alboot.HDR:so + alboot.HDR + f["size"]])
    if a.out.exists():
        raise SystemExit(f"refusing to replace {a.out}")
    a.out.write_bytes(img)
    meta = {"crc": zlib.crc32(img), "stg3_src": IMG_ADDR + so + alboot.HDR,
            "stg3_size": len(stg3), "stg3_crc": zlib.crc32(stg3)}
    a.out.with_suffix(".txt").write_text("".join(f"{k}={v:#x}\n" for k, v in meta.items()))
    for i, n, off, mx in toc_entries(img):
        if off < IMG_SIZE and i in (0, 4, 5, 6, 0x110):
            print(f"  {alboot.NAMES.get(i, hex(i)):12s} {n} @{off:#08x}: "
                  f"{alboot.check_object(img, off, mx, i)[0]}")
    print(f"{a.out}: {len(img):#x} B crc32 {meta['crc']:08x}; stage3 {meta['stg3_size']:#x} B "
          f"crc32 {meta['stg3_crc']:08x} (meta in {a.out.with_suffix('.txt').name})")


def cmd_tramp(a):
    if a.out.exists():
        raise SystemExit(f"refusing to replace {a.out}")
    a.out.write_bytes(TRAMP_BYTES)
    print(f"{a.out}: {len(TRAMP_BYTES)} B crc32 {zlib.crc32(a.out.read_bytes()):08x}")


def console(c, timeout=30):
    r = subprocess.run([sys.executable, str(ROOT / "console.py"), "--timeout", str(timeout), c],
                       capture_output=True, text=True)
    print(f"> {c}\n{r.stdout.rstrip()}")
    if r.returncode:
        raise SystemExit(f"console.py failed: {r.stderr.strip()}")
    return r.stdout


def pane():
    return subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", SESSION],
                          capture_output=True, text=True, errors="replace").stdout


def crc_of(out):
    last = None
    for w in out.split():
        if len(w) == 8 and all(c in "0123456789abcdef" for c in w):
            last = w
    if last is None:
        raise SystemExit(f"no crc32 in: {out!r}")
    return int(last, 16)


def cmd_route_a(a):
    meta = dict(l.split("=") for l in a.image.with_suffix(".txt").read_text().split())
    meta = {k: int(v, 0) for k, v in meta.items()}
    if zlib.crc32(a.image.read_bytes()) != meta["crc"]:
        raise SystemExit("image does not match its .txt")
    tramp = TRAMP_BYTES
    d = a.tftp_root / a.tftp_dir
    steps = ["dhcp", f"setenv serverip {a.server}",
             f"tftpboot {IMG_ADDR:#x} {a.tftp_dir}/{a.image.name}",
             f"crc32 {IMG_ADDR:#x} {IMG_SIZE:#x}",
             f"tftpboot {TRAMP_ADDR:#x} {a.tftp_dir}/tramp.bin",
             f"crc32 {TRAMP_ADDR:#x} {len(tramp):#x}",
             f"cp.b {meta['stg3_src']:#x} {STG3_ADDR:#x} {meta['stg3_size']:#x}",
             f"crc32 {STG3_ADDR:#x} {meta['stg3_size']:#x}",
             f"mw.b {STG3_ADDR + meta['stg3_size']:#x} 0 {STG3_BSS_END - STG3_ADDR - meta['stg3_size']:#x}",
             *([f"tftpboot {HOOK_ADDR:#x} {a.tftp_dir}/agent-trace.bin; crc32",
                "mw.l <11 sites in the stage3 copy: 9 agent calls, the SRAM agent resume literal, the reset call>"]
               if a.trace_agent else []),
             "md.l 0xfd8e0000 0x24   (DFX: the agent waits for 0xfd8e0080 == 2)",
             "md.l 0xf0280014 1   (GICR_WAKER CPU0)",
             "md.b 0xfbff41c0 4", "mw.b 0xfbff41c1 <old & ~3 | 1>", "md.b 0xfbff41c0 4",
             "md.l 0xf0072000 0x14", "md.l 0x1000 4   (expect the agent: " + AGENT_HEAD + ")",
             "md.l 0xfbff4120 2", "mw.l 0xfbff4120 0   (stage3 cold-boot path: VFP on, SRAM agent re-installed)",
             "md.l 0xfbff4010 0xc", "mw.l 0xfbff4020 1   (ROM debug print, only if 0xfbff4014 and 0xfbff4038/403c are 0)",
             f"go {TRAMP_ADDR:#x}   (then key {a.key!r} every 20 ms)" if a.key
             else f"go {TRAMP_ADDR:#x}"]
    print(f"image crc32 {meta['crc']:08x}, tramp crc32 {zlib.crc32(tramp):08x}, "
          f"stage3 crc32 {meta['stg3_crc']:08x}; TFTP dir {d}")
    if not a.run:
        print("dry run; commands:\n  " + "\n  ".join(steps))
        return
    d.mkdir(parents=True, exist_ok=True)
    (d / a.image.name).write_bytes(a.image.read_bytes())
    (d / "tramp.bin").write_bytes(tramp)
    # dhcp first: it overwrites serverip with the DHCP server's address (TFTP went to
    # the router once, 2026-11-15).
    if "bound to address" not in console("dhcp", 60):
        raise SystemExit("dhcp failed")
    console(f"setenv serverip {a.server}")
    for addr, name, size, want in ((IMG_ADDR, a.image.name, IMG_SIZE, meta["crc"]),
                                   (TRAMP_ADDR, "tramp.bin", len(tramp), zlib.crc32(tramp))):
        console(f"tftpboot {addr:#x} {a.tftp_dir}/{name}", 180)
        if crc_of(console(f"crc32 {addr:#x} {size:#x}")) != want:
            raise SystemExit(f"{name}: crc mismatch")
    console(f"cp.b {meta['stg3_src']:#x} {STG3_ADDR:#x} {meta['stg3_size']:#x}", 60)
    if crc_of(console(f"crc32 {STG3_ADDR:#x} {meta['stg3_size']:#x}")) != meta["stg3_crc"]:
        raise SystemExit("stage3 copy: crc mismatch")
    console(f"mw.b {STG3_ADDR + meta['stg3_size']:#x} 0 {STG3_BSS_END - STG3_ADDR - meta['stg3_size']:#x}")
    if a.trace_agent:
        hooks, patches = trace_blob(a.rmr_fix)
        (d / "agent-trace.bin").write_bytes(hooks)
        console(f"tftpboot {HOOK_ADDR:#x} {a.tftp_dir}/agent-trace.bin", 60)
        if crc_of(console(f"crc32 {HOOK_ADDR:#x} {len(hooks):#x}")) != zlib.crc32(hooks):
            raise SystemExit("agent-trace.bin: crc mismatch")
        img = a.image.read_bytes()
        for addr, new in patches:
            old, = struct.unpack_from("<I", img, meta["stg3_src"] - IMG_ADDR + addr - STG3_ADDR)
            if f"{old:08x}" not in console(f"md.l {addr:#x} 1"):
                raise SystemExit(f"{addr:#x}: stage3 copy does not hold {old:08x}")
            console(f"mw.l {addr:#x} {new:#010x}")
            if f"{new:08x}" not in console(f"md.l {addr:#x} 1"):
                raise SystemExit(f"{addr:#x}: patch did not stick")
    console("md.l 0xfd8e0000 0x24")
    console("md.l 0xf0280014 1")
    line = [l for l in console("md.b 0xfbff41c0 4").splitlines() if l.startswith("fbff41c0:")][0]
    magic, old = int(line.split()[1], 16), int(line.split()[2], 16)
    if magic < 0x3B:
        raise SystemExit(f"shared-data magic {magic:#x} < 0x3b: stage3 would ignore the flags")
    console(f"mw.b {FLAGS:#x} {(old & ~3) | 1:#x}")
    console("md.b 0xfbff41c0 4")
    console("md.l 0xf0072000 0x14")
    if AGENT_HEAD not in console("md.l 0x1000 4"):
        raise SystemExit("0x1000 does not hold stage3's agent: refusing to patch it")
    console("md.l 0xfbff4120 2")
    console("mw.l 0xfbff4120 0")
    if "fbff4120: 00000000" not in console("md.l 0xfbff4120 1"):
        raise SystemExit("cpu-resume watermark did not clear")
    if a.rom_debug:
        w = {}
        for l in console("md.l 0xfbff4010 0xc").splitlines():
            if l[:8] in ("fbff4010", "fbff4020", "fbff4030"):
                for i, v in enumerate(l[9:].split()[:4]):
                    w[int(l[:8], 16) + 4 * i] = int(v, 16)
        if w.get(0xFBFF4014) == 0 and w.get(0xFBFF4038) == 0 and w.get(0xFBFF403C) == 0:
            console("mw.l 0xfbff4020 1")
        else:
            print("ROM debug print not enabled: unexpected ROM state words", {hex(k): hex(v) for k, v in w.items()})
    marks = ("Stage 3 version", "UART recovery!", "User request", "Loading application",
             "Executing application", "U-Boot 20", "recovery boot command", "Entry: EL",
             "Exception!", "Hit any key")
    go = f"go {TRAMP_ADDR:#x}"

    def after_go():
        # only the text after the last `go` line counts (the visible screen survives clear-history)
        t = pane()
        i = t.rfind(go)
        return t[i + len(go):] if i >= 0 else ""
    subprocess.run(["tmux", "clear-history", "-t", SESSION], check=True)
    for byte in go.encode():
        subprocess.run(["tmux", "send-keys", "-t", SESSION, "-H", f"{byte:02x}"], check=True)
        time.sleep(0.02)
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "Enter"], check=True)
    t0, sent = time.monotonic(), 0
    while time.monotonic() - t0 < a.timeout:
        t = after_go()
        if a.key and "Loading application" not in t:
            subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", a.key], check=True)
            sent += 1
        if "Hit any key" in t or "Exception!" in t:
            break
        time.sleep(0.02)
    time.sleep(2)
    t = after_go()
    print(f"key {a.key!r} sent {sent}x" if a.key else "no key")
    stub = [l.strip() for l in t.splitlines() if l.strip().startswith("#") and not l.strip().startswith("##")]
    print(f"  {'# (EL3 stub)':22s} {stub[0] if stub else '-- not seen'}")
    if a.trace_agent:
        # the agent prints its chars right after stage3's "Executing application..." line
        i = t.find("Executing application")
        tail = t[i:].split("\n", 1)[1][:200] if i >= 0 and "\n" in t[i:] else ""
        print(f"  {'trace after exec':22s} {tail.splitlines()[0] if tail.strip() else '-- none'}"
              f"   (w<config_aarch64>, then AArch64 {''.join(c for _, c, _ in AGENT_TRACE)}"
              f" (Z = parked) or R = back in AArch32 via the ROM)")
        pre = t[:i] if i >= 0 else t
        k = pre.find("P")
        print(f"  {'P (stage3 entry)':22s} {pre[k:k+3] if k >= 0 else '-- not seen'}   (P<RMR.AA64 before><after>)")
        print(f"  {'R before exec':22s} {'yes (exec via SRAM agent, expected once)' if 'R' in pre else 'no'}")
    for m in marks:
        hits = [l.strip() for l in t.splitlines() if m in l]
        print(f"  {m:22s} {hits[0] if hits else '-- not seen'}")
    print("Done. Power-cycle (AC off >= 10 s) before anything else.")


def cmd_s2(a):
    d = alboot.load(str(a.mtd))
    st, info = alboot.check_s2(d, 0, 0x40000)
    if not st.startswith("OK"):
        raise SystemExit(f"S2 at 0: {st}")
    n = 0x20 + info["payload"] + 2
    if n > 0x68000:
        raise SystemExit("larger than the ROM's XMODEM limit 0x68000")
    if a.out.exists():
        raise SystemExit(f"refusing to replace {a.out}")
    a.out.write_bytes(d[:n])
    print(f"{a.out}: {n:#x} B (S2 header + payload + sum16); the ROM runs it at 0xf2200020")


def holders(tty):
    real, me, out = os.path.realpath(tty), os.getpid(), []
    for p in os.listdir("/proc"):
        if not p.isdigit() or int(p) == me:
            continue
        try:
            for fd in os.listdir(f"/proc/{p}/fd"):
                if os.path.realpath(f"/proc/{p}/fd/{fd}") == real:
                    out.append(p)
                    break
        except OSError:
            continue
    return out


def crc16(b):
    c = 0
    for x in b:
        c ^= x << 8
        for _ in range(8):
            c = ((c << 1) ^ 0x1021) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return c


def cmd_xmodem(a):
    data = a.file.read_bytes()
    if len(data) > 0x68000:
        raise SystemExit("ROM XMODEM limit is 0x68000 B (it truncates silently)")
    busy = holders(a.tty)
    if busy:
        raise SystemExit(f"{a.tty} is open by pid {', '.join(busy)} (tio?). Stop it first, see --help")
    fd = os.open(a.tty, os.O_RDWR | os.O_NOCTTY)
    at = termios.tcgetattr(fd)
    at[0] = at[1] = at[3] = 0
    at[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    at[4] = at[5] = termios.B115200
    at[6][termios.VMIN], at[6][termios.VTIME] = 0, 1
    termios.tcsetattr(fd, termios.TCSANOW, at)
    termios.tcflush(fd, termios.TCIOFLUSH)

    def rd(t):
        end = time.monotonic() + t
        while time.monotonic() < end:
            b = os.read(fd, 1)
            if b:
                return b[0]
        return None
    print("waiting for the ROM's 'C' (CRC) or NAK (power the box on now) ...")
    end, crc = time.monotonic() + a.wait, None
    while time.monotonic() < end and crc is None:
        c = rd(1)
        if c == 0x43:
            crc = True
        elif c == 0x15:
            crc = False
        elif c is not None:
            sys.stderr.write(chr(c) if 32 <= c < 127 or c == 10 else ".")
    if crc is None:
        raise SystemExit("no receiver")
    blocks = [data[i:i + 128].ljust(128, b"\x1a") for i in range(0, len(data), 128)]
    for n, blk in enumerate(blocks, 1):
        tail = struct.pack(">H", crc16(blk)) if crc else bytes([sum(blk) & 0xFF])
        pkt = bytes([1, n & 0xFF, 0xFF - (n & 0xFF)]) + blk + tail
        for _ in range(10):
            os.write(fd, pkt)
            r = rd(3)
            if r == 0x06:
                break
            if r == 0x18:
                raise SystemExit(f"receiver cancelled at block {n}")
        else:
            raise SystemExit(f"block {n}: no ACK after 10 tries")
        if n % 64 == 0:
            print(f"  {n * 128:#x}/{len(data):#x}")
    for _ in range(10):
        os.write(fd, b"\x04")
        if rd(3) == 0x06:
            print(f"sent {len(data):#x} B in {len(blocks)} blocks ({'CRC' if crc else 'sum'})")
            break
    else:
        raise SystemExit("no ACK for EOT")
    end = time.monotonic() + a.listen
    while time.monotonic() < end:
        c = rd(0.2)
        if c is not None:
            sys.stdout.write(chr(c) if 32 <= c < 127 or c in (10, 13) else ".")
            sys.stdout.flush()
    os.close(fd)


ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
sub = ap.add_subparsers(dest="cmd", required=True)
p = sub.add_parser("image")
p.add_argument("--mtd", type=pathlib.Path, default=MTD0)
p.add_argument("--app", required=True, help="BOOT_MODE app id (0x110, 0x10000005, ...) or 'none'")
p.add_argument("--uboot", type=pathlib.Path, help="u-boot.bin linked at 0x08000000 (for 0x110)")
p.add_argument("out", type=pathlib.Path)
p = sub.add_parser("tramp")
p.add_argument("out", type=pathlib.Path)
p = sub.add_parser("route-a")
p.add_argument("--image", type=pathlib.Path, required=True)
p.add_argument("--key", help="stage3 key to send from `go` on (u, p, e)")
p.add_argument("--run", action="store_true", help="drive the factory prompt (default: dry run)")
p.add_argument("--server", default="10.60.2.208")
p.add_argument("--tftp-root", type=pathlib.Path, default=pathlib.Path("/srv/tftpboot"))
p.add_argument("--tftp-dir", default="dmc12/route-a")
p.add_argument("--timeout", type=float, default=60)
p.add_argument("--no-agent-trace", dest="trace_agent", action="store_false",
               help="don't patch print hooks into the agent copy (they stay resident: debug runs only)")
p.add_argument("--no-rmr-fix", dest="rmr_fix", action="store_false",
               help="trace only: don't set RMR.AA64 = 1 in the stage3-entry hook (P)")
p.add_argument("--no-rom-debug", dest="rom_debug", action="store_false",
               help="don't set the boot ROM's debug-print flag (0xfbff4020)")
p = sub.add_parser("s2")
p.add_argument("mtd", type=pathlib.Path)
p.add_argument("out", type=pathlib.Path)
p = sub.add_parser("xmodem", description="Stop tio first: tmux send-keys -t serial-dmc12 C-t q "
                   "(the pane exits). Restart it afterwards with the same tio command and a new "
                   "--log-file (PROGRESS Status). Needs root for the tty, like tio.")
p.add_argument("--tty", required=True)
p.add_argument("--wait", type=float, default=600, help="seconds to wait for the receiver")
p.add_argument("--listen", type=float, default=20, help="seconds to echo output afterwards")
p.add_argument("file", type=pathlib.Path)
a = ap.parse_args()
{"image": cmd_image, "tramp": cmd_tramp, "route-a": cmd_route_a, "s2": cmd_s2,
 "xmodem": cmd_xmodem}[a.cmd](a)
