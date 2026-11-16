#!/usr/bin/env python3
"""Layer 4: lossless placed netlist for MachXO2/XO3 (Trellis text config). Generic.

  xo3l4.py build  DESIGN.config OUT.json      netlist + ownership of every config line
  xo3l4.py tocfg  NET.json OUT.config         inverse: regenerate the Trellis text config
  xo3l4.py check  DESIGN.config NET.json      partition proof + text identity
  xo3l4.py split  NET.json BLOCKS.json OUTDIR [--default NAME]   layer-5 split, one JSON per block
  xo3l4.py merge  SPLITDIR OUT.json           inverse of split
  xo3l4.py same   A.json B.json               canonical JSON equality (exit 1 if different)

Needs pytrellis (PYTHONPATH=$TRELLIS/lib/trellis) for build only.

Format ("xo3-netlist-l4-1"): the xo3net.py netlist (cells / nets / accounting,
same names and semantics) plus
  header      raw text before the first `.tile` (.device / .variant lines)
  tiles       every `.tile NAME:TYPE` key of the input config, sorted
  cells[c].config  [[tile, raw line], ...]  enum:/word:/unknown: lines the cell owns
  nets[n].config   [[tile, raw line], ...]  arc: lines the net owns
  unassigned  [[tile, raw line, reason], ...]
  ownership   counts by owner kind and reason
Ownership rules (each input line owned exactly once):
  arc: on the driver path of a net's sink -> that net.  A global-enable arc
       (both ends the same global net, e.g. BRANCH_HPBX <- G_VPTX) or an arc that
       reaches no BEL pin (stub/antenna) -> the net that drives its source end;
       a net with no sinks is added for it (`antenna: true`); when no BEL drives
       the source end either (an unused primary clock's branch enables), the net
       is `undriven:<root wire>` with driver null.  Arcs whose wire names do not
       exist on this device: unassigned.
  enum:/word: -> the first cell xo3net attributes it to.  Slice-shared settings
       (SLICEx.MODE, CLK/LSR muxes, REG-shared enums) live in both K0/K1 or
       FF0/FF1 `cfg`, but the line is owned by the K0 / FF0 cell.  Settings no
       BEL takes (SYSCONFIG.*) -> a tile pseudo-cell `TILE:<tile>` (type TILE).
  unknown: (bits outside the database's decoded options) -> the cell of the one
       site prefix (PIOA, ...) whose enum options use that bit, when exactly one
       such cell exists in the tile and the bit is one of its options' bits
       (e.g. the bit that completes PIOx.BASE_TYPE=BIDIR_*); otherwise unassigned
       with a reason (no database setting uses the bit / ambiguous).
tocfg writes tiles sorted by name and, per tile, arc / word / enum / unknown
lines each sorted, which is the order Trellis' ChipConfig::to_string uses (its
maps are sorted); `check` verifies byte identity of the text against the input,
so the ordering rule is checked, not assumed.

Split (layer 5 plumbing): BLOCKS.json = {"cells": {cell: path}, "nets": {net: path}}.
A net not listed takes its driver cell's block; anything unlisted goes to
--default (default "unannotated").  OUTDIR/_meta.json keeps everything that is
not a cell or net (header, tiles, unassigned, accounting, ownership) and the
manifest; one file per block path ('/' -> '__').
"""
import json, os, re, sys, argparse, hashlib
from collections import defaultdict, Counter

SECT = {'arc:': 0, 'word:': 1, 'enum:': 2, 'unknown:': 3}


def read_config(path):
    """-> header text, [(tile, type)], [(tile, line)] in file order"""
    header, tiles, lines = [], [], []
    cur = None
    for raw in open(path):
        l = raw.rstrip('\n')
        if l.startswith('.tile'):
            cur = l.split()[1]
            tiles.append(cur)
            continue
        if cur is None:
            header.append(raw)
            continue
        if l.strip():
            lines.append((cur, l))
    return ''.join(header), tiles, lines


