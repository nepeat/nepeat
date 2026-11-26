#!/usr/bin/env python3
"""Power-cycle the DMC-12 and send a key to stage3, then stop at the factory prompt.

Stage3 polls UART0 once, between its BOOT_MODE/DT/APCEA lookups and the app
lookup (uart-boot.md). The key is repeated every 20 ms from stage2's "Executing next" line
until "Loading application"; then the factory autoboot is stopped. Read-only:
`p` skips APCEA, `e` keeps early APCEA, `u` forces U-Boot (silent while the
app is already U-Boot). Prints the stage3 messages and the p/e/state words.

  stage3keys.py --plug 2 --key e [--off 8]
"""
import argparse, pathlib, re, subprocess, sys, time

ROOT = pathlib.Path(__file__).resolve().parent
S = "serial-dmc12"


def pane():
    return subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", S],
                          capture_output=True, text=True, errors="replace").stdout


def key(k):
    subprocess.run(["tmux", "send-keys", "-t", S, "-l", k], check=True)


def count(pat, text):
    return len(re.findall(pat, text))


ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument("--plug", type=int, required=True)
ap.add_argument("--key", required=True)
ap.add_argument("--off", type=int, default=8, help="seconds powered off")
a = ap.parse_args()
power = ["lab-power", str(a.plug)]
subprocess.run(power + ["off"], check=True, capture_output=True)
time.sleep(a.off)
subprocess.run(["tmux", "clear-history", "-t", S], check=True)
base = pane()  # the visible screen survives clear-history: count only new matches
b = {p: count(p, base) for p in ("Executing next", "Loading application", "Hit any key", "User request")}
subprocess.run(power + ["on"], check=True, capture_output=True)
t0, sent = time.monotonic(), 0
while time.monotonic() - t0 < 120:
    t = pane()
    new = {p: count(p, t) - b[p] for p in b}
    # From stage2's last line: stage2 is done, keys queue in the UART FIFO for
    # stage3's single poll (starting at the stage3 banner missed it 3 times in 4).
    if new["Executing next"] > 0 and new["Loading application"] == 0:
        key(a.key)
        sent += 1
    if new["Hit any key"] > 0:
        key(" ")
        break
    time.sleep(0.02)
deadline = time.monotonic() + 20
while not pane().rstrip().endswith("ALPINE_DB>") and time.monotonic() < deadline:
    time.sleep(0.2)
t = pane()
print(f"key {a.key!r} sent {sent}x")
for line in t.splitlines()[-200:]:
    if "User request" in line:
        print("stage3:", line.strip())
for c in ("md.l 0x0105a3e8 1", "md.l 0x0105a48c 1", "md.b 0xfbff41c0 4", "md.l 0x0105d268 1"):
    out = subprocess.run([sys.executable, str(ROOT / "console.py"), c], capture_output=True, text=True).stdout
    print(f"{c:22s} -> {out.strip().splitlines()[-1] if out.strip() else '?'}")
