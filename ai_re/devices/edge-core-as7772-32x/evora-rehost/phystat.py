#!/usr/bin/env python3
"""PHY-side counters of Evora PHYs through the re-hosted vendor 'extphy reg64 get', SONiC running.

  phystat.py [--taps] [--json] [--backend diag|ssh] [--txn phy|all|off] 0x00 0x01 0x17e 0x17f

One line of JSON per PHY (--json) or a table. Uses the saved library state of the init run (all-cfg.state) and,
by default, the 'diag' backend (clause 45 through syncd's diag socket, see mdio.py), each register read (one vendor
'reg64 get', a firmware-message conversation) inside its own diag transaction on that PHY (--txn phy; 'all': on every
PHY; 'off': bare raw MDIO, for a libsai without 'extphy txn'), so syncd's own MDIO users stay off the PHY during the
conversation. Note: reading counters that clear on read takes those counts away from SONiC. Register addresses are the
vendor's (status pmdiag prints them: 0x42.. = TSCF/Falcon, 0x41.. = CLMAC/MIB); the RS-FEC counter addresses are
from the public OpenBCM TSCF-16nm register header (the Evora PCS matches that layout: its 0x92b0 name is the
16nm one), read back as plausible on this chip but not documented for it:
  line side : c352/c353 corrected codewords lo/hi, c354/c355 uncorrected lo/hi, c360..c367 symbol errors per FEC
              lane lo/hi (all clear on read), c156/c161 PCS live status, c154/c155 latched block/AM lock
  sys side  : a082/a083 CL82 BER count (no FEC towards the ASIC), c156/c161
  both      : CLMIB counters of the PHY's two MACs (not clear on read)
  temp      : Falcon die temperature, line side core: 410.04 - 0.48705 * (0xd03e & 0x3ff)  (formula of the public
              falcon API; agrees with the vendor 'status dsc' LIVE_TEMP). 0xd03e is the microcode's command data
              mailbox: it holds a temperature only after a die-temperature command (a write to 0xd03d, which 'status
              dsc' makes and this tool does not), so the value is the one latched then, not live, on every backend;
              temp_c is None while the raw value is 0 (no such command since boot; 410.0 C before), temp_raw is kept
"""
import argparse, contextlib, json, os, re, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rehost

T = os.path.join(rehost.HERE, '..', 'dumps/x/evora-trace')
MIB = {'RPKT': 0x0b, 'RFCS': 0x0f, 'RERPKT': 0x18, 'RBYT': 0x3d, 'RXPF': 0x11, 'ROVR': 0x1a, 'RJBR': 0x1b,
       'RMTUE': 0x1c, 'RUND': 0x34, 'RFRG': 0x35, 'R16383': 0x0a, 'TPKT': 0x4c, 'TFCS': 0x53, 'TERR': 0x5d,
       'TBYT': 0x6f, 'TXPF': 0x50, 'TUFL': 0x61}


