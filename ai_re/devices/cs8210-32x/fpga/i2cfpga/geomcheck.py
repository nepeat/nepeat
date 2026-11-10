#!/usr/bin/env python3
"""Check which prjxray fabric matches the frames written by our bitstreams.

    geomcheck.py LISTINGDIR DBROOT/artix7

For every part.json given (one per fabric) replay the layer-1 listing with
the xc7frames engine and report: frames stored, addresses outside the
fabric, addresses never written, addresses written twice.  For the
xc7a100t fabric also cross-check part.json against tilegrid.json (every
tile's CLB_IO_CLK / BLOCK_RAM bit range must lie inside a configuration
column, and the largest frame extent per column must equal frame_count).
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import xc7geom  # noqa: E402
import xc7frames  # noqa: E402


def replay(ldir, geom):
    items = xc7frames.read_listing(ldir)
    eng = xc7frames.Engine(geom)
    out = []
    started = False
    for kind, reg, words, line in items:
        if kind == 'other':
            if line.split()[0] == 'sync':
                if started:
                    out.append(eng.stores)
                eng.reset()
                started = False
            continue
        try:
            if reg == 2:
                started = True
                eng.fdri(words)
            elif reg == 0xa:
                eng.mfwr(words)
            else:
                eng.reg_write(reg, words)
        except (SystemExit, KeyError, IndexError) as e:
            return 'replay failed: %s' % e
    if started:
        out.append(eng.stores)
    return out


def main():
    ldir, db = sys.argv[1], sys.argv[2]
    parts = ['xc7a35tcsg324-1', 'xc7a50tfgg484-1', 'xc7a100tfgg484-2', 'xc7a200tfbg484-2']
    for part in parts:
        pj = os.path.join(db, part, 'part.json')
        if not os.path.exists(pj):
            print('%-18s (part.json not fetched)' % part)
            continue
        g = xc7geom.Geometry(pj)
        r = replay(ldir, g)
        if isinstance(r, str):
            print('%-18s idcode %08x frames %5d: %s' % (part, g.idcode, g.frame_count(), r))
            continue
        for n, stores in enumerate(r):
            addrs = [a for a, fr, how in stores]
            s = set(addrs)
            print('%-18s idcode %08x frames %5d: image %d stored %d, distinct %d, '
                  'outside fabric %d, never written %d, written twice %d'
                  % (part, g.idcode, g.frame_count(), n, len(addrs), len(s),
                     len(s - g.valid), len(g.valid - s), len(addrs) - len(s)))
    # tilegrid cross-check for xc7a100t
    g = xc7geom.Geometry(os.path.join(db, 'xc7a100tfgg484-2', 'part.json'))
    tg = json.load(open(os.path.join(db, 'xc7a100t', 'tilegrid.json')))
    ext = {}
    bad = 0
    ntiles = 0
    for name, t in tg.items():
        for bt, b in t.get('bits', {}).items():
            ntiles += 1
            base = b['baseaddr'] if isinstance(b['baseaddr'], int) else int(b['baseaddr'], 16)
            col = base & ~0x7f
            key = (col, xc7geom.BUS[bt])
            last = base + b['frames'] - 1
            for a in range(base, last + 1):
                if a not in g.valid:
                    bad += 1
                    break
            ext[col] = max(ext.get(col, 0), (base & 0x7f) + b['frames'])
    mism = []
    for (bt, bottom, r), fcs in g.rows.items():
        for c, n in enumerate(fcs):
            col = xc7geom.far(bt, bottom, r, c, 0)
            if col in ext and ext[col] != n:
                mism.append((xc7geom.farstr(col), ext[col], n))
    cols = sum(len(v) for v in g.rows.values())
    print('tilegrid xc7a100t: %d tile bit-blocks, %d outside part.json; %d/%d columns '
          'covered by tiles; columns whose tile extent != frame_count: %d %s'
          % (ntiles, bad, len(ext), cols, len(mism), mism[:6]))


if __name__ == '__main__':
    main()
