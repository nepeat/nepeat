#!/usr/bin/env python3
"""Sandbox write-window test for the CS8210 installer windows (patch 0027 + the installer's
config): U-Boot sandbox with the emulated MX25U25635F (4-byte opcodes, NOR programming, test-only
sandbox-sf-4b.patch) backed by spi.bin (random data). Erase/write/raw-op attempts inside and
outside the five windows; afterwards only bytes inside the windows may differ.
usage: run.py <u-boot> <u-boot.dtb>
"""
import random, re, subprocess, sys

uboot, dtb = sys.argv[1:3]
SIZE = 0x2000000
WINDOWS = [(0x20000, 0x1000), (0x30000, 0x1000), (0x38000, 0x8000), (0xF0000, 0xA0000), (0x280000, 0x100000)]
random.seed(8210)
orig = bytes(random.getrandbits(8) for _ in range(SIZE))
open("spi.bin", "wb").write(orig)
cases = [  # (sfwtest args, expect rc 0)
    ("erase 20000 1000", True), ("write 20100 100 5a", True), ("erase 30000 1000", True),
    ("erase 38000 8000", True), ("erase f0000 a0000", True), ("write 18ff00 100 3c", True),
    ("erase 280000 100000", True), ("write 37ff00 100 11", True),
    ("erase 21000 1000", False), ("erase 1f000 2000", False), ("erase 2f000 2000", False),
    ("erase 31000 7000", False), ("erase 40000 1000", False), ("erase 0 2000000", False),
    ("erase 190000 1000", False), ("erase 27f000 2000", False), ("erase 380000 1000", False),
    ("write 18ff00 200 0", False), ("write 37ff80 100 0", False), ("write 0 100 0", False),
    ("write 1000000 100 0", False),
    ("op 06 0 0 0 none 0", True),          # WREN
    ("op 12 4 20200 0 out 8", True),       # 4-byte page program inside
    ("op 21 4 31000 0 none 0", False),     # 4 KiB erase just past BOOT_MODE
    ("op 21 4 20000 0 none 0", True),      # 4 KiB erase of TOC2
    ("op 02 3 20300 0 out 8", False),      # 3-byte program (meaning depends on the chip's mode)
    ("op 20 3 20000 0 none 0", False),     # 3-byte erase
    ("op dc 4 280000 0 none 0", True),     # 64 KiB erase inside slot B
    ("op dc 4 270000 0 none 0", False),    # 64 KiB block over the factory U-Boot
    ("op 01 0 0 0 out 1", False), ("op c7 0 0 0 none 0", False), ("op 60 0 0 0 none 0", False),
    ("op b7 0 0 0 none 0", False), ("op e9 0 0 0 none 0", False), ("op 66 0 0 0 none 0", False),
    ("op 12 4 20400 1 out 8", False),      # dummy byte
]
script = "; ".join(f"sfwtest {c}" for c, _ in cases)
r = subprocess.run([uboot, "-d", dtb, "-c", script], capture_output=True, text=True, timeout=300)
out = r.stdout + r.stderr
rcs = [int(x) for x in re.findall(r"sfwtest: rc (-?\d+)", out)]
if len(rcs) != len(cases):
    print(out[-4000:])
    raise SystemExit(f"FAIL: {len(rcs)} results for {len(cases)} cases")
bad = 0
for (c, ok), rc in zip(cases, rcs):
    good = (rc == 0) if ok else (rc != 0)
    print(f"{'ok ' if good else 'BAD'} sfwtest {c:26s} rc {rc}")
    bad += not good
now = open("spi.bin", "rb").read()
inside = lambda i: any(o <= i < o + n for o, n in WINDOWS)
outside = [i for i in range(SIZE) if now[i] != orig[i] and not inside(i)]
print(f"spi.bin: {sum(1 for i in range(0, SIZE) if now[i] != orig[i])} bytes changed, {len(outside)} outside the windows")
bad += bool(outside)
# (the raw 0x21 erase above comes without a WREN, so the chip ignores it: 0x20100 keeps 0x5a)
for a, v in {0x20100: 0x5A, 0x18FF00: 0x3C, 0x37FF00: 0x11, 0xF0000: 0xFF, 0x280000: 0xFF}.items():
    if now[a] != v:
        print(f"BAD byte {a:#x} = {now[a]:#x}, want {v:#x}")
        bad += 1
if bad:
    print(out[-3000:])
    raise SystemExit(f"FAIL: {bad}")
print(f"sandbox-window: ok ({len(cases)} attempts; erase/program only inside the five installer windows; "
      "3-byte program/erase, SR writes, chip erase, EN4B, reset and dummy refused)")
