#!/usr/bin/env python3
"""Layer 5: functional blocks over the lossless netlist.

    blocks.py analyse NET.json LOGIC.json OUTDIR [--name update]
    blocks.py compare LABEL:LOGIC.json:BLOCKS.json ...   block-level comparison

Reads the layer-4 physical netlist (fasmnet.py) and its logical view
(xc7bels.py), partitions the design into named blocks, and writes to OUTDIR:

  <name>.blocks.json    annotation: every physical cell and net -> block path,
                        every logical primitive -> block path (+ evidence)
  <name>.blocks.md      category table (counts, nets, bounding box, links,
                        evidence, confidence)
  <name>.blocks.dot     block-level graph
  <name>.channels.txt   per-channel replication report
  <name>.pcie.txt       PCIe hard-block neighbourhood (ports, BRAMs, ties)
  <name>.io.txt         IO ring by bank and function

Method (details and limitations in BLOCKS.md):
  1. hard blocks and clock tree from primitive types and dedicated wiring
  2. clock domains from the BUFG/BUFH/MMCM tree (FF/BRAM clock pins)
  3. IO pads classified from the logical IOB model; 64 open-drain pads paired
     into 32 channels by mutual-best overlap of their local logic
  4. lockstep replicated growth from the 32 pad pairs: a primitive joins
     channel c only if a primitive with the same label and the same
     connections to already-matched roles exists in >= T channels
  5. the rest of the user logic split by attachment to the PCIe TRN ports,
     the sideband pads and the channels
"""
import collections
import csv
import gzip
import hashlib
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, '..', 'work')
sys.path.insert(0, HERE)
import xc7bels  # noqa: E402

PKG = os.path.join(WORK, 'src', 'prjxray-db', 'artix7', 'xc7a100tfgg484-2', 'package_pins.csv')
CLOCK_PRIMS = ('BUFGCTRL', 'BUFHCE', 'MMCME2_ADV')
SITE_XY = re.compile(r'_X(\d+)Y(\d+)')


def open_any(path, mode='rt'):
    return gzip.open(path, mode) if path.endswith('.gz') else open(path, mode)


def h(*a):
    return hashlib.sha1(repr(a).encode()).hexdigest()[:12]


