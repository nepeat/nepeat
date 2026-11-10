#!/usr/bin/env python3
"""Layer 4: FASM <-> lossless netlist (cells, nets, unassigned).

    fasmnet.py build IN.fasm OUT.net.json      FASM -> netlist
    fasmnet.py tofasm IN.net.json OUT.fasm     netlist -> FASM (merge_and_sort)
    fasmnet.py check IN.fasm [OUT.net.json]    build + tofasm, compare with IN
    fasmnet.py stats IN.net.json               counts
    fasmnet.py split NET.json BLOCKS.json DIR  one file per block (blocks.py annotation)
    fasmnet.py merge DIR OUT.net.json          inverse of split
    fasmnet.py same A.net.json B.net.json      compare two netlists (canonical JSON)

Every FASM line of IN is owned by exactly one of
  cell        a site instance (SLICE_X10Y85, RAMB18_X0Y44, IOB_X0Y17, PCIE_X0Y0,
              ...), or a tile-level configuration pseudo-cell ("TILE:<tile>")
              for tile features that belong to no single site (BRAM ZALMOST_*,
              CASCOUT_*)
  net         a routed signal: the pip lines that realise it, plus clock-spine
              enable lines (HROW *_ACTIVE, REBUF GCLKn_ENABLE_*, HCLK_CMT *_USED,
              HCLK ENABLE_BUFFER.*) of wires the net uses
  unassigned  anything the above cannot own, with a reason
The raw text of each line is stored with its owner, so `tofasm` is exact by
construction; `check` proves it (every line owned once, merge_and_sort of the
regenerated FASM == IN, byte for byte).

Nets are reconstructed from the routing graph (xc7graph.py): every node is
walked back through its active driver (a FASM pip, or an `always` ppip;
`default` ppips give the IMUX/BYP/FAN VCC tie when nothing else drives) to a
root.  A root is a site output pin (the net driver), a TIEOFF (constant), the
INT default VCC, or an undriven node.  A net is everything with the same root.
"""
import collections
import gzip
import json
import os
import re
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, '..', 'work')
sys.path.insert(0, HERE)
sys.path[1:1] = [os.path.join(WORK, 'src', 'fasm'), os.path.join(WORK, 'src', 'prjxray')]
sys.setrecursionlimit(100000)

import xc7graph  # noqa: E402

SINGLE_SITE = {'RAMB36', 'MMCME2_ADV', 'PLLE2_ADV', 'GTPE2_CHANNEL', 'GTPE2_COMMON', 'PCIE'}
BY_NAME = {'BUFGCTRL', 'BUFHCE'}
IO_FLIPPED = {'IOB', 'ILOGIC', 'OLOGIC', 'IDELAY'}
RE_SLICE = re.compile(r'^SLICE[LM]_X(\d)$')
RE_Y = re.compile(r'^(\w+?)_Y(\d+)$')


def log(*a):
    print('[%6.1fs]' % (time.time() - T0), *a, file=sys.stderr, flush=True)


T0 = time.time()


def read_fasm(path):
    """[(raw line, feature)] for every non-empty line (no comments expected)."""
    out = []
    for line in open(path):
        line = line.rstrip('\n')
        if not line.strip():
            continue
        if '#' in line or '{' in line:
            raise SystemExit('unexpected FASM syntax (comment/annotation): %r' % line)
        feat = line.split('=', 1)[0].strip()
        out.append((line, feat))
    return out


def open_any(path, mode='rt'):
    return gzip.open(path, mode) if path.endswith('.gz') else open(path, mode)


