#!/usr/bin/env python3
"""Layer 7: readable view of the i2c_channel module template (layer 6).

    channelrtl.py analyse L6DIR LOGIC.json OUTDIR [--family B] [--blocks BLOCKS.json] [--tables FSM.txt]

Reads the layer-6 template (template.json) and, for context outside the module
(what drives the ports), the layer-4b logical view.  Writes:

  i2c_channel_<F>.v      pseudo-RTL: ports, registers, always-block style
                         descriptions; every element annotated with its template
                         element ids; every name marked V (structurally
                         verified) or I (inferred)
  i2c_channel_<F>.json   the analysis (names, structures, FSM tables)

This is a view: it is not used to rebuild anything (layer 6 carries the bytes).
"""
import collections
import gzip
import itertools
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import modgraph  # noqa: E402


def open_any(path, mode='rt'):
    return gzip.open(path, mode) if path.endswith('.gz') else open(path, mode)


# ====================================================================== boolean helpers
def qm_sop(table, k):
    """Minimal-ish sum of products (Quine-McCluskey + greedy cover) of a k-input
    truth table.  Returns [ {var: 0/1} ] (cubes)."""
    ones = [a for a in range(1 << k) if table >> a & 1]
    if not ones:
        return []
    if len(ones) == 1 << k:
        return [{}]
    # implicants as (value, mask) with mask bits = don't care
    cur = {(a, 0) for a in ones}
    primes = set()
    while cur:
        nxt = set()
        used = set()
        lst = sorted(cur)
        bym = collections.defaultdict(list)
        for v, m in lst:
            bym[m].append(v)
        for m, vs in bym.items():
            s = set(vs)
            for v in vs:
                for i in range(k):
                    b = 1 << i
                    if m & b or v & b:
                        continue
                    if v | b in s:
                        nxt.add((v, m | b))
                        used.add((v, m))
                        used.add((v | b, m))
        primes |= cur - used
        cur = nxt
    primes = sorted(primes, key=lambda p: -bin(p[1]).count('1'))
    # greedy cover
    left = set(ones)
    cover = []
    while left:
        best = max(primes, key=lambda p: (sum(1 for a in left if (a & ~p[1]) == p[0]), bin(p[1]).count('1')))
        cover.append(best)
        left -= {a for a in left if (a & ~best[1]) == best[0]}
    return [{i: (v >> i) & 1 for i in range(k) if not m >> i & 1} for v, m in cover]


def expr(table, names):
    """Readable expression of a truth table over named inputs."""
    k = len(names)
    if k == 0:
        return "1'b%d" % (table & 1)
    full = (1 << (1 << k)) - 1
    if table == 0:
        return "1'b0"
    if table == full:
        return "1'b1"
    # xor / xnor of all inputs
    par = sum(1 << a for a in range(1 << k) if bin(a).count('1') & 1)
    if table == par and k >= 2:
        return ' ^ '.join(names)
    if table == full ^ par and k >= 2:
        return '~(' + ' ^ '.join(names) + ')'
    # 2:1 mux on any select input
    if k == 3:
        for s in range(3):
            a, b = [i for i in range(3) if i != s]
            for x, y in ((a, b), (b, a)):
                t = 0
                for v in range(8):
                    if (v >> x & 1) if (v >> s & 1) else (v >> y & 1):
                        t |= 1 << v
                if t == table:
                    return '%s ? %s : %s' % (names[s], names[x], names[y])
    cubes = qm_sop(table, k)
    pos = None
    if len(cubes) > 2:
        inv = qm_sop(full ^ table, k)
        if len(inv) == 1:
            pos = '~(' + ' & '.join(('' if v else '~') + names[i] for i, v in sorted(inv[0].items())) + ')'
    terms = []
    for c in cubes:
        lits = [('' if v else '~') + names[i] for i, v in sorted(c.items())]
        terms.append(' & '.join(lits) if lits else "1'b1")
    sop = ' | '.join('(%s)' % t if len(cubes) > 1 and ' & ' in t else t for t in terms)
    if pos and len(pos) < len(sop):
        return pos
    return sop


def eval_table(table, vals):
    a = 0
    for i, v in enumerate(vals):
        if v:
            a |= 1 << i
    return table >> a & 1


# ====================================================================== BDD
class BDD:
    """Small reduced ordered BDD (enough for decode cones and FSM next-state
    functions).  Node ids: 0 = false, 1 = true; others index self.nodes."""

    def __init__(self):
        self.nodes = [None, None]          # id -> (var, lo, hi)
        self.uniq = {}
        self.cache = {}
        self.vars = []                     # var index -> name
        self.vidx = {}

    def var(self, name):
        if name not in self.vidx:
            self.vidx[name] = len(self.vars)
            self.vars.append(name)
        return self.mk(self.vidx[name], 0, 1)

    def mk(self, v, lo, hi):
        if lo == hi:
            return lo
        k = (v, lo, hi)
        r = self.uniq.get(k)
        if r is None:
            r = len(self.nodes)
            self.nodes.append(k)
            self.uniq[k] = r
        return r

    def top(self, u):
        return self.nodes[u][0] if u > 1 else 1 << 30

    def ite(self, f, g, h):
        if f == 1:
            return g
        if f == 0:
            return h
        if g == h:
            return g
        if g == 1 and h == 0:
            return f
        k = ('ite', f, g, h)
        r = self.cache.get(k)
        if r is not None:
            return r
        v = min(self.top(f), self.top(g), self.top(h))

        def cof(u, b):
            if u <= 1 or self.nodes[u][0] != v:
                return u
            return self.nodes[u][2] if b else self.nodes[u][1]
        r = self.mk(v, self.ite(cof(f, 0), cof(g, 0), cof(h, 0)), self.ite(cof(f, 1), cof(g, 1), cof(h, 1)))
        self.cache[k] = r
        return r

    def NOT(self, f):
        return self.ite(f, 0, 1)

    def AND(self, f, g):
        return self.ite(f, g, 0)

    def OR(self, f, g):
        return self.ite(f, 1, g)

    def XOR(self, f, g):
        return self.ite(f, self.NOT(g), g)

    def table(self, tab, ins):
        """function given by a truth table over input BDDs"""
        k = len(ins)

        def rec(j, base):
            if j < 0:
                return 1 if tab >> base & 1 else 0
            lo = rec(j - 1, base)
            hi = rec(j - 1, base | 1 << j)
            return self.ite(ins[j], hi, lo)
        return rec(k - 1, 0)

    def exists(self, f, names):
        vs = {self.vidx[n] for n in names if n in self.vidx}
        memo = {}

        def rec(u):
            if u <= 1:
                return u
            if u in memo:
                return memo[u]
            v, lo, hi = self.nodes[u]
            a, b = rec(lo), rec(hi)
            r = self.OR(a, b) if v in vs else self.mk(v, a, b)
            memo[u] = r
            return r
        return rec(f)

    def restrict(self, f, assign):
        """assign: {var name: 0/1}"""
        av = {self.vidx[n]: b for n, b in assign.items() if n in self.vidx}
        memo = {}

        def rec(u):
            if u <= 1:
                return u
            if u in memo:
                return memo[u]
            v, lo, hi = self.nodes[u]
            if v in av:
                r = rec(hi if av[v] else lo)
            else:
                r = self.ite(self.mk(v, 0, 1), rec(hi), rec(lo))
            memo[u] = r
            return r
        return rec(f)

    def support(self, f):
        out, seen, st = set(), set(), [f]
        while st:
            u = st.pop()
            if u <= 1 or u in seen:
                continue
            seen.add(u)
            v, lo, hi = self.nodes[u]
            out.add(self.vars[v])
            st += [lo, hi]
        return out

    def cubes(self, f, limit=64):
        """disjoint cubes of f: [{name: 0/1}]"""
        out = []

        def rec(u, path):
            if len(out) >= limit or u == 0:
                return
            if u == 1:
                out.append(dict(path))
                return
            v, lo, hi = self.nodes[u]
            path[self.vars[v]] = 0
            rec(lo, path)
            path[self.vars[v]] = 1
            rec(hi, path)
            del path[self.vars[v]]
        rec(f, {})
        return out

    def count(self, f, names):
        """number of satisfying assignments over the given variable names"""
        idx = sorted(self.vidx[n] for n in names if n in self.vidx)
        pos = {v: i for i, v in enumerate(idx)}
        memo = {}
        n = len(idx)

        def rec(u):
            if u == 0:
                return 0, n
            if u == 1:
                return 1, n
            if u in memo:
                return memo[u]
            v, lo, hi = self.nodes[u]
            i = pos[v]
            cl, ll = rec(lo)
            ch, lh = rec(hi)
            r = (cl * (1 << (ll - i - 1)) + ch * (1 << (lh - i - 1)), i)
            memo[u] = r
            return r
        c, l = rec(f)
        return c * (1 << l)


