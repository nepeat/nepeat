#!/usr/bin/env python3
"""Decode sections of a re-host MDIO trace into register-level operations (LMI accesses, firmware messages).

  seqtool.py decode TRACE [SECTION-SUBSTRING]     print every section (or the matching ones) decoded
Keys appear in SA messages: output stays under dumps/ or on the terminal, never in git (--redact masks record bytes).
Used to write and check nix/modules/boards/as7772/ext-macsec.nix (../macsec.md "Under SONiC").
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lmi


def sections(path):
    """[(title, [(kind, phy, dev, reg, val) | ('D', usec)])] split at the '# <command>' markers (rc lines dropped)."""
    out, cur = [], None
    for l in open(path):
        f = l.split()
        if not f:
            continue
        if f[0] == '#':
            if len(f) > 1 and f[1].startswith('rc='):
                continue
            cur = (' '.join(f[1:]), [])
            out.append(cur)
            continue
        if cur is None:
            cur = ('(start)', [])
            out.append(cur)
        if f[0] == 'D':
            cur[1].append(('D', int(f[1])))
        else:
            cur[1].append((f[0], int(f[1], 16), int(f[2], 16), int(f[3], 16), int(f[4], 16)))
    return out


def decode(ops, redact=False):
    """High-level view: 'lw blk addr val', 'lr blk addr val', 'msg cmd bytes -> ack [data]'. Delays are dropped."""
    out, p, msg, i = [], None, {}, 0
    rd = None
    for o in ops:
        if o[0] == 'D':
            continue
        k, phy, dev, reg, val = o
        if p is None:
            p = lmi.Parser(phy)
        if reg in (lmi.LMI_FIFO, lmi.LMI_SIDE):
            op = p.write(reg, val) if k == 'W' else p.read(reg, val)
            if op:
                out.append('%s %x %05x %s' % ('lw' if op.write else 'lr', op.block, op.full_addr,
                                               ' '.join('%0*x' % (op.width * 2, v) for v in op.values())))
        elif reg == lmi.LMI_STATUS:
            pass
        elif 0xa000 <= reg < 0xa100 and k == 'W':
            msg[reg - 0xa000] = val
        elif reg == 0x8222 and k == 'W':
            n = (msg.get(0, 0) & 0xff) + 2
            b = bytearray()
            for j in range((n + 1) // 2):
                b += bytes((msg.get(j, 0) & 0xff, msg.get(j, 0) >> 8))
            b = bytes(b[:n])
            if redact and len(b) > 60:
                b = b[:17] + b'\xee' * 48 + b[65:]
            out.append('msg %04x %s' % (val, b.hex()))
            rd = []
        elif reg == 0x8221 and k == 'R':
            out[-1] += ' -> %04x' % val if out and out[-1].startswith('msg') else ''
        elif 0xa100 <= reg < 0xa200 and k == 'R':
            out.append('   rd %04x %04x' % (reg, val))
        else:
            out.append('%s %x.%04x %04x' % (k, dev, reg, val))
    return out


if __name__ == '__main__':
    if len(sys.argv) < 3 or sys.argv[1] != 'decode':
        raise SystemExit(__doc__)
    redact = '--redact' in sys.argv
    a = [x for x in sys.argv[2:] if x != '--redact']
    for title, ops in sections(a[0]):
        if len(a) > 1 and a[1] not in title:
            continue
        print('## ' + title)
        for l in decode(ops, redact):
            print(l)
