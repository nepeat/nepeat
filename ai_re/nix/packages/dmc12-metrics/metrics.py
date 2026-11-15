#!/usr/bin/env python3
"""DMC-12-48R-AS0 metrics for the node exporter textfile collector.

Prints Prometheus text format on stdout (the textfile producer wrapper
writes it atomically). Strictly read-only: SMBus byte *reads* of the board
CPLD (I2C bus 4 / 0x68), sysfs/hwmon reads, readlink of /dev/P* and
/proc/*/fd. It never writes a CPLD register (fan control owns 0x50 and
0x2a, the interrupt monitor owns the mask 0x21) and never opens a serial
port (that would toggle DTR/RTS on the attached console).

  dmc12-metrics                      read hardware (root: /dev/i2c-N, /proc/*/fd)
  dmc12-metrics --cpld-regs FILE     CPLD values from a dump ("00:05 01:88 ...")
  dmc12-metrics --hwmon/--dev/--sys/--proc DIR   other trees (tests)

Register map: ai_re/devices/dmc-12-48r-as0/vendor-tooling.md.
"""
import argparse
import json
import os
import re
import sys
from pathlib import Path

CPLD_ADDR = 0x68
# Read in this order; version first (if it fails, the CPLD section is skipped).
REG_VERSION, REG_POWER, REG_LED, REG_UPLINK = 0x01, 0x11, 0x17, 0x18
REG_INT_STATUS, REG_INT_MASK, REG_WDT, REG_PWM = 0x20, 0x21, 0x2a, 0x50
REG_TACH = (0x60, 0x61, 0x62)
REGS = (REG_VERSION, REG_POWER, REG_LED, REG_UPLINK, REG_INT_STATUS, REG_INT_MASK,
        REG_WDT, REG_PWM) + REG_TACH
# Exported raw as well (bits not all decoded: LED, uplink PHY state, unknown power bits).
RAW = (REG_POWER, REG_LED, REG_UPLINK, REG_INT_STATUS, REG_INT_MASK)
# 0x20 (active low) / 0x21 (1 = masked) bit -> alarm label.
ALARMS = ("tmp75_alert_1", "tmp75_alert_2", "fan1_failure", "fan2_failure",
          "fan3_failure", "uplink_change", "psu1_failure", "psu2_failure")
# hwmon by I2C address (any bus, like sensors.d/dmc12_sensors); keys match fand's.
SENSORS = {"004a": "k2_uc", "0048": "sfp_console", "0049": "m2"}
PORTS = 48


def esc(v):
    return str(v).replace("\\", "\\\\").replace("\n", "\\n").replace('"', '\\"')


class Out:
    def __init__(self):
        self.fams = {}

    def add(self, name, help_, value, labels=None, typ="gauge"):
        f = self.fams.setdefault(name, [typ, help_, []])
        lab = ""
        if labels:
            lab = "{" + ",".join(f'{k}="{esc(v)}"' for k, v in labels.items()) + "}"
        if isinstance(value, bool):
            value = int(value)
        # %.15g, not repr(): the cross-built target Python lacks short float repr.
        f[2].append(f"{name}{lab} {value:.15g}" if isinstance(value, float) else f"{name}{lab} {value}")

    def render(self):
        lines = []
        for name, (typ, help_, samples) in self.fams.items():
            lines += [f"# HELP {name} {help_}", f"# TYPE {name} {typ}"] + samples
        return "\n".join(lines) + "\n"


# --- CPLD ---------------------------------------------------------------------
def read_cpld_i2c(bus):
    """SMBus read-byte-data of REGS; raises OSError if the CPLD does not answer."""
    from smbus2 import SMBus
    with SMBus(bus) as b:
        return {r: b.read_byte_data(CPLD_ADDR, r) for r in REGS}


def parse_dump(text):
    """'00:05 01:88 ...' (inventory/u01/cpld-regs.txt) -> {reg: value}."""
    regs = {}
    for tok in text.split():
        k, _, v = tok.partition(":")
        regs[int(k, 16)] = int(v, 16)
    missing = [f"{r:#04x}" for r in REGS if r not in regs]
    if missing:
        raise ValueError("dump lacks registers " + " ".join(missing))
    return regs


def bit(v, n):
    return (v >> n) & 1


def duty_percent(version, step):
    """Fan duty for a PWM step: fand's map / the factory `show` (v7+ = version > 134)."""
    if step == 0:
        return 0.0
    lo, top = (4.6, 63) if version > 134 else (17.6, 15)
    return round(min(100.0, lo + (step - 1) * (100 - lo) / (top - 1)), 2)


