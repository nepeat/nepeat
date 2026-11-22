#!/usr/bin/env python3
"""Offline checks of macsec.py against the vendor binary (mock backends only; needs dumps/, skips without them).

  python3 macsec_test.py        (or pytest macsec_test.py)
1. The library's SA builder, called through our marshalling, rebuilds the vendor's static mode-2 SA records.
2. The vendor's own 'extphy init <phy> 1 mode=2' runs to "Please send packets now" on the recording-driven mock.
3. Our 'on' (vPort + SA + rule through the dispatch table) with the vendor's parameters produces the same MACsec
   register writes and firmware messages as the vendor's mode 2 did in (2).
4. 'on' with our own key in an initialised library state, then stats and off.
"""
import argparse, os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
T = os.path.join(HERE, '..', 'dumps/x/evora-trace')
INIT_TRACE, STATE = os.path.join(T, 'all-init.trace.txt'), os.path.join(T, 'all-cfg.state')


def have():
    import rehost
    return all(os.path.exists(p) for p in (rehost.DEFAULT_BIN, INIT_TRACE, STATE))


def emu(backend, state=None):
    import rehost, macsec
    a = argparse.Namespace(bin=rehost.DEFAULT_BIN, backend='mock', trace=None, itrace=None, verbose=False, strace=False, log=None)
    e = rehost.Emu(a)
    e.be = backend
    e.say = lambda s: e.__dict__.setdefault('console', []).append(s)
    e.boot()
    if state:
        e.load_state(state)
    return e, macsec.Macsec(e)


def macsec_events(be, phy):
    """MACsec-block LMI writes and SA/rule firmware messages (0x1201) of one PHY, order-free."""
    out = []
    for x in be.events:
        if isinstance(x, str):
            f = x.split()
            if int(f[1], 16) == phy and f[2] == '1201':
                b = bytearray(bytes.fromhex(f[3]))
                if b[0] == 0x67:
                    b[13:17] = b'\0' * 4          # SA record word 1 = context id (the vendor's static record has 0x6227)
                out.append('msg ' + b.hex())
        elif x.write and x.phy == phy and x.block in (2, 3, 0xa, 0xb):
            out.append(str(x).replace(' side%d ' % x.side, ' '))    # 1.8b90 (line/system) does not apply to these blocks
    return out


def test_all():
    if not have():
        print('skipped: vendor binary / traces not present')
        return
    import macsec
    # 1
    e, m = emu(macsec.ChipMock())
    assert m.selftest()
    sci = bytes(e.uc.mem_read(macsec.VENDOR_SCI, 8))
    w = struct.unpack('>24I', bytes(e.uc.mem_read(macsec.VENDOR_TR[0], 96)))
    key = struct.pack('<4I', *w[2:6])
    # 2
    be = macsec.OracleMock(INIT_TRACE, 0x03)
    e, m = emu(be)
    rc = e.extphy('init 0x00 1 mode=2')
    con = ''.join(e.console)
    assert rc == 0 and 'Please send packets now' in con, con[-400:]
    m.adopt(0)
    vendor_all = macsec_events(be, 0)
    # 3: same calls through our API in a fresh library state (bypass init from the real run), same parameters
    be2 = macsec.ChipMock()
    e2, m2 = emu(be2, STATE)
    m2.on(0, 0, key, sci, pn=4, flags=8)
    m2.on(0, 1, key, sci, pn=5, window=0x80, flags=8)
    ours = macsec_events(be2, 0)
    # the vendor ends with reg_value_set(0x4400005c / 0x4600005c, 0) on both if_sides; before that come its
    # vPort / SA / rule calls, which must be exactly ours (the order differs: the vendor interleaves the two sides)
    v = list(vendor_all)
    while v and v[-1].endswith(' 0005c 00000000'):
        v.pop()
    tail = v[-len(ours):]
    diff = sorted(set(tail) ^ set(ours))
    print('vendor mode 2: %d MACsec writes/messages, the last %d before its final 0x5c writes vs our %d: %s' % (
        len(vendor_all), len(tail), len(ours), 'same multiset' if sorted(tail) == sorted(ours) else 'DIFFERENT'))
    for x in diff:
        print('   differs:', x[:120])
    assert sorted(tail) == sorted(ours)
    # 4
    be3 = macsec.ChipMock()
    e3, m3 = emu(be3, STATE)
    k = bytes(range(32))
    for side in (0, 1):
        m3.on(0x01, side, k, bytes.fromhex('02000000000100 01'.replace(' ', '')), an=1, window=64 if side else 0)
        m3.sa_add(0x01, side, bytes(range(16)), bytes.fromhex('0200000000010001'), an=2,
                  salt=bytes(12), ssci=b'\0\0\0\1')
    st = m3.stats(0x01, 0)
    assert 'error' not in st['sa an=1'] and st['sa an=1']['out_pkts_encrypted_protected'] == 0, st
    assert not any('_unnamed_bytes' in d or 'error' in d for d in st.values()), st
    sti = m3.stats(0x01, 1)
    assert not any('_unnamed_bytes' in d or 'error' in d for d in sti.values()), sti
    for side in (0, 1):
        m3.off(0x01, side)
    assert m3.reg['0x1']['0'] == {'sa': {}} and m3.reg['0x1']['1'] == {'sa': {}}, m3.reg
    print('ok')


if __name__ == '__main__':
    test_all()
