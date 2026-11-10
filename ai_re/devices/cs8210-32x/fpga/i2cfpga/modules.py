#!/usr/bin/env python3
"""Layer 6: re-roll the 32 I2C channel engines into one module template plus
32 instances, losslessly.

    modules.py extract NET LOGIC BLOCKS OUTDIR [--name N]   template + instances + remainder
                                                            -> OUTDIR/N.l6/, report OUTDIR/N.modules.txt
    modules.py rebuild L6DIR OUT.net.json                   template + instances + remainder -> netlist

NET/LOGIC are the layer-4 netlist and its logical view, BLOCKS the layer-5
annotation (channel membership and pad pairs).  The rebuilt netlist is
identical to NET (fasmnet.py same); roundtrip6.sh takes it on to the image.
See LAYER6.md for the format and the method.
"""
import collections
import gzip
import json
import os
import re
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, '..', 'work')
sys.path.insert(0, HERE)
import modgraph  # noqa: E402

T0 = time.time()


def log(*a):
    print('[%6.1fs]' % (time.time() - T0), *a, file=sys.stderr, flush=True)


def open_any(path, mode='rt'):
    return gzip.open(path, mode) if path.endswith('.gz') else open(path, mode)


# ====================================================================== matching
class Aligner:
    """Pairwise alignment of module candidates: reference instance R -> instance C.

    Seeded propagation: from matched pairs, neighbours reached through the same
    (port role, neighbour port role, neighbour label) are matched when that
    group has exactly one unmatched member on each side.  When propagation
    stalls, every unmatched frontier node gets a signature (its label and the
    reference ids of its matched neighbours, plus shared 'global' nets); pairs
    with a signature unique on both sides are matched.  Remaining ties (true
    symmetries) are broken in physical order and counted."""

    def __init__(self, G, members, glob):
        self.G = G
        self.members = members      # inst -> set(node)
        self.glob = glob            # nets touched by >= 2 instances (shared anchors)
        self.owner = {}
        for i, s in members.items():
            for x in s:
                self.owner[x] = i
        self.nb = {}

    def nbrs(self, x):
        r = self.nb.get(x)
        if r is None:
            r = self.nb[x] = self.G.neighbours(x)
        return r

    def align(self, R, C, seeds, mR=None, mC=None):
        G = self.G
        mR = self.members[R] if mR is None else mR
        mC = self.members[C] if mC is None else mC
        m, mi = {}, {}
        ties = 0
        relab = set()
        queue = []
        frR, frC = set(), set()

        def add(r, c):
            m[r] = c
            mi[c] = r
            queue.append((r, c))
            frR.discard(r)
            frC.discard(c)
            for _, _, _, y, _, _ in self.nbrs(r):
                if y in mR and y not in m:
                    frR.add(y)
            for _, _, _, y, _, _ in self.nbrs(c):
                if y in mC and y not in mi:
                    frC.add(y)

        for r, c in seeds:
            if r in mR and c in mC and r not in m and c not in mi and G.nodes[r]['label'] == G.nodes[c]['label']:
                add(r, c)
        while True:
            while queue:
                r, c = queue.pop()
                gr = collections.defaultdict(list)
                gc = collections.defaultdict(list)
                for mp, mrole, n, y, q, yrole in self.nbrs(r):
                    if y in mR and y not in m:
                        key = (mrole, yrole, G.nodes[y]['label'])
                        if y not in gr[key]:
                            gr[key].append(y)
                for mp, mrole, n, y, q, yrole in self.nbrs(c):
                    if y in mC and y not in mi:
                        key = (mrole, yrole, G.nodes[y]['label'])
                        if y not in gc[key]:
                            gc[key].append(y)
                for key, ys in gr.items():
                    zs = gc.get(key)
                    if zs and len(ys) == 1 and len(zs) == 1 and ys[0] not in m and zs[0] not in mi:
                        add(ys[0], zs[0])
            new, tb = self.refine(m, mi, mR, mC, frR, frC)
            if new:
                for r, c in new:
                    if r not in m and c not in mi:
                        add(r, c)
                continue
            if tb:
                for r, c in tb:
                    if r not in m and c not in mi:
                        add(r, c)
                        ties += 1
                continue
            # label-free phase: same kind, same matched neighbourhood, other function
            new, _ = self.refine(m, mi, mR, mC, frR, frC, nolabel=True)
            if not new:
                break
            for r, c in new:
                if r not in m and c not in mi:
                    add(r, c)
                    relab.add(r)
        return m, ties, relab

    def sig(self, x, side, m, mi, mset, nolabel=False):
        G = self.G
        items = []
        for mp, mrole, n, y, q, yrole in self.nbrs(x):
            if y in mset:
                if side == 'R' and y in m:
                    items.append((mrole, yrole, 'm', y))
                elif side == 'C' and y in mi:
                    items.append((mrole, yrole, 'm', mi[y]))
            elif n in self.glob:
                items.append((mrole, yrole, 'g', n))
        if not any(it[2] == 'm' for it in items):
            return None
        if nolabel:
            nd = G.nodes[x]
            # LUT input roles depend on the function: compare as plain 'I'
            items = [(('I' if a.startswith('I') else a), ('I' if b.startswith('I') else b), k, v)
                     for a, b, k, v in items]
            return (nd['kind'], tuple(sorted(items)))
        return (G.nodes[x]['label'], tuple(sorted(items)))

    def refine(self, m, mi, mR, mC, frR, frC, nolabel=False):
        """(unique pairs, tie-break pairs: one per equal-size ambiguous group)"""
        sr = collections.defaultdict(list)
        sc = collections.defaultdict(list)
        for y in sorted(frR):
            s = self.sig(y, 'R', m, mi, mR, nolabel)
            if s:
                sr[s].append(y)
        for y in sorted(frC):
            s = self.sig(y, 'C', m, mi, mC, nolabel)
            if s:
                sc[s].append(y)
        out, tb = [], []
        key = lambda x: (modgraph.site_xy(x), x)  # noqa: E731
        for s, ys in sorted(sr.items()):
            zs = sc.get(s)
            if not zs:
                continue
            if len(ys) == 1 and len(zs) == 1:
                out.append((ys[0], zs[0]))
            elif len(ys) == len(zs) and not nolabel:
                tb.append((min(ys, key=key), min(zs, key=key)))
        return out, tb


