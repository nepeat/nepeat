#!/usr/bin/env python3
"""Power-cycle the DMC-12 (lab plug 2) and stop factory U-Boot at its prompt.

All console I/O goes through the logged tmux pane `serial-dmc12`. The prompt
is watched in the pane, not the capture file: tio block-buffers its log, so
the countdown reaches the file only after it has expired.
Before cutting power to a running factory OS, send SysRq s+u (serial break)
so sda1 is synced and read-only: see PROGRESS.md.
"""
import pathlib, subprocess, sys, time

ROOT = pathlib.Path(__file__).resolve().parent
SESSION = "serial-dmc12"
POWER = [sys.executable, str(ROOT.parents[1] / "tools" / "lab_power.py"), "2"]
AUTOBOOT = b"Hit any key to stop autoboot"


def pane():
    return subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", SESSION],
                          check=True, capture_output=True).stdout


def wait_for(needle, baseline, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if pane().count(needle) > baseline:
            return
        time.sleep(0.05)
    raise SystemExit(f"timed out waiting for {needle!r} in pane {SESSION}")


subprocess.run(["tmux", "clear-history", "-t", SESSION], check=True)
baseline = pane().count(AUTOBOOT)
subprocess.run(POWER + ["off"], check=True)
time.sleep(5)
subprocess.run(POWER + ["on"], check=True)
# The board runs stage2/stage3 twice before U-Boot's countdown (see initial boot).
wait_for(AUTOBOOT, baseline, 120)
subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", " "], check=True)
time.sleep(2)
print(pane().decode(errors="replace").rstrip()[-400:])
