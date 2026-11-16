#!/usr/bin/env python3
"""netlist.json (xo3net.py) -> structural Verilog of what is on the chip. Generic.

  xo3vlog.py NETLIST.json OUT.v [--names NAMES.json] [--report REPORT.txt]

Only live logic is emitted: the backward cone of the output/bidir pads, of
the hard blocks' fabric inputs (EFB, GSR, OSC) and of anything named in
--names as a root.  Everything else is counted as dead in the report.

Cell semantics (MachXO2/3 slice, as fuzzed by Project Trellis; choices that
the database cannot distinguish are marked in comments of the output):
  LUT4      F = INIT[{D,C,B,A}]; emitted as a minimised sum of products.
            (Checked: no LUT in a design depends on an unrouted input.)
  PFUMX     K0.OFX = M ? F(K1) : F(K0);   L6MUX K1.OFX = M ? FXB : FXA
  CCU2      per half: P = LUT4(INIT), G = LUT2(A,B) with INIT ~INIT[15:12]
            (yosys CCU2D); S = P ^ (INJECT1=YES ? 0 : CIN);
            COUT = P ? CIN : (INJECT1=YES ? 0 : G).  Unrouted inputs = 0.
  FF        D = (REG.SD == 0) ? M : DI; clock CLK (CLKMUX INV: falling edge);
            CE routed or 1; LSR routed (LSRMUX INV inverts, LSRONMUX 0 = none);
            SRMODE ASYNC or LSR_OVER_CE (sync, wins over CE); set/reset value
            REGSET (Trellis default SET); GSR ENABLED (default) also loads it;
            REGMODE LATCH = transparent latch while the clock is high.
  IOLOGIC   TS register when the PIO's TRIMUX_TSREG = IOLTO; input register
            when IN is used (CLKIMUX/CLKOMUX CLK vs INV share one bit in the
            database: polarity is not known, rising edge is assumed).
Net names: the driving pin, R<y>C<x>_<bel>_<pin>, or a name from --names.
"""
import json, sys, re, argparse
from collections import defaultdict, Counter


def minimise(init, names):
    """Quine-McCluskey for <= 4 inputs; init: int (16 bits), names[i] for input i (A=0)."""
    n = 4
    ones = [m for m in range(16) if (init >> m) & 1]
    if not ones:
        return "1'b0"
    if len(ones) == 16:
        return "1'b1"
    # implicants as (value, mask) with mask bits = don't care
    terms = {(m, 0) for m in ones}
    primes = set()
    while terms:
        nxt = set()
        used = set()
        tl = sorted(terms)
        for i, (v1, m1) in enumerate(tl):
            for v2, m2 in tl[i + 1:]:
                if m1 == m2 and bin(v1 ^ v2).count('1') == 1:
                    nxt.add((v1 & v2, m1 | (v1 ^ v2)))
                    used.add((v1, m1)); used.add((v2, m2))
        primes |= terms - used
        terms = nxt
    # cover (greedy after essentials)
    def covers(p, m):
        return (m & ~p[1]) == (p[0] & ~p[1])
    left = set(ones)
    chosen = []
    while left:
        ess = None
        for m in sorted(left):
            c = [p for p in primes if covers(p, m)]
            if len(c) == 1:
                ess = c[0]; break
        if ess is None:
            ess = max(primes, key=lambda p: (sum(1 for m in left if covers(p, m)), bin(p[1]).count('1')))
        chosen.append(ess)
        left = {m for m in left if not covers(ess, m)}
    out = []
    for v, m in sorted(chosen, key=lambda p: (-bin(p[1]).count('1'), p)):
        lits = []
        for i in range(n):
            if not (m >> i) & 1:
                lits.append(names[i] if (v >> i) & 1 else '~' + names[i])
        out.append(' & '.join(lits) if lits else "1'b1")
    return ' | '.join('(%s)' % t if ' & ' in t and len(out) > 1 else t for t in out)


