#!/usr/bin/env python3
"""MachXO2/XO3 configuration image <-> packet listing + frames, byte exact.

Generic: no design data in here. Input is a sysCONFIG bitstream as Diamond
writes it, or a raw config-flash readback (16-byte pages; the bitstream runs
from page 0, then the erased tail reads as 0x00).

  xo3bit.py disasm IMAGE OUTDIR [--frames N --frame-bits B]
      -> OUTDIR/listing.txt  one packet per line (see FORMAT below)
         OUTDIR/frames.txt   decompressed frames, one hex line per frame,
                             frame 0 first, bit j of a frame = Trellis CRAM
                             bit (frame, j)
  xo3bit.py asm OUTDIR IMAGE   rebuild the image from listing + frames
  xo3bit.py check IMAGE        disasm -> asm in memory; prints BYTE-IDENTICAL or
                               the first differing offset

FORMAT (listing.txt), one record per line, fields space separated:
  preamble                       FF FF BD B3 (the sync word, 0xFFFF then BDB3)
  dummy N                        N bytes 0xFF (not part of the CRC)
  cmd OP ARGHEX [DATAHEX]        generic command: opcode, 3 argument bytes,
                                 optional payload; CRC accounting implied by
                                 the opcode (below)
  dict ARG P7..P0                LSC_WRITE_COMP_DIC 0x02: 8 pattern bytes,
                                 stored pattern7 first
  frames OP ARG COUNT CRC=xxxx PADBITS=hex  LSC_PROG_INCR_CMP 0xB8 (or
                                 0x82 uncompressed); the data is frames.txt;
                                 CRC is the stored CRC16 after the last frame
                                 (checked against the computed one)
  raw HEX                        bytes not understood as commands (kept verbatim)
  fill BYTE COUNT                COUNT copies of BYTE (flash tail)

CRC16: poly 0x8005, init 0, MSB first, finalised by shifting 16 zero bits
(Trellis BitstreamReadWriter). It runs over every non-dummy byte since the
last LSC_RESET_CRC or checked CRC.
"""
import sys, os, argparse

DUMMY = 0xFF
CMDS = {0x3B: 'LSC_RESET_CRC', 0xE2: 'VERIFY_ID', 0x02: 'LSC_WRITE_COMP_DIC',
        0x22: 'LSC_PROG_CNTRL0', 0x23: 'LSC_PROG_CNTRL1', 0x46: 'LSC_INIT_ADDRESS',
        0xB8: 'LSC_PROG_INCR_CMP', 0x82: 'LSC_PROG_INCR_RTI', 0xA2: 'LSC_PROG_SED_CRC',
        0xCE: 'ISC_PROGRAM_SECURITY', 0xC2: 'ISC_PROGRAM_USERCODE',
        0x5E: 'ISC_PROGRAM_DONE', 0xF6: 'LSC_EBR_ADDRESS', 0xB2: 'LSC_EBR_WRITE'}
# payload length (bytes after the 3 argument bytes) for simple commands
PAYLOAD = {0x3B: 0, 0xE2: 4, 0x22: 4, 0x23: 4, 0x46: 0, 0xA2: 4, 0xCE: 0,
           0xC2: 4, 0x5E: 0, 0xF6: 4}
PREAMBLE = bytes([0xFF, 0xFF, 0xBD, 0xB3])


class CRC:
    def __init__(self):
        self.c = 0

    def feed(self, bs):
        c = self.c
        for v in bs:
            for i in range(7, -1, -1):
                f = c >> 15
                c = ((c << 1) | ((v >> i) & 1)) & 0xFFFF
                if f:
                    c ^= 0x8005
        self.c = c

    def final(self):
        c = self.c
        for _ in range(16):
            f = c >> 15
            c = (c << 1) & 0xFFFF
            if f:
                c ^= 0x8005
        return c


def onehot(b):
    return b.bit_length() - 1 if b and (b & (b - 1)) == 0 else -1


