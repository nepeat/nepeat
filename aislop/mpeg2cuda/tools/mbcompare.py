#!/usr/bin/env python3
"""Compare encoder MB decisions (-dump_mb) with the decoder's -debug mb_type map.
usage: mbcompare.py <dump> <mbtype.txt> [mb_w]"""
import re, struct, sys
sys.path.insert(0, __import__("os").path.dirname(__file__))
FMT = "<4h8h4B4B6h6b"; SZ = struct.calcsize(FMT)

def pictures(path):
    with open(path, "rb") as f:
        while hdr := f.read(8):
            n, t = struct.unpack("<ii", hdr)
            infos = [struct.unpack(FMT, f.read(SZ)) for _ in range(n)]
            f.read(4 * n)
            yield t, infos

def decoder_maps(path, mb_w):
    frames, cur = [], None
    for line in open(path):
        if "New frame" in line:
            cur = []
            frames.append(cur)
            continue
        m = re.match(r"\[mpeg2video @ \w+\]\s+(\d+) (.*)$", line.rstrip("\n"))
        if cur is not None and m and m.group(2)[:1] != " " and len(m.group(2)) >= 3 * mb_w - 2:
            row = m.group(2)
            cur.append([row[3 * i:3 * i + 3] for i in range(mb_w)])
    return frames

mb_w = int(sys.argv[3]) if len(sys.argv) > 3 else 120
maps = decoder_maps(sys.argv[2], mb_w)
for pi, ((t, infos), dmap) in enumerate(zip(pictures(sys.argv[1]), maps)):
    if t == 1:
        continue
    for y, row in enumerate(dmap):
        for x, tok in enumerate(row):
            m = infos[y * mb_w + x]
            intra, skip, field = m[12], m[14], m[16]
            exp = "i" if intra else ("S" if skip else ">")
            got = tok[0]
            exp_field = bool(field)
            got_field = tok[1] == "-"
            if got != exp or (not intra and not skip and exp_field != got_field):
                print(f"pic {pi} first mismatch at mb ({x},{y}): enc {exp}{'F' if exp_field else ''} dec '{tok}'")
                for xx in range(max(0, x - 4), min(mb_w, x + 2)):
                    print("   ", xx, infos[y * mb_w + xx][:14], repr(row[xx]))
                break
        else:
            continue
        break