# ====================================================================== slots / lines
FF_RST = {'FDCE', 'FDRE', 'LDCE'}
RE_SLOT = [
    (re.compile(r'^([A-D])LUT\.'), 'LUT'),
    (re.compile(r'^([A-D])5FF(MUX)?\.'), '5FF'),
    (re.compile(r'^([A-D])FF(MUX)?\.'), 'FF'),
    (re.compile(r'^([A-D])OUTMUX\.'), 'OUTMUX'),
    (re.compile(r'^CARRY4\.([A-D])CY0$'), 'CARRY4'),
    (re.compile(r'^PRECYINIT\.(AX|CIN|C1)$'), 'CARRY4'),
]
CTRL_FEATS = ('CEUSEDMUX', 'SRUSEDMUX', 'FFSYNC', 'LATCH', 'CLKINV')
DEFAULT_FEATS = ('NOCLKINV', 'PRECYINIT.C0', 'ALUT.DI1MUX.BDI1_BMC31', 'BLUT.DI1MUX.DI_CMC31',
                 'CLUT.DI1MUX.DI_DMC31')
DI1_DEFAULT = {'A': ('ALUT.DI1MUX.BDI1_BMC31', 'ALUT.DI1MUX.AI'), 'B': ('BLUT.DI1MUX.DI_CMC31', 'BLUT.DI1MUX.BI'),
               'C': ('CLUT.DI1MUX.DI_DMC31', 'CLUT.DI1MUX.CI')}
PACK = re.compile(r'^([A-D](5?FFMUX|OUTMUX)\.|CARRY4\.[A-D]CY0$)')
SLICE_TYPES = ('SLICEL', 'SLICEM')


def split_line(line):
    """'TILE.SITE.feature...' -> (prefix 'TILE.SITE', feature)"""
    parts = line.split('.', 2)
    if len(parts) < 3:
        return None, line
    return parts[0] + '.' + parts[1], parts[2]


def slot_of(feat):
    if feat in DEFAULT_FEATS:
        return ('DEFAULT', '')
    for rx, kind in RE_SLOT:
        mm = rx.match(feat)
        if mm:
            return (kind, mm.group(1)) if kind != 'CARRY4' else ('CARRY4', '')
    base = feat.split(' ')[0]
    if base in CTRL_FEATS:
        return ('CTRL', '')
    if base in DEFAULT_FEATS:
        return ('DEFAULT', '')
    return ('OTHER', base)


def default_lines(prefix, feats, ctype):
    """SLICE default lines (no configuration bits): NOCLKINV unless CLKINV,
    PRECYINIT.C0 unless another PRECYINIT; in a SLICEM the A/B/C LUT DI1MUX
    defaults unless the LUT uses its own DI input.  Checked on every slice at
    extract time (cells where it does not hold keep their lines raw)."""
    out = []
    if ctype == 'SLICEM':
        for L, (dflt, own) in DI1_DEFAULT.items():
            if own not in feats:
                out.append(prefix + '.' + dflt)
    if 'CLKINV' not in feats:
        out.append(prefix + '.NOCLKINV')
    if not any(f.startswith('PRECYINIT.') for f in feats):
        out.append(prefix + '.PRECYINIT.C0')
    return out


def fmt_init(letter, v):
    """prjxray merge_and_sort form of a LUT INIT value (none if 0)."""
    if v == 0:
        return None
    if v & (v - 1) == 0:
        return '%sLUT.INIT[%d]' % (letter, v.bit_length() - 1)
    hi = v.bit_length() - 1
    return "%sLUT.INIT[%d:0] = %d'b%s" % (letter, hi, hi + 1, format(v, 'b'))


def phys_table(el, pin_tnet, n):
    """Truth table over physical pins A1..An for template LUT element el
    (canonical table over el['inputs']), given {pin: tnet}; None if an input
    is not found on exactly one pin."""
    pos = {}
    for q, t in enumerate(el['inputs']):
        pins = [j for j in range(1, n + 1) if t in (pin_tnet.get(j) or ())]
        if len(pins) != 1:
            return None
        pos[q] = pins[0]
    r = 0
    tab = el['table']
    for a in range(1 << n):
        ca = 0
        for q, j in pos.items():
            if a >> (j - 1) & 1:
                ca |= 1 << q
        if tab >> ca & 1:
            r |= 1 << a
    return r


def gen_slot(kind, letter, els, ctx):
    """Template-derived lines (site-relative features) of one slot.
    els: {role: template element}; ctx: dict(pin_tnet=fn(pin)->tnet,
    const=fn(pin)->'VCC'/'GND'/None).  Returns list or None (cannot derive)."""
    if kind == 'LUT':
        e6, e5 = els.get('O6'), els.get('O5')
        if e5 is None:
            if e6 is None:
                return None
            pt = {j: ctx['pin_tnet'](letter + str(j)) for j in range(1, 7)}
            v = phys_table(e6, pt, 6)
        else:
            if ctx['const'](letter + '6') != 'VCC':
                return None
            pt = {j: ctx['pin_tnet'](letter + str(j)) for j in range(1, 6)}
            lo = phys_table(e5, pt, 5)
            hi = phys_table(e6, pt, 5) if e6 is not None else 0
            v = None if lo is None or hi is None else (hi << 32 | lo)
        if v is None:
            return None
        ln = fmt_init(letter, v)
        return [ln] if ln else []
    if kind in ('FF', '5FF'):
        e = els['FF']
        out = []
        tag = letter + ('5FF' if kind == '5FF' else 'FF')
        if e['init'] == 0:
            out.append(tag + '.ZINI')
        if e['prim'] in FF_RST:
            out.append(tag + '.ZRST')
        return out
    if kind == 'CARRY4':
        e = els['CARRY4']
        return ['PRECYINIT.' + e['precyinit']] if e['precyinit'] in ('AX', 'CIN', 'C1') else []
    if kind == 'CTRL':
        out = set()
        for e in els.values():
            if e['prim'] in ('FDRE', 'FDSE'):
                out.add('FFSYNC')
            if e['prim'].startswith('LD'):
                out.add('LATCH')
            if e['ce'] != 'VCC':
                out.add('CEUSEDMUX')
            if e['sr'] != 'GND':
                out.add('SRUSEDMUX')
            if e['clkinv']:
                out.add('CLKINV')
        return sorted(out)
    if kind == 'SITE':
        return list(els['SITE']['features'])
    return None


