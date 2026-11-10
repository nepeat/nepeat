#!/usr/bin/env python3
"""First-glimpse statistics of a FASM file (prjxray feature names).

    fasmstats.py A.fasm [B.fasm]      summary of A (and diff A -> B)

Resource counts are inferred from feature presence and are approximate:
a LUT counts as used if its INIT is non-zero, an FF if any of its own
features (xFF.*, xFFMUX.*) is set, a RAMB18 if IN_USE is set.
Package pins come from prjxray-db package_pins.csv; the package is
inferred as the xc7a100t package(s) in which every used IOB is bonded.
"""
import collections
import csv
import glob
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DB = os.path.join(HERE, '..', 'work', 'src', 'prjxray-db', 'artix7')
TILE_RE = re.compile(r'_X\d+Y\d+$')


def load(path):
    feats = {}
    for line in open(path):
        line = line.split('#', 1)[0].strip()
        if not line:
            continue
        if '=' in line:
            k, v = (x.strip() for x in line.split('=', 1))
        else:
            k, v = line, "1'b1"
        feats[k] = v
    return feats


def value(v):
    if "'" in v:
        w, rest = v.split("'", 1)
        base = {'b': 2, 'h': 16, 'd': 10, 'o': 8}[rest[0]]
        return int(rest[1:].replace('_', ''), base)
    return int(v)


def ttype(tile):
    return TILE_RE.sub('', tile)


def packages():
    out = {}
    for p in sorted(glob.glob(os.path.join(DB, 'xc7a100t*', 'package_pins.csv'))):
        pkg = os.path.basename(os.path.dirname(p))
        pkgname = re.sub(r'-.*', '', pkg)[len('xc7a100t'):]
        if pkgname in out:
            continue
        pins = {}
        for row in csv.DictReader(open(p)):
            if row['site'].startswith('IOB_'):
                y = int(re.search(r'Y(\d+)$', row['site']).group(1))
                pins[(row['tile'], 'IOB_Y%d' % (y % 2))] = (row['pin'], row['bank'], row['pin_function'], row['site'])
        out[pkgname] = pins
    return out


def iobs(feats):
    sites = collections.defaultdict(list)
    for k in feats:
        p = k.split('.')
        if 'IOB33' in p[0] and len(p) > 2 and p[1].startswith('IOB_Y'):
            sites[(p[0], p[1])].append('.'.join(p[2:]))
    out = {}
    for s, fl in sites.items():
        inp = any(f.endswith('.IN') or f.endswith('.IN_ONLY') or f.endswith('.IN_DIFF') for f in fl)
        drv = [f for f in fl if '.DRIVE.' in f]
        inonly = any(f.endswith('IN_ONLY') for f in fl)
        if drv and inp and not inonly:
            d = 'inout'
        elif drv:
            d = 'out'
        elif inp:
            d = 'in'
        else:
            d = 'unused-cfg'
        std = sorted({f.split('.')[0] for f in fl if '.DRIVE.' in f or f.endswith('.IN')})
        pull = [f.split('.')[-1] for f in fl if f.startswith('PULLTYPE.')]
        slew = [f.split('.')[-1] for f in fl if '.SLEW.' in f]
        drive = [f.split('.')[-1] for f in drv]
        out[s] = dict(dir=d, std=std, pull=pull, slew=slew, drive=drive, feats=sorted(fl))
    return out


def summary(feats, pk):
    R = collections.OrderedDict()
    tt = collections.Counter(ttype(k.split('.')[0]) for k in feats)
    R['features'] = len(feats)
    R['routing pips (INT_L/INT_R features)'] = tt['INT_L'] + tt['INT_R']
    luts = [k for k, v in feats.items() if re.search(r'\.SLICE[LM]_X\d\.[A-D]LUT\.INIT', k) and value(v)]
    R['LUTs with non-zero INIT'] = len(luts)
    ffs = set()
    for k in feats:
        m = re.match(r'(\w+)\.(SLICE[LM]_X\d)\.([A-D]5?FF)(MUX)?\.', k)
        if m:
            ffs.add((m.group(1), m.group(2), m.group(3)))
    R['FFs with any feature (ZINI/ZRST/FFMUX)'] = len(ffs)
    R['  of which x5FF'] = sum(1 for f in ffs if '5' in f[2])
    R['LUTRAM/SRL LUTs'] = sum(1 for k in feats if re.search(r'\.[A-D]LUT\.(SRL|RAM|SMALL)$', k))
    R['CARRY4 in use (slices with XOR/CY outputs, CIN/AX carry-in or CARRY4.*)'] = len(
        {tuple(k.split('.')[:2]) for k in feats
         if re.search(r'\.SLICE[LM]_X\d\.([A-D](OUT|FF)MUX\.(XOR|CY)$|PRECYINIT\.(CIN|AX)$|CARRY4\.)', k)})
    slices = {tuple(k.split('.')[:2]) for k in feats if re.match(r'\w+\.SLICE[LM]_X\d\.', k)}
    R['slices with any feature'] = len(slices)
    R['RAMB18 halves IN_USE'] = sum(1 for k in feats if re.search(r'\.RAMB18_Y[01]\.IN_USE$', k))
    R['BRAM tiles with non-zero INIT/INITP content'] = len({k.split('.')[0] for k, v in feats.items()
                                                          if re.search(r'\.INITP?_[0-9A-F]{2}', k) and value(v)})
    R['DSP48 features'] = sum(1 for k in feats if ttype(k.split('.')[0]) in ('DSP_L', 'DSP_R') and '.DSP_' in k)
    R['BUFGCTRL IN_USE'] = sorted(k.split('.')[0] + '.' + k.split('.')[2] for k in feats if 'BUFGCTRL' in k and k.endswith('IN_USE'))
    R['BUFHCE IN_USE'] = sum(1 for k in feats if 'BUFHCE' in k and k.endswith('IN_USE'))
    R['MMCM/PLL features'] = sorted({'.'.join(k.split('.')[:2]) for k in feats if re.search(r'MMCM|PLLE2|PLL_ADV|MMCME2', k)})
    R['BUFR/BUFIO/IDELAYCTRL features'] = sorted({'.'.join(k.split('.')[:2]) for k in feats if re.search(r'BUFR|BUFIO|IDELAYCTRL', k)})
    R['PCIe (PCIE_BOT) features'] = tt['PCIE_BOT'] + tt['PCIE_TOP']
    R['GTP features (CHANNEL/COMMON)'] = {t: n for t, n in tt.items() if t.startswith('GTP_')}
    other = {t: n for t, n in tt.items() if not re.match(r'INT_[LR]$|CLBL[LM]_[LR]$', t)}
    R['features per tile type (non-INT/CLB)'] = dict(sorted(other.items(), key=lambda x: -x[1]))
    return R


