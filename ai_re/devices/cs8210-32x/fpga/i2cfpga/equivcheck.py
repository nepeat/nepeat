#!/usr/bin/env python3
"""Formal equivalence of the comparable part of the two channel templates.

    equivcheck.py L6DIR OUTDIR [--roots pads|all-common] [--realign NET LOGIC BLOCKS]

Region: the fan-in cone, inside family B's template, of the two pad
drivers (or of every common element with --roots all-common), followed only
through elements that have a family-A counterpart (template.json
family_map_B_to_A); family A's region is the image of that set.  Everything
that enters the region from outside becomes a primary input.  A cut input is
named after the B net; the A net that feeds the corresponding A element pin
is paired with it.  Where that pairing is not one to one (an A net would have
to equal two different B nets, or the reverse), the input is split and the
case is counted ("pairing conflicts"); a proof is still sound, a failure then
may be spurious.

Writes OUTDIR/eqB.v, eqA.v (modules with identical port lists: cut inputs +
the compared outputs = every region element output), eq.ys (yosys script:
miter -equiv, sat -tempinduct) and a report.  FFs: INIT as initial value,
CLR/PRE/R/S applied at the clock edge on both sides (as in simnet.py
--sync-reset); one clock.
"""
import collections
import json
import os
import re
import sys


def vid(x):
    return 'n_' + re.sub(r'[^A-Za-z0-9_]', '_', x)


def build(tpl, F, elems, inmap, outs):
    """Verilog for family F's elements; inmap: (eid, port) -> input name for cut
    inputs; outs: [(name, tnet)]"""
    E = tpl['families'][F]['elements']
    L = []
    ins = sorted(set(inmap.values()))
    L.append('module eq%s(input clk, %s, %s);' % (F, ', '.join('input %s' % i for i in ins),
                                                ', '.join('output %s' % o for o, t in outs)))
    regs, decl, body = set(), [], []

    def sig(eid, port, t):
        if t in ('VCC', 'GND', None):
            return "1'b1" if t == 'VCC' else "1'b0"
        if (eid, port) in inmap:
            return inmap[(eid, port)]
        return vid(t)
    nets = set()
    for eid in elems:
        e = E[eid]
        k = e['kind']
        if k == 'LUT':
            o = vid(eid + '.O')
            nets.add(o)
            ins_ = [sig(eid, 'I%d' % q, t) for q, t in enumerate(e['inputs'])]
            if ins_:
                body.append("assign %s = %d'h%x >> {%s};" % (o, 1 << len(ins_), e['table'], ', '.join(reversed(ins_))))
            else:
                body.append("assign %s = 1'b%d;" % (o, e['table'] & 1))
        elif k.startswith('FD'):
            q = vid(eid + '.Q')
            regs.add(q)
            decl.append("reg %s = 1'b%d;" % (q, e['init']))
            P = e['ports']
            d = sig(eid, 'D', P.get('D'))
            ce = sig(eid, 'CE', e['ce'])
            srp = e.get('srpin')
            sr = sig(eid, srp, e['sr']) if srp else "1'b0"
            rv = 1 if k in ('FDPE', 'FDSE') else 0
            body.append("always @(posedge clk) if (%s) %s <= 1'b%d; else if (%s) %s <= %s;" % (sr, q, rv, ce, q, d))
        elif k == 'CARRY4':
            P = e['ports']
            c = sig(eid, 'CI', P.get('CI')) if P.get('CI') not in (None, 'GND') else sig(eid, 'CYINIT', P.get('CYINIT'))
            prev = c
            for i in range(4):
                s, di = sig(eid, 'S[%d]' % i, P.get('S[%d]' % i)), sig(eid, 'DI[%d]' % i, P.get('DI[%d]' % i))
                cn = vid('%s.CO[%d]' % (eid, i))
                on = vid('%s.O[%d]' % (eid, i))
                nets.update([cn, on])
                body.append('assign %s = %s ^ %s;' % (on, s, prev))
                body.append('assign %s = %s ? %s : %s;' % (cn, s, prev, di))
                prev = cn
        elif k in ('MUXF7', 'MUXF8'):
            P = e['ports']
            o = vid(eid + '.O')
            nets.add(o)
            body.append('assign %s = %s ? %s : %s;' % (o, sig(eid, 'S', P.get('S')), sig(eid, 'I1', P.get('I1')),
                                                       sig(eid, 'I0', P.get('I0'))))
    for o, t in outs:
        body.append('assign %s = %s;' % (o, vid(t)))
    L += ['wire %s;' % n for n in sorted(nets - regs)] + decl + body + ['endmodule']
    return '\n'.join(L) + '\n'


