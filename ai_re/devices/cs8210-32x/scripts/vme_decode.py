#!/usr/bin/env python3
"""Decode a Lattice ispVM VME byte-code file (Diamond Deployment Tool, VME 12.x)
into a JTAG scan transcript and a MachXO2/MachXO3 programming summary.

Own implementation, written from the public ispVME opcode semantics. Static:
no hardware is touched. LCOUNT (intelligent-programming retry) bodies are
executed once, assuming the first pass succeeds; REPEAT loops are unrolled.

usage: vme_decode.py FILE.vme [--svf OUT.txt] [--json OUT.json] [--dump-dir DIR]
  --svf      write an SVF-like transcript (vendor-derived: keep it git-ignored)
  --json     write the summary as JSON
  --dump-dir write cfg.bin / ufm.bin (raw 16-byte pages in shift order) there
"""
import argparse, hashlib, json, os, sys, zlib

# ---- VME opcodes -----------------------------------------------------------
ENDDATA, RUNTEST, ENDDR, ENDIR, ENDSTATE, TRST, HIR, TIR, HDR, TDR, ISPEN, FREQUENCY = range(0x00, 0x0c)
STATE, SIR, SDR, TDI, TDO, MASK, XSDR, XTDI, XTDO, MEM, WAIT, TCK = range(0x10, 0x1c)
SHR, SHL, HEAP, REPEAT, LEFTPAREN, VAR = 0x23, 0x24, 0x32, 0x33, 0x35, 0x55
SETFLOW, RESETFLOW, CRC, CMASK, RMASK, READ, ENDLOOP = 0x30, 0x31, 0x47, 0x48, 0x49, 0x50, 0x59
SECUREHEAP, VUES, DMASK, COMMENT, HEADER, FILE_CRC = 0x60, 0x61, 0x62, 0x63, 0x64, 0x65
LCOUNT, LDELAY, LSDR, LHEAP, CONTINUE, LVDS, ENDVME = 0x66, 0x67, 0x68, 0x69, 0x70, 0x71, 0x7f
VENDOR = 0x56
STATES = {0: 'RESET', 1: 'IDLE', 2: 'IRPAUSE', 3: 'DRPAUSE', 4: 'SHIFTIR', 5: 'SHIFTDR', 6: 'DRCAPTURE'}
VENDORS = {1: 'LATTICE', 2: 'ALTERA', 3: 'XILINX'}

# ---- MachXO2/XO3 JTAG instructions (TN1204 / TN1279 names) -----------------
XO3 = {
    0xE0: 'IDCODE_PUB', 0xC0: 'USERCODE', 0xC2: 'ISC_PROGRAM_USERCODE', 0x19: 'UIDCODE_PUB',
    0xC6: 'ISC_ENABLE', 0x74: 'ISC_ENABLE_X', 0x0E: 'ISC_ERASE', 0x26: 'ISC_DISABLE',
    0x46: 'LSC_INIT_ADDRESS', 0x47: 'LSC_INIT_ADDR_UFM', 0xB4: 'LSC_WRITE_ADDRESS',
    0x70: 'LSC_PROG_INCR_NV', 0x73: 'LSC_READ_INCR_NV', 0x3C: 'LSC_READ_STATUS', 0xF0: 'LSC_CHECK_BUSY',
    0xE4: 'LSC_PROG_FEATURE', 0xE7: 'LSC_READ_FEATURE', 0xF8: 'LSC_PROG_FEABITS', 0xFB: 'LSC_READ_FEABITS',
    0x5E: 'ISC_PROGRAM_DONE', 0x79: 'LSC_REFRESH', 0xCE: 'ISC_PROGRAM_SECURITY', 0xCF: 'ISC_PROGRAM_SECPLUS',
    0xC9: 'LSC_PROG_TAG', 0xCA: 'LSC_READ_TAG', 0xCB: 'LSC_ERASE_TAG', 0x1C: 'PRELOAD_SAMPLE',
    0x7D: 'LSC_DEVICE_CONTROL', 0xFF: 'BYPASS', 0x3A: 'LSC_SPI_BRIDGE?', 0xF9: 'LSC_PROG_OTP',
    0xFA: 'LSC_READ_OTP', 0xBC: 'LSC_SHIFT_PASSWORD', 0xF1: 'LSC_PROG_PASSWORD', 0xF2: 'LSC_READ_PASSWORD',
    0x4E: 'LSC_SECURE_?', 0x0F: 'BSCAN_EXTEST?', 0x38: 'LSC_ENABLE_PASSWORD?',
}
PARTS = {0x612BD043: 'LCMXO3LF-6900C', 0xE12BC043: 'LCMXO3LF-2100C', 0xE12BB043: 'LCMXO3LF-1300C',
         0x612BC043: 'LCMXO3LF-2100C (alt)', 0x612BB043: 'LCMXO3LF-1300C (alt)', 0x612BE043: 'LCMXO3LF-9400C'}


