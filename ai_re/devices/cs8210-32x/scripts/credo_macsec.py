#!/usr/bin/env python3
"""credo_macsec.py: phy_shell command lists for static-key MACsec on the
CS8210's Credo PHY slices (vendor c-phy tools, firmware 1.92.2).

  credo_macsec.py loop --keys FILE [--a 1 --b 16] [--mac-a M --mac-b M]
                  [--cipher gcm-aes-128|gcm-aes-256] [--wrong-ingress a|b]
  credo_macsec.py off [--a 1 --b 16]

'loop' prints, for two slices cabled to each other, the commands the vendor
loader (c-phy/auto_load_user.sh -e GCM-AES-*) would emit, but with our own
keys and one SC per direction:
  slice A egress SA  = SCI(A), SAK_AB        slice B ingress SA = SCI(A), SAK_AB
  slice B egress SA  = SCI(B), SAK_BA        slice A ingress SA = SCI(B), SAK_BA
SCI = the port's MAC + port identifier 0001. 'auth' is the GCM hash subkey
H = AES_K(0^128) (checked against the vendor's built-in key/auth pair).
--wrong-ingress X replaces SAK on slice X's ingress SA by a different key
(negative test: that direction must drop). Keys are generated into FILE on
first use (JSON, keep it out of git) and reused afterwards.
Port mode stays MacsecBypass, as the vendor loader leaves it in MACsec mode
too; macsec_datapath_init + macsec_enable switch the engine in.
Needs openssl(1) for the hash subkey.
"""
import argparse
import binascii
import json
import os
import subprocess

FLAGS = 'SAB_MACSEC_FLAG_ROLLOVER'


def hexlist(b):
    return ','.join('0x%02x' % x for x in b)


def hash_subkey(sak):
    algo = {16: 'aes-128-ecb', 32: 'aes-256-ecb'}[len(sak)]
    out = subprocess.run(['openssl', 'enc', '-' + algo, '-nopad', '-K', sak.hex()],
                         input=bytes(16), stdout=subprocess.PIPE, check=True).stdout
    return out[:16]


def sci(mac, port_id=1):
    return binascii.unhexlify(mac.replace(':', '')) + port_id.to_bytes(2, 'big')


def keys(path, klen):
    if os.path.exists(path):
        k = json.load(open(path))
    else:
        k = {}
    for name in ('sak_ab', 'sak_ba', 'sak_wrong'):
        if len(bytes.fromhex(k.get(name, ''))) != klen:
            k[name] = os.urandom(klen).hex()
    for name in ('salt_ab', 'salt_ba'):
        k.setdefault(name, os.urandom(12).hex())
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, 'w') as f:
        json.dump(k, f, indent=1)
    return {n: bytes.fromhex(v) for n, v in k.items()}


def sa(slice_id, direct, sci_b, sak, salt, ssci=b'\x00\x00\x00\x01', an=0, rule=1):
    s = 'slice_id=%d direct=%s sa_id=1' % (slice_id, direct)
    return [
        'macsec_allocate_sa %s sci=%s an=%d' % (s, hexlist(sci_b), an),
        'macsec_sa_set %s k=flags v=%s' % (s, FLAGS),
        'macsec_sa_set %s k=sak v=%s' % (s, hexlist(sak)),
        'macsec_sa_set %s k=ssci v=%s' % (s, hexlist(ssci)),
        'macsec_sa_set %s k=salt v=%s' % (s, hexlist(salt)),
        'macsec_sa_set %s k=auth v=%s' % (s, hexlist(hash_subkey(sak))),
        'macsec_vport_add %s rule_id=%d port=0 da={0x0,0x0,0x0,0x0,0x0,0x0} '
        'num_tags=all pkt_type=CFYE_RULE_PKT_TYPE_OTHER' % (s, rule),
    ]


def main():
    p = argparse.ArgumentParser()
    p.add_argument('cmd', choices=['loop', 'off'])
    p.add_argument('--a', type=int, default=1)
    p.add_argument('--b', type=int, default=16)
    p.add_argument('--mac-a', default='0e:48:c6:7a:55:cf')   # fpp1
    p.add_argument('--mac-b', default='0e:48:c6:7a:55:de')   # fpp16
    p.add_argument('--keys')
    p.add_argument('--cipher', choices=['gcm-aes-128', 'gcm-aes-256'], default='gcm-aes-128')
    p.add_argument('--wrong-ingress', choices=['a', 'b'])
    p.add_argument('--only', choices=['a', 'b'], help='configure one slice only')
    a = p.parse_args()
    out = []
    if a.cmd == 'off':
        for s in (a.a, a.b):
            out += ['macsec_datapath_flush slice_id=%d' % s, 'macsec_disable slice_id=%d port=0' % s]
    else:
        if not a.keys:
            p.error('--keys FILE is required for loop')
        k = keys(a.keys, 16 if a.cipher == 'gcm-aes-128' else 32)
        sci_a, sci_b = sci(a.mac_a), sci(a.mac_b)
        in_a = k['sak_wrong'] if a.wrong_ingress == 'a' else k['sak_ba']
        in_b = k['sak_wrong'] if a.wrong_ingress == 'b' else k['sak_ab']
        plan = [('a', a.a, sci_b, in_a, k['salt_ba'], sci_a, k['sak_ab'], k['salt_ab']),
                ('b', a.b, sci_a, in_b, k['salt_ab'], sci_b, k['sak_ba'], k['salt_ba'])]
        for tag, s, isci, isak, isalt, esci, esak, esalt in plan:
            if a.only and a.only != tag:
                continue
            out += ['macsec_datapath_init slice_id=%d' % s, 'macsec_enable slice_id=%d port=0' % s]
            out += sa(s, 'ingress', isci, isak, isalt)
            out += sa(s, 'egress', esci, esak, esalt)
    print('\n'.join(out))


if __name__ == '__main__':
    main()
