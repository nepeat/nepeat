#!/usr/bin/env python3
"""Send ICMP echo requests as raw Ethernet frames out of one netdev, bypassing
the kernel's routing (the destination may be an address the kernel owns).
On the AS7760 a KNET netdev transmits straight out of its port, so the frame
enters the ASIC at the cabled peer port and is routed there. Stdlib only.

  python3 l3_inject.py --dev Ethernet0 --src 10.0.0.1 --dst 192.0.2.1 --count 50
  (--dmac defaults to the netdev's own MAC = the switch's router MAC)
"""
import argparse, socket, struct, time

def csum(b):
    if len(b) % 2:
        b += b"\0"
    s = sum(struct.unpack("!%dH" % (len(b) // 2), b))
    s = (s >> 16) + (s & 0xffff)
    s += s >> 16
    return (~s) & 0xffff

def mac(s):
    return bytes(int(x, 16) for x in s.split(":"))

ap = argparse.ArgumentParser()
ap.add_argument("--dev", required=True)
ap.add_argument("--src", required=True)
ap.add_argument("--dst", required=True)
ap.add_argument("--dmac")
ap.add_argument("--count", type=int, default=10)
ap.add_argument("--ttl", type=int, default=64)
ap.add_argument("--interval", type=float, default=0.02)
a = ap.parse_args()

own = open("/sys/class/net/%s/address" % a.dev).read().strip()
dmac, smac = mac(a.dmac or own), mac(own)
s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW)
s.bind((a.dev, 0))
for seq in range(a.count):
    payload = struct.pack("!d", time.time()) + b"sai-openbcm l3 test"
    icmp = struct.pack("!BBHHH", 8, 0, 0, 0x5a1, seq) + payload
    icmp = icmp[:2] + struct.pack("!H", csum(icmp)) + icmp[4:]
    ip = struct.pack("!BBHHHBBH4s4s", 0x45, 0, 20 + len(icmp), seq & 0xffff, 0, a.ttl, 1, 0,
                     socket.inet_aton(a.src), socket.inet_aton(a.dst))
    ip = ip[:10] + struct.pack("!H", csum(ip)) + ip[12:]
    s.send(dmac + smac + b"\x08\x00" + ip + icmp)
    time.sleep(a.interval)
print("sent %d frames %s -> %s on %s (dmac %s)" % (a.count, a.src, a.dst, a.dev, a.dmac or own))
