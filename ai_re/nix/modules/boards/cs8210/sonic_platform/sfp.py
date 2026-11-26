"""Front-panel cages: 32 x QSFP-DD, every access through cs8210-optics (optics.py),
the daemon that owns the I2C FPGA.

EEPROM as optoe presents it (what sonic_xcvr expects): 0-255 = lower page +
upper page 0; upper page n at 128 + 128 * n (the daemon selects pages, bank
0); an SFP's A2h at 256-511. Writes, LPMode (the FPGA pin) and reset (a
ResetL pulse; the daemon brings the module up again) go to the daemon,
which refuses them for undeclared cages and for the bytes its bring-up owns.
Module bring-up (power class, CMIS application select) is the daemon's: xcvrd
runs with --skip_cmis_mgr.

Two xcvrd DOM-loop habits, per module (until it is removed or reset):
  * the CDB firmware query (page 9Fh writes; these Molex modules answer "enter
    password" and fail) runs once, its answer is kept;
  * VDM statistics: only reported for a module that has page 2Fh (VDM control,
    the freeze), whatever page 01h advertises: the Molex 1064281405 DR4s (CMIS 4.0)
    advertise VDM and have pages 20h.. but refuse a select of 2Fh (the daemon
    answers such a read with an error); a module whose freeze is not confirmed
    within the time is reported as having none too, so xcvrd stops freezing it.
"""
import sys
import time

from sonic_platform_base.sonic_xcvr.sfp_optoe_base import SfpOptoeBase

from . import optics

VDM_DONE_S = 0.9                                    # xcvrd polls for 1 s
VDM_CONTROL = 128 + 128 * 0x2f                      # page 2Fh byte 128, optoe flat

QSFP_IDS = (0x0c, 0x0d, 0x11, 0x18, 0x19, 0x1e)    # SFF-8436/8636 and CMIS


def sideband():
    """cage sideband masks, or None when the daemon / the FPGA cannot be used."""
    return optics.client().sideband()


class Sfp(SfpOptoeBase):
    def __init__(self, port):
        SfpOptoeBase.__init__(self)
        self.port = port
        self._fw_info = None                        # cached CDB firmware answer
        self._vdm_freeze_broken = False
        self._vdm_page = None                       # page 2Fh readable (None: not asked)
        self._vdm_t0 = None

    def invalidate(self):
        self._xcvr_api = None
        self._fw_info = None
        self._vdm_freeze_broken = False
        self._vdm_page = None
        self._vdm_t0 = None

    # ------------------------------------------------------------ xcvrd DOM habits
    def get_transceiver_info_firmware_versions(self):
        if self._fw_info is None:
            self._fw_info = SfpOptoeBase.get_transceiver_info_firmware_versions(self)
        return self._fw_info

    def is_vdm_statistic_supported(self):
        if self._vdm_freeze_broken or not SfpOptoeBase.is_vdm_statistic_supported(self):
            return False
        if self._vdm_page is None:
            self._vdm_page = self.read_eeprom(VDM_CONTROL, 1) is not None
            if not self._vdm_page:
                sys.stderr.write("cs8210 sonic_platform: cage %d: module advertises VDM but has no page 2Fh; "
                                 "VDM statistics off for it\n" % self.port)
        return self._vdm_page

    def freeze_vdm_stats(self):
        self._vdm_t0 = time.monotonic()
        return SfpOptoeBase.freeze_vdm_stats(self)

    def get_vdm_freeze_status(self):
        done = SfpOptoeBase.get_vdm_freeze_status(self)
        if done:
            self._vdm_t0 = None
        elif self._vdm_t0 is not None and time.monotonic() - self._vdm_t0 > VDM_DONE_S:
            if not self._vdm_freeze_broken:
                sys.stderr.write("cs8210 sonic_platform: cage %d: module does not confirm a VDM freeze; "
                                 "VDM statistics off for it\n" % self.port)
            self._vdm_freeze_broken = True
        return done

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
