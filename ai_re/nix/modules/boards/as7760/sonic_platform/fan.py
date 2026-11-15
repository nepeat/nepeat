"""Fans: six drawers of two rotors (front, rear), one shared PWM.

Read only. fand (rust/crates/fand) owns the PWM; nothing here writes it
(set_speed refuses), and there is no fan LED to drive (set_status_led
refuses). Speeds: hwmon as7760_fan (fan<N>_input, N = 2*drawer-1 front,
2*drawer rear); target and limits from fand's state file when fand runs.
"""
from sonic_platform_base.fan_base import FanBase
from sonic_platform_base.fan_drawer_base import FanDrawerBase

from . import hw

# Full-speed rotor rpm (fand's max_rpm_by_label; thermal.md: 21.2-21.6k
# front, 17.9-18.0k rear at 100%), used when fand gives no max_rpm.
MAX_RPM = {"front": 21000, "rear": 18000}
# fand's tach follow tolerance (follow_tolerance = 0.35), in percent.
SPEED_TOLERANCE = 35


def _hwmon_dir():
    d = hw.hwmon("as7760_fan")
    return d[0] if d else None


def _rotors():
    """fand's rotor list, keyed by hwmon label, or {}."""
    st = hw.fand_state()
    if not st:
        return {}, None
    out = (st.get("outputs") or {}).get("fans") or {}
    return {r.get("label"): r for r in out.get("rotors", [])}, out.get("duty")


class Fan(FanBase):
    def __init__(self, drawer, side):
        FanBase.__init__(self)
        self.drawer, self.side = drawer, side            # 1..6, "front"/"rear"
        self.channel = 2 * drawer - (1 if side == "front" else 0)   # hwmon fan<channel>
        self.label = "fan%d_%s" % (drawer, side)

    def _attr(self, a):
        d = _hwmon_dir()
        return hw.read_int("%s/fan%d_%s" % (d, self.channel, a)) if d else None

    def _rpm(self):
        return self._attr("input")

    def _fand(self):
        rotors, duty = _rotors()
        return rotors.get(self.label), duty

    def _max_rpm(self):
        r, _ = self._fand()
        return (r or {}).get("max_rpm") or MAX_RPM[self.side]

    # DeviceBase
    def get_name(self):
        return "Fan %d %s" % (self.drawer, self.side)

    def get_presence(self):
        # The driver's fault = drawer absent (fan CPLD 0x0f) or rotor stopped;
        # a drawer counts as present while any of its rotors turns.
        rpm, fault = self._rpm(), self._attr("fault")
        if rpm is None:
            return False
        return not (fault and rpm == 0)

    def get_status(self):
        rpm, fault = self._rpm(), self._attr("fault")
        r, _ = self._fand()
        return bool(rpm) and not fault and not (r or {}).get("fault", False)

    def get_model(self):
        return "N/A"

    def get_serial(self):
        return "N/A"

    def get_position_in_parent(self):
        return 1 if self.side == "front" else 2

    def is_replaceable(self):
        return False                                     # the drawer is

    # FanBase
    def get_direction(self):
        # Fan CPLD 0x10 is an airflow bitmap whose polarity is not
        # established (thermal.md); not reported rather than guessed.
        return self.FAN_DIRECTION_NOT_APPLICABLE

    def get_speed(self):
        rpm = self._rpm()
        if rpm is None:
            raise NotImplementedError
        return max(0, min(100, int(round(rpm * 100.0 / self._max_rpm()))))

    def get_target_speed(self):
        r, duty = self._fand()
        if r and r.get("expected") is not None:
            return max(0, min(100, int(round(r["expected"] * 100.0 / self._max_rpm()))))
        if duty is not None:
            return max(0, min(100, int(round(duty))))
        d = _hwmon_dir()
        raw = hw.read_int(d + "/pwm1") if d else None       # read only
        if raw is None:
            raise NotImplementedError
        return int(round(raw * 100.0 / 255))

    def get_speed_tolerance(self):
        return SPEED_TOLERANCE

    def set_speed(self, speed):
        return False                                     # fand owns the PWM

    def set_status_led(self, color):
        return False                                     # no fan LED control

    def get_status_led(self):
        return self.STATUS_LED_COLOR_GREEN if self.get_status() else self.STATUS_LED_COLOR_RED


class FanDrawer(FanDrawerBase):
    def __init__(self, index):
        FanDrawerBase.__init__(self)
        self.index = index
        self._fan_list = [Fan(index, "front"), Fan(index, "rear")]

    def get_name(self):
        return "FanTray%d" % self.index

    def get_presence(self):
        return any(f.get_presence() for f in self._fan_list)

    def get_status(self):
        return all(f.get_status() for f in self._fan_list)

    def get_model(self):
        return "N/A"

    def get_serial(self):
        return "N/A"

    def get_position_in_parent(self):
        return self.index

    def is_replaceable(self):
        return True

    def set_status_led(self, color):
        return False

    def get_status_led(self):
        return self.STATUS_LED_COLOR_GREEN if self.get_status() else self.STATUS_LED_COLOR_RED

    def get_maximum_consumed_power(self):
        raise NotImplementedError
