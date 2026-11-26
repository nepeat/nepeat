#!/usr/bin/env python3
"""Soak test an installed DMC-12: repeated unattended boots, each smoke-tested.

Nothing is typed at any loader: every cycle must boot on its own (factory
autoboot -> preamble -> our U-Boot from raw sda1 -> extlinux -> NixOS).
Cycle kinds: power cycle (short off), cold (AC off --cold-off s, every 5th),
warm (`systemctl reboot` from NixOS = CPLD reset, every 3rd). One JSON line per
cycle in inventory/<unit>/soak.jsonl; exit 1 if any cycle failed.

  soak.py --unit u01 --plug 2 --ip 10.60.2.10 --cycles 20
"""
import argparse, json, pathlib, re, subprocess, sys, time

ROOT = pathlib.Path(__file__).resolve().parent
SESSION = "serial-dmc12"
SSH = ["sudo", "-n", "ssh", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no",
       "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR", "-o", "ConnectTimeout=5"]
# (event, pattern) in boot order; the first match after the cycle start is timed.
EVENTS = [("stage2", r"stage2_loader_k2"), ("factory_uboot", r"U-Boot 2015\.07"),
          ("bootp", r"DHCP client bound"), ("our_uboot", r"U-Boot 2026\.10-dmc12-(?:chainload|selfinit)"),
          ("kernel", r"Booting Linux on physical CPU"), ("login", r"root@dmc12[^\n]*#")]
BAD = [r"Kernel panic", r"Synchronous Abort", r"### ERROR", r"Reboot failed", r"Call trace",
       r"Unknown command", r"Retrieving file: .* failed", r"Wrong Image"]


def pane():
    return subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", SESSION],
                          capture_output=True, text=True, errors="replace").stdout


def power(plug, state):
    subprocess.run(["lab-power", str(plug), state],
                   check=True, capture_output=True)


def ssh(ip, cmd, timeout=30):
    return subprocess.run(SSH + [f"root@{ip}", cmd], capture_output=True, text=True, timeout=timeout)


def one(args, i, kind):
    rec = {"cycle": i, "kind": kind, "t": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    subprocess.run(["tmux", "clear-history", "-t", SESSION], check=True)
    # clear-history keeps the visible screen (the old login prompt is on it):
    # only count matches beyond what is there before the cycle starts.
    base = pane()
    seen = {n: len(re.findall(p, base)) for n, p in EVENTS}
    seen_bad = {b: len(re.findall(b, base)) for b in BAD}
    if kind == "warm":
        ssh(args.ip, "systemctl reboot")
    else:
        power(args.plug, "off")
        time.sleep(args.cold_off if kind == "cold" else 8)
        power(args.plug, "on")
    t0 = time.monotonic()
    times = {}
    while time.monotonic() - t0 < args.timeout:
        text = pane()
        for name, pat in EVENTS:
            if name not in times and len(re.findall(pat, text)) > seen[name]:
                times[name] = round(time.monotonic() - t0, 1)
        bad = [b for b in BAD if len(re.findall(b, text)) > seen_bad[b]]
        if bad or "login" in times:
            break
        time.sleep(0.5)
    rec["times"] = times
    rec["bad"] = bad
    if "login" not in times or bad:
        rec["pass"] = False
        log = ROOT / f"serial-soak-{args.unit}-{i:03d}.txt"
        log.write_text(pane())
        rec["console"] = log.name
        return rec
    # network + 48 ports, then the read-only smoke checks
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        try:
            if ssh(args.ip, "ls /dev/P* | wc -l").stdout.strip() == "48":
                break
        except subprocess.TimeoutExpired:
            pass
        time.sleep(3)
    rec["ready_s"] = round(time.monotonic() - t0, 1)
    r = subprocess.run([sys.executable, str(ROOT / "smoke.py"), "--ip", args.ip, "--json"],
                       capture_output=True, text=True, timeout=600)
    res = json.loads(r.stdout) if r.stdout.strip().startswith("{") else {"pass": False, "results": []}
    rec["smoke_failed"] = [c["check"] for c in res["results"] if not c["pass"]]
    rec["pass"] = bool(res["pass"])
    out = ssh(args.ip, "i2cget -y 4 0x68 0x2a; cat /sys/bus/i2c/devices/5-004a/hwmon/*/temp1_input")
    rec["cpld_wdt_flag"], rec["uc_mC"] = (out.stdout.split() + ["", ""])[:2]
    return rec


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--unit", required=True)
    p.add_argument("--plug", type=int, required=True)
    p.add_argument("--ip", required=True)
    p.add_argument("--cycles", type=int, default=6)
    p.add_argument("--cold-off", type=int, default=30, help="AC-off seconds for cold cycles")
    p.add_argument("--timeout", type=int, default=420, help="seconds from power-on to login")
    a = p.parse_args()
    out = ROOT / "inventory" / a.unit / "soak.jsonl"
    fails = 0
    for i in range(1, a.cycles + 1):
        kind = ("power", "warm", "cold")[(i - 1) % 3]
        rec = one(a, i, kind)
        fails += not rec["pass"]
        with out.open("a") as f:
            f.write(json.dumps(rec) + "\n")
        t = rec.get("times", {})
        print(f"cycle {i:3d} {kind:5s} {'PASS' if rec['pass'] else 'FAIL'}  "
              f"our_uboot {t.get('our_uboot', '-')}s login {t.get('login', '-')}s ready {rec.get('ready_s', '-')}s"
              + (f"  bad={rec['bad']} smoke_failed={rec.get('smoke_failed')}" if not rec["pass"] else ""),
              flush=True)
    print(f"{a.cycles - fails}/{a.cycles} cycles passed")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