class Builder:
    def __init__(self, fab):
        self.fab = fab
        self.cells = {}          # name -> dict
        self.nets = {}           # name -> dict
        self.unassigned = []     # [raw, reason]
        self.fasm_drv = collections.defaultdict(list)   # (tile, dst) -> [(src, raw, bidir)]
        self.pip_lines = []      # (raw, tile, a, b, bidir)
        self.routecfg = []       # (raw, tile, wire)
        self.root = {}           # node -> root descriptor
        self.edge = {}           # node -> (kind, tile, dst, src, raw) chosen driver
        self.notes = collections.Counter()
        self.examples = {}

    def note(self, key, ex=None):
        self.notes[key] += 1
        if ex is not None and len(self.examples.setdefault(key, [])) < 8:
            self.examples[key].append(ex)

    # ---------------------------------------------------------------- cells
    def site_for(self, tile, parts):
        """(site_idx, n_parts_used) or None."""
        tt = self.fab.tt[self.fab.ttype(tile)]
        p0 = parts[0]
        m = RE_SLICE.match(p0)
        if m:
            for i, s in enumerate(tt['sites']):
                if s['type'] == p0.split('_')[0] and s['x'] == int(m.group(1)):
                    return i, 1
            return None
        if p0 in BY_NAME and len(parts) > 2:
            for i, s in enumerate(tt['sites']):
                if s['prefix'] == p0 and '%s_%s' % (p0, s['name']) == parts[1]:
                    return i, 2
            return None
        if p0 in SINGLE_SITE:
            c = [i for i, s in enumerate(tt['sites']) if s['prefix'] == p0]
            return (c[0], 1) if len(c) == 1 else None
        m = RE_Y.match(p0)
        if m:
            y = int(m.group(2))
            if m.group(1) in IO_FLIPPED:
                # prjxray names IOB/IOI sites of a 2-site tile by global Y parity:
                # _Y0 is the upper site (even Y), _Y1 the lower one (as in
                # fasm2bels get_iob_site/get_ioi_site: sites_by_y[1 - Y])
                n = sum(1 for s in tt['sites'] if s['prefix'] == m.group(1))
                if n == 2:
                    y = 1 - y
            c = [i for i, s in enumerate(tt['sites'])
                 if s['prefix'] == m.group(1) and s['y'] == y]
            if len(c) > 1:   # e.g. RAMB18_Y0 = FIFO18E1 site in BRAM tiles
                c = sorted(c, key=lambda i: tt['sites'][i]['type'])[:1]
            if not c and self.fab.ttype(tile).endswith('_SING'):
                # *_SING tiles use the segbits (and site names) of the full tile
                # type through a tilegrid alias: IOB_Y1 = the only IOB site
                c = [i for i, s in enumerate(tt['sites']) if s['prefix'] == m.group(1)]
                c = c if len(c) == 1 else []
            return (c[0], 1) if c else None
        return None

    def cell(self, tile, idx, implicit=False):
        fab = self.fab
        name = fab.site_name(tile, idx) or '%s/site%d' % (tile, idx)
        c = self.cells.get(name)
        if c is None:
            s = fab.tt[fab.ttype(tile)]['sites'][idx]
            c = dict(name=name, kind='site', type=s['type'], tile=tile, idx=idx,
                     fasm=[], pins={}, implicit=implicit)
            self.cells[name] = c
        return c

    def tilecell(self, tile):
        name = 'TILE:' + tile
        c = self.cells.get(name)
        if c is None:
            c = dict(name=name, kind='tile', type=self.fab.ttype(tile) + '_TILE', tile=tile,
                     idx=None, fasm=[], pins={}, implicit=False)
            self.cells[name] = c
        return c

    ROUTECFG = [
        (re.compile(r'^(CLK_HROW_CK_IN_[LR]\d+)_ACTIVE$'), r'\1'),
        (re.compile(r'^(CLK_HROW_R_CK_GCLK\d+)_ACTIVE$'), r'\1'),
        # REBUF enables: the GCLK line through this tile (either half)
        (re.compile(r'^GCLK(\d+)_ENABLE_ABOVE$'), r'CLK_BUFG_REBUF_R_CK_GCLK\1_TOP|CLK_BUFG_REBUF_R_CK_GCLK\1_BOT'),
        (re.compile(r'^GCLK(\d+)_ENABLE_BELOW$'), r'CLK_BUFG_REBUF_R_CK_GCLK\1_BOT|CLK_BUFG_REBUF_R_CK_GCLK\1_TOP'),
        (re.compile(r'^(HCLK_CMT_CK_BUFHCLK\d+)_USED$'), r'\1'),
        (re.compile(r'^ENABLE_BUFFER\.(\w+)$'), r'\1'),
    ]

    def classify(self, lines):
        fab = self.fab
        for raw, feat in lines:
            tile, rest = feat.split('.', 1)
            ttn = fab.ttype(tile)
            tt = fab.tt[ttn]
            parts = rest.split('.')
            if len(parts) == 2 and (parts[0], parts[1]) in tt['fasm_pips']:
                a, b = parts
                bidir = tt['fasm_pips'][(a, b)][2]
                self.pip_lines.append((raw, tile, a, b, bidir))
                self.fasm_drv[(tile, a)].append((b, raw, bidir))
                if bidir:
                    self.fasm_drv[(tile, b)].append((a, raw, bidir))
                continue
            r = self.site_for(tile, parts)
            if r is not None and ttn.startswith('BRAM_') and parts[0] == 'RAMB36':
                # RAMB36 = both RAMB18 halves; its few own bits (RAM_EXTENSION_*,
                # EN_ECC_*, BRAM36_*) are kept with the tile, and the RAMB18
                # sites own the pins (see LAYER4.md)
                self.tilecell(tile)['fasm'].append(raw)
                continue
            if r is not None:
                self.cell(tile, r[0])['fasm'].append(raw)
                continue
            base = parts[0] if len(parts) == 1 else rest.split('[')[0]
            for rx, rep in self.ROUTECFG:
                m = rx.match(base)
                if m:
                    ws = [w for w in m.expand(rep).split('|') if w in tt['wireset']]
                    if ws:
                        self.routecfg.append((raw, tile, ws))
                        break
            else:
                if ttn.startswith('BRAM_'):
                    self.tilecell(tile)['fasm'].append(raw)
                else:
                    self.unassigned.append([raw, 'no site/pip/route-config rule'])

    # ---------------------------------------------------------------- routing
    def drivers(self, node):
        fab = self.fab
        out = []
        for t, w in node:
            for src, raw, bidir in self.fasm_drv.get((t, w), ()):
                out.append(('fasm', t, w, src, raw))
            for src, kind, bidir in fab.tt[fab.ttype(t)]['by_dst'].get(w, ()):
                if kind == 'always':
                    out.append(('always', t, w, src, None))
        return out

    def resolve(self, node, stack=None):
        """Root of node: ('pin', tile, idx, pin) | ('default',) | ('undriven', node0)."""
        r = self.root.get(node)
        if r is not None:
            return r
        if stack is None:
            stack = set()
        if node in stack:
            return None
        fab = self.fab
        for t, i, p, d in fab.site_pins_on_node(node):
            if d == 'OUT':
                r = ('pin', t, i, p)
                self.root[node] = r
                return r
        drv = self.drivers(node)
        stack.add(node)
        cand = []
        for e in drv:
            src_node = fab.node(e[1], e[3])
            if src_node == node:
                continue
            rr = self.resolve(src_node, stack)
            if rr is not None:
                cand.append((e, rr))
        stack.discard(node)
        if cand:
            def score(x):
                e, rr = x
                used = rr[0] == 'pin' and bool(self.cells.get(fab.site_name(rr[1], rr[2]), {}).get('fasm'))
                return (e[0] != 'fasm', rr[0] != 'pin', not used, rr[0] == 'undriven', e[1:4])
            cand.sort(key=score)
            nf = sum(1 for e, rr in cand if e[0] == 'fasm')
            good = [x for x in cand if x[1][0] == 'pin' and score(x)[2] is False]
            if nf > 1:
                self.note('node with >1 active FASM driver', node[0])
            elif nf == 0 and len(good) > 1:
                self.note('node with >1 always-ppip driver reaching a used site pin', node[0])
            e, rr = cand[0]
            self.edge[node] = e
            self.root[node] = rr
            return rr
        if drv and stack:
            # every driver is part of the current cycle: let the caller decide
            return None
        for t, w in node:
            for src, kind, bidir in fab.tt[fab.ttype(t)]['by_dst'].get(w, ()):
                if kind == 'default':
                    r = ('default',)
                    self.edge[node] = ('default', t, w, src, None)
                    self.root[node] = r
                    return r
        r = ('undriven', node[0])
        self.root[node] = r
        return r

    def unknown_drivers(self, node):
        fab = self.fab
        n = 0
        for t, w in node:
            for src, kind, bidir in fab.tt[fab.ttype(t)]['by_dst'].get(w, ()):
                if kind == 'unknown':
                    n += 1
        return n

    def net_name(self, root):
        if root[0] == 'pin':
            _, t, i, p = root
            s = self.fab.tt[self.fab.ttype(t)]['sites'][i]
            if s['type'] == 'TIEOFF':
                return '%s/%s' % ({'HARD1': '$VCC', 'HARD0': '$GND'}.get(p, '$TIE_' + p),
                                  self.fab.site_name(t, i))
            return '%s/%s' % (self.fab.site_name(t, i), p)
        if root[0] == 'default':
            return '$VCC_DEFAULT'
        return '$UNDRIVEN/%s/%s' % root[1]

    def get_net(self, root):
        name = self.net_name(root)
        n = self.nets.get(name)
        if n is None:
            drv = None
            if root[0] == 'pin':
                _, t, i, p = root
                s = self.fab.tt[self.fab.ttype(t)]['sites'][i]
                if s['type'] == 'TIEOFF':
                    drv = ['$' + ('VCC' if p == 'HARD1' else 'GND'), p]
                else:
                    c = self.cell(t, i, implicit=True)
                    drv = [c['name'], p]
                    c['pins'][p] = name
            elif root[0] == 'default':
                drv = ['$VCC', 'default']
            n = dict(name=name, driver=drv, sinks=[], fasm=[], ppips=[], const=(
                drv is not None and drv[0] in ('$VCC', '$GND')))
            self.nets[name] = n
        return n

    def path_edges(self, node):
        """Driver edges from node back to its root."""
        out = []
        seen = set()
        while node in self.edge and node not in seen:
            seen.add(node)
            e = self.edge[node]
            out.append(e)
            if e[0] == 'default':
                break
            node = self.fab.node(e[1], e[3])
        return out

    FEATURELESS_OK = {'IPAD', 'TIEOFF'}

    def real_driver(self, r, node):
        """A root pin drives for real if its site has FASM lines (or is an
        IPAD/TIEOFF) or the path from it uses at least one FASM pip."""
        _, t, i, p = r
        name = self.fab.site_name(t, i)
        c = self.cells.get(name)
        if c is not None and c['fasm']:
            return True
        if self.fab.tt[self.fab.ttype(t)]['sites'][i]['type'] in self.FEATURELESS_OK:
            return True
        return any(e[0] == 'fasm' for e in self.path_edges(node))

    def cell_pins(self, c, used_ppips):
        fab = self.fab
        unk = 0
        s = fab.tt[fab.ttype(c['tile'])]['sites'][c['idx']]
        for p, (w, d) in sorted(s['pins'].items()):
            n = fab.node(c['tile'], w)
            r = self.resolve(n)
            if d == 'OUT':
                if r[0] == 'pin' and r[1] == c['tile'] and r[2] == c['idx'] and r[3] == p:
                    name = self.net_name(r)
                    if name in self.nets:
                        c['pins'][p] = name
                continue
            if r[0] == 'undriven':
                if self.unknown_drivers(n):
                    unk += 1
                    c.setdefault('unknown_pins', []).append(p)
                continue
            if r[0] == 'default' and not c['fasm']:
                continue
            if r[0] == 'pin' and not self.real_driver(r, n):
                self.note('input pins fed only by a dedicated wire from an unused site',
                          '%s/%s' % (c['name'], p))
                continue
            net = self.get_net(r)
            net['sinks'].append([c['name'], p])
            c['pins'][p] = net['name']
            for e in self.path_edges(n):
                if e[0] in ('always', 'default'):
                    used_ppips[net['name']].add('%s.%s.%s %s' % (e[1], e[2], e[3], e[0]))
        return unk

    def route(self):
        fab = self.fab
        log('routing: %d pip lines, %d route-config lines' % (len(self.pip_lines), len(self.routecfg)))
        # 1. every FASM pip -> owning net (root of its driving end)
        used_ppips = collections.defaultdict(set)
        pip_owner = {}
        for raw, t, a, b, bidir in self.pip_lines:
            na, nb = fab.node(t, a), fab.node(t, b)
            self.resolve(na)
            self.resolve(nb)
            ea, eb = self.edge.get(na), self.edge.get(nb)
            if ea and ea[0] == 'fasm' and ea[4] == raw:
                drv_end = nb
            elif eb and eb[0] == 'fasm' and eb[4] == raw:
                drv_end = na   # bidirectional pip used b <- a
                self.note('bidirectional pip used reversed', raw)
            else:
                drv_end = nb
                self.note('FASM pip not chosen as driver of its dst node', raw)
            net = self.get_net(self.root[drv_end])
            net['fasm'].append(raw)
            pip_owner[raw] = net['name']
            for e in self.path_edges(drv_end) + ([ea] if drv_end is nb and ea else []):
                if e[0] in ('always', 'default'):
                    used_ppips[net['name']].add('%s.%s.%s %s' % (e[1], e[2], e[3], e[0]))
        log('pips assigned to %d nets' % len(self.nets))
        # 2. sinks and drivers of every cell pin (implicit cells created on the
        #    way, as drivers of nets, are processed too)
        unk = 0
        done = set()
        while True:
            todo = [c for c in self.cells.values() if c['name'] not in done]
            if not todo:
                break
            for c in todo:
                done.add(c['name'])
                if c['kind'] == 'site':
                    unk += self.cell_pins(c, used_ppips)
        self.notes['cell input pins whose only possible drivers are db-unknown pips'] = unk
        for name, pp in used_ppips.items():
            self.nets[name]['ppips'] = sorted(pp)
        # 3. clock-spine enables -> net using that wire
        for raw, t, ws in self.routecfg:
            for w in ws:
                r = self.resolve(fab.node(t, w))
                name = self.net_name(r)
                if r[0] == 'pin' and name in self.nets and self.nets[name]['fasm']:
                    self.nets[name]['fasm'].append(raw)
                    break
            else:
                self.unassigned.append([raw, 'route-config wire %s/%s not on a routed net' % (t, ws)])
        log('nets %d, cells %d' % (len(self.nets), len(self.cells)))

    def result(self, src):
        cells = []
        for c in sorted(self.cells.values(), key=lambda c: c['name']):
            cells.append(dict(name=c['name'], kind=c['kind'], type=c['type'], tile=c['tile'],
                              implicit=c['implicit'], pins=dict(sorted(c['pins'].items())),
                              unknown_pins=c.get('unknown_pins', []),
                              fasm=sorted(c['fasm'])))
        nets = []
        for n in sorted(self.nets.values(), key=lambda n: n['name']):
            nets.append(dict(name=n['name'], driver=n['driver'], const=n['const'],
                             sinks=sorted(n['sinks']), fasm=sorted(n['fasm']), ppips=n['ppips']))
        return dict(format='i2cfpga-netlist-1', source=os.path.basename(src),
                    fabric=xc7graph.FABRIC, notes=dict(self.notes),
                    note_examples=self.examples, cells=cells, nets=nets,
                    unassigned=sorted(self.unassigned))


