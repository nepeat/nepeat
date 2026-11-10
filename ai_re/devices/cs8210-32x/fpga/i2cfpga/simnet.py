#!/usr/bin/env python3
"""Simulation netlist of the whole user-clock design (a view of layer 4b).

    simnet.py verilog LOGIC.json OUT.v [--map OUT.map.json] [--sync-reset]

--sync-reset applies CLR/PRE at the clock edge instead of asynchronously
(the asynchronous form does not settle in Verilator after USERRSTN is
released: several FFs clear themselves through logic, e.g. the cb+0x0C bit-31
pulse; with it, a self-clearing pulse lasts one clock instead of a few ns).

Writes one Verilog module `top` for Verilator from the logical primitives
(xc7bels.py):

  - LUT outputs (O6, O5): table lookups over their support inputs
  - FDCE/FDPE (async clear/preset), FDRE/FDSE (sync), INIT values; only FFs
    on the user clock (BUFGCTRL_X0Y0 tree) are clocked, FFs on other clocks
    (PIPE/DRP/refclk logic) are held at INIT (V: the user logic has no other
    clock; I: holding the PIPE side constant does not matter for TLP-level
    simulation because the TRN pins are driven by the testbench)
  - CARRY4, MUXF7/8 as gates
  - RAMB18E1 in SDP mode (the channel BRAMs): behavioural 512 x 36,
    synchronous read on port A, byte-enabled write on port B (I: the
    WEBWE pin -> byte mapping (byte k <- WEBWE[2k]) is taken from the
    engine's own write pattern in simulation)
  - PCIE_2_1: its fabric-facing outputs become top-level inputs `pcie_<PIN>`,
    its fabric-driven inputs top-level outputs `pcie_<PIN>`; pads: IBUF and
    IOBUF inputs -> `pin_<site>` inputs, IOBUF T / OBUF I -> `t_<site>` /
    `o_<site>` outputs.

The map file gives net -> Verilog identifier, for probes from a testbench.
This is a view for simulation, not used for the byte round trip.
"""
import collections
import gzip
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def open_any(path, mode='rt'):
    return gzip.open(path, mode) if path.endswith('.gz') else open(path, mode)


CLOCK = ('BUFGCTRL', 'BUFHCE')


