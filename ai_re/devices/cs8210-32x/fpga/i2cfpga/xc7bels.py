#!/usr/bin/env python3
"""Layer 4b: physical netlist (fasmnet.py) -> logical primitives (UNISIM-like).

    xc7bels.py logic IN.net.json OUT.logic.json     cells -> primitives + logical nets
    xc7bels.py verilog IN.logic.json OUT.v          structural Verilog view

This is a *view* of the lossless netlist, never an input to the rebuild: the
FASM lines stay owned by the cells/nets of fasmnet.py.  Each primitive records
the cell it came from; each FASM feature of a SLICE/BRAM/IOI cell is checked to
be consumed by the decoder ("unconsumed" lists anything the decoder ignored).

Decoding rules follow prjxray feature semantics (same as chipsalliance
f4pga-xc-fasm2bels clb_models.py / ioi_models.py / iob_models.py):
  FF type   (FFSYNC, LATCH, xFF.ZRST) -> FDRE/FDSE/FDCE/FDPE/LDCE/LDPE,
            INIT = !xFF.ZINI, CE = CEUSEDMUX ? CE : 1, SR = SRUSEDMUX ? SR : 0,
            IS_C_INVERTED = CLKINV
  CARRY4    CI = PRECYINIT.{CIN,AX,C1} (none = C0), DI[i] = CARRY4.xCY0 ? xO5 : xX,
            S[i] = xO6
  MUXF7A    I0 = BO6, I1 = AO6, S = AX;  MUXF7B I0 = DO6, I1 = CO6, S = CX;
  MUXF8     I0 = F7B, I1 = F7A, S = BX
  OLOGIC    OMUX.D1 -> OQ = D1 (bypass); DATA_RATE_TQ.BUF -> TQ = ZINV_T1 ? T1 : ~T1
  ILOGIC    ZINV_D (no ISERDES/IDDR) -> O = D (bypass)
"""
import collections
import gzip
import itertools
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

FF_TYPES = {
    (False, False, False): ('FDPE', 'PRE'),
    (True, False, False): ('FDSE', 'S'),
    (True, False, True): ('FDRE', 'R'),
    (False, False, True): ('FDCE', 'CLR'),
    (False, True, True): ('LDCE', 'CLR'),
    (False, True, False): ('LDPE', 'PRE'),
}


def open_any(path, mode='rt'):
    return gzip.open(path, mode) if path.endswith('.gz') else open(path, mode)


def fasm_value(v):
    v = v.strip()
    if "'" in v:
        w, rest = v.split("'", 1)
        return int(rest[1:].replace('_', ''), {'b': 2, 'h': 16, 'd': 10, 'o': 8}[rest[0]])
    return int(v)


RE_RANGE = re.compile(r'^(.*)\[(\d+)(?::(\d+))?\]$')


def cell_features(cell):
    """{feature-after-site: int value} for a site cell's FASM lines.  Bit-range
    features (NAME[hi:lo] = v, NAME[i]) are merged into one integer NAME."""
    out = {}
    for l in cell['fasm']:
        k, _, v = l.partition('=')
        parts = k.strip().split('.')
        skip = 3 if parts[1] in ('BUFGCTRL', 'BUFHCE') else 2
        name = '.'.join(parts[skip:])
        val = fasm_value(v) if v else 1
        m = RE_RANGE.match(name)
        if m:
            lo = int(m.group(3) if m.group(3) is not None else m.group(2))
            out[m.group(1)] = out.get(m.group(1), 0) | (val << lo)
        else:
            out[name] = val
    return out


# ------------------------------------------------------------------ LUT functions
def lut_support(init, n=6):
    """Inputs (1-based) the n-input function depends on."""
    sup = []
    for i in range(n):
        for a in range(1 << n):
            if not a >> i & 1 and (init >> a & 1) != (init >> (a | 1 << i) & 1):
                sup.append(i + 1)
                break
    return sup


