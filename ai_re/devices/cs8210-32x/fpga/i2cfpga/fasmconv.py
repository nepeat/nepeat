#!/usr/bin/env python3
"""Layer 3: configuration frames <-> FASM + "unknown bits", lossless.

    fasmconv.py frames2fasm FRAMES OUT.fasm OUT.unknown
        Disassemble FRAMES (xc7frames.py output) with the Project X-Ray
        database into FASM features, then re-assemble that FASM and record
        every bit where the result differs from FRAMES in OUT.unknown.
    fasmconv.py fasm2frames IN.fasm IN.unknown OUT.frames
        Assemble IN.fasm, apply IN.unknown, recompute the per-frame ECC and
        write a full frames file.
    fasmconv.py check FRAMES
        frames2fasm + fasm2frames into a temp dir, compare frames exactly.

Options: --db DBROOT (prjxray-db/artix7), --part PART (default
xc7a100tfgg484-2: the xc7a100t fabric; our part is the XC7A75T which uses
the same die, see LAYER2.md).

ECC: word 50 bits [12:0] of every frame hold an ECC (xc7frames.ecc).  It
is removed before disassembly and recomputed on assembly.  Rule observed in
all four images we have: Vivado computes it with the INIT bits of LUTs used
as SRL/LUTRAM (run-time writable, found from the FASM) taken as zero
(dynamic_mask).  Frames whose stored ECC still differs from that rule are
listed as `ecc FAR VALUE` lines in the unknown file.

Unknown-bits file, one item per line ('#' comments carry the tile name):
    ecc 0xFAR 0xVALUE           stored ECC differs from the ECC rule
    + 0xFAR WORD BIT            bit set in the frames, not produced by FASM
    - 0xFAR WORD BIT            bit produced by FASM but clear in the frames
"""
import os
import re
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, '..', 'work')
sys.path.insert(0, HERE)
sys.path[1:1] = [os.path.join(WORK, 'src', 'fasm'), os.path.join(WORK, 'src', 'prjxray')]
import warnings  # noqa: E402
warnings.filterwarnings('ignore', message='.*antlr.*')
warnings.filterwarnings('ignore', message='.*textx.*')

import fasm  # noqa: E402
import fasm.output  # noqa: E402
from prjxray.db import Database  # noqa: E402
from prjxray import fasm_disassembler, fasm_assembler  # noqa: E402

import xc7frames  # noqa: E402
import xc7geom  # noqa: E402
from xc7geom import WORDS  # noqa: E402

DB = os.path.join(WORK, 'src', 'prjxray-db', 'artix7')
PART = 'xc7a100tfgg484-2'
ECC_WORD, ECC_MASK = 50, 0x1fff


def opts(args):
    o = {'db': DB, 'part': PART}
    for k in ('db', 'part'):
        if '--' + k in args:
            i = args.index('--' + k)
            o[k] = args[i + 1]
            del args[i:i + 2]
    return o


LUTMODE_RE = re.compile(r'^(\w+\.SLICEM_X\d\.[A-D]LUT)\.(SRL|RAM)$')


def dynamic_mask(db, fasm_path):
    """Frame bits holding run-time-writable contents (LUTs used as SRL or
    LUTRAM, per the FASM): Vivado computes the stored frame ECC with these
    bits taken as 0.  Returns {far: [101 mask words]}."""
    luts = set()
    for line in open(fasm_path):
        m = LUTMODE_RE.match(line.split('#', 1)[0].strip())
        if m:
            luts.add(m.group(1))
    if not luts:
        return {}
    with tempfile.NamedTemporaryFile('w', suffix='.fasm', delete=False) as f:
        for lut in sorted(luts):
            f.write("%s.INIT[63:0] = 64'hFFFFFFFFFFFFFFFF\n" % lut)
        tmp = f.name
    try:
        return assemble(db, tmp)
    finally:
        os.unlink(tmp)


def ecc_rule(frame, mask):
    """Expected stored ECC: computed over the frame with dynamic bits cleared."""
    if mask is None:
        return xc7frames.ecc(frame)
    return xc7frames.ecc([w & ~m for w, m in zip(frame, mask)])


def strip_ecc(frames):
    out = {}
    for a, w in frames.items():
        w = list(w)
        w[ECC_WORD] &= ~ECC_MASK
        out[a] = w
    return out


def to_bitdata(frames):
    bd = {}
    for a, w in frames.items():
        words, bits = set(), set()
        for i, x in enumerate(w):
            if x:
                words.add(i)
                b = 0
                while x:
                    if x & 1:
                        bits.add(i * 32 + b)
                    x >>= 1
                    b += 1
        if bits:
            bd[a] = (words, bits)
    return bd


class Tiles:
    """Which tile(s) own a given (frame, word)."""

    def __init__(self, db):
        self.smap = db.grid().get_segment_map()

    def at(self, frame, word):
        out = []
        for bi in self.smap.segment_info_for_frame(frame):
            b = bi.bits
            if b.offset <= word < b.offset + b.words:
                out.append(bi.tile)
        return out


