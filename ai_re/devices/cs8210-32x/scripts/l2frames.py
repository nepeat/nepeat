#!/usr/bin/env python3
"""l2frames.py: raw Ethernet frames on a Linux netdev, standard library only.

  l2frames.py send IFACE DMAC [-n COUNT] [-s SIZE] [--ethertype 0x88b5]
  l2frames.py sniff IFACE [-t SECONDS] [-x N]

send: COUNT frames of SIZE bytes (without FCS) to DMAC, source = IFACE's
MAC, payload = sequence number + incrementing bytes. Used to seed the
hardware loop of loop400_fwd.py and for one-off MACsec checks.
sniff: counts received frames per ethertype for SECONDS and hex-dumps the
first N; for 0x88e5 (MACsec) it decodes the SecTAG (TCI/AN, SL, PN, SCI),
so ciphertext reaching the CPU can be told from plaintext without tcpdump.
Run as root.
"""
import argparse
import binascii
import collections
import socket
import struct
import time

ETH_P_ALL = 0x0003


def mac_bytes(s):
    return binascii.unhexlify(s.replace(':', ''))


def cmd_send(a):
    s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW)
    s.bind((a.iface, 0))
    src = s.getsockname()[4][:6]
    hdr = mac_bytes(a.dmac) + src + struct.pack('!H', a.ethertype)
    body_len = max(a.size - 14, 46)
    pad = bytes((i + 65) & 0xff for i in range(body_len))
    t0 = time.time()
    for i in range(a.n):
        s.send(hdr + struct.pack('!I', i) + pad[4:])
    dt = time.time() - t0
    print('sent %d x %d B on %s to %s in %.3f s' % (a.n, 14 + body_len, a.iface, a.dmac, dt))


def sectag(b):
    # b = frame from the ethertype on: 88e5, TCI/AN, SL, PN, [SCI]
    tci = b[2]
    sl = b[3] & 0x3f
    pn = struct.unpack('!I', b[4:8])[0]
    sc = bool(tci & 0x20)
    out = 'TCI=0x%02x (V=%d ES=%d SC=%d SCB=%d E=%d C=%d) AN=%d SL=%d PN=%d' % (
        tci & 0xfc, tci >> 7, (tci >> 6) & 1, (tci >> 5) & 1, (tci >> 4) & 1,
        (tci >> 3) & 1, (tci >> 2) & 1, tci & 3, sl, pn)
    if sc:
        out += ' SCI=%s' % binascii.hexlify(b[8:16]).decode()
    return out


def cmd_sniff(a):
    s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(ETH_P_ALL))
    s.bind((a.iface, 0))
    s.settimeout(0.2)
    counts = collections.Counter()
    sizes = collections.defaultdict(collections.Counter)
    shown = 0
    end = time.time() + a.t
    while time.time() < end:
        try:
            f, addr = s.recvfrom(65535)
        except socket.timeout:
            continue
        if addr[2] == socket.PACKET_OUTGOING:
            continue
        et = struct.unpack('!H', f[12:14])[0]
        counts[et] += 1
        sizes[et][len(f)] += 1
        if shown < a.x:
            shown += 1
            print('frame %d len %d dst %s src %s type 0x%04x' % (
                shown, len(f), binascii.hexlify(f[0:6]).decode(),
                binascii.hexlify(f[6:12]).decode(), et))
            if et == 0x88e5:
                print('  SecTAG ' + sectag(f[12:]))
            print('  ' + binascii.hexlify(f[:96]).decode())
    for et, n in sorted(counts.items()):
        print('ethertype 0x%04x: %d frames, sizes %s' % (et, n, dict(sizes[et].most_common(5))))
    if not counts:
        print('no frames received on %s in %d s' % (a.iface, a.t))


def main():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest='cmd', required=True)
    ps = sub.add_parser('send')
    ps.add_argument('iface')
    ps.add_argument('dmac')
    ps.add_argument('-n', type=int, default=1)
    ps.add_argument('-s', '--size', type=int, default=1500)
    ps.add_argument('--ethertype', type=lambda x: int(x, 0), default=0x88b5)
    pn = sub.add_parser('sniff')
    pn.add_argument('iface')
    pn.add_argument('-t', type=int, default=5)
    pn.add_argument('-x', type=int, default=3)
    a = p.parse_args()
    {'send': cmd_send, 'sniff': cmd_sniff}[a.cmd](a)


if __name__ == '__main__':
    main()
