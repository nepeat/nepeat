"""PSUs: two bays on this 19-inch variant (system CPLD 0x09 bit 0 = 1; four on the
21-inch one), status from the system CPLD, identity and readings from PMBus.

Bits from the vendor sys_cpld.ko (DWARF + emulation, devices/cs8210-32x/
analysis/cpld-diff/sys_cpld_psu_attrs.json; table in cpld-diff.md):
  reg 0x08: PSU1 present b4, PSU2 present b5 (active low);
            input good b2 / b3, output good b0 / b1 (active high)
  reg 0xa8: the same for PSU3 / PSU4 (21-inch variant only; reads 0 here)
Confirmed 2026-11-26 with two Delta TDPS-1500AB-6 B fitted, one fed: 0x08 =
0x4a = both present, PSU2 input + output good, PSU1 neither (its PMBus Vin 0).
PMBus (0x59, mux 0x72 ch1 = PSU1, ch0 = PSU2): cs8210-psu (rust/cs8210) reads the
identity into /run/cs8210-psu/psuN.json and binds the kernel pmbus driver; the
readings here come from that hwmon device. No LEDs written (the CPLD drives them).
"""
import glob
import json

from sonic_platform_base.psu_base import PsuBase

from . import hw

# index: (register, present bit, input-good bit, output-good bit)
BITS = {1: (0x08, 4, 2, 0), 2: (0x08, 5, 3, 1), 3: (0xa8, 4, 2, 0), 4: (0xa8, 5, 3, 1)}
RUN = "/run/cs8210-psu"


def num_psus():
    """2 on the 19-inch variant (sys CPLD 0x09 bit 0 set), 4 on the 21-inch one."""
    v = hw.read_reg("sys_cpld", 0x09)
    return 4 if v is not None and not v & 1 else 2


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

    def _id(self):
        try:
            with open(hw.path("%s/psu%d.json" % (RUN, self.index))) as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}

    def _hwmon(self, label):
        """A reading of the bound pmbus device by hwmon label (vin, iout1, pout1, temp1...), SI units."""
        bus = self._id().get("bus")
        if bus is None:
            return None
        for lab in glob.glob(hw.path("/sys/bus/i2c/devices/%d-0059/hwmon/hwmon*/*_label" % bus)):
            if hw.read_text(lab[len(hw.ROOT):]) != label:
                continue
            v = hw.read_float(lab[len(hw.ROOT):].replace("_label", "_input"))
            if v is None:
                return None
            kind = lab.rsplit("/", 1)[1].split("_")[0].rstrip("0123456789")
            return v / ({"power": 1e6, "fan": 1.0}.get(kind, 1e3))
        return None

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
        return self._id().get("model") or "N/A"

    def get_serial(self):
        return self._id().get("serial") or "N/A"

    def get_revision(self):
        return self._id().get("revision") or "N/A"

    def get_voltage(self):
        return self._hwmon("vout1")

    def get_current(self):
        return self._hwmon("iout1")

    def get_power(self):
        return self._hwmon("pout1")

    def get_input_voltage(self):
        return self._hwmon("vin")

    def get_input_current(self):
        return self._hwmon("iin")

    def get_temperature(self):
        return self._hwmon("temp1")

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
