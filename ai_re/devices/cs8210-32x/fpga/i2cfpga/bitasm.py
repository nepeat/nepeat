#!/usr/bin/env python3
"""Xilinx 7-series configuration image <-> readable packet listing.

    bitasm.py dis IMAGE OUTDIR    write OUTDIR/listing.txt + OUTDIR/blobs/*.bin
    bitasm.py asm OUTDIR IMAGE    rebuild IMAGE from them
    bitasm.py check IMAGE         dis + asm into a temp dir, compare bytes

The listing is line based and complete: every byte of the input is
represented, so `asm` reproduces the image exactly. Frame-data payloads
(FDRI, MFWR) are kept in blob files named after their offset; their
contents are interpreted at the next layer (frames / FASM).

Listing lines:
    raw HEX...                 bytes outside any sync'd stream (headers, padding)
    fill 0xNN COUNT            COUNT repeats of byte NN (padding runs)
    sync                       0xAA995566
    noop [COUNT]               type-1 NOOP word(s) 0x20000000
    w1 REG v1 v2 ...           type-1 write to REG with inline words
    w1 REG @blob N             type-1 write, N payload words in a blob
    w2 @blob N                 type-2 write of N words (register = previous type 1)
    hdr1 REG N                 a type-1 header with N words whose payload follows
                               as a type-2 packet (the usual FDRI pattern)
    word HEX                   any other 32-bit word, verbatim
Register writes to CMD are shown with the command name for readability.
"""
import os
import struct
import sys
import tempfile

SYNC = 0xAA995566
NOOP = 0x20000000
REG = {0: 'CRC', 1: 'FAR', 2: 'FDRI', 3: 'FDRO', 4: 'CMD', 5: 'CTL0', 6: 'MASK',
       7: 'STAT', 8: 'LOUT', 9: 'COR0', 0xa: 'MFWR', 0xb: 'CBC', 0xc: 'IDCODE',
       0xd: 'AXSS', 0xe: 'COR1', 0x10: 'WBSTAR', 0x11: 'TIMER', 0x13: 'RBCRC_SW',
       0x16: 'BOOTSTS', 0x18: 'CTL1', 0x1f: 'BSPI'}
REGN = {v: k for k, v in REG.items()}
CMD = {0: 'NULL', 1: 'WCFG', 2: 'MFW', 3: 'LFRM', 4: 'RCFG', 5: 'START', 6: 'RCAP',
       7: 'RCRC', 8: 'AGHIGH', 9: 'SWITCH', 0xa: 'GRESTORE', 0xb: 'SHUTDOWN',
       0xc: 'GCAPTURE', 0xd: 'DESYNC', 0xf: 'IPROG', 0x10: 'CRCC', 0x11: 'LTIMER',
       0x12: 'BSPI_READ', 0x13: 'FALL_EDGE'}
CMDN = {v: k for k, v in CMD.items()}
INLINE_MAX = 8


def regname(r):
    return REG.get(r, 'R%02x' % r)


def regnum(s):
    return REGN[s] if s in REGN else int(s[1:], 16)


