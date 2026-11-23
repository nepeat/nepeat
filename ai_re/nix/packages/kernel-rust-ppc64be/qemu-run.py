#!/usr/bin/env python3
"""Run a QEMU command until the guest prints "TEST: done" (or a timeout), log
the console, then stop QEMU: ppce500 has no power-off device, so the guest
cannot end the run itself. usage: qemu-run.py <log> <timeout s> -- <qemu argv>"""
import os, select, subprocess, sys, time

log, timeout = sys.argv[1], float(sys.argv[2])
argv = sys.argv[sys.argv.index("--") + 1:]
p = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
buf, end = b"", time.time() + timeout
with open(log, "wb") as f:
    while time.time() < end:
        r, _, _ = select.select([p.stdout], [], [], 1)
        if r:
            chunk = os.read(p.stdout.fileno(), 65536)
            if not chunk:
                break
            f.write(chunk); f.flush(); buf += chunk
            if b"TEST: done" in buf:
                time.sleep(2)
                break
        elif p.poll() is not None:
            break
p.kill()
p.wait()
sys.exit(0 if b"TEST: done" in buf else 1)