def vname(net, names):
    if net in names:
        return names[net]
    return re.sub(r'[^A-Za-z0-9_]', '_', net)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('netlist'); ap.add_argument('out')
    ap.add_argument('--names', help='JSON {net or cell.pin: name}')
    ap.add_argument('--report')
    ap.add_argument('--sim', action='store_true',
                    help='simulation view: pads split into <pad>_in/_out/_oe, OSCH output = input port osc, '
                         'other hard-block outputs tied to 1')
    a = ap.parse_args()
    d = json.load(open(a.netlist))
    cells, nets = d['cells'], d['nets']
    names = json.load(open(a.names)) if a.names else {}

    def pin(c, p):
        return cells[c]['pins'].get(p)

    # which input pins an output pin depends on (mode-aware)
    def deps(cn, op):
        c = cells[cn]
        t, cf, pins = c['type'], c['cfg'], c['pins']
        k0 = cn.endswith('.K0')
        if t == 'TRELLIS_COMB':
            mode = cf.get('MODE', 'LOGIC')
            lut = [p for p in 'ABCD' if p in pins] + (['FCI'] if mode == 'CCU2' else [])
            if op in ('F', 'FCO'):
                return lut
            if op == 'OFX':
                return (['M', 'F1'] + lut) if k0 else ['M', 'FXA', 'FXB']
            return []
        if t == 'TRELLIS_FF':
            return ['CLK', 'CE', 'LSR', 'M' if cf.get('SD') == '0' else 'DI']
        if t == 'PIO':
            return []
        if 'IOLOGIC' in t:
            return {'IOLTO': ['CLK', 'CE', 'LSR', 'TS'], 'IN': ['CLK', 'CE', 'LSR', 'DI'],
                    'IP': ['CLK', 'CE', 'LSR', 'DI'], 'IOLDO': ['CLK', 'CE', 'LSR', 'OPOS', 'ONEG'],
                    'INDD': ['DI']}.get(op, [])
        return [p for p, dd in c['dirs'].items() if dd == 'in']

    # roots: (cell, input pin)
    root_pins = []
    for cn, c in cells.items():
        bt = c['cfg'].get('BASE_TYPE', '')
        if c['type'] == 'PIO' and (bt.startswith('OUTPUT') or bt.startswith('BIDIR')):
            root_pins += [(cn, 'I'), (cn, 'T')]
        if c['type'] in ('EFB', 'GSR', 'OSCH', 'PCNTR', 'SEDFA', 'JTAGF', 'START', 'TSALL', 'DP8KC', 'EHXPLLJ', 'DCCA'):
            root_pins += [(cn, p) for p, dd in c['dirs'].items() if dd == 'in']
    used_in = set()   # nets that feed live logic
    live_out = set()  # (cell, outpin)
    stack = list(root_pins)
    while stack:
        cn, p = stack.pop()
        net = cells[cn]['pins'].get(p)
        if not net or net.startswith('undriven:') or net in used_in:
            continue
        used_in.add(net)
        drv, op = net.rsplit('.', 1)
        if drv in cells and (drv, op) not in live_out:
            live_out.add((drv, op))
            for q in deps(drv, op):
                stack.append((drv, q))
    live = {cn for cn, _ in live_out} | {cn for cn, _ in root_pins}

    def inputs(cn):
        return [p for p in cells[cn]['pins'] if cells[cn]['dirs'].get(p) == 'in']

    W = []
    w = W.append
    stats = Counter()
    ports = []
    decl = set()
    body = []

    def N(net):
        return vname(net, names)

    def ref(cn, p, default="1'b0"):
        net = pin(cn, p)
        if not net or net.startswith('undriven:'):
            return default
        decl.add(net)
        return N(net)

    def outnet(cn, p):
        net = '%s.%s' % (cn, p)
        decl.add(net)
        return N(net)

    # pads
    pads = []
    for cn, c in sorted(cells.items()):
        if c['type'] != 'PIO':
            continue
        bt = c['cfg'].get('BASE_TYPE', '')
        o = '%s.O' % cn
        if cn in live or o in used_in:
            pads.append(cn)
    for cn in pads:
        c = cells[cn]
        bt = c['cfg'].get('BASE_TYPE', 'NONE')
        pn = names.get(cn, cn)
        o = '%s.O' % cn
        attrs = ', '.join('%s=%s' % kv for kv in sorted(c['cfg'].items()))
        stats['pad ' + bt] += 1
        drives = cn in live and ('I' in c['pins'] or 'T' in c['pins'])
        if a.sim:
            ports.append('input  %s_in /* %s */' % (pn, attrs))
            if drives:
                ports.append('output %s_out' % pn)
                ports.append('output %s_oe' % pn)
        else:
            ports.append('inout  %s /* %s */' % (pn, attrs))
        if o in used_in:
            decl.add(o)
            body.append('assign %s = %s%s;' % (N(o), pn, '_in' if a.sim else ''))
        if drives:
            i = ref(cn, 'I', "1'b0")
            t = ref(cn, 'T', "1'b0")
            if a.sim:
                body.append('assign %s_out = %s;' % (pn, i))
                body.append('assign %s_oe = ~%s;' % (pn, t))
            elif bt.startswith('OUTPUT') and 'T' not in c['pins']:
                body.append('assign %s = %s;' % (pn, i))
            else:
                body.append('assign %s = %s ? 1\'bz : %s;' % (pn, t, i))

    ffs = []
    for cn in sorted(live):
        c = cells[cn]
        t, cf = c['type'], c['cfg']
        if t == 'TRELLIS_COMB':
            mode = cf.get('MODE', 'LOGIC')
            init = int(cf.get('INIT', '0' * 16), 2)
            k0 = cn.endswith('.K0')
            if mode == 'LOGIC':
                if '%s.F' % cn in used_in or '%s.OFX' % cn in used_in:
                    nm = [ref(cn, p) for p in 'ABCD']
                    body.append('assign %s = %s; // %s INIT=16\'h%04x' % (outnet(cn, 'F'), minimise(init, nm), cn, init))
                    stats['LUT4'] += 1
            elif mode == 'CCU2' and ('%s.F' % cn in used_in or '%s.FCO' % cn in used_in or '%s.OFX' % cn in used_in):
                inj = cf.get('INJECT1', 'YES')
                nm = [ref(cn, p) for p in 'ABCD']
                p_ = minimise(init, nm)
                g_init = 0
                hi = (~(init >> 12)) & 0xF
                g = minimise(hi | (hi << 4) | (hi << 8) | (hi << 12), nm)
                cin = ref(cn, 'FCI')
                gate = "1'b0" if inj == 'YES' else cin
                body.append('// %s CCU2 INIT=16\'h%04x INJECT1=%s' % (cn, init, inj))
                body.append('wire %s_P = %s;' % (N(cn), p_))
                body.append('wire %s_G = %s;' % (N(cn), g if inj != 'YES' else "1'b0"))
                body.append('assign %s = %s_P ^ %s;' % (outnet(cn, 'F'), N(cn), gate))
                body.append('assign %s = %s_P ? %s : %s_G;' % (outnet(cn, 'FCO'), N(cn), cin, N(cn)))
                stats['CCU2 half'] += 1
            if '%s.OFX' % cn in used_in:
                if k0:
                    body.append('assign %s = %s ? %s : %s; // PFUMX' % (outnet(cn, 'OFX'), ref(cn, 'M'), ref(cn, 'F1'), outnet(cn, 'F')))
                    stats['PFUMX'] += 1
                else:
                    body.append('assign %s = %s ? %s : %s; // L6MUX' % (outnet(cn, 'OFX'), ref(cn, 'M'), ref(cn, 'FXB'), ref(cn, 'FXA')))
                    stats['L6MUX'] += 1
        elif t == 'TRELLIS_FF' and '%s.Q' % cn in used_in:
            if 'CLK' not in c['pins']:
                stats['FF without clock'] += 1
            ffs.append(cn)
    for cn in ffs:
        c = cells[cn]
        cf = c['cfg']
        q = outnet(cn, 'Q')
        dsrc = ref(cn, 'M') if cf.get('SD') == '0' else ref(cn, 'DI')
        clk = ref(cn, 'CLK', None)
        inv = cf.get('CLK.CLKMUX') == 'INV'
        ce = ref(cn, 'CE', "1'b1") if 'CE' in c['pins'] else "1'b1"
        lsr = None
        if cf.get('LSR.LSRONMUX') != '0' and 'LSR' in c['pins']:
            lsr = ref(cn, 'LSR')
            if cf.get('LSR.LSRMUX') == 'INV':
                lsr = '~' + lsr
        sync = cf.get('LSR.SRMODE') != 'ASYNC'
        val = "1'b0" if cf.get('REGSET') == 'RESET' else "1'b1"
        gsr = cf.get('GSR') != 'DISABLED'
        latch = cf.get('REGMODE') == 'LATCH'
        decl.add('%s.Q' % cn)
        tag = '// %s %s%s%s%s' % (cn, 'LATCH ' if latch else '', 'negedge ' if inv else '', 'sync ' if sync else 'async ', 'gsr' if gsr else 'nogsr')
        init = 'initial %s = %s;' % (q, val)
        if clk is None:
            body.append('%s %s // no clock routed' % (init, tag)); continue
        edge = ('negedge ' if inv else 'posedge ') + clk
        rst = []
        if gsr:
            rst.append('gsr')
        if lsr and not sync:
            rst.append(lsr)
        if latch:
            en = ('~' if inv else '') + clk
            r = ' | '.join(rst) if rst else None
            body.append('%s %s' % (init, tag))
            body.append('always @* if (%s) %s = %s; else if (%s%s) %s = %s;' % (
                r or "1'b0", q, val, en, (' & %s' % ce) if ce != "1'b1" else '', q, dsrc))
            stats['latch'] += 1
            continue
        sens = [edge] + ['posedge ' + (r[1:] if r.startswith('~') else r) if not r.startswith('~') else 'negedge ' + r[1:] for r in rst]
        lines = []
        cond = []
        if rst:
            cond.append(('if (%s) %s <= %s;' % (' | '.join(rst), q, val)))
        if lsr and sync:
            cond.append(('if (%s) %s <= %s;' % (lsr, q, val)))
        cond.append(('if (%s) %s <= %s;' % (ce, q, dsrc)) if ce != "1'b1" else ('%s <= %s;' % (q, dsrc)))
        s_ = ' else '.join(cond)
        body.append('%s %s' % (init, tag))
        body.append('always @(%s) %s' % (' or '.join(sens), s_))
        stats['FF %s' % ('negedge' if inv else 'posedge')] += 1
        stats['FF %s' % ('sync-LSR' if (lsr and sync) else 'async-LSR' if lsr else 'no-LSR')] += 1
    # IOLOGIC registers
    for cn in sorted(live):
        c = cells[cn]
        if 'IOLOGIC' not in c['type']:
            continue
        cf = c['cfg']
        clk = ref(cn, 'CLK', None)
        val_ts = "1'b1" if cf.get('TSREG.REGSET') == 'SET' else "1'b0"
        gsr = cf.get('GSR') != 'DISABLED'
        lsr = ref(cn, 'LSR', None) if 'LSR' in c['pins'] else None
        if '%s.IOLTO' % cn in used_in:
            q = outnet(cn, 'IOLTO')
            ce = ref(cn, 'CE', "1'b1") if cf.get('CEOMUX') != '1' and 'CE' in c['pins'] else "1'b1"
            rst = (['gsr'] if gsr else []) + ([lsr] if lsr and cf.get('LSROMUX') == 'LSRMUX' else [])
            sync = cf.get('SRMODE') == 'LSR_OVER_CE'
            body.append('initial %s = %s; // %s TS register (clock polarity assumed rising)' % (q, val_ts, cn))
            sens = ['posedge ' + clk] + ([] if sync else ['posedge ' + r for r in rst])
            pre = ('if (%s) %s <= %s; else ' % (' | '.join(rst), q, val_ts)) if rst else ''
            body.append('always @(%s) %sif (%s) %s <= %s;' % (' or '.join(sens), pre, ce, q, ref(cn, 'TS')))
            stats['IOL TS reg'] += 1
        if '%s.IN' % cn in used_in:
            q = outnet(cn, 'IN')
            val = "1'b1" if cf.get('FF.REGSET') == 'SET' else "1'b0"
            ce = "1'b1" if cf.get('CEIMUX') == '1' else ref(cn, 'CE', "1'b1")
            rst = (['gsr'] if gsr else []) + ([lsr] if lsr and cf.get('LSRIMUX') == 'LSRMUX' else [])
            body.append('initial %s = %s; // %s input register (clock polarity assumed rising)' % (q, val, cn))
            pre = ('if (%s) %s <= %s; else ' % (' | '.join(rst), q, val)) if rst else ''
            body.append('always @(posedge %s) %sif (%s) %s <= %s;' % (clk, pre, ce, q, ref(cn, 'DI')))
            stats['IOL input reg'] += 1
    # hard blocks as black-box instances
    for cn in sorted(live):
        c = cells[cn]
        if c['type'] in ('EFB', 'GSR', 'OSCH', 'JTAGF', 'PCNTR', 'SEDFA', 'START', 'TSALL', 'DP8KC', 'EHXPLLJ', 'DCCA'):
            conns = []
            for p in sorted(c['pins']):
                if c['dirs'][p] == 'out':
                    net = '%s.%s' % (cn, p)
                    if net in used_in:
                        decl.add(net)
                        conns.append('.%s(%s)' % (p, N(net)))
                else:
                    r = ref(cn, p, None)
                    if r:
                        conns.append('.%s(%s)' % (p, r))
            if a.sim:
                for p in sorted(c['pins']):
                    net = '%s.%s' % (cn, p)
                    if c['dirs'][p] == 'out' and net in used_in:
                        body.append('assign %s = %s;' % (N(net), 'osc' if c['type'] == 'OSCH' and p == 'OSC' else "1'b1"))
                continue
            if conns or c['cfg']:
                prm = ', '.join('.%s("%s")' % (k.replace('.', '_'), v) for k, v in sorted(c['cfg'].items()))
                body.append('%s %s%s (%s);' % (c['type'], ('#(%s) ' % prm) if prm else '', N(cn), ', '.join(conns)))
                stats['hard ' + c['type']] += 1
    w('// generated by tools/machxo3/xo3vlog.py from %s' % a.netlist)
    w('module top (')
    if a.sim:
        ports.insert(0, 'input  osc /* OSCH output */')
    w(',\n'.join('  ' + p for p in ports))
    w(');')
    gsrc = [cn for cn in cells if cells[cn]['type'] == 'GSR']
    gnet = cells[gsrc[0]]['pins'].get('GSR') if gsrc else None
    if gnet and not gnet.startswith('undriven:'):
        mode = cells[gsrc[0]]['cfg'].get('GSRMODE', 'ACTIVE_LOW')
        decl.add(gnet)
        gsr_line = 'wire gsr = %s%s; // GSR block, GSRMODE=%s' % ('' if mode == 'ACTIVE_HIGH' else '~', N(gnet), mode)
    else:
        gsr_line = "wire gsr = 1'b0; // GSR block input not routed"
    for net in sorted(decl):
        if net.endswith('.Q') or net.endswith('.IOLTO') or net.endswith('.IN'):
            if '%s' % net.rsplit('.', 1)[0] in cells and ('FF' in net or 'IOLOGIC' in cells[net.rsplit('.', 1)[0]]['type']):
                w('reg %s;' % N(net)); continue
        w('wire %s;' % N(net))
    w(gsr_line)
    for l in body:
        w(l)
    w('endmodule')
    open(a.out, 'w').write('\n'.join(W) + '\n')
    dead = Counter(cells[cn]['type'] for cn in cells if cn not in live and (cells[cn]['cfg']))
    rep = ['live cells: %d of %d' % (len(live), len(cells)), 'emitted: ' + ', '.join('%s=%d' % kv for kv in sorted(stats.items())),
           'dead (configured but outside the live cone): ' + ', '.join('%s=%d' % kv for kv in sorted(dead.items()))]
    if a.report:
        open(a.report, 'w').write('\n'.join(rep) + '\n')
    print('\n'.join(rep))


if __name__ == '__main__':
    main()
