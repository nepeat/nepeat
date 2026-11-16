#!/usr/bin/env python3
"""Lift the simulation netlist (xo3vlog.py --sim) to readable, hierarchical Verilog. Generic.

    xo3lift.py SIM.v NAMES.json OUTDIR [--maxsop 8]

NAMES.json (all keys optional):
  {"names":    {netlist_net: "name" | "vec[3]"},   # registers, pads (<pad>_in/_out/_oe ports
                                                     # keep their port names), or wires (cut points)
   "blocks":   {netlist_net_or_name: "block"},      # module of a signal; default "misc"
   "comments": {netlist_net_or_name: "text"},
   "modules":  {"block": "module comment"},
   "ports":    {pad_port: "readable alias"}}       # comment only: the top keeps the netlist port names

Every register keeps its netlist cell in a trailing comment (`// R<y>C<x>_SLICE<s>.FF<n>`). Each next-state
input (async reset, sync reset, clock enable, data) is written as a minimised sum of products over the
named leaves when its support is at most --maxsop leaves; otherwise as a tree of intermediate wires
that follows the netlist's own LUTs (`w_<lut>`, also minimised where small). Carry chains whose
halves are plain incrementers (P = one register, G = 0, carry-in 1) become `vec + 1`; other chains
stay as explicit carry logic (`P ? cin : G`).

Outputs: OUTDIR/<block>.v (one module per block, ports derived from use), OUTDIR/top.v (instances +
pad wiring, same port list as SIM.v), OUTDIR/regmap.json (netlist register -> hierarchical RTL name
`u_<block>.<name>`, for the equivalence check: rename the gold netlist's registers to these names),
OUTDIR/netblocks.json (netlist register/wire/pad port -> block, for a layer-5 annotation).

This is a view: equivalence with SIM.v must be proven (yosys equiv_* or a miter); see xo3equiv.sh.
"""
import os, re, sys, json, argparse, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import xo3ana

ID = re.compile(r'^[A-Za-z_][A-Za-z0-9_]*$')


def vbase(n):
    m = re.fullmatch(r'(\w+)\[(\d+)\]', n)
    return (m.group(1), int(m.group(2))) if m else (n, None)


