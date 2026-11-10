#!/usr/bin/env python3
"""
CS8210-32X fan controller.

Reads every hwmon temperature once per interval and writes one PWM value to
all 14 fan slots on the two fan CPLDs (I2C 0x66 behind the 0x74 mux, channels
2 and 3). Structure follows dmc12-nixos' fancontrol.py; the hardware contract
is this board's:

  * per-fan PWM registers 0x22,0x32..0x82, 0-255, power-on default 0xff
  * fan watchdog register 0x0c: bit 0 enable, bits 7:1 timeout in 5 s units
    (power-on 0x19 = armed, 60 s). How to pet it is not known, so, like the
    vendor's ONIE /usr/bin/disable_fcs.sh, the daemon disarms it and takes
    responsibility: any error or exit drives the fans to full speed.

Curve: linear from (low_temp, min_pwm) to (high_temp, 255) on the hottest
board sensor, and separately from (asic_low_temp, min_pwm) to
(asic_high_temp, 255) on the switch ASIC die temperature; the higher of the
two wins. Small hysteresis so the fans do not hunt. The vendor's own policy
uses a PID on the ASIC with a 97 C setpoint and a 105 C limit. The defaults keep the
vendor's 50% floor; the switch ASIC and MACsec PHYs are idle at this stage of
the port, so revisit the curve before putting load on them.
"""
import argparse
import glob
import os
import signal
import sys
import time

from smbus2 import SMBus, i2c_msg  # type: ignore

FAN_CPLD = 0x66
REG_WDT = 0x0C
PWM_REGS = [slot + 2 for slot in range(0x20, 0x90, 0x10)]
I2C_ROOT = "/sys/bus/platform/devices/fd880000.i2c"
BOARDS = {
    "top": "i2c-*/*-0070/channel-2/*-0074/channel-2",
    "bottom": "i2c-*/*-0070/channel-2/*-0074/channel-3",
}


# System CPLD 0x60 behind 0x70:2 -> 0x74:0 -> 0x71:2. Register 0x05 bits 7
# and 5 are the vendor's fan_max_speed / fan2_max_speed, active LOW: cleared
# means the CPLD drives every fan at full speed regardless of the PWM
# registers. They get cleared once the switch ASIC is initialised; the
# vendor's install.sh sets them again ("echo 0 > fan_max_speed").
SYS_CPLD = 0x60
SYS_CPLD_BUS = "i2c-*/*-0070/channel-2/*-0074/channel-0/*-0071/channel-2"
REG_FAN_MAX = 0x05
FAN_MAX_BITS = 0xA0

# Switch ASIC die temperature: Microchip MCP3425 ADC at 0x68 behind
# 0x70:2 -> 0x74:0 -> 0x72:7, sampling the ASIC's thermal diode voltage.
# Power-on config (continuous, 12 bit, gain 1) gives 1 mV per count; the
# conversion is the vendor's check_switch_temp:
#   T = -124.28 V^2 - 422.03 V + 384.62
ASIC_ADC = 0x68
ASIC_ADC_BUS = "i2c-*/*-0070/channel-2/*-0074/channel-0/*-0072/channel-7"
ASIC_ADC_CONFIG = 0x10


def read_asic_temp(bus):
    msg = i2c_msg.read(ASIC_ADC, 3)
    bus.i2c_rdwr(msg)
    hi, lo, cfg = list(msg)
    if cfg & 0x7F != ASIC_ADC_CONFIG:
        raise RuntimeError(f"MCP3425 config 0x{cfg:02x}, expected 0x{ASIC_ADC_CONFIG:02x}")
    raw = (hi << 8) | lo
    if raw & 0x8000:
        raw -= 0x10000
    v = raw / 1000.0
    t = -124.28 * v * v - 422.03 * v + 384.62
    if not 0.0 < t < 150.0:
        raise RuntimeError(f"implausible ASIC temperature {t:.1f} C (adc {raw})")
    return t


def log(msg):
    print(msg, flush=True)


def find_bus(pattern):
    hits = glob.glob(os.path.join(I2C_ROOT, pattern))
    if not hits:
        raise RuntimeError(f"i2c path not found: {pattern}")
    return int(os.path.realpath(hits[0]).rsplit("i2c-", 1)[1])


def read_temps():
    temps = {}
    for path in glob.glob("/sys/class/hwmon/hwmon*/temp*_input"):
        try:
            with open(path) as f:
                milli = int(f.read().strip())
        except (OSError, ValueError):
            continue
        base = path[: -len("_input")]
        try:
            with open(base + "_label") as f:
                name = f.read().strip()
        except OSError:
            name = os.path.basename(os.path.dirname(path))
        temps[name] = milli / 1000.0
    return temps


