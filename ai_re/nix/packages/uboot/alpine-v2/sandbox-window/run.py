#!/usr/bin/env python3
"""Sandbox write-window test: U-Boot sandbox with the series, the read-only
SPI NOR + write window (bus 0 CS 0, 0xf90000 + 0x20000) and an emulated
MX25U12835F backed by spi.bin. Erase/write/raw-op attempts inside and
outside the window; afterwards only bytes inside the window may differ.
usage: run.py <u-boot> <u-boot.dtb>
"""
import os, random, re, subprocess, sys

uboot, dtb = sys.argv[1:3]
WS, WZ, SIZE = 0xF90000, 0x20000, 0x1000000
EROFS = -30

random.seed(7)
orig = bytes(random.getrandbits(8) for _ in range(SIZE))
open("spi.bin", "wb").write(orig)

# (command, expect rc 0?) -- every out-of-window or forbidden case must fail
cases = [
    ("erase f90000 1000", True), ("write f90000 100 5a", True), ("erase fa0000 10000", True),
    ("write faff00 100 3c", True),
    ("erase f8f000 1000", False), ("erase fb0000 1000", False), ("erase f8f000 2000", False),
    ("erase faf000 2000", False), ("write faff00 200 11", False), ("write 0 100 0", False),
    ("erase 0 1000000", False), ("erase fd0000 10000", False), ("erase fe0000 10000", False),
    ("write fd0000 4 0", False), ("write f8ffff 2 0", False),
    ("op 01 0 0 0 out 1", False),        # WRSR
    ("op 31 0 0 0 out 1", False),        # WRSR2
    ("op c7 0 0 0 none 0", False),       # chip erase
    ("op 60 0 0 0 none 0", False),       # chip erase
    ("op 35 0 0 0 none 0", False),       # EQIO (Macronix)
    ("op b7 0 0 0 none 0", False),       # EN4B
    ("op 66 0 0 0 none 0", False),       # reset enable
    ("op 06 0 0 0 none 0", True),        # WREN (on the window chip)
    ("op 02 3 0 0 out 8", False),        # program at 0
    ("op 02 4 f90000 0 out 8", False),   # 4-byte address with a 3-byte opcode
    ("op 02 3 f90100 1 out 8", False),   # dummy byte
    ("op 20 3 f80000 0 none 0", False),  # erase below
    ("op d8 3 fb0000 0 none 0", False),  # erase above
    ("op 52 3 f88000 0 none 0", False),  # 32K block below
    ("op 06 0 0 0 none 0", True),
    ("op 02 3 f90200 0 out 8", True),    # raw program inside
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
    print(f"{'ok ' if good else 'BAD'} sfwtest {c:28s} rc {rc}")
    bad += not good
now = open("spi.bin", "rb").read()
diff = [i for i in range(SIZE) if now[i] != orig[i]]
outside = [i for i in diff if not WS <= i < WS + WZ]
print(f"spi.bin: {len(diff)} bytes changed, {len(outside)} outside the window")
if outside:
    print("first outside:", hex(outside[0]))
    bad += 1
# the allowed operations really happened
exp = {0xF90000: 0x5A, 0xF90FFF: 0xFF, 0xFA0000: 0xFF, 0xFAFF00: 0x3C, 0xF90200: 0xA5}
for a, v in exp.items():
    if now[a] != v:
        print(f"BAD byte {a:#x} = {now[a]:#x}, want {v:#x}")
        bad += 1
if "refused" not in out:
    print("BAD: no refusal message")
    bad += 1
if bad:
    print(out[-3000:])
    raise SystemExit(f"FAIL: {bad}")
print("sandbox-window: ok (%d attempts; erase/program only inside 0xf90000+0x20000; "
      "SR writes, chip erase, EQIO, EN4B, reset, width mismatch and dummy refused)" % len(cases))