def build(fasm_path, out_path):
    fab = xc7graph.Fabric()
    log('fabric loaded')
    lines = read_fasm(fasm_path)
    b = Builder(fab)
    b.classify(lines)
    log('classified %d lines' % len(lines))
    b.route()
    res = b.result(fasm_path)
    with open_any(out_path, 'wt') as f:
        json.dump(res, f, indent=0, separators=(',', ':'))
    log('wrote', out_path)
    return res, lines


def owned_lines(net):
    out = []
    for c in net['cells']:
        out += [(l, 'cell:' + c['name']) for l in c['fasm']]
    for n in net['nets']:
        out += [(l, 'net:' + n['name']) for l in n['fasm']]
    out += [(l, 'unassigned') for l, why in net['unassigned']]
    return out


def tofasm(net, out_path):
    """Write the netlist's FASM through prjxray-style merge_and_sort."""
    import warnings
    warnings.filterwarnings('ignore')
    import fasm
    import fasm.output
    from prjxray.db import Database
    db = Database(xc7graph.DB, 'xc7a100tfgg484-2')
    lines = [l for l, _ in owned_lines(net)]
    with tempfile.NamedTemporaryFile('w', suffix='.fasm', delete=False) as f:
        f.write('\n'.join(lines) + '\n')
        tmp = f.name
    try:
        model = fasm.output.merge_and_sort(fasm.parse_fasm_filename(tmp),
                                           sort_key=db.grid().tile_key)
        txt = fasm.fasm_tuple_to_string(model, canonical=False)
    finally:
        os.unlink(tmp)
    with open(out_path, 'w') as f:
        f.write(txt)
    return txt