class Design:
    def __init__(self, net, logic):
        self.phys = net
        self.logic = logic
        self.P = {p['name']: p for p in logic['prims']}
        self.N = logic['nets']
        self.cells = {c['name']: c for c in net['cells']}
        self.block = {}        # prim -> block path
        self.evidence = collections.defaultdict(list)   # block -> [(conf, text)]
        self.reports = {}

    # ------------------------------------------------------------ helpers
    def drv(self, n):
        v = self.N.get(n)
        return v['driver'] if v else None

    def prim_of_net(self, n):
        d = self.drv(n)
        return self.P[d[0]] if d else None

    def assign(self, prim, block, force=False):
        if force or prim not in self.block:
            self.block[prim] = block

    def ev(self, block, conf, text):
        self.evidence[block].append((conf, text))

    def label(self, p):
        t = p['prim']
        if p.get('fn'):
            return t + ':' + ','.join('%s=%d/%x' % (o, len(f['support']), xc7bels.lut_pcanon(f['table'], len(f['support'])))
                                      for o, f in sorted(p['fn'].items()))
        if t.startswith(('FD', 'LD')):
            c = p['conns']
            sr = c.get('R', c.get('S', c.get('CLR', c.get('PRE'))))
            return '%s:%s:ce1=%d:sr0=%d' % (t, p['params']['INIT'], c.get('CE') == 'VCC', sr == 'GND')
        if t == 'CARRY4':
            return 'CARRY4:' + p['params'].get('PRECYINIT', '')
        return t

    @staticmethod
    def role(p, port):
        t = p['prim']
        if t.startswith('LUT'):
            return port if port in ('O5', 'O6') else 'I'
        if t in ('SRLC32E', 'SRL16E', 'RAM32X1S', 'RAM64X1S') and re.match(r'A\d$', port):
            return 'A'
        return re.sub(r'\d+$', '#', port) if t in ('RAMB18E1', 'PCIE_2_1', 'GTPE2_CHANNEL') else port

    # ------------------------------------------------------------ 1. clocks
    def clocks(self):
        P, N = self.P, self.N
        self.clocknets = {n for n, v in N.items() if v['driver'] and P[v['driver'][0]]['prim'] in CLOCK_PRIMS}
        # root of each clock net through BUFH/BUFG
        def root(n, depth=0):
            p = self.prim_of_net(n)
            if p is None or depth > 6:
                return n
            if p['prim'] in ('BUFHCE',):
                return root(p['conns'].get('I'), depth + 1)
            if p['prim'] == 'BUFGCTRL':
                return p['name'] + '/O'
            return n
        self.clkroot = {n: root(n) for n in self.clocknets}
        names = {
            'BUFGCTRL_X0Y0/O': ('user_clk', 'MMCM CLKOUT2 (/16: 62.5 MHz if refclk 100 MHz) -> BUFG: PCIe USERCLK and all user logic'),
            'BUFGCTRL_X0Y1/O': ('pipe_clk', 'BUFGCTRL mux of MMCM CLKOUT0/CLKOUT1 (125/250 MHz): PCIe PIPECLK, GTP USRCLK, PIPE logic'),
            'BUFGCTRL_X0Y2/O': ('dclk_125', 'MMCM CLKOUT0 -> BUFG (125 MHz): GTP DRP / PIPE reset logic'),
            'BUFGCTRL_X0Y16/O': ('refclk', 'IBUFDS_GTE2 (MGTREFCLK0 216) -> BUFG'),
            'BUFGCTRL_X0Y17/O': ('txoutclk', 'GTP TXOUTCLK -> BUFG -> BUFH -> MMCM CLKIN1'),
        }
        self.domain = {}
        counts = collections.Counter()
        for p in self.P.values():
            for port in ('C', 'G', 'CLK', 'CLKARDCLK', 'CLKBWRCLK', 'USERCLK', 'PIPECLK', 'TXUSRCLK', 'RXUSRCLK',
                         'TXUSRCLK2', 'RXUSRCLK2', 'DRPCLK'):
                n = p['conns'].get(port)
                if n in self.clocknets:
                    d = names.get(self.clkroot[n], (self.clkroot[n], ''))[0]
                    self.domain.setdefault(p['name'], d)
                    if p['prim'].startswith(('FD', 'LD')) or p['prim'] in ('RAMB18E1', 'SRLC32E', 'SRL16E'):
                        counts[d] += 1
                    break
        self.clock_names = names
        self.domain_counts = counts

    # ------------------------------------------------------------ 2. hard blocks
    def hard(self):
        P = self.P
        for p in P.values():
            t = p['prim']
            if t == 'PCIE_2_1':
                self.assign(p['name'], 'pcie/hard_block')
            elif t in ('GTPE2_CHANNEL', 'GTPE2_COMMON', 'IBUFDS_GTE2', 'IPAD'):
                self.assign(p['name'], 'pcie/gtp')
            elif t in ('BUFGCTRL', 'BUFHCE'):
                self.assign(p['name'], 'clock/tree')
            elif t == 'MMCME2_ADV':
                self.assign(p['name'], 'clock/pipe_clock')
        pcie = next(p for p in P.values() if p['prim'] == 'PCIE_2_1')
        self.pcie = pcie
        # BRAMs on the MIM ports
        mim = collections.defaultdict(set)
        for port, n in pcie['conns'].items():
            if port.startswith('MIM'):
                d = self.drv(n)
                peers = set()
                if d and d[0] != pcie['name']:
                    peers.add(d[0])
                for s in self.N.get(n, {}).get('sinks', []):
                    if s[0] != pcie['name']:
                        peers.add(s[0])
                for q in peers:
                    if P[q]['prim'] == 'RAMB18E1':
                        mim[q].add(re.sub(r'\d+$', '', port))
        self.mim = mim
        for b, ports in mim.items():
            kind = 'tx_retry_buffer' if any(x.startswith('MIMTX') for x in ports) else 'rx_buffer'
            self.assign(b, 'pcie/bram/' + kind)
        tx = sum(1 for b, ps in mim.items() if any(x.startswith('MIMTX') for x in ps))
        self.ev('pcie/bram', 'V', '%d RAMB18E1 halves on MIMTX* ports and %d on MIMRX* ports of PCIE_2_1 '
                '(pcie_bram_7x: TX retry buffer / RX receive buffer)' % (tx, len(mim) - tx))
        # pipe clock: MMCM and the BUFGs it feeds / is fed by
        mm = next(p for p in P.values() if p['prim'] == 'MMCME2_ADV')
        self.ev('clock/pipe_clock', 'V', 'MMCME2_ADV CLKIN1 <- BUFH <- BUFG <- GTP TXOUTCLK; CLKFBOUT -> CLKFBIN '
                'directly; CLKOUT0 -> BUFG + BUFGCTRL.I0, CLKOUT1 -> BUFGCTRL.I1 (S0/S1 from fabric), CLKOUT2 -> BUFG '
                '(user clock): the structure of Xilinx pcie_7x pipe_clock (gen2, PCIE_LANE=1)')
        par = {k: v for k, v in mm['params'].items()}
        self.reports['mmcm'] = par
        # IO prims
        for p in P.values():
            if p['prim'] in ('IOBUF', 'IBUF', 'OBUF', 'OBUFT', 'IOB_CFG_ONLY'):
                self.assign(p['name'], 'io/pads')

    # ------------------------------------------------------------ 3. PIPE logic
    def pipe(self):
        for p, d in self.domain.items():
            if d in ('pipe_clk', 'dclk_125', 'refclk', 'txoutclk') and p not in self.block:
                self.assign(p, 'pcie/pipe_wrapper/' + d)
        # combinational cones feeding only PIPE-domain sequential logic
        changed = True
        while changed:
            changed = False
            for p in self.P.values():
                if p['name'] in self.block or not p['prim'].startswith(('LUT', 'CARRY', 'MUXF', 'INV', 'TIE')):
                    continue
                outs = [n for k, n in p['conns'].items() if k in ('O', 'O5', 'O6') or k.startswith(('O[', 'CO['))]
                sinks = [s for n in outs for s in self.N.get(n, {}).get('sinks', [])]
                if not sinks:
                    continue
                bl = {self.block.get(s[0], '') for s in sinks}
                if len(bl) == 1:
                    b = next(iter(bl))
                    if b.startswith('pcie/pipe_wrapper'):
                        self.assign(p['name'], b)
                        changed = True
        n = collections.Counter(self.block[p] for p in self.block if self.block[p].startswith('pcie/pipe_wrapper'))
        self.ev('pcie/pipe_wrapper', 'V', 'sequential logic clocked by the PIPE clocks (pipe_clk %d, dclk_125 %d, '
                'refclk %d, txoutclk %d) plus LUT cones feeding only it' % tuple(
                    self.domain_counts.get(k, 0) for k in ('pipe_clk', 'dclk_125', 'refclk', 'txoutclk')))
        self.ev('pcie/pipe_wrapper', 'I', 'pcie_7x pipe_wrapper (pipe_reset, pipe_rate, pipe_sync, pipe_drp, '
                'qpll/cpll reset FSMs), by clock and by its GTP/PCIE PIPE connections')

    # ------------------------------------------------------------ 4. IO
    def io(self):
        pk = {}
        if os.path.exists(PKG):
            for r in csv.DictReader(open(PKG)):
                pk[r['site']] = r
        self.pkg = pk
        rows = []
        cls = collections.defaultdict(list)
        for p in self.P.values():
            if p['prim'] not in ('IOBUF', 'IBUF', 'OBUF', 'OBUFT', 'IOB_CFG_ONLY'):
                continue
            c = p['conns']

            def kind(n):
                if n is None:
                    return '-'
                if n in ('VCC', 'GND'):
                    return n
                q = self.prim_of_net(n)
                return q['prim'] if q else '?'
            k = '%s I=%s T=%s O=%s %s' % (p['prim'], kind(c.get('I')), kind(c.get('T')),
                                          'used' if c.get('O') else '-', '/'.join(p['params']['DRIVE']))
            cls[k].append(p['name'])
        self.padclass = cls
        od = [x for k, v in cls.items() if k.startswith('IOBUF I=GND T=FDPE') for x in v]
        outs = [x for k, v in cls.items() if k.startswith('OBUF I=LUT6_2') for x in v]
        ins = [x for k, v in cls.items() if k.startswith('IBUF ') for x in v]
        self.od_pads, self.sb_out, self.sb_in = od, outs, ins
        for x in od:
            self.assign(x, 'io/i2c_pads', force=True)
        for x in outs:
            self.assign(x, 'io/sideband_out', force=True)
        for x in ins:
            self.assign(x, 'io/inputs', force=True)
        self.ev('io/i2c_pads', 'V', '%d IOBUF with I=GND and T from an FDPE INIT=1 (open drain, released at reset), '
                'LVCMOS33 drive I12_I8' % len(od))
        self.ev('io/sideband_out', 'V', '%d OBUF driven by LUT6_2, drive I12_I16' % len(outs))

    # ------------------------------------------------------------ 5. channels
    def graph(self):
        """Analysis graph: LUT6_2 split into its O6 and O5 functions (Vivado packs
        unrelated functions together differently in each replica), LUT inputs
        limited to the support of the function.  Node 'X#O5' / 'X#O6' maps back to
        primitive X."""
        if getattr(self, '_graph', None):
            return self._graph
        nodes = {}
        for p in self.P.values():
            if p.get('fn'):
                for o, fn in p['fn'].items():
                    conns = {'A%d' % i: p['conns'].get('A%d' % i) for i in fn['support']}
                    conns[o] = p['conns'].get(o)
                    lab = 'LUT%d:%x' % (len(fn['support']), xc7bels.lut_pcanon(fn['table'], len(fn['support'])))
                    nodes['%s#%s' % (p['name'], o)] = dict(prim=p['name'], label=lab, conns=conns, lut=True)
            else:
                nodes[p['name']] = dict(prim=p['name'], label=self.label(p), conns=p['conns'], lut=False)
        outs = {}
        for x, nd in nodes.items():
            p = self.P[nd['prim']]
            for port, n in nd['conns'].items():
                if n is None:
                    continue
                d = self.N.get(n, {}).get('driver')
                if d and d[0] == nd['prim'] and d[1] == port:
                    outs[n] = (x, port)
        nets = collections.defaultdict(lambda: dict(driver=None, sinks=[]))
        for x, nd in nodes.items():
            for port, n in nd['conns'].items():
                if n is None or n.startswith('PAD:'):
                    continue
                if outs.get(n) == (x, port):
                    nets[n]['driver'] = [x, port]
                else:
                    nets[n]['sinks'].append([x, port])
        self._graph = (nodes, dict(nets))
        return self._graph

    def node_role(self, nd, port):
        if nd['lut']:
            return 'O' if port in ('O5', 'O6') else 'I'
        return self.role(self.P[nd['prim']], port)

    def adjacency(self, K, exclude=()):
        key = (K, tuple(sorted(exclude)))
        cache = self.__dict__.setdefault('_adj', {})
        if key in cache:
            return cache[key]
        nodes, nets = self.graph()
        adj = collections.defaultdict(list)   # node -> [(my_role, other_role, other)]
        for n, v in nets.items():
            if n in self.clocknets or n in ('VCC', 'GND') or not v['driver'] or len(v['sinks']) > K:
                continue
            d = v['driver']
            for s in v['sinks']:
                if s[0] == d[0] or s[0] in exclude or d[0] in exclude:
                    continue
                adj[d[0]].append(('>' + self.node_role(nodes[d[0]], d[1]), self.node_role(nodes[s[0]], s[1]), s[0]))
                adj[s[0]].append(('<' + self.node_role(nodes[s[0]], s[1]), self.node_role(nodes[d[0]], d[1]), d[0]))
        cache[key] = adj
        return adj

    def pair_pads(self, pads, K=12, depth=8):
        adj = self.adjacency(K)
        nb = {}
        for s in pads:
            seen = {s}
            fr = [s]
            for _ in range(depth):
                nf = []
                for x in fr:
                    for _, _, y in adj[x]:
                        if y not in seen:
                            seen.add(y)
                            nf.append(y)
                fr = nf
            nb[s] = seen
        best = {}
        for a in pads:
            sc = sorted(((len(nb[a] & nb[b]) / len(nb[a] | nb[b]), b) for b in pads if b != a), reverse=True)
            best[a] = (sc[0][1], sc[0][0], sc[1][0])
        pairs = sorted({tuple(sorted((a, best[a][0]))) for a in pads if best[best[a][0]][0] == a})
        return pairs, best

    def lockstep(self, seeds, T, K=24, stop=()):
        """seeds: list (one per instance) of {node: role}.  Returns
        (owner {node: inst}, role {node: role}, contested set, rounds)."""
        adj = self.adjacency(K)
        nodes, _ = self.graph()
        owner, role = {}, {}
        for i, sd in enumerate(seeds):
            for x, r in sd.items():
                owner[x] = i
                role[x] = r
        contested = set()
        labels = {}
        rounds = 0
        while True:
            rounds += 1
            cand = collections.defaultdict(lambda: collections.defaultdict(list))   # node -> inst -> edges
            for x, i in owner.items():
                for mine, other, y in adj[x]:
                    if y in owner or y in contested or y in stop or nodes[y]['prim'] in stop:
                        continue
                    # edge seen from y: y's port role, x's port role, x's role
                    cand[y][i].append((other, mine[1:], role[x]))
            groups = collections.defaultdict(lambda: collections.defaultdict(list))  # sig -> inst -> [nodes]
            for y, per in cand.items():
                if len(per) > 1:
                    contested.add(y)
                    continue
                (i, edges), = per.items()
                sig = h(nodes[y]['label'], tuple(sorted(edges)))
                groups[sig][i].append(y)
            added = 0
            for sig, per in groups.items():
                if len(per) < T:
                    continue
                counts = collections.Counter(len(v) for v in per.values())
                m = counts.most_common(1)[0][0]
                for i, ys in per.items():
                    if len(ys) != m:
                        continue
                    for y in ys:
                        owner[y] = i
                        role[y] = sig
                        added += 1
            if not added:
                break
        return owner, role, contested, rounds

    def channels(self, T=16, absorb_rounds=8):
        pairs, best = self.pair_pads(self.od_pads)
        self.pairs = pairs
        self.pad_best = best
        self.ev('i2c', 'V', '%d open-drain pads pair into %d mutual-best pairs by overlap of their local logic '
                '(Jaccard of 8-hop neighbourhoods over nets with <= 12 sinks, LUT6_2 halves split; best partner %.2f-%.2f, '
                'second best <= %.2f for every pad)' % (len(self.od_pads), len(pairs),
                                                        min(b[1] for b in best.values()), max(b[1] for b in best.values()),
                                                        max(b[2] for b in best.values())))
        seeds = [{a: 'pad', b: 'pad'} for a, b in pairs]
        stop = set(p for p in self.P if self.block.get(p, '').startswith(('pcie/', 'clock/')))
        stop |= set(self.sb_out) | set(self.sb_in)
        owner, role, contested, rounds = self.lockstep(seeds, T, stop=stop)
        nodes, _ = self.graph()
        matched = dict(owner)
        # role-set similarity between channels -> families
        rs = collections.defaultdict(collections.Counter)
        for x, i in owner.items():
            rs[i][role[x]] += 1
        sim = {}
        for i in range(len(pairs)):
            for j in range(len(pairs)):
                if i < j:
                    a, b = rs[i], rs[j]
                    sim[(i, j)] = sum((a & b).values()) / max(1, sum((a | b).values()))
        fam, nf = {}, 0
        for i in range(len(pairs)):
            if i in fam:
                continue
            fam[i] = nf
            for j in range(len(pairs)):
                if j not in fam and sim.get((min(i, j), max(i, j)), 0) >= 0.7:
                    fam[j] = nf
            nf += 1
        self.families = fam
        self.ch_matched = matched
        absorbed = 0
        self.ch_owner, self.ch_role = owner, role
        per = collections.defaultdict(list)
        for x, i in owner.items():
            per[i].append(x)
        inst_by_role = collections.defaultdict(set)
        for x, r in role.items():
            inst_by_role[r].add(matched[x])
        hist = collections.Counter(len(v) for v in inst_by_role.values())
        self.ch_stats = dict(rounds=rounds, roles=len(inst_by_role), roles_by_instances=dict(sorted(hist.items())),
                             matched_nodes=len(matched), contested=len(contested), absorbed=absorbed,
                             families={f: sorted(i for i, g in fam.items() if g == f) for f in set(fam.values())},
                             per_channel_matched=[sum(1 for x in matched if matched[x] == i) for i in range(len(pairs))],
                             per_channel_total=[len(per[i]) for i in range(len(pairs))])
        for x, i in owner.items():
            px = nodes[x]['prim']
            if self.P[px]['prim'] in ('IOBUF',):
                continue
            self.assign(px, 'i2c/ch%02d' % i)
        self.ev('i2c/chNN', 'V', 'lockstep growth: %d node roles; %d nodes matched (a role joins only when the same '
                'label and the same connections to matched roles occur in >= %d channels)' % (
                    len(inst_by_role), len(matched), T))
        return per

    def absorb(self, rounds=8, frac=0.5):
        """Give unassigned logic to a channel when most of its neighbours
        (nets <= 64 sinks) are already in that channel and none is in another."""
        nodes, _ = self.graph()
        adj = self.adjacency(64)
        owner = self.ch_owner
        absorbed = 0
        for _ in range(rounds):
            moves = {}
            for y in nodes:
                py = nodes[y]['prim']
                if y in owner or py in self.block:
                    continue
                nb = adj.get(y, ())
                if not nb:
                    continue
                cs = collections.Counter(owner[x] for _, _, x in nb if x in owner)
                other = sum(1 for _, _, x in nb if x not in owner and nodes[x]['prim'] in self.block)
                if len(cs) == 1:
                    c, k = cs.most_common(1)[0]
                    if k >= frac * len(nb) and other == 0:
                        moves[y] = c
            if not moves:
                break
            owner.update(moves)
            absorbed += len(moves)
            for y, c in moves.items():
                self.assign(nodes[y]['prim'], 'i2c/ch%02d' % c)
        # a RAMB18 whose only channel neighbour is channel c belongs to c
        for p in self.P.values():
            if p['prim'] != 'RAMB18E1' or p['name'] in self.block:
                continue
            cs = {owner[x] for _, _, x in adj.get(p['name'], ()) if x in owner}
            if len(cs) == 1:
                c = next(iter(cs))
                owner[p['name']] = c
                self.assign(p['name'], 'i2c/ch%02d' % c)
                absorbed += 1
        self.ch_stats['absorbed'] = absorbed
        self.ch_stats['per_channel_total'] = [sum(1 for x, i in owner.items() if i == c) for c in range(len(self.pairs))]
        self.ev('i2c/chNN', 'I', '%d further nodes absorbed: at least half of their neighbours are matched nodes '
                'of one channel and none belongs to another block' % absorbed)

    # ------------------------------------------------------------ 6. shared
    def shared(self):
        P = self.P
        pcie = self.pcie['name']
        trn_in = [n for k, n in self.pcie['conns'].items() if k.startswith(('TRNRD', 'TRNRSOF', 'TRNREOF', 'TRNRSRC',
                                                                            'TRNRBARHIT', 'TRNRREM', 'TRNRERRFWD'))]
        trn_out = [n for k, n in self.pcie['conns'].items() if k.startswith(('TRNTD', 'TRNTSOF', 'TRNTEOF', 'TRNTSRC',
                                                                             'TRNTREM', 'TRNTSTR'))]
        cfg = [n for k, n in self.pcie['conns'].items() if k.startswith(('CFG', 'PL', 'TRNLNKUP', 'USERRSTN',
                                                                          'TRNTBUFAV', 'TRNTDSTRDY', 'TRNRDSTRDY',
                                                                          'TRNTCFG', 'SYSRSTN'))]

        def first_stage(nets, forward=True, depth=2):
            out = set()
            fr = set()
            for n in nets:
                if n in ('VCC', 'GND'):
                    continue
                if forward:
                    fr |= {s[0] for s in self.N.get(n, {}).get('sinks', []) if s[0] != pcie}
                else:
                    d = self.drv(n)
                    if d and d[0] != pcie:
                        fr.add(d[0])
            for _ in range(depth):
                out |= fr
                nf = set()
                for x in fr:
                    p = P[x]
                    if p['prim'].startswith(('FD', 'LD')):
                        continue   # stop after the first register stage
                    for k, n in p['conns'].items():
                        if forward and (k in ('O', 'O5', 'O6') or k.startswith(('O[', 'CO['))):
                            nf |= {s[0] for s in self.N.get(n, {}).get('sinks', [])}
                        if not forward and not (k in ('O', 'O5', 'O6') or k.startswith(('O[', 'CO['))):
                            d = self.drv(n)
                            if d:
                                nf.add(d[0])
                fr = nf - out
            return {x for x in out if x not in self.block}
        rx1 = first_stage(trn_in, True)
        tx1 = first_stage(trn_out, False)
        for x in rx1:
            self.assign(x, 'pcie/user_if/rx')
        for x in tx1:
            self.assign(x, 'pcie/user_if/tx')
        for x in first_stage(cfg, True, 1) | first_stage(cfg, False, 1):
            self.assign(x, 'pcie/user_if/cfg')
        self.ev('pcie/user_if', 'V', 'first register stage and LUTs next to the PCIE_2_1 TRN RX (%d), TRN TX (%d) '
                'and CFG/PL ports' % (len(rx1), len(tx1)))
        # sideband: logic local to the sideband pads (lockstep over the pads themselves)
        for name, pads in (('sideband/io', self.sb_out + self.sb_in),):
            seeds = [{p: 'pad'} for p in pads]
            stop = set(x for x in P if x in self.block)
            owner, role, contested, rounds = self.lockstep(seeds, 16, K=12, stop=stop)
            n = 0
            nodes, _ = self.graph()
            for x, i in owner.items():
                px = nodes[x]['prim']
                if px in self.block:
                    continue
                self.assign(px, name)
                n += 1
            self.ev(name, 'V', '%d primitives matched in lockstep from %d pads (role replicated in >= 16 pads)' % (
                n, len(pads)))
        self.absorb()
        # everything else in the user clock domain
        rest = [x for x in P if x not in self.block]
        for x in rest:
            d = self.domain.get(x)
            p = P[x]
            if p['prim'] in ('RAMB18E1',):
                self.assign(x, 'mem/bram')
            elif p['prim'] in ('TIE', 'INV', 'IDELAYE2', 'ILOGICE3', 'OLOGICE3', 'RAMB18E1_UNUSED'):
                self.assign(x, 'io/misc')
            else:
                self.assign(x, 'core/shared')

    # ------------------------------------------------------------ 7. refine shared
    def refine_shared(self):
        """Split core/shared by the dominant attachment of each primitive."""
        P = self.P
        shared = [x for x, b in self.block.items() if b == 'core/shared']
        sset = set(shared)
        # neighbour block histogram (nets <= 64 sinks, no clocks)
        adj = self.adjacency(64)
        for it in range(3):
            moves = {}
            for x in shared:
                if self.block[x] != 'core/shared':
                    continue
                c = collections.Counter()
                nodes, _ = self.graph()
                ys = [y for k in (x, x + '#O6', x + '#O5') for _, _, y in adj.get(k, ())]
                for y in ys:
                    b = self.block.get(nodes[y]['prim'], '')
                    if b.startswith('i2c/ch'):
                        c['core/i2c_glue'] += 1
                    elif b.startswith('pcie/user_if'):
                        c['core/pcie_glue'] += 1
                    elif b.startswith('sideband'):
                        c['core/sideband_regs'] += 1
                if c:
                    b, n = c.most_common(1)[0]
                    if n >= 2 and n >= sum(c.values()) * 0.6:
                        moves[x] = b
            for x, b in moves.items():
                self.block[x] = b
            if not moves:
                break

    # ------------------------------------------------------------ output
    def bbox(self, prims):
        xs, ys = [], []
        for x in prims:
            c = self.cells.get(self.P[x]['cell'])
            if c is None:
                continue
            m = SITE_XY.search(c['tile'])
            if m:
                xs.append(int(m.group(1)))
                ys.append(int(m.group(2)))
        if not xs:
            return ''
        return 'X%d..%d Y%d..%d' % (min(xs), max(xs), min(ys), max(ys))

    def annotate(self):
        """Annotation over the lossless netlist: physical cells and nets."""
        bycell = collections.defaultdict(collections.Counter)
        for x, b in self.block.items():
            bycell[self.P[x]['cell']][b] += 1
        cell_block = {}
        for c in self.phys['cells']:
            cnt = bycell.get(c['name'])
            if cnt:
                cell_block[c['name']] = cnt.most_common(1)[0][0]
            elif c['kind'] == 'tile':
                cell_block[c['name']] = None   # resolved below from its tile's sites
            else:
                cell_block[c['name']] = 'unused/config_only'
        tile_block = {}
        for c in self.phys['cells']:
            if cell_block.get(c['name']):
                tile_block.setdefault(c['tile'], cell_block[c['name']])
        for c in self.phys['cells']:
            if cell_block[c['name']] is None:
                cell_block[c['name']] = tile_block.get(c['tile'], 'unused/config_only')
        net_block = {}
        for n in self.phys['nets']:
            d = n['driver']
            if n['const']:
                net_block[n['name']] = 'const'
            elif d and d[0] in cell_block:
                # logical driver block if the net is a primitive output
                lp = self.N.get(n['name'])
                if lp and lp['driver'] and lp['driver'][0] in self.block:
                    net_block[n['name']] = self.block[lp['driver'][0]]
                else:
                    net_block[n['name']] = cell_block[d[0]]
            else:
                net_block[n['name']] = 'unrouted/undriven'
        mixed = sum(1 for c, cnt in bycell.items() if len(cnt) > 1)
        return cell_block, net_block, mixed

    def table(self):
        rows = collections.defaultdict(collections.Counter)
        for x, b in self.block.items():
            t = self.P[x]['prim']
            t = 'LUT' if t.startswith('LUT') else 'FF' if t.startswith(('FD', 'LD')) else t
            rows[b][t] += 1
        return rows

    def links(self):
        L = collections.Counter()
        for n, v in self.N.items():
            if n in self.clocknets or n in ('VCC', 'GND') or not v['driver']:
                continue
            a = self.top(self.block.get(v['driver'][0], '?'))
            for s in v['sinks']:
                b = self.top(self.block.get(s[0], '?'))
                if a != b:
                    L[(a, b)] += 1
        return L

    @staticmethod
    def top(b):
        if b.startswith('i2c/ch'):
            return 'i2c/chNN'
        return '/'.join(b.split('/')[:2])