def build(path):
    import pytrellis
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import xo3net
    tr = {}
    net = xo3net.build(path, trace=tr)
    header, tiles, lines = read_config(path)
    cells, nets = net['cells'], net['nets']
    for c in cells.values():
        c['config'] = []
    for n in nets.values():
        n['config'] = []
    unassigned = []
    why = Counter()
    owned_arcs = {}
    for nn, n in nets.items():
        for t, pp in n['pips']:
            src, sink = pp.split('->')
            owned_arcs[(t, 'arc: %s %s' % (sink, src))] = (nn, 'net path')
    resolve, canon = tr['resolve'], tr['canon']
    for (label, sk, sr, kind) in tr['cfg_arcs']:
        t, pp = label
        src, sink = pp.split('->')
        ln = (t, 'arc: %s %s' % (sink, src))
        if ln in owned_arcs:
            continue
        d, _path, _root = resolve(sr)
        if d is None:
            # configured wire with no driver (e.g. the branch enables of an
            # unused primary clock): owned by a driverless net named after its root
            dn = 'undriven:' + xo3net.kname(_root)
            if dn not in nets:
                nets[dn] = {'driver': None, 'sinks': [], 'pips': [], 'config': [], 'antenna': True}
            owned_arcs[ln] = (dn, 'arc on a driverless wire (' + ('global enable' if kind == 'enable' else 'stub') + ')')
            continue
        dn = '%s.%s' % d
        if dn not in nets:
            nets[dn] = {'driver': dn, 'sinks': [], 'pips': [], 'config': [], 'antenna': True}
            cells.setdefault(d[0], {'type': '?', 'cfg': {}, 'pins': {}, 'dirs': {}, 'config': []})
            cells[d[0]]['pins'][d[1]] = dn
            cells[d[0]]['dirs'][d[1]] = 'out'
        owned_arcs[ln] = (dn, 'global enable' if kind == 'enable' else 'stub (reaches no BEL pin)')
    sown = tr['setting_owner']
    chip, bitdbs, ttype = tr['chip'], tr['bitdbs'], tr['tile_type']
    tloc = tr['tile_loc']

    def unknown_owner(t, bitname):
        f, b = map(int, re.match(r'F(\d+)B(\d+)$', bitname).groups())
        db = bitdbs[ttype[t]]
        pfx = set()
        for en in db.get_settings_enums():
            for opt, bg in db.get_data_for_enum(en).options.items():
                if any((x.frame, x.bit) == (f, b) for x in bg.bits):
                    pfx.add(en.split('.')[0])
        for w in db.get_settings_words():
            for bg in db.get_data_for_setword(w).bits:
                if any((x.frame, x.bit) == (f, b) for x in bg.bits):
                    pfx.add(w.split('.')[0])
        if not pfx:
            return None, 'no database setting uses this bit'
        x, y = tloc[t]
        cand = sorted(cn for cn, c in cells.items() if c.get('bel') in pfx and (c.get('x'), c.get('y')) == (x, y))
        # PIC tiles: sites of the tile may sit in a neighbouring R/C (the PIO
        # cell coordinates are the routing-graph location); match by settings
        if not cand:
            cand = sorted(cn for cn, c in cells.items() if c.get('bel') in pfx and
                          any(tt == t for tt, _ in c.get('config', [])))
        if len(cand) == 1 and len(pfx) == 1:
            return cand[0], 'bit of %s options (%s), not decoded by Trellis' % (sorted(pfx)[0], bitname)
        if len(cand) == 1:
            return cand[0], 'bit of %s options; only %s is configured in the tile' % ('/'.join(sorted(pfx)), cand[0])
        return None, 'bit of %s options, owner ambiguous (%d configured candidates)' % ('/'.join(sorted(pfx)), len(cand))

    pending_unknown = []
    for t, l in lines:
        kw = l.split()[0]
        if kw == 'arc:':
            o = owned_arcs.get((t, l))
            if o:
                nets[o[0]]['config'].append([t, l])
                why['net: ' + o[1]] += 1
            else:
                unassigned.append([t, l, 'arc not on any resolved net (or other device)'])
                why['unassigned: arc'] += 1
        elif kw in ('enum:', 'word:'):
            _, k, v = l.split(None, 2)
            o = sown.get((t, kw[:-1], k))
            if o:
                cells[o]['config'].append([t, l])
                why['cell: setting'] += 1
            else:
                pc = 'TILE:' + t
                c = cells.setdefault(pc, {'type': 'TILE', 'tile': t, 'cfg': {}, 'pins': {}, 'dirs': {}, 'config': []})
                c['cfg'][k] = v
                c['config'].append([t, l])
                why['tile pseudo-cell: setting no BEL takes'] += 1
        elif kw == 'unknown:':
            pending_unknown.append((t, l))
        else:
            raise SystemExit('unexpected line in tile %s: %r' % (t, l))
    for t, l in pending_unknown:
        o, reason = unknown_owner(t, l.split()[1])
        if o:
            cells[o]['config'].append([t, l])
            cells[o].setdefault('unknown_bits', []).append([l.split()[1], reason])
            why['cell: unknown bit (%s)' % reason.split(' (')[0].split(';')[0]] += 1
        else:
            unassigned.append([t, l, reason])
            why['unassigned: unknown bit, ' + reason.split(' (')[0]] += 1
    for c in cells.values():
        c['config'].sort()
    for n in nets.values():
        n['config'].sort()
    net['format'] = 'xo3-netlist-l4-1'
    net['source'] = os.path.basename(path)
    net['header'] = header
    net['tiles'] = sorted(tiles)
    net['unassigned'] = sorted(unassigned)
    net['ownership'] = dict(why)
    return net