def stats(net):
    S = collections.OrderedDict()
    S['cells'] = len(net['cells'])
    S['cells by type'] = dict(collections.Counter(c['type'] for c in net['cells']).most_common())
    S['implicit cells (no FASM lines of their own)'] = sum(1 for c in net['cells'] if not c['fasm'])
    S['nets'] = len(net['nets'])
    S['nets with driver pin + >=1 sink'] = sum(1 for n in net['nets'] if n['driver'] and n['sinks'] and not n['const'])
    S['constant nets (TIEOFF / default VCC)'] = sum(1 for n in net['nets'] if n['const'])
    S['undriven nets'] = sum(1 for n in net['nets'] if n['driver'] is None)
    S['nets without sinks'] = sum(1 for n in net['nets'] if not n['sinks'])
    S['FASM lines owned by cells'] = sum(len(c['fasm']) for c in net['cells'])
    S['FASM lines owned by nets'] = sum(len(n['fasm']) for n in net['nets'])
    S['FASM lines unassigned'] = len(net['unassigned'])
    S['ppips used by nets (derived, not FASM)'] = sum(len(n['ppips']) for n in net['nets'])
    fan = collections.Counter(min(len(n['sinks']), 33) for n in net['nets'] if not n['const'])
    S['fanout histogram (33 = >32)'] = dict(sorted(fan.items()))
    S['notes'] = net['notes']
    S['unassigned reasons'] = dict(collections.Counter(w.split(' (')[0] for l, w in net['unassigned']))
    return S


