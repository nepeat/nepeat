#!/usr/bin/env python3
"""Offline tests for as7760-metrics and as7760-qsfp's decoder.

Fakes the I2C layer (qsfp.bus_for/read/select_page0) with synthetic EEPROM
images and builds a fake hwmon tree; no hardware or root needed.
  python3 test.py OUTDIR      writes OUTDIR/{optics.json,metrics.prom}
"""
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import qsfp     # noqa: E402
import metrics  # noqa: E402


def put(buf, off, data):
    buf[off:off + len(data)] = data


def be16(v):
    return int(v).to_bytes(2, "big", signed=v < 0)


def ascii16(s):
    return s.ljust(16).encode()


def qsfp_image(active):
    lo, up = bytearray(128), bytearray(128)
    lo[0] = up[0] = 0x11
    put(lo, 22, be16(35.5 * 256))
    put(lo, 26, be16(33000))
    for i in range(4):
        put(lo, 34 + 2 * i, be16(5000 + 100 * i))   # 0.5.. mW
        put(lo, 42 + 2 * i, be16(3250))              # 6.5 mA
        put(lo, 50 + 2 * i, be16(6000))              # 0.6 mW
    up[19] = 0x00 if active else 0xa0
    up[18] = 0 if active else 1
    put(up, 20, ascii16("ACME OPTICS" if active else "ACME CABLES"))
    put(up, 40, ascii16("SR4-TEST" if active else "DAC-1M"))
    put(up, 56, b"A1")
    put(up, 68, ascii16("TESTSERIAL"))
    put(up, 84, b"26010100")
    up[64] = 0x02 if active else 0x0b
    up[92] = 0x0c if active else 0x00
    return {(0x50, 0): bytes(lo), (0x50, 128): bytes(up)}


def sfp_image():
    a0, a2 = bytearray(128), bytearray(128)
    a0[0] = 0x03
    a0[36] = 0x02
    a0[92] = 0x68
    put(a0, 20, ascii16("ACME SFP"))
    put(a0, 40, ascii16("SFP-SR-TEST"))
    put(a0, 56, b"1.0 ")
    put(a2, 96, be16(40 * 256) + be16(33100) + be16(3500) + be16(5500) + be16(4500))
    return {(0x50, 0): bytes(a0), (0x51, 0): bytes(a2)}


class FakeBus:
    def __init__(self, modules):
        self.modules = dict(modules)       # port -> image
        self.calls = []

    def bus_for(self, port):
        return 100 + port

    def read(self, bus, addr, offset, length):
        img = self.modules.get(bus - 100)
        self.calls.append((bus - 100, addr, offset, length))
        if img is None:
            raise OSError(6, "No such device or address")
        base = 0 if offset < 128 or (addr, 128) not in img else 128
        page = img.get((addr, base))
        if page is None:
            raise OSError(6, "No such device or address")
        return page[offset - base:offset - base + length]

    def select_page0(self, bus):
        self.calls.append((bus - 100, 0x50, "page0"))

    def install(self):
        qsfp.bus_for, qsfp.read, qsfp.select_page0 = self.bus_for, self.read, self.select_page0


def upper_reads(fake):
    return [c for c in fake.calls if c[2] in ("page0", 128) or (c[1] == 0x50 and c[2] == 0 and c[3] == 128)]


def test_optics(out):
    fake = FakeBus({1: qsfp_image(True), 2: qsfp_image(False), 3: sfp_image()})
    fake.install()

    # as7760-qsfp's own decode keeps its JSON shape (keys and values as before).
    d1 = qsfp.decode(1)
    assert d1 == {"port": 1, "present": True, "bus": 101, "id": 0x11, "type": "QSFP28",
                  "vendor": "ACME OPTICS", "part": "SR4-TEST", "rev": "A1", "serial": "TESTSERIAL",
                  "date": "26010100", "compliance": "100GBASE-SR4/25GBASE-SR", "copper_m": None,
                  "breakout": None, "media": "active/optical", "temp_c": 35.5, "vcc_v": 3.3,
                  "rx_power_mw": [0.5, 0.51, 0.52, 0.53], "tx_bias_ma": [6.5] * 4,
                  "tx_power_mw": [0.6] * 4}, d1
    d2 = qsfp.decode(2)
    assert d2["media"] == "passive copper" and d2["copper_m"] == 1 and "rx_power_mw" not in d2, d2
    d3 = qsfp.decode(3)
    assert d3["type"] == "SFP/SFP+/SFP28" and d3["temp_c"] == 40.0 and d3["rx_power_mw"] == [0.45], d3
    assert qsfp.decode(4) == {"port": 4, "present": False, "bus": 104}

    # Cache: the second run must not touch upper pages / A0 identity again.
    fake.calls.clear()
    ports, cache = metrics.read_optics({}, 600, now=1000)
    first = len(upper_reads(fake))
    assert first >= 3, fake.calls
    fake.calls.clear()
    ports2, cache = metrics.read_optics(json.loads(json.dumps(cache)), 600, now=1030)
    assert upper_reads(fake) == [], fake.calls
    assert ports2 == ports, (ports, ports2)
    assert all(c[3] <= 64 or c[1] == 0x51 for c in fake.calls), fake.calls
    # Expired identity is refreshed; a removed module drops out of the cache.
    fake.calls.clear()
    _, cache = metrics.read_optics(cache, 600, now=2000)
    assert len(upper_reads(fake)) == first, fake.calls
    del fake.modules[2]
    ports3, cache = metrics.read_optics(cache, 600, now=2030)
    assert "2" not in cache and not ports3[1]["present"]
    (out / "optics.json").write_text(json.dumps(ports, indent=2))
    return ports