def tocfg(net):
    by = defaultdict(list)
    for c in net['cells'].values():
        for t, l in c.get('config', []):
            by[t].append(l)
    for n in net['nets'].values():
        for t, l in n.get('config', []):
            by[t].append(l)
    for t, l, _ in net['unassigned']:
        by[t].append(l)
    out = [net['header']]
    for t in sorted(net['tiles']):
        ls = sorted(by.get(t, []), key=lambda l: (SECT[l.split()[0]], l))
        out.append('.tile %s\n' % t + ''.join(l + '\n' for l in ls) + '\n')
    extra = set(by) - set(net['tiles'])
    if extra:
        raise SystemExit('lines for tiles not in the header: %s' % sorted(extra)[:5])
    return ''.join(out)


def owned_lines(net):
    L = []
    for cn, c in net['cells'].items():
        L += [(t, l, 'cell ' + cn) for t, l in c.get('config', [])]
    for nn, n in net['nets'].items():
        L += [(t, l, 'net ' + nn) for t, l in n.get('config', [])]
    L += [(t, l, 'unassigned') for t, l, _ in net['unassigned']]
    return L


def check(cfgpath, net):
    header, tiles, lines = read_config(cfgpath)
    inp = Counter(lines)
    own = Counter((t, l) for t, l, _ in owned_lines(net))
    dup = [k for k, v in own.items() if v > 1]
    missing = [k for k in inp if own[k] < inp[k]]
    extra = [k for k in own if own[k] > inp.get(k, 0)]
    txt = tocfg(net)
    orig = open(cfgpath).read()
    same = txt == orig
    kinds = Counter(o.split()[0] if not o.startswith('cell TILE:') else 'tile-pseudo-cell' for _, _, o in owned_lines(net))
    print('input lines %d (in %d tiles); owned: %s' % (sum(inp.values()), len(tiles),
          ', '.join('%s %d' % kv for kv in sorted(kinds.items()))))
    print('partition: duplicates %d, missing %d, extra %d' % (len(dup), len(missing), len(extra)))
    for k in (dup + missing + extra)[:10]:
        print('  ', k)
    if same:
        print('tocfg == input: IDENTICAL (%d bytes, sha256 %s)' % (len(orig), hashlib.sha256(orig.encode()).hexdigest()))
    else:
        a, b = txt.splitlines(), orig.splitlines()
        i = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))
        print('tocfg DIFFERS at line %d: %r vs %r' % (i + 1, a[i] if i < len(a) else None, b[i] if i < len(b) else None))
    ok = same and not dup and not missing and not extra
    return ok