class Fans:
    def __init__(self):
        self.buses = {name: SMBus(find_bus(p)) for name, p in BOARDS.items()}

    def set_pwm(self, pwm):
        for bus in self.buses.values():
            for reg in PWM_REGS:
                bus.write_byte_data(FAN_CPLD, reg, pwm)

    def pwm_ok(self, pwm):
        return all(bus.read_byte_data(FAN_CPLD, reg) == pwm
                   for bus in self.buses.values() for reg in PWM_REGS)

    def watchdog(self, armed):
        for bus in self.buses.values():
            cur = bus.read_byte_data(FAN_CPLD, REG_WDT)
            new = (cur | 1) if armed else (cur & 0xFE)
            if new != cur:
                bus.write_byte_data(FAN_CPLD, REG_WDT, new)


def target_pwm(temp, low, high, min_pwm):
    if temp <= low:
        return min_pwm
    if temp >= high:
        return 255
    return round(min_pwm + (255 - min_pwm) * (temp - low) / (high - low))


def main():
    ap = argparse.ArgumentParser(description="CS8210 fan controller")
    ap.add_argument("--interval", type=float, default=10.0)
    ap.add_argument("--low-temp", type=float, default=35.0, help="at or below: min PWM")
    ap.add_argument("--high-temp", type=float, default=55.0, help="at or above: full speed")
    ap.add_argument("--min-pwm", type=int, default=127, help="floor, 0-255 (vendor quiet = 127)")
    ap.add_argument("--hysteresis", type=float, default=2.0)
    ap.add_argument("--asic-low-temp", type=float, default=70.0,
                    help="ASIC die temperature at or below which it asks for no more than min PWM")
    ap.add_argument("--asic-high-temp", type=float, default=95.0,
                    help="ASIC die temperature at or above which fans run at full speed")
    ap.add_argument("--once", action="store_true")
    a = ap.parse_args()
    if not 0 <= a.min_pwm <= 255 or a.high_temp <= a.low_temp:
        ap.error("need 0 <= min-pwm <= 255 and high-temp > low-temp")

    fans = Fans()
    sys_bus = SMBus(find_bus(SYS_CPLD_BUS))
    asic_bus = SMBus(find_bus(ASIC_ADC_BUS))

    def force_max(on):
        cur = sys_bus.read_byte_data(SYS_CPLD, REG_FAN_MAX)
        new = (cur & ~FAN_MAX_BITS & 0xFF) if on else (cur | FAN_MAX_BITS)
        if new != cur:
            sys_bus.write_byte_data(SYS_CPLD, REG_FAN_MAX, new)
        return cur

    def full_speed(*_):
        try:
            fans.set_pwm(255)
            fans.watchdog(True)
            force_max(True)
            log("fans set to full speed, CPLD forced max and watchdog re-armed")
        finally:
            sys.exit(0)

    signal.signal(signal.SIGTERM, full_speed)
    signal.signal(signal.SIGINT, full_speed)

    fans.watchdog(False)
    current = None
    ref_temp = None
    asic_ref = None
    while True:
        try:
            temps = read_temps()
            if not temps:
                raise RuntimeError("no temperature sensors readable")
            # Without a valid ASIC reading this raises, and the fans go and
            # stay at full speed (CPLD forced max) until it reads again.
            asic = read_asic_temp(asic_bus)
            name, hottest = max(temps.items(), key=lambda kv: kv[1])
            # Only move when the temperature left the hysteresis band, or up.
            if ref_temp is None or hottest > ref_temp or hottest < ref_temp - a.hysteresis:
                ref_temp = hottest
            if asic_ref is None or asic > asic_ref or asic < asic_ref - a.hysteresis:
                asic_ref = asic
            pwm = max(target_pwm(ref_temp, a.low_temp, a.high_temp, a.min_pwm),
                      target_pwm(asic_ref, a.asic_low_temp, a.asic_high_temp, a.min_pwm))

            if force_max(False) & FAN_MAX_BITS != FAN_MAX_BITS:
                log(f"system CPLD was forcing full speed; released (switch-core {asic:.1f}C)")
                current = None
            # The CPLD rewrites the PWM registers while it forces full speed.
            if pwm != current or not fans.pwm_ok(pwm):
                fans.set_pwm(pwm)
                log(f"hottest {name} {hottest:.1f}C, switch-core {asic:.1f}C -> pwm {pwm} ({pwm * 100 // 255}%)")
                current = pwm
        except Exception as ex:  # fail safe: loud beats cooked
            log(f"error: {ex!r}; forcing full speed")
            try:
                fans.set_pwm(255)
                force_max(True)
            except Exception as ex2:
                log(f"could not set full speed: {ex2!r}")
            current = None
            asic_ref = None
        if a.once:
            return 0
        time.sleep(a.interval)


if __name__ == "__main__":
    sys.exit(main() or 0)
