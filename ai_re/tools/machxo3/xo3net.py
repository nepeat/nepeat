#!/usr/bin/env python3
"""MachXO2/XO3 Trellis text config -> placed netlist (JSON). Generic, no design data.

  xo3net.py DESIGN.config OUT.json

Why not Trellis' own RoutingGraph for the wires: for MachXO2/3 it drops
(a) edge wires whose span leaves the fabric and (b) the secondary high-fanout
network (VSRX/VSTX/HSBX, "not fuzzed yet" in RoutingGraph.cpp), and only
partly models primary-clock branches.  This tool names wires itself:

* local wires: Trellis' prefix convention ([NS]n)([EW]n)_NAME = NAME at the
  tile offset by that much; device-size prefixes (2100_...) kept only for
  this device.
* edge reflection (inferred from this kind of bitstream, see cpld.md): the
  fabric is x 0..max_col, y 1..max_row-1.  An x2/x6 wire whose *origin* tile
  lies outside it does not exist on its own: it is the reflection of the
  opposite-direction wire, with the home coordinate mirrored about the edge
  (x -> -1-x, x -> 2*max_col+1-x, y -> 1-y, y -> 2*(max_row-1)+1-y).
* primary clocks: every G_VPRX0n00, [LRG]_HPSX0n00, [UDG]_VPTX0n00,
  BRANCH_HPBX0n00 and per-column DCC wire of index n is the one net PCLKn,
  driven by G_VPRXCLKIn (the centre DCC n; DCC CE pins are checked unused).
* secondary nets (inferred, see cpld.md): G_VSRX0n00 = SCLKn.  In column x,
  G_VSTX0200 carries SCLK((x+2)%4) and G_VSTX0300 carries SCLK(4+(x+2)%4).
  A tile's G_HSBX0000 / G_HSBX0400 output drives branch track (x+2)%4 /
  4+(x+2)%4 of its row over columns x-1..x+2; a tap G_HSBX0k01 in column x
  reads track k from the driver column xd in [x-2, x+1] with
  (xd+2)%4 == k%4.
* BEL pins come from Trellis' routing graph (names are local there).

Output JSON: cells {name: {type, x, y, z, bel, cfg, pins{pin: net}}},
nets {net: {driver, sinks, pips}}, accounting (every configured arc and
setting either explains a connection/cell, or is listed).
"""
import os, re, sys, json, argparse
from collections import defaultdict
import pytrellis

DB = os.path.join(os.environ.get('TRELLIS', ''), 'share/trellis/database')
PFX = re.compile(r'^([NS]\d+)?([EW]\d+)?_(.*)$')
DEVPFX = re.compile(r'^(\d+D?)_(.*)$')
PIO_WIRE = ('DI', 'JDI', 'PADD', 'INDD', 'IOLDO', 'IOLTO', 'JCE', 'JCLK', 'JLSR',
            'JONEG', 'JOPOS', 'JTS', 'JIN', 'JIP', 'JINCK')
SPAN = re.compile(r'^([HV])0([26])([NESW])(\d\d)(\d\d)$')
PCLK_RE = [re.compile(p) for p in (r'^G_VPRX0(\d)00$', r'^[LRG]_HPSX0(\d)00$', r'^[UDG]_VPTX0(\d)00$',
                                   r'^BRANCH_HPBX0(\d)00$', r'^G_J?CLK[IO](\d)[TB]_DCC$')]
LOCAL_G = re.compile(r'^G_(VPTX|HPSX|HSBX|VSTX|HSSX)|^G_J?CLK[IO]\d[TB]_DCC$|^G_JOSC|^[LRUD]_|^BRANCH_')


