"""Temperatures: the four board LM75s and the switch ASIC (PVTMON hottest).

Thresholds mirror fand's policy (nix/modules/thermal/as7760-fan.nix,
devices/edge-core-7760-32x/thermal.md): board zone 100% fan at 45 C,
critical 50 C; ASIC 100% at 95 C, critical 100 C. Display only: nothing here
acts on them. The LM75s' own alarm registers are not changed.
"""
from sonic_platform_base.thermal_base import ThermalBase

from . import hw

BOARD = [("board_1", 0x48), ("board_2", 0x49), ("board_3", 0x4a), ("cpu_board", 0x4b)]


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
    high, critical = 45, 50

    def __init__(self, index):
        name, self.addr = BOARD[index - 1]
        _Thermal.__init__(self, name, index)

    def _read(self):
        d = hw.hwmon("lm75", self.addr)
        v = hw.read_int(d[0] + "/temp1_input") if d else None
        return None if v is None else round(v / 1000.0, 1)


class AsicThermal(_Thermal):
    high, critical = 95, 100

    def __init__(self, index):
        _Thermal.__init__(self, "asic", index)

    def _read(self):
        # Removed by its writer when stale or implausible: then no reading.
        return hw.read_float(hw.ASIC_TEMP_FILE)
