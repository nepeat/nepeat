#!/usr/bin/env python3
"""Send one paced command through the existing, watchable tmux serial pane.

Wait for the device prompt before invoking. Factory U-Boot can drop bursts
while echoing/editing input; pacing avoids overflowing its small UART FIFO.
"""
import argparse
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("command")
parser.add_argument("--clear", action="store_true")
args = parser.parse_args()
if "\n" in args.command or "\r" in args.command:
    parser.error("send one command at a time")
base = ["tmux", "send-keys", "-t", "serial-7760"]
if args.clear:
    subprocess.run(base + ["C-u"], check=True)
    time.sleep(0.2)
for byte in args.command.encode("utf-8"):
    # Hex input also preserves semicolons: tmux treats a literal ; argument
    # as its own command separator even with send-keys -l.
    subprocess.run(base + ["-H", f"{byte:02x}"], check=True)
    time.sleep(0.02)
subprocess.run(base + ["Enter"], check=True)
