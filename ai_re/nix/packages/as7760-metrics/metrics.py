#!/usr/bin/env python3
"""AS7760-32X switch metrics for the node exporter textfile collector.

Prints Prometheus text format on stdout (the textfile producer wrapper
writes it atomically). Metric names are board-neutral (switch_*), so other
boards can emit the same families; board identity is in switch_board_info.

  as7760-metrics                      read hardware (needs root for /dev/i2c-N)
  as7760-metrics --cache FILE         keep module identity between runs
  as7760-metrics --qsfp-json FILE     optics from saved "as7760-qsfp --json"
  as7760-metrics --hwmon DIR          fans/temperatures from another sysfs tree

Optics: each run reads the lower page (64 bytes, one transaction) of every
cage; vendor/part fields (upper page, page-select write) are re-read only
when a module appears, its identifier changes, or the cache entry is older
than --static-max-age. A module swapped within one interval keeps the old
identity labels until then.
"""
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import qsfp

PORTS = sorted(qsfp.FACTORY_BUS)
# lm75 behind fan/temperature mux 0x76 channel 1 (see sensors.d/as7760.conf).
BOARD_SENSORS = {"48": "board_1", "49": "board_2", "4a": "board_3", "4b": "cpu_board"}
LM75_PATH = "i2c@119000/pca9548@76/i2c@1/temperature-sensor@"
INFO_LABELS = ("type", "vendor", "part", "rev", "compliance", "media")


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
        # %.15g, not repr(): the cross-built target Python lacks short float
        # repr (PY_NO_SHORT_FLOAT_REPR) and would print 3.2999999999999998.
        f[2].append(f"{name}{lab} {value:.15g}" if isinstance(value, float) else f"{name}{lab} {value}")

    def render(self):
        lines = []
        for name, (typ, help_, samples) in self.fams.items():
            lines += [f"# HELP {name} {help_}", f"# TYPE {name} {typ}"] + samples
        return "\n".join(lines) + "\n"


