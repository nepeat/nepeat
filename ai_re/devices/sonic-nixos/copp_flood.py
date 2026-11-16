#!/usr/bin/env python3
"""Flood the switch's CPU through the chip for CoPP tests: raw Ethernet
frames out of one KNET netdev (it transmits straight out of its port), so
they enter the ASIC at the cabled peer port, are trapped there and reach the
CPU through the peer port's netdev at whatever rate the CoPP policer lets
through. Counts that netdev's rx_packets before/after. Stdlib only, root.

  python3 copp_flood.py --dev Ethernet0 --rx Ethernet124 --kind arp  --src 10.0.0.0 --dst 10.0.0.1 --seconds 10
  python3 copp_flood.py --dev Ethernet0 --rx Ethernet124 --kind icmp --src 10.0.0.0 --dst 10.0.0.1 --seconds 10
  (icmp: dmac = the router MAC, dst = an address the peer port's VRF owns: IP2ME)
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

def rx(dev):
    return int(open("/sys/class/net/%s/statistics/rx_packets" % dev).read())

ap = argparse.ArgumentParser()
ap.add_argument("--dev", required=True)
ap.add_argument("--rx", required=True)
ap.add_argument("--kind", choices=["arp", "icmp"], required=True)
ap.add_argument("--src", required=True)
ap.add_argument("--dst", required=True)
ap.add_argument("--seconds", type=float, default=10)
a = ap.parse_args()

own = mac(open("/sys/class/net/%s/address" % a.dev).read().strip())
src, dst = socket.inet_aton(a.src), socket.inet_aton(a.dst)
if a.kind == "arp":   # who-has dst tell src, broadcast
    frame = b"\xff" * 6 + own + b"\x08\x06" + struct.pack("!HHBBH", 1, 0x0800, 6, 4, 1) + own + src + b"\0" * 6 + dst
else:
    icmp = struct.pack("!BBHHH", 8, 0, 0, 0xc0b, 1) + b"copp flood test"
    icmp = icmp[:2] + struct.pack("!H", csum(icmp)) + icmp[4:]
    ip = struct.pack("!BBHHHBBH4s4s", 0x45, 0, 20 + len(icmp), 0, 0, 64, 1, 0, src, dst)
    ip = ip[:10] + struct.pack("!H", csum(ip)) + ip[12:]
    frame = own + own + b"\x08\x00" + ip + icmp
frame += b"\0" * max(0, 60 - len(frame))

s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW)
s.bind((a.dev, 0))
r0, sent, t0 = rx(a.rx), 0, time.time()
while time.time() - t0 < a.seconds:
    for _ in range(100):
        s.send(frame)
    sent += 100
dt = time.time() - t0
time.sleep(1)
got = rx(a.rx) - r0
print("sent %d %s frames in %.1f s (%.0f/s); %s rx +%d (%.0f/s)" % (sent, a.kind, dt, sent / dt, a.rx, got, got / dt))