def pad_roles(T):
    """[sda_pad, scl_pad] template elements: SDA's release FF has a 3-input OR
    as next state, SCL's a 4-input function (layer 7)"""
    E = T['elements']
    pads = [e for e in T['order'] if E[e]['kind'] == 'IOBUF'][:2]

    def k(p):
        ff = E[E[p]['ports']['T'].split('.')[0]]
        return E[ff['ports']['D'].split('.')[0]]['k']
    return pads if (k(pads[0]), k(pads[1])) == (3, 4) else pads[::-1]


def realign(tpl, net_path, logic_path, blocks_path):
    """B->A element correspondence from a fresh alignment of the two
    reference instances, seeded with the pads by role (the template's
    family map comes from an alignment that tries both pad orientations and
    keeps the larger match, which pairs the symmetric pad structures
    crosswise)."""
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import modules
    net, logic, ann = (json.load(modules.open_any(p)) for p in (net_path, logic_path, blocks_path))
    X = modules.Extractor(net, logic, ann, 'eq')
    TA, TB = tpl['families']['A'], tpl['families']['B']
    RA, RB = int(TA['reference'][2:]), int(TB['reference'][2:])
    sb, sa = pad_roles(TB), pad_roles(TA)
    seeds = [(TB['elements'][sb[0]]['ref'], TA['elements'][sa[0]]['ref']),
             (TB['elements'][sb[1]]['ref'], TA['elements'][sa[1]]['ref'])]
    m, ties, relab = X.A.align(RB, RA, seeds)
    nodeA = {e['ref']: eid for eid, e in TA['elements'].items()}
    out = {}
    for eid, e in TB['elements'].items():
        a = nodeA.get(m.get(e['ref']))
        if a and TA['elements'][a]['label'] == e['label']:
            out[eid] = a
    old = tpl['family_map_B_to_A']
    print('realigned B->A map: %d pairs (template family map %d; %d differ)' % (
        len(out), len(old), sum(1 for k, v in out.items() if old.get(k) != v)), file=sys.stderr)
    return out


