"""PSUs: four slots, presence and input/output good from the system CPLD.

Bits from the vendor sys_cpld.ko (DWARF + emulation, devices/cs8210-32x/
analysis/cpld-diff/sys_cpld_psu_attrs.json; table in cpld-diff.md):
  reg 0x08: PSU1 present b4, PSU2 present b5 (active low);
            input good b2 / b3, output good b0 / b1 (active high)
  reg 0xa8: the same for PSU3 / PSU4
Power good here = present and output good. UNVERIFIED on this unit: the
live dump of 2026-11-15 (0x08 = 0x45, 0xa8 = 0x00) decodes as PSU1 present
with input and output good, and PSU2, PSU3 and PSU4 "present" without input
or output, while only PSU1 is known to be fitted (PROGRESS 2026-11-10 (g):
PMBus answers on PSU1 only). So the present bits of slots 2-4 do not match
what we know; 0xa8 reading all zero suggests it is not wired on this
(19-inch, sys 0x09 b0 = 1) variant. A pull/plug test settles it.
No PMBus here (model, serial, voltage, current, power: not available). No LEDs.
"""
from sonic_platform_base.psu_base import PsuBase

from . import hw

# index: (register, present bit, input-good bit, output-good bit)
BITS = {1: (0x08, 4, 2, 0), 2: (0x08, 5, 3, 1), 3: (0xa8, 4, 2, 0), 4: (0xa8, 5, 3, 1)}


class Psu(PsuBase):
    def __init__(self, index):
        PsuBase.__init__(self)
        self.index = index

    def _bits(self):
        reg, prs, inp, outp = BITS[self.index]
        v = hw.read_reg("sys_cpld", reg)
        if v is None:
            return None
        return {"present": not v >> prs & 1, "input": bool(v >> inp & 1), "output": bool(v >> outp & 1)}

    def get_name(self):
        return "PSU %d" % self.index

    def get_presence(self):
        b = self._bits()
        return bool(b and b["present"])

    def get_powergood_status(self):
        b = self._bits()
        return bool(b and b["present"] and b["output"])

    def get_input_good(self):
        """Not in PsuBase: the CPLD's input-good bit."""
        b = self._bits()
        return bool(b and b["present"] and b["input"])

    def get_status(self):
        return self.get_powergood_status()

    def get_model(self):
        return "N/A"

    def get_serial(self):
        return "N/A"

    def get_revision(self):
        return "N/A"

    def get_position_in_parent(self):
        return self.index

    def is_replaceable(self):
        return True

    def get_type(self):
        return "AC"                                   # "AC PSU 1/2 status" (vendor diag table)

    def set_status_led(self, color):
        return False

    def get_status_led(self):
        return self.STATUS_LED_COLOR_GREEN if self.get_powergood_status() else self.STATUS_LED_COLOR_OFF