class Wires:
    def __init__(self, devname, max_col, max_row):
        m = re.search(r'-(\d+)', devname)
        self.size = m.group(1) if m else ''
        self.mx, self.my = max_col, max_row
        self.notes = defaultdict(int)

    def origin_off(self, d, dirn, h):
        """span-d wire, direction, home coordinate -> origin outside fabric?"""
        half = int(d) // 2
        if dirn == 'E':
            o = h - half; return o < 0 or o > self.mx
        if dirn == 'W':
            o = h + half; return o < 0 or o > self.mx
        if dirn == 'S':
            o = h - half; return o < 1 or o > self.my - 1
        o = h + half
        return o < 1 or o > self.my - 1

    def mirror(self, axis, v):
        if axis == 'x':
            return -1 - v if v < self.mx / 2 else 2 * self.mx + 1 - v
        return 1 - v if v < self.my / 2 else 2 * (self.my - 1) + 1 - v

    def canon(self, name, x, y):
        """tile-local wire name at tile (x, y) -> hashable global key, or None."""
        n = name
        m = DEVPFX.match(n)
        if m:
            if m.group(1) != self.size:
                return None
            n = m.group(2)
        # globals
        for r in PCLK_RE:
            mm = r.match(n)
            if mm:
                return ('PCLK', int(mm.group(1)))
        mm = re.match(r'^G_VPRXCLKI(\d)$', n)
        if mm:
            return ('VPRXCLKI', int(mm.group(1)))
        mm = re.match(r'^G_VSRX0(\d)00$', n)
        if mm:
            return ('SCLK', int(mm.group(1)))
        mm = re.match(r'^G_VSTX0(\d)00(_B2T|_T2B)?$', n)
        if mm:
            loc = int(mm.group(1))
            if loc == 2:
                return ('SCLK', (x + 2) % 4)
            if loc == 3:
                return ('SCLK', 4 + (x + 2) % 4)
            self.notes['VSTX local index %d' % loc] += 1
            return ('VSTX?', loc, x, y)
        mm = re.match(r'^G_HSBX0(\d)0([01])$', n)
        if mm:
            k, tap = int(mm.group(1)), mm.group(2)
            if tap == '0':
                if k not in (0, 4):
                    self.notes['HSBX driver local index %d' % k] += 1
                    return ('HSBX?', k, x, y)
                tr = (x + 2) % 4 + (4 if k == 4 else 0)
                return ('HSBX', tr, x, y)
            xd = [c for c in range(x - 2, x + 2) if (c + 2) % 4 == k % 4]
            assert len(xd) == 1
            return ('HSBX', k, xd[0], y)
        if n.startswith('G_') and not LOCAL_G.match(n):
            return ('G', n)
        if n.startswith(('L_', 'R_', 'U_', 'D_', 'BRANCH_', 'G_')):
            return (n, x, y)
        # local
        m = PFX.match(n)
        groups = m.groups()[:2] if m else ()
        for g in groups:
            if not g:
                continue
            d = int(g[1:])
            if g[0] == 'N':
                y -= d
            elif g[0] == 'S':
                y += d
            elif g[0] == 'E':
                x += d
            else:
                x -= d
        base = m.group(3) if m else n
        if any(p in name for p in PIO_WIRE):
            if x == -1:
                x = 0
            if x == self.mx + 1:
                x = self.mx
        sm = SPAN.match(base)
        if sm:
            hv, d, dirn, trk, seg = sm.groups()
            h = x if hv == 'H' else y
            if self.origin_off(d, dirn, h):
                opp = {'E': 'W', 'W': 'E', 'N': 'S', 'S': 'N'}[dirn]
                base = '%s0%s%s%s%s' % (hv, d, opp, trk, seg)
                if hv == 'H':
                    x = self.mirror('x', x)
                else:
                    y = self.mirror('y', y)
                self.notes['reflected %s0%s' % (hv, d)] += 1
        return (base, x, y)


