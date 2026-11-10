#!/usr/bin/env python3
"""Compare the register writes of a fake-PHY SAI run against the post-vendor
register image (seeds.txt from trace2fake.py).

usage: check-diff.py RUN_DIR [SEEDS] [-v]
  RUN_DIR/access.txt   fakephy log ("W t ctx addr reg val")
  SEEDS                default RUN_DIR/seeds.txt

Every write is classed as
  transient  mailbox b71b/b71c/b723, handshake b718, firmware-variable window
             b8fc/b8fe and b900..b90f (inherently per-exchange values)
  same       lane/config write whose value equals the post-vendor value
  DIFF       lane/config write whose value differs from the post-vendor value
  unknown    lane/config write to a register the vendor trace never touched

TCM writes (fakephy "# tcm W addr val vendor old" lines, mailbox 5040) are
compared with the post-vendor TCM model RUN_DIR/tcm.txt the same way: these
go through "transient" registers but change MCU memory.

  --phase NAME   only count accesses after "# phase NAME" (until next phase)

Exit status 0 iff there are no DIFF/unknown register writes and no DIFF/unknown
TCM writes ("diff = 0").
"""
import collections, sys

TRANSIENT = {0xb71b, 0xb71c, 0xb723, 0xb718, 0xb8fc, 0xb8fe} | set(range(0xb900, 0xb910))

argv = sys.argv[1:]
phase = None
if '--phase' in argv:
    k = argv.index('--phase')
    phase = argv[k + 1]
    del argv[k:k + 2]
args = [a for a in argv if not a.startswith('-')]
verbose = '-v' in argv
run = args[0]
seeds_path = args[1] if len(args) > 1 else f'{run}/seeds.txt'
seed = {}
for line in open(seeds_path):
    r, v = line.split()
    seed[int(r, 16)] = int(v, 16)

cls = collections.Counter()
diff_regs = collections.OrderedDict()
unknown_regs = collections.OrderedDict()
last = {}
nwrites = 0
tcm_seed = {}
try:
    for line in open(f'{run}/tcm.txt'):
        a, v, _ = line.split()
        tcm_seed[int(a, 16)] = int(v, 16)
except FileNotFoundError:
    pass
tcm_cls = collections.Counter()
tcm_diff = []
cur = None
for line in open(f'{run}/access.txt'):
    f = line.split()
    if line.startswith('# phase '):
        cur = f[2]
        continue
    if phase and cur != phase:
        continue
    if line.startswith('# tcm W '):
        a, v = int(f[3], 16), int(f[4], 16)
        if a not in tcm_seed:
            tcm_cls['unknown'] += 1
            tcm_diff.append((a, None, v))
        elif tcm_seed[a] == v:
            tcm_cls['same'] += 1
        else:
            tcm_cls['DIFF'] += 1
            tcm_diff.append((a, tcm_seed[a], v))
        continue
    if not f or f[0] != 'W':
        continue
    nwrites += 1
    r, v = int(f[4], 16), int(f[5], 16)
    last[r] = v
    if r in TRANSIENT:
        cls['transient'] += 1
    elif r not in seed:
        cls['unknown'] += 1
        unknown_regs.setdefault(r, []).append(v)
    elif v == seed[r]:
        cls['same'] += 1
    else:
        cls['DIFF'] += 1
        diff_regs.setdefault(r, []).append(v)

final_diff = {r: v for r, v in last.items()
              if r not in TRANSIENT and r in seed and seed[r] != v}
print(f'{run}{" phase " + phase if phase else ""}: {nwrites} writes: ' + ', '.join(f'{k} {cls[k]}' for k in ('transient', 'same', 'DIFF', 'unknown')))
print(f'  DIFF registers {len(diff_regs)}, final-state differences {len(final_diff)}, unknown registers {len(unknown_regs)}')

def lane_of(r):
    return f'lane {r >> 9:2d} +{r & 0x1ff:03x}' if r < 0x3000 else 'global       '

by_off = collections.Counter()
for r in diff_regs:
    by_off[f'+{r & 0x1ff:03x}' if r < 0x3000 else f'{r:04x}'] += 1
if by_off:
    print('  DIFF by offset: ' + ', '.join(f'{k} x{n}' for k, n in sorted(by_off.items())))
if verbose or len(diff_regs) <= 40:
    for r, vs in diff_regs.items():
        print(f'  DIFF {r:04x} ({lane_of(r)}) vendor {seed[r]:04x} written ' + ' '.join(f'{v:04x}' for v in vs)
              + ('' if r not in final_diff else '  [final differs]'))
for r, vs in unknown_regs.items():
    print(f'  unknown {r:04x} ({lane_of(r)}) written ' + ' '.join(f'{v:04x}' for v in vs))
if tcm_cls or tcm_seed:
    print('  TCM writes (mailbox 5040): ' + ', '.join(f'{k} {tcm_cls[k]}' for k in ('same', 'DIFF', 'unknown')))
    for a, old, v in tcm_diff:
        print(f'  TCM {"DIFF" if old is not None else "unknown"} {a:08x} vendor '
              + ('--------' if old is None else f'{old:08x}') + f' written {v:08x}')
sys.exit(0 if not diff_regs and not unknown_regs and not tcm_diff else 1)
