"""Fans: two fan boards (top, bottom) x 7 slots = 14 fan modules, each two
rotors (front, rear) on one PWM. Names use the chassis silkscreen: FAN1..7 =
bottom slots 1..7, FAN8..14 = top slots 1..7. Read only: fand owns the PWMs and
the tray LEDs (set_speed / set_status_led refuse).

Source: fand's state file (outputs top1..top7, bottom1..bottom7 of
nix/nixos/module/cs8210/fan-control; rotors labelled front/rear, rpm = tach
raw x 150 (vendor fan_cpld.ko), max_rpm 28500 front / 31500 rear; per output
present, led state ok / blink / fault / absent, isolated). Without a fresh state
file (fand stopped): the fan CPLD registers directly, slot n (1..7) at
0x10 + 0x10*n: +0 front tach, +1 rear tach, +2 PWM, +4 LED (1 green, 2 red,
3 off), +6 presence (bit 0 = 1 absent) (board-control.md "Fan CPLD register map").
"""
from sonic_platform_base.fan_base import FanBase
from sonic_platform_base.fan_drawer_base import FanDrawerBase

from . import hw

BOARDS = ("top", "bottom")
SLOTS = 7
MAX_RPM = {"front": 28500, "rear": 31500}       # fand's max_rpm_by_label
TACH_SCALE = 150                                 # rpm per tach count (fand: scale = 150)
SPEED_TOLERANCE = 35                             # fand's follow tolerance, percent


def label(board, slot):
    """The chassis silkscreen number: FAN1..7 bottom, FAN8..14 top."""
    return slot + (SLOTS if board == "top" else 0)


def _output(board, slot):
    """fand's output dict for the module, or None (no fresh state)."""
    st = hw.fand_state()
    return ((st or {}).get("outputs") or {}).get("%s%d" % (board, slot))


def module_present(board, slot):
    """fand's presence for the module; the CPLD's bit (slot +6 bit 0, 1 = absent)
    without a fresh fand state or when fand does not report it; None if unreadable."""
    out = _output(board, slot)
    if out is not None and out.get("present") is not None:
        return bool(out["present"])
    v = hw.read_reg("fan_" + board, 0x10 + 0x10 * slot + 6)
    return None if v is None else not v & 1


def led_color(base, board, slot, healthy):
    """The tray LED as fand drives it (state ok / blink / fault / absent), else
    the CPLD's LED register, else from the health."""
    out = _output(board, slot) or {}
    led = out.get("led") or {}
    state = led.get("state")
    if state == "ok":
        return base.STATUS_LED_COLOR_GREEN
    if state == "blink":
        return getattr(base, "STATUS_LED_COLOR_RED_BLINK", "red_blink")   # not in every sonic_platform_base
    if state in ("fault", "absent"):
        return base.STATUS_LED_COLOR_RED
    v = hw.read_reg("fan_" + board, 0x10 + 0x10 * slot + 4)
    if v is not None:
        return {1: base.STATUS_LED_COLOR_GREEN, 2: base.STATUS_LED_COLOR_RED,
                3: base.STATUS_LED_COLOR_OFF}.get(v & 3, base.STATUS_LED_COLOR_OFF)
    return base.STATUS_LED_COLOR_GREEN if healthy else base.STATUS_LED_COLOR_RED


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
        return "FAN%d %s" % (label(self.board, self.slot), self.side)

    def get_presence(self):
        p = module_present(self.board, self.slot)
        if p is None:                                    # nothing readable: a turning rotor is present
            rpm = self._rpm()
            return rpm is not None and rpm > 0
        return p

    def get_status(self):
        if not self.get_presence():
            return False
        rpm = self._rpm()
        r, out = self._fand()
        if out is not None:
            return (bool(rpm) and r is not None and not r.get("fault") and not r.get("error")
                    and not out.get("isolated"))
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
        return False                                     # fand drives the tray LED

    def get_status_led(self):
        return led_color(self, self.board, self.slot, self.get_status())


class FanDrawer(FanDrawerBase):
    def __init__(self, board, slot):
        FanDrawerBase.__init__(self)
        self.board, self.slot = board, slot
        self._fan_list = [Fan(board, slot, "front"), Fan(board, slot, "rear")]

    def get_name(self):
        return "FAN%d" % label(self.board, self.slot)

    def get_presence(self):
        p = module_present(self.board, self.slot)
        return any(f.get_presence() for f in self._fan_list) if p is None else p

    def get_status(self):
        return all(f.get_status() for f in self._fan_list)

    def get_model(self):
        return "N/A"

    def get_serial(self):
        return "N/A"

    def get_position_in_parent(self):
        return label(self.board, self.slot)

    def is_replaceable(self):
        return True

    def set_status_led(self, color):
        return False                                     # fand drives the tray LED

    def get_status_led(self):
        return led_color(self, self.board, self.slot, self.get_status())

    def get_maximum_consumed_power(self):
        raise NotImplementedError


def drawers():
    return [FanDrawer(b, s) for b in BOARDS for s in range(1, SLOTS + 1)]