def lut_fold(init, n, consts):
    """n-input table with the inputs in consts {1-based pin: 0/1} fixed (the
    result no longer depends on them)."""
    t = 0
    for a in range(1 << n):
        b = a
        for i, v in consts.items():
            b = (b | 1 << (i - 1)) if v else (b & ~(1 << (i - 1)))
        if init >> b & 1:
            t |= 1 << a
    return t


def lut_reduce(init, sup, n=6):
    """Truth table over the support inputs only (input k of result = sup[k])."""
    t = 0
    for a in range(1 << len(sup)):
        full = 0
        for k, i in enumerate(sup):
            if a >> k & 1:
                full |= 1 << (i - 1)
        if init >> full & 1:
            t |= 1 << a
    return t


_PCANON = {}


def lut_pcanon(t, k):
    """Permutation-canonical form of a k-input truth table (min over input orders)."""
    key = (t, k)
    r = _PCANON.get(key)
    if r is not None:
        return r
    best = None
    for perm in itertools.permutations(range(k)):
        u = 0
        for a in range(1 << k):
            if t >> a & 1:
                b = 0
                for j in range(k):
                    if a >> j & 1:
                        b |= 1 << perm[j]
                u |= 1 << b
        if best is None or u < best:
            best = u
    _PCANON[key] = best
    return best


def lut_expr(t, names):
    """Small readable equation: sum of minterms for <=3 inputs, else hex table."""
    k = len(names)
    if k == 0:
        return "1'b%d" % (t & 1)
    ones = [a for a in range(1 << k) if t >> a & 1]
    if len(ones) == 1 << k:
        return "1'b1"
    if not ones:
        return "1'b0"
    if len(ones) == 1:
        a = ones[0]
        return ' & '.join(('' if a >> j & 1 else '~') + names[j] for j in range(k))
    if len(ones) == (1 << k) - 1:
        a = [x for x in range(1 << k) if not t >> x & 1][0]
        return ' | '.join(('~' if a >> j & 1 else '') + names[j] for j in range(k))
    if k == 2 and sorted(ones) == [1, 2]:
        return '%s ^ %s' % tuple(names)
    if k <= 3:
        return ' | '.join('(' + ' & '.join(('' if a >> j & 1 else '~') + names[j] for j in range(k)) + ')'
                          for a in ones)
    return "%d'h%x[{%s}]" % (1 << k, t, ','.join(reversed(names)))


