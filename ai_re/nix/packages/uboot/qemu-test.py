#!/usr/bin/env python3
"""Boot U-Boot on QEMU ppce500 with an e6500 core and exercise the shell."""
import os, select, subprocess, sys, time

qemu, uboot = sys.argv[1], sys.argv[2]
p = subprocess.Popen([qemu, "-M", "ppce500", "-cpu", "e6500", "-smp", "1", "-m", "1G",
                      "-nographic", "-monitor", "none", "-serial", "stdio", "-bios", uboot],
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
buf = b""
def until(token, timeout):
    global buf
    end = time.time() + timeout
    while time.time() < end:
        r, _, _ = select.select([p.stdout], [], [], 0.5)
        if r:
            chunk = os.read(p.stdout.fileno(), 4096)
            if not chunk:
                break
            buf += chunk
            sys.stdout.write(chunk.decode(errors="replace")); sys.stdout.flush()
            if token.encode() in buf:
                return True
    raise SystemExit(f"FAIL: no {token!r} within {timeout}s")
def cmd(c, expect, timeout=20):
    global buf
    buf = b""
    p.stdin.write(c.encode() + b"\n"); p.stdin.flush()
    until(expect, timeout)
    until("=> ", timeout)
try:
    until("U-Boot 2026.10", 120)
    until("Core:  e6500", 60)          # CPU identification read from PVR
    p.stdin.write(b"\n"); p.stdin.flush()   # stop autoboot if any
    until("=> ", 120)
    cmd("version", "U-Boot 2026.10")
    cmd("bdinfo", "DRAM bank")
    cmd("mw.l 0x1000000 0xa5a5f00d 0x400", "=>")
    cmd("cmp.l 0x1000000 0x1000004 0x3ff", "Total of 1023 word(s) were the same")
    cmd("crc32 0x1000000 0x1000", "crc32 for")
    print("\nPASS: U-Boot ran on an emulated e6500 core")
finally:
    p.kill()