def cone_bdd(B, G, net, leaf, memo=None, depth=0):
    """BDD of a logical net of the full design: combinational primitives are
    composed, leaf(net) returns a variable name for registered / port nets."""
    if memo is None:
        memo = {}
    if net in memo:
        return memo[net]
    if net == 'VCC':
        return 1
    if net == 'GND':
        return 0
    name = leaf(net)
    if name is not None:
        r = B.var(name)
        memo[net] = r
        return r
    d = G.net_drv.get(net)
    nd = G.nodes[d[0]] if d else None
    if nd is None or depth > 60:
        r = B.var('?' + net)
    elif nd['kind'] == 'LUT':
        fn = G.P[nd['prim']]['fn'][nd['out']]
        ins = [cone_bdd(B, G, nd['ports']['A%d' % p], leaf, memo, depth + 1) for p in fn['support']]
        r = B.table(fn['table'], ins)
    elif nd['kind'] == 'CARRY4':
        pc = G.P[nd['prim']]['conns']
        c = cone_bdd(B, G, pc.get('CI', 'GND'), leaf, memo, depth + 1) if pc.get('CI', 'GND') != 'GND' else \
            cone_bdd(B, G, pc.get('CYINIT', 'GND'), leaf, memo, depth + 1)
        r = None
        for i in range(4):
            si = cone_bdd(B, G, pc.get('S[%d]' % i, 'GND'), leaf, memo, depth + 1)
            if d[1] == 'O[%d]' % i:
                r = B.XOR(si, c)
                break
            di = cone_bdd(B, G, pc.get('DI[%d]' % i, 'GND'), leaf, memo, depth + 1)
            c = B.ite(si, c, di)
            if d[1] == 'CO[%d]' % i:
                r = c
                break
    elif nd['kind'] in ('MUXF7', 'MUXF8'):
        pc = G.P[nd['prim']]['conns']
        r = B.ite(cone_bdd(B, G, pc['S'], leaf, memo, depth + 1), cone_bdd(B, G, pc['I1'], leaf, memo, depth + 1),
                  cone_bdd(B, G, pc['I0'], leaf, memo, depth + 1))
    else:
        r = B.var('?' + net)
    memo[net] = r
    return r