def check(fasm_path, net_path=None):
    t = tempfile.mkdtemp(prefix='fasmnet-')
    net_path = net_path or os.path.join(t, 'x.net.json')
    res, lines = build(fasm_path, net_path)
    net = json.load(open_any(net_path))
    own = owned_lines(net)
    cnt = collections.Counter(l for l, _ in own)
    src = collections.Counter(l for l, _ in lines)
    dup = [l for l, n in cnt.items() if n > 1]
    missing = [l for l in src if l not in cnt]
    extra = [l for l in cnt if l not in src]
    print('lines in FASM: %d, owned: %d (cells %d, nets %d, unassigned %d)' % (
        len(lines), len(own), sum(1 for _, o in own if o.startswith('cell:')),
        sum(1 for _, o in own if o.startswith('net:')), sum(1 for _, o in own if o == 'unassigned')))
    print('owned twice: %d, not owned: %d, not in source: %d' % (len(dup), len(missing), len(extra)))
    ok = not dup and not missing and not extra and cnt == src
    out = os.path.join(t, 'x.fasm')
    log('tofasm (merge_and_sort)')
    txt = tofasm(net, out)
    same = txt == open(fasm_path).read()
    print('netlist -> FASM (merge_and_sort) == input: %s' % ('IDENTICAL' if same else 'DIFFERENT'))
    if not (ok and same):
        raise SystemExit(1)
    print('FASM <-> netlist: LOSSLESS')


