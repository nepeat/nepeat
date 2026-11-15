#!/usr/bin/env python3
"""
DMC-12-48R-AS0 fan controller — port of the factory Python script.

Reads the highest temperature from `sensors` (lm_sensors) every N seconds and
writes the resulting PWM step to the board CPLD over I2C.

Hardware contract (unchanged from the factory script):
  * I2C bus  : configurable, default 4 (matches U-Boot's `i2c dev 4`).
                Linux bus number depends on the DT/mux tree; --bus overrides.
  * CPLD i2c addr   : 0x68
  * Fan PWM register: 0x50  (write byte; 0x00 = off, max = 15 or 63 by CPLD ver)
  * WDT flag reg    : 0x2a  (== 3 after a WDT reset; clear by writing 0)
  * CPLD ver reg    : 0x01

PI control loop: step = clamp(round(Kp*err + Ki*Sum(err)), 1..upper).
upper is 15 on CPLD < v7, 63 on CPLD >= v7. Minimum step forced to 9 to
keep fans above the AS0 chassis 2000 RPM alarm threshold (a quirk of this
SKU; the original DN0 chassis would tolerate lower).
"""
import argparse
import configparser
import re
import subprocess
import sys
import time
from syslog import LOG_ERR, LOG_INFO, openlog, syslog

# smbus2 is the maintained pure-Python smbus binding; the factory used the
# older 'smbus' C extension. The wire-level API for read/write_byte_data is
# identical. NixOS ships python3Packages.smbus2 in nixpkgs.
from smbus2 import SMBus  # type: ignore


CPLD_ADDR = 0x68
REG_VERSION = 0x01
REG_WDT_FLAG = 0x2a
REG_FAN_PWM = 0x50
WDT_TRIPPED_VALUE = 3
MIN_FAN_STEP = 0x09  # 16.92% PWM ~= 2000 RPM, above AS0's RPM alarm floor

# Output of `sensors`: `M.2 (U10):    +28.0°C  (high = +80.0°C, hyst = +75.0°C)`
# The `\s+\(` anchor matches the trailing "  (" so we pick the actual reading
# at the start of the line and skip the +80.0°C / +75.0°C trip points (which
# are followed by ", " not " (").  The optional `°` covers post-3.5 lm-sensors
# (UTF-8 degree sign in front of C); pre-3.5 builds emit a literal space C.
TEMP_PATTERN = re.compile(r"\+(\d+\.*\d*)\s*°?C\s+\(", re.UNICODE)
# Same, with the sensors.d label in front: `M.2 (U10):    +28.0°C  (high = ...`
LABELLED_PATTERN = re.compile(r"^(.+?):\s+\+(\d+\.*\d*)\s*°?C\s+\(", re.UNICODE | re.MULTILINE)


class PI:
    """PI controller. D term skipped — temperature changes are slow."""

    def __init__(self, set_temp, Kp, Ki, upper_limit,
                 integrator_max=1000, integrator_min=-1000):
        self.Kp = Kp
        self.Ki = Ki
        self.set_temp = set_temp
        self.upper_limit = upper_limit
        self.integrator = 0
        self.integrator_max = integrator_max
        self.integrator_min = integrator_min

    def step(self, current_temp):
        err = current_temp - self.set_temp
        self.integrator += err
        if self.integrator > self.integrator_max:
            self.integrator = self.integrator_max
        elif self.integrator < self.integrator_min:
            self.integrator = self.integrator_min
        pi = round(self.Kp * err + self.Ki * self.integrator)
        if pi > self.upper_limit:
            pi = self.upper_limit
        elif pi < 1:
            pi = 1
        return pi


def load_config(path):
    cfg = configparser.ConfigParser()
    cfg["fancontrol"] = {
        "timer": "10",
        "target_temperature": "45",
        "Kp": "3.0",
        "Ki": "0.1",
    }
    cfg["targets"] = {}
    try:
        cfg.read(path)
        syslog(LOG_INFO, f"loaded config: {path}")
    except Exception as ex:
        syslog(LOG_ERR, f"falling back to defaults; could not read {path}: {ex}")
    return cfg["fancontrol"], {k: float(v) for k, v in cfg["targets"].items()}


