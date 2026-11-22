"""Temperatures: the board sensors (hwmon) and the switch ASIC (PVTMON hottest).
The AS7760 has four LM75s; a derivative board ships its own list as
sensors.json next to this module (sonic-platform-package.nix `thermals`).

Thresholds mirror fand's policy (nix/modules/thermal/as7760-fan.nix,
devices/edge-core-7760-32x/thermal.md): board zone 100% fan at 45 C,
critical 50 C; ASIC 100% at 95 C, critical 100 C. Display only: nothing here
acts on them. The LM75s' own alarm registers are not changed.
"""
from sonic_platform_base.thermal_base import ThermalBase

import json
import os

from . import hw

# name, hwmon name, I2C address, temp<N>_input, high, critical (C)
AS7760 = [["board_1", "lm75", 0x48, 1, 45, 50], ["board_2", "lm75", 0x49, 1, 45, 50],
          ["board_3", "lm75", 0x4a, 1, 45, 50], ["cpu_board", "lm75", 0x4b, 1, 45, 50]]
_sensors = None


def sensors():
    global _sensors
    if _sensors is None:
        try:
            with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "sensors.json")) as f:
                _sensors = json.load(f)
        except FileNotFoundError:
            _sensors = AS7760
    return _sensors


class _Thermal(ThermalBase):
    high, critical = None, None

    def __init__(self, name, index):
        ThermalBase.__init__(self)
        self.name, self.index = name, index
        self.minimum = self.maximum = None

    def _read(self):
        raise NotImplementedError

    def get_temperature(self):
        t = self._read()
        if t is None:
            return None
        self.minimum = t if self.minimum is None else min(self.minimum, t)
        self.maximum = t if self.maximum is None else max(self.maximum, t)
        return t

    def get_name(self):
        return self.name

    def get_presence(self):
        return self._read() is not None

    def get_status(self):
        return self._read() is not None

    def get_model(self):
        return "N/A"

    def get_serial(self):
        return "N/A"

    def get_position_in_parent(self):
        return self.index

    def is_replaceable(self):
        return False

    def get_high_threshold(self):
        return float(self.high)

    def get_high_critical_threshold(self):
        return float(self.critical)

    def get_low_threshold(self):
        raise NotImplementedError

    def get_low_critical_threshold(self):
        raise NotImplementedError

    def set_high_threshold(self, temperature):
        return False

    def set_low_threshold(self, temperature):
        return False

    def set_high_critical_threshold(self, temperature):
        return False

    def set_low_critical_threshold(self, temperature):
        return False

    def get_minimum_recorded(self):
        if self.minimum is None:
            self.get_temperature()
        return self.minimum

    def get_maximum_recorded(self):
        if self.maximum is None:
            self.get_temperature()
        return self.maximum


class BoardThermal(_Thermal):
    def __init__(self, index):
        name, self.chip, self.addr, self.input, self.high, self.critical = sensors()[index - 1]
        _Thermal.__init__(self, name, index)

    def _read(self):
        d = hw.hwmon(self.chip, self.addr)
        v = hw.read_int("%s/temp%d_input" % (d[0], self.input)) if d else None
        return None if v is None else round(v / 1000.0, 1)


class AsicThermal(_Thermal):
    high, critical = 95, 100

    def __init__(self, index):
        _Thermal.__init__(self, "asic", index)

    def _read(self):
        # Removed by its writer when stale or implausible: then no reading.
        return hw.read_float(hw.ASIC_TEMP_FILE)
