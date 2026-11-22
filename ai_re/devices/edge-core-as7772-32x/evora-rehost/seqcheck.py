#!/usr/bin/env python3
"""Check the board's external MACsec table against bus traces of the vendor library doing the same operations.

  seqcheck.py --table ext-macsec.tbl --render openbcm-ext-macsec SESSION...
SESSION = path prefix of a recording made by the session of ../macsec.md "Under SONiC" (S.trace.txt + S.params.json,
both under dumps/: they hold the test keys). Every section of the trace is paired with the table sequence that the
SAI layer would run for it, the sequence is rendered by sai-openbcm's own executor (openbcm-ext-macsec --render)
with the session's parameters, and compared operation by operation: writes and delays must be identical, a read
must be the same register; the executor is fed the recorded read values, so where the table compares a read the
recorded value must pass, and values the table keeps and writes back are the recording's.
Nothing is printed that contains key bytes.
"""
import argparse, json, os, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import seqtool

CIPHER = {'128': 'gcm-aes-128', '256': 'gcm-aes-256', 'x128': 'gcm-aes-xpn-128', 'x256': 'gcm-aes-xpn-256'}


def aes_ecb(key, block):
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    e = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    return e.update(block) + e.finalize()


def render(a, seq, phy, par=None, reads=None):
    """reads: the trace's read values from the place the sequence starts (the executor then compares and keeps them)."""
    cmd = [a.render, '--render', a.table, seq, '%x' % phy]
    tmp = []
    for content in ([''.join('%s %s\n' % (k, v.hex()) for k, v in par.items())] if par else ['-']) + \
            ([' '.join('%x' % v for v in reads)] if reads is not None else []):
        if content == '-':
            cmd.append('-')
            continue
        f = tempfile.NamedTemporaryFile('w', dir=os.path.dirname(a.table) or '.', suffix='.tmp', delete=False)
        f.write(content)
        f.close()
        tmp.append(f.name)
        cmd.append(f.name)
    try:
        out = subprocess.run(cmd, capture_output=True, text=True)
    finally:
        for t in tmp:
            os.unlink(t)
    if out.returncode:
        raise SystemExit('render %s failed: %s' % (seq, out.stderr.strip()))
    return [l.split() for l in out.stdout.splitlines()]


def compare(exp, ops, pos, what):
    """Match rendered lines against trace ops from pos; returns the new position or raises."""
    for i, e in enumerate(exp):
        if pos >= len(ops):
            raise AssertionError('%s: trace ends at rendered operation %d of %d' % (what, i, len(exp)))
        o = ops[pos]
        ok = False
        if e[0] == 'D':
            ok = o[0] == 'D' and o[1] == int(e[1])
        elif o[0] == e[0]:
            same = (o[1], o[2], o[3]) == (int(e[1], 16), int(e[2], 16), int(e[3], 16))
            if e[0] == 'W':
                ok = same and o[4] == int(e[4], 16)
            else:
                ok = same and (len(e) < 6 or (o[4] & int(e[5], 16)) == int(e[4], 16))
        if not ok:
            # registers only: a value could be key material
            raise AssertionError('%s: operation %d differs: table %s, trace %s' % (
                what, i, ' '.join(e[:4]), ' '.join(str(x) if o[0] == 'D' else ('%x' % x if isinstance(x, int) else x) for x in o[:4])))
        pos += 1
    return pos


def sa_params(P, d, an):
    a = [x for x in P['an'] if x['dir'] == d and x['an'] == an][0]
    key = bytes.fromhex(a['key'])
    par = {'key': key, 'hkey': aes_ecb(key, b'\0' * 16), 'sci': bytes.fromhex(P['sci_e' if d == 'egress' else 'sci_i']),
           'pn': (a['pn'] | (a['pn_hi'] << 32)).to_bytes(8, 'little'), 'window': int(P['window']).to_bytes(4, 'little')}
    if a['salt']:
        salt, ssci = bytes.fromhex(a['salt']), bytes.fromhex(a['ssci'])
        par.update(salt=salt, ssci=ssci, iv=bytes(x ^ y for x, y in zip(salt, ssci + b'\0' * 8)))
    return CIPHER[a['cipher']], par