def build(logic, sync_reset=False):
    P = {p['name']: p for p in logic['prims']}
    N = logic['nets']
    ids = {}
    bmap = {}

    def w(n):
        if n in ('VCC', None):
            return "1'b1" if n == 'VCC' else "1'b0"
        if n == 'GND':
            return "1'b0"
        if n not in ids:
            ids[n] = 'w%d' % len(ids)
        return ids[n]

    # user clock nets: BUFHCE/BUFGCTRL outputs rooted at BUFGCTRL_X0Y0
    def root(n, d=0):
        dr = N.get(n, {}).get('driver')
        if not dr or d > 6:
            return None
        p = P[dr[0]]
        if p['prim'] == 'BUFHCE':
            return root(p['conns'].get('I'), d + 1)
        if p['prim'] == 'BUFGCTRL':
            return p['name']
        return None
    clocknets = {n for n, v in N.items() if v['driver'] and P[v['driver'][0]]['prim'] in CLOCK}
    userclk = {n for n in clocknets if root(n) == 'BUFGCTRL_X0Y0'}
    decl, body, ports_in, ports_out = [], [], [], []
    driven = set()
    regs = set()
    for n in userclk:
        driven.add(n)
    nff = nlut = ncarry = nbram = 0
    for p in logic['prims']:
        t = p['prim']
        c = p['conns']
        if p.get('fn'):
            for o, fn in p['fn'].items():
                out = c.get(o)
                if out is None or out in driven:
                    continue
                ins = [c.get('A%d' % i) for i in fn['support']]
                k = len(ins)
                if k == 0:
                    body.append("assign %s = 1'b%d;" % (w(out), fn['table'] & 1))
                else:
                    body.append("assign %s = %d'h%x >> {%s};" % (
                        w(out), 1 << k, fn['table'], ', '.join(w(x) for x in reversed(ins))))
                driven.add(out)
                nlut += 1
        elif t.startswith('FD'):
            q = c.get('Q')
            if q is None or q in driven:
                continue
            regs.add(q)
            driven.add(q)
            init = p['params']['INIT']
            decl.append("reg %s = 1'b%d;" % (w(q), init))
            clk = c.get('C')
            if clk not in userclk:
                continue
            d, ce = w(c.get('D')), w(c.get('CE', 'VCC'))
            edge = 'negedge' if p['params'].get('IS_C_INVERTED') else 'posedge'
            asy = '' if sync_reset else ' or posedge %s'
            if t == 'FDCE' and c.get('CLR') not in (None, 'GND'):
                r = w(c['CLR'])
                body.append(('always @(%s clk' + ((asy % r) if asy else '') + ') if (%s) %s <= 1\'b0; else if (%s) %s <= %s;') % (
                    edge, r, w(q), ce, w(q), d))
            elif t == 'FDPE' and c.get('PRE') not in (None, 'GND'):
                r = w(c['PRE'])
                body.append(('always @(%s clk' + ((asy % r) if asy else '') + ') if (%s) %s <= 1\'b1; else if (%s) %s <= %s;') % (
                    edge, r, w(q), ce, w(q), d))
            elif t in ('FDRE', 'FDSE') and c.get('R', c.get('S')) not in (None, 'GND'):
                r = w(c.get('R', c.get('S')))
                body.append('always @(%s clk) if (%s) %s <= 1\'b%d; else if (%s) %s <= %s;' % (
                    edge, r, w(q), 1 if t == 'FDSE' else 0, ce, w(q), d))
            else:
                body.append('always @(%s clk) if (%s) %s <= %s;' % (edge, ce, w(q), d))
            nff += 1
        elif t == 'CARRY4':
            ci = c.get('CI', 'GND')
            cy = w(ci) if ci not in ('GND',) else w(c.get('CYINIT', 'GND'))
            prev = cy
            for i in range(4):
                s, di = w(c.get('S[%d]' % i, 'GND')), w(c.get('DI[%d]' % i, 'GND'))
                o, co = c.get('O[%d]' % i), c.get('CO[%d]' % i)
                cn = 'c4_%d_%d' % (ncarry, i)
                decl.append('wire %s;' % cn)
                body.append('assign %s = %s ? %s : %s;' % (cn, s, prev, di))
                if o and o not in driven:
                    body.append('assign %s = %s ^ %s;' % (w(o), s, prev))
                    driven.add(o)
                if co and co not in driven:
                    body.append('assign %s = %s;' % (w(co), cn))
                    driven.add(co)
                prev = cn
            ncarry += 1
        elif t in ('MUXF7', 'MUXF8'):
            o = c.get('O')
            if o and o not in driven:
                body.append('assign %s = %s ? %s : %s;' % (w(o), w(c.get('S')), w(c.get('I1')), w(c.get('I0'))))
                driven.add(o)
        elif t == 'RAMB18E1' and p['params'].get('RAM_MODE') == 'SDP':
            g = lambda *names: next((c[x] for x in names if x in c), None)  # noqa: E731
            m = 'mem%d' % nbram
            bmap['#bram:' + p['name']] = m
            decl.append('reg [31:0] %s [0:511];' % m)
            wa = '{%s}' % ', '.join(w(c.get('ADDRBWRADDR%d' % i, 'GND')) for i in range(13, 4, -1))
            ra = '{%s}' % ', '.join(w(c.get('ADDRARDADDR%d' % i, 'GND')) for i in range(13, 4, -1))
            din = '{%s, %s}' % (', '.join(w(c.get('DIBDI%d' % i, 'GND')) for i in range(15, -1, -1)),
                                ', '.join(w(c.get('DIADI%d' % i, 'GND')) for i in range(15, -1, -1)))
            wen = w(g('ENBWREN', 'WREN'))
            ren = w(g('ENARDEN', 'RDEN'))
            dout = 'do%d' % nbram
            decl.append('reg [31:0] %s = 32\'d0;' % dout)
            # byte k written when WEBWE[k] (pins 0..3); see the module docstring
            # byte k <- WEBWE[2k] (the site doubles the SDP byte enables; seen in
            # simulation: the engine raises pins 2k and 2k+1 together)
            we = [w(c.get('WEBWE%d' % (2 * k), 'GND')) for k in range(4)]
            body.append(('always @(posedge clk) begin if (%s) %s <= %s[%s]; if (%s) begin '
                         'if (bram_debug && (%s)) $display("[bram %s] wa=%%0d din=%%08x we=%%b%%b%%b%%b we4..7=%%b%%b%%b%%b", %s, %s, %s, %s, %s, %s, %s, %s, %s, %s); %s end end') % (
                ren, dout, m, ra, wen, ' | '.join([w(c.get('WEBWE%d' % k, 'GND')) for k in range(8)]), p['name'], wa, din,
                *[w(c.get('WEBWE%d' % k, 'GND')) for k in (3, 2, 1, 0, 7, 6, 5, 4)], ' '.join('if (%s) %s[%s][%d:%d] <= %s[%d:%d];' % (
                    we[k], m, wa, 8 * k + 7, 8 * k, din, 8 * k + 7, 8 * k) for k in range(4))))
            for i in range(32):
                # FIFO18E1-type site: DO0..31; RAMB18E1-type site: DOADO = data[15:0], DOBDO = data[31:16]
                o = c.get('DO%d' % i) or c.get('DOADO%d' % i if i < 16 else 'DOBDO%d' % (i - 16))
                if o and o not in driven:
                    body.append('assign %s = %s[%d];' % (w(o), dout, i))
                    driven.add(o)
            nbram += 1
        elif t == 'PCIE_2_1':
            for pin, n in sorted(c.items()):
                if n in ('VCC', 'GND'):
                    continue
                dr = N.get(n, {}).get('driver')
                if dr and dr[0] == p['name']:
                    if N[n]['sinks'] and n not in driven:
                        ports_in.append('pcie_%s' % pin)
                        body.append('assign %s = pcie_%s;' % (w(n), pin))
                        driven.add(n)
                elif n not in userclk:
                    ports_out.append('pcie_%s' % pin)
                    body.append('assign pcie_%s = %s;' % (pin, w(n)))
        elif t in ('IBUF', 'IOBUF'):
            o = c.get('O')
            site = p['name']
            if o and o not in driven:
                ports_in.append('pin_%s' % site)
                body.append('assign %s = pin_%s;' % (w(o), site))
                driven.add(o)
            if t == 'IOBUF':
                ports_out.append('t_%s' % site)
                body.append('assign t_%s = %s;' % (site, w(c.get('T'))))
        elif t in ('OBUF', 'OBUFT'):
            ports_out.append('o_%s' % p['name'])
            body.append('assign o_%s = %s;' % (p['name'], w(c.get('I'))))
    # everything read but never driven: constant 0 (outputs of GTP/MMCM/SRL/other clocks)
    used = set(ids)
    undriven = sorted(n for n in used if n not in driven and n not in userclk)
    for n in undriven:
        body.append("assign %s = 1'b0;" % w(n))
    for n in userclk:
        body.append('assign %s = clk;' % w(n))
    wires = sorted(set(ids) - regs)
    lines = ['// generated by simnet.py: simulation view of the user-clock logic', '/* verilator lint_off UNOPTFLAT */',
             '/* verilator lint_off WIDTH */', 'module top(input clk,']
    lines += ['  input %s,' % x for x in sorted(set(ports_in))]
    lines += ['  output %s%s' % (x, ',' if i < len(set(ports_out)) - 1 else '') for i, x in enumerate(sorted(set(ports_out)))]
    lines.append(');')
    lines += ['wire %s;' % ids[n] for n in wires]
    lines += ['integer bram_debug = 0; initial void\'($value$plusargs("bram_debug=%d", bram_debug));']
    lines += decl + body + ['endmodule']
    stats = dict(ff=nff, lut=nlut, carry=ncarry, bram=nbram, undriven_const0=len(undriven), ports_in=len(set(ports_in)),
                 ports_out=len(set(ports_out)))
    return '\n'.join(lines) + '\n', dict(ids, **bmap), stats


def main():
    a = sys.argv[1:]
    if len(a) < 3 or a[0] != 'verilog':
        raise SystemExit(__doc__)
    logic = json.load(open_any(a[1]))
    txt, ids, stats = build(logic, sync_reset='--sync-reset' in a)
    open(a[2], 'w').write(txt)
    if '--map' in a:
        json.dump(ids, open(a[a.index('--map') + 1], 'w'))
    print(stats)


if __name__ == '__main__':
    main()
