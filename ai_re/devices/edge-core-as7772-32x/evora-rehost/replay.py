#!/usr/bin/env python3
"""Replay a recorded Evora MDIO trace (see mdio.py for the format) through the bridge, without the emulator.

Writes and delays are replayed as recorded. Every recorded read is done again and compared:
  - a recorded poll (the same register read again before any other access; delays between the reads belong to
    it) is collapsed: the register is re-read until it shows the value that ended the recorded poll;
  - a single read that differs is retried if it is a status/handshake register (POLL_REGS: LMI status 1.900a,
    download handshake 1.8221, boot status 1.8213/1.8251, verify words); the chip may need longer than it did
    when the trace was recorded;
  - 1.900a (LMI status) clears on read, so it is replayed read for read with the recorded delays; only a read
    recorded as non-zero (done) that returns 0 is repeated until the chip is done;
  - 1.9005 (LMI data FIFO: every read pops a word) is never read twice; a differing value is only reported
    (status values such as temperature legitimately differ);
  - after --max-bad mismatches on POLL_REGS the replay stops, because the chip is then not following the trace.
Reads never change what is written, so a replay on a chip that takes another branch (other strap, other
firmware state) is NOT equivalent to running the vendor code: mismatches are the warning.
"""
import argparse, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mdio


POLL_REGS = {0x900a, 0x8221, 0x8213, 0x8251, 0x8245, 0x8247, 0x8249, 0x824b, 0x8210, 0x8b90}
POP_REGS = {0x9005}
LMI_STATUS = 0x900a


def load(path):
    ev = []
    for l in open(path):
        p = l.split()
        if not p or p[0] == '#':
            continue
        if p[0] == 'D':
            ev.append(('D', int(p[1])))
        else:
            ev.append((p[0], int(p[1], 16), int(p[2], 16), int(p[3], 16), int(p[4], 16)))
    return ev


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('trace')
    ap.add_argument('--backend', default='mock', choices=['mock', 'ssh'])
    ap.add_argument('--ssh', default='sudo ssh -T root@10.60.2.23')
    ap.add_argument('--bridge-log')
    ap.add_argument('--retries', type=int, default=200, help='re-reads of a mismatching read (1 ms apart)')
    ap.add_argument('--max-bad', type=int, default=3, help='stop after this many unresolved status/handshake mismatches')
    ap.add_argument('--strict', action='store_true', help='stop at the first read that never matches')
    ap.add_argument('--no-delay', action='store_true')
    ap.add_argument('--map', action='append', default=[], metavar='OLD=NEW', help='replay phy id OLD (hex) on NEW')
    a = ap.parse_args()
    ev = load(a.trace)
    idmap = dict((int(x.split('=')[0], 16), int(x.split('=')[1], 16)) for x in a.map)
    be = mdio.make_backend(a.backend, a)
    bad = n = polls = retried = soft = 0
    t0 = time.time()
    i = 0
    try:
        while i < len(ev):
            e = ev[i]
            if e[0] == 'D':
                if not a.no_delay and be.real_sleep:
                    if hasattr(be, 'flush'):
                        be.flush()
                    time.sleep(e[1] / 1e6)
            elif e[0] == 'W':
                be.write(idmap.get(e[1], e[1]), e[2], e[3], e[4])
            else:
                # collapse a recorded poll: R x, (D)*, R x, ... -> wait for the last value
                j, last, dly = i, e, 0
                k = i + 1
                while k < len(ev) and e[3] not in POP_REGS and e[3] != LMI_STATUS:
                    if ev[k][0] == 'D':
                        dly = max(dly, ev[k][1]); k += 1; continue
                    if ev[k][0] == 'R' and ev[k][1:4] == e[1:4]:
                        j, last = k, ev[k]; k += 1; continue
                    break
                if j > i:
                    polls += 1
                want = last[4]
                tries = a.retries + (j - i) if e[3] in POLL_REGS else 0
                v = None
                for t in range(tries + 1):
                    v = be.read(idmap.get(e[1], e[1]), e[2], e[3])
                    if v == want or (e[3] == LMI_STATUS and (v or not want)):
                        break
                    if be.real_sleep:
                        time.sleep(max(dly, 1000) / 1e6)
                if t:
                    retried += 1
                if v != want and (e[3] not in POLL_REGS or (e[3] == LMI_STATUS and (v or not want))):
                    soft += 1
                    if soft <= 40:
                        sys.stderr.write('differs  event %d: R %03x %x.%04x recorded %04x, chip %04x\n' % (i, e[1], e[2], e[3], want, v))
                elif v != want:
                    bad += 1
                    sys.stderr.write('MISMATCH event %d: R %03x %x.%04x recorded %04x, chip %04x\n' % (i, e[1], e[2], e[3], want, v))
                    if a.strict or bad >= a.max_bad:
                        sys.stderr.write('STOPPED at event %d of %d: the chip does not follow the trace\n' % (i, len(ev)))
                        raise SystemExit(2)
                n += 1
                i = j
            i += 1
    finally:
        be.close()
    print('replayed %d events in %.1f s: %d reads checked (%d recorded polls collapsed, %d needed a retry), %d status/handshake\n'
          'mismatches, %d data reads with another value' % (len(ev), time.time() - t0, n, polls, retried, bad, soft))
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
