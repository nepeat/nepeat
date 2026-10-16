#!/usr/bin/env python3
"""Read a -dump_mb file. usage: mbdump.py <file> <picture> <mb_y> [x0 x1]"""
import struct, sys

FMT = "<4h8h4B4B6h6b"   # must match M2CMBInfo
SZ = struct.calcsize(FMT)

def pictures(path):
    with open(path, "rb") as f:
        while hdr := f.read(8):
            n, t = struct.unpack("<ii", hdr)
            infos = [struct.unpack(FMT, f.read(SZ)) for _ in range(n)]
            bits = struct.unpack(f"<{n}i", f.read(4 * n))
            yield t, infos, bits

path, pic, row = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
x0, x1 = (int(sys.argv[4]), int(sys.argv[5])) if len(sys.argv) > 5 else (0, 120)
for i, (t, infos, bits) in enumerate(pictures(path)):
    if i != pic:
        continue
    mb_w = 120
    for x in range(x0, x1):
        m = infos[row * mb_w + x]
        mv = m[0:4]; fmv = m[4:12]
        intra, cbp, skip, dct, field, fsel, d = m[12:19]
        print(f"x={x:3d} bits={bits[row*mb_w+x]:5d} intra={intra} skip={skip} cbp={cbp:2d} dct={dct} "
              f"dir={d} field={field} fsel={fsel} mv={mv} fmv={fmv} last={m[25:]}")