def session(a, prefix):
    P = json.load(open(prefix + '.params.json'))
    phy, n, total = P['phy'], 0, 0
    secs = seqtool.sections(prefix + '.trace.txt')
    enable, devupd, skipped = {}, set(), []
    for title, ops in secs:
        w = title.split()
        if w[0] != 'macsec':
            continue
        cmd, opt = w[1], dict(x.split('=', 1) for x in w[3:] if '=' in x)
        d = opt.get('side')
        name = opt.get('name')
        plan = None          # list of (sequence, params) or ('skip', count)
        if cmd in ('bypass', 'x-vport', 'x-rule') or (cmd == 'x-sa' and name == 'byp'):
            if cmd == 'bypass' and w[3] == '1':
                enable.setdefault(('dis', d), []).extend(ops)      # the last piece of disable
                plan = [('disable.' + d, None)]
                ops = enable.pop(('dis', d))
                if d not in devupd:
                    # a recording from before the teardown put the classifier back into its bypass: it left the
                    # port dropping every frame, and is not what "disable" is
                    skipped.append('disable.' + d)
                    continue
            else:
                enable.setdefault(d, []).extend(ops)               # enable = five library calls in a row
                if cmd == 'x-rule' and name == 'data':
                    plan = [('enable.' + d, None)]
                    ops = enable.pop(d)
                else:
                    continue
        elif cmd in ('x-rule-del', 'x-vport-del', 'x-devupd') or (cmd == 'x-sa-del' and name == 'byp'):
            enable.setdefault(('dis', d), []).extend(ops)
            if cmd == 'x-devupd':
                devupd.add(d)
            continue
        elif cmd == 'x-sa':
            an = int(opt['an'])
            cipher, par = sa_params(P, d, an)
            first = an == 0
            plan = [('sa-add%s.%s.%d.%s%s' % ('-first' if first else '', d, an, cipher,
                                               '.replay' if first and d == 'ingress' and P['replay'] else ''), par)]
        elif cmd == 'x-rule-upd':
            plan = [('gate-%s.%s' % ('close' if opt['drop'] == '1' else 'open', d), None)]
        elif cmd == 'x-sa-read':
            plan = [('pn.%s.%s' % (d, name[2:]), None)]
        elif cmd == 'x-sa-del':
            an = int(name[2:])
            plan = [('sa-del%s.%s.%d' % ('-last' if an == 3 else '', d, an), None)]
        elif cmd == 'x-stats':
            sa = len(render(a, 'stats-sa.%s.0' % d, phy))
            fl = len(render(a, 'stats-flow.' + d, phy))
            # the library read: SA an0..an3, the BYPASS SA, the control vPort, the data vPort
            plan = [('stats-sa.%s.%d' % (d, i), None) for i in range(4)] + [('skip', sa), ('skip', fl), ('stats-flow.' + d, None)]
        elif cmd in ('bypass?', 'reg'):
            continue
        else:
            raise SystemExit('unmapped section: ' + cmd)
        pos = 0
        for seq, par in plan:
            if seq == 'skip':
                pos += par
                continue
            exp = render(a, seq, phy, par, [o[4] for o in ops[pos:] if o[0] == 'R'])
            pos = compare(exp, ops, pos, '%s [%s]' % (seq, cmd))
            n += 1
            total += len(exp)
        if pos != len(ops):
            raise AssertionError('%s: %d trace operations left after the table sequence(s)' % (cmd, len(ops) - pos))
    if enable:
        raise AssertionError('unfinished enable/disable groups: %s' % list(enable))
    print('%s: PHY 0x%x: %d sequences, %d bus operations identical to the trace%s' % (
        os.path.basename(prefix), phy, n, total, ' (not compared: %s, an incomplete teardown in this recording)' % ', '.join(skipped) if skipped else ''))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--table', required=True)
    ap.add_argument('--render', required=True)
    ap.add_argument('sessions', nargs='+')
    a = ap.parse_args()
    for s in a.sessions:
        try:
            session(a, s)
        except AssertionError as e:
            print('%s: MISMATCH: %s' % (os.path.basename(s), e))
            return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
