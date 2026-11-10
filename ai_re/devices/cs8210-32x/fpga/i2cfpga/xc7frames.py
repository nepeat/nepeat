#!/usr/bin/env python3
"""Layer 2: packet listing (layer 1) <-> configuration frames + packing plan.

    xc7frames.py decode LISTINGDIR OUTDIR [--part PART_JSON] [--names a,b,..]
        Read LISTINGDIR/listing.txt + blobs/ (bitasm.py output), simulate the
        7-series configuration engine and write
            OUTDIR/<name>.frames   full configuration memory of every image
                                   (prjxray frames format, every frame of the
                                   device in FAR order, 101 words each)
            OUTDIR/plan.txt        the packing plan: everything that is not
                                   frame data, plus how the frames were packed
    xc7frames.py encode OUTDIR LISTINGDIR [--part PART_JSON]
        Rebuild listing.txt + blobs/ from plan.txt and the .frames files.
    xc7frames.py check LISTINGDIR [--part PART_JSON] [--names ...]
        decode + encode into a temp dir and compare listing and blobs.

Configuration-engine model (all of it verified by the fact that, for both
images in the flash, every one of the 9448 frames of the xc7a100t fabric is
written exactly once, no invalid or duplicate address is ever written, and
encode reproduces the stream byte for byte):

  * FAR write while CMD=WCFG starts a new frame write at that address.
  * FDRI data is a sequence of 101-word frames.  Frame k of a write lands
    at seq[start + k] where seq is the FAR auto-increment order including
    two pad slots after the last frame of every row (xc7geom.Geometry).
    The *last* frame of every FDRI write stays in the frame buffer and is
    not stored by the FDRI write itself ("pipeline" frame).
  * CMD=MFW then MFWR (dummy payload, zeros) copies the frame buffer to the
    current FAR: the first MFWR after an FDRI goes to the auto-incremented
    address (seq[start + k_last]), every later MFWR to the FAR written just
    before it.  So "FDRI ...; MFW; MFWR; FAR a; MFWR; FAR b; MFWR" stores
    the buffered frame at its natural address and at a, b.
  * If the buffered frame is not used by MFWR it is a dummy (all zero here).
  * CRC: CRC-32C (reflected poly 0x82F63B78) over 37-bit {reg[4:0], word}
    for every word written to any register; CMD RCRC resets it, and so does
    a write to the CRC register (the check).  `crc` lines in the plan are
    recomputed by the encoder.

Plan grammar (one item per line, '#' starts a comment):
    <any layer-1 listing line>   copied verbatim (sync, noop, w1 REG ..., raw, fill)
    image NAME                   following frame ops use NAME.frames
    fdri K [dummy=KEY] [pad=KEY,KEY..] [form=t1|t2]
                                 FDRI write of K frames starting at the FAR
                                 register (header form: type 1 if <= 2047
                                 words, else type-1 hdr + type-2 packet)
    mfw T [T..]                  MFWR of zeros: 8 words to the auto address
                                 (T[0], must match), then FAR t + 4 zero words
                                 for each later target; `A..B` is the inclusive
                                 run of consecutive valid frames A to B
    crc                          type-1 CRC write with the recomputed value
Extra frame contents (non-zero dummies / pad frames, if any) are kept in
OUTDIR/extra.frames keyed by name.
"""
import os
import struct
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import xc7geom  # noqa: E402
from xc7geom import WORDS, farstr  # noqa: E402

DEFAULT_PART = os.path.join(HERE, '..', 'work', 'src', 'prjxray-db', 'artix7',
                            'xc7a100tfgg484-2', 'part.json')

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
ZERO = (0,) * WORDS
MFWR_FIRST, MFWR_NEXT = 8, 4
TYPE1_MAX = 0x7ff


def regnum(s):
    return REGN[s] if s in REGN else int(s[1:], 16)


# ---------------------------------------------------------------- CRC / misc
_POLY = 0x82F63B78 << 1


