#!/usr/bin/env python3
"""Summarise an Evora MDIO trace: per-PHY counts, direct dev-1 registers written (with the distinct values), recorded
polls (same register read repeatedly) and the LMI (indirect, via 1.9005/1.900a, side select 1.8b90) addresses written.
  summarize.py TRACE
LMI framing as seen on the wire (inferred from the trace, not from vendor source): write 1.9005 = cmd, [data lo,
data hi,] addr; cmd 0x1100 = read, 0x1108 = write, +0x0800 when 1.8b90 = 1 (system side); then poll 1.900a
(0x0001 = write done, 0x0007 = read data ready -> two reads of 1.9005 = lo, hi).
"""
import sys, collections

ev = []
for l in open(sys.argv[1]):
    p = l.split()
    if p and p[0] in 'RW':
        ev.append((p[0], int(p[1], 16), int(p[3], 16), int(p[4], 16)))
    elif p and p[0] == 'D':
        ev.append(('D', int(p[1])))
per = collections.Counter((e[1], e[0]) for e in ev if e[0] != 'D')
print('accesses per PHY id:')
for phy in sorted(set(k[0] for k in per)):
    print('  0x%03x  reads %6d  writes %6d' % (phy, per[(phy, 'R')], per[(phy, 'W')]))
print('delays: %d, total %.2f s' % (sum(1 for e in ev if e[0] == 'D'), sum(e[1] for e in ev if e[0] == 'D') / 1e6))
wr = collections.defaultdict(collections.Counter)
rd = collections.Counter()
order = []
for e in ev:
    if e[0] == 'W':
        if e[2] not in wr:
            order.append(e[2])
        wr[e[2]][e[3]] += 1
    elif e[0] == 'R':
        rd[e[2]] += 1
print('\ndirect registers written (dev 1), in order of first write:')
for r in order:
    vals = wr[r]
    s = ' '.join('%04x' % v + ('x%d' % n if n > 1 else '') for v, n in sorted(vals.items())[:10])
    print('  1.%04x  %6d writes, %5d distinct: %s%s' % (r, sum(vals.values()), len(vals), s, ' ...' if len(vals) > 10 else ''))
print('\nregisters only read: ' + ' '.join('1.%04x(%d)' % (r, n) for r, n in sorted(rd.items()) if r not in wr))
# polls
polls = collections.Counter(); maxlen = collections.Counter()
i = 0
while i < len(ev):
    e = ev[i]
    if e[0] == 'R':
        k, cnt = i + 1, 1
        while k < len(ev) and (ev[k][0] == 'D' or (ev[k][0] == 'R' and ev[k][1:3] == e[1:3])):
            cnt += ev[k][0] == 'R'; k += 1
        if cnt > 1:
            polls[e[2]] += 1; maxlen[e[2]] = max(maxlen[e[2]], cnt)
        i = k
    else:
        i += 1
print('\nrecorded polls (register read again before any other access): ' +
      ', '.join('1.%04x x%d (longest %d reads)' % (r, n, maxlen[r]) for r, n in sorted(polls.items())))
# LMI decode
side = {}; lmi = collections.defaultdict(collections.Counter); seq = {}
for e in ev:
    if e[0] == 'W' and e[2] == 0x8b90:
        side[e[1]] = e[3]
    elif e[0] == 'W' and e[2] == 0x9005:
        q = seq.setdefault(e[1], [])
        q.append(e[3])
        if q[0] & 0xf7ff == 0x1100 and len(q) == 2:
            seq[e[1]] = []
        elif q[0] & 0xf7ff == 0x1108 and len(q) == 4:
            lmi[(side.get(e[1], 0), q[3])][(q[2] << 16) | q[1]] += 1
            seq[e[1]] = []
        elif q[0] & 0xf7ff not in (0x1100, 0x1108):
            seq[e[1]] = []
print('\nLMI writes (side, 16-bit address as sent: count, distinct 32-bit values; first values):')
for (sd, a), vals in sorted(lmi.items()):
    print('  %s 0x%04x  x%-5d %s%s' % ('sys ' if sd else 'line', a, sum(vals.values()),
          ' '.join('%08x' % v for v in list(vals)[:6]), ' ...' if len(vals) > 6 else ''))
