#!/usr/bin/env python3
"""Read clause-45 1.8b00 / 1.8b01 (Evora chip id) at the 32 vendor PHY ids through the bridge; print one line per id.
  idscan.py [--label TEXT] [--extra REG ...]      (read-only)
"""
import argparse, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mdio

IDS = [b + i for b in (0x00, 0x24, 0x48, 0x6c, 0x110, 0x134, 0x158, 0x17c) for i in range(4)]


def scan(be, extra=()):
    rows = []
    for n, i in enumerate(IDS):
        v = [be.read(i, 1, r) for r in (0x8b00, 0x8b01) + tuple(extra)]
        rows.append((n + 1, i, v))
    return rows


def fmt(rows, label, extra=()):
    ok = [i for _, i, v in rows if v[0] in (0x2391, 0x2392)]
    out = ['# %s  %s' % (time.strftime('%Y-%m-%d %H:%M:%S'), label),
           '# port id    1.8b00 1.8b01' + ''.join(' 1.%04x' % r for r in extra)]
    for n, i, v in rows:
        out.append('  %2d  0x%03x  ' % (n, i) + '   '.join('%04x' % x for x in v) + ('' if i in ok else '   <- no id'))
    out.append('# answering: %d of 32; not answering: %s' % (len(ok), ' '.join('0x%x' % i for _, i, _ in rows if i not in ok) or 'none'))
    return '\n'.join(out) + '\n'


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('--ssh', default='sudo ssh -T root@10.60.2.23')
    ap.add_argument('--label', default='')
    ap.add_argument('--extra', nargs='*', default=[], help='more dev-1 registers (hex)')
    ap.add_argument('--bridge-log')
    a = ap.parse_args()
    ex = tuple(int(x, 16) for x in a.extra)
    be = mdio.Ssh(a.ssh.split(), log=a.bridge_log)
    try:
        sys.stdout.write(fmt(scan(be, ex), a.label, ex))
    finally:
        be.close()