CONF = {
    'pcie/hard_block': 'V', 'pcie/gtp': 'V', 'pcie/bram': 'V', 'clock/pipe_clock': 'V', 'clock/tree': 'V',
    'pcie/pipe_wrapper': 'V (by clock) / I (IP name)', 'pcie/user_if': 'V (adjacency) / I (role)',
    'io/i2c_pads': 'V', 'io/sideband_out': 'V', 'io/inputs': 'V', 'io/pads': 'V', 'io/misc': 'V',
    'i2c/chNN': 'V (matched part) / I (absorbed part)', 'sideband/out': 'V (replicated) / I (role)',
    'sideband/in': 'V (replicated) / I (role)', 'core/shared': 'I', 'core/i2c_glue': 'I',
    'core/pcie_glue': 'I', 'core/sideband_regs': 'I', 'mem/bram': 'I',
}

DESC = {
    'pcie/hard_block': 'PCIE_2_1 integrated block (gen2 x1, 64-bit TRN user interface)',
    'pcie/gtp': 'GTPE2_CHANNEL lane 0 + GTPE2_COMMON (PLL) + IBUFDS_GTE2 refclk + pads',
    'pcie/bram': 'RAMB18 halves on the PCIE_2_1 MIM ports (pcie_bram_7x)',
    'pcie/pipe_wrapper': 'logic on the PIPE/DRP/refclk clocks (pcie_7x pipe_wrapper)',
    'pcie/user_if': 'first register stage next to the TRN RX/TX and CFG ports',
    'clock/pipe_clock': 'MMCME2_ADV of the pipe clock module',
    'clock/tree': 'BUFGCTRL and BUFHCE',
    'i2c/chNN': '32 replicated I2C channel engines (one SDA/SCL pad pair and one RAMB18 each)',
    'sideband/io': 'logic around the 64 sideband output pads and 72 input pads, matched in lockstep per pad',
    'core/shared': 'remaining user-clock logic: TLP decode, register file, read mux, completion path (not split further)',
    'core/i2c_glue': 'shared logic whose neighbours are mostly channel logic',
    'core/pcie_glue': 'shared logic whose neighbours are mostly the TRN interface stage',
    'core/sideband_regs': 'shared logic whose neighbours are mostly sideband logic',
    'io/i2c_pads': '64 open-drain IOBUF (I2C SDA/SCL)', 'io/sideband_out': '64 OBUF driven by LUTs',
    'io/inputs': 'IBUF with a load', 'io/pads': 'other configured pads', 'io/misc': 'IOI ties, unused BRAM sites',
    'mem/bram': 'RAMB18 not attached to a channel or to PCIe',
}