class Phy:
    def __init__(self, backend='diag', state=None, ssh='sudo ssh -T root@10.60.2.23', txn='phy'):
        a = argparse.Namespace(bin=rehost.DEFAULT_BIN, backend=backend, ssh=ssh, bridge_log=None, trace=None, log=None,
                               verbose=False, strace=False, itrace=None)
        self.e = rehost.Emu(a)
        self.buf = []
        self.e.say = lambda s: self.buf.append(s)
        self.e.boot()
        self.e.load_state(state or os.path.join(T, 'all-cfg.state'))
        self.txn = txn if hasattr(self.e.be, 'transaction') else 'off'

    def cmd(self, line):
        self.buf = []
        rc = self.e.extphy(line)
        return rc, ''.join(self.buf)

    def reg(self, phy, addr, side=0, lane=0):
        scope = None if self.txn == 'off' else ('all' if self.txn == 'all' else [phy])
        with (self.e.be.transaction(scope) if scope else contextlib.nullcontext()):
            rc, out = self.cmd('reg64 get 0x%x 0x%x lane=%d if_side=%d' % (phy, addr, lane, side))
        m = re.search(r'value64 = (0x[0-9a-fA-F]+|\d+)', out)
        if rc or not m:
            raise RuntimeError('reg64 get failed: rc=%d %r' % (rc, out))
        return int(m.group(1), 0)

    def pair(self, phy, lo, side=0):
        l = self.reg(phy, 0x42020000 | lo, side); h = self.reg(phy, 0x42020000 | (lo + 1), side)
        return (h << 16) | l

    def snapshot(self, phy, taps=False):
        r = {'phy': phy, 't': time.time()}
        r['temp_raw'] = self.reg(phy, 0x4200d03e) & 0x3ff   # latched by the last die-temperature command, see above
        r['temp_c'] = round(410.04 - 0.48705 * r['temp_raw'], 1) if r['temp_raw'] else None
        r['fec_corr'] = self.pair(phy, 0xc352)
        r['fec_uncorr'] = self.pair(phy, 0xc354)
        r['fec_symerr'] = [self.pair(phy, 0xc360 + 2 * i) for i in range(4)]
        for side, n in ((0, 'line'), (1, 'sys')):
            r[n + '_pcs_live'] = [self.reg(phy, 0x4202c156, side), self.reg(phy, 0x4202c161, side)]
            r[n + '_lock_latched'] = [self.reg(phy, 0x4202c154, side), self.reg(phy, 0x4202c155, side)]
            r[n + '_mib'] = {k: self.reg(phy, 0x41000000 | v, side) for k, v in MIB.items()}
        r['sys_cl82_ber'] = self.pair(phy, 0xa082, 1)
        if taps:
            for side, n in ((0, 'line'), (1, 'sys')):
                t = []
                for lane in range(4):
                    # the registers the vendor 'pre' set writes (trace of 2026-11-22): d094 = pre (low byte) /
                    # post1 (high byte), d095 = main, d131 = post2, d132 = post3, d0d2 = amplitude
                    a = self.reg(phy, 0x4200d094, side, lane)
                    t.append({'pre': a & 0xff, 'post': a >> 8, 'main': self.reg(phy, 0x4200d095, side, lane),
                              'post2': self.reg(phy, 0x4200d131, side, lane), 'post3': self.reg(phy, 0x4200d132, side, lane),
                              'amp': self.reg(phy, 0x4200d0d2, side, lane)})
                r[n + '_txfir_raw'] = t
        return r

    def close(self):
        self.e.be.close()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('phys', nargs='+')
    ap.add_argument('--backend', default='diag')
    ap.add_argument('--state')
    ap.add_argument('--taps', action='store_true', help='also the TX FIR registers of both sides, per lane')
    ap.add_argument('--json', action='store_true')
    ap.add_argument('--interval', type=float, default=0, help='repeat every N seconds until killed')
    ap.add_argument('--txn', choices=['phy', 'all', 'off'], default='phy',
                    help="diag transaction per register read: on its PHY (default), on all PHYs, or none")
    a = ap.parse_args()
    p = Phy(a.backend, a.state, txn=a.txn)
    try:
        while True:
            for s in a.phys:
                r = p.snapshot(int(s, 0), a.taps)
                if a.json:
                    print(json.dumps(r), flush=True)
                    continue
                print('phy 0x%03x  %s C (latched)  FEC corrected %d uncorrected %d symerr %s  sys CL82 BER %d' % (
                    r['phy'], '-' if r['temp_c'] is None else '%.1f' % r['temp_c'], r['fec_corr'], r['fec_uncorr'],
                    r['fec_symerr'], r['sys_cl82_ber']))
                for n in ('line', 'sys'):
                    m = r[n + '_mib']
                    print('  %-4s pcs_live %s latched %s  %s' % (n, ['%04x' % x for x in r[n + '_pcs_live']],
                          ['%04x' % x for x in r[n + '_lock_latched']], ' '.join('%s=%d' % kv for kv in m.items() if kv[1] or kv[0] in ('RPKT', 'TPKT', 'RFCS'))))
                    if a.taps:
                        print('       txfir ' + ' | '.join('pre %(pre)d main %(main)d post %(post)d post2 %(post2)d post3 %(post3)d amp %(amp)d' % t for t in r[n + '_txfir_raw']))
                sys.stdout.flush()
            if not a.interval:
                break
            time.sleep(a.interval)
    finally:
        p.close()


if __name__ == '__main__':
    main()