def emit_cpld(o, regs, fans):
    v = regs[REG_VERSION]
    o.add("dmc12_cpld_version", "Board CPLD version register 0x01 (above 134: v7+, 64 PWM steps).", v)
    for r in RAW:
        o.add("dmc12_cpld_register", "Raw value of a board CPLD status register (0x11 power, 0x17 LED, 0x18 uplink PHY, 0x20/0x21 interrupts).",
              regs[r], {"register": f"{r:#04x}"})
    step = regs[REG_PWM]
    o.add("dmc12_fan_pwm_step", "Shared fan PWM step (CPLD 0x50; 1..63 on v7+, 1..15 before, 0 = off).", step)
    o.add("dmc12_fan_duty_percent", "Shared fan duty for the PWM step (fand's map / factory show).",
          float(duty_percent(v, step)))
    for n, r in enumerate(REG_TACH[:fans], start=1):
        o.add("dmc12_fan_rpm", "Fan speed (CPLD tach x 100 rpm).", regs[r] * 100, {"fan": str(n)})
    wdt = regs[REG_WDT]
    o.add("dmc12_cpld_wdt_flag", "CPLD watchdog flag register 0x2a (3 after a reset, 1 once software took over).", wdt)
    o.add("dmc12_fan_watchdog_failsafe",
          "1 if 0x2a bit 1 is set: the CPLD drives the fans at full speed until fan control clears it.", bit(wdt, 1))
    pw = regs[REG_POWER]
    for psu, b in (("1", 3), ("2", 4)):
        o.add("dmc12_psu_present", "1 if the CPLD reports the PSU (0x11 bit 3/4).", bit(pw, b), {"psu": psu})
    o.add("dmc12_dc_input_present", "1 if the 12 V DC pass-through input is detected (0x11 bit 5; PSU alarms then mean nothing).",
          bit(pw, 5))
    st, mask = regs[REG_INT_STATUS], regs[REG_INT_MASK]
    for i, name in enumerate(ALARMS):
        o.add("dmc12_cpld_alarm", "1 if the CPLD interrupt status bit is asserted (0x20, active low), masked or not.",
              1 - bit(st, i), {"alarm": name})
    for i, name in enumerate(ALARMS):
        o.add("dmc12_cpld_alarm_masked", "1 if the alarm is masked in 0x21 (set by dmc12-interruptmon).",
              bit(mask, i), {"alarm": name})


# --- temperatures ------------------------------------------------------------
def rd(path):
    return Path(path).read_text().strip()


def emit_temps(o, root):
    for p in sorted(Path(root).glob("hwmon*")):
        try:
            if rd(p / "name") not in ("lm75", "tmp75"):
                continue
            dev = os.path.basename(os.path.realpath(p / "device"))   # e.g. 5-004a
        except OSError:
            continue
        m = re.fullmatch(r"\d+-([0-9a-f]{4})", dev)
        sensor = SENSORS.get(m.group(1)) if m else None
        if sensor is None:
            continue
        try:
            o.add("dmc12_temperature_celsius", "Board temperature sensor (hwmon), sensors.d/dmc12_sensors label as sensor.",
                  int(rd(p / "temp1_input")) / 1000, {"sensor": sensor})
        except (OSError, ValueError):
            pass


# --- serial ports ------------------------------------------------------------
def open_files(proc):
    """{target path: set(pid)} from /proc/*/fd; complete=False if any process was unreadable."""
    holders, complete = {}, True
    try:
        pids = [d for d in os.listdir(proc) if d.isdigit()]
    except OSError:
        return {}, False
    for pid in pids:
        fd_dir = os.path.join(proc, pid, "fd")
        try:
            fds = os.listdir(fd_dir)
        except PermissionError:
            complete = False
            continue
        except OSError:
            continue                    # process exited
        for fd in fds:
            try:
                holders.setdefault(os.readlink(os.path.join(fd_dir, fd)), set()).add(pid)
            except OSError:
                pass
    return holders, complete


def emit_ports(o, dev, sys_root, proc):
    ports = []
    for n in range(1, PORTS + 1):
        link = os.path.join(dev, f"P{n}")
        tty = os.path.realpath(link)
        present = os.path.islink(link) and os.path.exists(tty)
        ports.append((str(n), present, tty))
        o.add("dmc12_serial_port_present", "1 if /dev/P<port> (udev, serial-ports.rules) points at a tty.",
              1 if present else 0, {"port": str(n)})
    for port, present, tty in ports:
        if present:
            name = os.path.basename(tty)
            usb = os.path.basename(os.path.dirname(os.path.realpath(os.path.join(sys_root, "class/tty", name, "device"))))
            o.add("dmc12_serial_port_info", "Front-panel port to tty and USB interface (hub.port:iface of a CP2108); always 1.",
                  1, {"port": port, "tty": name, "usb": usb})
    holders, complete = open_files(proc)
    for port, present, tty in ports:
        if present:
            o.add("dmc12_serial_port_open_processes", "Processes holding the port's tty open (from /proc/*/fd, best effort).",
                  len(holders.get(tty, ())), {"port": port})
    o.add("dmc12_serial_open_scan_complete", "1 if every process's fd table could be read (else open counts may be low).",
          1 if complete else 0)


def model(path):
    try:
        return str(json.loads(Path(path).read_text()).get("ProductName", ""))
    except (OSError, ValueError, AttributeError):
        return ""


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bus", type=int, default=4, help="CPLD I2C bus (default 4)")
    ap.add_argument("--fans", type=int, choices=(1, 2, 3), default=3, help="fans with a tach (DN0: 2)")
    ap.add_argument("--cpld-regs", help="read CPLD values from a register dump instead of I2C")
    ap.add_argument("--eeprom-json", default="/run/dmc12/eeprom.json")
    ap.add_argument("--hwmon", default="/sys/class/hwmon")
    ap.add_argument("--dev", default="/dev")
    ap.add_argument("--sys", default="/sys")
    ap.add_argument("--proc", default="/proc")
    a = ap.parse_args()
    o = Out()
    o.add("dmc12_board_info", "Console server identity (model = EEPROM ProductName); always 1.", 1,
          {"vendor": "edgecore", "board": "dmc-12-48r-as0", "model": model(a.eeprom_json)})
    ok = 1
    try:
        regs = parse_dump(Path(a.cpld_regs).read_text()) if a.cpld_regs else read_cpld_i2c(a.bus)
        emit_cpld(o, regs, a.fans)
    except (OSError, ValueError, ImportError) as exc:
        print(f"dmc12-metrics: cpld: {exc}", file=sys.stderr)
        ok = 0
    o.add("dmc12_cpld_scrape_success", "1 if all board CPLD registers could be read.", ok)
    emit_temps(o, a.hwmon)
    emit_ports(o, a.dev, a.sys, a.proc)
    sys.stdout.write(o.render())


if __name__ == "__main__":
    main()
