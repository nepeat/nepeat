#!/usr/bin/env python3
"""Receive UDP MPEG-TS into a file with a big socket buffer, then report
continuity-counter gaps per PID. usage: udprx.py <port> <seconds> <out.ts>"""
import socket, sys, time
port, secs, out = int(sys.argv[1]), float(sys.argv[2]), sys.argv[3]
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 64 << 20)
s.bind(("127.0.0.1", port))
s.settimeout(5)
data, dgrams, t0 = bytearray(), 0, None
while True:
    try:
        d = s.recv(65536)
    except socket.timeout:
        if t0: break
        continue
    t0 = t0 or time.time()
    data += d; dgrams += 1
    if time.time() - t0 > secs:
        break
open(out, "wb").write(data)
cc, gaps, n = {}, {}, len(data) // 188
for i in range(n):
    p = data[i * 188:(i + 1) * 188]
    if p[0] != 0x47:
        gaps["sync"] = gaps.get("sync", 0) + 1; continue
    pid = ((p[1] & 0x1F) << 8) | p[2]
    if pid == 0x1FFF or not (p[3] & 0x10):      # null packets / no payload: no CC check
        continue
    c = p[3] & 0x0F
    if pid in cc and c != (cc[pid] + 1) & 0x0F:
        gaps[pid] = gaps.get(pid, 0) + 1
    cc[pid] = c
print(f"{dgrams} datagrams, {n} TS packets, CC gaps per PID: {gaps or 'none'}")
