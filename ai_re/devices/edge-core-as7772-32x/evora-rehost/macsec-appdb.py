#!/usr/bin/env python3
"""Static MACsec SAs through SONiC's own path, without MKA: APPL_DB MACSEC_* entries for swssconfig.

  macsec-appdb.py DIR PORT_A PORT_B [--cipher GCM-AES-XPN-256]
Reads DIR/a2b.par and DIR/b2a.par (macsec-params.py) and writes DIR/appdb-set.json and DIR/appdb-del.json (0600):
what wpa_supplicant's macsec_sonic driver would write for the two ends of one link, in its order (port not
enabled, SCs, SAs, port enabled), so orchagent's MACsecOrch creates the SAI objects. On the switch:
  <swss>/bin/swssconfig DIR/appdb-set.json      ...      <swss>/bin/swssconfig DIR/appdb-del.json
The files hold the keys: dumps/ here, /var/lib/evora-phy on the switch. Stage S2 of ../macsec.md "Under SONiC".
"""
import json, os, sys


def par(path):
    return {k: bytes.fromhex(v) for k, v in (l.split() for l in open(path))}


def main():
    d, a, b = sys.argv[1:4]
    cipher = sys.argv[5] if len(sys.argv) > 5 else 'GCM-AES-XPN-256'
    a2b, b2a = par(os.path.join(d, 'a2b.par')), par(os.path.join(d, 'b2a.par'))
    port = lambda enable: {'enable': enable, 'cipher_suite': cipher, 'enable_protect': 'true', 'enable_encrypt': 'true',
                           'enable_replay_protect': 'false', 'replay_window': '0', 'send_sci': 'true'}

    def sa(p, pn_field):
        e = {'sak': p['key'].hex(), 'auth_key': p['hkey'].hex(), pn_field: str(int.from_bytes(p['pn'], 'little'))}
        if 'salt' in p:
            e.update(salt=p['salt'].hex(), ssci=str(int.from_bytes(p['ssci'], 'big')))
        return e
    setl, dell = [], []
    for me, tx, rx in ((a, a2b, b2a), (b, b2a, a2b)):
        tsci, rsci = tx['sci'].hex(), rx['sci'].hex()
        setl += [('MACSEC_PORT_TABLE:' + me, port('false')),
                 ('MACSEC_EGRESS_SC_TABLE:%s:%s' % (me, tsci), {'encoding_an': '0'}),
                 ('MACSEC_INGRESS_SC_TABLE:%s:%s' % (me, rsci), {'Null': 'Null'}),
                 ('MACSEC_INGRESS_SA_TABLE:%s:%s:0' % (me, rsci), dict(sa(rx, 'lowest_acceptable_pn'), active='true')),
                 ('MACSEC_EGRESS_SA_TABLE:%s:%s:0' % (me, tsci), sa(tx, 'next_pn'))]
        dell += ['MACSEC_EGRESS_SA_TABLE:%s:%s:0' % (me, tsci), 'MACSEC_INGRESS_SA_TABLE:%s:%s:0' % (me, rsci),
                 'MACSEC_EGRESS_SC_TABLE:%s:%s' % (me, tsci), 'MACSEC_INGRESS_SC_TABLE:%s:%s' % (me, rsci), 'MACSEC_PORT_TABLE:' + me]
    setl += [('MACSEC_PORT_TABLE:' + p, port('true')) for p in (a, b)]
    for name, body in (('appdb-set.json', [{k: v, 'OP': 'SET'} for k, v in setl]), ('appdb-del.json', [{k: {}, 'OP': 'DEL'} for k in dell])):
        fd = os.open(os.path.join(d, name), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, 'w') as f:
            json.dump(body, f, indent=1)


if __name__ == '__main__':
    main()