def vme_bits(data, nbits):
    """VME byte order -> integer in SVF convention (bit 0 = first bit shifted)."""
    v = 0
    for i in range(nbits):
        if (data[i // 8] << (i % 8)) & 0x80:
            v |= 1 << i
    return v


def svf_hex(v, nbits):
    return '%0*X' % ((nbits + 3) // 4, v)


class VMEError(Exception):
    pass


class Decoder:
    def __init__(self, blob):
        self.f = blob
        self.fp = 0
        self.heap = b''
        self.hp = 0
        self.ibuf = b''
        self.ip = 0
        self.heap_in = False
        self.lheap_in = False
        self.compress = False
        self.events = []          # (kind, ...) transcript
        self.comments = []
        self.vendor = None
        self.maxsize = 0
        self.flow = 0
        self.repeat_loops = 0
        self.shift = None         # ('SHR'|'SHL', bits)
        self.indata = bytearray(8192)
        self.outdata = bytearray(8192)
        self.hir = self.tir = self.hdr = self.tdr = 0
        self.lcount_blocks = 0
        self.repeat_blocks = []
        self.freq = None

    # byte sources, same priority as the reference engine: heap, then intel buffer, then file
    def gb(self):
        if self.heap_in:
            if self.hp >= len(self.heap):
                return 0xFF
            b = self.heap[self.hp]; self.hp += 1; return b
        if self.lheap_in:
            if self.ip >= len(self.ibuf):
                return 0xFF
            b = self.ibuf[self.ip]; self.ip += 1; return b
        if self.fp >= len(self.f):
            return 0xFF
        b = self.f[self.fp]; self.fp += 1; return b

    def size(self):
        v, sh = 0, 0
        while True:
            b = self.gb()
            v |= (b & 0x7F) << sh
            sh += 7
            if not b & 0x80:
                return v

    def data(self, nbits):
        n = (nbits + 7) // 8
        out = bytearray(n)
        get_data = False
        compression = False
        key = 0xFF
        if self.compress:
            compression = True
            c = self.gb()
            if c == VAR and self.heap_in:
                get_data = True
                self.heap_in = False
                c = self.gb()
            if c == 0x00:
                compression = False
            elif c == 0x01:
                key = 0x00
            elif c == 0x02:
                key = 0xFF
            elif c == 0xFF:
                key = self.gb()
                i, cur = 8, 0
                for idx in range(n):
                    if i > 7:
                        cur = self.gb(); i = 0
                    bit = (cur << i) & 0x80; i += 1
                    if bit:
                        val = 0
                        for j in range(8):
                            if i > 7:
                                cur = self.gb(); i = 0
                            val |= (((cur << i) & 0x80) >> j); i += 1
                        out[idx] = val & 0xFF
                    else:
                        out[idx] = key
                n = 0
            else:  # nibble-block repeat
                for idx in range(c):
                    if idx % 2 == 0:
                        cur = self.gb()
                    for k in range(len(out) * 2 // c):
                        j = idx + k * c
                        if j // 2 >= len(out):
                            continue
                        if j % 2:
                            out[j // 2] |= (cur & 0x0F) if idx % 2 else (cur >> 4)
                        else:
                            out[j // 2] |= ((cur << 4) & 0xF0) if idx % 2 else (cur & 0xF0)
                n = 0
        run = 0
        for idx in range(n):
            if run <= 0:
                b = self.gb()
                if b == VAR and self.heap_in and not get_data and not self.compress:
                    get_data = True
                    self.heap_in = False
                    b = self.gb()
                out[idx] = b
                if compression and b == key:
                    run = self.size()
            else:
                run -= 1
                out[idx] = key
        if get_data:
            self.heap_in = True
        return bytes(out)

    def bitshift(self, mode, bits, nbits):
        size = (nbits + 7) // 8
        d = self.indata
        i = 0
        while i < size:
            if d[i]:
                t = bits
                while t > 0:
                    if mode == 'SHR':
                        d[i] = (d[i] << 1) & 0xFF
                        if d[i] == 0 and i > 0:
                            i -= 1; d[i] = 1
                    else:
                        d[i] >>= 1
                        if d[i] == 0 and i > 0:
                            i -= 1; d[i] = 8
                    t -= 1
            i += 1

    def datacode(self, nbits):
        from_heap = self.heap_in
        got = {}
        while True:
            b = self.gb()
            if b >= 0x80:
                raise VMEError('bad data opcode 0x%02x at file 0x%x' % (b, self.fp))
            if b == TDI:
                d = self.data(nbits)
                self.indata[:len(d)] = d
                if self.shift:
                    self.bitshift(self.shift[0], self.shift[1], nbits)
                    self.shift = None
                got['TDI'] = bytes(self.indata[:(nbits + 7) // 8])
            elif b == XTDO:
                got['TDO'] = bytes(self.outdata[:(nbits + 7) // 8]); got['XTDO'] = True
            elif b == TDO:
                got['TDO'] = self.data(nbits)
            elif b == MASK:
                got['MASK'] = self.data(nbits)
            elif b == DMASK:
                got['DMASK'] = self.data(nbits)
            elif b == CMASK:
                got['CMASK'] = self.data(nbits)
            elif b == CONTINUE:
                return got
            else:
                raise VMEError('unexpected 0x%02x in scan data at file 0x%x' % (b, self.fp))
            if from_heap:
                self.heap_in = True

    def shiftop(self, op):
        nbits = self.size()
        got = self.datacode(nbits)
        kind = 'SIR' if op == SIR else 'SDR'
        ev = {'k': kind, 'n': nbits, 'lc': self.lheap_in, 'rep': self.repeat_loops if self.heap_in else None}
        for key in ('TDI', 'TDO', 'MASK', 'DMASK', 'CMASK'):
            if key in got:
                ev[key] = vme_bits(got[key], nbits)
        if 'XTDO' in got:
            ev['xtdo'] = True
        if op == XSDR:
            ev['xsdr'] = True
        self.events.append(ev)
        if op in (SDR, XSDR) and 'TDI' in got:
            self.outdata[:len(got['TDI'])] = got['TDI']

    def amble(self, op):
        nbits = self.size()
        if nbits:
            self.gb()
            was = self.compress
            self.compress = False
            self.data(nbits)
            self.compress = was
        name = {HIR: 'HIR', TIR: 'TIR', HDR: 'HDR', TDR: 'TDR'}[op]
        setattr(self, name.lower(), nbits)
        self.events.append({'k': name, 'n': nbits})
        if nbits and self.gb() != CONTINUE:
            raise VMEError('amble without CONTINUE at 0x%x' % self.fp)

    def lcount(self, count):
        bsize = self.size()
        self.ibuf = bytes(self.gb() for _ in range(bsize))
        self.lcount_blocks += 1
        rep_heap = self.heap_in
        self.heap_in = False
        self.lheap_in = True
        self.ip = 0
        self.events.append({'k': 'LCOUNT', 'n': count})
        while True:
            op = self.gb()
            if op in (HIR, TIR, HDR, TDR):
                self.amble(op)
            elif op == STATE:
                self.events.append({'k': 'STATE', 's': STATES.get(self.gb(), '?')})
            elif op in (SIR, SDR):
                self.shiftop(op)
            elif op == WAIT:
                self.events.append({'k': 'WAIT', 'v': self.size()})
            elif op == TCK:
                self.events.append({'k': 'TCK', 'v': self.size()})
            elif op == ENDLOOP:
                break
            elif op == COMMENT:
                self.comment()
            elif op in (ISPEN, TRST):
                self.gb()
            else:
                raise VMEError('bad LCOUNT opcode 0x%02x' % op)
        self.lheap_in = False
        if rep_heap:
            self.heap_in = True
        self.events.append({'k': 'ENDLCOUNT'})

    def comment(self):
        n = self.size()
        s = bytes(self.gb() for _ in range(n)).decode('latin-1')
        self.comments.append(s)
        self.events.append({'k': 'COMMENT', 's': s})

    def loop(self, count, heapsize):
        body = bytes(self.gb() for _ in range(heapsize))
        if not body or body[-1] != ENDLOOP:
            raise VMEError('REPEAT body does not end in ENDLOOP at 0x%x' % self.fp)
        self.repeat_blocks.append(count)
        self.events.append({'k': 'REPEAT', 'n': count})
        saved = (self.heap, self.hp, self.heap_in)
        self.heap = body
        self.heap_in = True
        for _ in range(count):
            self.hp = 0
            self.code(nested=True)
            self.repeat_loops += 1
        self.heap, self.hp, self.heap_in = saved
        self.heap_in = False
        self.events.append({'k': 'ENDREPEAT'})

    def code(self, nested=False):
        if not nested and not self.heap_in and not self.lheap_in:
            c = self.gb()
            if c == 0xF1:
                self.compress = True
            elif c == 0xF2:
                self.compress = False
            else:
                raise VMEError('bad compression byte 0x%02x' % c)
        heapsize = 0
        while True:
            op = self.gb()
            if op >= 0x80:
                raise VMEError('opcode 0x%02x >= 0x80 at file 0x%x' % (op, self.fp))
            if op == STATE:
                self.events.append({'k': 'STATE', 's': STATES.get(self.gb(), '?')})
            elif op in (SIR, SDR, XSDR):
                self.shiftop(op)
            elif op == WAIT:
                self.events.append({'k': 'WAIT', 'v': self.size()})
            elif op == TCK:
                self.events.append({'k': 'TCK', 'v': self.size()})
            elif op == ENDDR:
                self.events.append({'k': 'ENDDR', 's': STATES.get(self.gb(), '?')})
            elif op == ENDIR:
                self.events.append({'k': 'ENDIR', 's': STATES.get(self.gb(), '?')})
            elif op in (HIR, TIR, HDR, TDR):
                self.amble(op)
            elif op == MEM:
                self.maxsize = self.size()
            elif op == VENDOR:
                self.vendor = VENDORS.get(self.gb(), '?')
            elif op == SETFLOW:
                self.flow |= self.size()
            elif op == RESETFLOW:
                self.flow &= ~self.size()
            elif op == HEAP:
                if self.gb() != SECUREHEAP:
                    raise VMEError('HEAP without SECUREHEAP')
                heapsize = self.size()
                self._heapsize = heapsize
            elif op == REPEAT:
                self.repeat_loops = 0
                self.loop(self.size(), self._heapsize)
            elif op in (ENDLOOP, ENDVME):
                return op
            elif op in (SHR, SHL):
                self.shift = ('SHR' if op == SHR else 'SHL', self.repeat_loops * self.gb())
            elif op == FREQUENCY:
                self.freq = self.size()
            elif op == LCOUNT:
                self.lcount(self.size())
            elif op == VUES:
                self.events.append({'k': 'VUES'})
            elif op == COMMENT:
                self.comment()
            elif op == LVDS:
                n = self.size()
                for _ in range(2 * n):
                    self.size()
            elif op == HEADER:
                n = self.size()
                for _ in range(n):
                    self.gb()
            elif op in (ISPEN, TRST):
                self.events.append({'k': 'PIN', 'op': op, 'v': self.gb()})
            else:
                raise VMEError('invalid opcode 0x%02x at file 0x%x' % (op, self.fp))


def crc16_vme(data):
    """The engine's file CRC (nibble table, input bytes bit-reversed)."""
    t = [0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401,
         0xA001, 0x6C00, 0x7800, 0xB401, 0x5000, 0x9C01, 0x8801, 0x4400]
    c = 0
    for b in data:
        f = int('{:08b}'.format(b)[::-1], 2)
        e = t[c & 0xF]; c = ((c >> 4) & 0x0FFF) ^ e ^ t[f & 0xF]
        e = t[c & 0xF]; c = ((c >> 4) & 0x0FFF) ^ e ^ t[(f >> 4) & 0xF]
    return c


def decode(path):
    blob = open(path, 'rb').read()
    d = Decoder(blob)
    hdr = {}
    first = d.gb()
    crc_start = None
    if first == FILE_CRC:
        hdr['file_crc'] = '%04X' % ((d.f[1] << 8) | d.f[2])
        d.fp = 3
        crc_start = 3
    else:
        d.fp = 0
    hdr['version'] = bytes(d.gb() for _ in range(8)).decode('latin-1')
    end = d.code()
    hdr['end'] = 'ENDVME' if end == ENDVME else 'ENDLOOP?'
    hdr['bytes_used'] = d.fp
    hdr['bytes_total'] = len(blob)
    if crc_start is not None:
        hdr['crc_calc'] = '%04X' % crc16_vme(blob[crc_start:d.fp])
        hdr['crc_ok'] = hdr['crc_calc'] == hdr['file_crc']
    hdr['compressed'] = d.compress
    hdr['vendor'] = d.vendor
    hdr['freq_hz'] = d.freq
    hdr['sha256'] = hashlib.sha256(blob).hexdigest()
    hdr['repeat_blocks'] = d.repeat_blocks
    hdr['lcount_blocks'] = d.lcount_blocks
    return hdr, d


def analyse(d):
    """Walk the scan transcript as a MachXO2/XO3 programming flow."""
    ir = None
    s = {'instr_seq': [], 'idcode_checks': [], 'usercode_prog': [], 'usercode_verify': [],
         'feature_prog': [], 'feabits_prog': [], 'feature_verify': [], 'feabits_verify': [],
         'erase_ops': [], 'cfg_pages': 0, 'ufm_pages': 0, 'reads_verify': 0, 'status_checks': [],
         'uidcode': [], 'enable_mode': [], 'refresh': 0, 'security': [], 'done': 0, 'unknown_ir': {},
         'write_address': []}
    region = None
    cfg, ufm = [], []
    for ev in d.events:
        k = ev['k']
        if k == 'SIR':
            ir = ev.get('TDI')
            name = XO3.get(ir, '0x%02X' % ir if ir is not None else '?')
            if not s['instr_seq'] or s['instr_seq'][-1][0] != name:
                s['instr_seq'].append([name, 1])
            else:
                s['instr_seq'][-1][1] += 1
            if ir not in XO3 and ir is not None:
                s['unknown_ir']['%02X' % ir] = s['unknown_ir'].get('%02X' % ir, 0) + 1
            if ir == 0x46:
                region = 'cfg'
            elif ir == 0x47:
                region = 'ufm'
            elif ir == 0x79:
                s['refresh'] += 1
            elif ir == 0x5E:
                s['done'] += 1
            elif ir in (0xCE, 0xCF):
                s['security'].append(XO3[ir])
            continue
        if k != 'SDR' or ir is None:
            continue
        n = ev['n']
        tdi, tdo, mask = ev.get('TDI'), ev.get('TDO'), ev.get('MASK')
        if ir == 0xE0 and tdo is not None:
            s['idcode_checks'].append('%08X' % tdo)
        elif ir in (0xC6, 0x74):
            s['enable_mode'].append('%s 0x%02X' % (XO3[ir], tdi or 0))
        elif ir == 0x0E:
            s['erase_ops'].append('0x%0*X' % ((n + 3) // 4, tdi or 0))
        elif ir == 0xC0 and tdo is None and tdi is not None:
            s['usercode_prog'].append('%08X' % tdi)   # value loaded before ISC_PROGRAM_USERCODE
        elif ir == 0xC0 and tdo is not None:
            s['usercode_verify'].append('%08X' % tdo)
        elif ir == 0xE4 and tdi is not None:
            s['feature_prog'].append(svf_hex(tdi, n))
        elif ir == 0xE7 and tdo is not None:
            s['feature_verify'].append({'tdo': svf_hex(tdo, n), 'mask': svf_hex(mask, n) if mask is not None else None})
        elif ir == 0xF8 and tdi is not None:
            s['feabits_prog'].append(svf_hex(tdi, n))
        elif ir == 0xFB and tdo is not None:
            s['feabits_verify'].append({'tdo': svf_hex(tdo, n), 'mask': svf_hex(mask, n) if mask is not None else None})
        elif ir == 0x70 and tdi is not None:
            (ufm if region == 'ufm' else cfg).append(tdi.to_bytes((n + 7) // 8, 'little'))
        elif ir == 0x73 and tdo is not None:
            s['reads_verify'] += 1
        elif ir == 0x3C and tdo is not None:
            s['status_checks'].append({'tdo': svf_hex(tdo, n), 'mask': svf_hex(mask, n) if mask is not None else None})
        elif ir == 0x19:
            s['uidcode'].append(svf_hex(tdo or 0, n))
        elif ir == 0xB4 and tdi is not None:
            s['write_address'].append(svf_hex(tdi, n))
            region = 'ufm' if (tdi >> 30) & 1 else region
    s['instr_seq'] = collapse([n for n, c in s['instr_seq'] for _ in range(c)])
    s['cfg_pages'] = len(cfg)
    s['ufm_pages'] = len(ufm)
    # status checks: keep distinct
    seen, uniq = set(), []
    for c in s['status_checks']:
        key = (c['tdo'], c['mask'])
        if key not in seen:
            seen.add(key); uniq.append(c)
    s['status_checks'] = uniq
    return s, cfg, ufm


def collapse(tokens):
    """Run-length encode a token list, folding repeated groups of up to 4 tokens."""
    out, i = [], 0
    while i < len(tokens):
        best = (1, 1)
        for w in range(1, 5):
            grp = tokens[i:i + w]
            if len(grp) < w:
                break
            r = 1
            while tokens[i + r * w:i + (r + 1) * w] == grp:
                r += 1
            if r > 1 and r * w > best[0] * best[1]:
                best = (w, r)
        w, r = best
        grp = tokens[i:i + w]
        out.append(('(%s) x%d' % (' '.join(grp), r)) if r > 1 else ' '.join(grp))
        i += w * r
    return out


def page_stats(pages):
    blob = b''.join(pages)
    nonzero = sum(1 for p in pages if any(p))
    return {'pages': len(pages), 'bytes': len(blob), 'sha256': hashlib.sha256(blob).hexdigest() if blob else None,
            'crc32': '%08X' % (zlib.crc32(blob) & 0xFFFFFFFF), 'pages_nonzero': nonzero}


def feabits_decode(v):
    """MachXO2/XO3 FEABITS (16 bit, as shifted: SVF value)."""
    return {
        'boot_sequence': (v >> 12) & 0x3, 'master_spi_port': bool((v >> 11) & 1),
        'i2c_port_disabled': bool((v >> 10) & 1), 'slave_spi_port_disabled': bool((v >> 9) & 1),
        'jtag_port_disabled': bool((v >> 8) & 1), 'done_persistent': bool((v >> 7) & 1),
        'initn_persistent': bool((v >> 6) & 1), 'programn_disabled': bool((v >> 5) & 1),
        'my_assp': bool((v >> 4) & 1), 'password_protect_all': bool((v >> 3) & 1),
        'password_protect': bool((v >> 2) & 1), 'decrypt_only': bool((v >> 1) & 1),
    }


def transcript(d):
    out = []
    for ev in d.events:
        k = ev['k']
        if k == 'SIR':
            ir = ev.get('TDI', 0)
            out.append('SIR %d TDI (%s);  // %s' % (ev['n'], svf_hex(ir, ev['n']), XO3.get(ir, '?')))
        elif k == 'SDR':
            parts = ['SDR %d' % ev['n']]
            for key in ('TDI', 'TDO', 'MASK', 'DMASK', 'CMASK'):
                if key in ev:
                    parts.append('%s (%s)' % (key, svf_hex(ev[key], ev['n'])))
            out.append(' '.join(parts) + ';' + ('  // LCOUNT body' if ev.get('lc') else ''))
        elif k == 'WAIT':
            v = ev['v']
            out.append('RUNTEST %s;' % (('%d ms' % (v & 0x7FFF)) if v & 0x8000 else ('%d us' % v)))
        elif k == 'TCK':
            out.append('RUNTEST %d TCK;' % ev['v'])
        elif k == 'COMMENT':
            out.append('// ' + ev['s'].replace('\n', '\n// '))
        elif k in ('STATE', 'ENDDR', 'ENDIR'):
            out.append('%s %s;' % (k, ev['s']))
        elif k in ('HIR', 'TIR', 'HDR', 'TDR'):
            out.append('%s %d;' % (k, ev['n']))
        elif k in ('REPEAT', 'LCOUNT'):
            out.append('// %s %d {' % (k, ev['n']))
        elif k in ('ENDREPEAT', 'ENDLCOUNT'):
            out.append('// }')
        else:
            out.append('// ' + json.dumps(ev))
    return '\n'.join(out) + '\n'


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('vme')
    ap.add_argument('--svf')
    ap.add_argument('--json')
    ap.add_argument('--dump-dir')
    a = ap.parse_args()
    hdr, d = decode(a.vme)
    s, cfg, ufm = analyse(d)
    res = {'file': os.path.basename(a.vme), 'header': hdr, 'comments': d.comments,
           'amble': {'HIR': d.hir, 'TIR': d.tir, 'HDR': d.hdr, 'TDR': d.tdr}, 'flow': s,
           'cfg': page_stats(cfg), 'ufm': page_stats(ufm)}
    for idc in s['idcode_checks']:
        res.setdefault('parts', []).append(PARTS.get(int(idc, 16), 'unknown'))
    if s['usercode_prog']:
        u = int(s['usercode_prog'][0], 16)
        res['usercode_ascii'] = u.to_bytes(4, 'big').decode('latin-1')
    if s['feabits_prog']:
        res['feabits_decoded'] = feabits_decode(int(s['feabits_prog'][0], 16))
    if a.svf:
        open(a.svf, 'w').write(transcript(d))
    if a.dump_dir:
        os.makedirs(a.dump_dir, exist_ok=True)
        open(os.path.join(a.dump_dir, 'cfg.bin'), 'wb').write(b''.join(cfg))
        open(os.path.join(a.dump_dir, 'ufm.bin'), 'wb').write(b''.join(ufm))
    js = json.dumps(res, indent=1)
    if a.json:
        open(a.json, 'w').write(js + '\n')
    print(js)


if __name__ == '__main__':
    main()