def crc_word(addr, data, crc):
    val = (addr & 0x1f) << 32 | data
    for _ in range(37):
        if (val ^ crc) & 1:
            crc ^= _POLY
        val >>= 1
        crc >>= 1
    return crc


def ecc(frame):
    """7-series per-frame ECC (word 50, bits 12:0), as prjxray ecc.cc."""
    e = 0
    for idx, data in enumerate(frame):
        val = idx * 32 + (0x1360 if idx > 0x25 else 0x1340 if idx > 6 else 0x1320)
        if idx == 0x32:
            data &= 0xFFFFE000
        i = 0
        while data:
            if data & 1:
                e ^= val + i
            data >>= 1
            i += 1
    v = e & 0xfff
    v ^= v >> 8
    v ^= v >> 4
    v ^= v >> 2
    v ^= v >> 1
    e ^= (v & 1) << 12
    return e & 0x1fff


def words_of(b):
    return struct.unpack('>%dI' % (len(b) // 4), b)


# ---------------------------------------------------------------- frames files
def write_frames(path, geom, frames):
    with open(path, 'w') as f:
        for a in geom.seq:
            if isinstance(a, int):
                f.write('0x%08X ' % a + ','.join('0x%08X' % w for w in frames.get(a, ZERO)) + '\n')


def read_frames(path):
    out = {}
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            a, ws = line.split(None, 1)
            words = tuple(int(x, 16) for x in ws.replace(' ', '').split(','))
            assert len(words) == WORDS, line[:40]
            out[int(a, 16)] = words
    return out


# ---------------------------------------------------------------- listing io
def read_listing(ldir):
    bdir = os.path.join(ldir, 'blobs')
    items = []      # (line, reg or None, words or None)
    lines = open(os.path.join(ldir, 'listing.txt')).read().split('\n')
    i = 0
    while i < len(lines):
        line = lines[i]
        f = line.split()
        i += 1
        if not f:
            continue
        if f[0] == 'hdr1' and i < len(lines) and lines[i].startswith('w2 '):
            g = lines[i].split()
            i += 1
            data = words_of(open(os.path.join(bdir, g[1][1:] + '.bin'), 'rb').read())
            items.append(('w2form', regnum(f[1]), data, line + '\n' + lines[i - 1]))
        elif f[0] == 'w1':
            reg = regnum(f[1])
            if len(f) > 2 and f[2].startswith('@'):
                data = words_of(open(os.path.join(bdir, f[2][1:] + '.bin'), 'rb').read())
                items.append(('w1blob', reg, data, line))
            elif reg == 4 and len(f) == 3 and f[2] in CMDN:
                items.append(('w1', reg, (CMDN[f[2]],), line))
            else:
                items.append(('w1', reg, tuple(int(x, 16) for x in f[2:]), line))
        elif f[0] == 'w2':
            raise SystemExit('w2 without hdr1 not supported: ' + line)
        else:
            items.append(('other', None, None, line))
    return items


class Listing:
    """Builds a layer-1 listing exactly the way bitasm.dis names things."""

    def __init__(self, ldir):
        self.ldir = ldir
        os.makedirs(os.path.join(ldir, 'blobs'), exist_ok=True)
        self.lines = []
        self.nblob = 0
        self.crc = 0

    def blob(self, words):
        name = 'b%05d' % self.nblob
        self.nblob += 1
        open(os.path.join(self.ldir, 'blobs', name + '.bin'), 'wb').write(
            struct.pack('>%dI' % len(words), *words))
        return name

    def account(self, reg, words):
        if reg == 4 and words and words[0] == CMDN['RCRC']:
            self.crc = 0
            return
        if reg == 0:
            self.crc = 0
            return
        for w in words:
            self.crc = crc_word(reg, w, self.crc)

    def verbatim(self, line, reg=None, words=None):
        if line.split()[0] == 'sync':
            self.crc = 0
        self.lines.append(line)
        if reg is not None:
            self.account(reg, words)

    def write_blob(self, reg, words, form=None):
        if form is None:
            form = 't1' if len(words) <= TYPE1_MAX else 't2'
        if form == 't1':
            self.lines.append('w1 %s @%s %d' % (REG.get(reg, 'R%02x' % reg), self.blob(words), len(words)))
        else:
            self.lines.append('hdr1 %s 0' % REG.get(reg, 'R%02x' % reg))
            self.lines.append('w2 @%s %d' % (self.blob(words), len(words)))
        self.account(reg, words)

    def crc_write(self):
        v = self.crc
        self.lines.append('w1 CRC %08x' % v)
        self.crc = 0
        return v

    def close(self):
        open(os.path.join(self.ldir, 'listing.txt'), 'w').write('\n'.join(self.lines) + '\n')


# ---------------------------------------------------------------- decode
class Engine:
    """7-series frame-write state machine (see module docstring)."""

    def __init__(self, geom):
        self.g = geom
        self.reset()

    def reset(self):
        self.cmd = None
        self.far = None
        self.far_auto = False
        self.newwrite = False
        self.idx = None
        self.buf = None
        self.buf_used = False
        self.stores = []   # (addr, frame, how)

    def reg_write(self, reg, words):
        if reg == 4:
            self.cmd = words[0]
            if self.cmd == CMDN['WCFG']:
                self.newwrite = True
        elif reg == 1:
            self.far = words[0]
            self.far_auto = False
            if self.cmd == CMDN['WCFG']:
                self.newwrite = True

    def fdri(self, words):
        """Returns (start_index, list of frames, dropped buffer, used flag)."""
        assert len(words) % WORDS == 0, 'FDRI length %d not a frame multiple' % len(words)
        if not self.newwrite:
            raise SystemExit('FDRI continuing a previous write is not modelled')
        dropped = (self.buf, self.buf_used)
        self.newwrite = False
        if self.far not in self.g.index:
            raise SystemExit('FDRI at invalid FAR 0x%08x' % self.far)
        self.idx = self.g.index[self.far]
        start = self.idx
        frames = [tuple(words[k:k + WORDS]) for k in range(0, len(words), WORDS)]
        self.buf = None
        for fr in frames:
            if self.buf is not None:
                a = self.g.seq[self.idx]
                if isinstance(a, int):          # pad slots are not stored
                    self.stores.append((a, self.buf, 'fdri'))
                self.idx += 1
            self.buf = fr
        self.buf_used = False
        self.far_auto = True
        return start, frames, dropped

    def mfwr(self, words):
        target = self.g.seq[self.idx] if self.far_auto else self.far
        if not isinstance(target, int):
            raise SystemExit('MFWR into pad slot %r' % (target,))
        self.stores.append((target, self.buf, 'mfw'))
        self.buf_used = True
        self.far_auto = False
        return target


def noop_count(item):
    f = item[3].split() if item[0] == 'other' else []
    if f and f[0] == 'noop':
        return int(f[1]) if len(f) > 1 else 1
    return 0


def ranges(geom, targets):
    """Compress targets into A or A..B runs of consecutive valid frames."""
    vi = geom.valid_index
    out = []
    i = 0
    while i < len(targets):
        j = i
        while j + 1 < len(targets) and vi.get(targets[j + 1]) == vi[targets[j]] + 1:
            j += 1
        out.append('%08x' % targets[i] if i == j else '%08x..%08x' % (targets[i], targets[j]))
        i = j + 1
    return out


def expand(geom, toks):
    out = []
    for t in toks:
        if '..' in t:
            a, b = (int(x, 16) for x in t.split('..'))
            out.extend(geom.valid_list[geom.valid_index[a]:geom.valid_index[b] + 1])
        else:
            out.append(int(t, 16))
    return out


def decode(ldir, odir, part=DEFAULT_PART, names=None):
    geom = xc7geom.Geometry(part)
    items = read_listing(ldir)
    os.makedirs(odir, exist_ok=True)
    eng = Engine(geom)
    plan = []          # list of strings, or dicts for fdri (finalised later)
    extra = {}
    images = []        # (name, frames)
    stats = []
    cur_image = None
    last_fdri = None
    crc = 0
    n_stream = -1
    names = list(names or [])

    def finish_buffer(dropped):
        buf, used = dropped
        if buf is None or last_fdri is None:
            return
        if not used and buf != ZERO:
            key = 'dummy%d' % len(extra)
            extra[key] = buf
            last_fdri['dummy'] = key

    def close_image():
        nonlocal cur_image
        if cur_image is None:
            return
        finish_buffer((eng.buf, eng.buf_used))
        frames = {}
        for a, fr, how in eng.stores:
            if a in frames:
                raise SystemExit('frame 0x%08x written twice: not modelled' % a)
            if a not in geom.valid:
                raise SystemExit('frame 0x%08x invalid in this geometry' % a)
            frames[a] = fr
        name = cur_image
        images.append((name, frames))
        n_fdri = sum(1 for p in plan if isinstance(p, dict) and p.get('image') == name)
        n_mfw = sum(1 for a, fr, how in eng.stores if how == 'mfw')
        stats.append('%s: %d frames stored (%d of %d valid addresses, %d via MFW); '
                     '%d FDRI writes; %d non-zero frames'
                     % (name, len(eng.stores), len(frames), geom.frame_count(), n_mfw,
                        n_fdri, sum(1 for f in frames.values() if f != ZERO)))
        cur_image = None

    i = 0
    while i < len(items):
        kind, reg, words, line = items[i]
        i += 1
        if kind == 'other':
            if line.split()[0] == 'sync':
                close_image()
                eng.reset()
                crc = 0
                n_stream += 1
                last_fdri = None
            plan.append(line)
            continue
        # register write
        if reg in (2, 0xa) and cur_image is None:
            cur_image = names.pop(0) if names else 'image%d' % len(images)
            plan.append('image %s' % cur_image)
        if reg == 2:                                           # FDRI
            start, frs, dropped = eng.fdri(words)
            finish_buffer(dropped)
            form = 'w2' if kind == 'w2form' else 't1'
            auto = 't1' if len(words) <= TYPE1_MAX else 'w2'
            p = {'image': cur_image, 'k': len(frs), 'start': start, 'far': eng.far,
                 'form': None if form == auto else ('t2' if form == 'w2' else 't1')}
            pads = []
            for j, fr in enumerate(frs[:-1]):
                a = geom.seq[start + j]
                if not isinstance(a, int):
                    if fr != ZERO:
                        key = 'pad%d' % len(extra)
                        extra[key] = fr
                        pads.append(key)
                    else:
                        pads.append('0')
            p['pads'] = pads
            p['npad'] = len(pads)
            plan.append(p)
            last_fdri = p
        elif reg == 0xa:                                       # MFWR
            if any(words):
                raise SystemExit('non-zero MFWR payload not modelled')
            # gather the whole group: MFWR [noop G] (FAR MFWR [noop G])*
            group = [eng.mfwr(words)]
            sizes = [len(words)]
            for x in words:
                crc = crc_word(0xa, x, crc)
            gap = noop_count(items[i]) if i < len(items) else 0
            if gap:
                i += 1
            while True:
                j = i
                if not (j + 1 < len(items) and items[j][1] == 1 and items[j][0] == 'w1'
                        and items[j + 1][1] == 0xa and items[j + 1][0] == 'w1blob'
                        and not any(items[j + 1][2])):
                    break
                if gap and not (j + 2 < len(items) and noop_count(items[j + 2]) == gap):
                    break
                eng.reg_write(1, items[j][2])
                group.append(eng.mfwr(items[j + 1][2]))
                sizes.append(len(items[j + 1][2]))
                for r, w in ((1, items[j][2]), (0xa, items[j + 1][2])):
                    for x in w:
                        crc = crc_word(r, x, crc)
                i += 3 if gap else 2
            if sizes[0] != MFWR_FIRST or any(s != MFWR_NEXT for s in sizes[1:]):
                raise SystemExit('unexpected MFWR sizes %r' % sizes[:4])
            if last_fdri is None or eng.g.seq[last_fdri['start'] + last_fdri['k'] - 1] != group[0]:
                raise SystemExit('MFW group does not start at the auto address')
            plan.append('mfw ' + ('gap=%d ' % gap if gap else '') + ' '.join(ranges(geom, group)))
            continue
        elif reg == 0:                                         # CRC check
            if words[0] == crc:
                plan.append('crc')
            else:
                plan.append(line + '  # MISMATCH: recomputed %08x' % crc)
                print('warning: CRC mismatch in stream %d: file %08x computed %08x'
                      % (n_stream, words[0], crc))
            crc = 0
            continue
        else:
            eng.reg_write(reg, words)
            plan.append(line)
        if reg == 4 and words[0] == CMDN['RCRC']:
            crc = 0
        else:
            for x in words:
                crc = crc_word(reg, x, crc)
    close_image()

    # frames files
    for name, frames in images:
        write_frames(os.path.join(odir, name + '.frames'), geom, frames)
    if extra:
        with open(os.path.join(odir, 'extra.frames'), 'w') as f:
            for k, fr in extra.items():
                f.write(k + ' ' + ','.join('0x%08X' % w for w in fr) + '\n')
    # plan text
    out = ['# xc7frames packing plan (see xc7frames.py docstring for the grammar)',
           '# part: %s' % os.path.relpath(part, odir)]
    out += ['# ' + s for s in stats]
    for p in plan:
        if isinstance(p, dict):
            s = 'fdri %d' % p['k']
            if p.get('dummy'):
                s += ' dummy=' + p['dummy']
            if any(x != '0' for x in p['pads']):
                s += ' pad=' + ','.join(p['pads'])
            if p['form']:
                s += ' form=' + p['form']
            first = geom.seq[p['start']]
            last = geom.seq[p['start'] + p['k'] - 2] if p['k'] > 1 else None
            c = '  # %s' % farstr(first)
            if last is not None:
                c += ' .. %s' % (farstr(last) if isinstance(last, int) else 'pad')
                c += ' (%d stored' % (p['k'] - 1)
                c += ', %d pad' % p['npad'] if p['npad'] else ''
                c += ')'
            else:
                c += ' (buffer only)'
            s += c
            out.append(s)
        else:
            out.append(p)
    open(os.path.join(odir, 'plan.txt'), 'w').write('\n'.join(out) + '\n')
    return stats


# ---------------------------------------------------------------- encode
def encode(odir, ldir, part=DEFAULT_PART):
    geom = xc7geom.Geometry(part)
    lst = Listing(ldir)
    extra = {}
    xp = os.path.join(odir, 'extra.frames')
    if os.path.exists(xp):
        for line in open(xp):
            k, ws = line.split(None, 1)
            extra[k] = tuple(int(x, 16) for x in ws.split(','))
    frames = None
    far = None
    plan = [l.split('#', 1)[0].rstrip() for l in open(os.path.join(odir, 'plan.txt'))]
    plan = [l for l in plan if l]
    pending = None    # last fdri awaiting its buffered frame: (lst index, words, start, k)
    cmd = None

    def flush_pending(buf):
        nonlocal pending
        if pending is None:
            return
        words, start, k, form, dummy = pending
        if buf is None:
            buf = extra[dummy] if dummy else ZERO
        words.extend(buf)
        lst.write_blob(2, words, form)
        pending = None

    for n, line in enumerate(plan):
        f = line.split()
        op = f[0]
        if op == 'image':
            flush_pending(None)
            frames = read_frames(os.path.join(odir, f[1] + '.frames'))
            continue
        if op == 'fdri':
            flush_pending(None)
            k = int(f[1])
            opts = dict(x.split('=', 1) for x in f[2:])
            pads = opts.get('pad', '').split(',') if 'pad' in opts else []
            start = geom.index[far]
            words = []
            for j in range(k - 1):
                a = geom.seq[start + j]
                if isinstance(a, int):
                    words.extend(frames[a])
                else:
                    key = pads.pop(0) if pads else '0'
                    words.extend(ZERO if key == '0' else extra[key])
            pending = (words, start, k, opts.get('form'), opts.get('dummy'))
            # the buffered frame is decided by what follows (mfw or not)
            nxt = plan[n + 1].split()[0] if n + 1 < len(plan) else ''
            j = n + 1
            while j < len(plan) and plan[j].split()[0] not in ('mfw', 'fdri', 'image', 'sync'):
                j += 1
            if j < len(plan) and plan[j].split()[0] == 'mfw':
                toks = [t for t in plan[j].split()[1:] if not t.startswith('gap=')]
                pending_mfw_first = expand(geom, toks[:1])[0]
                auto = geom.seq[start + k - 1]
                if auto != pending_mfw_first:
                    raise SystemExit('plan: mfw first target %08x != auto address %r'
                                     % (pending_mfw_first, auto))
                flush_pending(frames[auto])
            else:
                flush_pending(None)
            continue
        if op == 'mfw':
            gap = 0
            if f[1].startswith('gap='):
                gap = int(f[1][4:])
                f = f[1:]
            targets = expand(geom, f[1:])
            gapline = 'noop' if gap == 1 else 'noop %d' % gap
            lst.write_blob(0xa, (0,) * MFWR_FIRST)
            if gap:
                lst.verbatim(gapline)
            for t in targets[1:]:
                lst.verbatim('w1 FAR %08x' % t, 1, (t,))
                lst.write_blob(0xa, (0,) * MFWR_NEXT)
                if gap:
                    lst.verbatim(gapline)
            far = targets[-1]
            continue
        if op == 'crc':
            lst.crc_write()
            continue
        # verbatim layer-1 line
        if op == 'w1':
            reg = regnum(f[1])
            if reg == 4 and len(f) == 3 and f[2] in CMDN:
                words = (CMDN[f[2]],)
            else:
                words = tuple(int(x, 16) for x in f[2:])
            if reg == 1:
                far = words[0]
            if reg == 4:
                cmd = words[0]
            lst.verbatim(line, reg, words)
        else:
            lst.verbatim(line)
    flush_pending(None)
    lst.close()
    return len(lst.lines), lst.nblob


# ---------------------------------------------------------------- check
def compare_listings(a, b):
    la = open(os.path.join(a, 'listing.txt')).read()
    lb = open(os.path.join(b, 'listing.txt')).read()
    if la != lb:
        x, y = la.split('\n'), lb.split('\n')
        n = next((k for k in range(min(len(x), len(y))) if x[k] != y[k]), min(len(x), len(y)))
        return 'listing differs at line %d: %r vs %r' % (n + 1, x[n:n + 1], y[n:n + 1])
    ba = sorted(os.listdir(os.path.join(a, 'blobs')))
    bb = sorted(os.listdir(os.path.join(b, 'blobs')))
    if ba != bb:
        return 'blob sets differ (%d vs %d)' % (len(ba), len(bb))
    for name in ba:
        if open(os.path.join(a, 'blobs', name), 'rb').read() != open(os.path.join(b, 'blobs', name), 'rb').read():
            return 'blob %s differs' % name
    return None


def main():
    args = sys.argv[1:]
    part = DEFAULT_PART
    names = None
    if '--part' in args:
        k = args.index('--part')
        part = args[k + 1]
        del args[k:k + 2]
    if '--names' in args:
        k = args.index('--names')
        names = args[k + 1].split(',')
        del args[k:k + 2]
    if len(args) < 2:
        raise SystemExit(__doc__)
    if args[0] == 'decode':
        for s in decode(args[1], args[2], part, names):
            print(s)
    elif args[0] == 'encode':
        print('%d listing lines, %d blobs' % encode(args[1], args[2], part))
    elif args[0] == 'check':
        with tempfile.TemporaryDirectory() as t:
            for s in decode(args[1], os.path.join(t, 'f'), part, names):
                print(s)
            encode(os.path.join(t, 'f'), os.path.join(t, 'l'), part)
            err = compare_listings(args[1], os.path.join(t, 'l'))
            if err:
                raise SystemExit('DIFFER: ' + err)
            print('LISTING+BLOBS IDENTICAL')
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main()