def assemble(db, fasm_path):
    asm = fasm_assembler.FasmAssembler(db)
    asm.parse_fasm_filename(fasm_path)
    return asm.get_frames(sparse=True)


def frames2fasm(frames_path, fasm_path, unk_path, o):
    db = Database(o['db'], o['part'])
    orig = xc7frames.read_frames(frames_path)
    clean = strip_ecc(orig)
    dis = fasm_disassembler.FasmDisassembler(db)
    model = fasm.output.merge_and_sort(
        dis.find_features_in_bitstream(to_bitdata(clean), verbose=False),
        zero_function=dis.is_zero_feature, sort_key=db.grid().tile_key)
    with open(fasm_path, 'w') as f:
        f.write(fasm.fasm_tuple_to_string(model, canonical=False))
    got = assemble(db, fasm_path)
    tiles = Tiles(db)
    lines = ['# unknown bits for %s (see fasmconv.py docstring)' % os.path.basename(frames_path)]
    stats = {'set_bits': 0, 'fasm_bits': 0, 'plus': 0, 'minus': 0, 'ecc': 0}
    per_tile = {}
    dmask = dynamic_mask(db, fasm_path)
    stats['dynamic_mask_frames'] = len(dmask)
    for a in sorted(orig):
        w = orig[a]
        if ecc_rule(w, dmask.get(a)) != w[ECC_WORD] & ECC_MASK:
            lines.append('ecc 0x%08X 0x%04X' % (a, w[ECC_WORD] & ECC_MASK))
            stats['ecc'] += 1
    for a in sorted(set(clean) | set(got)):
        c = clean.get(a, [0] * WORDS)
        g = got.get(a, [0] * WORDS)
        for i in range(WORDS):
            stats['set_bits'] += bin(c[i]).count('1')
            stats['fasm_bits'] += bin(c[i] & g[i]).count('1')
            for sign, diff in (('+', c[i] & ~g[i]), ('-', g[i] & ~c[i])):
                b = 0
                while diff:
                    if diff & 1:
                        t = tiles.at(a, i)
                        lines.append('%s 0x%08X %3d %2d  # %s %s' % (
                            sign, a, i, b, xc7geom.farstr(a), ','.join(t) or '(no tile)'))
                        stats['plus' if sign == '+' else 'minus'] += 1
                        key = ','.join(t) or '(no tile) ' + xc7geom.farstr(a & ~0x7f)
                        per_tile[key] = per_tile.get(key, 0) + 1
                    diff >>= 1
                    b += 1
    open(unk_path, 'w').write('\n'.join(lines) + '\n')
    stats['features'] = sum(1 for l in open(fasm_path) if l.strip() and not l.startswith('#'))
    return stats, per_tile


def read_unknown(path):
    ecc, plus, minus = {}, [], []
    for line in open(path):
        line = line.split('#', 1)[0].split()
        if not line:
            continue
        if line[0] == 'ecc':
            ecc[int(line[1], 16)] = int(line[2], 16)
        else:
            item = (int(line[1], 16), int(line[2]), int(line[3]))
            (plus if line[0] == '+' else minus).append(item)
    return ecc, plus, minus


def fasm2frames(fasm_path, unk_path, out_path, o, part_json=xc7frames.DEFAULT_PART):
    db = Database(o['db'], o['part'])
    geom = xc7geom.Geometry(part_json)
    got = assemble(db, fasm_path)
    frames = {a: list(got.get(a, [0] * WORDS)) for a in geom.valid_list}
    extra = set(got) - geom.valid
    if extra:
        raise SystemExit('FASM produced frames outside the device: %r' % sorted(extra)[:5])
    ecc, plus, minus = read_unknown(unk_path)
    for a, i, b in plus:
        frames[a][i] |= 1 << b
    for a, i, b in minus:
        frames[a][i] &= ~(1 << b)
    dmask = dynamic_mask(db, fasm_path)
    for a, w in frames.items():
        w[ECC_WORD] = (w[ECC_WORD] & ~ECC_MASK) | ecc.get(a, ecc_rule(w, dmask.get(a)))
    xc7frames.write_frames(out_path, geom, {a: tuple(w) for a, w in frames.items()})


def main():
    args = sys.argv[1:]
    o = opts(args)
    if len(args) < 2:
        raise SystemExit(__doc__)
    if args[0] == 'frames2fasm':
        stats, per_tile = frames2fasm(args[1], args[2], args[3], o)
        print(stats)
    elif args[0] == 'fasm2frames':
        fasm2frames(args[1], args[2], args[3], o)
    elif args[0] == 'check':
        with tempfile.TemporaryDirectory() as t:
            stats, per_tile = frames2fasm(args[1], t + '/x.fasm', t + '/x.unknown', o)
            print(stats)
            fasm2frames(t + '/x.fasm', t + '/x.unknown', t + '/x.frames', o)
            a = open(args[1]).read()
            b = open(t + '/x.frames').read()
            print('FRAMES IDENTICAL' if a == b else 'FRAMES DIFFER')
            if a != b:
                raise SystemExit(1)
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main()