def top2(b):
    if b.startswith('i2c/ch'):
        return 'i2c/chNN'
    if b.startswith(('pcie/bram', 'pcie/pipe_wrapper', 'pcie/user_if')):
        return '/'.join(b.split('/')[:2])
    return b


def write_reports(D, ann, outdir, name):
    rows = D.table()
    links = collections.Counter()
    for n, v in D.N.items():
        if n in D.clocknets or n in ('VCC', 'GND') or not v['driver']:
            continue
        a = top2(D.block.get(v['driver'][0], '?'))
        for snk in v['sinks']:
            b = top2(D.block.get(snk[0], '?'))
            if a != b:
                links[(a, b)] += 1
    groups = collections.defaultdict(list)
    for x, b in D.block.items():
        groups[top2(b)].append(x)
    nets_by = collections.Counter(top2(D.block.get(v['driver'][0], '?')) for n, v in D.N.items()
                                  if v['driver'] and n not in ('VCC', 'GND'))
    ev = collections.defaultdict(list)
    for b, items in D.evidence.items():
        ev[top2(b) if b != 'i2c' else 'i2c/chNN'] += items
    lines = ['# Block table: %s' % name, '',
             'Generated by `blocks.py analyse` from the layer-4 netlist. Counts are logical primitives '
             '(LUT = LUT6/LUT5/LUT6_2 sites, FF = FD*/LD*). "nets" = logical nets driven from the block. '
             'Links = logical connections to other blocks (driver -> sink pins), top 4. '
             'Confidence: V = exact structural check, I = inference.', '',
             '| block | what | cells by type | nets | area (tile X/Y) | links (out) | confidence |',
             '|---|---|---|---|---|---|---|']
    for b in sorted(groups, key=lambda b: (b.split('/')[0], b)):
        cnt = collections.Counter()
        for x in groups[b]:
            t = D.P[x]['prim']
            cnt['LUT' if t.startswith('LUT') else 'FF' if t.startswith(('FD', 'LD')) else t] += 1
        out = sorted(((n, l[1]) for l, n in links.items() if l[0] == b), reverse=True)[:4]
        lines.append('| `%s` | %s | %s | %d | %s | %s | %s |' % (
            b, DESC.get(b, ''), ', '.join('%s %d' % kv for kv in cnt.most_common()), nets_by.get(b, 0),
            D.bbox(groups[b]), ', '.join('%s %d' % (t, n) for n, t in out), CONF.get(b, 'I')))
    lines += ['', '## Evidence', '']
    for b in sorted(ev):
        for conf, text in ev[b]:
            lines.append('- `%s` (%s): %s' % (b, conf, text))
    lines += ['', '## Clock domains (sequential primitives per clock)', '']
    for d, n in D.domain_counts.most_common():
        root = next((k for k, v in D.clock_names.items() if v[0] == d), d)
        lines.append('- `%s` (%s): %d -- %s' % (d, root, n, D.clock_names.get(root, ('', ''))[1]))
    st = D.ch_stats
    lines += ['', '## Channel replication', '',
              '- lockstep rounds %d, roles %d, matched nodes %d, contested %d, absorbed %d' % (
                  st['rounds'], st['roles'], st['matched_nodes'], st['contested'], st.get('absorbed', 0)),
              '- roles by number of channels that contain them: %s' % st['roles_by_instances'],
              '- structural families (role-set overlap >= 0.7 with the first member): %s' % {k: v for k, v in st['families'].items()},
              '- see `%s.channels.txt` for per-channel numbers' % name]
    open(os.path.join(outdir, name + '.blocks.md'), 'w').write('\n'.join(lines) + '\n')
    # DOT
    dot = ['digraph blocks {', '  rankdir=LR; node [shape=box, fontsize=10];']
    for b in sorted(groups):
        lab = '%s\\n%d prims' % (b, len(groups[b]))
        if b == 'i2c/chNN':
            lab = 'i2c/chNN (32x)\\n%d prims total' % len(groups[b])
        dot.append('  "%s" [label="%s"];' % (b, lab))
    for (a, b), n in links.items():
        if n >= 8 and a in groups and b in groups:
            dot.append('  "%s" -> "%s" [label="%d", penwidth=%.1f];' % (a, b, n, 1 + min(6, n / 400)))
    dot.append('}')
    open(os.path.join(outdir, name + '.blocks.dot'), 'w').write('\n'.join(dot) + '\n')
    # channels
    cl = ['# per-channel report (%s)' % name, '',
          'ch  pads (pair)                 family matched total  FF  LUT CARRY4 BRAM']
    for i, (a, b) in enumerate(D.pairs):
        cnt = collections.Counter()
        for x, bb in D.block.items():
            if bb == 'i2c/ch%02d' % i:
                t = D.P[x]['prim']
                cnt['LUT' if t.startswith('LUT') else 'FF' if t.startswith(('FD', 'LD')) else t] += 1
        pa = '%s/%s' % (D.pkg.get(a, {}).get('pin', '?'), D.pkg.get(b, {}).get('pin', '?'))
        fam = next((k for k, v in st['families'].items() if i in v), '?')
        brams = [x for x, bb in D.block.items() if bb == 'i2c/ch%02d' % i and D.P[x]['prim'] == 'RAMB18E1']
        cl.append('%02d  %-11s %-15s %-6s %7d %5d %4d %4d %6d %s' % (
            i, a.replace('IOB_', ''), '(%s)' % pa, fam, st['per_channel_matched'][i], st['per_channel_total'][i],
            cnt['FF'], cnt['LUT'], cnt['CARRY4'], ','.join(brams)))
    open(os.path.join(outdir, name + '.channels.txt'), 'w').write('\n'.join(cl) + '\n')
    # PCIe report
    pc = D.pcie
    pl = ['# PCIE_2_1 neighbourhood (%s)' % name, '']
    ties = collections.defaultdict(dict)
    for port, n in pc['conns'].items():
        m = re.match(r'^(.*?)(\d+)$', port)
        if n in ('VCC', 'GND') and m:
            ties[m.group(1)][int(m.group(2))] = 1 if n == 'VCC' else 0
    for base in ('CFGVENDID', 'CFGDEVID', 'CFGREVID', 'CFGSUBSYSVENDID', 'CFGSUBSYSID'):
        bits = ties.get(base, {})
        if bits:
            v = sum(b << i for i, b in bits.items())
            pl.append('- %s tied to 0x%0*x (%d bits tied, all from TIEOFF/const nets)' % (base, (max(bits) + 4) // 4, v, len(bits)))
    groups_p = collections.defaultdict(collections.Counter)
    for port, n in pc['conns'].items():
        base = re.sub(r'\d+$', '', port)
        if n in ('VCC', 'GND'):
            groups_p[base][n] += 1
        else:
            d = D.drv(n)
            if d and d[0] == pc['name']:
                for s2 in D.N.get(n, {}).get('sinks', []):
                    groups_p[base]['-> ' + top2(D.block.get(s2[0], '?'))] += 1
            elif d:
                groups_p[base]['<- ' + top2(D.block.get(d[0], '?'))] += 1
    pl += ['', 'port group: connections (by block)', '']
    for base in sorted(groups_p):
        pl.append('- %-28s %s' % (base, dict(groups_p[base])))
    pl += ['', 'MMCME2_ADV features:', ''] + ['- %s = %s' % kv for kv in sorted(D.reports.get('mmcm', {}).items())]
    open(os.path.join(outdir, name + '.pcie.txt'), 'w').write('\n'.join(pl) + '\n')
    # IO report
    il = ['# IO ring (%s); package pins assume FGG484 (see LAYER2.md)' % name, '',
          'class (prim, I-driver, T-driver, input used, drive): count', '']
    for k, v in sorted(D.padclass.items(), key=lambda kv: -len(kv[1])):
        il.append('- %-50s %d' % (k, len(v)))
    il += ['', 'per bank: class counts', '']
    bank = collections.defaultdict(collections.Counter)
    for k, v in D.padclass.items():
        for x in v:
            bank[D.pkg.get(x, {}).get('bank', '?')][k.split(' T=')[0]] += 1
    for b in sorted(bank):
        il.append('- bank %s: %s' % (b, dict(bank[b])))
    il += ['', 'channel pad pairs (channel index = order of this list, not the driver port number)', '']
    for i, (a, b) in enumerate(D.pairs):
        il.append('- ch%02d: %s (%s, bank %s) + %s (%s, bank %s)' % (
            i, a, D.pkg.get(a, {}).get('pin', '?'), D.pkg.get(a, {}).get('bank', '?'),
            b, D.pkg.get(b, {}).get('pin', '?'), D.pkg.get(b, {}).get('bank', '?')))
    open(os.path.join(outdir, name + '.io.txt'), 'w').write('\n'.join(il) + '\n')


def analyse(net_path, logic_path, outdir, name):
    net = json.load(open_any(net_path))
    logic = json.load(open_any(logic_path))
    D = Design(net, logic)
    D.clocks()
    D.hard()
    D.pipe()
    D.io()
    per = D.channels()
    D.shared()
    D.refine_shared()
    os.makedirs(outdir, exist_ok=True)
    cell_block, net_block, mixed = D.annotate()
    ann = dict(format='i2cfpga-blocks-1', source=os.path.basename(net_path),
               cells=cell_block, nets=net_block, prims=D.block,
               channels=[list(p) for p in D.pairs], channel_stats=D.ch_stats,
               evidence={k: v for k, v in D.evidence.items()})
    with open(os.path.join(outdir, name + '.blocks.json'), 'w') as f:
        json.dump(ann, f, indent=0, sort_keys=True)
    write_reports(D, ann, outdir, name)
    return D, ann, mixed


def compare(specs, out=sys.stdout):
    """specs: [(label, logic.json, blocks.json)].  Per top-level block: counts and
    the multiset difference of primitive labels (LUT functions up to input
    permutation, FF types) against the first spec."""
    data = []
    for lab, lp, bp in specs:
        logic = json.load(open_any(lp))
        ann = json.load(open_any(bp))
        D = Design(dict(cells=[], nets=[]), logic)
        per = collections.defaultdict(collections.Counter)
        cnt = collections.defaultdict(collections.Counter)
        for x, b in ann['prims'].items():
            p = D.P.get(x)
            if p is None:
                continue
            t = p['prim']
            cnt[top2(b)]['LUT' if t.startswith('LUT') else 'FF' if t.startswith(('FD', 'LD')) else t] += 1
            if p.get('fn'):
                for o, f in p['fn'].items():
                    per[top2(b)]['LUT%d:%x' % (len(f['support']), xc7bels.lut_pcanon(f['table'], len(f['support'])))] += 1
            else:
                per[top2(b)][D.label(p)] += 1
        data.append((lab, cnt, per, ann.get('channel_stats', {})))
    blocks = sorted(set().union(*[set(d[1]) for d in data]))
    print('block-level comparison: %s' % ' vs '.join(d[0] for d in data), file=out)
    print('labels = multiset of primitive labels (LUT function up to input order, FF type/INIT/CE/SR);', file=out)
    print('"diff" = labels added + removed relative to %s' % data[0][0], file=out)
    print('', file=out)
    print('%-22s ' % 'block' + ' '.join('%-30s' % d[0] for d in data), file=out)
    for b in blocks:
        row = []
        for lab, cnt, per, st in data:
            c = cnt.get(b, {})
            row.append('LUT %5d FF %5d other %3d' % (c.get('LUT', 0), c.get('FF', 0),
                                                     sum(v for k, v in c.items() if k not in ('LUT', 'FF'))))
        diffs = []
        for lab, cnt, per, st in data[1:]:
            a, bb = data[0][2].get(b, collections.Counter()), per.get(b, collections.Counter())
            diffs.append('diff +%d/-%d' % (sum((bb - a).values()), sum((a - bb).values())))
        print('%-22s ' % b + ' '.join('%-30s' % r for r in row) + '  ' + ' '.join(diffs), file=out)
    print('', file=out)
    for lab, cnt, per, st in data:
        fams = st.get('families', {})
        print('%s: channel families %s; per-channel matched %s' % (
            lab, {k: len(v) for k, v in fams.items()}, st.get('per_channel_matched')), file=out)


def main():
    a = sys.argv[1:]
    if a and a[0] == 'compare':
        specs = []
        for x in a[1:]:
            lab, lp, bp = x.split(':')
            specs.append((lab, lp, bp))
        compare(specs)
        return
    if len(a) < 4 or a[0] != 'analyse':
        raise SystemExit(__doc__)
    name = 'update'
    if '--name' in a:
        i = a.index('--name')
        name = a[i + 1]
        del a[i:i + 2]
    D, ann, mixed = analyse(a[1], a[2], a[3], name)
    rows = D.table()
    for b in sorted(rows):
        print('%-34s %s' % (b, dict(rows[b].most_common())))
    print('channel stats', D.ch_stats)
    print('cells with prims from >1 block (packing):', mixed)


if __name__ == '__main__':
    main()
