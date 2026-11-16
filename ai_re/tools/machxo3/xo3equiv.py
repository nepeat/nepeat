#!/usr/bin/env python3
"""Formal equivalence: lifted RTL (xo3lift.py) vs the decoded netlist (xo3vlog.py --sim). Generic.

    xo3equiv.py SIM.v RTLDIR WORKDIR [--seq N]      then: yosys -q -s WORKDIR/equiv.ys

Gold = SIM.v with every register renamed to the RTL's hierarchical name (RTLDIR/regmap.json), gate =
RTLDIR/*.v flattened. Register correspondence makes the proof an induction over matched state:
equiv_make pairs every same-named wire (ports + registers), equiv_simple proves the pairs over their
combinational cones (-seq N looks N steps back), equiv_induct proves the rest by temporal induction
assuming all pairs equal in the previous step, equiv_status -assert fails on any unproven pair.
Async resets: async2sync on both sides alike. The 8-or-so LUT-gated latches: both sides get the same\ncycle model (q = r ? v : en ? d : q_hold, q_hold sampled each OSC edge, matched by name), so the proof\ncovers their reset/enable/data functions; the latch timing itself is the same primitive on both sides.
"""
import json, os, sys, argparse
ap = argparse.ArgumentParser()
ap.add_argument('sim'); ap.add_argument('rtldir'); ap.add_argument('work')
ap.add_argument('--seq', type=int, default=1)
a = ap.parse_args()
os.makedirs(a.work, exist_ok=True)
rm = json.load(open(os.path.join(a.rtldir, 'regmap.json')))
import re
src = open(a.sim).read()
def latch(m):
    r, q, v, en, d = m.groups()
    return ('reg %s_hold; initial %s_hold = %s; always @(posedge osc) %s_hold <= %s;\n'
            'always @* %s = (%s) ? %s : (%s) ? %s : %s_hold;' % (q, q, 'INIT_' + q, q, q, q, r, v, en, d, q))
src = re.sub(r"^always @\*\s*if \((.*?)\) (\w+) = (1'b[01]); else if \((.*?)\) \w+ = (\w+);", latch, src, flags=re.M)
for q, v in re.findall(r"^initial (\w+) = (1'b[01]);", src, re.M):
    src = src.replace('INIT_' + q, v)
gold = os.path.join(os.path.abspath(a.work), 'gold.v')
open(gold, 'w').write(src)
vs = sorted(os.path.join(os.path.abspath(a.rtldir), f) for f in os.listdir(a.rtldir) if f.endswith('.v'))
L = ['read_verilog -DXO3_EQUIV %s' % ' '.join(vs), 'hierarchy -top top', 'proc', 'flatten', 'splitnets', 'opt_clean', 'rename top gate',
     'design -stash gate',
     'read_verilog %s' % gold, 'proc', 'opt_clean', 'rename top gold', 'cd gold']
for k, v in sorted(rm.items()):
    L.append('rename %s \\%s' % (k, v))
L += ['cd ..', 'design -copy-from gate -as gate gate',
      'equiv_make gold gate equiv', 'hierarchy -top equiv', 'async2sync',
      'equiv_simple -seq %d' % a.seq, 'equiv_induct', 'tee -o %s/equiv_status.txt equiv_status' % os.path.abspath(a.work),
      'equiv_status -assert']
open(os.path.join(a.work, 'equiv.ys'), 'w').write('\n'.join(L) + '\n')
print('wrote', os.path.join(a.work, 'equiv.ys'), len(rm), 'register renames')