# ====================================================================== template view
class Chan:
    def __init__(self, l6dir, logic, F):
        tpl = json.load(open(os.path.join(l6dir, 'template.json')))
        self.F = F
        self.T = tpl['families'][F]
        self.E = self.T['elements']
        self.logic = logic
        modgraph.load_canon_cache()
        self.G = modgraph.Graph(logic)
        self.drv = {}                       # tnet -> (eid, port)
        self.snk = collections.defaultdict(list)
        for eid, e in self.E.items():
            for p, x in self.inputs(e):
                if x:
                    self.snk[x].append((eid, p))
        for x in list(self.snk) + [eid + '.O' for eid, e in self.E.items() if e['kind'] == 'LUT']:
            if '.' in x and x.split('.')[0] in self.E:
                self.drv[x] = (x.split('.')[0], x.split('.', 1)[1])
        self.names = {}                     # tnet -> (name, conf, why)
        self.pcie = next(p for p in logic['prims'] if p['prim'] == 'PCIE_2_1')
        self.pnet = collections.defaultdict(list)
        for k, v in self.pcie['conns'].items():
            self.pnet[v].append(k)

    @staticmethod
    def inputs(e):
        if e['kind'] == 'LUT':
            return [('I%d' % q, x) for q, x in enumerate(e['inputs'])]
        return list(e.get('ports', {}).items())

    def fanout(self, t):
        return self.snk.get(t, [])

    def name(self, t):
        if t in ('VCC', 'GND'):
            return "1'b1" if t == 'VCC' else "1'b0"
        n = self.names.get(t)
        return n[0] if n else t.replace('.', '_').replace('[', '').replace(']', '')

    def setname(self, t, name, conf, why):
        if t not in self.names:
            self.names[t] = (name, conf, why)

    def lut_expr(self, eid):
        e = self.E[eid]
        return expr(e['table'], [self.name(x) for x in e['inputs']])

    # ---------------------------------------------------------------- context
    def back(self, n, stages=3, lim=4000):
        """PCIE_2_1 pins reachable backwards from logical net n, with the number
        of FF stages crossed: Counter{(pin, stages)}."""
        G = self.G
        out = collections.Counter()
        seen = set()
        fr = [(n, 0)]
        while fr and len(seen) < lim:
            m, s = fr.pop()
            if (m, s) in seen or m in ('VCC', 'GND') or m is None:
                continue
            seen.add((m, s))
            if m in self.pnet:
                for k in self.pnet[m]:
                    out[(k, s)] += 1
                continue
            d = G.net_drv.get(m)
            if not d:
                continue
            nd = G.nodes[d[0]]
            if nd['kind'].startswith('FD'):
                if s < stages:
                    fr.append((nd['ports'].get('D'), s + 1))
            elif nd['kind'] in ('LUT', 'CARRY4', 'MUXF7', 'MUXF8'):
                for p, x in nd['ports'].items():
                    if G.net_drv.get(x) != (d[0], p):
                        fr.append((x, s))
        return out

    def classify_ports(self):
        """Write-data bits: a port whose cone reaches TRNRD[i] and TRNRD[i+32]
        after exactly two register stages (the 2:1 dword mux of the 64-bit RX
        beat).  TRN bit i carries host byte (3 - i//8), bit i%8 (the TRN
        interface is big-endian within a dword); so host bit = 8*(3-i//8)+i%8."""
        self.wd = {}
        self.portcls = {}
        for pid, p in self.T['ports'].items():
            n = p['ref_net']
            b = self.back(n)
            s2 = sorted(int(k[5:]) for (k, s) in b if s == 2 and k.startswith('TRNRD') and k[5:].isdigit())
            s3 = sorted(int(k[5:]) for (k, s) in b if s == 3 and k.startswith('TRNRD') and k[5:].isdigit())
            d = self.G.net_drv.get(n)
            kind = self.G.nodes[d[0]]['kind'] if d else None
            if kind == 'BUFHCE':
                self.portcls[pid] = 'clk'
                self.setname(pid, 'clk', 'V', 'BUFHCE output (user clock, 62.5 MHz)')
            elif len(s2) == 2 and s2[1] == s2[0] + 32:
                i = s2[0]
                hb = 8 * (3 - i // 8) + i % 8
                self.wd[pid] = hb
                self.portcls[pid] = 'wdata'
                self.setname(pid, 'wdata[%d]' % hb, 'V', 'mux of TRNRD[%d]/TRNRD[%d] two register stages back; '
                             'host bit %d by the TRN dword byte order (I for the byte order)' % (i, i + 32, hb))
            elif s3:
                self.portcls[pid] = 'decode'
            elif any(k == 'USERRSTN' for k, s in b):
                self.portcls[pid] = 'reset'
                self.setname(pid, 'user_rst', 'V', 'from PCIE_2_1 USERRSTN')
            else:
                self.portcls[pid] = 'core'
        return self.wd

    # ---------------------------------------------------------------- dump
    def describe(self, eid):
        e = self.E[eid]
        k = e['kind']
        if k == 'LUT':
            return '%s = %s' % (self.name(eid + '.O'), self.lut_expr(eid))
        if k.startswith('FD'):
            return '%s <= %s  [%s init %d ce %s %s %s]' % (
                self.name(eid + '.Q'), self.name(e['ports'].get('D')), k, e['init'], self.name(e['ce']),
                e['srpin'], self.name(e['sr']))
        return '%s %s %s' % (eid, k, {p: self.name(x) for p, x in self.inputs(e)})

    def dump(self, eids, depth=1):
        seen = set()
        fr = list(eids)
        for _ in range(depth + 1):
            nf = []
            for x in fr:
                if x in seen or x not in self.E:
                    continue
                seen.add(x)
                print('  %-6s %s' % (x, self.describe(x)))
                for p, t in self.inputs(self.E[x]):
                    if t in self.drv:
                        nf.append(self.drv[t][0])
            fr = nf


def load(l6dir, logic_path, F):
    logic = json.load(open_any(logic_path))
    C = Chan(l6dir, logic, F)
    C.classify_ports()
    return C


# ====================================================================== simulation
class Sim:
    """Cycle-based simulation of the template netlist (one clock domain).
    FDCE/FDPE with asynchronous clear/preset, LUTs, CARRY4, MUXF7/8, RAMB18E1
    in SDP mode (behavioural: 256 x 32 + parity, read-first), IOBUF as an
    open-drain pad with a pull-up (pad = T ? 1 : 0, optionally pulled low by a
    model of the bus device).  Ports are driven by the caller."""

    def __init__(self, C):
        self.C = C
        E = C.E
        self.val = collections.defaultdict(int)
        self.ffs = [e for e in E.values() if e['kind'].startswith(('FD', 'LD'))]
        comb = [e for e in E.values() if e['kind'] in ('LUT', 'CARRY4', 'MUXF7', 'MUXF8')]
        # topological order of combinational elements
        deps = {}
        for e in comb:
            deps[e['id']] = {C.drv[x][0] for p, x in C.inputs(e) if x in C.drv and
                             E[C.drv[x][0]]['kind'] in ('LUT', 'CARRY4', 'MUXF7', 'MUXF8')}
        order, done, temp = [], set(), set()

        def visit(x):
            if x in done:
                return
            if x in temp:
                raise SystemExit('combinational loop at %s' % x)
            temp.add(x)
            for y in sorted(deps[x]):
                visit(y)
            temp.discard(x)
            done.add(x)
            order.append(x)
        for e in sorted(deps):
            visit(e)
        self.order = [E[x] for x in order]
        self.pads = [e for e in E.values() if e['kind'] == 'IOBUF']
        self.brams = [e for e in E.values() if e['kind'] == 'RAMB18E1']
        self.mem = {b['id']: collections.defaultdict(int) for b in self.brams}
        self.pulldown = {}      # pad eid -> bool (bus device drives low)
        for f in self.ffs:
            self.val[f['id'] + '.Q'] = f['init']
        self.ports = {}
        self.force = {}

    def v(self, t):
        if t == 'VCC':
            return 1
        if t == 'GND' or t is None:
            return 0
        if t in self.force:
            return self.force[t]
        if t in self.ports:
            return self.ports[t]
        a = getattr(self.C, 'alias', {}).get(t)
        if a is not None:
            return self.val[a[0]] ^ a[1]
        return self.val[t]

    def comb(self):
        V = self.val
        for p in self.pads:
            T = self.v(p['ports'].get('T'))
            V[p['id'] + '.O'] = 0 if (not T or self.pulldown.get(p['id'])) else 1
        for b in self.brams:
            pass
        for e in self.order:
            k = e['kind']
            if k == 'LUT':
                a = 0
                for q, x in enumerate(e['inputs']):
                    if self.v(x):
                        a |= 1 << q
                V[e['id'] + '.O'] = e['table'] >> a & 1
            elif k == 'CARRY4':
                P = e['ports']
                c = self.v(P.get('CI')) if P.get('CI') not in (None, 'GND') else self.v(P.get('CYINIT'))
                for i in range(4):
                    s = self.v(P.get('S[%d]' % i))
                    V[e['id'] + '.O[%d]' % i] = s ^ c
                    c = c if s else self.v(P.get('DI[%d]' % i))
                    V[e['id'] + '.CO[%d]' % i] = c
            else:
                P = e['ports']
                V[e['id'] + '.O'] = self.v(P['I1']) if self.v(P['S']) else self.v(P['I0'])

    def bram_ports(self, b, prefix, n):
        return sum(self.v(b['ports'].get('%s%d' % (prefix, i))) << i for i in range(n))

    def clock(self):
        """one rising edge (after comb has settled)"""
        new = {}
        for f in self.ffs:
            P = f['ports']
            q = f['id'] + '.Q'
            sr = self.v(f['sr']) if f['sr'] not in ('GND',) else 0
            if f['prim'] in ('FDCE', 'FDPE'):
                if sr:
                    new[q] = 1 if f['prim'] == 'FDPE' else 0
                    continue
            elif sr:                                   # FDRE / FDSE synchronous
                new[q] = 1 if f['prim'] == 'FDSE' else 0
                continue
            if self.v(f['ce']):
                new[q] = self.v(P.get('D'))
        for b in self.brams:
            P = b['ports']
            m = self.mem[b['id']]
            # SDP 36: write port B (WEBWE[3:0] byte enables), read port A
            if self.v(P.get('ENBWREN')):
                wa = sum(self.v(P.get('ADDRBWRADDR%d' % i)) << (i - 5) for i in range(5, 14))
                data = self.bram_ports(b, 'DIADI', 16) | self.bram_ports(b, 'DIBDI', 16) << 16
                old = m[wa]
                for j in range(4):
                    if self.v(P.get('WEBWE%d' % (2 * j))) or self.v(P.get('WEBWE%d' % j)):
                        old = (old & ~(0xff << 8 * j)) | (data & 0xff << 8 * j)
                m[wa] = old
            if self.v(P.get('ENARDEN')):
                ra = sum(self.v(P.get('ADDRARDADDR%d' % i)) << (i - 5) for i in range(5, 14))
                d = m[ra]
                for i in range(32):
                    new[b['id'] + '.DO%d' % i] = d >> i & 1
        self.val.update(new)

    def step(self, n=1):
        for _ in range(n):
            self.comb()
            self.clock()
        self.comb()


def port_aliases(C):
    """Ports whose reference net is a LUT1 buffer/inverter chain of a template
    net (route-through LUTs that fell below the template threshold): port ->
    (tnet, inverted).  Structural (V) for the reference instance."""
    G = C.G
    refnet = {}
    for eid, e in C.E.items():
        nd = G.nodes[e['ref']]
        for port, n in nd['ports'].items():
            d = G.net_drv.get(n)
            if n and d and d[0] == e['ref'] and d[1] == port:
                refnet[n] = '%s.%s' % (eid, 'O' if e['kind'] == 'LUT' else port)
    out = {}
    for pid, p in C.T['ports'].items():
        n, inv = p['ref_net'], 0
        for _ in range(6):
            if n in refnet:
                out[pid] = (refnet[n], inv)
                break
            d = G.net_drv.get(n)
            if not d or G.nodes[d[0]]['kind'] != 'LUT':
                break
            nd = G.nodes[d[0]]
            fn = G.P[nd['prim']]['fn'][nd['out']]
            if len(fn['support']) != 1 or fn['table'] not in (1, 2):
                break
            inv ^= fn['table'] == 1
            n = nd['ports']['A%d' % fn['support'][0]]
    C.alias = out
    return out


# ====================================================================== analysis passes
KNOWN_REGS = {0x00: 'profile (clock codes: byte k = profile k+1)', 0x08: 'profile select',
              0x0c: 'master reset / control', 0x50: 'command (read bit 28, address 15:8, length 7:0)',
              0x54: 'start (bit 31) / register offset 7:0', 0x60: 'status (bit 1 = NACK error)'}


class Analysis:
    def __init__(self, C):
        self.C = C
        self.G = C.G
        self.notes = []
        self.amap = {}

    # ------------------------------------------------------------ address decode
    def addr_leaf(self, net):
        G, C = self.G, self.C
        if net in self.amap:
            return self.amap[net]
        d = G.net_drv.get(net)
        r = None
        if d is None:
            r = 'x:' + net
        elif G.nodes[d[0]]['kind'] in ('LUT', 'CARRY4', 'MUXF7', 'MUXF8'):
            r = None
        else:
            r = 's:' + net
            if G.nodes[d[0]]['kind'].startswith('FD'):
                # address register bit k: the first TRN bits reached are TRNRD[k] and
                # TRNRD[k+32] (3DW / 4DW header), at the same stage (2 in the flash
                # builds, 3 in the vendor v01.02.0E file)
                b = C.back(G.nodes[d[0]]['ports'].get('D'), stages=3)
                trn = collections.defaultdict(set)
                for (k, s) in b:
                    if k.startswith('TRNRD') and k[5:].isdigit():
                        trn[s].add(int(k[5:]))
                first = min(trn) if trn else None
                t2 = sorted(trn[first]) if first is not None else []
                if first and first >= 2 and len(t2) == 2 and t2[1] == t2[0] + 32 and 2 <= t2[0] <= 19:
                    r = 'a%02d' % t2[0]
                else:
                    # a second address register (r[k] <= ... a[k] ... | hold): its
                    # combinational next-state cone reads exactly one address bit
                    self.amap[net] = r       # break cycles
                    ks = set()
                    for x in self.comb_leaves(G.nodes[d[0]]['ports'].get('D')):
                        lx = self.addr_leaf(x)
                        if lx and lx.startswith('a'):
                            ks.add(lx)
                    if len(ks) == 1:
                        r = ks.pop()
        self.amap[net] = r
        return r

    def addr_sensitive(self, f):
        """Addresses for which f can be 1 in a state where f still depends on
        the address: g(a) = exists s: f(a, s) & ~(forall a': f(a', s)).  Other
        leaves (address-mux select, write strobes, byte enables, overrides) are
        quantified; assignments that make f true for every address are excluded."""
        B = self.B
        sup = B.support(f)
        addr = [v for v in sup if v.startswith('a')]
        rest = [v for v in sup if not v.startswith('a')]
        if not addr:
            return f if f in (0, 1) else None
        allf = B.NOT(B.exists(B.NOT(f), addr))      # forall a: f
        g = B.AND(f, B.NOT(allf))
        return B.exists(g, rest) if rest else g

    def comb_leaves(self, net, lim=64):
        G = self.G
        out, seen, st = set(), set(), [net]
        while st and len(seen) < lim:
            m = st.pop()
            if m in seen or m in ('VCC', 'GND') or m is None:
                continue
            seen.add(m)
            d = G.net_drv.get(m)
            if d and G.nodes[d[0]]['kind'] in ('LUT', 'CARRY4', 'MUXF7', 'MUXF8'):
                for p, x in G.nodes[d[0]]['ports'].items():
                    if G.net_drv.get(x) != (d[0], p):
                        st.append(x)
            else:
                out.add(m)
        return out

    def decode_ports(self, ctrl_in=None):
        """Address function of every port with the decode-mux controls at 0
        (the controls are the non-address leaves of the page-hit cone).
        Classifies: page hit, page & a[k], window hits."""
        C, G = self.C, self.G
        self.B = B = BDD()
        for k in range(2, 20):
            B.var('a%02d' % k)
        memo = {}
        fn = {}
        for pid, p in C.T['ports'].items():
            if C.portcls.get(pid) != 'decode':
                continue
            fn[pid] = cone_bdd(B, G, p['ref_net'], self.addr_leaf, memo)
        # page port: address support exactly a[19:8]
        page = None
        for pid, f in sorted(fn.items()):
            sup = B.support(f)
            if {v for v in sup if v.startswith('a')} == {'a%02d' % k for k in range(8, 20)}:
                g = self.addr_sensitive(f)
                cs = B.cubes(g, 4) if g is not None else []
                if len(cs) == 1 and len(cs[0]) == 12:
                    page = pid
                    break
        self.page_port = page
        if ctrl_in is not None:
            ctrl = set(ctrl_in)
        elif page:
            ctrl = {v for v in B.support(fn[page]) if not v.startswith('a')}
        else:
            # shared address-mux controls: non-address leaves present in at least
            # half of the decode cones that read address bits
            cnt = collections.Counter()
            n = 0
            for f in fn.values():
                sup = B.support(f)
                if any(v.startswith('a') for v in sup):
                    n += 1
                    for v in sup:
                        if not v.startswith('a'):
                            cnt[v] += 1
            ctrl = {v for v, c in cnt.items() if c * 2 >= n}
        self.ctrl = ctrl
        self.port_addr = {}
        for pid, f in fn.items():
            self.port_addr[pid] = self.addr_sensitive(f)
        self.page_base = None
        if page:
            c = B.cubes(self.port_addr[page], 2)[0]
            self.page_base = sum(1 << k for k in range(8, 20) if c['a%02d' % k])
        else:
            pages = collections.Counter()
            for pid, g in self.port_addr.items():
                if g is None:
                    continue
                cs = B.cubes(g, 2)
                if len(cs) == 1 and all('a%02d' % k in cs[0] for k in range(8, 20)):
                    pages[sum(1 << k for k in range(8, 20) if cs[0]['a%02d' % k])] += 1
            if pages:
                self.page_base = pages.most_common(1)[0][0]
        meaning = {}
        pb = self.page_base or 0
        for pid, g in self.port_addr.items():
            if g is None:
                meaning[pid] = ('qual', None)
                continue
            if pid == page:
                meaning[pid] = ('page', None)
                continue
            cs = B.cubes(g, 8)
            if not cs:
                meaning[pid] = ('zero', None)
            elif len(cs) == 1 and not cs[0]:
                meaning[pid] = ('one', None)
            elif self.page_base is not None and len(cs) == 1 and all(
                    cs[0].get('a%02d' % k) == ((pb >> k) & 1) for k in range(8, 20)):
                extra = {int(v[1:]): b for v, b in cs[0].items() if int(v[1:]) < 8}
                if len(extra) == 1 and list(extra.values())[0] == 1:
                    meaning[pid] = ('page_a', list(extra)[0])
                elif len(extra) == 6:
                    meaning[pid] = ('strobe', sum(1 << k for k, b in extra.items() if b))
                else:
                    meaning[pid] = ('page_cube', {('a%02d' % k): b for k, b in extra.items()})
            else:
                vals = set()
                for c in cs:
                    vals.add(sum(1 << k for k in range(9, 20) if c.get('a%02d' % k)))
                meaning[pid] = ('window', sorted(vals))
        self.meaning = meaning
        # value of every decode port at each register address of this page
        self.port_at = {}
        for pid, g in self.port_addr.items():
            if g is None or self.page_base is None:
                continue
            vals = []
            for off in range(0, 0x100, 4):
                a = self.page_base + off
                r = B.restrict(g, {'a%02d' % k: (a >> k) & 1 for k in range(2, 20)})
                vals.append(r if r in (0, 1) else None)
            self.port_at[pid] = vals
        for pid, (kind, arg) in meaning.items():
            if kind == 'page':
                C.setname(pid, 'sel_page', 'V', 'address a[19:8] == 0x%03x (this channel\'s register page) '
                          'with the decode-mux controls at 0' % (self.page_base >> 8))
            elif kind == 'page_a':
                C.setname(pid, 'sel_a%d' % arg, 'V', 'sel_page & a[%d]' % arg)
            elif kind == 'strobe':
                C.setname(pid, 'sel_r%02x' % arg, 'V', 'address == cb+0x%02x (page 0x%03x) with the decode-mux '
                          'controls at 0' % (arg, pb >> 8))
            elif kind == 'page_cube':
                C.setname(pid, 'sel_' + ''.join('%s%s' % ('' if b else 'n', v) for v, b in sorted(arg.items())),
                          'V', 'sel_page & %s' % arg)
            elif kind == 'window':
                C.setname(pid, 'sel_win_%s' % '_'.join('%x' % (v >> 8) for v in arg), 'V',
                          'address window hit: a[19:9] in %s' % ['0x%05x' % v for v in arg])
        return meaning

    def offsets_of(self, t, deriv=None):
        """Register offsets (cb-relative, multiples of 4) for which template net t
        can be 1 (or, with deriv=port, for which t depends on that port), with
        sel_page = 1, sel_a[k] = offset bits, other ports existentially free."""
        C = self.C
        B = BDD()
        memo = {}

        def tb(x):
            if x in memo:
                return memo[x]
            if x in ('VCC', 'GND'):
                return 1 if x == 'VCC' else 0
            if x in C.drv and C.E[C.drv[x][0]]['kind'] == 'LUT':
                e = C.E[C.drv[x][0]]
                r = B.table(e['table'], [tb(y) for y in e['inputs']])
            else:
                r = B.var(x)
            memo[x] = r
            return r
        f = tb(t)
        if deriv:
            f = B.XOR(B.restrict(f, {deriv: 1}), B.restrict(f, {deriv: 0}))
        sup = B.support(f)
        dec = {p: m for p, m in self.meaning.items() if p in sup}
        SEL = ('page', 'page_a', 'strobe', 'page_cube')
        if not any(m[0] in SEL for m in dec.values()):
            return None
        # must be gated by the page hit (all page-qualified ports 0 -> 0)
        if B.restrict(f, {p: 0 for p, m in dec.items() if m[0] in SEL}) != 0:
            return None
        out = []
        for i, off in enumerate(range(0, 0x100, 4)):
            asg = {p: self.port_at[p][i] for p in dec if p in self.port_at and self.port_at[p][i] is not None}
            g = B.restrict(f, asg)
            if g != 0:
                out.append(off)
        if len(out) == 64:
            return None
        return out

    # ------------------------------------------------------------ registers
    def wsrc(self, t, depth=2):
        C = self.C
        if t in C.wd:
            return {(t, C.wd[t])}
        out = set()
        if depth and t in C.drv and C.E[C.drv[t][0]]['kind'] == 'LUT':
            for x in C.E[C.drv[t][0]]['inputs']:
                out |= self.wsrc(x, depth - 1)
        return out

    def registers(self):
        """Host-writable FFs: offset and host bit, from CE decode or from the
        data dependency of D (write-through-LUT registers)."""
        C = self.C
        regs = collections.defaultdict(dict)       # off -> {bit: [eid]}
        self.ffreg = {}
        ce_cache = {}
        for eid in C.T['order']:
            e = C.E[eid]
            if not e['kind'].startswith('FD'):
                continue
            ws = self.wsrc(e['ports'].get('D'))
            if len(ws) != 1:
                continue
            port, hb = next(iter(ws))
            if e['ce'] not in ('VCC',):
                if e['ce'] not in ce_cache:
                    ce_cache[e['ce']] = self.offsets_of(e['ce'])
                offs = ce_cache[e['ce']]
            else:
                offs = None
            if not offs:
                offs = self.offsets_of(e['ports']['D'], deriv=port)
            if not offs:
                continue
            for off in offs:
                regs[off].setdefault(hb, []).append(eid)
            self.ffreg[eid] = (tuple(offs), hb)
        self.regs = dict(sorted(regs.items()))
        for eid, (offs, hb) in self.ffreg.items():
            if len(offs) == 1:
                nm = 'r%02x[%d]' % (offs[0], hb)
            else:
                nm = 'r%s[%d]' % ('_'.join('%02x' % o for o in offs), hb)
            C.setname(eid + '.Q', nm, 'V', 'captures host data bit %d on a write to cb+%s' % (
                hb, '/'.join('0x%02x' % o for o in offs)))
        return self.regs

    # ------------------------------------------------------------ counters
    def chains(self):
        C = self.C
        E = C.E
        carries = [e for e in E.values() if e['kind'] == 'CARRY4']
        nxt = {}
        for e in carries:
            ci = e['ports'].get('CI')
            if ci and ci not in ('GND', 'VCC') and ci in C.drv:
                nxt[C.drv[ci][0]] = e['id']
        heads = [e['id'] for e in carries if e['ports'].get('CI') in ('GND', 'VCC', None)]
        out = []
        for h in heads:
            ch = [h]
            while ch[-1] in nxt:
                ch.append(nxt[ch[-1]])
            bits, outs = [], []
            first = E[ch[0]]
            if first['ports'].get('CYINIT') not in ('GND', 'VCC', None):
                bits.append(first['ports']['CYINIT'])
            down = True
            for c in ch:
                for i in range(4):
                    di = E[c]['ports'].get('DI[%d]' % i)
                    s = E[c]['ports'].get('S[%d]' % i)
                    if s in C.drv and E[C.drv[s][0]]['kind'] == 'LUT':
                        se = E[C.drv[s][0]]
                        if se['k'] == 1 and se['table'] == 1:
                            q = se['inputs'][0]
                            bits.append(q)
                            outs.append('%s.O[%d]' % (c, i))
                            continue
                        if se['k'] == 0 and se['table'] == 0:
                            continue
                    down = False
            # counter FFs: FFs whose D cone (1 LUT) reads an output of this chain
            regs = []
            for o in outs:
                for (x, p) in C.fanout(o):
                    if E[x]['kind'] == 'LUT':
                        for (y, q) in C.fanout(x + '.O'):
                            if E[y]['kind'].startswith('FD') and q == 'D':
                                regs.append(y)
            out.append(dict(carries=ch, bits=bits, down=down, regs=sorted(set(regs))))
        self.chain = out
        return out

    def load_sources(self, ff):
        """inputs of the D LUT of a counter FF other than its own chain output / Q"""
        C = self.C
        d = C.E[ff]['ports'].get('D')
        if d not in C.drv or C.E[C.drv[d][0]]['kind'] != 'LUT':
            return []
        return [x for x in C.E[C.drv[d][0]]['inputs'] if not ('.O[' in x or x == ff + '.Q')]

    # ------------------------------------------------------------ divider, tick, bit FSM
    def divider(self):
        """The counter loaded from the profile-select mux: its load sources are
        LUTs that read both the profile register (cb+0x00) and the profile
        select register (cb+0x08)."""
        C = self.C
        prof = {eid for eid, (offs, hb) in self.ffreg.items() if offs == (0,)}
        psel = {eid for eid, (offs, hb) in self.ffreg.items() if offs == (8,)}
        self.prof, self.psel = prof, psel
        best = None
        for ch in self.chain:
            n = 0
            for ff in ch['regs']:
                for x in self.load_sources(ff):
                    if x in C.drv and C.E[C.drv[x][0]]['kind'] == 'LUT':
                        ins = {y.split('.')[0] for y in C.E[C.drv[x][0]]['inputs']}
                        if ins & prof and ins & psel:
                            n += 1
            if n and (best is None or n > best[0]):
                best = (n, ch)
        if not best:
            return None
        ch = best[1]
        self.div = ch
        for i, q in enumerate(ch['bits']):
            C.setname(q, 'div_cnt[%d]' % i, 'V', 'carry-chain bit %d of the down-counter loaded from the selected '
                      'profile code' % i)
        # code mux outputs
        for eid, e in C.E.items():
            if e['kind'] == 'LUT':
                ins = {y.split('.')[0] for y in e['inputs']}
                pr = ins & self.prof
                if pr and ins & self.psel and len(ins) <= 6:
                    bits = {self.ffreg[x][1] % 8 for x in pr}
                    if len(bits) == 1:
                        b = bits.pop()
                        # polarity: value with profile bits all 1, select 0
                        vals = [1 if (y.split('.')[0] in pr) else 0 for y in e['inputs']]
                        pol = eval_table(e['table'], vals)
                        C.setname(eid + '.O', ('code[%d]' if pol else 'code_n[%d]') % b, 'V',
                                  'profile byte bit %d selected by profile select (0,3 -> profile 1; 1 -> 2; '
                                  '2 -> 3)' % b)
        # reload / tick: the LUT driving the CE of the counter bits that is a zero detect
        return ch

    def tick(self):
        C = self.C
        self.tickq = None
        if not getattr(self, 'div', None):
            return None
        # registered tick FF: a FF whose D aliases the divider reload net
        al = getattr(C, 'alias', {})
        self.tickq = None
        for eid, e in C.E.items():
            if e['kind'].startswith('FD'):
                d = e['ports'].get('D')
                if d in al:
                    src = al[d][0]
                    if src in C.drv and C.E[C.drv[src][0]]['kind'] == 'LUT':
                        ins = set(C.E[C.drv[src][0]]['inputs'])
                        # the reload LUT reads OR/NOR terms of the counter bits
                        sub = set()
                        for x in ins:
                            if x in C.drv and C.E[C.drv[x][0]]['kind'] == 'LUT':
                                sub |= set(C.E[C.drv[x][0]]['inputs'])
                        if len(sub & set(self.div['bits'])) >= 8:
                            self.tickq = eid
                            self.reload = src
                            C.setname(src, 'div_reload', 'V', 'divider == 0 (OR-reduction of all counter bits) '
                                      'or forced reload; loads the selected code')
                            C.setname(eid + '.Q', 'tick', 'V', 'registered div_reload: one pulse every code+1 '
                                      'clocks (through a route-through LUT in the reference instance)')
        return self.tickq

    def bit_fsm(self):
        """Phase register: FFs whose CE cone reads the registered tick."""
        C = self.C
        E = C.E
        if not self.tickq:
            return None
        ph = []
        for eid, e in E.items():
            if e['kind'].startswith('FD') and e['ce'] in C.drv and E[C.drv[e['ce']][0]]['kind'] == 'LUT':
                if self.tickq + '.Q' in E[C.drv[e['ce']][0]]['inputs']:
                    ph.append(eid)
        ph.sort()
        # keep those whose next state depends on the others (one register)
        self.phase = ph
        for i, x in enumerate(ph):
            C.setname(x + '.Q', 'phase[%d]' % i, 'V', 'bit-level phase register, advances on tick')
        # command inputs: support FFs that move the FSM out of state 0
        B = BDD()
        memo = {}

        def tb(x):
            if x in memo:
                return memo[x]
            if x in ('VCC', 'GND'):
                return 1 if x == 'VCC' else 0
            if x in C.drv and E[C.drv[x][0]]['kind'] == 'LUT':
                e = E[C.drv[x][0]]
                r = B.table(e['table'], [tb(y) for y in e['inputs']])
            else:
                r = B.var(x)
            memo[x] = r
            return r
        nexts = {x: tb(E[x]['ports']['D']) for x in ph}
        sup = set()
        for f in nexts.values():
            sup |= B.support(f)
        cand = sorted(v for v in sup if v.endswith('.Q') and v.split('.')[0] not in ph)
        base = {x + '.Q': 0 for x in ph}
        cmds, forcing = [], []
        for v in cand:
            asg = dict(base)
            for w in cand:
                asg[w] = 1 if w == v else 0
            nx = [B.restrict(nexts[x], asg) for x in ph]
            if any(n == 1 for n in nx):
                cmds.append(v)
        self.cmds = cmds
        self.nexts = (B, nexts)
        # transition table from state 0 for each command
        table = {}
        for cmd in cmds:
            st, seq = 0, [0]
            for step in range(40):
                asg = {w: 0 for w in cand}
                if step == 0:
                    asg[cmd] = 1
                for i, x in enumerate(ph):
                    asg[x + '.Q'] = (st >> i) & 1
                nx = 0
                for i, x in enumerate(ph):
                    r = B.restrict(nexts[x], asg)
                    if r not in (0, 1):
                        r = 0
                    nx |= r << i
                st = nx
                seq.append(st)
                if st == 0:
                    break
            table[cmd] = seq
        self.fsm_table = table
        return ph, cmds, table

    # ------------------------------------------------------------ simulation-based labels
    def harness(self):
        """Simulator plus register-write helper.  Qualifier ports take the values
        of one satisfying assignment of the profile register's write enable."""
        C = self.C
        S = Sim(C)
        quals = {}
        prof_ce = {C.E[x]['ce'] for x in self.prof if C.E[x]['ce'] != 'VCC'}
        if prof_ce:
            B = BDD()
            memo = {}

            def tb(x):
                if x in memo:
                    return memo[x]
                if x in ('VCC', 'GND'):
                    return 1 if x == 'VCC' else 0
                if x in C.drv and C.E[C.drv[x][0]]['kind'] == 'LUT':
                    e = C.E[C.drv[x][0]]
                    r = B.table(e['table'], [tb(y) for y in e['inputs']])
                else:
                    r = B.var(x)
                memo[x] = r
                return r
            f = tb(sorted(prof_ce)[0])
            asg = {p: v[0] for p, v in self.port_at.items()
                   if self.meaning.get(p, ('',))[0] in ('page', 'page_a', 'strobe', 'page_cube', 'window')
                   and v[0] is not None}
            g = B.restrict(f, asg)
            cs = B.cubes(g, 1)
            if cs:
                quals = {k: v for k, v in cs[0].items() if k in C.T['ports']}
        self.quals = quals
        hostbit = collections.defaultdict(list)
        for pid, h in C.wd.items():
            hostbit[h].append(pid)
        dec = {p: m for p, m in self.meaning.items()}

        def idle():
            for p in C.T['ports']:
                if p not in getattr(C, 'alias', {}):
                    S.ports[p] = 0
            for p, v in quals.items():
                if dec.get(p, ('',))[0] not in ('page', 'page_a', 'strobe', 'page_cube', 'window'):
                    S.ports[p] = v

        def write(off, data):
            idle()
            for p, vals in self.port_at.items():
                if self.meaning.get(p, ('',))[0] in ('page', 'page_a', 'strobe', 'page_cube', 'window') \
                        and vals[off // 4] is not None:
                    S.ports[p] = vals[off // 4]
            for h, ps in hostbit.items():
                for p in ps:
                    S.ports[p] = (data >> h) & 1
            S.step(1)
            idle()
            S.step(2)
        rst = [p for p, n in C.names.items() if n[0] == 'user_rst']
        idle()
        for p in rst:
            S.ports[p] = 1
        S.step(4)
        for p in rst:
            S.ports[p] = 0
        S.step(20)
        return S, write, idle

    def sim_labels(self, profile=0x00660918, ncyc=1600):
        """Force each bit-level command and watch the pads: START / STOP / bit
        with SDA released / bit with SDA low; then hold a bit command and
        measure the SCL period for profile select 0, 1, 2."""
        import copy
        C = self.C
        S, write, idle = self.harness()
        write(0x0c, 0x80000000)
        write(0x00, profile)
        base = copy.deepcopy(dict(S.val))
        pads = [e['id'] for e in S.pads]
        res = {}
        for cmd in self.cmds:
            S.val = collections.defaultdict(int, copy.deepcopy(base))
            S.force = {}
            write(0x08, 0)
            for i in range(400):
                S.step(1)
                if S.v(self.tickq + '.Q'):
                    break
            S.force = {cmd: 1}
            tr = []
            for i in range(ncyc):
                S.step(1)
                if i >= 2:
                    S.force = {}
                tr.append(tuple(S.v(C.E[p]['ports']['T']) for p in pads))
            res[cmd] = tr
        # which pad is SCL: the one that goes low and high again in a bit command
        def edges(tr, k):
            return [(i, tr[i][k]) for i in range(1, len(tr)) if tr[i][k] != tr[i - 1][k]]
        tog = [sum(len(edges(tr, k)) for tr in res.values()) for k in range(len(pads))]
        scl = max(range(len(pads)), key=lambda k: tog[k])
        sda = 1 - scl
        self.pad_role = {pads[scl]: 'scl', pads[sda]: 'sda'}
        labels = {}
        for cmd, tr in res.items():
            ev = []
            for i in range(1, len(tr)):
                if tr[i] != tr[i - 1]:
                    ev.append((i, tr[i][sda], tr[i][scl], tr[i - 1][sda], tr[i - 1][scl]))
            lab = 'other'
            for i, d, c, pd, pc in ev:
                if pc == 1 and c == 1 and pd == 1 and d == 0:
                    lab = 'START'
                    break
                if pc == 1 and c == 1 and pd == 0 and d == 1:
                    lab = 'STOP'
                    break
            if lab == 'other':
                sdas = {d for i, d, c, pd, pc in ev if c == 1}
                lab = 'BIT_SDA_RELEASED' if sdas == {1} else ('BIT_SDA_LOW' if sdas == {0} else 'other')
            labels[cmd] = lab
            C.setname(cmd, 'cmd_' + lab.lower(), 'I', 'bit-level command: forcing it for one cycle in the template '
                      'simulation produces %s on the pads' % lab)
        self.cmd_labels = labels
        # SCL period per profile select, with a bit command held
        bitcmd = next((c for c, l in labels.items() if l.startswith('BIT')), None)
        periods = {}
        for psel in (0, 1, 2):
            S.val = collections.defaultdict(int, copy.deepcopy(base))
            S.force = {}
            write(0x08, psel)
            S.force = {bitcmd: 1}
            tr = []
            for i in range(4000):
                S.step(1)
                tr.append(S.v(C.E[pads[scl]]['ports']['T']))
            fall = [i for i in range(1, len(tr)) if tr[i - 1] == 1 and tr[i] == 0]
            per = [b - a for a, b in zip(fall, fall[1:])]
            code = (profile >> (8 * psel)) & 0xff
            periods[psel] = dict(code=code, period=max(set(per[1:]), key=per[1:].count) if len(per) > 1 else None,
                                 periods=per[:8])
        self.scl_periods = periods
        for pad, role in self.pad_role.items():
            C.setname(pad + '.O', role + '_in', 'I', 'pad input of the %s pad (role from simulation)' % role.upper())
            t = C.E[pad]['ports'].get('T')
            C.setname(t, role + '_oe_n', 'V' if False else 'I', 'open-drain release of the %s pad (1 = released, '
                      'pulled up; 0 = driven low); FDPE INIT 1' % role.upper())
        return labels, periods

    # ------------------------------------------------------------ channel numbers
    def channel_map(self, l6dir):
        """Driver channel index of every instance of this family, from the page
        constant decoded by the net bound to the template's page port."""
        out = {}
        for f in sorted(os.listdir(l6dir)):
            if not re.match(r'ch\d\d\.json$', f):
                continue
            I = json.load(open(os.path.join(l6dir, f)))
            if I['family'] != self.C.F:
                continue
            # the page port, else any page & a[k] port: all fix a[19:8] to the page
            cands = ([self.page_port] if self.page_port else []) + sorted(
                p for p, m in self.meaning.items() if m[0] in ('page_a', 'strobe', 'page_cube'))
            out[I['channel']] = None
            B = self.B
            for port in cands:
                for net in I['bind'].get(port) or []:
                    g = cone_bdd(B, self.G, net, self.addr_leaf, {})
                    g = B.restrict(g, {v: 0 for v in B.support(g) if v in self.ctrl})
                    cs = B.cubes(g, 2)
                    if len(cs) == 1 and all('a%02d' % k in cs[0] for k in range(8, 20)):
                        pg = sum(1 << k for k in range(8, 20) if cs[0]['a%02d' % k])
                        out[I['channel']] = dict(page=pg, driver_channel=(pg - 0x1000) >> 8, via=port)
                        break
                if out[I['channel']]:
                    break
        self.chmap = out
        return out


# ====================================================================== writer
def vname(s):
    s = re.sub(r'[^A-Za-z0-9_\[\]]', '_', s)
    return s


def write_rtl(A, path, tables=None):
    C = A.C
    E = C.E
    L = []
    w = L.append
    nm = C.name
    T = C.T
    w('// i2c_channel, family %s: layer-7 pseudo-RTL view of the layer-6 module template' % C.F)
    w('// Generated by channelrtl.py from template.json (reference instance %s, %d elements).' % (
        T['reference'], len(E)))
    w('// This is a readable VIEW, not a synthesis model and not used to rebuild anything; layer 6')
    w('// (modules.py) carries the bytes.  Every element is annotated with its template element id(s).')
    w('// Name marks: V = structurally verified (exact check on the netlist / template), I = inferred')
    w('// (simulation of the template, register-map knowledge from the vendor driver, or naming by role).')
    w('// Element ids without a better name are printed as <eid>_<port> (V: they are the netlist).')
    w('')
    # ---- ports
    w('module i2c_channel_%s (' % C.F)
    w('  input  clk,            // V  user clock (62.5 MHz, BUFG->BUFH); ports %s' % ', '.join(
        p for p, c in C.portcls.items() if c == 'clk'))
    for p, c in sorted(C.portcls.items()):
        if c == 'reset':
            w('  input  %-14s  // V  %s (%s)' % (nm(p) + ',', C.names[p][2], p))
    wd = collections.defaultdict(list)
    for p, h in C.wd.items():
        wd[h].append(p)
    w('  input  [31:0] wdata,   // V  write data bus from the TLP RX path, host bit order (I: byte order from the')
    w('                         //    TRN dword convention; V: profile bytes land as the driver expects in simulation)')
    for h in sorted(wd):
        w('                         //    wdata[%d] = %s' % (h, ', '.join(sorted(wd[h]))))
    for p, (kind, arg) in sorted(A.meaning.items()):
        if p in C.names and kind in ('page', 'page_a', 'window', 'strobe', 'page_cube'):
            w('  input  %-14s  // V  %s (%s)' % (nm(p) + ',', C.names[p][2], p))
    others = [p for p in sorted(T['ports']) if C.portcls.get(p) not in ('clk', 'reset', 'wdata')
              and not (p in A.meaning and A.meaning[p][0] in ('page', 'page_a', 'window', 'strobe', 'page_cube'))
              and p not in getattr(C, 'alias', {})]
    w('  // %d further inputs from the shared core (decode qualifiers, byte enables, core-side state):' % len(others))
    for p in others:
        n = T['ports'][p]['ref_net']
        d = C.G.net_drv.get(n)
        lab = C.G.nodes[d[0]]['label'][:24] if d else '?'
        m = A.meaning.get(p, (C.portcls.get(p), None))
        w('  input  %-14s  // %s; class %s; driver in reference: %s %s' % (
            p + ',', 'V', m[0], lab, n))
    al = getattr(C, 'alias', {})
    if al:
        w('  // %d ports are route-through LUT1s of template nets in the reference (folded):' % len(al))
        for p, (t, inv) in sorted(al.items()):
            w('  //   %s == %s%s' % (p, '~' if inv else '', nm(t)))
    for pad, role in sorted(A.pad_role.items(), key=lambda kv: kv[1]):
        w('  inout  %-14s  // I  role from simulation; V open-drain IOBUF %s (I = GND, T = %s)' % (
            role + ',', pad, nm(E[pad]['ports'].get('T'))))
    w('  output [%d:0] to_core  // V  %d template nets read outside the module (read mux, status, BRAM data)'
      % (len(T['outputs']) - 1, len(T['outputs'])))
    w(');')
    w('')
    # ---- registers
    w('  // ================================================================ host registers (cb = 0x1000 + ch*0x100)')
    w('  // Offsets: V (BDD of the write enable / data dependency with sel_page = 1 and sel_a[7:2] = offset).')
    w('  // Register names from the vendor driver map: I.')
    for off, bits in A.regs.items():
        name = KNOWN_REGS.get(off, 'not used by the vendor driver')
        w('  // cb+0x%02x  %s' % (off, name))
        for hb in sorted(bits):
            w('  //    bit %2d <- %s' % (hb, ', '.join('%s (%s %s)' % (nm(x + '.Q'), x, E[x]['prim']) for x in bits[hb])))
    w('')
    # ---- divider
    w('  // ================================================================ SCL clock divider')
    if getattr(A, 'div', None):
        d = A.div
        w('  // V: %d-bit down-counter on carry chain %s (S = ~q, DI = q, CYINIT = bit 0).' % (
            len(d['bits']), ','.join(d['carries'])))
        w('  // V: on div_reload it loads code[7:0] (bits 15:8 load 0); otherwise it decrements while enabled.')
        w('  // V: div_reload = (div_cnt == 0) | forced reload (reset, abort, SCL released but still low = stretch wait).')
        w('  //    => one tick every code+1 clocks.')
        w('  // V: code = profile byte (sel 0 -> cb+0x00[7:0], 1 -> [15:8], 2 -> [23:16], 3 -> [7:0]).')
        w('  always @(posedge clk) if (div_reload) div_cnt <= {8\'d0, code}; else if (div_en) div_cnt <= div_cnt - 1;')
        w('  //   bits: %s' % ', '.join('%s=%s' % (nm(q), q.split('.')[0]) for q in d['bits']))
        w('  //   div_reload = %s   // %s' % (C.lut_expr(A.reload.split('.')[0]), A.reload.split('.')[0]))
    if getattr(A, 'scl_periods', None):
        w('  // I (template simulation, bit command held, 62.5 MHz clock):')
        for psel, r in A.scl_periods.items():
            if r['period']:
                w('  //   profile select %d, code 0x%02x: SCL period %d clocks = %.1f kHz  (6*(code+1) = %d)' % (
                    psel, r['code'], r['period'], 62500.0 / r['period'], 6 * (r['code'] + 1)))
    w('')
    # ---- bit FSM
    w('  // ================================================================ bit-level FSM (phase register)')
    if getattr(A, 'phase', None):
        w('  // V: phase[%d:0] = %s; CE = tick (or reset/abort); next state = f(phase, commands).' % (
            len(A.phase) - 1, ', '.join(A.phase)))
        w('  // V: transition sequences from state 0 (one step per tick), command held for the first step only:')
        for cmd, seq in A.fsm_table.items():
            w('  //   %-22s %s  (%d ticks)' % (nm(cmd), ' -> '.join(str(s) for s in seq), len(seq) - 1))
        w('  // I: command meaning from the pads in simulation: %s' % ', '.join(
            '%s=%s' % (c.split('.')[0], l) for c, l in A.cmd_labels.items()))
    if tables and C.F == 'B':
        w('  // ---------------------------------------------------------------- controller state tables')
        for l in open(tables).read().splitlines():
            w('  // ' + l)
    w('')
    # ---- pads
    w('  // ================================================================ pads')
    for pad, role in A.pad_role.items():
        t = E[pad]['ports'].get('T')
        if t in C.drv:
            ff = C.drv[t][0]
            d = E[ff]['ports'].get('D')
            w('  // %s: T = %s (%s, %s INIT %d, preset by %s)' % (role.upper(), nm(t), ff, E[ff]['prim'], E[ff]['init'],
                                                                nm(E[ff]['sr'])))
            if d in C.drv and E[C.drv[d][0]]['kind'] == 'LUT':
                w('  //        D = %s' % C.lut_expr(C.drv[d][0]))
    w('  // synchronisers: FF chains from the pad inputs')
    for pad, role in A.pad_role.items():
        chain = []
        cur = pad + '.O'
        for _ in range(4):
            nxt = [x for x, p in C.fanout(cur) if E[x]['kind'].startswith('FD') and p == 'D']
            if not nxt:
                break
            chain.append(nxt[0])
            cur = nxt[0] + '.Q'
        w('  //   %s_in -> %s' % (role, ' -> '.join(chain) if chain else '(through core ports or LUTs)'))
    w('')
    # ---- BRAM
    w('  // ================================================================ BRAM (RAMB18E1, SDP 36: 256 x 32 used)')
    for e in E.values():
        if e['kind'] == 'RAMB18E1':
            P = e['ports']
            grp = collections.defaultdict(list)
            for p, t in sorted(P.items(), key=lambda kv: (re.sub(r'\d+$', '', kv[0]), int(re.search(r'(\d*)$', kv[0]).group(1) or 0))):
                if t in ('VCC', 'GND'):
                    continue
                grp[re.sub(r'\d+$', '', p)].append('%s=%s' % (re.search(r'(\d*)$', p).group(1), nm(t)))
            for g, xs in grp.items():
                w('  //   %s %s: %s' % (e['id'], g, ', '.join(xs)))
    w('')
    # ---- everything
    w('  // ================================================================ all elements (netlist view)')
    for eid in T['order']:
        e = E[eid]
        k = e['kind']
        if k == 'LUT':
            out = eid + '.O'
            w('  assign %s = %s;  // %s %s' % (vname(nm(out)), C.lut_expr(eid), eid, C.names.get(out, ('', 'V'))[1]))
        elif k.startswith('FD'):
            q = eid + '.Q'
            sr = nm(e['sr']) if e['sr'] not in ('GND',) else None
            ce = nm(e['ce']) if e['ce'] != 'VCC' else None
            body = '%s%s <= %s;' % (('if (%s) ' % ce) if ce else '', vname(nm(q)), nm(e['ports'].get('D')))
            if sr:
                w('  always @(posedge clk or posedge %s) if (%s) %s <= 1\'b%d; else %s  // %s %s %s' % (
                    sr, sr, vname(nm(q)), 1 if k == 'FDPE' else 0, body, eid, k, C.names.get(q, ('', 'V'))[1]))
            else:
                w('  always @(posedge clk) %s  // %s %s INIT %d' % (body, eid, k, e['init']))
        elif k == 'CARRY4':
            w('  // %s CARRY4 PRECYINIT=%s %s' % (eid, e['precyinit'], {p: nm(t) for p, t in e['ports'].items()
                                                                         if t not in ('GND',)}))
        else:
            w('  // %s %s %s' % (eid, k, {p: nm(t) for p, t in e.get('ports', {}).items()}))
    w('endmodule')
    w('')
    w('// ---------------------------------------------------------------- names')
    for t, (n, conf, why) in sorted(C.names.items(), key=lambda kv: kv[1][0]):
        w('// %-24s %s  %-14s %s' % (n, conf, t, why))
    open(path, 'w').write('\n'.join(L) + '\n')


def analyse(l6dir, logic_path, outdir, F, ctrl=None, tables=None):
    C = load(l6dir, logic_path, F)
    port_aliases(C)
    A = Analysis(C)
    A.decode_ports(ctrl)
    A.registers()
    A.chains()
    A.divider()
    A.tick()
    A.bit_fsm()
    A.pad_role = {}
    A.cmd_labels = {}
    if A.tickq and A.cmds:
        A.sim_labels()
    A.channel_map(l6dir)
    os.makedirs(outdir, exist_ok=True)
    write_rtl(A, os.path.join(outdir, 'i2c_channel_%s.v' % F), tables)
    res = dict(family=F, reference=C.T['reference'], page_port=A.page_port,
               page_base=A.page_base, port_meaning={p: [m[0], m[1] if not isinstance(m[1], dict) else None]
                                                    for p, m in A.meaning.items()},
               registers={'0x%02x' % off: {str(b): v for b, v in bits.items()} for off, bits in A.regs.items()},
               divider=getattr(A, 'div', None), tick=A.tickq, phase=getattr(A, 'phase', None),
               commands=A.cmd_labels, fsm_table=getattr(A, 'fsm_table', None),
               scl_periods=getattr(A, 'scl_periods', None), pads=A.pad_role, channel_map=A.chmap,
               aliases=getattr(C, 'alias', {}),
               names={t: list(v) for t, v in C.names.items()})
    json.dump(res, open(os.path.join(outdir, 'i2c_channel_%s.json' % F), 'w'), indent=1, sort_keys=True)
    modgraph.save_canon_cache()
    return A, res


def main():
    a = sys.argv[1:]
    if len(a) < 4 or a[0] != 'analyse':
        raise SystemExit(__doc__)
    fams = 'BA'
    if '--family' in a:
        i = a.index('--family')
        fams = a[i + 1]
        del a[i:i + 2]
    tables = None
    if '--tables' in a:
        i = a.index('--tables')
        tables = a[i + 1]
        del a[i:i + 2]
    blocks = None
    if '--blocks' in a:
        i = a.index('--blocks')
        blocks = a[i + 1]
        del a[i:i + 2]
    ctrl = None
    allres = {}
    for F in fams:
        A, res = analyse(a[1], a[2], a[3], F, ctrl, tables)
        if A.page_port and ctrl is None:
            ctrl = A.ctrl      # the shared address-mux controls, reused for the other family
        print('family %s: page port %s base %s; registers %s; divider %s bits; phase %s; commands %s' % (
            F, res['page_port'], hex(res['page_base'] or 0), sorted(res['registers']),
            len(res['divider']['bits']) if res['divider'] else None, res['phase'], res['commands']))
        print('  SCL periods', res['scl_periods'])
        print('  channel map', res['channel_map'])
        allres[F] = (A, res)
    write_chmap(allres, blocks, os.path.join(a[3], 'channel-map.txt'))


def write_chmap(allres, blocks, path):
    import csv
    pads = {}
    pkg = {}
    if blocks:
        ann = json.load(open_any(blocks))
        pads = {'ch%02d' % i: p for i, p in enumerate(ann['channels'])}
        pk = os.path.join(HERE, '..', 'work', 'src', 'prjxray-db', 'artix7', 'xc7a100tfgg484-2', 'package_pins.csv')
        if os.path.exists(pk):
            pkg = {r['site']: r for r in csv.DictReader(open(pk))}
    rows = []
    for F, (A, res) in allres.items():
        roles = {}
        for pad, role in (res.get('pads') or {}).items():
            roles[role] = pad
        for ch, m in res['channel_map'].items():
            rows.append((ch, F, m))
    L = ['# channel map: pad-pair channel (layer 5 order) -> driver channel', '',
         'driver channel = (page - 0x1000) / 0x100, page decoded from the net bound to the template\'s',
         'page / page&a[k] / strobe port with the address-mux controls quantified (V for the decode;',
         'port number = driver channel + 1 as in the vendor driver).  Pads: package pins of the pair',
         '(FGG484 assumed); SDA/SCL role per family from simulation (I).', '',
         'ch    family  page    driver-ch  port  pads (pins)']
    for ch, F, m in sorted(rows):
        pp = pads.get(ch, [])
        pins = '/'.join(pkg.get(p, {}).get('pin', '?') for p in pp)
        if m:
            L.append('%s  %s       0x%04x  %2d         %2d    %s (%s)' % (ch, F, m['page'], m['driver_channel'],
                                                                     m['driver_channel'] + 1, ' + '.join(pp), pins))
        else:
            L.append('%s  %s       ?       ?          ?     %s (%s)' % (ch, F, ' + '.join(pp), pins))
    drv = sorted(m['driver_channel'] for _, _, m in rows if m)
    L += ['', 'decoded %d of %d; driver channels distinct: %s; all of 0..31: %s' % (
        len(drv), len(rows), len(set(drv)) == len(drv), drv == list(range(32)))]
    for F in sorted(allres):
        L.append('family %s: driver channels %s' % (F, sorted(m['driver_channel'] for _, f, m in rows if m and f == F)))
    open(path, 'w').write('\n'.join(L) + '\n')


if __name__ == '__main__':
    main()