def main():
    l6, out = sys.argv[1], sys.argv[2]
    roots_mode = 'pads'
    strict = '--strict' in sys.argv
    if '--roots' in sys.argv:
        roots_mode = sys.argv[sys.argv.index('--roots') + 1]
    tpl = json.load(open(os.path.join(l6, 'template.json')))
    fm = tpl['family_map_B_to_A']
    if '--realign' in sys.argv:
        fm = realign(tpl, *sys.argv[sys.argv.index('--realign') + 1:][:3])
    EB, EA = tpl['families']['B']['elements'], tpl['families']['A']['elements']
    LOGIC = ('LUT', 'FDCE', 'FDPE', 'FDRE', 'FDSE', 'CARRY4', 'MUXF7', 'MUXF8')

    def inputs(e):
        if e['kind'] == 'LUT':
            return [('I%d' % q, t) for q, t in enumerate(e['inputs'])]
        ps = dict(e.get('ports', {}))
        if e['kind'].startswith('FD'):
            ps = {k: v for k, v in ps.items() if k != 'C'}
        return list(ps.items())

    def drv(t):
        return t.split('.')[0] if t and '.' in t else None
    if roots_mode == 'pads':
        pads = [x for x in EB if EB[x]['kind'] == 'IOBUF']
        roots = [drv(EB[p]['ports']['T']) for p in pads]
    else:
        roots = [x for x in fm if EB[x]['kind'] in LOGIC]
    region = set()
    st = [r for r in roots if r in fm]
    while st:
        x = st.pop()
        if x in region or x not in fm or EB[x]['kind'] not in LOGIC or EA[fm[x]]['kind'] != EB[x]['kind']:
            continue
        region.add(x)
        for p, t in inputs(EB[x]):
            d = drv(t)
            if d and d in EB:
                st.append(d)
    regA = {fm[x] for x in region}
    # LUTs: reorder A's inputs to B's order where the pairing is a symmetry of the function
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import modgraph
    EA = {k: dict(v) for k, v in EA.items()}
    reord = 0
    for x in sorted(region):
        eb, ea = EB[x], EA[fm[x]]
        if eb['kind'] != 'LUT' or eb['inputs'] == ea['inputs']:
            continue
        want = [(fm.get(drv(t)) + '.' + t.split('.', 1)[1]) if drv(t) in fm else None for t in eb['inputs']]
        perm, used = [], set()
        for q, w in enumerate(want):
            cand = [j for j, t in enumerate(ea['inputs']) if t == w and j not in used] if w else []
            if not cand:
                cand = [j for j in range(len(ea['inputs'])) if j not in used and j == q] or \
                       [j for j in range(len(ea['inputs'])) if j not in used]
            perm.append(cand[0])
            used.add(cand[0])
        # new A input q = old A input perm[q]; table must stay the same function
        inv = [0] * len(perm)
        for q, j in enumerate(perm):
            inv[j] = q
        t2 = modgraph.permute_table(ea['table'], len(perm), inv)
        if t2 == eb['table']:
            ea['inputs'] = [ea['inputs'][j] for j in perm]
            reord += 1
    tpl['families']['A']['elements'] = EA
    # cut inputs: B pin whose net is not driven inside the region
    inB, inA = {}, {}
    pairs = collections.defaultdict(set)     # B net -> A nets
    rpairs = collections.defaultdict(set)
    sinks = []
    for x in sorted(region):
        a = fm[x]
        pb, pa = dict(inputs(EB[x])), dict(inputs(EA[a]))
        for p, tb in pb.items():
            ta = pa.get(p)
            db, da = drv(tb), drv(ta)
            inside_b = db in region
            inside_a = da in regA
            if inside_b and inside_a and fm.get(db) == da and tb.split('.', 1)[1] == ta.split('.', 1)[1]:
                continue                     # internal, consistent
            if tb in ('VCC', 'GND') and ta == tb:
                continue
            sinks.append((x, a, p, tb, ta, inside_b, inside_a))
            pairs[tb].add(ta)
            rpairs[ta].add(tb)
    conflicts = 0
    cat = collections.Counter()
    examples = collections.defaultdict(list)
    for x, a, p, tb, ta, ib, ia in sinks:
        if ib and ia:
            c = 'both inside, different driver'
        elif ib:
            c = 'B inside, A outside'
        elif ia:
            c = 'A inside, B outside'
        elif not (len(pairs[tb]) == 1 and len(rpairs[ta]) == 1):
            c = 'outside, not one-to-one'
        else:
            c = None
        if c:
            cat[c] += 1
            if len(examples[c]) < 4:
                examples[c].append('%s.%s: B %s / A %s (A driver %s, kind %s)' % (
                    x, p, tb, ta, drv(ta), EA[drv(ta)]['kind'] if drv(ta) in EA else '-'))
        if strict and ib and ia:
            continue                         # keep each side's own internal driver (checked, not assumed)
        if ib or ia:
            # one side internal, the other not (or inconsistent): cut both sides at this pin
            name = 'cut_%s_%s' % (x, re.sub(r'\W', '_', p))
            conflicts += 1
        elif len(pairs[tb]) == 1 and len(rpairs[ta]) == 1:
            name = 'in_' + re.sub(r'\W', '_', tb)
        else:
            name = 'cut_%s_%s' % (x, re.sub(r'\W', '_', p))
            conflicts += 1
        inB[(x, p)] = name
        inA[(a, p)] = name
    outs = []
    for x in sorted(region):
        k = EB[x]['kind']
        if k.startswith('FD'):
            outs.append(('o_%s' % x, (x + '.Q', fm[x] + '.Q')))
        elif k in ('LUT', 'MUXF7', 'MUXF8'):
            outs.append(('o_%s' % x, (x + '.O', fm[x] + '.O')))
    os.makedirs(out, exist_ok=True)
    open(os.path.join(out, 'eqB.v'), 'w').write(build(tpl, 'B', sorted(region), inB, [(o, t[0]) for o, t in outs]))
    open(os.path.join(out, 'eqA.v'), 'w').write(build(tpl, 'A', sorted(regA), inA, [(o, t[1]) for o, t in outs]))
    ys = """read_verilog eqB.v
read_verilog eqA.v
proc
opt_clean
miter -equiv -flatten -make_assert -ignore_gold_x eqB eqA miter
hierarchy -top miter
flatten
opt -fast
sat -verify -tempinduct -prove-asserts -enable_undef -set-init-def -maxsteps 40 miter
"""
    open(os.path.join(out, 'eq.ys'), 'w').write(ys)
    kinds = collections.Counter(EB[x]['kind'] for x in region)
    rep = dict(strict=strict, lut_inputs_reordered=reord, roots=roots_mode, region_elements=len(region), kinds=dict(kinds), cut_inputs=len(set(inB.values())),
               pairing_conflicts=conflicts, conflict_kinds=dict(cat), conflict_examples=dict(examples),
               compared_outputs=len(outs))
    json.dump(rep, open(os.path.join(out, 'report.json'), 'w'), indent=1)
    print(rep)


if __name__ == '__main__':
    main()