def parse_config(path):
    tiles = {}
    cur = None
    variant = dev = None
    for raw in open(path):
        line = raw.strip()
        if not line:
            continue
        if line.startswith('.device'):
            dev = line.split()[1]
        elif line.startswith('.variant'):
            variant = line.split()[1]
        elif line.startswith('.tile'):
            cur = tiles.setdefault(line.split()[1], {'arcs': [], 'enums': {}, 'words': {}, 'unknown': []})
        elif line.startswith('arc:'):
            _, sink, src = line.split()
            cur['arcs'].append((src, sink))
        elif line.startswith('enum:'):
            _, k, v = line.split()
            cur['enums'][k] = v
        elif line.startswith('word:'):
            _, k, v = line.split()
            cur['words'][k] = v
        elif line.startswith('unknown:'):
            cur['unknown'].append(line.split()[1])
    return dev, variant, tiles


def kname(k):
    if k[0] in ('PCLK', 'SCLK', 'VPRXCLKI'):
        return '%s%d' % k
    if k[0] == 'G':
        return k[1]
    if k[0] == 'HSBX':
        return 'HSBX%d_R%dC%d' % (k[1], k[3], k[2])
    return 'R%dC%d_%s' % (k[2], k[1], k[0]) if len(k) == 3 else '_'.join(map(str, k))