class Lifter:
    def __init__(self, sim, cfg, maxsop):
        self.cfg = cfg
        self.names = dict(cfg.get('names', {}))
        self.maxsop = maxsop
        cuts = [n for n in self.names if not n.endswith(('_out', '_oe'))]
        self.D = xo3ana.Design(sim, cuts=[c for c in cuts])
        D = self.D
        # pad ports keep their names
        for p in D.ports_in:
            self.names.setdefault(p, p)
        self.blocks = {}
        for k, v in cfg.get('blocks', {}).items():
            self.blocks[k] = v
        self.comments = cfg.get('comments', {})
        self.auto = {}        # net -> (name, verilog expr, block)
        self.order = []       # emission order of auto wires
        self.chains = {}      # half -> (vecname, index) for incrementers
        self.undriven = set()
        self.find_chains()

    # ---------- naming
    def nm(self, net):
        if net in self.names:
            return self.names[net]
        if net in self.D.regs:
            return 'r_' + net
        return 'w_' + net

    def block_of(self, net):
        for k in (net, self.names.get(net)):
            if k and k in self.blocks:
                return self.blocks[k]
        n = self.names.get(net)
        if n:
            b, _ = vbase(n)
            if b in self.blocks:
                return self.blocks[b]
        return None

    # ---------- carry chains
    def find_chains(self):
        D = self.D
        halves = sorted(n[:-2] for n in D.vexpr if n.endswith('_P') and n[:-2] + '_FCO' in D.vexpr)
        hs = set(halves)
        cin = {}
        for h in halves:
            m = re.fullmatch(r'\w+ \? (\S+) : \S+', D.vexpr[h + '_FCO'])
            cin[h] = m.group(1)
        nxt = {c[:-4]: h for h, c in cin.items() if c.endswith('_FCO') and c[:-4] in hs}
        starts = [h for h in halves if not (cin[h].endswith('_FCO') and cin[h][:-4] in hs)]
        self.chain_list = []
        for s in starts:
            ch = [s]
            while ch[-1] in nxt:
                ch.append(nxt[ch[-1]])
            self.chain_list.append(ch)
        for k, ch in enumerate(sorted(self.chain_list)):
            # incrementer?  first half: P == 0, G == 1 (carry in 1); rest: P == single reg, G == 0
            def fn(n):
                s = sorted(D.support(n))
                return s, D.tt(n, s) if len(s) <= 4 else None
            s0, t0 = fn(ch[0] + '_P')
            g0s, g0 = fn(ch[0] + '_G')
            if not (s0 == [] and t0 == 0 and g0s == [] and g0 == 1):
                continue
            bits = []
            ok = True
            for h in ch[1:]:
                s, t = fn(h + '_P')
                gs, g = fn(h + '_G')
                if len(s) == 1 and t == 2 and gs == [] and g == 0:
                    bits.append(s[0])
                else:
                    ok = False; break
            if not ok:
                continue
            vec = 'inc%d' % k
            self.chain_inc = getattr(self, 'chain_inc', {})
            self.chain_inc[vec] = (ch, bits)
            for i, h in enumerate(ch[1:]):
                self.chains[h + '_F'] = (vec, i)
            self.chains[ch[-1] + '_FCO'] = (vec, len(bits))

    # ---------- expressions
    def leafname(self, n, blk):
        if n in self.chains:
            vec, i = self.chains[n]
            self.need_chain(vec, blk)
            return '%s[%d]' % (vec, i)
        if n in self.D.leaves:
            return self.nm(n)
        if n not in self.D.vexpr:
            self.undriven.add(n)
            return "1'b0"   # undriven in the netlist (a LOGIC slice's FCO into a chain start whose P = 0)
        return self.wire(n, blk)

    def need_chain(self, vec, blk):
        if vec in self.auto:
            return
        ch, bits = self.chain_inc[vec]
        cat = '{%s}' % ', '.join(self.nm(b) for b in reversed(bits))
        self.auto[vec] = (vec, None, blk, '%s + 1\'b1' % ('{1\'b0, %s}' % cat), len(bits) + 1,
                          'incrementer, carry chain %s..%s' % (ch[0], ch[-1]))
        self.order.append(vec)

    def wire(self, n, blk):
        if n in self.auto:
            return self.auto[n][0]
        name = self.nm(n)
        b = self.block_of(n) or blk
        self.auto[n] = (name, None, b, None, 1, None)
        cut = n in self.D.leaves
        if cut:
            self.D.leaves.discard(n); self.D._sup.clear()
        e = self.expr(self.D.vexpr[n], b, n)
        if cut:
            self.D.leaves.add(n); self.D._sup.clear()
        self.auto[n] = (name, None, b, e, 1, n)
        self.order.append(n)
        return name

    def expr(self, e, blk, owner=None):
        D = self.D
        e = e.strip()
        sup = sorted(D.support_expr(e))
        if not any(x in self.chains for x in xo3ana.names_in(e)) and len(sup) <= self.maxsop and \
                not any(x in self.chains for x in sup):
            t = D.tt(e, sup)
            used = self.used_leaves(t, len(sup))
            return self.sop(t, [self.leafname(x, blk) if k in used else 'UNUSED' for k, x in enumerate(sup)])
        m = re.fullmatch(r'(\S+) \? (\S+) : (\S+)', e)
        if m:
            s, a, b = (self.leafname(x, blk) if not x.startswith("1'b") else x for x in m.groups())
            return '%s ? %s : %s' % (s, a, b)
        out = []
        for t in xo3ana.TOK.findall(e):
            if re.match(r'[A-Za-z_]', t):
                out.append(self.leafname(t, blk))
            else:
                out.append(t)
        s = ''.join(' %s ' % t if t in '&|^' else t for t in out)
        return re.sub(r'\s+', ' ', s).strip()

    @staticmethod
    def used_leaves(t, n):
        used = set()
        for k in range(n):
            blk = 1 << k
            m0 = 0
            for st in range(0, 1 << n, blk * 2):
                m0 |= ((1 << blk) - 1) << st
            if (t & m0) != ((t >> blk) & m0):
                used.add(k)
        return used

    @staticmethod
    def sop(t, names):
        n = len(names)
        if t == 0:
            return "1'b0"
        if t == (1 << (1 << n)) - 1:
            return "1'b1"
        s = xo3ana.sop(t, names)
        terms = [x.strip() for x in s.split(' | ')]
        terms = [' & '.join(x.split('&')) for x in terms]
        if len(terms) == 1:
            return terms[0]
        return ' | '.join('(%s)' % x if ' & ' in x else x for x in terms)

    # ---------- emission
    def run(self, outdir):
        D = self.D
        os.makedirs(outdir, exist_ok=True)
        items = collections.defaultdict(list)   # block -> lines (regs)
        regblk = {}
        for q in sorted(D.regs):
            b = self.block_of(q) or 'misc'
            regblk[q] = b
        stmts = collections.defaultdict(list)
        for q in sorted(D.regs, key=lambda q: (regblk[q], self.nm(q))):
            r = D.regs[q]
            b = regblk[q]
            qn = self.nm(q)
            tag = r.get('tag', '')
            cm = self.comments.get(q) or self.comments.get(qn)
            lines = []
            if cm:
                lines.append('// %s' % cm)
            init = r.get('init', "1'b0")
            lines.append('initial %s = %s;' % (qn, init))
            if r['kind'] == 'latch':
                ar = self.expr(r['arst'], b)
                en = self.expr(r['en'], b)
                d = self.expr(r['d'], b)
                lines.append('`ifdef XO3_EQUIV  // equivalence model: hold = state sampled each OSC edge (same in the gold)')
                lines.append('reg %s_hold; initial %s_hold = %s; always @(posedge osc) %s_hold <= %s;' % (qn, qn, init, qn, qn))
                lines.append('always @* %s = (%s) ? %s : (%s) ? %s : %s_hold;' % (qn, ar, r['arst_val'], en, d, qn))
                lines.append('`else')
                lines.append('always @* if (%s) %s = %s; else if (%s) %s = %s; // %s (transparent latch)' % (
                    ar, qn, r['arst_val'], en, qn, d, tag))
                lines.append('`endif')
                stmts[b].append('\n'.join(lines)); continue
            clk = 'osc' if r['clk'] == 'R1C3_OSCH_OSC' else self.leafname(r['clk'], b)
            sens = ['%s %s' % (r['edge'], clk)]
            body = []
            if r.get('arst'):
                a = self.expr(r['arst'], b)
                if not ID.match(a):
                    an = 'arst_' + re.sub(r'\W', '_', qn)
                    an = self.share_wire(a, b, 'rst')
                    a = an
                sens.append('posedge %s' % a)
                body.append('if (%s) %s <= %s;' % (a, qn, r['arst_val']))
            if r.get('srst'):
                body.append('if (%s) %s <= %s;' % (self.expr(r['srst'], b), qn, r['srst_val']))
            d = self.expr(r['d'], b)
            if r.get('ce'):
                body.append('if (%s) %s <= %s;' % (self.expr(r['ce'], b), qn, d))
            else:
                body.append('%s <= %s;' % (qn, d))
            lines.append('always @(%s) %s // %s' % (' or '.join(sens), ' else '.join(body), tag))
            stmts[b].append('\n'.join(lines))
        # named wires that nobody used yet (cut points) and pad outputs
        for n in self.names:      # named wires (cut points) are leaves of every cone: define them here
            if n in D.vexpr and n not in D.regs and n not in self.auto:
                self.wire(n, self.block_of(n) or 'misc')
        padout = {}
        for p in D.ports_out:
            src = re.search(r'^assign %s = (.*?);' % re.escape(p), D.src, re.M)
            e = src.group(1)
            if p.endswith('_oe'):
                padout[p] = e
            else:
                padout[p] = e
        top_assign = []
        for p, e in sorted(padout.items()):
            names = xo3ana.names_in(e)
            b = None
            for x in names:
                b = b or self.block_of(x) or (self.auto[x][2] if x in self.auto else None)
            b = self.blocks.get(p) or b or 'io'
            v = self.expr(e, b)
            top_assign.append((p, v, b))
        # ---- collect symbols per block
        sym_blk = {}
        for q in D.regs:
            sym_blk[self.nm(q)] = regblk[q]
        for k, (name, _, b, e, w, _src) in self.auto.items():
            sym_blk[name] = b
        decl_w = {}
        for k, (name, _, b, e, w, _src) in self.auto.items():
            decl_w[name] = w
        defs = collections.defaultdict(list)
        for k in self.order:
            name, _, b, e, w, src = self.auto[k]
            c = self.comments.get(k) or self.comments.get(name)
            tail = ' // %s' % (c or src) if (c or src) else ''
            defs[b].append('assign %s = %s;%s' % (name, e, tail))
        pad_lines = collections.defaultdict(list)
        for p, v, b in top_assign:
            pad_lines[b].append('assign %s = %s;' % (p, v))
        blocks = sorted(set(list(stmts) + list(defs) + list(pad_lines)))
        # references
        tokre = re.compile(r'\b([A-Za-z_][A-Za-z0-9_]*)\b')

        def refs(text):
            return set(tokre.findall(re.sub(r"\d+'[bhd][0-9a-fA-F]+", '', re.sub(r'//.*', '', text))))
        kw = {'initial', 'always', 'posedge', 'negedge', 'or', 'if', 'else', 'assign', 'begin', 'end'}
        pad_ins = set(D.ports_in)
        regmap = {}
        uses = collections.defaultdict(set)   # block -> symbols referenced
        owned = collections.defaultdict(set)
        for b in blocks:
            text = '\n'.join(stmts[b] + defs[b] + pad_lines[b])
            for s in refs(text) - kw:
                base = vbase(s)[0]
                uses[b].add(base)
        for s, b in sym_blk.items():
            owned[b].add(vbase(s)[0])
        for p, v, b in top_assign:
            owned[b].add(p)
        exported = collections.defaultdict(set)
        for b in blocks:
            for s in uses[b]:
                for b2 in blocks:
                    if b2 != b and s in owned[b2]:
                        exported[b2].add(s)
        # vector widths
        width = collections.defaultdict(int)
        kind = {}
        isvec = set()
        for s, b in sym_blk.items():
            base, i = vbase(s)
            if i is not None:
                isvec.add(base)
                width[base] = max(width[base], i + 1)
            elif s in decl_w and decl_w[s] > 1:
                width[base] = decl_w[s]
            kind[base] = 'reg' if s in {self.nm(q) for q in D.regs} else 'wire'
        ports_out = [p for p in D.ports_out]
        mods = []
        for b in blocks:
            mod = 'cpld_' + re.sub(r'\W', '_', b)
            ins = sorted(s for s in uses[b] if s not in owned[b] and s not in kw and (s in pad_ins or s == 'osc' or any(s in owned[b2] for b2 in blocks)))
            outs = sorted(exported[b] | {p for p, v, bb in top_assign if bb == b})
            L = ['// %s' % self.cfg.get('modules', {}).get(b, b), '// generated by tools/machxo3/xo3lift.py; netlist cells in trailing comments', 'module %s (' % mod]
            pl = []
            def wdecl(s):
                w = width.get(s, 0)
                return ('[%d:0] ' % (w - 1)) if (w > 1 or s in isvec) else ''
            for s in ins:
                pl.append('  input  %s%s' % (wdecl(s), s))
            for s in outs:
                pl.append('  output %s%s' % (wdecl(s), s))
            L.append(',\n'.join(pl))
            L.append(');')
            for s in sorted(owned[b]):
                if s in [p for p in ports_out]:
                    continue
                L.append('%s %s%s;' % (kind.get(s, 'wire'), wdecl(s), s))
            L.append('')
            L.extend(defs[b])
            L.append('')
            L.extend(stmts[b])
            L.extend(pad_lines[b])
            L.append('endmodule')
            open(os.path.join(outdir, '%s.v' % re.sub(r'\W', '_', b)), 'w').write('\n'.join(L) + '\n')
            mods.append((b, mod, ins, outs))
        for q in D.regs:
            regmap[q] = 'u_%s.%s' % (re.sub(r'\W', '_', regblk[q]), self.nm(q))
            if D.regs[q]['kind'] == 'latch':
                regmap[q + '_hold'] = regmap[q] + '_hold'
        # top
        T = ['// generated by tools/machxo3/xo3lift.py: pads + module instances', 'module top (']
        T.append(',\n'.join(['  input  osc'] + ['  input  %s' % p for p in D.ports_in if p != 'osc'] + ['  output %s' % p for p in D.ports_out]))
        T.append(');')
        allsig = set()
        for b, mod, ins, outs in mods:
            allsig |= set(outs)
        for s in sorted(allsig - set(D.ports_out)):
            w = width.get(s, 0)
            T.append('wire %s%s;' % (('[%d:0] ' % (w - 1)) if (w > 1 or s in isvec) else '', s))
        for b, mod, ins, outs in mods:
            conns = ['.%s(%s)' % (s, s) for s in ins + outs]
            T.append('%s u_%s (%s);' % (mod, re.sub(r'\W', '_', b), ', '.join(conns)))
        T.append('endmodule')
        open(os.path.join(outdir, 'top.v'), 'w').write('\n'.join(T) + '\n')
        json.dump(regmap, open(os.path.join(outdir, 'regmap.json'), 'w'), indent=1, sort_keys=True)
        netblk = {q: regblk[q] for q in D.regs}
        for k, (name, _, b, e, w, src) in self.auto.items():
            if isinstance(k, str) and k in D.vexpr:
                netblk[k] = b
            elif isinstance(k, str) and k in getattr(self, 'chain_inc', {}):
                for h in self.chain_inc[k][0]:
                    netblk[h + '_F'] = b
        for p, v, b in top_assign:
            netblk[p] = b
        json.dump(netblk, open(os.path.join(outdir, 'netblocks.json'), 'w'), indent=1, sort_keys=True)
        print('blocks %d, regs %d, wires %d, incrementer chains %d' % (len(blocks), len(D.regs), len(self.auto), len(getattr(self, 'chain_inc', {}))))

    def share_wire(self, e, b, pfx):
        key = ('expr', e)
        if key in self.auto:
            return self.auto[key][0]
        name = '%s_%d' % (pfx, sum(1 for k in self.auto if isinstance(k, tuple)))
        for n, (nm_, _, _, ee, _, _) in self.auto.items():
            pass
        self.auto[key] = (name, None, b, e, 1, None)
        self.order.append(key)
        return name


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('sim'); ap.add_argument('names'); ap.add_argument('outdir')
    ap.add_argument('--maxsop', type=int, default=8)
    a = ap.parse_args()
    cfg = json.load(open(a.names)) if os.path.exists(a.names) else {}
    Lifter(a.sim, cfg, a.maxsop).run(a.outdir)


if __name__ == '__main__':
    main()
