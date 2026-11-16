"""Front-panel cages: 32 x QSFP-DD through the I2C FPGA (fpga.py). Read only.

EEPROM as optoe presents it (what sonic_xcvr expects): 0-255 = lower page +
upper page 0 at 0x50; for QSFP/CMIS upper page n at n*128+128 (page select,
byte 127, under cs8210-qsfp's lock, page 0 restored after); for an SFP (in a
QSA) A2h (0x51) at 256-511. Banked pages: bank 0 only (byte 126 untouched).

Not implemented in this phase (each returns False / refuses): write_eeprom
(so tx_disable, power override, CMIS application select), set_lpmode (the
FPGA LPMode line and the module's software low-power bit), reset (ResetL).
xcvrd's CMIS manager therefore cannot bring a CMIS module's data path up:
run xcvrd with --skip_cmis_mgr. A module held in reset (the FPGA's power-up
state) does not answer: present, EEPROM unreadable, get_reset_status True.
"""
from sonic_platform_base.sonic_xcvr.sfp_optoe_base import SfpOptoeBase

from . import fpga as F

QSFP_IDS = (0x0c, 0x0d, 0x11, 0x18, 0x19, 0x1e)    # SFF-8436/8636 and CMIS
SFP_ID = 0x03


def sideband():
    """fpga sideband masks, or None when the FPGA cannot be used."""
    f = F.fpga()
    try:
        sb = f.sideband()
        f.problem = None
        return sb
    except (OSError, ValueError) as e:
        f.problem = str(e)
        return None


class Sfp(SfpOptoeBase):
    def __init__(self, port):
        SfpOptoeBase.__init__(self)
        self.port = port
        self._kind = None                   # (identifier, flat memory) while plugged

    def invalidate(self):
        self._kind = None
        self._xcvr_api = None

    def _bit(self, name):
        sb = sideband()
        return None if sb is None else bool(sb[name] >> (self.port - 1) & 1)

    # ------------------------------------------------------------ sideband
    def get_presence(self):
        p = bool(self._bit("present"))
        if not p:
            self._kind = None
        return p

    def get_reset_status(self):
        return bool(self._bit("reset"))

    def get_lpmode_via_pin(self):
        return bool(self._bit("lpmode"))

    def reset(self):
        return False                        # read-only phase: ResetL is not driven

    def set_lpmode(self, lpmode):
        return False                        # read-only phase

    def set_lpmode_via_pin(self, lpmode):
        return False

    def get_lpmode(self):
        """Module view (CMIS LowPwr / SFF-8636 power override) via the EEPROM;
        False for an SFP or when unreadable."""
        if not self.get_presence():
            return False
        try:
            ident, _ = self._module_kind()
            if ident not in QSFP_IDS:
                return False
            return bool(SfpOptoeBase.get_lpmode(self))
        except (OSError, ValueError, TypeError, KeyError, AttributeError, NotImplementedError):
            return False

    # ------------------------------------------------------------ EEPROM
    def get_eeprom_path(self):
        return None                         # no optoe driver; read_eeprom talks to the FPGA

    def _module_kind(self):
        if self._kind is None:
            f = F.fpga()
            with f.locked():
                ident = f.i2c_read(self.port, 0x50, 0, 1)[0]
                flat = True
                if ident in QSFP_IDS:
                    b2 = f.i2c_read(self.port, 0x50, 2, 1)[0]
                    flat = bool(b2 & (0x80 if ident >= 0x18 else 0x04))
            self._kind = (ident, flat)
        return self._kind

    def _chunks(self, offset, num_bytes):
        """(addr, page or None, byte offset, length) per page touched."""
        ident, flat = self._module_kind()
        end = offset + num_bytes
        while offset < end:
            n = min(end, (offset // 128 + 1) * 128) - offset
            if ident not in QSFP_IDS:
                if offset >= 512 or (offset >= 256 and ident != SFP_ID):
                    raise OSError("offset %d beyond the module's memory" % offset)
                yield (0x51, None, offset - 256, n) if offset >= 256 else (0x50, None, offset, n)
            elif offset < 128:
                yield 0x50, None, offset, n
            elif offset < 256:
                yield 0x50, (None if flat else 0), offset, n
            else:
                if flat:
                    raise OSError("flat-memory module has no page %d" % (offset // 128 - 1))
                yield 0x50, offset // 128 - 1, 128 + offset % 128, n
            offset += n

    def read_eeprom(self, offset, num_bytes):
        if num_bytes <= 0 or not self.get_presence():
            return None
        try:
            chunks = list(self._chunks(offset, num_bytes))
            f = F.fpga()
            out = bytearray()
            with f.locked():
                for addr, page, off, n in chunks:
                    if page is not None:
                        f.select_page(self.port, page)
                    try:
                        out += f.i2c_read(self.port, addr, off, n)
                    finally:
                        if page:
                            f.select_page(self.port, 0)
            return out
        except (OSError, ValueError):
            self._kind = None
            return None

    def write_eeprom(self, offset, num_bytes, write_buffer):
        return False                        # read-only phase

    # ------------------------------------------------------------ DeviceBase
    def get_name(self):
        return "QSFP-DD%d" % self.port

    def get_status(self):
        return self.get_presence() and not self.get_reset_status()

    def get_position_in_parent(self):
        return self.port

    def is_replaceable(self):
        return True