def split(net, ann, outdir):
    """One JSON file per block path (cells + nets of that block), plus index.json.
    Cells/nets go by the block annotation (blocks.py); FASM lines stay inside
    their cell or net, so nothing is lost by the split."""
    os.makedirs(outdir, exist_ok=True)
    per = collections.defaultdict(lambda: dict(cells=[], nets=[], unassigned=[]))
    for c in net['cells']:
        per[ann['cells'].get(c['name']) or 'unannotated']['cells'].append(c)
    for n in net['nets']:
        per[ann['nets'].get(n['name']) or 'unannotated']['nets'].append(n)
    per['unassigned']['unassigned'] = net['unassigned']
    files = {}
    for b, d in sorted(per.items()):
        fn = re.sub(r'[^A-Za-z0-9_.-]', '__', b) + '.json'
        files[b] = fn
        with open(os.path.join(outdir, fn), 'w') as f:
            json.dump(dict(block=b, **d), f, indent=0, separators=(',', ':'))
    head = {k: v for k, v in net.items() if k not in ('cells', 'nets', 'unassigned')}
    json.dump(dict(header=head, files=files), open(os.path.join(outdir, 'index.json'), 'w'), indent=1)
    return files


def merge(indir):
    idx = json.load(open(os.path.join(indir, 'index.json')))
    cells, nets, unassigned = [], [], []
    for b, fn in sorted(idx['files'].items()):
        d = json.load(open(os.path.join(indir, fn)))
        cells += d['cells']
        nets += d['nets']
        unassigned += d['unassigned']
    out = dict(idx['header'])
    out['cells'] = sorted(cells, key=lambda c: c['name'])
    out['nets'] = sorted(nets, key=lambda n: n['name'])
    out['unassigned'] = sorted(unassigned)
    return out


