#!/usr/bin/env python3
"""Offline tests for dmc12-metrics: a fake smbus2 (reads only), u01's CPLD
dump, and fake hwmon, /dev, /sys and /proc trees; no hardware or root.
  python3 test.py OUTDIR      writes OUTDIR/metrics.prom (u01 values)
"""
import os
import subprocess
import sys
import types
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import metrics  # noqa: E402

# u01, 2026-11-15 (devices/dmc-12-48r-as0/inventory/u01/cpld-regs.txt), factory OS.
U01 = ("00:05 01:88 02:cc 10:3f 11:17 12:07 13:00 14:1f 15:ff 16:03 17:20 18:12 20:bf 21:00 "
       "28:00 29:ff 2a:01 50:2a 5f:cc 60:57 61:57 62:57")


def test_read_only_i2c():
    """read_cpld_i2c only issues read_byte_data at 0x68; any other SMBus call fails."""
    calls = []
    regs = metrics.parse_dump(U01)

    class FakeSMBus:
        def __init__(self, bus):
            calls.append(("open", bus))

        def __enter__(self):
            return self

        def __exit__(self, *a):
            calls.append(("close",))

        def read_byte_data(self, addr, reg):
            calls.append(("read", addr, reg))
            return regs[reg]

        def __getattr__(self, name):
            raise AssertionError(f"dmc12-metrics used SMBus.{name}")

    sys.modules["smbus2"] = types.SimpleNamespace(SMBus=FakeSMBus)
    got = metrics.read_cpld_i2c(4)
    assert got == {r: regs[r] for r in metrics.REGS}, got
    assert calls[0] == ("open", 4) and calls[-1] == ("close",), calls
    assert all(c[0] != "read" or (c[1] == 0x68 and c[2] in metrics.REGS) for c in calls), calls
    assert metrics.REG_INT_MASK in metrics.REGS          # read, never written
    del sys.modules["smbus2"]


def test_duty():
    assert metrics.duty_percent(0x88, 0) == 0
    assert metrics.duty_percent(0x88, 1) == 4.6
    assert metrics.duty_percent(0x88, 9) == 16.91            # fand floor, ~2000 rpm
    assert metrics.duty_percent(0x88, 0x2a) == 67.69         # U-Boot fan_speed_70
    assert metrics.duty_percent(0x88, 63) == 100
    assert metrics.duty_percent(0x80, 15) == 100             # pre-v7, 16 steps
    assert metrics.duty_percent(0x80, 1) == 17.6


def fake_tree(root):
    hw = root / "sys/class/hwmon"
    i2c = root / "sys/devices/platform/i2c"
    for n, (name, dev, mc) in enumerate([("lm75", "5-004a", 31500), ("tmp75", "7-0048", 28250),
                                         ("tmp75", "7-0049", 33000), ("lm75", "9-004c", 99000),
                                         ("ina219", "7-0044", 99000)]):
        d = i2c / dev
        d.mkdir(parents=True)
        h = hw / f"hwmon{n}"
        h.mkdir(parents=True)
        (h / "device").symlink_to(d)
        (h / "name").write_text(name + "\n")
        (h / "temp1_input").write_text(f"{mc}\n")
    dev = root / "dev"
    dev.mkdir()
    usb = root / "sys/devices/usb1"
    for p in range(1, 49):
        if p == 30:
            continue                                     # port 30 missing (CP2108 interface gone)
        tty = f"ttyUSB{p - 1}"
        (dev / tty).write_text("")
        (dev / f"P{p}").symlink_to(tty)
        hub, rest = divmod(p - 1, 16)
        iface = usb / f"1-{hub + 1}" / f"1-{hub + 1}.{rest // 4 + 1}:1.{rest % 4}" / tty
        iface.mkdir(parents=True)
        (root / "sys/class/tty" / tty).mkdir(parents=True)
        (root / "sys/class/tty" / tty / "device").symlink_to(iface)
    (dev / "P49").symlink_to("ttyUSB99")                 # dangling, ignored (only P1..P48)
    proc = root / "proc"
    for pid, targets in {"100": ["ttyUSB47", "ttyUSB47", "null"], "101": ["ttyUSB47"],
                         "200": ["ttyUSB0"], "300": []}.items():
        fd = proc / pid / "fd"
        fd.mkdir(parents=True)
        for i, t in enumerate(targets):
            (fd / str(i)).symlink_to(dev / t)
    (proc / "self").mkdir()                              # non-numeric entries are skipped
    eeprom = root / "eeprom.json"
    eeprom.write_text('{"ProductName": "DMC-12-48R-AS0", "SerialNumber": "SECRET"}')
    return eeprom