# ====================================================================== extraction
def make_inv(bind):
    """logical net -> sorted tuple of template nets bound to it"""
    inv = collections.defaultdict(set)
    for t, ns in bind.items():
        for n in ns:
            inv[n].add(t)
    return {n: tuple(sorted(v)) for n, v in inv.items()}


def resolve_alias(alias, n):
    seen = set()
    while n in alias and n not in seen:
        seen.add(n)
        n = alias[n]
    return n


class PinView:
    """Logical net seen at a physical cell pin (the same function is used at
    extract time on the original netlist and at rebuild time on the rebuilt one)."""

    def __init__(self, cells_pins, netinfo, alias):
        self.cp = cells_pins     # cell -> {pin: physical net}
        self.ni = netinfo        # physical net -> 'VCC'/'GND'/None (const)
        self.alias = alias

    def logical(self, cell, pin):
        n = self.cp.get(cell, {}).get(pin)
        if n is None:
            return None
        c = self.ni.get(n)
        if c:
            return c
        return resolve_alias(self.alias, n)


class Extractor:
    def __init__(self, net, logic, ann, name):
        import xc7bels
        self.name = name
        self.net, self.logic, self.ann = net, logic, ann
        self.cells = {c['name']: c for c in net['cells']}
        log('logical view: recomputing alias map (xc7bels)')
        L = xc7bels.Logic(net)
        L.build()
        self.alias = dict(L.alias)
        modgraph.load_canon_cache()
        self.G = modgraph.Graph(logic)
        modgraph.save_canon_cache()
        G = self.G
        self.netconst = {}
        for n in net['nets']:
            if n['const']:
                self.netconst[n['name']] = 'GND' if n['driver'][0] == '$GND' else 'VCC'
        self.pv = PinView({c['name']: c['pins'] for c in net['cells']}, self.netconst, self.alias)
        # members: layer-5 channel blocks + the pad pair
        self.nch = len(ann['channels'])
        self.mem = collections.defaultdict(set)
        for x, nd in G.nodes.items():
            b = ann['prims'].get(nd['prim'], '')
            if b.startswith('i2c/ch'):
                self.mem[int(b[6:])].add(x)
        for i, (a, b) in enumerate(ann['channels']):
            self.mem[i].add(a)
            self.mem[i].add(b)
        self.owner = {x: i for i, s in self.mem.items() for x in s}
        self.grown = collections.Counter()
        if os.environ.get('L6_GROW', '1') == '1':
            self.grow()
        touch = collections.defaultdict(set)
        for i, s in self.mem.items():
            for x in s:
                for n in G.nodes[x]['ports'].values():
                    if n:
                        touch[n].add(i)
        self.glob = {n for n, s in touch.items() if len(s) >= 2}
        self.A = Aligner(G, self.mem, self.glob)
        self.cache = {}

    def grow(self, rounds=6, K=8):
        """Membership growth into the shared core: an unowned core/* node joins
        channel i when every neighbour it has through a local net (<= K sinks,
        no clock, not a net touched by two channels) belongs to channel i, and at
        least one does.  Added for both families alike; the template threshold
        then keeps only what recurs.  Disable with L6_GROW=0."""
        G, ann = self.G, self.ann
        pool = {x for x, nd in G.nodes.items() if ann['prims'].get(nd['prim'], '').startswith('core/')
                and x not in self.owner}
        clocknets = {n for n, d in G.net_drv.items() if G.nodes[d[0]]['kind'] in ('BUFGCTRL', 'BUFHCE')}
        for _ in range(rounds):
            touch = collections.defaultdict(set)
            for x, i in self.owner.items():
                for n in G.nodes[x]['ports'].values():
                    if n:
                        touch[n].add(i)
            moves = {}
            for y in sorted(pool):
                insts, foreign = set(), False
                for port, n in G.nodes[y]['ports'].items():
                    if n is None or n in ('VCC', 'GND') or n in clocknets:
                        continue
                    if len(G.net_snk.get(n, ())) > K or len(touch.get(n, ())) > 1:
                        continue
                    d = G.net_drv.get(n)
                    peers = [s for s, _ in G.net_snk.get(n, ())] + ([d[0]] if d else [])
                    for z in peers:
                        if z == y:
                            continue
                        o = self.owner.get(z)
                        if o is None:
                            if z not in pool and not G.nodes[z]['kind'] in ('IBUF', 'IOBUF'):
                                foreign = True
                            elif z in pool:
                                foreign = True
                        else:
                            insts.add(o)
                if len(insts) == 1 and not foreign:
                    moves[y] = next(iter(insts))
            if not moves:
                break
            for y, i in moves.items():
                self.mem[i].add(y)
                self.owner[y] = i
                pool.discard(y)
                self.grown[i] += 1
        log('grow: %d core nodes added to channels (per channel %s)' % (
            sum(self.grown.values()), dict(sorted(self.grown.items()))))

    # ------------------------------------------------------------ alignment
    def align(self, R, C):
        key = (R, C)
        if key in self.cache:
            return self.cache[key]
        pr, pc = self.ann['channels'][R], self.ann['channels'][C]
        best = None
        for orient, seeds in ((0, [(pr[0], pc[0]), (pr[1], pc[1])]), (1, [(pr[0], pc[1]), (pr[1], pc[0])])):
            m, ties, relab = self.A.align(R, C, seeds)
            if best is None or len(m) > len(best['m']):
                best = dict(m=m, ties=ties, relab=relab, orient=orient)
        self.cache[key] = best
        return best

    def families(self):
        """Two structural families: start from the two largest layer-5 families,
        assign every channel to the reference it matches best, then take the
        medoid (largest total match with the other members) as reference."""
        fam5 = sorted(self.ann['channel_stats']['families'].values(), key=len, reverse=True)[:2]
        refs = [max(f, key=lambda c: len(self.mem[c])) for f in fam5]
        fam = {}
        for c in range(self.nch):
            cov = [len(self.align(r, c)['m']) / max(1, len(self.mem[c])) if r != c else 2.0 for r in refs]
            fam[c] = 0 if cov[0] >= cov[1] else 1
        groups = [sorted(c for c in fam if fam[c] == k) for k in (0, 1)]
        # family A = the smaller channels
        groups.sort(key=lambda g: sum(len(self.mem[c]) for c in g) / max(1, len(g)))
        self.fam = {}
        self.ref = {}
        for name, g in zip('AB', groups):
            score = {r: sum(len(self.align(r, c)['m']) for c in g if c != r) for r in g}
            self.ref[name] = max(g, key=lambda r: (score[r], -r))
            for c in g:
                self.fam[c] = name
            log('family %s: %d channels, reference ch%02d' % (name, len(g), self.ref[name]))
        return self.fam

    # ------------------------------------------------------------ template
    def bfs_order(self, R, keep):
        G = self.G
        pads = [x for x in self.ann['channels'][R]]
        dist = {p: 0 for p in pads}
        fr = list(pads)
        while fr:
            nf = []
            for x in fr:
                for _, _, _, y, _, _ in self.A.nbrs(x):
                    if y in keep and y not in dist:
                        dist[y] = dist[x] + 1
                        nf.append(y)
            fr = nf
        kind_ord = {'IOBUF': 0, 'FDPE': 1, 'FDCE': 1, 'FDRE': 1, 'FDSE': 1, 'LUT': 2, 'CARRY4': 3, 'MUXF7': 4,
                    'MUXF8': 4, 'RAMB18E1': 5}
        return sorted(keep, key=lambda x: (dist.get(x, 999), kind_ord.get(self.G.nodes[x]['kind'], 9),
                                           modgraph.site_xy(x), x))

    def site_cells(self, x):
        """Physical cells (role -> cell name) of a site element."""
        nd = self.G.nodes[x]
        if nd['kind'] == 'RAMB18E1':
            return {'SITE': nd['prim']}
        if nd['kind'] == 'IOBUF':
            xy = re.search(r'_X(\d+)Y(\d+)$', nd['prim']).group(0)
            out = {'IOB': nd['prim']}
            for k in ('OLOGIC', 'ILOGIC', 'IDELAY'):
                if k + xy in self.cells:
                    out[k] = k + xy
            return out
        return {}

    def site_features(self, cell):
        return sorted(split_line(l)[1] for l in self.cells[cell]['fasm'])

    def template(self, F):
        G = self.G
        R = self.ref[F]
        members = [c for c in range(self.nch) if self.fam[c] == F]
        others = [c for c in members if c != R]
        support = collections.Counter()
        for c in others:
            al = self.align(R, c)
            for x in al['m']:
                support[x] += 1
        need = (len(members) + 1) // 2
        keep = {x for x in self.mem[R] if support[x] + 1 >= need}
        order = self.bfs_order(R, keep)
        eid = {x: '%s%04d' % (F, i) for i, x in enumerate(order)}
        P = G.P
        tnet_of_net = {}    # ref logical net -> tnet
        for x in order:
            nd = G.nodes[x]
            for port, n in nd['ports'].items():
                d = G.net_drv.get(n)
                if n and d and d[0] == x and d[1] == port:
                    tnet_of_net[n] = '%s.%s' % (eid[x], 'O' if nd['kind'] == 'LUT' else port)
        ports = {}

        def tn(n):
            if n is None:
                return None
            if n in ('VCC', 'GND'):
                return n
            t = tnet_of_net.get(n)
            if t:
                return t
            if n not in ports:
                ports[n] = '%sP%03d' % (F, len(ports))
            return ports[n]
        elements = {}
        for x in order:
            nd = G.nodes[x]
            p = P[nd['prim']]
            e = dict(id=eid[x], kind=nd['kind'], label=nd['label'], ref=x)
            if nd['kind'] == 'LUT':
                fn = p['fn'][nd['out']]
                sup, t = fn['support'], fn['table']
                cv, perms = modgraph.canon(t, len(sup))
                perm = perms[0]
                ins = [None] * len(sup)
                for j, pin in enumerate(sup):
                    ins[perm[j]] = tn(p['conns'].get('A%d' % pin))
                e.update(out=nd['out'], k=len(sup), table=cv, inputs=ins)
            elif nd['kind'].startswith(('FD', 'LD')):
                c = p['conns']
                srpin = next(k for k in ('R', 'S', 'CLR', 'PRE') if k in c) if any(k in c for k in ('R', 'S', 'CLR', 'PRE')) else None
                e.update(prim=nd['kind'], init=p['params']['INIT'],
                         clkinv=int(p['params'].get('IS_C_INVERTED', 0)),
                         ce=tn(c.get('CE', 'VCC')), sr=tn(c.get(srpin, 'GND')) if srpin else 'GND', srpin=srpin,
                         ports={k: tn(v) for k, v in sorted(c.items()) if k != 'Q'})
            elif nd['kind'] == 'CARRY4':
                e.update(precyinit=p['params'].get('PRECYINIT', 'C0'),
                         ports={k: tn(v) for k, v in sorted(p['conns'].items()) if not k.startswith(('O[', 'CO['))})
            elif nd['kind'] in ('RAMB18E1', 'IOBUF'):
                feats = {}
                for role, cell in self.site_cells(x).items():
                    feats[role] = self.site_features(cell)
                e.update(site_features=feats, ports={k: tn(v) for k, v in sorted(nd['ports'].items())
                                                     if not (G.net_drv.get(v) == (x, k))})
            else:
                e.update(ports={k: tn(v) for k, v in sorted(nd['ports'].items())
                                if not (G.net_drv.get(v) == (x, k))})
            elements[eid[x]] = e
        # site features: majority over the family (deviations are recorded per instance)
        for x in order:
            e = elements[eid[x]]
            if 'site_features' not in e:
                continue
            for role in list(e['site_features']):
                cnt = collections.Counter()
                for c in members:
                    z = x if c == R else self.align(R, c)['m'].get(x)
                    if z is None:
                        continue
                    cell = self.site_cells(z).get(role)
                    if cell:
                        cnt[tuple(self.site_features(cell))] += 1
                if cnt:
                    e['site_features'][role] = list(cnt.most_common(1)[0][0])
        glob = {pid: n for n, pid in ports.items() if n in self.glob}
        outs = {}
        for n, t in tnet_of_net.items():
            ext = [s for s in G.net_snk.get(n, ()) if s[0] not in keep]
            if ext:
                outs[t] = len(ext)
        T = dict(family=F, reference='ch%02d' % R, members=['ch%02d' % c for c in members],
                 support_threshold=need, elements=elements, order=[eid[x] for x in order],
                 ports={pid: dict(ref_net=n, global_net=n if n in self.glob else None) for n, pid in ports.items()},
                 outputs=outs, ref_eid=eid)
        log('template %s: %d elements (ref ch%02d has %d nodes; threshold %d of %d), %d input ports (%d global), %d output nets' % (
            F, len(elements), R, len(self.mem[R]), need, len(members), len(ports), len(glob), len(outs)))
        return T

    # ------------------------------------------------------------ instances
    def instance(self, c, T):
        G = self.G
        F = T['family']
        R = self.ref[F]
        if c == R:
            m, ties, relab, orient = {x: x for x in self.mem[R]}, 0, set(), 0
        else:
            al = self.align(R, c)
            m, ties, relab, orient = al['m'], al['ties'], al['relab'], al['orient']
        E = T['elements']
        eid = T['ref_eid']
        node_el, dev = {}, []
        for x, e in eid.items():
            z = m.get(x)
            if z is None:
                dev.append(dict(kind='missing', element=e))
            elif G.nodes[z]['label'] != G.nodes[x]['label']:
                dev.append(dict(kind='function', element=e, node=z, label=G.nodes[z]['label'],
                                template_label=G.nodes[x]['label']))
            else:
                node_el[z] = e
        image = set(m.values())
        extras = sorted(z for z in self.mem[c] if z not in node_el and not any(
            d.get('node') == z for d in dev))
        for z in extras:
            dev.append(dict(kind='extra', node=z, label=G.nodes[z]['label'],
                            matched_to_nontemplate=z in image))
        bind = {}
        for z, e in node_el.items():
            nd = G.nodes[z]
            for port, n in nd['ports'].items():
                d = G.net_drv.get(n)
                if n and d and d[0] == z and d[1] == port:
                    bind['%s.%s' % (e, 'O' if nd['kind'] == 'LUT' else port)] = [n]
        conn = collections.defaultdict(list)
        repl = collections.defaultdict(list)

        def chk(e, port, t, actual):
            if t is None:
                return
            if t in ('VCC', 'GND'):
                if actual != t:
                    conn[e].append((port, t, actual))
                return
            if t in bind:
                if actual in bind[t]:
                    return
                if self.equivalent(actual, bind[t][0]):
                    repl[t].append(actual)      # replicated driver (same function, same inputs)
                else:
                    conn[e].append((port, t, actual))
                bind[t].append(actual)
            else:
                if t not in T['ports']:
                    conn[e].append((port, t, actual))   # input from a missing/deviating element
                bind[t] = [actual]
        luts = []
        for z, e in sorted(node_el.items(), key=lambda kv: kv[1]):
            el = E[e]
            nd = G.nodes[z]
            if el['kind'] == 'LUT':
                luts.append((z, e))
                continue
            for port, t in el['ports'].items():
                if el['kind'].startswith(('FD', 'LD')) and port in ('R', 'S', 'CLR', 'PRE', 'CE'):
                    actual = self.G.P[nd['prim']]['conns'].get(port)
                else:
                    actual = nd['ports'].get(port)
                chk(e, port, t, actual)
        # LUT inputs: choose the canonical permutation that agrees with known bindings
        pending = list(luts)
        for rnd in range(3):
            nxt = []
            for z, e in pending:
                el = E[e]
                fn = G.P[G.nodes[z]['prim']]['fn'][G.nodes[z]['out']]
                sup = fn['support']
                cv, perms = modgraph.canon(fn['table'], len(sup))
                acts = [G.nodes[z]['ports'].get('A%d' % pin) for pin in sup]

                def score(p):
                    return sum(1 for j in range(len(sup)) if acts[j] in bind.get(el['inputs'][p[j]], ()))
                best = max(perms, key=score)
                unbound = [el['inputs'][best[j]] for j in range(len(sup)) if el['inputs'][best[j]] not in bind]
                amb = len(perms) > 1 and sum(1 for p in perms if score(p) == score(best)) > 1 and unbound
                if amb and rnd < 2:
                    nxt.append((z, e))
                    continue
                for j in range(len(sup)):
                    chk(e, 'I%d' % best[j], el['inputs'][best[j]], acts[j])
            pending = nxt
        for e, lst in sorted(conn.items()):
            dev.append(dict(kind='connection', element=e, node=next(z for z, ee in node_el.items() if ee == e),
                            inputs=[list(x) for x in lst]))
        for t, lst in sorted(repl.items()):
            dev.append(dict(kind='replica', tnet=t, primary=bind[t][0], replicas=lst))
        return dict(channel=c, family=F, reference=(c == R), orient=orient, ties=ties, relabeled=len(relab),
                    map={e: z for z, e in node_el.items()}, node_el=node_el, bind=bind, inv=make_inv(bind),
                    deviations=dev, conn=dict(conn))

    def equivalent(self, n1, n2, depth=2):
        """Two logical nets driven by structurally equivalent primitives (same
        label, same input nets up to equivalence): a replicated driver."""
        if n1 == n2:
            return True
        if depth < 0 or not n1 or not n2:
            return False
        G = self.G
        d1, d2 = G.net_drv.get(n1), G.net_drv.get(n2)
        if not d1 or not d2 or d1[1] != d2[1]:
            return False
        a, b = G.nodes[d1[0]], G.nodes[d2[0]]
        if a['label'] != b['label'] or a['kind'] != b['kind']:
            return False
        if a['kind'] == 'LUT':
            fa = G.P[a['prim']]['fn'][a['out']]
            fb = G.P[b['prim']]['fn'][b['out']]
            ia = [a['ports']['A%d' % p] for p in fa['support']]
            ib = [b['ports']['A%d' % p] for p in fb['support']]
            # same table under an input order that pairs equivalent nets
            for perm in modgraph._PERMS[len(ia)]:
                if modgraph.permute_table(fa['table'], len(ia), perm) != fb['table']:
                    continue
                if all(self.equivalent(ia[j], ib[perm[j]], depth - 1) for j in range(len(ia))):
                    return True
            return False
        pa = {k: v for k, v in G.P[a['prim']]['conns'].items() if k not in modgraph.OUT_PORTS}
        pb = {k: v for k, v in G.P[b['prim']]['conns'].items() if k not in modgraph.OUT_PORTS}
        if set(pa) != set(pb):
            return False
        return all(self.equivalent(pa[k], pb[k], depth - 1) if pa[k] not in ('VCC', 'GND') else pa[k] == pb[k]
                   for k in pa)

    # ------------------------------------------------------------ slots
    def slice_slots(self, cell):
        """{(kind, letter): {'lines': [feat], 'nodes': {role: node}}} for a SLICE cell."""
        G = self.G
        s = cell['name']
        out = collections.defaultdict(lambda: dict(lines=[], nodes={}))
        for l in cell['fasm']:
            pre, f = split_line(l)
            k = slot_of(f)
            out[k]['lines'].append(f)
        for (kind, L), d in out.items():
            nodes = {}
            if kind == 'LUT':
                for o in ('O6', 'O5'):
                    x = '%s/%sLUT#%s' % (s, L, o)
                    if x in G.nodes:
                        nodes[o] = x
                if '%s/%sLUT' % (s, L) in G.nodes:
                    nodes['RAW'] = '%s/%sLUT' % (s, L)
            elif kind in ('FF', '5FF'):
                x = '%s/%s%s' % (s, L, kind)
                if x in G.nodes:
                    nodes['FF'] = x
            elif kind == 'CARRY4':
                if s + '/CARRY4' in G.nodes:
                    nodes['CARRY4'] = s + '/CARRY4'
            elif kind == 'OUTMUX':
                opt = d['lines'][0].split('.', 1)[1]
                x = {'O6': '%s/%sLUT#O6' % (s, L), 'O5': '%s/%sLUT#O5' % (s, L), 'XOR': s + '/CARRY4',
                     'CY': s + '/CARRY4', L + '5Q': '%s/%s5FF' % (s, L),
                     'F7': '%s/F7%sMUX' % (s, 'A' if L == 'A' else 'B'), 'F8': s + '/F8MUX'}.get(opt)
                if x in G.nodes:
                    nodes['SRC'] = x
            elif kind == 'CTRL':
                for L2 in 'ABCD':
                    for t in ('FF', '5FF'):
                        x = '%s/%s%s' % (s, L2, t)
                        if x in G.nodes:
                            nodes[L2 + t] = x
            d['nodes'] = nodes
        return out

    def run(self):
        self.families()
        self.T = {F: self.template(F) for F in 'AB'}
        # cross-family correspondence: reference B aligned to reference A
        G = self.G
        RA, RB = self.ref['A'], self.ref['B']
        al = self.align(RB, RA)
        eA, eB = self.T['A']['ref_eid'], self.T['B']['ref_eid']
        fm = {}
        for x, y in al['m'].items():
            if x in eB and y in eA and G.nodes[x]['label'] == G.nodes[y]['label']:
                fm[eB[x]] = eA[y]
        self.family_map = dict(sorted(fm.items()))
        log('family map: %d of %d B elements and %d of %d A elements correspond (same label, aligned)' % (
            len(fm), len(eB), len(fm), len(eA)))
        self.inst = {}
        for c in range(self.nch):
            self.inst[c] = self.instance(c, self.T[self.fam[c]])
        node_inst = {}
        for c, I in self.inst.items():
            for z in I['node_el']:
                node_inst[z] = c
        self.node_inst = node_inst
        G = self.G
        # ---- slot ownership and line generation
        self.gen = collections.defaultdict(lambda: collections.defaultdict(lambda: dict(gen=[], pack=[], raw=[])))
        self.remraw = collections.defaultdict(list)
        self.prefix = {}
        self.nodefault = []
        st = collections.Counter()
        elem_slots = collections.defaultdict(list)    # (c, eid) -> [ok?]
        for cell in self.net['cells']:
            if not cell['fasm']:
                continue
            pres = {split_line(l)[0] for l in cell['fasm']}
            name = cell['name']
            if len(pres) == 1 and None not in pres:
                self.prefix[name] = next(iter(pres))
            if cell['type'] in SLICE_TYPES and name in self.prefix:
                pre = self.prefix[name]
                feats = {split_line(l)[1] for l in cell['fasm']}
                dl = set(default_lines(pre, feats, cell['type']))
                have = {l for l in cell['fasm'] if split_line(l)[1] in DEFAULT_FEATS}
                if dl != have:
                    self.nodefault.append(name)
                for (kind, L), d in self.slice_slots(cell).items():
                    full = [pre + '.' + f for f in d['lines']]
                    if kind == 'DEFAULT':
                        if name in self.nodefault:
                            self.remraw[name] += full
                        st['lines default rule'] += 0 if name in self.nodefault else len(full)
                        continue
                    nodes = d['nodes']
                    owners = {self.owner.get(x) for x in nodes.values()}
                    if kind == 'OTHER' or not nodes or len(owners) != 1 or None in owners:
                        self.remraw[name] += full
                        st['lines remainder (not channel or shared slot)'] += len(full)
                        if nodes and any(o is not None for o in owners):
                            st['slots shared between a channel and other logic'] += 1
                        continue
                    c = next(iter(owners))
                    I = self.inst[c]
                    E = self.T[I['family']]['elements']
                    els = {r: I['node_el'].get(x) for r, x in nodes.items() if r != 'RAW'}
                    eids = sorted({e for e in els.values() if e})
                    derived = None
                    if all(els.values()) and 'RAW' not in nodes:
                        ctx = dict(pin_tnet=lambda pin, name=name, I=I: I['inv'].get(self.pv.logical(name, pin)),
                                   const=lambda pin, name=name: self.pv.logical(name, pin))
                        if kind == 'OUTMUX':
                            derived = []
                        else:
                            derived = gen_slot(kind, L, {r: E[e] for r, e in els.items()}, ctx)
                    pack = sorted(f for f in d['lines'] if PACK.match(f))
                    rest = sorted(f for f in d['lines'] if not PACK.match(f))
                    ok = derived is not None and sorted(derived) == rest
                    for e in eids:
                        elem_slots[(c, e)].append(ok)
                    if ok:
                        self.gen[c][name]['gen'].append([kind, L, els])
                        self.gen[c][name]['pack'] += pack
                        st['lines generated from template'] += len(rest)
                        st['lines packing (per-instance)'] += len(pack)
                        st['slots generated'] += 1
                    else:
                        self.gen[c][name]['raw'] += full
                        st['lines instance deviation (raw)'] += len(full)
                        st['slots instance deviation'] += 1
                continue
            # other cells; channel site elements are moved out of the remainder below
            self.remraw[name] += list(cell['fasm'])
        # site elements: move their cells from the remainder to the instance
        for c, I in self.inst.items():
            E = self.T[I['family']]['elements']
            for z, e in I['node_el'].items():
                el = E[e]
                if 'site_features' not in el:
                    continue
                for role, cell in self.site_cells(z).items():
                    if cell not in self.prefix or cell not in self.remraw:
                        continue
                    lines = self.remraw[cell]
                    if sorted(lines) != sorted(self.cells[cell]['fasm']):
                        continue      # already split (should not happen)
                    feats = sorted(split_line(l)[1] for l in lines)
                    ok = feats == sorted(el['site_features'].get(role, []))
                    elem_slots[(c, e)].append(ok)
                    del self.remraw[cell]
                    if ok:
                        self.gen[c][cell]['gen'].append(['SITE', role, {'SITE': e}])
                        st['lines generated from template'] += len(lines)
                        st['slots generated'] += 1
                    else:
                        self.gen[c][cell]['raw'] += list(lines)
                        st['lines instance deviation (raw)'] += len(lines)
                        st['slots instance deviation'] += 1
        for c, I in self.inst.items():
            T = self.T[I['family']]
            pure = sum(1 for e in T['elements'] if e in I['map'] and e not in I['conn']
                       and all(elem_slots.get((c, e), [True])))
            I['pure'] = pure
            I['slot_fail'] = sorted(e for e in T['elements'] if not all(elem_slots.get((c, e), [True])))
        self.stats = st
        # ---- nets
        self.inet = collections.defaultdict(list)
        self.rnets = []
        for n in self.net['nets']:
            lg = None if n['const'] else resolve_alias(self.alias, n['name'])
            d = G.net_drv.get(lg) if lg else None
            c = self.owner.get(d[0]) if d else None
            if c is not None:
                self.inet[c].append(n)
            else:
                self.rnets.append(n)
        log('slots: %s' % dict(st))

    # ------------------------------------------------------------ output
    def write(self, outdir):
        d = os.path.join(outdir, self.name + '.l6')
        os.makedirs(d, exist_ok=True)
        net = self.net
        head = {k: v for k, v in net.items() if k not in ('cells', 'nets', 'unassigned')}
        skel = {c['name']: dict(kind=c['kind'], type=c['type'], tile=c['tile'], implicit=c['implicit'],
                                unknown_pins=c['unknown_pins'], prefix=self.prefix.get(c['name']))
                for c in net['cells']}
        # alias entries needed to see logical nets at the pins of generated LUT slots
        need = set()
        for c, cells in self.gen.items():
            for cell, spec in cells.items():
                for kind, L, els in spec['gen']:
                    if kind == 'LUT':
                        for j in range(1, 7):
                            n = self.cells[cell]['pins'].get('%s%d' % (L, j))
                            if n:
                                need.add(n)
        alias = {}
        for n in sorted(need):
            while n in self.alias and n not in alias:
                alias[n] = self.alias[n]
                n = self.alias[n]
        json.dump(dict(format='i2cfpga-modules-1', header=head, skeleton=skel, alias=alias,
                       nodefault=sorted(self.nodefault)), open(os.path.join(d, 'meta.json'), 'w'), separators=(',', ':'))
        tpl = {}
        for F, T in self.T.items():
            t = {k: v for k, v in T.items() if k != 'ref_eid'}
            tpl[F] = t
        json.dump(dict(format='i2cfpga-modules-1', module='i2c_channel', families=tpl,
                       family_of={'ch%02d' % c: f for c, f in sorted(self.fam.items())},
                       family_map_B_to_A=self.family_map),
                  open(os.path.join(d, 'template.json'), 'w'), indent=0, sort_keys=True)
        for c, I in sorted(self.inst.items()):
            cells = {k: v for k, v in sorted(self.gen[c].items())}
            rec = dict(channel='ch%02d' % c, family=I['family'], reference=I['reference'], pads=self.ann['channels'][c],
                       pad_orientation=I['orient'], ties_broken=I['ties'], relabeled=I['relabeled'],
                       placement=dict(sorted(I['map'].items())), bind=dict(sorted(I['bind'].items())),
                       deviations=I['deviations'], cells=cells, nets=sorted(self.inet[c], key=lambda n: n['name']))
            json.dump(rec, open(os.path.join(d, 'ch%02d.json' % c), 'w'), indent=0, separators=(',', ':'))
        json.dump(dict(cells_raw={k: sorted(v) for k, v in sorted(self.remraw.items()) if v},
                       nets=self.rnets, unassigned=net['unassigned']),
                  open(os.path.join(d, 'remainder.json'), 'w'), indent=0, separators=(',', ':'))
        return d

    def report(self):
        """Per-instance statistics (text) + family comparison."""
        L = []
        st = self.stats
        L.append('# Layer 6: i2c_channel module re-rolled (%s)' % self.name)
        L.append('')
        tot = sum(len(c['fasm']) for c in self.net['cells'])
        L.append('cell FASM lines: %d total' % tot)
        for k in sorted(st):
            L.append('  %-50s %d' % (k, st[k]))
        L.append('  %-50s %d' % ('pip lines (nets: per-instance routing or remainder)',
                                  sum(len(n['fasm']) for n in self.net['nets'])))
        for F in 'AB':
            T = self.T[F]
            kinds = collections.Counter(e['kind'] if not e['kind'].startswith('FD') else 'FF' for e in T['elements'].values())
            L.append('')
            L.append('template %s: reference %s, %d elements %s, %d input ports (%d global), %d output nets, members %s' % (
                F, T['reference'], len(T['elements']), dict(kinds.most_common()), len(T['ports']),
                sum(1 for p in T['ports'].values() if p['global_net']), len(T['outputs']), ' '.join(T['members'])))
        fm = self.family_map
        kinds = lambda F, ids: collections.Counter(  # noqa: E731
            'FF' if self.T[F]['elements'][e]['kind'].startswith('FD') else self.T[F]['elements'][e]['kind'] for e in ids)
        onlyB = [e for e in self.T['B']['elements'] if e not in fm]
        onlyA = [e for e in self.T['A']['elements'] if e not in set(fm.values())]
        L.append('')
        L.append('families: %d elements correspond between templates A and B (reference %s aligned to %s, same '
                 'label): %s' % (len(fm), self.T['B']['reference'], self.T['A']['reference'],
                                 dict(kinds('B', fm).most_common())))
        L.append('  only in B: %d %s' % (len(onlyB), dict(kinds('B', onlyB).most_common())))
        L.append('  only in A: %d %s' % (len(onlyA), dict(kinds('A', onlyA).most_common())))
        L.append('')
        L.append('ch  fam ref  template  present  pure   pure%  missing function conn  extra replica ties relab  lines pack/raw')
        for c, I in sorted(self.inst.items()):
            T = self.T[I['family']]
            dv = collections.Counter(x['kind'] for x in I['deviations'])
            g = p = r = 0
            for cell, spec in self.gen[c].items():
                p += len(spec['pack'])
                r += len(spec['raw'])
                for kind, Lt, els in spec['gen']:
                    if kind == 'SITE':
                        g += len(self.cells[cell]['fasm'])
            lines_gen = sum(1 for cell, spec in self.gen[c].items() for _ in [0]) and None
            n = len(T['elements'])
            L.append('%02d  %s   %s  %8d %8d %5d  %5.1f%% %8d %8d %4d %6d %7d %4d %5d   %d/%d' % (
                c, I['family'], '*' if I['reference'] else ' ', n, len(I['map']), I['pure'], 100.0 * I['pure'] / n,
                dv['missing'], dv['function'], dv['connection'], dv['extra'], dv['replica'], I['ties'], I['relabeled'],
                p, r))
        return '\n'.join(L) + '\n'


