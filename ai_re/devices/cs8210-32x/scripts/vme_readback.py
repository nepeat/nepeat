#!/usr/bin/env python3
"""Read-only JTAG readback of a MachXO3LF CPLD through the vendor 'vme' player.

gen   : write a read-only VME 12.1 file (own encoder, uncompressed) that reads
        status, USERCODE, TraceID, FEABITS, feature row and every CFG (and
        optionally UFM) page in transparent mode (ISC_ENABLE_X, user logic keeps
        running), then ISC_DISABLE.  No erase/program/refresh opcode is emitted;
        the file is decoded again with vme_decode.py and every SIR is checked
        against an allow-list before it is written.
parse : parse the player's 'Display Data: 0x..' lines back into pages and diff
        them against a page dump from vme_decode.py --dump-dir (the old image).

usage:
  vme_readback.py gen {sys,swled,macled} [--ufm] -o OUT.vme
  vme_readback.py parse {sys,swled,macled} PLAYER_OUTPUT.txt --ref OLD/cfg.bin [--ufm] [--out DIR]
No hardware access here; running the generated file is a separate, risky step
(see ../cpld-diff.md).
"""
import argparse, hashlib, importlib.util, os, re, sys

TARGETS = {  # IDCODE, CFG pages, UFM pages, chain amble (HIR bits, HDR bits) as in the vendor files
    'sys':    dict(idcode=0x612BD043, cfg=9212, ufm=2046, hir=0, hdr=0),
    'swled':  dict(idcode=0xE12BC043, cfg=5758, ufm=768, hir=0, hdr=0),
    'macled': dict(idcode=0xE12BC043, cfg=5758, ufm=768, hir=8, hdr=1),
}
ALLOWED_IR = {0xE0, 0x3C, 0xC0, 0x19, 0x74, 0xFB, 0xE7, 0x46, 0x47, 0x73, 0x26, 0xFF}