def canon(obj):
    return json.dumps(obj, sort_keys=True, separators=(',', ':'))


def fname(path):
    return re.sub(r'[^A-Za-z0-9_.-]', '_', path.replace('/', '__')) + '.json'


def split(net, ann, outdir, default):
    os.makedirs(outdir, exist_ok=True)
    cblk = ann.get('cells', {})
    nblk = ann.get('nets', {})
    blocks = defaultdict(lambda: {'cells': {}, 'nets': {}})
    for cn, c in net['cells'].items():
        blocks[cblk.get(cn, default)]['cells'][cn] = c
    for nn, n in net['nets'].items():
        b = nblk.get(nn)
        if b is None and n.get('driver'):
            b = cblk.get(n['driver'].rsplit('.', 1)[0])
        blocks[b or default]['nets'][nn] = n
    unknown = sorted(set(cblk) - set(net['cells'])) + sorted(set(nblk) - set(net['nets']))
    meta = {k: v for k, v in net.items() if k not in ('cells', 'nets')}
    man = {}
    for b, body in sorted(blocks.items()):
        fn = fname(b)
        s = canon({'block': b, 'cells': body['cells'], 'nets': body['nets']})
        open(os.path.join(outdir, fn), 'w').write(s + '\n')
        nl = sum(len(c.get('config', [])) for c in body['cells'].values()) + \
            sum(len(n.get('config', [])) for n in body['nets'].values())
        man[b] = {'file': fn, 'cells': len(body['cells']), 'nets': len(body['nets']), 'config_lines': nl,
                  'sha256': hashlib.sha256(s.encode()).hexdigest()}
    meta['_split'] = {'default': default, 'blocks': man, 'annotation_names_not_in_netlist': unknown}
    open(os.path.join(outdir, '_meta.json'), 'w').write(canon(meta) + '\n')
    for b, m in sorted(man.items()):
        print('%-40s cells %5d nets %5d config lines %6d' % (b, m['cells'], m['nets'], m['config_lines']))
    if unknown:
        print('annotation names not in the netlist: %d (e.g. %s)' % (len(unknown), unknown[:3]))


def merge(d):
    meta = json.load(open(os.path.join(d, '_meta.json')))
    sp = meta.pop('_split')
    net = dict(meta)
    net['cells'], net['nets'] = {}, {}
    for b, m in sp['blocks'].items():
        body = json.load(open(os.path.join(d, m['file'])))
        assert body['block'] == b
        for k in ('cells', 'nets'):
            for n, v in body[k].items():
                assert n not in net[k], (k, n)
                net[k][n] = v
    return net


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('op', choices=['build', 'tocfg', 'check', 'split', 'merge', 'same'])
    ap.add_argument('args', nargs='+')
    ap.add_argument('--default', default='unannotated')
    a = ap.parse_args()
    x = a.args
    if a.op == 'build':
        net = build(x[0])
        json.dump(net, open(x[1], 'w'), indent=1, sort_keys=True)
        print('cells %d, nets %d, unassigned %d; ownership %s' % (len(net['cells']), len(net['nets']),
              len(net['unassigned']), json.dumps(net['ownership'], sort_keys=True)))
    elif a.op == 'tocfg':
        open(x[1], 'w').write(tocfg(json.load(open(x[0]))))
    elif a.op == 'check':
        sys.exit(0 if check(x[0], json.load(open(x[1]))) else 1)
    elif a.op == 'split':
        split(json.load(open(x[0])), json.load(open(x[1])), x[2], a.default)
    elif a.op == 'merge':
        json.dump(merge(x[0]), open(x[1], 'w'), indent=1, sort_keys=True)
    elif a.op == 'same':
        A, B = (json.load(open(p)) for p in x[:2])
        s = canon(A) == canon(B)
        print('IDENTICAL (canonical JSON)' if s else 'DIFFERENT')
        sys.exit(0 if s else 1)


if __name__ == '__main__':
    main()