# ------------------------------------------------------------------ builder
class Logic:
    def __init__(self, net):
        self.phys = net
        self.cells = {c['name']: c for c in net['cells']}
        self.pnets = {n['name']: n for n in net['nets']}
        self.prims = []
        self.unconsumed = collections.Counter()
        self.unconsumed_ex = {}
        self.alias = {}

    def pnet(self, cell, pin):
        """Logical net name of a site pin (None = not connected)."""
        n = cell['pins'].get(pin)
        if n is None:
            return None
        pn = self.pnets.get(n)
        if pn and pn['const']:
            return 'GND' if pn['driver'][0] == '$GND' else 'VCC'
        return n

    def add(self, name, prim, cell, bel, params, conns, **extra):
        p = dict(name=name, prim=prim, cell=cell['name'], bel=bel, params=params,
                 conns={k: v for k, v in conns.items() if v is not None}, **extra)
        self.prims.append(p)
        return p

    def consume_check(self, cell, F, used):
        for k in F:
            if k not in used:
                key = (cell['type'], re.sub(r'\[\d+:\d+\]', '', k))
                self.unconsumed[key] += 1
                self.unconsumed_ex.setdefault(key, cell['name'])

    # ---------------------------------------------------------------- SLICE
    def slice(self, cell):
        F = cell_features(cell)
        used = set()
        s = cell['name']

        def has(k):
            if k in F:
                used.add(k)
                return True
            return False

        def outnet(pin, internal):
            n = self.pnet(cell, pin)
            return n if n else '%s/%s' % (s, internal)

        P = lambda p: self.pnet(cell, p)  # noqa: E731
        for k in ('NOCLKINV',):
            has(k)
        clkinv = has('CLKINV')
        ce = P('CE') if has('CEUSEDMUX') else 'VCC'
        sr = P('SR') if has('SRUSEDMUX') else 'GND'
        clk = P('CLK')
        ffsync = has('FFSYNC')
        latch = has('LATCH')
        sig = {}   # internal signal -> net name
        # outputs exposed on pins
        for L in 'ABCD':
            sig[L + 'O6'] = outnet(L, L + 'O6')
        # which internal signals the output muxes expose
        outmux = {}
        for L in 'ABCD':
            for opt in ('O6', 'O5', 'XOR', 'CY', 'F7', 'F8', L + '5Q', 'MC31'):
                if has('%sOUTMUX.%s' % (L, opt)):
                    outmux[L] = opt
        ffmux = {}
        for L in 'ABCD':
            for opt in ('O6', 'O5', 'XOR', 'CY', 'F7', 'F8', L + 'X', 'MC31'):
                if has('%sFFMUX.%s' % (L, opt)):
                    ffmux[L] = opt
        f5mux = {}
        for L in 'ABCD':
            for opt in ('IN_A', 'IN_B'):
                if has('%s5FFMUX.%s' % (L, opt)):
                    f5mux[L] = opt
        precy = None
        for opt in ('CIN', 'AX', 'C0', 'C1'):
            if has('PRECYINIT.' + opt):
                precy = opt
        cy0 = {L: has('CARRY4.%sCY0' % L) for L in 'ABCD'}

        def internal(L, opt):
            """net for an internal signal selected by a mux option."""
            if opt == 'O6':
                return sig[L + 'O6']
            if opt == 'O5':
                return sig.setdefault(L + 'O5', '%s/%sO5' % (s, L))
            if opt == 'XOR':
                return sig.setdefault(L + '_XOR', '%s/%s_XOR' % (s, L))
            if opt == 'CY':
                return sig.setdefault(L + '_CY', '%s/%s_CY' % (s, L))
            if opt == 'F7':
                return sig.setdefault('F7' + {'A': 'A', 'C': 'B'}[L], '%s/F7%sMUX_O' % (s, {'A': 'A', 'C': 'B'}[L]))
            if opt == 'F8':
                return sig.setdefault('F8', '%s/F8MUX_O' % s)
            if opt == L + '5Q':
                return sig.setdefault(L + '5Q', '%s/%s5Q' % (s, L))
            if opt == 'MC31':
                return sig.setdefault('MC31', '%s/DMC31' % s)
            if opt == L + 'X':
                return P(L + 'X')
            raise KeyError(opt)

        # name the signals exposed on xMUX pins with the physical net name
        for L, opt in outmux.items():
            n = self.pnet(cell, L + 'MUX')
            key = {'O5': L + 'O5', 'XOR': L + '_XOR', 'CY': L + '_CY', 'F8': 'F8', L + '5Q': L + '5Q',
                   'MC31': 'MC31', 'F7': 'F7' + {'A': 'A', 'C': 'B'}.get(L, '?')}.get(opt)
            if n is None:
                continue
            if opt == 'O6':
                self.alias[n] = sig[L + 'O6']
            elif key in sig:
                self.alias[n] = sig[key]
            else:
                sig[key] = n
        if self.pnet(cell, 'COUT') and False:
            sig['D_CY'] = self.pnet(cell, 'COUT')
        for L in 'ABCD':
            if self.pnet(cell, L + 'Q'):
                sig[L + 'Q'] = self.pnet(cell, L + 'Q')

        cout = self.pnet(cell, 'COUT')
        cout_used = cout is not None and any(self.cin_used.get(c) for c, p in self.pnets[cout]['sinks'])
        # PRECYINIT.C0 has no set bits and is emitted for almost every slice
        carry = precy in ('CIN', 'AX', 'C1') or any(o in ('XOR', 'CY') for o in list(outmux.values()) + list(ffmux.values())) \
            or any(cy0.values()) or cout_used
        need_f8 = 'F8' in outmux.values() or 'F8' in ffmux.values()
        need_f7a = need_f8 or outmux.get('A') == 'F7' or ffmux.get('A') == 'F7'
        need_f7b = need_f8 or outmux.get('C') == 'F7' or ffmux.get('C') == 'F7'
        wa7, wa8 = has('WA7USED'), has('WA8USED')

        # LUTs
        for L in 'ABCD':
            ik = '%sLUT.INIT' % L
            init = F.get(ik, 0)
            if ik in F:
                used.add(ik)
            ram, srl, small = has(L + 'LUT.RAM'), has(L + 'LUT.SRL'), has(L + 'LUT.SMALL')
            dimux = None
            for opt in (L + 'I', 'BDI1_BMC31', 'DI_CMC31', 'DI_DMC31'):
                if has('%sLUT.DI1MUX.%s' % (L, opt)):
                    dimux = opt
            o6_used = (P(L) is not None or outmux.get(L) == 'O6' or ffmux.get(L) == 'O6' or carry
                       or (L in 'AB' and need_f7a) or (L in 'CD' and need_f7b))
            o5_used = (outmux.get(L) == 'O5' or ffmux.get(L) == 'O5' or f5mux.get(L) == 'IN_A'
                       or (carry and cy0[L]))
            ins = {'A%d' % i: P('%s%d' % (L, i)) for i in range(1, 7)}
            if ram or srl:
                prim = ('SRL16E' if small else 'SRLC32E') if srl else ('RAM32X1S' if small else 'RAM64X1S')
                d = {L + 'I': P(L + 'I'), 'BDI1_BMC31': '%s/BMC31' % s, 'DI_CMC31': '%s/CMC31' % s,
                     'DI_DMC31': '%s/DMC31' % s}.get(dimux, P('DI') if L == 'D' else None)
                we = P('CE') if has('WEMUX.CE') else P('WE')
                conns = dict(ins, D=d, CLK=clk, WE=we, O6=sig[L + 'O6'])
                if srl:
                    conns['MC31'] = sig.setdefault('MC31', '%s/DMC31' % s) if L == 'D' else '%s/%sMC31' % (s, L)
                if o5_used:
                    conns['O5'] = internal(L, 'O5')
                self.add('%s/%sLUT' % (s, L), prim, cell, L + '6LUT',
                         dict(INIT='%d\'h%016x' % (64, init), MODE='SRL' if srl else 'RAM', SMALL=small,
                              DI1MUX=dimux, IS_CLK_INVERTED=int(clkinv)), conns)
                continue
            if not (o6_used or o5_used or init):
                continue
            params = {'INIT': "64'h%016x" % init}
            conns = dict(ins)
            if o6_used or not o5_used:
                conns['O6'] = sig[L + 'O6']
            if o5_used:
                conns['O5'] = internal(L, 'O5')
            prim = 'LUT6_2' if 'O5' in conns and 'O6' in conns else ('LUT5' if 'O5' in conns else 'LUT6')
            # fold inputs tied to VCC/GND (A6 = VCC in a fractured LUT6_2, unused
            # IMUX pins at their default VCC) into the function before taking
            # its support
            consts = {i: (1 if ins['A%d' % i] == 'VCC' else 0) for i in range(1, 7)
                      if ins['A%d' % i] in ('VCC', 'GND')}
            fn = {}
            if 'O6' in conns:
                t6 = lut_fold(init, 6, consts)
                sup6 = lut_support(t6, 6)
                fn['O6'] = dict(support=sup6, table=lut_reduce(t6, sup6))
            if 'O5' in conns:
                t5 = lut_fold(init & 0xffffffff, 5, {i: v for i, v in consts.items() if i <= 5})
                sup5 = lut_support(t5, 5)
                fn['O5'] = dict(support=sup5, table=lut_reduce(t5, sup5, 5))
            self.add('%s/%sLUT' % (s, L), prim, cell, L + '6LUT', params, conns, fn=fn)

        # F7/F8
        if need_f7a or wa7:
            self.add('%s/F7AMUX' % s, 'MUXF7', cell, 'F7AMUX', {},
                     dict(I0=sig['BO6'], I1=sig['AO6'], S=P('AX'), O=internal('A', 'F7')))
        if need_f7b:
            self.add('%s/F7BMUX' % s, 'MUXF7', cell, 'F7BMUX', {},
                     dict(I0=sig['DO6'], I1=sig['CO6'], S=P('CX'), O=internal('C', 'F7')))
        if need_f8 or wa8:
            self.add('%s/F8MUX' % s, 'MUXF8', cell, 'F8MUX', {},
                     dict(I0=internal('C', 'F7'), I1=internal('A', 'F7'), S=P('BX'), O=internal('B', 'F8')))
        # CARRY4
        if carry:
            if precy == 'CIN':
                conns = {'CI': P('CIN'), 'CYINIT': 'GND'}
            else:
                conns = {'CI': 'GND', 'CYINIT': {'AX': P('AX'), 'C1': 'VCC', 'C0': 'GND', None: 'GND'}[precy]}
            for i, L in enumerate('ABCD'):
                conns['DI[%d]' % i] = internal(L, 'O5') if cy0[L] else P(L + 'X')
                conns['S[%d]' % i] = sig[L + 'O6']
                if (L + '_XOR') in sig or outmux.get(L) == 'XOR' or ffmux.get(L) == 'XOR':
                    conns['O[%d]' % i] = internal(L, 'XOR')
                if (L + '_CY') in sig or outmux.get(L) == 'CY' or ffmux.get(L) == 'CY':
                    conns['CO[%d]' % i] = internal(L, 'CY')
            if cout_used:
                conns['CO[3]'] = cout
                if 'D_CY' in sig and outmux.get('D') == 'CY' and sig.get('D_CY') != conns['CO[3]']:
                    self.alias[sig['D_CY']] = conns['CO[3]']
            self.add('%s/CARRY4' % s, 'CARRY4', cell, 'CARRY4', dict(PRECYINIT=precy or 'C0'), conns)
        # FFs
        for L in 'ABCD':
            z = (has(L + 'FF.ZINI'), has(L + 'FF.ZRST'))
            if L in ffmux or self.pnet(cell, L + 'Q') or any(z):
                prim, srpin = FF_TYPES[(ffsync, latch, z[1])]
                d = internal(L, ffmux[L]) if L in ffmux else None
                q = sig.get(L + 'Q') or '%s/%sQ' % (s, L)
                ports = ('G', 'GE') if latch else ('C', 'CE')
                self.add('%s/%sFF' % (s, L), prim, cell, L + 'FF',
                         {'INIT': int(not z[0]), ('IS_G_INVERTED' if latch else 'IS_C_INVERTED'):
                          int(not clkinv) if latch else int(clkinv)},
                         {'D': d, ports[0]: clk, ports[1]: ce, srpin: sr, 'Q': q})
            z5 = (has(L + '5FF.ZINI'), has(L + '5FF.ZRST'))
            if L in f5mux or outmux.get(L) == L + '5Q' or any(z5):
                prim, srpin = FF_TYPES[(ffsync, False, z5[1])]
                d = internal(L, 'O5') if f5mux.get(L) == 'IN_A' else P(L + 'X')
                self.add('%s/%s5FF' % (s, L), prim, cell, L + '5FF',
                         {'INIT': int(not z5[0]), 'IS_C_INVERTED': int(clkinv)},
                         {'D': d, 'C': clk, 'CE': ce, srpin: sr, 'Q': internal(L, L + '5Q')})
        self.consume_check(cell, F, used)

    # ---------------------------------------------------------------- others
    def generic(self, cell, prim, F=None, used=None, skip_pins=()):
        """Site-level primitive: ports = site pins, params = FASM features."""
        F = cell_features(cell) if F is None else F
        conns = {}
        for p in cell['pins']:
            if p in skip_pins:
                continue
            conns[p] = self.pnet(cell, p)
        return self.add(cell['name'], prim, cell, cell['type'],
                        {k: v for k, v in sorted(F.items())}, conns)

    def iob(self, cell):
        F = cell_features(cell)
        o, t, i = self.pnet(cell, 'O'), self.pnet(cell, 'T'), self.pnet(cell, 'I')
        drive = any('.DRIVE.' in k for k in F)
        inp = any(k.endswith('.IN') or k.endswith('.IN_ONLY') for k in F)
        # input used = the pad's I net reaches the fabric through ILOGIC O
        il = self.ilogic_of.get(i)
        has_i = i is not None and il is not None and 'O' in self.cells[il]['pins']
        tconst = self.t_of_iob.get(cell['name'])
        if drive and o is not None and has_i and tconst != 'always-on':
            prim = 'IOBUF'
        elif drive and o is not None and tconst != 'always-on':
            prim = 'OBUFT'
        elif drive and o is not None:
            prim = 'OBUF'
        elif has_i:
            prim = 'IBUF'
        else:
            prim = 'IOB_CFG_ONLY'
        std = sorted({k.split('.')[0] for k in F if '.DRIVE.' in k or k.endswith('.IN')})
        params = dict(FEATURES=sorted(F), IOSTANDARD_GROUP=std, IN_FEATURE=int(inp),
                      READBACK=int(prim == 'OBUF' and has_i),
                      DRIVE=[k.split('.')[-1] for k in F if '.DRIVE.' in k],
                      SLEW=[k.split('.')[-1] for k in F if '.SLEW.' in k],
                      PULLTYPE=[k.split('.')[-1] for k in F if k.startswith('PULLTYPE.')])
        conns = {'O': i}   # UNISIM: O = to fabric
        if not has_i:
            del conns['O']
        if prim in ('IOBUF', 'OBUFT', 'OBUF'):
            conns['I'] = o
        if prim in ('IOBUF', 'OBUFT'):
            conns['T'] = t
        conns['PAD'] = 'PAD:' + cell['name']
        self.add(cell['name'], prim, cell, 'IOB', params, conns)

    def ologic(self, cell):
        F = cell_features(cell)
        if 'OMUX.D1' in F and 'OSERDES.DATA_RATE_TQ.BUF' in F and not any(k.startswith('OSERDES.IN_USE') for k in F):
            d1, t1 = self.pnet(cell, 'D1'), self.pnet(cell, 'T1')
            oq, tq = self.pnet(cell, 'OQ'), self.pnet(cell, 'TQ')
            if oq and d1:
                self.alias[oq] = d1
            inv = 'ZINV_T1' not in F
            if tq:
                if inv:
                    if t1 in (None, 'VCC'):
                        self.t_const[tq] = 'always-on'
                        self.add(cell['name'] + '/TINV', 'TIE', cell, 'OLOGIC', {'VALUE': 0}, {'O': tq})
                    else:
                        self.add(cell['name'] + '/TINV', 'INV', cell, 'OLOGIC', {}, {'I': t1, 'O': tq})
                elif t1:
                    self.alias[tq] = t1
            return
        self.generic(cell, 'OLOGICE3')

    def ilogic(self, cell):
        F = cell_features(cell)
        if not any('IN_USE' in k for k in F):
            # bypass: placed ILOGIC (ZINV_D set) or the D->O route-through of an
            # unused site (no ZINV_D; that path has no inverter) -- see LAYER4.md
            o, d = self.pnet(cell, 'O'), self.pnet(cell, 'D')
            if o and d:
                self.alias[o] = d
            return
        if cell['pins'].get('O'):
            self.generic(cell, 'ILOGICE3')

    def bram(self, cell):
        F = cell_features(cell)
        if 'IN_USE' not in F:
            if cell['pins']:
                self.generic(cell, 'RAMB18E1_UNUSED')
            return
        sdp = 'SDP_READ_WIDTH_36' in F
        rw = [k for k in F if k.startswith('READ_WIDTH_A_')]
        prim = 'RAMB18E1'
        params = dict(sorted(F.items()))
        params['RAM_MODE'] = 'SDP' if sdp else 'TDP'
        conns = {p: self.pnet(cell, p) for p in cell['pins']}
        self.add(cell['name'], prim, cell, cell['type'], params, conns, width=rw)

    def build(self):
        self.cin_used = {c['name']: any(l.endswith('.PRECYINIT.CIN') for l in c['fasm'])
                         for c in self.cells.values() if c['type'] in ('SLICEL', 'SLICEM')}
        self.t_const = {}
        self.t_of_iob = {}
        self.ilogic_of = {}
        for c in self.cells.values():
            if c['type'] == 'ILOGICE3' and 'D' in c['pins']:
                self.ilogic_of[c['pins']['D']] = c['name']
        order = sorted(self.cells.values(), key=lambda c: (c['type'] not in ('OLOGICE3',), c['name']))
        for c in order:
            t = c['type']
            if c['kind'] != 'site':
                continue
            if t in ('SLICEL', 'SLICEM'):
                self.slice(c)
            elif t == 'OLOGICE3':
                self.ologic(c)
            elif t == 'ILOGICE3':
                self.ilogic(c)
            elif t in ('IOB33', 'IOB33S', 'IOB33M'):
                tn = c['pins'].get('T')
                self.t_of_iob[c['name']] = self.t_const.get(tn)
                self.iob(c)
            elif t in ('RAMB18E1', 'FIFO18E1'):
                self.bram(c)
            elif t == 'IDELAYE2':
                F = cell_features(c)
                # IDELAY_TYPE_FIXED alone is the unused default; ILOGIC IDELMUXE3.P1
                # (seen everywhere here) selects D, not DDLY
                if set(F) - {'IDELAY_TYPE_FIXED'}:
                    self.generic(c, 'IDELAYE2')
            elif t == 'BUFGCTRL':
                self.generic(c, 'BUFGCTRL')
            elif t == 'BUFHCE':
                self.generic(c, 'BUFHCE')
            elif t == 'MMCME2_ADV':
                self.generic(c, 'MMCME2_ADV')
            elif t == 'PCIE_2_1':
                self.generic(c, 'PCIE_2_1')
            elif t == 'GTPE2_CHANNEL':
                self.generic(c, 'GTPE2_CHANNEL')
            elif t == 'GTPE2_COMMON':
                self.generic(c, 'GTPE2_COMMON')
            elif t == 'IBUFDS_GTE2':
                self.generic(c, 'IBUFDS_GTE2')
            elif t == 'IPAD':
                self.add(c['name'], 'IPAD', c, 'IPAD', {}, {'O': self.pnet(c, 'O'), 'PAD': 'PAD:' + c['name']})
            else:
                self.generic(c, t)
        return self.finish()

    def finish(self):
        # resolve aliases (chains) and build logical nets
        def res(n):
            seen = set()
            while n in self.alias and n not in seen:
                seen.add(n)
                n = self.alias[n]
            return n
        nets = collections.defaultdict(lambda: dict(driver=None, sinks=[]))
        outs = {'O', 'O5', 'O6', 'Q', 'MC31'} | {'O[%d]' % i for i in range(4)} | {'CO[%d]' % i for i in range(4)}
        for p in self.prims:
            for port, n in list(p['conns'].items()):
                n = res(n)
                p['conns'][port] = n
                if n.startswith('PAD:'):
                    continue
                is_out = self.is_output(p, port, outs)
                if is_out:
                    if nets[n]['driver'] is not None and n not in ('VCC', 'GND'):
                        nets[n].setdefault('extra_drivers', []).append([p['name'], port])
                    else:
                        nets[n]['driver'] = [p['name'], port]
                else:
                    nets[n]['sinks'].append([p['name'], port])
        return dict(prims=self.prims, nets=dict(nets),
                    unconsumed={'%s:%s' % k: v for k, v in self.unconsumed.items()},
                    unconsumed_examples={'%s:%s' % k: v for k, v in self.unconsumed_ex.items()},
                    aliases=len(self.alias))

    def is_output(self, p, port, outs):
        if p['prim'] in ('LUT6', 'LUT5', 'LUT6_2', 'CARRY4', 'MUXF7', 'MUXF8', 'INV', 'TIE', 'IPAD',
                         'SRL16E', 'SRLC32E', 'RAM32X1S', 'RAM64X1S') or p['prim'].startswith(('FD', 'LD')):
            return port in outs
        if p['prim'] in ('IBUF', 'IOBUF', 'OBUF', 'OBUFT'):
            return port == 'O'
        c = self.cells[p['cell']]
        return self.pin_dir(c, port) == 'OUT'

    _dirs = None

    def pin_dir(self, cell, pin):
        if Logic._dirs is None:
            sys.path.insert(0, HERE)
            import xc7graph
            Logic._dirs = xc7graph.Fabric().site_types
        return Logic._dirs.get(cell['type'], {}).get(pin, '?')