def pcie(feats):
    out = {}
    for k, v in feats.items():
        p = k.split('.')
        if ttype(p[0]) in ('PCIE_BOT', 'PCIE_TOP') and p[1] == 'PCIE':
            name = '.'.join(p[2:])
            m = re.match(r'(\w+)\[(\d+)(?::(\d+))?\]$', name)
            if m:
                base, hi, lo = m.group(1), int(m.group(2)), int(m.group(3) or m.group(2))
                out[base] = out.get(base, 0) | (value(v) << lo)
            else:
                out[name] = value(v)
    return out


def main():
    a = load(sys.argv[1])
    pk = packages()
    print('== %s' % sys.argv[1])
    for k, v in summary(a, pk).items():
        print('%-48s %s' % (k, v))
    io = iobs(a)
    used = {s: d for s, d in io.items() if d['dir'] != 'unused-cfg'}
    print('\n-- IOB sites with configuration: %d (used: %d, only pull/term cfg: %d)'
          % (len(io), len(used), len(io) - len(used)))
    fit = [p for p, pins in pk.items() if all(s in pins for s in used)]
    print('packages (xc7a100t) bonding every used IOB: %s' % fit)
    pins = pk.get('fgg484') if 'fgg484' in fit else (pk[fit[0]] if fit else {})
    dirs = collections.Counter(d['dir'] for d in used.values())
    print('directions: %s' % dict(dirs))
    combos = collections.Counter((d['dir'], '/'.join(d['std']), ','.join(d['drive']), ','.join(d['slew']), ','.join(d['pull']))
                                 for d in used.values())
    print('dir / iostandard-group / drive / slew / pull : count')
    for c, n in combos.most_common():
        print('   %-6s %-40s %-10s %-5s %-8s %d' % (c + (n,)))
    banks = collections.Counter(pins.get(s, ('?', '?'))[1] for s in used)
    print('used IOBs per bank (%s): %s' % ('fgg484' if 'fgg484' in fit else (fit[0] if fit else '?'), dict(sorted(banks.items()))))
    print('pin list (package %s):' % ('fgg484' if 'fgg484' in fit else (fit[0] if fit else '?')))
    for s in sorted(used, key=lambda s: (pins.get(s, ('?', '?'))[1], pins.get(s, ('?',))[0])):
        d = used[s]
        p = pins.get(s, ('?', '?', '?', '?'))
        print('   %-5s bank %-3s %-22s %-16s %-6s %s' % (p[0], p[1], p[2], s[0] + '.' + s[1], d['dir'],
                                                         ' '.join(d['std'] + d['drive'] + d['slew'] + d['pull'])))
    pc = pcie(a)
    if pc:
        print('\n-- PCIE_2_1 attributes (decoded from FASM, value as integer):')
        for k in sorted(pc):
            print('   %-40s 0x%x' % (k, pc[k]))
    if len(sys.argv) > 2:
        b = load(sys.argv[2])
        print('\n== diff %s -> %s' % (sys.argv[1], sys.argv[2]))
        only_a = set(a) - set(b)
        only_b = set(b) - set(a)
        chg = {k for k in set(a) & set(b) if a[k] != b[k]}
        print('features only in A: %d, only in B: %d, value changed: %d' % (len(only_a), len(only_b), len(chg)))
        per = collections.Counter()
        for k in only_a | only_b | chg:
            per[ttype(k.split('.')[0])] += 1
        print('changed features per tile type: %s' % dict(per.most_common()))
        sa, sb = summary(a, pk), summary(b, pk)
        for k in sa:
            if sa[k] != sb[k] and not k.startswith('features per'):
                print('   %-48s %s -> %s' % (k, sa[k], sb[k]))
        ia, ib = iobs(a), iobs(b)
        for s in sorted(set(ia) | set(ib)):
            x, y = ia.get(s), ib.get(s)
            if (x and x['feats']) != (y and y['feats']):
                p = pins.get(s, ('?', '?', '?'))
                print('   IOB %-5s %-22s %s -> %s' % (p[0], s[0] + '.' + s[1],
                                                     x and (x['dir'], x['std'], x['drive'], x['pull']),
                                                     y and (y['dir'], y['std'], y['drive'], y['pull'])))
        pa, pb = pcie(a), pcie(b)
        for k in sorted(set(pa) | set(pb)):
            if pa.get(k) != pb.get(k):
                print('   PCIE %-40s %s -> %s' % (k, pa.get(k), pb.get(k)))
        bra = {k.split('.')[0] for k in only_a | only_b | chg if re.search(r'\.INITP?_[0-9A-F]{2}', k)}
        print('   BRAM tiles whose INIT content differs: %d %s' % (len(bra), sorted(bra)[:12]))


if __name__ == '__main__':
    main()
