#!/usr/bin/env python3
"""Compile recorded Evora MDIO traces (mdio.py format, made by rehost.py) into one MDIO script for
openbcm-mdio-script on the switch (format: nix/packages/sai-openbcm/src/mdio_script.h).

  mkscript.py -o OUT.mdio all-init.trace.txt all-cfg.trace.txt [all-pre.trace.txt]

all-pre = the factory TX taps ('extphy pre <id> <lane> ...' for 32 x 4 lanes, preem-25G.soc, then a 'pre <id>' get
per PHY, recorded from PHYs with default taps): masked register writes, compiled like everything else; its reads
of the old and new tap values become E lines, so a PHY whose taps do not read back as recorded shows up in the
runner's count of differing values.

The script is replay.py's behaviour written down operation by operation, so the runner on the switch needs no
knowledge of this chip:
  W, D            as recorded, except delays under 30 us, which become 10 ms (SHORT_DELAY_*, see below)
  R -> E          a plain read (read-modify-write input, FIFO data 1.9005): read once, a different value is only
                  counted; a recorded poll of such a register becomes one read expecting the last value
  R -> P          status/handshake registers (POLL_REGS): poll until the value that ended the recorded poll; the
                  script fails when it never shows
  1.900a          LMI status, clears on read: replayed read for read. Recorded non-zero (done): poll until non-zero;
                  recorded zero: one read, value ignored
Sections whose command is in --skip (default: ver, status: diagnostics the trace happens to end with) are left out;
a section that did not end with rc=0 is refused. A verification block is appended: every PHY id written to must
then read ID 1.8b00/1.8b01 = 2391/80a0, firmware 1.8215 = d00b, checksum 1.8245 = 600d, 1.8251 & 0xd = 0xd
(line/system side active). The output contains the vendor firmware words: keep it under dumps/ and out of git and
out of the Nix store.
"""
import argparse, sys

POLL_REGS = {0x900a, 0x8221, 0x8213, 0x8251, 0x8245, 0x8247, 0x8249, 0x824b, 0x8210, 0x8b90}
POP_REGS = {0x9005}
# Delays the vendor asks for as "1 us" (usleep(1): in fact at least a scheduler wake-up on its Linux). All 192 of
# them follow the start of an engine operation (message register 0x00c05001 bit 0) whose done flags (0xd148 =
# 7f00) are read right after. The engine needs about 1 ms: a replay with the delay as recorded read 6000 on 147
# to 152 of the 192, one with sleeps rounded up to a 4 ms tick on 40, one on a 2 MHz MDIO clock on none. A
# replay cannot repeat a read, so the wait is made long enough instead.
SHORT_DELAY_BELOW, SHORT_DELAY_US = 30, 10000
LMI_STATUS = 0x900a
VERIFY = [(0x8b00, 0x2391, 0xffff), (0x8b01, 0x80a0, 0xffff), (0x8215, 0xd00b, 0xffff), (0x8245, 0x600d, 0xffff),
          (0x8251, 0x000d, 0x000d)]


def sections(path):
    """[(command, [events])] split at the '# extphy ...' / '# rc=N' markers."""
    out, cur, name = [], None, None
    for n, l in enumerate(open(path), 1):
        p = l.split()
        if not p:
            continue
        if p[0] == '#':
            text = l[1:].strip()
            if text.startswith('rc='):
                if cur is None:
                    raise SystemExit('%s:%d: rc marker without a command' % (path, n))
                if text != 'rc=0':
                    raise SystemExit('%s:%d: "%s" ended with %s: not a trace to replay' % (path, n, name, text))
                out.append((name, cur)); cur = None
            else:
                if cur is not None:
                    raise SystemExit('%s:%d: "%s" has no rc marker (aborted run?)' % (path, n, name))
                name, cur = text, []
            continue
        if cur is None:
            raise SystemExit('%s:%d: event outside a command section' % (path, n))
        if p[0] == 'D':
            cur.append(('D', int(p[1])))
        elif p[0] in ('R', 'W'):
            cur.append((p[0], int(p[1], 16), int(p[2], 16), int(p[3], 16), int(p[4], 16)))
        else:
            raise SystemExit('%s:%d: unknown event %r' % (path, n, l))
    if cur is not None:
        raise SystemExit('%s: "%s" has no rc marker (aborted run?)' % (path, name))
    return out


def compile_events(ev, retries, w):
    i = 0
    while i < len(ev):
        e = ev[i]
        if e[0] == 'D':
            w('D %d' % (SHORT_DELAY_US if e[1] < SHORT_DELAY_BELOW else e[1]))
        elif e[0] == 'W':
            if e[2] == 0:
                raise SystemExit('clause-22 write in the trace: refused')
            w('W %x %x %04x %04x' % e[1:])
        else:
            j, last, dly, k = i, e, 0, i + 1
            while k < len(ev) and e[3] not in POP_REGS and e[3] != LMI_STATUS:
                if ev[k][0] == 'D':
                    dly = max(dly, ev[k][1]); k += 1; continue
                if ev[k][0] == 'R' and ev[k][1:4] == e[1:4]:
                    j, last = k, ev[k]; k += 1; continue
                break
            want = last[4]
            a = '%x %x %04x' % e[1:4]
            if e[3] == LMI_STATUS:
                if want:
                    w('N %s 0000 ffff %d 1000' % (a, retries + 1))
                else:
                    w('R ' + a)
            elif e[3] in POLL_REGS:
                w('P %s %04x ffff %d %d' % (a, want, retries + (j - i) + 1, max(dly, 1000)))
            else:
                w('E %s %04x' % (a, want))
            i = j
        i += 1


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('traces', nargs='+')
    ap.add_argument('-o', '--out', required=True)
    ap.add_argument('--retries', type=int, default=2000, help='extra re-reads of a poll, 1 ms apart (default 2000)')
    ap.add_argument('--skip', default='ver,status', help='extphy sub-commands to leave out')
    ap.add_argument('--no-verify', action='store_true')
    a = ap.parse_args()
    skip = set(a.skip.split(','))
    lines = ['mdio-script 1', '# Evora (BCM82391) bring-up compiled by mkscript.py from: ' + ' '.join(t.split('/')[-1] for t in a.traces),
             '# PRIVATE: contains vendor firmware. Not for git, not for the Nix store.']
    phys, n = set(), 0
    for t in a.traces:
        for name, ev in sections(t):
            cmd = name.split()
            if len(cmd) > 1 and cmd[0] == 'extphy' and cmd[1] in skip:
                continue
            lines.append('L ' + name)
            compile_events(ev, a.retries, lines.append)
            phys |= set(e[1] for e in ev if e[0] == 'W')
            n += 1
    if not n:
        raise SystemExit('no command section left to compile')
    if not a.no_verify:
        for phy in sorted(phys):
            lines.append('L verify phy 0x%x: id 2391/80a0, firmware d00b, checksum 600d, both sides active' % phy)
            for reg, val, mask in VERIFY:
                lines.append('P %x 1 %04x %04x %04x 1 0' % (phy, reg, val, mask))
    with open(a.out, 'w') as f:
        f.write('\n'.join(lines) + '\n')
    ops = {}
    for l in lines:
        ops[l[0]] = ops.get(l[0], 0) + 1
    print('%s: %d sections, %d PHY ids, %s' % (a.out, n, len(phys), ', '.join('%s %d' % (k, v) for k, v in sorted(ops.items()) if k in 'WDREPNL')))


if __name__ == '__main__':
    main()