# ------------------------------------------------------------------ verilog
def vid(n):
    if re.match(r'^[A-Za-z_][A-Za-z0-9_]*$', n):
        return n
    return '\\' + n + ' '


def verilog(logic, out, title='i2cfpga'):
    prims = logic['prims']
    nets = logic['nets']
    pads = sorted({v for p in prims for v in p['conns'].values() if v.startswith('PAD:')})
    with open(out, 'w') as f:
        f.write('// Structural view generated by xc7bels.py from the lossless layer-4 netlist.\n')
        f.write('// Primitive names follow Xilinx UNISIM where the mapping is direct; site-level\n')
        f.write('// hard blocks keep site pin names as ports.  This file is a view: it is not\n')
        f.write('// used to rebuild the bitstream (the FASM lines stay with fasmnet.py cells/nets).\n')
        f.write('module %s (\n' % title)
        f.write(',\n'.join('  inout %s' % vid(p) for p in pads))
        f.write('\n);\n')
        for n in sorted(nets):
            if n in ('VCC', 'GND'):
                continue
            f.write('  wire %s;\n' % vid(n))
        f.write("  wire VCC = 1'b1;\n  wire GND = 1'b0;\n")
        for p in prims:
            params = p['params']
            ps = []
            for k, v in params.items():
                if k == 'FEATURES':
                    continue
                if isinstance(v, int) and not isinstance(v, bool):
                    ps.append('.%s(%d)' % (re.sub(r'\W', '_', k), v))
                else:
                    ps.append('.%s("%s")' % (re.sub(r'\W', '_', k), str(v).replace('"', "'")))
            comment = ''
            if p.get('fn'):
                eq = []
                for o, fn in p['fn'].items():
                    names = ['%s%d' % ('I', i - 1) for i in fn['support']]
                    eq.append('%s = %s' % (o, lut_expr(fn['table'], names)))
                comment = '  // ' + '; '.join(eq)
            f.write('  %s %s%s (%s);%s\n' % (
                p['prim'], ('#(' + ', '.join(ps) + ') ') if ps and len(ps) < 40 else '',
                vid(p['name']),
                ', '.join('.%s(%s)' % (re.sub(r'\W', '_', k), vid(v)) for k, v in sorted(p['conns'].items())),
                comment))
        f.write('endmodule\n')


def main():
    a = sys.argv[1:]
    if len(a) < 3:
        raise SystemExit(__doc__)
    if a[0] == 'logic':
        net = json.load(open_any(a[1]))
        L = Logic(net)
        res = L.build()
        json.dump(res, open_any(a[2], 'wt'), separators=(',', ':'))
        c = collections.Counter(p['prim'] for p in res['prims'])
        print('primitives: %d' % len(res['prims']))
        print(dict(c.most_common()))
        print('logical nets: %d, aliases folded: %d' % (len(res['nets']), res['aliases']))
        print('nets without driver: %d, with >1 driver: %d, without sinks: %d' % (
            sum(1 for n in res['nets'].values() if n['driver'] is None),
            sum(1 for n in res['nets'].values() if n.get('extra_drivers')),
            sum(1 for n in res['nets'].values() if not n['sinks'])))
        print('unconsumed features:', res['unconsumed'])
    elif a[0] == 'verilog':
        verilog(json.load(open_any(a[1])), a[2])
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main()