class BitReader:
    def __init__(self, data, pos):
        self.d, self.bp = data, pos * 8

    def bit(self):
        b = (self.d[self.bp >> 3] >> (7 - (self.bp & 7))) & 1
        self.bp += 1
        return b

    def bits(self, n):
        v = 0
        for _ in range(n):
            v = (v << 1) | self.bit()
        return v


def decompress_frame(br, nbytes, dic):
    """Returns (bytes, codes). codes records the encoder's choice per byte so
    a non-canonical encoder (literal where a short code exists) is caught."""
    out, codes = [], []
    for _ in range(nbytes):
        if br.bit() == 0:
            out.append(0); codes.append('z')
        elif br.bit() == 1:
            out.append(br.bits(8)); codes.append('L')
        else:
            idx = br.bits(4)
            out.append((1 << idx) if idx < 8 else dic[idx - 8])
            codes.append('o' if idx < 8 else 'd')
    pad = 0
    padbits = (-br.bp) % 8
    if padbits:
        pad = br.bits(padbits)
    return bytes(out), ''.join(codes), padbits, pad


def compress_frame(fr, dic):
    bits = []
    def add(v, n):
        for i in range(n - 1, -1, -1):
            bits.append((v >> i) & 1)
    for b in fr:
        if b == 0:
            add(0, 1)
        elif onehot(b) >= 0:
            add(0b100, 3); add(onehot(b), 3)
        elif b in dic:
            add(0b101, 3); add(dic.index(b), 3)
        else:
            add(0b11, 2); add(b, 8)
    while len(bits) % 8:
        bits.append(0)
    return bytes(int(''.join(map(str, bits[i:i + 8])), 2) for i in range(0, len(bits), 8))