# --- optics -------------------------------------------------------------------
def load_cache(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return {}


def save_cache(path, cache):
    tmp = Path(str(path) + ".tmp")
    tmp.write_text(json.dumps(cache))
    os.replace(tmp, path)


def read_optics(cache, max_age, now=None):
    """Live read of all cages; returns (as7760-qsfp --json shaped list, cache)."""
    now = time.time() if now is None else now
    out, new_cache = [], {}
    for port in PORTS:
        bus = qsfp.bus_for(port)      # RuntimeError: adapter missing, abort the optics section
        try:
            lo = qsfp.read(bus, 0x50, 0, 64)
        except OSError:
            out.append({"port": port, "present": False})
            continue
        ident = lo[0]
        ent = cache.get(str(port))
        if not ent or ent.get("id") != ident or now - ent.get("t", 0) > max_age:
            try:
                static = qsfp.static_fields(bus, lo)
            except OSError:
                static = None
            # SFF-8636 Data_Not_Ready (byte 2 bit 0): fresh module, re-read next time.
            if static is not None and not (ident != 0x03 and lo[2] & 0x01):
                ent = {"id": ident, "t": now, "static": static}
            else:
                ent = None
        info = {"port": port, "present": True, "id": ident,
                "type": qsfp.IDS.get(ident, f"unknown {ident:#04x}")}
        # Identity unknown (read failed or module not ready): temperature and
        # supply only, no lane values that might be a passive cable's zeros.
        static = ent["static"] if ent else {"_passive": True}
        info.update(qsfp.public(static))
        try:
            info.update(qsfp.dynamic_fields(bus, lo, static))
        except OSError:
            pass
        if ent:
            new_cache[str(port)] = ent
        out.append(info)
    return out, new_cache


def emit_optics(o, ports):
    for d in ports:
        p = {"port": str(d["port"])}
        o.add("switch_optic_present", "1 if a transceiver/cable answers on the front-panel port's EEPROM address.",
              1 if d.get("present") else 0, p)
    for d in ports:
        if d.get("present"):
            lab = {"port": str(d["port"])}
            lab.update({k: str(d.get(k) or "") for k in INFO_LABELS})
            o.add("switch_optic_info", "Front-panel module identity (SFF-8024 type, vendor, part, compliance); always 1.", 1, lab)
    for key, name, help_ in (("temp_c", "switch_optic_temperature_celsius", "Module internal temperature."),
                             ("vcc_v", "switch_optic_supply_voltage_volts", "Module supply voltage (3.3 V rail).")):
        for d in ports:
            if d.get("present") and isinstance(d.get(key), (int, float)):
                o.add(name, help_, float(d[key]), {"port": str(d["port"])})
    for key, name, scale, help_ in (
            ("rx_power_mw", "switch_optic_rx_power_watts", 0.001, "Per-lane received optical power."),
            ("tx_power_mw", "switch_optic_tx_power_watts", 0.001, "Per-lane transmitted optical power (if the module monitors it)."),
            ("tx_bias_ma", "switch_optic_tx_bias_amperes", 0.001, "Per-lane laser bias current.")):
        for d in ports:
            if d.get("present") and isinstance(d.get(key), list):
                for i, v in enumerate(d[key]):
                    o.add(name, help_, round(float(v) * scale, 9), {"port": str(d["port"]), "lane": str(i + 1)})


# --- fans and board temperatures ---------------------------------------------
def rd(path):
    return Path(path).read_text().strip()


def find_hwmon(root, name):
    out = []
    for p in sorted(Path(root).glob("hwmon*")):
        try:
            if rd(p / "name") == name:
                out.append(p)
        except OSError:
            pass
    return out


def emit_fans(o, root, quiet_duty, control_active):
    fans = find_hwmon(root, "as7760_fan")
    o.add("switch_fan_controller_present", "1 if exactly one fan controller (as7760_fan hwmon) is bound.",
          1 if len(fans) == 1 else 0)
    if len(fans) != 1:
        return
    fan = fans[0]
    duty = None
    try:
        pwm = int(rd(fan / "pwm1"))
        duty = round(pwm * 100 / 255)
        o.add("switch_fan_pwm", "Shared fan PWM as read back from the fan CPLD (0-255).", pwm)
        o.add("switch_fan_duty_percent", "Shared fan duty as read back from the fan CPLD.", duty)
    except (OSError, ValueError):
        pass
    for n in range(1, 13):
        try:
            label = rd(fan / f"fan{n}_label")              # fan1_front ... fan6_rear
            num_, _, rotor = label.partition("_")
            lab = {"fan": num_[3:], "rotor": rotor}
        except OSError:
            continue
        try:
            o.add("switch_fan_rpm", "Rotor speed.", int(rd(fan / f"fan{n}_input")), lab)
        except (OSError, ValueError):
            pass
        try:
            o.add("switch_fan_fault", "1 if the rotor's fan module is absent or the rotor reads 0 rpm.",
                  int(rd(fan / f"fan{n}_fault")), lab)
        except (OSError, ValueError):
            pass
    if control_active is not None:
        o.add("switch_fan_control_active", "1 if the fan control/thermal guard service is running.",
              1 if control_active else 0)
    if quiet_duty is not None:
        o.add("switch_fan_control_quiet_duty_percent", "Configured duty while all sensors are cool and healthy.", quiet_duty)
        if duty is not None and quiet_duty < 100:
            forced = duty > quiet_duty or control_active is False
            o.add("switch_fan_full_speed_forced",
                  "1 if fans run above the quiet duty: thermal guard tripped (hot, sensor or fan error, start-up settle) or control service stopped.",
                  1 if forced else 0)


def emit_board_temps(o, root):
    for p in find_hwmon(root, "lm75"):
        try:
            node = str((p / "device/of_node").resolve())
        except OSError:
            continue
        i = node.find(LM75_PATH)
        if i < 0:
            continue
        sensor = BOARD_SENSORS.get(node[i + len(LM75_PATH):].strip("/"))
        if sensor is None:
            continue
        try:
            o.add("switch_board_temperature_celsius", "Board temperature sensor (LM75), friendly sensors.d label.",
                  int(rd(p / "temp1_input")) / 1000, {"sensor": sensor})
        except (OSError, ValueError):
            pass


def unit_active(unit):
    if not unit:
        return None
    try:
        r = subprocess.run(["systemctl", "is-active", "--quiet", unit], timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return r.returncode == 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--qsfp-json", help="use a saved 'as7760-qsfp --json' instead of reading I2C")
    ap.add_argument("--hwmon", default="/sys/class/hwmon")
    ap.add_argument("--cache", help="static module-identity cache file (e.g. under /run)")
    ap.add_argument("--static-max-age", type=float, default=600, help="seconds (default 600)")
    ap.add_argument("--quiet-duty", type=int, help="the fan service's configured quiet duty (percent)")
    ap.add_argument("--thermal-unit", default="as7760-thermal.service", help="'' to skip the check")
    a = ap.parse_args()
    o = Out()
    o.add("switch_board_info", "Switch board identity; always 1.", 1,
          {"vendor": "edgecore", "board": "as7760-32x", "asic": "bcm56930"})
    ok = 1
    try:
        if a.qsfp_json:
            ports = json.loads(Path(a.qsfp_json).read_text())
        else:
            cache = load_cache(a.cache) if a.cache else {}
            ports, cache = read_optics(cache, a.static_max_age)
            if a.cache:
                save_cache(a.cache, cache)
        emit_optics(o, ports)
    except (OSError, RuntimeError, ValueError) as exc:
        print(f"as7760-metrics: optics: {exc}", file=sys.stderr)
        ok = 0
    o.add("switch_optic_scrape_success", "1 if the front-panel module EEPROMs could be enumerated.", ok)
    emit_fans(o, a.hwmon, a.quiet_duty, unit_active(a.thermal_unit))
    emit_board_temps(o, a.hwmon)
    # TODO(psu): PSU presence/status/power is not mapped on this board yet (no
    # register or PMBus address is known from the notes); add switch_psu_* here.
    sys.stdout.write(o.render())


if __name__ == "__main__":
    main()