def size(v):
    out = bytearray()
    while True:
        b = v & 0x7F
        v >>= 7
        if v:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def vbytes(v, n):
    """SVF-convention integer -> VME byte stream (first shifted bit = MSB of byte 0)."""
    out = bytearray((n + 7) // 8)
    for i in range(n):
        if (v >> i) & 1:
            out[i // 8] |= 0x80 >> (i % 8)
    return bytes(out)


class Enc:
    def __init__(self):
        self.b = bytearray(b'____12.1')
        self.b += b'\xf2'                         # uncompressed
        self.b += b'\x56\x01'                     # VENDOR LATTICE
        self.b += b'\x0b' + size(1000)            # FREQUENCY, as the vendor files
        self.b += b'\x19' + size(128)             # MEM
        self.sirs = []

    def amble(self, hir, hdr):
        for op, n in ((0x08, hdr), (0x06, hir), (0x09, 0), (0x07, 0)):  # HDR HIR TDR TIR
            self.b += bytes([op]) + size(n)
            if n:  # bypass ones for the leading device, as the vendor MACLED file
                self.b += b'\x13' + vbytes((1 << n) - 1, n) + b'\x70'
        self.b += b'\x02\x03'                     # ENDDR DRPAUSE
        self.b += b'\x03\x02'                     # ENDIR IRPAUSE
        self.state('IDLE')

    def state(self, s):
        self.b += b'\x10' + bytes([{'IDLE': 1, 'RESET': 0}[s]])

    def sir(self, ir):
        assert ir in ALLOWED_IR, hex(ir)
        self.sirs.append(ir)
        self.b += b'\x11' + size(8) + b'\x13' + vbytes(ir, 8) + b'\x70'

    def sdr(self, n, tdi=0, tdo=None, mask=None):
        self.b += b'\x12' + size(n) + b'\x13' + vbytes(tdi, n)
        if tdo is not None:
            self.b += b'\x14' + vbytes(tdo, n)
        if mask is not None:
            self.b += b'\x15' + vbytes(mask, n)
        self.b += b'\x70'

    def idle_wait(self, ms, tck=2):
        self.state('IDLE')
        self.b += b'\x1b' + size(tck)
        self.b += b'\x1a' + size(0x8000 | ms)

    def display(self, ir, n, ms=1):
        self.sir(ir)
        self.idle_wait(ms)
        self.sdr(n, 0, 0, 0)                     # MASK all zero -> the player prints the data

    def end(self):
        self.b += b'\x7f'
        return bytes(self.b)


def gen(target, ufm):
    t = TARGETS[target]
    e = Enc()
    e.amble(t['hir'], t['hdr'])
    e.sir(0xE0)                                   # IDCODE check: the player aborts on mismatch
    e.sdr(32, 0, t['idcode'], 0xFFFFFFFF)
    e.display(0x3C, 32)                           # status before enable
    e.display(0xC0, 32)                           # USERCODE
    e.display(0x19, 64)                           # TraceID (UIDCODE_PUB)
    e.sir(0x74); e.sdr(8, 0x08); e.idle_wait(1)   # ISC_ENABLE_X 0x08, as the vendor files
    e.display(0x3C, 32)
    e.display(0xFB, 16)                           # FEABITS
    e.display(0xE7, 64)                           # feature row
    e.sir(0x46); e.sdr(8, 0x04); e.idle_wait(1)   # LSC_INIT_ADDRESS (CFG), as the vendor files
    e.sir(0x73); e.idle_wait(1)
    for _ in range(t['cfg']):
        e.sdr(128, 0, 0, 0); e.idle_wait(1)
    if ufm:
        e.sir(0x47); e.idle_wait(1)
        e.sir(0x73); e.idle_wait(1)
        for _ in range(t['ufm']):
            e.sdr(128, 0, 0, 0); e.idle_wait(1)
    e.display(0x3C, 32)
    e.sir(0xFF); e.sir(0x26); e.idle_wait(1000)   # BYPASS, ISC_DISABLE (no PROGRAM_DONE, no REFRESH)
    e.sir(0xFF); e.idle_wait(100, tck=100)
    return e.end(), e.sirs


def load_decoder():
    p = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'vme_decode.py')
    spec = importlib.util.spec_from_file_location('vme_decode', p)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def selfcheck(blob, target, ufm):
    vd = load_decoder()
    d = vd.Decoder(blob)
    d.fp = 8
    end = d.code()
    assert end == 0x7F and d.fp == len(blob), 'decoder did not consume the whole file'
    irs = [ev['TDI'] for ev in d.events if ev['k'] == 'SIR']
    bad = sorted({i for i in irs if i not in ALLOWED_IR})
    assert not bad, 'forbidden IR %s' % bad
    reads = sum(1 for ev in d.events if ev['k'] == 'SDR' and ev.get('MASK') == 0 and 'TDO' in ev)
    t = TARGETS[target]
    want = 7 + t['cfg'] + (t['ufm'] if ufm else 0)   # 6 header reads + pages + final status
    assert reads == want, (reads, want)
    return {'sir_count': len(irs), 'distinct_ir': ['%02X' % i for i in sorted(set(irs))], 'display_reads': reads}


def parse(target, path, ref, ufm, outdir):
    t = TARGETS[target]
    vals = [m.group(1) for m in re.finditer(r'Display Data: 0x([0-9A-Fa-f]+)', open(path, errors='replace').read())]
    n_cfg, n_ufm = t['cfg'], (t['ufm'] if ufm else 0)
    want = 7 + n_cfg + n_ufm
    if len(vals) != want:
        print('WARNING: %d display lines, expected %d (player aborted early?)' % (len(vals), want))
    head = dict(zip(['status0', 'usercode', 'traceid', 'status1', 'feabits', 'feature'], vals[:6]))
    pages = [int(v, 16).to_bytes(16, 'little') for v in vals[6:6 + n_cfg]]
    upages = [int(v, 16).to_bytes(16, 'little') for v in vals[6 + n_cfg:6 + n_cfg + n_ufm]]
    tail = vals[6 + n_cfg + n_ufm:]
    old = open(ref, 'rb').read() if ref else b''
    oldp = [old[i:i + 16] for i in range(0, len(old), 16)]
    diff = [i for i in range(min(len(pages), len(oldp))) if pages[i] != oldp[i]]
    ranges, s = [], None
    for i in diff:
        if s is None or i != ranges[-1][1] + 1:
            ranges.append([i, i])
        else:
            ranges[-1][1] = i
        s = i
    blob = b''.join(pages)
    print('header:', head, 'final status:', tail[:1])
    print('cfg pages read %d, nonzero %d, sha256 %s' % (len(pages), sum(1 for p in pages if any(p)), hashlib.sha256(blob).hexdigest()))
    if not any(blob):
        print('ALL CFG PAGES ZERO: readback blocked (security bit?) or chain/IR problem')
    print('pages differing from reference: %d in %d runs' % (len(diff), len(ranges)))
    for a, b in ranges[:60]:
        print('  pages %d..%d' % (a, b))
    if outdir:
        os.makedirs(outdir, exist_ok=True)
        open(os.path.join(outdir, 'cfg.bin'), 'wb').write(blob)
        if upages:
            open(os.path.join(outdir, 'ufm.bin'), 'wb').write(b''.join(upages))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    g = sub.add_parser('gen'); g.add_argument('target', choices=TARGETS); g.add_argument('--ufm', action='store_true'); g.add_argument('-o', required=True)
    p = sub.add_parser('parse'); p.add_argument('target', choices=TARGETS); p.add_argument('output'); p.add_argument('--ref'); p.add_argument('--ufm', action='store_true'); p.add_argument('--out')
    a = ap.parse_args()
    if a.cmd == 'gen':
        blob, _ = gen(a.target, a.ufm)
        info = selfcheck(blob, a.target, a.ufm)
        open(a.o, 'wb').write(blob)
        print('%s: %d bytes, sha256 %s, %s' % (a.o, len(blob), hashlib.sha256(blob).hexdigest(), info))
    else:
        parse(a.target, a.output, a.ref, a.ufm, a.out)


if __name__ == '__main__':
    main()
