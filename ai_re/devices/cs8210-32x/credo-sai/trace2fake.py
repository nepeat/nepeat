#!/usr/bin/env python3
"""Turn a libMdio trace of the vendor bring-up into fake-PHY inputs.

usage: trace2fake.py TRACE[.gz] OUTDIR [PHY]
  seeds.txt  "reg val": last value seen per register on PHY (default 02),
             i.e. the register image of a slice after a real bring-up.
  mbox.txt   "cmd a723 a71c resp data count [d723]": every mailbox exchange
             seen on any PHY (args in b723/b71c, command written to b71b, the
             MCU answers by rewriting b71b; data read back from b71c and, for
             some commands, b723).
  tcm.txt    "addr val how": last known value of each MCU TCM word on PHY,
             from TCM reads (address in b8fe:b8fc, value b8f6:b8f8) and TCM
             writes (mailbox 5040: address b900 | (b901 & 0xff00) << 9,
             value b723:b71c). Encoding inferred from the trace (README step 2).
"""
import collections, gzip, sys

path, out = sys.argv[1], sys.argv[2]
want = sys.argv[3] if len(sys.argv) > 3 else '02'
op = gzip.open if path.endswith('.gz') else open
last = {}
mbox = collections.Counter()
st = {}  # per phy: a723, a71c, cmd, resp
tcm = {}  # PHY `want` only: addr -> (val, 'R'|'W')
tst = {'b8fc': 0, 'b8fe': 0, 'b8f6': None, 'b900': 0, 'b901': 0}
for line in op(path, 'rt'):
    t, rw, phy, reg, val, ret = line.split()
    if ret != '0':
        continue
    r, v = int(reg, 16), int(val, 16)
    if phy == want:
        last[r] = v
        if rw == 'W' and reg in ('b8fc', 'b8fe', 'b900', 'b901'):
            tst[reg] = v
            if reg in ('b8fc', 'b8fe'):
                tst['b8f6'] = None
        elif rw == 'R' and reg == 'b8f6':
            tst['b8f6'] = v
        elif rw == 'R' and reg == 'b8f8' and tst['b8f6'] is not None:
            tcm[tst['b8fe'] << 16 | tst['b8fc']] = (tst['b8f6'] << 16 | v, 'R')
        elif rw == 'W' and reg == 'b71b' and v == 0x5040:
            s0 = st.get(phy, {'a723': 0, 'a71c': 0})
            tcm[tst['b900'] | (tst['b901'] & 0xff00) << 9] = (s0['a723'] << 16 | s0['a71c'], 'W')
    s = st.setdefault(phy, {'a723': 0, 'a71c': 0, 'cmd': None, 'resp': None, 'data': -1, 'd723': -1})
    if rw == 'W' and s['cmd'] is not None and s['resp'] is not None:
        # next write after the answer: the exchange is finished
        mbox[(s['cmd'], s['a723'], s['a71c'], s['resp'], s['data'], s['d723'])] += 1
        s['cmd'] = None
    if rw == 'W' and r == 0xb723:
        s['a723'] = v
    elif rw == 'W' and r == 0xb71c:
        s['a71c'] = v
    elif rw == 'W' and r == 0xb71b:
        s['cmd'], s['resp'], s['data'], s['d723'] = v, None, -1, -1
    elif rw == 'R' and r == 0xb71b and s['cmd'] is not None and v != s['cmd']:
        s['resp'] = v
    elif rw == 'R' and r == 0xb71c and s['resp'] is not None:
        s['data'] = v
    elif rw == 'R' and r == 0xb723 and s['resp'] is not None:
        s['d723'] = v
with open(f'{out}/seeds.txt', 'w') as f:
    for r in sorted(last):
        f.write(f'{r:04x} {last[r]:04x}\n')
with open(f'{out}/mbox.txt', 'w') as f:
    for (c, a, b, resp, d, d7), n in sorted(mbox.items()):
        f.write(f'{c:04x} {a:04x} {b:04x} {resp:04x} {f"{d:04x}" if d >= 0 else "-"} {n}'
                + (f' {d7:04x}' if d7 >= 0 else '') + '\n')
with open(f'{out}/tcm.txt', 'w') as f:
    for a in sorted(tcm):
        f.write(f'{a:08x} {tcm[a][0]:08x} {tcm[a][1]}\n')
print(f'{len(tcm)} TCM words; {len(last)} seeded registers, {len(mbox)} distinct mailbox exchanges, '
      f'{len({k[0] for k in mbox})} commands: ' + ' '.join(sorted({f"{k[0]:04x}" for k in mbox})))
