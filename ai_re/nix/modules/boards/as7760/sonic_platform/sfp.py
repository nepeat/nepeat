"""Front-panel cages (32 x QSFP28): presence/reset from CPLD#1, EEPROM over I2C.

EEPROM layout as optoe presents it (what sonic_xcvr expects): bytes 0-255 =
lower page + upper page 0 (0x50); for QSFP/CMIS upper page n at
n*128+128 (page select, byte 127, under the port lock, page 0 restored
after); for SFP-type modules (QSFA adapters) 0x51 (A2h) at 256-511.
Writes: only SFF-8636 control bytes 86-126 and SFP A2h 110-127 (TX disable,
power/low-power, rate select, masks); identity and threshold pages are never
written. Low power is software (SFF-8636 byte 93), as as7760-qsfp --lpmode:
the board has no LPMode line (qsfp-control.md).
"""
import time

from sonic_platform_base.sonic_xcvr.sfp_optoe_base import SfpOptoeBase

from . import hw

QSFP_IDS = (0x0c, 0x0d, 0x11, 0x18, 0x19, 0x1e)    # SFF-8436/8636 and CMIS
SFF8636_IDS = (0x0c, 0x0d, 0x11)
SFP_ID = 0x03
PAGE_SELECT = 127


class Sfp(SfpOptoeBase):
    def __init__(self, port):
        SfpOptoeBase.__init__(self)
        self.port = port
        self._kind = None                    # (identifier, flat memory) while plugged

    def invalidate(self):
        self._kind = None
        self._xcvr_api = None

    # ------------------------------------------------------------ CPLD
    def _cpld_bit(self, base):
        v = hw.i2c().cpld_read(base + (self.port - 1) // 8)
        return not (v >> ((self.port - 1) % 8)) & 1

    def get_presence(self):
        try:
            present = self._cpld_bit(hw.MODPRS_L)
        except (OSError, RuntimeError):
            return False
        if not present:
            self._kind = None
        return present

    def get_reset_status(self):
        return self._cpld_bit(hw.RESET_L)

    def reset(self):
        """Pulse ResetL (read-modify-write, the other 7 cages untouched)."""
        try:
            hw.i2c().cpld_reset(self.port)
        except (OSError, RuntimeError):
            return False
        self.invalidate()
        time.sleep(2)                        # SFF-8679 t_init before the next access
        return True

    # ------------------------------------------------------------ EEPROM
    def get_eeprom_path(self):
        return None                          # no optoe driver; read_eeprom talks I2C

    def _module_kind(self):
        if self._kind is None:
            i2c = hw.i2c()
            ident = i2c.module_read(self.port, 0x50, 0, 1)[0]
            flat = True
            if ident in QSFP_IDS:
                b2 = i2c.module_read(self.port, 0x50, 2, 1)[0]
                flat = bool(b2 & (0x80 if ident >= 0x18 else 0x04))
            self._kind = (ident, flat)
        return self._kind

    def _chunks(self, offset, num_bytes):
        """(addr, page or None, byte offset, length) per page touched."""
        ident, flat = self._module_kind()
        end = offset + num_bytes
        while offset < end:
            n = min(end, (offset // 128 + 1) * 128) - offset
            if ident == SFP_ID or ident not in QSFP_IDS:
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

    def _xfer(self, offset, num_bytes, data=None):
        i2c = hw.i2c()
        out = bytearray()
        with i2c.module_lock(self.port):
            for addr, page, off, n in self._chunks(offset, num_bytes):
                if page is not None:
                    i2c.module_write(self.port, 0x50, PAGE_SELECT, bytes([page]))
                try:
                    if data is None:
                        out += i2c.module_read(self.port, addr, off, n)
                    else:
                        i2c.module_write(self.port, addr, off, bytes(data[len(out):len(out) + n]))
                        out += bytes(n)
                finally:
                    if page:
                        i2c.module_write(self.port, 0x50, PAGE_SELECT, b"\x00")
        return out

    def read_eeprom(self, offset, num_bytes):
        if num_bytes <= 0 or not self.get_presence():
            return None
        try:
            return bytearray(self._xfer(offset, num_bytes))
        except (OSError, RuntimeError):
            self._kind = None
            return None

    def _writable(self, offset, num_bytes):
        try:
            ident, _ = self._module_kind()
        except OSError:
            return False
        last = offset + num_bytes - 1
        if ident in SFF8636_IDS:
            return 86 <= offset and last <= 126
        if ident == SFP_ID:
            return 256 + 110 <= offset and last <= 256 + 127
        return False

    def write_eeprom(self, offset, num_bytes, write_buffer):
        if num_bytes <= 0 or not self.get_presence() or not self._writable(offset, num_bytes):
            return False
        try:
            self._xfer(offset, num_bytes, write_buffer)
            return True
        except (OSError, RuntimeError):
            self._kind = None
            return False

    # ------------------------------------------------------------ low power
    def get_lpmode(self):
        """SFF-8636 byte 93: power override (bit 0) with low power (bit 1)."""
        if not self.get_presence():
            return False
        try:
            ident, _ = self._module_kind()
        except OSError:
            return False
        if ident not in SFF8636_IDS:
            return False if ident == SFP_ID else SfpOptoeBase.get_lpmode(self)
        b = self.read_eeprom(93, 1)
        return bool(b) and b[0] & 0x03 == 0x03

    def set_lpmode(self, lpmode):
        if not self.get_presence():
            return False
        try:
            ident, _ = self._module_kind()
        except OSError:
            return False
        if ident not in SFF8636_IDS:
            return False if ident == SFP_ID else SfpOptoeBase.set_lpmode(self, lpmode)
        cur = self.read_eeprom(93, 1)
        if not cur:
            return False
        new = (cur[0] & ~0x03) | 0x01 | (0x02 if lpmode else 0)
        if not self.write_eeprom(93, 1, bytearray([new])):
            return False
        time.sleep(0.05)
        back = self.read_eeprom(93, 1)
        return bool(back) and back[0] & 0x03 == new & 0x03

    # ------------------------------------------------------------ DeviceBase
    def get_name(self):
        return "QSFP%d" % self.port

    def get_status(self):
        return self.get_presence() and not self.get_reset_status()

    def get_position_in_parent(self):
        return self.port

    def is_replaceable(self):
        return True
