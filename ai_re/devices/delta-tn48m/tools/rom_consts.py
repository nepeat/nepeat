#!/usr/bin/env python3
"""List constants built with mov/movk in a raw AArch64 image (default: whole file).
usage: rom_consts.py <image> [start end] -- prints value and the code offsets using it."""
import sys, collections
from capstone import Cs, CS_ARCH_ARM64, CS_MODE_ARM
d = open(sys.argv[1], 'rb').read()
a, b = (int(sys.argv[2], 0), int(sys.argv[3], 0)) if len(sys.argv) > 3 else (0, len(d))
md = Cs(CS_ARCH_ARM64, CS_MODE_ARM)
regs, out = {}, collections.defaultdict(list)
def flush(r):
    if r in regs:
        v, at = regs.pop(r)
        if v >= 0x10000: out[v].append(at)
for off in range(a, b, 4):
    l = list(md.disasm(d[off:off+4], off))
    if not l: regs.clear(); continue
    i = l[0]; ops = i.op_str.split(', ')
    r = ops[0].replace('w', 'x') if ops and ops[0][:1] in 'wx' else None
    if i.mnemonic == 'mov' and len(ops) == 2 and ops[1].startswith('#'):
        flush(r); regs[r] = (int(ops[1][1:], 0) & 0xffffffffffffffff, off)
    elif i.mnemonic == 'movk' and r in regs:
        sh = int(ops[2].split('#')[1]) if len(ops) > 2 else 0
        v, at = regs[r]; regs[r] = ((v & ~(0xffff << sh)) | (int(ops[1][1:], 0) << sh), at)
    elif i.mnemonic in ('ret', 'b', 'br'):
        for k in list(regs): flush(k)
    elif r in regs and i.mnemonic not in ('str', 'strb', 'strh', 'stp', 'cmp', 'tst', 'cbz', 'cbnz', 'tbz', 'tbnz'):
        flush(r)
for k in list(regs): flush(k)
for v in sorted(out):
    print("%12x  @ %s" % (v, " ".join("%x" % x for x in out[v][:10]) + (" ..." if len(out[v]) > 10 else "")))
