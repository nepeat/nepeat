#!/usr/bin/env python3
"""Turn captured U-Boot `md.l` output back into a binary, offline.

  md2bin.py bootrom.bin capture.txt [capture2.txt ...] [--base 0xfbff0000] [--size 0x4000]

Reads lines like `fbff0000: e59ff018 e59ff018 ... ` (the ASCII column is
ignored), checks that the addresses are contiguous from --base, writes the
little-endian words, and prints the length, sha256 and the zlib CRC32 that
U-Boot's `crc32 <base> <size>` prints, so the two can be compared.
Never touches hardware. Refuses to overwrite an existing output file.
"""
import argparse, hashlib, pathlib, re, struct, sys, zlib

LINE = re.compile(r"^([0-9a-f]{8}):((?: [0-9a-f]{8}){1,4})", re.I)

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument("out", type=pathlib.Path)
ap.add_argument("captures", nargs="+", type=pathlib.Path)
ap.add_argument("--base", type=lambda s: int(s, 0), default=0xfbff0000)
ap.add_argument("--size", type=lambda s: int(s, 0), default=0x4000)
a = ap.parse_args()
if a.out.exists():
    sys.exit(f"refusing to replace {a.out}")

words = {}
for cap in a.captures:
    for line in cap.read_text(errors="replace").splitlines():
        m = LINE.match(line.strip())
        if not m:
            continue
        addr = int(m.group(1), 16)
        for i, w in enumerate(m.group(2).split()):
            at = addr + 4 * i
            v = int(w, 16)
            if words.get(at, v) != v:
                sys.exit(f"conflicting values at {at:#x}")
            words[at] = v

data = bytearray()
for at in range(a.base, a.base + a.size, 4):
    if at not in words:
        sys.exit(f"missing word at {at:#x} (have {len(data)} bytes)")
    data += struct.pack("<I", words[at])
a.out.write_bytes(data)
print(f"{a.out} {len(data):#x} bytes sha256 {hashlib.sha256(data).hexdigest()} "
      f"crc32 {zlib.crc32(data):08x}")