def dis(img, out):
    d = open(img, 'rb').read()
    os.makedirs(os.path.join(out, 'blobs'), exist_ok=True)
    L = []
    nblob = [0]

    def blob(data):
        name = 'b%05d' % nblob[0]
        nblob[0] += 1
        open(os.path.join(out, 'blobs', name + '.bin'), 'wb').write(data)
        return name

    def emit_raw(chunk):
        i = 0
        while i < len(chunk):
            j = i
            while j < len(chunk) and chunk[j] == chunk[i]:
                j += 1
            if j - i >= 16:
                L.append('fill 0x%02x %d' % (chunk[i], j - i))
                i = j
            else:
                k = i
                while k < len(chunk) and k - i < 32:
                    run = k
                    while run < len(chunk) and chunk[run] == chunk[k]:
                        run += 1
                    if run - k >= 16:
                        break
                    k += 1
                L.append('raw ' + chunk[i:k].hex())
                i = k

    pos = 0
    while pos < len(d):
        s = d.find(struct.pack('>I', SYNC), pos)
        if s < 0:
            emit_raw(d[pos:])
            break
        emit_raw(d[pos:s])
        L.append('sync')
        i = s + 4
        last = None
        while i + 4 <= len(d):
            w = struct.unpack_from('>I', d, i)[0]
            t = w >> 29
            if w == NOOP:
                n = 0
                while i + 4 <= len(d) and struct.unpack_from('>I', d, i)[0] == NOOP:
                    n += 1
                    i += 4
                L.append('noop' + ('' if n == 1 else ' %d' % n))
                continue
            if t == 1 and (w >> 27) & 3 == 2 and (w >> 11) & 3 == 0:
                reg = (w >> 13) & 0x3fff
                cnt = w & 0x7ff
                i += 4
                last = reg
                if cnt == 0:
                    nxt = struct.unpack_from('>I', d, i)[0] if i + 4 <= len(d) else 0
                    if nxt >> 29 == 2:
                        L.append('hdr1 %s 0' % regname(reg))
                    else:
                        L.append('w1 %s' % regname(reg))
                    continue
                words = struct.unpack_from('>%dI' % cnt, d, i)
                if reg == 4 and cnt == 1 and words[0] in CMD:
                    L.append('w1 CMD %s' % CMD[words[0]])
                elif cnt <= INLINE_MAX and reg not in (2, 0xa):
                    L.append('w1 %s %s' % (regname(reg), ' '.join('%08x' % x for x in words)))
                else:
                    L.append('w1 %s @%s %d' % (regname(reg), blob(d[i:i + 4 * cnt]), cnt))
                i += 4 * cnt
                if reg == 4 and words[0] == 0xd:     # DESYNC ends the stream
                    break
                continue
            if t == 2 and (w >> 27) & 3 == 2:
                cnt = w & 0x7ffffff
                i += 4
                L.append('w2 @%s %d' % (blob(d[i:i + 4 * cnt]), cnt))
                i += 4 * cnt
                continue
            # Anything else: stop interpreting, fall back to raw bytes.
            break
        pos = i
    open(os.path.join(out, 'listing.txt'), 'w').write('\n'.join(L) + '\n')
    return len(L), nblob[0]


def asm(out, img):
    bdir = os.path.join(out, 'blobs')
    o = bytearray()
    for line in open(os.path.join(out, 'listing.txt')):
        f = line.split()
        if not f:
            continue
        op = f[0]
        if op == 'raw':
            o += bytes.fromhex(f[1])
        elif op == 'fill':
            o += bytes([int(f[1], 16)]) * int(f[2])
        elif op == 'sync':
            o += struct.pack('>I', SYNC)
        elif op == 'noop':
            o += struct.pack('>I', NOOP) * (int(f[1]) if len(f) > 1 else 1)
        elif op == 'hdr1':
            o += struct.pack('>I', 0x30000000 | regnum(f[1]) << 13 | int(f[2]))
        elif op == 'w1':
            reg = regnum(f[1])
            if len(f) > 2 and f[2].startswith('@'):
                data = open(os.path.join(bdir, f[2][1:] + '.bin'), 'rb').read()
                assert len(data) == 4 * int(f[3])
                o += struct.pack('>I', 0x30000000 | reg << 13 | len(data) // 4) + data
            elif reg == 4 and len(f) == 3 and f[2] in CMDN:
                o += struct.pack('>II', 0x30000001 | reg << 13, CMDN[f[2]])
            else:
                words = [int(x, 16) for x in f[2:]]
                o += struct.pack('>I', 0x30000000 | reg << 13 | len(words))
                o += struct.pack('>%dI' % len(words), *words)
        elif op == 'w2':
            data = open(os.path.join(bdir, f[1][1:] + '.bin'), 'rb').read()
            assert len(data) == 4 * int(f[2])
            o += struct.pack('>I', 0x50000000 | len(data) // 4) + data
        elif op == 'word':
            o += struct.pack('>I', int(f[1], 16))
        else:
            raise SystemExit('bad listing line: ' + line)
    open(img, 'wb').write(o)
    return len(o)


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    if sys.argv[1] == 'dis':
        print('%d listing lines, %d blobs' % dis(sys.argv[2], sys.argv[3]))
    elif sys.argv[1] == 'asm':
        print('%d bytes' % asm(sys.argv[2], sys.argv[3]))
    elif sys.argv[1] == 'check':
        with tempfile.TemporaryDirectory() as t:
            dis(sys.argv[2], t)
            asm(t, os.path.join(t, 'rebuilt.bin'))
            a = open(sys.argv[2], 'rb').read()
            b = open(os.path.join(t, 'rebuilt.bin'), 'rb').read()
            if a == b:
                print('BYTE-IDENTICAL (%d bytes)' % len(a))
            else:
                n = next((k for k in range(min(len(a), len(b))) if a[k] != b[k]), min(len(a), len(b)))
                raise SystemExit('DIFFER at 0x%x (sizes %d / %d)' % (n, len(a), len(b)))
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main()
