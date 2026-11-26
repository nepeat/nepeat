"""Front-panel cages: 32 x QSFP-DD, every access through cs8210-optics (optics.py),
the daemon that owns the I2C FPGA.

EEPROM as optoe presents it (what sonic_xcvr expects): 0-255 = lower page +
upper page 0; upper page n at 128 + 128 * n (the daemon selects pages, bank
0); an SFP's A2h at 256-511. Writes, LPMode (the FPGA pin) and reset (a
ResetL pulse; the daemon brings the module up again) go to the daemon,
which refuses them for undeclared cages. Module bring-up (power class, CMIS
application select) is the daemon's: xcvrd runs with --skip_cmis_mgr.
"""
from sonic_platform_base.sonic_xcvr.sfp_optoe_base import SfpOptoeBase

from . import optics

QSFP_IDS = (0x0c, 0x0d, 0x11, 0x18, 0x19, 0x1e)    # SFF-8436/8636 and CMIS


def sideband():
    """cage sideband masks, or None when the daemon / the FPGA cannot be used."""
    return optics.client().sideband()


class Sfp(SfpOptoeBase):
    def __init__(self, port):
        SfpOptoeBase.__init__(self)
        self.port = port

    def invalidate(self):
        self._xcvr_api = None

    def _bit(self, name):
        sb = sideband()
        return None if sb is None else bool(sb[name] >> (self.port - 1) & 1)

    # ------------------------------------------------------------ sideband
    def get_presence(self):
        return bool(self._bit("present"))

    def get_reset_status(self):
        return bool(self._bit("reset"))

    def get_lpmode_via_pin(self):
        return bool(self._bit("lpmode"))

    def reset(self):
        try:
            optics.client().reset(self.port)
            self.invalidate()
            return True
        except OSError:
            return False

    def set_lpmode_via_pin(self, lpmode):
        try:
            optics.client().set_lpmode(self.port, lpmode)
            return True
        except OSError:
            return False

    def set_lpmode(self, lpmode):
        return self.set_lpmode_via_pin(lpmode)

    def get_lpmode(self):
        """The LPMode pin, or the module's own view (CMIS LowPwr / SFF-8636 power
        override) if the pin is deasserted; False for an SFP or when unreadable."""
        if not self.get_presence():
            return False
        if self.get_lpmode_via_pin():
            return True
        try:
            ident = self.read_eeprom(0, 1)
            if not ident or ident[0] not in QSFP_IDS:
                return False
            return bool(SfpOptoeBase.get_lpmode(self))
        except (OSError, ValueError, TypeError, KeyError, AttributeError, NotImplementedError):
            return False

    # ------------------------------------------------------------ EEPROM
    def get_eeprom_path(self):
        return None                         # no optoe driver: read_eeprom asks the daemon

    def read_eeprom(self, offset, num_bytes):
        if num_bytes <= 0:
            return None
        try:
            return bytearray(optics.client().read(self.port, offset, num_bytes))
        except (OSError, ValueError):
            return None

    def write_eeprom(self, offset, num_bytes, write_buffer):
        try:
            optics.client().write(self.port, offset, bytes(write_buffer[:num_bytes]))
            return True
        except (OSError, ValueError):
            return False

    # ------------------------------------------------------------ DeviceBase
    def get_name(self):
        return "QSFP-DD%d" % self.port

    def get_status(self):
        return self.get_presence() and not self.get_reset_status()

    def get_position_in_parent(self):
        return self.port

    def is_replaceable(self):
        return True
