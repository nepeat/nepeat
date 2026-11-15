"""PSUs: presence and power-good from CPLD#1 register 0x02.

Bit layout from the public siblings (sonic-buildimage, AS7712-32X and
AS7716-32X): PSU1 present = bit 1 clear, PSU2 present = bit 0 clear (active
low); power good = bits 3+5 (PSU1) / 2+4 (PSU2) all set (AS7712 PDDF
pddf-device.json masks 0x28 / 0x14; the kernel drivers accton_as77{12,16}_32x_psu.c
use bits 3 / 2 alone). The AS7760's diag kernel reads the same register
(qsfp-control.md); the mapping on this board is unverified until the PSU test
in devices/edge-core-7760-32x/sonic-platform.md. No PMBus: the PSUs' own
I2C devices are not in the AS7760 device tree, so model, serial, voltage,
current and power are not available. LEDs are not driven.
"""
from sonic_platform_base.psu_base import PsuBase

from . import hw

# index: (present bit (active low), power-good mask)
BITS = {1: (1, 0x28), 2: (0, 0x14)}


class Psu(PsuBase):
    def __init__(self, index):
        PsuBase.__init__(self)
        self.index = index

    def _reg(self):
        try:
            return hw.i2c().cpld_read(hw.PSU_STATUS)
        except (OSError, RuntimeError):
            return None

    def get_name(self):
        return "PSU %d" % self.index

    def get_presence(self):
        v = self._reg()
        return v is not None and not (v >> BITS[self.index][0]) & 1

    def get_powergood_status(self):
        v = self._reg()
        if v is None or (v >> BITS[self.index][0]) & 1:
            return False
        mask = BITS[self.index][1]
        return v & mask == mask

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
        return "AC"                                        # 7760-32X-A-AC-F (syseeprom)

    def set_status_led(self, color):
        return False

    def get_status_led(self):
        return self.STATUS_LED_COLOR_GREEN if self.get_powergood_status() else self.STATUS_LED_COLOR_OFF
