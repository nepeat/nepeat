"""Temperatures: the 13 board LM75s and the switch ASIC die. Read only
(the LM75 alarm registers are not touched).

Names and DT nodes are fand's (nix/nixos/module/cs8210/fan-control, the lm75
table); positions: devices/cs8210-32x/board-control.md. Each reading comes
from fand's sensor of the same name while fand's state file is fresh, else
from the source directly: the LM75's hwmon, and for the ASIC the file syncd
publishes (SAI_OPENBCM_TEMP_FILE = CS8210_ASIC_TEMP_FILE). get_source()
says which one answered.

Thresholds (display only) mirror fand's zones: board fans at 100% at
CS8210_BOARD_HIGH_C (55), no board critical value (fand has none); ASIC
100% at CS8210_ASIC_HIGH_C (95), every zone failsafe at CS8210_ASIC_CRIT_C (100).
"""
import os

from sonic_platform_base.thermal_base import ThermalBase

from . import hw

LM75 = [  # fand sensor name, DT node tail
    ("i2cfpga_card", "i2c-mux@71/i2c@0/temperature-sensor@48"),
    ("switchboard_left", "i2c-mux@71/i2c@1/temperature-sensor@48"),
    ("switchboard_center", "i2c-mux@71/i2c@1/temperature-sensor@4b"),
    ("switchboard_right", "i2c-mux@72/i2c@7/temperature-sensor@49"),
    ("macsec_left", "i2c-mux@73/i2c@5/temperature-sensor@49"),
    ("macsec_center", "i2c-mux@73/i2c@5/temperature-sensor@4a"),
    ("macsec_right", "i2c-mux@73/i2c@5/temperature-sensor@4c"),
    ("top_fanboard_right", "i2c-mux@74/i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4d"),
    ("top_fanboard_center", "i2c-mux@74/i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4e"),
    ("top_fanboard_left", "i2c-mux@74/i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4f"),
    ("bottom_fanboard_right", "i2c-mux@74/i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4d"),
    ("bottom_fanboard_center", "i2c-mux@74/i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4e"),
    ("bottom_fanboard_left", "i2c-mux@74/i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4f"),
]


def _env(name, default):
    try:
        return float(os.environ.get(name, default))
    except ValueError:
        return float(default)


class _Thermal(ThermalBase):
    high = critical = None

    def __init__(self, name, index):
        ThermalBase.__init__(self)
        self.name, self.index = name, index
        self.minimum = self.maximum = None
        self._source = None

    def _direct(self):
        raise NotImplementedError

    def _read(self):
        v = hw.fand_sensor(self.name)
        if v is not None:
            self._source = "fand"
            return v
        v = self._direct()
        self._source = None if v is None else self.direct_source
        return v

    def get_source(self):
        """"fand", the direct source's name, or None (no reading)."""
        self._read()
        return self._source

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
        if self.high is None:
            raise NotImplementedError
        return self.high

    def get_high_critical_threshold(self):
        if self.critical is None:
            raise NotImplementedError
        return self.critical

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
    direct_source = "hwmon"

    def __init__(self, index):
        name, self.node = LM75[index - 1]
        _Thermal.__init__(self, name, index)
        self.high = _env("CS8210_BOARD_HIGH_C", 55)

    def _direct(self):
        d = hw.hwmon_by_node(self.node)
        v = hw.read_float(d + "/temp1_input") if d else None
        return None if v is None else round(v / 1000.0, 1)


class AsicThermal(_Thermal):
    direct_source = "file"

    def __init__(self, index):
        _Thermal.__init__(self, "asic", index)
        self.high = _env("CS8210_ASIC_HIGH_C", 95)
        self.critical = _env("CS8210_ASIC_CRIT_C", 100)

    def _direct(self):
        # Removed by its writer when stale or implausible: then no reading.
        return hw.read_float(hw.ASIC_TEMP_FILE)


def thermals():
    return [BoardThermal(i) for i in range(1, len(LM75) + 1)] + [AsicThermal(len(LM75) + 1)]
