"""Fans: two fan boards (top, bottom) x 7 slots = 14 fan modules, each two
rotors (front, rear) on one PWM. Read only: fand owns the PWMs (set_speed
refuses), no fan LED is driven.

Source: fand's state file (outputs top1..top7, bottom1..bottom7 of
nix/nixos/module/cs8210/fan-control; rotors labelled front/rear, rpm = tach
raw x 100 (assumed scale), max_rpm 19000 front / 21000 rear). Without a fresh
state file (fand stopped): the fan CPLD registers directly, slot n (1..7) at
0x10 + 0x10*n: +0 front tach, +1 rear tach, +2 PWM (cs8210-lib.sh).

Presence: no presence bit is known (fan CPLD slot +6 "status" is
undocumented), so a module counts as present while any of its rotors turns:
an absent and a stalled module look the same.
"""
from sonic_platform_base.fan_base import FanBase
from sonic_platform_base.fan_drawer_base import FanDrawerBase

from . import hw

BOARDS = ("top", "bottom")
SLOTS = 7
MAX_RPM = {"front": 19000, "rear": 21000}       # fand's max_rpm_by_label
TACH_SCALE = 100                                 # rpm per tach count (fand: scale = 100)
SPEED_TOLERANCE = 35                             # fand's follow tolerance, percent


def _pct(v, full):
    return max(0, min(100, int(round(v * 100.0 / full))))


class Fan(FanBase):
    def __init__(self, board, slot, side):
        FanBase.__init__(self)
        self.board, self.slot, self.side = board, slot, side
        self.output = "%s%d" % (board, slot)          # fand output name

    # ---------------------------------------------------------- sources
    def _fand(self):
        """(rotor dict, output dict) from fand, or (None, None)."""
        st = hw.fand_state()
        out = ((st or {}).get("outputs") or {}).get(self.output)
        if not out:
            return None, None
        for r in out.get("rotors") or []:
            if r.get("label") == self.side:
                return r, out
        return None, out

    def _cpld(self, plus):
        return hw.read_reg("fan_" + self.board, 0x10 + 0x10 * self.slot + plus)

    def _rpm(self):
        r, out = self._fand()
        if out is not None:
            return None if r is None or r.get("rpm") is None else float(r["rpm"])
        raw = self._cpld(0 if self.side == "front" else 1)
        return None if raw is None else float(raw * TACH_SCALE)

    def _max_rpm(self):
        r, _ = self._fand()
        return (r or {}).get("max_rpm") or MAX_RPM[self.side]

    def source(self):
        """"fand" or "cpld": where the readings come from right now."""
        return "fand" if self._fand()[1] is not None else "cpld"

    # ---------------------------------------------------------- DeviceBase
    def get_name(self):
        return "Fan %s%d %s" % (self.board, self.slot, self.side)

    def get_presence(self):
        rpm = self._rpm()
        return rpm is not None and rpm > 0

    def get_status(self):
        rpm = self._rpm()
        r, out = self._fand()
        if out is not None:
            return bool(rpm) and r is not None and not r.get("fault") and not r.get("error")
        return bool(rpm)

    def get_model(self):
        return "N/A"

    def get_serial(self):
        return "N/A"

    def get_position_in_parent(self):
        return 1 if self.side == "front" else 2

    def is_replaceable(self):
        return False                                     # the module is

    # ---------------------------------------------------------- FanBase
    def get_direction(self):
        return self.FAN_DIRECTION_NOT_APPLICABLE         # not known from any register

    def get_speed(self):
        rpm = self._rpm()
        if rpm is None:
            raise NotImplementedError
        return _pct(rpm, self._max_rpm())

    def get_target_speed(self):
        r, out = self._fand()
        if r and r.get("expected") is not None:
            return _pct(r["expected"], self._max_rpm())
        if out and out.get("duty") is not None:
            return max(0, min(100, int(round(out["duty"]))))
        raw = self._cpld(2)                              # PWM register, read only
        if raw is None:
            raise NotImplementedError
        return _pct(raw, 255)

    def get_speed_tolerance(self):
        return SPEED_TOLERANCE

    def set_speed(self, speed):
        return False                                     # fand owns the PWM

    def set_status_led(self, color):
        return False

    def get_status_led(self):
        return self.STATUS_LED_COLOR_GREEN if self.get_status() else self.STATUS_LED_COLOR_RED


class FanDrawer(FanDrawerBase):
    def __init__(self, board, slot):
        FanDrawerBase.__init__(self)
        self.board, self.slot = board, slot
        self._fan_list = [Fan(board, slot, "front"), Fan(board, slot, "rear")]

    def get_name(self):
        return "FanTray %s%d" % (self.board, self.slot)

    def get_presence(self):
        return any(f.get_presence() for f in self._fan_list)

    def get_status(self):
        return all(f.get_status() for f in self._fan_list)

    def get_model(self):
        return "N/A"

    def get_serial(self):
        return "N/A"

    def get_position_in_parent(self):
        return BOARDS.index(self.board) * SLOTS + self.slot

    def is_replaceable(self):
        return True

    def set_status_led(self, color):
        return False

    def get_status_led(self):
        return self.STATUS_LED_COLOR_GREEN if self.get_status() else self.STATUS_LED_COLOR_RED

    def get_maximum_consumed_power(self):
        raise NotImplementedError


def drawers():
    return [FanDrawer(b, s) for b in BOARDS for s in range(1, SLOTS + 1)]