def clear_wdt(bus):
    flag = bus.read_byte_data(CPLD_ADDR, REG_WDT_FLAG)
    if flag == WDT_TRIPPED_VALUE:
        bus.write_byte_data(CPLD_ADDR, REG_WDT_FLAG, 0)
        syslog(LOG_INFO, "WDT trip flag cleared; fan control enabled")


def adjust_fans(bus, step):
    if step < MIN_FAN_STEP:
        step = MIN_FAN_STEP
    cur = bus.read_byte_data(CPLD_ADDR, REG_FAN_PWM)
    if cur != step:
        bus.write_byte_data(CPLD_ADDR, REG_FAN_PWM, step)


def hottest_sensor(sensors_cmd, targets=None, default=45.0):
    """Worst sensor, as a temperature on the default target's scale.

    Without per-label targets this is the factory behaviour (the hottest
    reading). With targets (label -> degC, e.g. the K2 uC at 60 while the
    board sensors keep the stock 45), each reading counts by how far it is
    above its own target: returns default + max(temp - target(label)).
    """
    out = subprocess.check_output(sensors_cmd, text=True)
    if targets:
        pairs = [(label.strip().lower(), float(t)) for label, t in LABELLED_PATTERN.findall(out)]
        if pairs:
            return default + max(t - targets.get(label, default) for label, t in pairs)
    temps = [float(m) for m in TEMP_PATTERN.findall(out)]
    if not temps:
        raise RuntimeError(f"no temperatures parsed from `{' '.join(sensors_cmd)}` output")
    return max(temps)


def main():
    ap = argparse.ArgumentParser(description="DMC-12 fan controller (PI loop on CPLD PWM via i2c)")
    ap.add_argument("--bus", type=int, default=4,
                    help="Linux i2c bus number that maps to U-Boot dev 4 (default 4)")
    ap.add_argument("--config", default="/etc/fancontrol.cfg",
                    help="ini config with [fancontrol] section (default /etc/fancontrol.cfg)")
    ap.add_argument("--sensors-cmd", default="sensors",
                    help="program to invoke for temperature readings (default 'sensors')")
    ap.add_argument("--once", action="store_true",
                    help="run one iteration and exit (useful for testing)")
    a = ap.parse_args()

    openlog("dmc12-fancontrol")
    cfg, targets = load_config(a.config)
    timer = cfg.getint("timer")
    target = cfg.getfloat("target_temperature")
    Kp = cfg.getfloat("Kp")
    Ki = cfg.getfloat("Ki")

    bus = SMBus(a.bus)
    clear_wdt(bus)

    cpld_version = bus.read_byte_data(CPLD_ADDR, REG_VERSION)
    syslog(LOG_INFO, f"CPLD version byte 0x{cpld_version:02x}")
    # >134 is the factory's CPLD-v7-or-newer threshold; below that the chip
    # has only 16 PWM steps instead of 64.
    upper = 63 if cpld_version > 134 else 15
    controller = PI(target, Kp, Ki, upper)
    if targets:
        syslog(LOG_INFO, f"per-sensor targets: {targets} (others {target})")

    if a.once:
        temp = hottest_sensor([a.sensors_cmd], targets, target)
        step = controller.step(temp)
        adjust_fans(bus, step)
        syslog(LOG_INFO, f"oneshot: T={temp:.1f}C step={step}/{upper}")
        return 0

    elapsed = 0
    while True:
        try:
            time.sleep(1)
            elapsed += 1
            if elapsed < timer:
                continue
            elapsed = 0
            temp = hottest_sensor([a.sensors_cmd], targets, target)
            step = controller.step(temp)
            adjust_fans(bus, step)
        except KeyboardInterrupt:
            return 0
        except Exception as ex:  # pragma: no cover
            syslog(LOG_ERR, f"loop error: {ex!r}")
            # Don't fast-spin on persistent errors.
            time.sleep(timer)


if __name__ == "__main__":
    sys.exit(main() or 0)