def build(config_path, trace=None):
    """trace: optional dict, filled with internals for xo3l4.py (lossless layer 4);
    the returned netlist is the same with or without it."""
    pytrellis.load_database(DB)
    dev, variant, tcfg = parse_config(config_path)
    cc = pytrellis.ChipConfig.from_string(open(config_path).read())
    chip = cc.to_chip()
    rg = chip.get_routing_graph(False, True)
    S = rg.to_str
    W = Wires(chip.info.name, chip.get_max_col(), chip.get_max_row())

    tile_loc, tile_type = {}, {}
    for name, tile in chip.tiles.items():
        rc = tile.info.get_row_col()
        tile_loc[name] = (rc.second, rc.first)
        tile_type[name] = tile.info.type
    bitdbs = {}
    for tt in set(tile_type.values()):
        bitdbs[tt] = pytrellis.get_tile_bitdata(pytrellis.TileLocator(chip.info.family, chip.info.name, tt))

    drv = {}
    conflicts = []
    RANK = {'cfg': 3, 'model': 2, 'fixed': 1, 'zero': 0}

    def set_drv(sink, src, kind, label):
        if sink is None or src is None or sink == src:
            return
        if sink in drv:
            old = drv[sink]
            if old[0] == src:
                return
            if RANK[kind] > RANK[old[1]]:
                drv[sink] = (src, kind, label)
            elif RANK[kind] == RANK[old[1]]:
                conflicts.append((kname(sink), kname(old[0]), old[1], str(old[2]), kname(src), kind, str(label)))
            return
        drv[sink] = (src, kind, label)

    cfg_arcs = []
    configured = set()
    dropped = []
    for tname, cfg in tcfg.items():
        x, y = tile_loc[tname]
        for src, sink in cfg['arcs']:
            sk, sr = W.canon(sink, x, y), W.canon(src, x, y)
            if sk is None or sr is None:
                dropped.append('%s %s->%s' % (tname, src, sink)); continue
            label = (tname, '%s->%s' % (src, sink))
            if sk == sr:
                # collapsed global (e.g. BRANCH_HPBX0000 <- G_VPTX0000): enable only
                cfg_arcs.append((label, sk, sr, 'enable'))
                continue
            set_drv(sk, sr, 'cfg', label)
            configured.add(sk)
            cfg_arcs.append((label, sk, sr, 'arc'))
    # model arcs: PCLKn <- VPRXCLKIn (centre DCC n)
    for n in range(8):
        set_drv(('PCLK', n), ('VPRXCLKI', n), 'model', ('model', 'DCC%d' % n))
    # fixed and zero-bit
    for tname, tt in tile_type.items():
        x, y = tile_loc[tname]
        db = bitdbs[tt]
        for fc in db.get_fixed_conns():
            if fc.source.endswith('KEEP') or 'HSBX' in fc.sink:
                continue
            set_drv(W.canon(fc.sink, x, y), W.canon(fc.source, x, y), 'fixed', (tname, '%s=>%s' % (fc.source, fc.sink)))
    for tname, tt in tile_type.items():
        x, y = tile_loc[tname]
        db = bitdbs[tt]
        for sink in db.get_sinks():
            md = db.get_mux_data_for_sink(sink)
            zs = [src for src, ad in md.arcs.items() if len(ad.bits.bits) == 0]
            if len(zs) != 1:
                continue
            sk = W.canon(sink, x, y)
            if sk in configured:
                continue
            set_drv(sk, W.canon(zs[0], x, y), 'zero', (tname, '%s->%s' % (zs[0], sink)))
    # PIC direct input (inferred, cpld.md): the database files the delay-select
    # bit as the only option of the JQn <- JDIx and DIx <- JDIx muxes, so those
    # read as unconnected by default; physically JQn (fabric) and DI (IOLOGIC)
    # always see JDIx, which is PADDI or, with that bit, the delayed INDD.
    jq_re = re.compile(r'^(?:[NSEW]\d+)?_?JQ([0-3])$')
    for tname, tt in tile_type.items():
        if 'PIC' not in tt or 'CIB' in tt or 'DUMMY' in tt:
            continue
        x, y = tile_loc[tname]
        db = bitdbs[tt]
        jdi_pfx = None
        have_jq = set()
        for sink in db.get_sinks():
            md = db.get_mux_data_for_sink(sink)
            srcs = [src for src in md.arcs.keys() if src != '#']
            for src in srcs:
                mm = re.match(r'^((?:[NSEW]\d+_)?)JDI([A-D])$', src)
                if mm:
                    jdi_pfx = mm.group(1)
            if len(srcs) == 1 and re.search(r'JDI[A-D]$', srcs[0]) and \
                    (re.search(r'JQ[0-3]$', sink) or re.search(r'(^|_)DI[A-D]_\w*IOLOGIC$', sink)):
                set_drv(W.canon(sink, x, y), W.canon(srcs[0], x, y), 'fixed', (tname, 'model %s<-%s' % (sink, srcs[0])))
                mq = re.search(r'JQ([0-3])$', sink)
                if mq:
                    have_jq.add(int(mq.group(1)))
        if jdi_pfx is not None:
            for n in range(4):
                if n not in have_jq:
                    set_drv(W.canon('JQ%d' % n, x, y), W.canon('%sJDI%s' % (jdi_pfx, 'ABCD'[n]), x, y), 'fixed',
                            (tname, 'model JQ%d<-%sJDI%s' % (n, jdi_pfx, 'ABCD'[n])))

    # BELs
    bel_out = {}
    cells = {}
    for loc, rtile in rg.tiles.items():
        for bid, bel in rtile.bels.items():
            cname = 'R%dC%d_%s' % (loc.y, loc.x, S(bid))
            c = {'type': S(bel.type), 'x': loc.x, 'y': loc.y, 'z': bel.z, 'bel': S(bid),
                 'cfg': {}, 'pins': {}, 'pinwire': {}, 'dirs': {}}
            cells[cname] = c
            for pid, (rid, d) in bel.pins.items():
                k = W.canon(S(rid.id), rid.loc.x, rid.loc.y)
                c['pinwire'][S(pid)] = k
                out = d == pytrellis.PortDirection.PORT_OUT
                c['dirs'][S(pid)] = 'out' if out else 'in'
                if out:
                    if k in bel_out:
                        conflicts.append(('two BEL drivers', kname(k), str(bel_out[k]), cname))
                    else:
                        bel_out[k] = (cname, S(pid))

    # settings -> cells
    by_name = defaultdict(list)
    for cname, c in cells.items():
        by_name[c['bel']].append(cname)
    leftovers = []

    hit = []

    def put(cname, key, val):
        if cname in cells:
            cells[cname]['cfg'][key] = val
            hit.append(cname)
            return True
        return False

    setting_owner = {}

    for tname, cfg in tcfg.items():
        x, y = tile_loc[tname]
        tt = tile_type[tname]
        pre = 'R%dC%d_' % (y, x)
        items = [('enum', k, v) for k, v in cfg['enums'].items()] + [('word', k, v) for k, v in cfg['words'].items()]
        for kind, k, v in items:
            pfx, rest = k.split('.', 1)
            ok = False
            del hit[:]
            if tt.startswith('PLC') and pfx.startswith('SLICE'):
                if rest[:2] in ('K0', 'K1'):
                    ok = put(pre + pfx + '.' + rest[:2], rest[3:], v)
                elif rest[:4] in ('REG0', 'REG1'):
                    ok = put(pre + pfx + '.FF' + rest[3], rest[5:], v)
                elif rest == 'MODE':
                    ok = put(pre + pfx + '.K0', 'MODE', v) | put(pre + pfx + '.K1', 'MODE', v)
                elif rest.startswith('CCU2.INJECT1_'):
                    ok = put(pre + pfx + '.K' + rest[-1], 'INJECT1', v)
                else:
                    ok = put(pre + pfx + '.FF0', rest, v) | put(pre + pfx + '.FF1', rest, v)
                    if rest == 'WREMUX':
                        ok |= put(pre + pfx + '.K0', rest, v) | put(pre + pfx + '.K1', rest, v)
            elif tt.startswith('PLC') and pfx[:3] in ('CLK', 'LSR'):
                s = 'ABCD'[int(pfx[3])]
                ok = put(pre + 'SLICE%s.FF0' % s, pfx[:3] + '.' + rest, v) | \
                    put(pre + 'SLICE%s.FF1' % s, pfx[:3] + '.' + rest, v)
            else:
                cands = [n for n in by_name.get(pfx, []) if (cells[n]['x'], cells[n]['y']) == (x, y)]
                if not cands:
                    cands = [n for n in cells if cells[n]['bel'].endswith(pfx) and (cells[n]['x'], cells[n]['y']) == (x, y)]
                if not cands and len(by_name.get(pfx, [])) == 1:
                    cands = by_name[pfx]
                if len(cands) == 1:
                    ok = put(cands[0], rest, v)
            if not ok:
                leftovers.append('%s %s=%s' % (tname, k, v))
            else:
                setting_owner[(tname, kind, k)] = hit[0]

    # PIO output/tristate source (Trellis has no arc for it): the fabric
    # JPADDO/JPADDT wires, or the IOLOGIC output/TS register when the
    # DATAMUX/TRIMUX enums say so.
    for cname, c in cells.items():
        if c['type'] != 'PIO':
            continue
        L = c['bel'][-1]
        x, y = c['x'], c['y']
        cf = c['cfg']
        if 'I' in c['pinwire']:
            reg = 'IOLDO' in (cf.get('DATAMUX_OREG'), cf.get('DATAMUX_ODDR'))
            src = W.canon(('IOLDO%s_PIO' if reg else 'JPADDO%s') % L, x, y)
            set_drv(c['pinwire']['I'], src, 'model', (cname, 'model I<-%s' % ('IOLDO' if reg else 'JPADDO')))
        if 'T' in c['pinwire']:
            reg = cf.get('TRIMUX_TSREG') == 'IOLTO'
            src = W.canon(('IOLTO%s_PIO' if reg else 'JPADDT%s') % L, x, y)
            set_drv(c['pinwire']['T'], src, 'model', (cname, 'model T<-%s' % ('IOLTO' if reg else 'JPADDT')))

    def resolve(k):
        path, seen = [], set()
        while True:
            if k in bel_out:
                return bel_out[k], path, k
            if k in seen:
                return None, path, k
            seen.add(k)
            if k not in drv:
                return None, path, k
            src, kind, label = drv[k]
            path.append((kind, label))
            k = src

    nets = {}
    used_cfg = set()
    for cname, c in cells.items():
        for pin, k in c['pinwire'].items():
            if c['dirs'][pin] == 'out':
                continue
            d, path, root = resolve(k)
            direct = d is not None and not path and d[0] != cname  # same wire, e.g. K0.FCO -> K1.FCI
            if not direct and not any(kind == 'cfg' for kind, _ in path) and not (
                    d and path and all(kind in ('fixed', 'zero', 'model') for kind, _ in path) and
                    cells[d[0]]['x'] == c['x'] and cells[d[0]]['y'] == c['y'] and d[0] != cname):
                # only report a pin when routing (or an intra-tile default
                # path between two different cells) connects it
                if (not path and not direct) or d is None:
                    continue
            net = '%s.%s' % d if d else 'undriven:' + kname(root)
            if d is None and not any(kind == 'cfg' for kind, _ in path):
                continue
            c['pins'][pin] = net
            n = nets.setdefault(net, {'driver': net if d else None, 'sinks': [], 'pips': []})
            n['sinks'].append('%s.%s' % (cname, pin))
            for kind, label in path:
                if kind == 'cfg':
                    n['pips'].append(list(label)); used_cfg.add(label)
    for k, (cname, pin) in bel_out.items():
        net = '%s.%s' % (cname, pin)
        if net in nets:
            cells[cname]['pins'][pin] = net

    keep = {}
    for cname, c in cells.items():
        if c['cfg'] or c['pins']:
            keep[cname] = {k: v for k, v in c.items() if k not in ('pinwire',)}
            keep[cname]['dirs'] = {p: c['dirs'][p] for p in c['pins']}
    unused_arcs = ['%s %s' % (l[0], l[1]) for (l, sk, sr, kind) in cfg_arcs if kind == 'arc' and l not in used_cfg]
    enables = ['%s %s' % (l[0], l[1]) for (l, sk, sr, kind) in cfg_arcs if kind == 'enable']
    undriven_cfg = sorted(set(n for n in nets if n.startswith('undriven:')))
    acc = {
        'arcs_total': sum(len(c['arcs']) for c in tcfg.values()),
        'arcs_dropped_other_device': dropped,
        'arcs_global_enables': len(enables),
        'arcs_unused': sorted(unused_arcs),
        'nets_undriven': undriven_cfg,
        'settings_unattributed': sorted(leftovers),
        'unknown_bits': ['%s %s' % (t, u) for t, c in tcfg.items() for u in c['unknown']],
        'driver_conflicts': conflicts,
        'wire_model_notes': dict(W.notes),
    }
    if trace is not None:
        trace.update(resolve=resolve, setting_owner=setting_owner, cfg_arcs=cfg_arcs, dropped=dropped,
                     tile_type=tile_type, tile_loc=tile_loc, canon=W.canon, chip=chip, bitdbs=bitdbs,
                     device=dev, variant=variant)
    return {'device': variant or dev, 'cells': keep, 'nets': nets, 'accounting': acc}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('config'); ap.add_argument('out')
    a = ap.parse_args()
    out = build(a.config)
    json.dump(out, open(a.out, 'w'), indent=1, sort_keys=True)
    acc = out['accounting']
    print('cells %d, nets %d; arcs %d: dropped %d, global enables %d, unused %d; undriven nets %d; '
          'settings unattributed %d; unknown bits %d; conflicts %d; notes %s'
          % (len(out['cells']), len(out['nets']), acc['arcs_total'], len(acc['arcs_dropped_other_device']),
             acc['arcs_global_enables'], len(acc['arcs_unused']), len(acc['nets_undriven']),
             len(acc['settings_unattributed']), len(acc['unknown_bits']), len(acc['driver_conflicts']),
             acc['wire_model_notes']))


if __name__ == '__main__':
    main()
