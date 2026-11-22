#!/usr/bin/env python3
"""Parameter files for "openbcm-diag extmacsec run <port> <sequence> <file>" (static MACsec SAs by hand, for bring-up).

  macsec-params.py DIR [--cipher gcm-aes-xpn-256]
Writes DIR/a2b.par, DIR/b2a.par (one SA each: the transmitter's egress SA and the receiver's ingress SA use the
same file) and DIR/wrong.par (a2b with another key), mode 0600, with fresh random test keys. The files hold keys:
DIR belongs under dumps/ (git-ignored) and on the switch under /var/lib/evora-phy (0700). Nothing is printed.
"""
import os, secrets, sys


def aes_ecb(key, block):
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    e = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    return e.update(block) + e.finalize()


def params(cipher, sci, key=None):
    key = key or secrets.token_bytes(32 if cipher.endswith('256') else 16)
    p = {'key': key, 'hkey': aes_ecb(key, b'\0' * 16), 'sci': sci, 'pn': (1).to_bytes(8, 'little'), 'window': bytes(4)}
    if 'xpn' in cipher:
        salt, ssci = secrets.token_bytes(12), secrets.token_bytes(4)
        p.update(salt=salt, ssci=ssci, iv=bytes(a ^ b for a, b in zip(salt, ssci + bytes(8))))
    return p


def main():
    d = sys.argv[1]
    cipher = sys.argv[3] if len(sys.argv) > 3 and sys.argv[2] == '--cipher' else 'gcm-aes-xpn-256'
    os.makedirs(d, mode=0o700, exist_ok=True)
    a = params(cipher, bytes.fromhex('1cea0b382bd50001'))
    b = params(cipher, bytes.fromhex('1cea0b382bd50002'))
    w = dict(a, **{k: v for k, v in params(cipher, a['sci']).items() if k in ('key', 'hkey')})
    for name, p in (('a2b', a), ('b2a', b), ('wrong', w)):
        fd = os.open(os.path.join(d, name + '.par'), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, 'w') as f:
            for k, v in p.items():
                f.write('%s %s\n' % (k, v.hex()))


if __name__ == '__main__':
    main()
