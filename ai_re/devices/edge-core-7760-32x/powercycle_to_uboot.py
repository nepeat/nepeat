#!/usr/bin/env python3
"""Power-cycle the AS7760 lab outlet and stop U-Boot autoboot via tmux.

Handles both loaders: upstream U-Boot in NOR ("Hit any key", prompt "=>") and
the factory 2014 U-Boot ("Type 123<ENTER>", prompt "LOADER=>").

Run inside the ai_re devshell. Requires the existing serial-7760 session and
its active ram-probe-boot capture pipe. Does not save any U-Boot variables.
"""
import subprocess
import time
from pathlib import Path

capture = Path(__file__).resolve().parent / "ram-probe-boot-20261114.txt"
start = capture.stat().st_size
power = ["lab-power", "--entity", "switch.server_room_lab_plug"]
subprocess.run(power + ["off"], check=True)
time.sleep(2)
subprocess.run(power + ["on"], check=True)
for _ in range(250):
    with capture.open("rb") as stream:
        stream.seek(start)
        data = stream.read()
    factory = b"Type 123<ENTER> to STOP autoboot" in data
    if factory or b"Hit any key to stop autoboot" in data:
        keys = ["123", "Enter"] if factory else ["Enter"]
        subprocess.run(["tmux", "send-keys", "-t", "serial-7760", *keys], check=True)
        print("sent autoboot interruption through serial-7760", flush=True)
        break
    time.sleep(0.2)
else:
    raise SystemExit("autoboot prompt not observed; inspect serial-7760")