def disasm(data, nframes=None, frame_bits=None):
    """-> (records, frames). Keeps every byte; raises on anything ambiguous."""
    recs, frames = [], []
    p = 0
    crc = CRC()
    dic = None
    n = len(data)
    # leading dummy bytes before the preamble
    q = data.find(PREAMBLE)
    if q < 0:
        raise ValueError('no preamble')
    if q:
        if set(data[:q]) != {0xFF}:
            raise ValueError('non-0xFF bytes before preamble')
        recs.append(('dummy', q)); p = q
    recs.append(('preamble',)); p += 4
    while p < n:
        op = data[p]
        if op == DUMMY:
            q = p
            while q < n and data[q] == 0xFF:
                q += 1
            # trailing 0xFF run up to the erased (0x00) tail: emitted as dummy
            recs.append(('dummy', q - p)); p = q
            continue
        if op == 0x00 and set(data[p:]) == {0}:
            recs.append(('fill', 0, n - p)); p = n
            break
        if op in PAYLOAD and op not in (0xC2, 0x5E):
            ln = 4 + PAYLOAD[op]
            pkt = data[p:p + ln]
            recs.append(('cmd', op, pkt[1:4], pkt[4:]))
            if op == 0x3B:
                crc = CRC()
            else:
                crc.feed(pkt)
            p += ln
            continue
        if op in (0xC2, 0x5E):  # arg[0] bit 7 = a CRC16 follows
            ln = 4 + PAYLOAD[op]
            pkt = data[p:p + ln]
            crc.feed(pkt)
            p += ln
            if pkt[1] & 0x80:
                calc = crc.final(); stored = int.from_bytes(data[p:p + 2], 'big')
                if calc != stored:
                    raise ValueError('CRC mismatch after %02x at %d' % (op, p))
                p += 2
                crc = CRC()
            if op == 0xC2:  # Trellis: CRC resets after USERCODE either way
                crc = CRC()
            recs.append(('cmd', op, pkt[1:4], pkt[4:]))
            continue
        if op == 0x02:
            pkt = data[p:p + 12]
            if pkt[1] & 0x80:
                raise ValueError('dict with CRC not handled')
            crc.feed(pkt)
            dic = [pkt[11 - i] for i in range(8)]  # dic[i] = pattern i
            recs.append(('dict', pkt[1:4], pkt[4:12]))
            p += 12
            continue
        if op in (0xB8, 0x82):
            arg = data[p + 1:p + 4]
            crc.feed(data[p:p + 4]); p += 4
            cnt = (arg[1] << 8) | arg[2]
            if arg[0] & 0x0F:
                raise ValueError('dummy bytes per frame not handled')
            once = (arg[0] & 0x80) and (arg[0] & 0x40)
            if (arg[0] & 0x80) and not once:
                raise ValueError('per-frame CRC not handled')
            nbytes = frame_bits // 8 if frame_bits else None
            if nbytes is None:
                raise ValueError('need --frame-bits')
            if op == 0xB8:
                nb = nbytes + (7 - ((nbytes - 1) % 8))  # padded to 64 bits
                br = BitReader(data, p)
                padinfo = []
                start = p
                for i in range(cnt):
                    fr, codes, pb, pv = decompress_frame(br, nb, dic)
                    # canonical check: every byte uses the shortest code
                    for b, c in zip(fr, codes):
                        want = 'z' if b == 0 else 'o' if onehot(b) >= 0 else 'd' if b in dic else 'L'
                        if want != c:
                            raise ValueError('non-canonical code in frame %d' % i)
                    if any(fr[:nb - nbytes]):
                        raise ValueError('nonzero frame pad in frame %d' % i)
                    padinfo.append(pv)
                    frames.append(fr[nb - nbytes:])
                p = br.bp >> 3
                crc.feed(data[start:p])
                if any(padinfo):
                    raise ValueError('nonzero byte-alignment pad bits')
            else:
                for i in range(cnt):
                    frames.append(data[p:p + nbytes]); crc.feed(data[p:p + nbytes]); p += nbytes
            stored = None
            if arg[0] & 0x80:
                calc = crc.final(); stored = int.from_bytes(data[p:p + 2], 'big')
                if calc != stored:
                    raise ValueError('frame CRC mismatch: calc %04x stored %04x' % (calc, stored))
                p += 2
            crc = CRC()
            recs.append(('frames', op, arg, cnt, stored))
            continue
        # unknown: take bytes up to the next recognised opcode boundary is not
        # safe; keep a raw run up to the next 0xC2/0x22/0xFF byte
        q = p + 1
        while q < n and data[q] not in (0xC2, 0x22, 0xFF):
            q += 1
        crc.feed(data[p:q])
        recs.append(('raw', data[p:q])); p = q
    return recs, frames


def asm(recs, frames):
    out = bytearray()
    crc = CRC()
    dic = None
    fi = 0
    for r in recs:
        k = r[0]
        if k == 'dummy':
            out += b'\xff' * r[1]
        elif k == 'preamble':
            out += PREAMBLE
        elif k == 'fill':
            out += bytes([r[1]]) * r[2]
        elif k == 'raw':
            out += r[1]; crc.feed(r[1])
        elif k == 'dict':
            pkt = bytes([0x02]) + r[1] + r[2]
            out += pkt; crc.feed(pkt)
            dic = [r[2][7 - i] for i in range(8)]
        elif k == 'cmd':
            op, arg, pay = r[1], r[2], r[3]
            pkt = bytes([op]) + arg + pay
            out += pkt
            if op == 0x3B:
                crc = CRC(); continue
            crc.feed(pkt)
            if op in (0xC2, 0x5E) and arg[0] & 0x80:
                out += crc.final().to_bytes(2, 'big'); crc = CRC()
            if op == 0xC2:
                crc = CRC()
        elif k == 'frames':
            op, arg, cnt = r[1], r[2], r[3]
            hdr = bytes([op]) + arg
            out += hdr; crc.feed(hdr)
            body = bytearray()
            for i in range(cnt):
                fr = frames[fi]; fi += 1
                if op == 0xB8:
                    nb = len(fr) + (7 - ((len(fr) - 1) % 8))
                    body += compress_frame(bytes(nb - len(fr)) + fr, dic)
                else:
                    body += fr
            out += body; crc.feed(body)
            if arg[0] & 0x80:
                c = crc.final()
                if r[4] is not None and c != r[4]:
                    raise ValueError('CRC changed: %04x vs recorded %04x' % (c, r[4]))
                out += c.to_bytes(2, 'big')
            crc = CRC()
    return bytes(out)


