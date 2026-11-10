#!/usr/bin/env python3
"""Per-port sideband/I2C pin table from the full-design simulation.

    sbmap.py SIDEBAND_LOG CHANNEL_MAP.txt OUT.txt

SIDEBAND_LOG: output of the simtb.py testbench run with +sideband=1 (writes
one-hot values to BAR0 0x170 (LPMode) and 0x178 (ResetL) and pulls each input
pin low while reading 0x140 (present) and 0x160 (IntL)).  CHANNEL_MAP: the
layer-7 channel map (driver channel -> I2C pad pair).  Bit k of the BAR0
bitmaps = driver channel k = port k+1 (vendor driver).  Package pins assume
FGG484 (as in the layer-5 reports).
"""
import csv
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PKG = os.path.join(HERE, '..', 'work', 'src', 'prjxray-db', 'artix7', 'xc7a100tfgg484-2', 'package_pins.csv')


def bitpos(x):
    v = int(x, 16)
    return v.bit_length() - 1 if v and v & (v - 1) == 0 else None


def main():
    log, chmap, out = sys.argv[1:4]
    pkg = {r['site']: r for r in csv.DictReader(open(PKG))} if os.path.exists(PKG) else {}
    ins, outs = {}, {}
    pres, intl, lpm, rst = {}, {}, {}, {}
    special = []
    for l in open(log):
        m = re.match(r'in (\d+) (\S+)', l)
        if m:
            ins[int(m.group(1))] = m.group(2)
        m = re.match(r'out (\d+) (\S+)', l)
        if m:
            outs[int(m.group(1))] = m.group(2)
        m = re.match(r'IN (\d+) low -> 0x140 \^ ([0-9a-f]+)', l)
        if m and bitpos(m.group(2)) is not None:
            pres[bitpos(m.group(2))] = ins[int(m.group(1))]
        m = re.match(r'INTL (\d+) low -> 0x160 \^ ([0-9a-f]+)', l)
        if m and bitpos(m.group(2)) is not None:
            intl[bitpos(m.group(2))] = ins[int(m.group(1))]
        m = re.match(r'LPMODE\(present\) bit (\d+) -> out (\d+)', l)
        if m:
            lpm[int(m.group(1))] = outs[int(m.group(2))]
        m = re.match(r'RESET bit (\d+) -> out (\d+)', l)
        if m:
            rst[int(m.group(1))] = outs[int(m.group(2))]
        m = re.match(r'IN (\d+) low -> core stops', l)
        if m:
            special.append(ins[int(m.group(1))])
    i2c = {}
    for l in open(chmap):
        m = re.match(r'(ch\d\d)\s+([AB])\s+0x\w+\s+(\d+)\s+\d+\s+(\S+) \+ (\S+) \((\S+)\)', l)
        if m:
            i2c[int(m.group(3))] = (m.group(1), m.group(2), m.group(4), m.group(5), m.group(6))

    def pin(site):
        return '%s(%s)' % (site, pkg.get(site, {}).get('pin', '?')) if site else '-'
    L = ['# per-port pin table (flash update image), from the full-design simulation', '',
         'port = driver channel + 1.  present = BAR0 0x140 bit, IntL = 0x160 bit, LPMode = 0x170 bit,',
         'ResetL = 0x178 bit (all bit k = driver channel k).  V for the decode structure, I for the',
         'simulation model (see LAYER6.md / CHANNEL.md); pins: IOB site (FGG484 package pin).', '',
         'port ch  l5ch fam  I2C pads (pins)            present          IntL             LPMode           ResetL']
    for k in range(32):
        c = i2c.get(k)
        L.append('%3d  %2d  %s  %s    %-26s %-16s %-16s %-16s %-16s' % (
            k + 1, k, c[0] if c else '?', c[1] if c else '?', ('%s+%s (%s)' % (c[2][4:], c[3][4:], c[4])) if c else '?',
            pin(pres.get(k)), pin(intl.get(k)), pin(lpm.get(k)), pin(rst.get(k))))
    L += ['', 'mapped: present %d, IntL %d, LPMode %d, ResetL %d of 32' % (len(pres), len(intl), len(lpm), len(rst)),
          'inputs that stop the PCIe core while low (reset-like): %s' % ', '.join(pin(s) for s in special)]
    used = set(pres.values()) | set(intl.values()) | set(special)
    L.append('other inputs (no visible effect in this test): %s' % ', '.join(pin(s) for k, s in sorted(ins.items())
                                                                         if s not in used))
    open(out, 'w').write('\n'.join(L) + '\n')
    print('\n'.join(L[-3:]))


if __name__ == '__main__':
    main()