def fake_hwmon(root):
    fan = root / "hwmon0"
    fan.mkdir(parents=True)
    (fan / "name").write_text("as7760_fan\n")
    (fan / "pwm1").write_text("204\n")
    for n in range(1, 13):
        (fan / f"fan{n}_label").write_text(f"fan{(n + 1) // 2}_{'front' if n % 2 else 'rear'}\n")
        (fan / f"fan{n}_input").write_text(("0" if n == 4 else str(15000 + n * 100)) + "\n")
        (fan / f"fan{n}_fault").write_text(("1" if n == 4 else "0") + "\n")
    for i, addr in enumerate(["48", "49", "4a", "4b"], start=1):
        h = root / f"hwmon{i}"
        node = root / "of" / "soc@ffe000000" / "i2c@119000" / "pca9548@76" / "i2c@1" / f"temperature-sensor@{addr}"
        node.mkdir(parents=True)
        dev = root / "devices" / f"0-00{addr}"
        dev.mkdir(parents=True)
        (dev / "of_node").symlink_to(node)
        h.mkdir()
        (h / "device").symlink_to(dev)
        (h / "name").write_text("lm75\n")
        (h / "temp1_input").write_text(f"{29000 + i * 500}\n")
    other = root / "hwmon9"           # an lm75 elsewhere must be ignored
    other.mkdir()
    (other / "name").write_text("lm75\n")
    (other / "temp1_input").write_text("99000\n")


def test_cli(out):
    hw = out / "sys-class-hwmon"
    fake_hwmon(hw)
    r = subprocess.run([sys.executable, str(HERE / "metrics.py"), "--qsfp-json", str(out / "optics.json"),
                        "--hwmon", str(hw), "--quiet-duty", "70", "--thermal-unit", ""],
                       capture_output=True, text=True, check=True)
    text = r.stdout
    (out / "metrics.prom").write_text(text)
    want = [
        'switch_board_info{vendor="edgecore",board="as7760-32x",asic="bcm56930"} 1',
        'switch_optic_present{port="1"} 1', 'switch_optic_present{port="4"} 0', 'switch_optic_present{port="32"} 0',
        'switch_optic_info{port="1",type="QSFP28",vendor="ACME OPTICS",part="SR4-TEST",rev="A1",compliance="100GBASE-SR4/25GBASE-SR",media="active/optical"} 1',
        'switch_optic_temperature_celsius{port="1"} 35.5', 'switch_optic_supply_voltage_volts{port="1"} 3.3',
        'switch_optic_rx_power_watts{port="1",lane="2"} 0.00051', 'switch_optic_tx_bias_amperes{port="1",lane="4"} 0.0065',
        'switch_optic_tx_power_watts{port="1",lane="1"} 0.0006', 'switch_optic_temperature_celsius{port="3"} 40',
        'switch_optic_rx_power_watts{port="3",lane="1"} 0.00045', 'switch_optic_scrape_success 1',
        'switch_fan_controller_present 1', 'switch_fan_pwm 204', 'switch_fan_duty_percent 80',
        'switch_fan_rpm{fan="1",rotor="front"} 15100', 'switch_fan_rpm{fan="6",rotor="rear"} 16200',
        'switch_fan_fault{fan="2",rotor="rear"} 1', 'switch_fan_fault{fan="2",rotor="front"} 0',
        'switch_fan_control_quiet_duty_percent 70', 'switch_fan_full_speed_forced 1',
        'switch_board_temperature_celsius{sensor="board_1"} 29.5',
        'switch_board_temperature_celsius{sensor="cpu_board"} 31',
    ]
    lines = set(text.splitlines())
    missing = [w for w in want if w not in lines]
    assert not missing, "missing:\n" + "\n".join(missing) + "\n--- got ---\n" + text
    for absent in ('switch_optic_rx_power_watts{port="2"', "99.0", "switch_fan_control_active"):
        assert absent not in text, absent
    assert text.count("# TYPE switch_optic_present gauge") == 1


if __name__ == "__main__":
    out = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    out.mkdir(parents=True, exist_ok=True)
    test_optics(out)
    test_cli(out)
    print("as7760-metrics tests passed")