# ====================================================================== rebuild
def rebuild(d):
    meta = json.load(open(os.path.join(d, 'meta.json')))
    tpl = json.load(open(os.path.join(d, 'template.json')))
    rem = json.load(open(os.path.join(d, 'remainder.json')))
    insts = [json.load(open(os.path.join(d, f))) for f in sorted(os.listdir(d)) if re.match(r'ch\d\d\.json$', f)]
    nets = list(rem['nets'])
    for I in insts:
        nets += I['nets']
    nets.sort(key=lambda n: n['name'])
    pins = collections.defaultdict(dict)
    const = {}
    for n in nets:
        drv = n['driver']
        if drv and not drv[0].startswith('$'):
            pins[drv[0]][drv[1]] = n['name']
        for c, p in n['sinks']:
            pins[c][p] = n['name']
        if n['const']:
            const[n['name']] = 'GND' if drv[0] == '$GND' else 'VCC'
    pv = PinView(pins, const, meta['alias'])
    lines = collections.defaultdict(list)
    for c, ls in rem['cells_raw'].items():
        lines[c] += ls
    ngen = 0
    for I in insts:
        E = tpl['families'][I['family']]['elements']
        inv = make_inv(I['bind'])
        for cell, spec in I['cells'].items():
            pre = meta['skeleton'][cell]['prefix']
            for kind, L, els in spec['gen']:
                if kind == 'OUTMUX':
                    der = []
                elif kind == 'SITE':
                    der = E[els['SITE']]['site_features'][L]
                else:
                    ctx = dict(pin_tnet=lambda pin, cell=cell, inv=inv: inv.get(pv.logical(cell, pin)),
                               const=lambda pin, cell=cell: pv.logical(cell, pin))
                    der = gen_slot(kind, L, {r: E[e] for r, e in els.items()}, ctx)
                    if der is None:
                        raise SystemExit('cannot regenerate %s %s%s in %s' % (I['channel'], kind, L, cell))
                ngen += len(der)
                lines[cell] += [pre + '.' + f for f in der]
            lines[cell] += [pre + '.' + f for f in spec['pack']]
            lines[cell] += spec['raw']
    nodef = set(meta['nodefault'])
    cells = []
    for name, sk in sorted(meta['skeleton'].items()):
        ls = lines.get(name, [])
        if sk['type'] in SLICE_TYPES and name not in nodef and sk['prefix']:
            feats = {split_line(l)[1] for l in ls}
            ls = ls + default_lines(sk['prefix'], feats, sk['type'])
        cells.append(dict(name=name, kind=sk['kind'], type=sk['type'], tile=sk['tile'], implicit=sk['implicit'],
                          pins=dict(sorted(pins.get(name, {}).items())), unknown_pins=sk['unknown_pins'],
                          fasm=sorted(ls)))
    out = dict(meta['header'])
    out['cells'] = cells
    out['nets'] = nets
    out['unassigned'] = rem['unassigned']
    log('rebuilt %d cells (%d lines regenerated from templates), %d nets' % (len(cells), ngen, len(nets)))
    return out


def main():
    a = sys.argv[1:]
    if not a:
        raise SystemExit(__doc__)
    name = 'update'
    if '--name' in a:
        i = a.index('--name')
        name = a[i + 1]
        del a[i:i + 2]
    if a[0] == 'extract':
        net = json.load(open_any(a[1]))
        logic = json.load(open_any(a[2]))
        ann = json.load(open_any(a[3]))
        X = Extractor(net, logic, ann, name)
        X.run()
        d = X.write(a[4])
        rep = X.report()
        open(os.path.join(a[4], name + '.modules.txt'), 'w').write(rep)
        print(rep)
        import pickle
        pickle.dump(dict(fam=X.fam, ref=X.ref, T=X.T, inst={c: {k: v for k, v in I.items()} for c, I in X.inst.items()},
                         align={k: v for k, v in X.cache.items()}),
                    open(os.path.join(a[4], name + '.modules.pickle'), 'wb'))
        log('wrote', d)
    elif a[0] == 'rebuild':
        out = rebuild(a[1])
        with open_any(a[2], 'wt') as f:
            json.dump(out, f, indent=0, separators=(',', ':'))
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main()
