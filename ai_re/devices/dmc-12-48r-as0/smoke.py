#!/usr/bin/env python3
"""Smoke-test a running DMC-12 NixOS (netbooted or installed). Read-only by default.

  smoke.py --ip 10.60.2.10              # health checks, PASS/FAIL per item
  smoke.py --ip 10.60.2.10 --reboot     # + reboot: the CPLD hook must fire
  smoke.py --ip 10.60.2.10 --json

--reboot needs the serial session `serial-dmc12` and leaves the board stopped
at the factory U-Boot prompt (ALPINE_DB>): after the reset it checks that the
CPLD WDT flag reads 0x03, i.e. the CPLD did the reset, not the sp805 SoC-only
fallback (which leaves it at 0x01).
"""
import argparse, json, pathlib, re, subprocess, sys, time

ROOT = pathlib.Path(__file__).resolve().parent
SSH = ["sudo", "-n", "ssh", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no",
       "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR", "-o", "ConnectTimeout=10"]

# name, remote command, check(stdout) -> bool, what PASS means
CHECKS = [
    ("system", "systemctl is-system-running", lambda o: o.strip() == "running", "systemd running, no failed units"),
    ("failed-units", "systemctl --failed --no-legend | wc -l", lambda o: o.strip() == "0", "0 failed units"),
    ("kernel", "uname -r; cat /proc/device-tree/model", lambda o: "DMC-12" in o, "board DT loaded"),
    ("cpus", "nproc", lambda o: o.strip() == "4", "4 CPUs (PSCI cpu_on)"),
    ("eth0", "cat /sys/class/net/eth0/carrier /sys/class/net/eth0/speed", lambda o: o.split()[:2] == ["1", "1000"], "al_eth link at 1G"),
    ("ports", "ls /dev/P* | wc -l; ls /dev/ttyUSB* | wc -l", lambda o: o.split() == ["48", "48"], "48 /dev/P* + 48 ttyUSB"),
    ("i2c-numbering", "cat /sys/bus/i2c/devices/i2c-4/name /sys/bus/i2c/devices/i2c-8/name", lambda o: "chan_id 3" in o and o.count("chan_id 3") == 2, "i2c-4 = 0x70 ch3, i2c-8 = 0x71 ch3"),
    ("sensors", "cat /sys/bus/i2c/devices/5-004a/hwmon/*/temp1_input /sys/bus/i2c/devices/7-0048/hwmon/*/temp1_input /sys/bus/i2c/devices/7-0049/hwmon/*/temp1_input", lambda o: len(o.split()) == 3 and all(15000 < int(x) < 75000 for x in o.split()), "3 temps, 15..75 C"),
    ("gpio-names", "gpioinfo | grep -c -E '\"CPLD_(INT_N|JTAG_EN)\"' || true", lambda o: o.strip() == "2", "named CPLD lines"),
    # MTD_WRITEABLE = 0x400: only our U-Boot's env window (boards/dmc12/uboot-env DT overlay).
    ("spi-nor", "for d in /sys/class/mtd/mtd[0-9]*; do case $d in *ro) continue;; esac; echo \"$(cat $d/name)=$(cat $d/flags)\"; done | sort | tr '\\n' ' '",
     lambda o: o.split() == ["spi0-boot=0x800", "spi0-tail=0x800", "spi1-alt=0x800", "u-boot-env=0xc00"],
     "SPI NOR partitions: only u-boot-env writable"),
    ("consoled", "systemctl is-active consoled; console --metrics | grep -c '^consoled_port_up{.*} 1$'",
     lambda o: o.split() == ["active", "48"], "consoled up, all 48 serial ports open"),
    ("watchdog", "cat /sys/class/watchdog/watchdog0/identity", lambda o: "sp805" in o, "sp805 bound"),
    ("cpld", "i2cget -y 4 0x68 0x01", lambda o: o.strip() == "0x88", "CPLD answers (version 0x88 on u01)"),
    ("services", "for u in dmc12-interruptmon dmc12-onieeeprom dmc12-setphyspeed; do systemctl is-active $u; done; systemctl is-active fand",
     lambda o: o.split() == ["active"] * 4, "board services and fand up"),
    ("fans", "for r in 0x60 0x61 0x62; do echo $(( $(i2cget -y 4 0x68 $r) * 100 )); done", lambda o: all(1500 <= int(x) <= 15000 for x in o.split()), "3 fans spinning"),
    ("dmesg-warn", "dmesg --level=err,warn | grep -c -E 'WARNING|BUG|Oops|Call trace' || true", lambda o: o.strip() == "0", "no kernel WARNING/BUG/Oops"),
]


def ssh(ip, cmd, timeout=60):
    r = subprocess.run(SSH + [f"root@{ip}", cmd], capture_output=True, text=True, timeout=timeout)
    return r.returncode, r.stdout.replace("\0", "\n")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--ip", required=True)
    p.add_argument("--reboot", action="store_true")
    p.add_argument("--json", action="store_true")
    a = p.parse_args()
    results = []
    for name, cmd, ok, means in CHECKS:
        try:
            rc, out = ssh(a.ip, cmd)
            good = rc == 0 and ok(out)
        except Exception as e:  # noqa: BLE001 - a broken check is a FAIL, not a crash
            good, out = False, f"error: {e}"
        results.append({"check": name, "pass": good, "means": means, "out": out.strip()[-200:]})
    if a.reboot:
        sys.path.insert(0, str(ROOT))
        waiter = subprocess.Popen([sys.executable, str(ROOT / "powercycle_to_uboot.py"), "--no-power"],
                                  stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        time.sleep(1)
        t0 = time.monotonic()
        subprocess.run(SSH + [f"root@{a.ip}", "systemctl reboot"], capture_output=True, timeout=30)
        out, _ = waiter.communicate(timeout=240)
        took = time.monotonic() - t0
        log = subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", "serial-dmc12"],
                             capture_output=True, text=True).stdout
        hook = "board reset via CPLD" in log
        flag = subprocess.run([sys.executable, str(ROOT / "console.py"), "i2c dev 4; i2c md 0x68 0x2a 1"],
                              capture_output=True, text=True).stdout
        results.append({"check": "reboot-hook", "pass": hook, "means": "CPLD shutdown hook ran", "out": f"{took:.0f}s to U-Boot"})
        results.append({"check": "reboot-cpld", "pass": "002a: 03" in flag, "means": "CPLD did the reset (WDT flag 0x03)", "out": flag.strip()[-60:]})
        results.append({"check": "reboot-no-warn", "pass": "Call trace" not in log.split("The system will reboot now")[-1], "means": "no kernel trace during shutdown", "out": ""})
    failed = [r for r in results if not r["pass"]]
    if a.json:
        print(json.dumps({"pass": not failed, "results": results}))
    else:
        for r in results:
            print(f"{'PASS' if r['pass'] else 'FAIL'}  {r['check']:<15} {r['means']}" + ("" if r["pass"] else f"   [{r['out']}]"))
        print(f"{len(results) - len(failed)}/{len(results)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