def write_listing(recs, path):
    with open(path, 'w') as f:
        f.write('# xo3bit listing (tools/machxo3/xo3bit.py); frames in frames.txt\n')
        for r in recs:
            k = r[0]
            if k == 'dummy':
                f.write('dummy %d\n' % r[1])
            elif k == 'preamble':
                f.write('preamble\n')
            elif k == 'fill':
                f.write('fill %02x %d\n' % (r[1], r[2]))
            elif k == 'raw':
                f.write('raw %s\n' % r[1].hex())
            elif k == 'dict':
                f.write('dict %s %s\n' % (r[1].hex(), r[2].hex()))
            elif k == 'cmd':
                f.write('cmd %02x %s %s  # %s\n' % (r[1], r[2].hex(), r[3].hex() or '-', CMDS.get(r[1], '?')))
            elif k == 'frames':
                f.write('frames %02x %s %d CRC=%s  # %s\n' % (r[1], r[2].hex(), r[3],
                        '%04x' % r[4] if r[4] is not None else '-', CMDS[r[1]]))


def read_listing(path):
    recs = []
    for line in open(path):
        line = line.split('#')[0].split()
        if not line:
            continue
        k = line[0]
        if k == 'dummy':
            recs.append(('dummy', int(line[1])))
        elif k == 'preamble':
            recs.append(('preamble',))
        elif k == 'fill':
            recs.append(('fill', int(line[1], 16), int(line[2])))
        elif k == 'raw':
            recs.append(('raw', bytes.fromhex(line[1])))
        elif k == 'dict':
            recs.append(('dict', bytes.fromhex(line[1]), bytes.fromhex(line[2])))
        elif k == 'cmd':
            recs.append(('cmd', int(line[1], 16), bytes.fromhex(line[2]),
                         b'' if line[3] == '-' else bytes.fromhex(line[3])))
        elif k == 'frames':
            crc = line[4].split('=')[1]
            recs.append(('frames', int(line[1], 16), bytes.fromhex(line[2]), int(line[3]),
                         None if crc == '-' else int(crc, 16)))
        else:
            raise ValueError('bad listing line: %r' % line)
    return recs


def write_frames(frames, path):
    with open(path, 'w') as f:
        for fr in frames:
            f.write(fr.hex() + '\n')


def read_frames(path):
    return [bytes.fromhex(l.strip()) for l in open(path) if l.strip()]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('op', choices=['disasm', 'asm', 'check'])
    ap.add_argument('a'); ap.add_argument('b', nargs='?')
    ap.add_argument('--frame-bits', type=int, default=1272,
                    help='bits per frame (Trellis devices.json; 1272 = LCMXO3-2100)')
    a = ap.parse_args()
    if a.op in ('disasm', 'check'):
        data = open(a.a, 'rb').read()
        recs, frames = disasm(data, frame_bits=a.frame_bits)
        if a.op == 'disasm':
            os.makedirs(a.b, exist_ok=True)
            write_listing(recs, os.path.join(a.b, 'listing.txt'))
            write_frames(frames, os.path.join(a.b, 'frames.txt'))
            print('%d records, %d frames' % (len(recs), len(frames)))
        else:
            out = asm(recs, frames)
            if out == data:
                print('BYTE-IDENTICAL (%d bytes)' % len(data))
            else:
                d = next((i for i in range(min(len(out), len(data))) if out[i] != data[i]), min(len(out), len(data)))
                print('DIFFER at offset %d (len %d vs %d)' % (d, len(out), len(data))); sys.exit(1)
    else:
        recs = read_listing(os.path.join(a.a, 'listing.txt'))
        frames = read_frames(os.path.join(a.a, 'frames.txt'))
        open(a.b, 'wb').write(asm(recs, frames))


if __name__ == '__main__':
    main()
