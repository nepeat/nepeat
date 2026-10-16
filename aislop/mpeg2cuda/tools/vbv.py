#!/usr/bin/env python3
"""VBV check for an MPEG-2 elementary stream (vbv_delay = 0xFFFF semantics:
the buffer fills at bit_rate until full; each picture is removed at once at
its decode time; underflow = picture larger than the buffer content).

usage: vbv.py <file.m2v> [fps]   (bit_rate, vbv_buffer_size and, unless given,
the frame rate come from the sequence header)
"""
import sys

data = open(sys.argv[1], "rb").read()
FPS = [0, 24000 / 1001, 24, 25, 30000 / 1001, 30, 50, 60000 / 1001, 60]
fps = float(sys.argv[2]) if len(sys.argv) > 2 else None

starts, br, vbv = [], None, None
i = 0
pic_starts = []
seq_pending = None
while True:
    i = data.find(b"\x00\x00\x01", i)
    if i < 0:
        break
    sc = data[i + 3]
    if sc == 0xB3 and br is None:
        b = int.from_bytes(data[i + 4:i + 12], "big")
        br = ((b >> 14) & 0x3FFFF) * 400        # h12 v12 aspect4 rate4 | bit_rate18 marker vbv10
        vbv = ((b >> 3) & 0x3FF) * 16384
        if fps is None:
            fps = FPS[(b >> 32) & 0xF]
    if sc in (0xB3, 0xB8) and seq_pending is None:
        seq_pending = i
    if sc == 0x00:
        pic_starts.append(seq_pending if seq_pending is not None else i)
        seq_pending = None
    i += 4
pic_starts.append(len(data))
sizes = [(pic_starts[k + 1] - pic_starts[k]) * 8 for k in range(len(pic_starts) - 1)]

if len(sys.argv) > 3:
    br = int(sys.argv[3])
full, worst, under = vbv, vbv, 0
for n, s in enumerate(sizes):
    if s > full:
        under += 1
        if under <= 5:
            print(f"UNDERFLOW at picture {n}: size {s} > fullness {full:.0f}")
    full = max(full - s, 0)
    worst = min(worst, full)
    full = min(vbv, full + br / fps)
print(f"{len(sizes)} pictures, bit_rate {br}, vbv {vbv} bits, max picture {max(sizes)} bits, "
      f"min fullness after removal {worst:.0f}, underflows {under}")