def run(args):
    r = subprocess.run([sys.executable, str(HERE / "metrics.py")] + args, capture_output=True, text=True, check=True)
    return r.stdout, r.stderr


def test_cli(out):
    root = out / "tree"
    eeprom = fake_tree(root)
    dump = out / "cpld-regs.txt"
    dump.write_text(U01 + "\n")
    common = ["--eeprom-json", str(eeprom), "--hwmon", str(root / "sys/class/hwmon"), "--dev", str(root / "dev"),
              "--sys", str(root / "sys"), "--proc", str(root / "proc")]
    text, _ = run(["--cpld-regs", str(dump)] + common)
    (out / "metrics.prom").write_text(text)
    want = [
        'dmc12_board_info{vendor="edgecore",board="dmc-12-48r-as0",model="DMC-12-48R-AS0"} 1',
        'dmc12_cpld_scrape_success 1', 'dmc12_cpld_version 136',
        'dmc12_cpld_register{register="0x11"} 23', 'dmc12_cpld_register{register="0x20"} 191',
        'dmc12_fan_pwm_step 42', 'dmc12_fan_duty_percent 67.69',
        'dmc12_fan_rpm{fan="1"} 8700', 'dmc12_fan_rpm{fan="3"} 8700',
        'dmc12_cpld_wdt_flag 1', 'dmc12_fan_watchdog_failsafe 0',
        'dmc12_psu_present{psu="1"} 0', 'dmc12_psu_present{psu="2"} 1', 'dmc12_dc_input_present 0',
        'dmc12_cpld_alarm{alarm="psu1_failure"} 1', 'dmc12_cpld_alarm{alarm="psu2_failure"} 0',
        'dmc12_cpld_alarm{alarm="uplink_change"} 0', 'dmc12_cpld_alarm_masked{alarm="psu1_failure"} 0',
        'dmc12_temperature_celsius{sensor="k2_uc"} 31.5', 'dmc12_temperature_celsius{sensor="sfp_console"} 28.25',
        'dmc12_temperature_celsius{sensor="m2"} 33',
        'dmc12_serial_port_present{port="1"} 1', 'dmc12_serial_port_present{port="30"} 0',
        'dmc12_serial_port_info{port="1",tty="ttyUSB0",usb="1-1.1:1.0"} 1',
        'dmc12_serial_port_info{port="48",tty="ttyUSB47",usb="1-3.4:1.3"} 1',
        'dmc12_serial_port_open_processes{port="48"} 2', 'dmc12_serial_port_open_processes{port="1"} 1',
        'dmc12_serial_port_open_processes{port="2"} 0', 'dmc12_serial_open_scan_complete 1',
    ]
    lines = set(text.splitlines())
    missing = [w for w in want if w not in lines]
    assert not missing, "missing:\n" + "\n".join(missing) + "\n--- got ---\n" + text
    for absent in ("SECRET", "99", 'port="49"', 'dmc12_serial_port_info{port="30"', 'port="30",'):
        assert absent not in text, absent
    assert text.count('dmc12_serial_port_present{') == 48
    assert text.count("# TYPE dmc12_cpld_alarm gauge") == 1

    # DN0 (two fans); CPLD missing: no CPLD families, success 0, the rest still there.
    two, _ = run(["--cpld-regs", str(dump), "--fans", "2"] + common)
    assert 'dmc12_fan_rpm{fan="2"}' in two and 'dmc12_fan_rpm{fan="3"}' not in two
    bad, err = run(["--cpld-regs", str(out / "nope.txt")] + common)
    assert "dmc12_cpld_scrape_success 0" in bad and "dmc12_fan_rpm" not in bad and "dmc12_cpld_alarm" not in bad, bad
    assert 'dmc12_temperature_celsius{sensor="k2_uc"} 31.5' in bad and "cpld:" in err
    # Unreadable fd table: counted as incomplete, not fatal.
    if os.geteuid() != 0:
        os.chmod(root / "proc/300/fd", 0)
        part, _ = run(["--cpld-regs", str(dump)] + common)
        os.chmod(root / "proc/300/fd", 0o755)
        assert "dmc12_serial_open_scan_complete 0" in part, part


if __name__ == "__main__":
    out = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    out.mkdir(parents=True, exist_ok=True)
    test_read_only_i2c()
    test_duty()
    test_cli(out)
    print("dmc12-metrics tests passed")
