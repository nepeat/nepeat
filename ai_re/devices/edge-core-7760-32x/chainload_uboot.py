#!/usr/bin/env python3
"""Test a RAM-linked U-Boot (#uboot-as7760-chainload) on the T2080 without flashing.

Reversible: everything happens in RAM; power-cycle returns to the loader in
NOR. Requires an idle U-Boot prompt in tmux serial-7760 (powercycle_to_uboot.py):
upstream "=>" or factory "LOADER=>" both work as the parent.

  1. TFTP the image to 0x00200000 (its TEXT_BASE), verify size.
  2. go 0x00200000 (the image itself writes back L2/CPC and drops the parent's
     TLB entries: patch 0001), require the new banner + prompt, then run
     read-only checks.
Transcript: chainload-<date>.txt in this directory. Exit 0 only on PASS.
"""
import argparse, re, shutil, subprocess, sys, time
from pathlib import Path

base = Path(__file__).resolve().parent
ap = argparse.ArgumentParser()
ap.add_argument("image", type=Path)
ap.add_argument("--banner", default="U-Boot 2026.10-as7760-chainload")
ap.add_argument("--cmd", action="append", default=[],
                help="extra step 'command::expected substring' run in the new U-Boot")
ap.add_argument("--final", default=None,
                help="last command (no prompt expected), e.g. booting Linux")
ap.add_argument("--final-expect", default=None, help="substring proving --final worked")
ap.add_argument("--final-timeout", type=int, default=180)
ap.add_argument("--pre", action="append", default=[],
                help="factory-prompt command(s) to run before loading the image")
ap.add_argument("--autoboot", default=None,
                help="don't stop autoboot; PASS when this substring appears (e.g. login:)")
ap.add_argument("--after", action="append", default=[],
                help="steps 'command::expected' run at the => prompt reached after --final")
args = ap.parse_args()
capture = base / "ram-probe-boot-20261114.txt"
log = base / time.strftime("chainload-%Y%m%d-%H%M%S.txt")
start_off = capture.stat().st_size

def pane_last():
    return subprocess.check_output(["tmux", "capture-pane", "-t", "serial-7760", "-p"], text=True).rstrip()
def send(text):
    time.sleep(0.5)  # let the factory loader finish printing before typing
    subprocess.run([sys.executable, str(base / "serial_command.py"), text], check=True)
def wait_for(pattern, offset, timeout):
    end = time.time() + timeout
    while time.time() < end:
        out = capture.read_bytes()[offset:].decode(errors="replace")
        if re.search(pattern, out):
            return out
        time.sleep(0.1)
    return None
def run(text, prompt, timeout=60, bad=("Unknown command", "Bad Magic", "TFTP error")):
    off = capture.stat().st_size
    send(text)
    out = wait_for(re.escape(prompt) + r"\s*$", off, timeout)
    if out is None or any(b in out for b in bad):
        finish(False, f"{text!r} failed:\n{out}")
    return out
def finish(ok, why=""):
    log.write_bytes(capture.read_bytes()[start_off:])
    print(("PASS" if ok else "FAIL") + (": " + why if why else ""), f"(transcript {log.name})")
    sys.exit(0 if ok else 1)

if not pane_last().endswith("=>"):
    raise SystemExit("not at an idle U-Boot prompt; run powercycle_to_uboot.py")
dst = Path("/srv/tftpboot/as7760-probe/u-boot-chainload.bin")
dst.unlink(missing_ok=True); shutil.copyfile(args.image, dst); dst.chmod(0o644)
size = args.image.stat().st_size
F = "=>"  # also matches the factory "LOADER=>"
for c in args.pre:
    run(c, F, 120)
run("setenv autoload no", F)
for _ in range(3):  # the uplink port may still be coming up after power-on
    if "bound to address" in run("dhcp", F, 120, bad=()):
        break
else:
    finish(False, "DHCP failed")
run("setenv serverip 10.60.2.208", F)
out = run("tftpboot 0x00200000 as7760-probe/u-boot-chainload.bin", F, 120)
if f"Bytes transferred = {size} " not in out:
    finish(False, "size mismatch")
off = capture.stat().st_size
send("go 0x00200000")
if wait_for(re.escape(args.banner), off, 30) is None:
    finish(False, "no new banner within 30 s (hung in early start.S?)")
if args.autoboot:
    ok = wait_for(re.escape(args.autoboot), off, 300) is not None
    finish(ok, ("autoboot reached " + repr(args.autoboot)) if ok else "autoboot did not reach " + repr(args.autoboot))
if wait_for(r"Hit any key to stop autoboot", off, 60) is not None:
    subprocess.run(["tmux", "send-keys", "-t", "serial-7760", "Enter"], check=True)
if wait_for(r"\n=> $", off, 90) is None:
    finish(False, "banner but no prompt within 90 s")
P = "=>"
for c in ("version", "bdinfo", "dm tree", "md.b ffdf0000 20", "mmc info", "i2c bus",
          "mw.l 0x10000000 0x5a5aa5a5 0x1000", "cmp.l 0x10000000 0x10000004 0xfff"):
    run(c, P, 60, bad=("Unknown command", "exception", "Machine Check", "### ERROR"))
extra = {}
for spec in args.cmd:
    c, _, want = spec.partition("::")
    o = run(c, P, 120, bad=("Unknown command", "exception", "Machine Check", "### ERROR"))
    extra[c] = (want in o) if want else True
if args.final:
    foff = capture.stat().st_size
    send(args.final)
    if args.final_expect:
        extra[args.final] = wait_for(re.escape(args.final_expect), foff, args.final_timeout) is not None
    if args.after and extra.get(args.final, True):
        if wait_for(r"\n=> $", foff, 120) is None:
            finish(False, "no prompt after --final")
        for spec in args.after:
            c, _, want = spec.partition("::")
            o = run(c, P, 600, bad=("Unknown command", "exception", "Machine Check", "### ERROR"))
            extra["after: " + c] = (want in o) if want else True
out = capture.read_bytes()[off:].decode(errors="replace")
checks = {"T2080 CPU": "T2080" in out, "e6500 core": "e6500" in out,
          "DRAM 16 GiB": bool(re.search(r"\b16 GiB \(total", out)),
          "memory compare": "Total of 4095 word(s) were the same" in out,
          **{f"step: {k}": v for k, v in extra.items()}}
bad = [k for k, v in checks.items() if not v]
finish(not bad, "missing: " + ", ".join(bad) if bad else "new U-Boot ran on the T2080 (" + ", ".join(checks) + ")")
