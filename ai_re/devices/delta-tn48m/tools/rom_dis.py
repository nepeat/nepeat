#!/usr/bin/env python3
"""Disassemble a range of a raw AArch64 image, folding mov/movk constants and
resolving literal loads / adrp+add strings.  usage: rom_dis.py <image> <start> <end> [base]"""
import sys, re
from capstone import Cs, CS_ARCH_ARM64, CS_MODE_ARM
d = open(sys.argv[1], 'rb').read()
a, b = int(sys.argv[2], 0), int(sys.argv[3], 0)
base = int(sys.argv[4], 0) if len(sys.argv) > 4 else 0
md = Cs(CS_ARCH_ARM64, CS_MODE_ARM)
def cstr(t):
    t -= base
    if 0 <= t < len(d) and 32 <= d[t] < 127:
        e = d.find(b'\0', t); s = d[t:e]
        if len(s) >= 4 and all(32 <= c < 127 or c in (10, 13) for c in s): return s.decode()[:60].strip()
regs = {}; prev = None
for off in range(a, b, 4):
    l = list(md.disasm(d[off:off+4], off + base))
    if not l: print("%6x: .word %08x" % (off + base, int.from_bytes(d[off:off+4], 'little'))); continue
    i = l[0]; ops = i.op_str.split(', '); extra = ''
    r = ops[0].replace('w', 'x') if ops and ops[0][:1] in 'wx' else None
    if i.mnemonic == 'mov' and len(ops) == 2 and ops[1].startswith('#'):
        regs[r] = int(ops[1][1:], 0) & 0xffffffffffffffff
    elif i.mnemonic == 'movk' and r in regs:
        sh = int(ops[2].split('#')[1]) if len(ops) > 2 else 0
        regs[r] = (regs[r] & ~(0xffff << sh)) | (int(ops[1][1:], 0) << sh); extra = '  ; = %#x' % regs[r]
    elif i.mnemonic == 'adrp':
        regs[r] = int(ops[1][1:], 0)
    elif i.mnemonic == 'add' and len(ops) == 3 and ops[1].replace('w', 'x') in regs and ops[2].startswith('#') and prev and prev.mnemonic == 'adrp':
        t = regs[ops[1].replace('w', 'x')] + int(ops[2][1:], 0); s = cstr(t)
        extra = '  ; %#x%s' % (t, ' "%s"' % s if s else '')
    elif i.mnemonic == 'adr':
        t = int(ops[1][1:], 0); s = cstr(t); extra = '  ; "%s"' % s if s else ''
    elif i.mnemonic in ('ldr', 'ldrsw') and len(ops) == 2 and ops[1].startswith('#'):
        t = int(ops[1][1:], 0) - base; extra = '  ; = %#x' % int.from_bytes(d[t:t+8], 'little')
    print("%6x: %-6s %s%s" % (i.address, i.mnemonic, i.op_str, extra)); prev = i