def main():
    a = sys.argv[1:]
    if len(a) < 2:
        raise SystemExit(__doc__)
    if a[0] == 'build':
        res, _ = build(a[1], a[2])
        for k, v in stats(res).items():
            print('%-45s %s' % (k, v))
    elif a[0] == 'tofasm':
        tofasm(json.load(open_any(a[1])), a[2])
    elif a[0] == 'check':
        check(a[1], a[2] if len(a) > 2 else None)
    elif a[0] == 'split':
        files = split(json.load(open_any(a[1])), json.load(open_any(a[2])), a[3])
        print('wrote %d block files to %s' % (len(files), a[3]))
    elif a[0] == 'merge':
        m = merge(a[1])
        with open_any(a[2], 'wt') as f:
            json.dump(m, f, indent=0, separators=(',', ':'))
        print('merged %d cells, %d nets, %d unassigned' % (len(m['cells']), len(m['nets']), len(m['unassigned'])))
    elif a[0] == 'same':
        x, y = json.load(open_any(a[1])), json.load(open_any(a[2]))
        same = json.dumps(x, sort_keys=True) == json.dumps(y, sort_keys=True)
        print('netlists %s' % ('IDENTICAL' if same else 'DIFFERENT'))
        if not same:
            raise SystemExit(1)
    elif a[0] == 'stats':
        for k, v in stats(json.load(open_any(a[1]))).items():
            print('%-45s %s' % (k, v))
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main()
