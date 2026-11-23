#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0
"""The Rust al_eth's MMIO trace (debugfs al_eth_rs/trace, binary: ../rust/trace.rs)
as the C recorder's text (/proc/al_eth_trace, ./al_trace_rec.c), so ./parse.py
and ../difftest --hw take either.

  rs2txt.py trace.bin > trace.txt
"""
import struct
import sys

MARKS = ["probe", "open", "close", "remove", "rx_mode", "mtu", "set_mac", "stats", "bars"]

data = open(sys.argv[1], "rb").read()
magic, ver, n, dropped, lu, lm, le, _ = struct.unpack_from("<4sIIIIIII", data, 0)
if magic != b"ALRT" or ver != 1:
    sys.exit("rs2txt: not an al_eth_rs trace (magic %r, version %d)" % (magic, ver))
out = ["# al_eth_trace: %u records, %u dropped; bar sizes u 0x%016x m 0x%016x e 0x%016x" % (n, dropped, lu, lm, le)]
skipped = 0
for i in range(n):
    w0, pid, off, val = struct.unpack_from("<IIII", data, 32 + 16 * i)
    op, bar, ctx, cpu = w0 & 0xff, (w0 >> 8) & 0xff, (w0 >> 16) & 0xff, w0 >> 24
    c = " @" if ctx else " [%u]" % pid
    op = chr(op) if op else ""
    if op in ("R", "W"):
        out.append("%s %c %05x %08x%s" % (op, bar, off, val, c))
    elif op == "D":
        out.append("D %u%s" % (val, c))
    elif op in ("C", "c"):
        out.append("C%s %03x %08x%s" % ("R" if op == "C" else "W", off, val, c))
    elif op == "K":
        out.append("# mark %s cpu %u%s" % (MARKS[val] if val < len(MARKS) else "?", cpu, c))
    else:
        skipped += 1   # claimed while the file was read, not yet written
print("\n".join(out))
if skipped:
    print("rs2txt: %d records not yet written when read" % skipped, file=sys.stderr)
