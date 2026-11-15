#!/usr/bin/env python3
"""Boot the RAM-root NixOS from the SD card's FAT via boot.scr (volatile).

Requires an idle U-Boot prompt ("=>" or factory "LOADER=>") in tmux serial-7760 (powercycle_to_uboot.py).
Sends: mmc rescan; fatload boot.scr to 0x07000000; source it. The script then
fatloads uImage/uInitrd/DTB, sets bootargs and runs bootm. Never saveenv.
"""
import subprocess, sys, time
from pathlib import Path

base = Path(__file__).resolve().parent
capture = base / "ram-probe-boot-20261114.txt"
last = subprocess.check_output(["tmux", "capture-pane", "-t", "serial-7760", "-p"], text=True).rstrip()
if not last.endswith("=>"):
    raise SystemExit("not at an idle U-Boot prompt; inspect serial-7760")

def command(text, wait=True):
    offset = capture.stat().st_size
    subprocess.run([sys.executable, str(base / "serial_command.py"), text], check=True)
    if not wait:
        return ""
    for _ in range(1200):
        out = capture.read_bytes()[offset:].decode(errors="replace")
        if out.rstrip().endswith("=>"):
            if any(e in out for e in ("Unknown command", "Bad Magic", "** ", "error")):
                raise SystemExit(out)
            return out
        time.sleep(0.1)
    raise SystemExit("no prompt after " + text)

command("mmc rescan")
print(command("fatload mmc 0:1 0x07000000 boot.scr").strip().splitlines()[-2], flush=True)
command("source 0x07000000", wait=False)
print("boot.scr sourced; watch serial-7760", flush=True)
