"""System EEPROM: ONIE TlvInfo on the switch board, 24c64 at 0x56 behind
0x70:2 -> 0x74:0 -> 0x71:7, bound to at24 by the board DT (read-only node),
so this reads the driver's sysfs file, as `cs8210-show eeprom` does. Nothing
writes it.
"""
from sonic_platform_base.sonic_eeprom.eeprom_tlvinfo import TlvInfoDecoder

from . import hw


class Eeprom(TlvInfoDecoder):
    def __init__(self):
        p = hw.syseeprom_path()
        if p is None:
            raise RuntimeError("switch board EEPROM (DT %s) not bound" % hw.SYSEEPROM_NODE)
        TlvInfoDecoder.__init__(self, p, 0, "", True)
        self._info = None

    def get_info(self):
        """{"0x21": "CS8210-32X-UN-00", ...} (cached; the TLV does not change)."""
        if self._info is None:
            e = self.read_eeprom()
            info = {}
            if self.is_valid_tlvinfo_header(e):
                total = (e[9] << 8) | e[10]
                i = self._TLV_INFO_HDR_LEN
                end = min(len(e), self._TLV_INFO_HDR_LEN + total)
                while i + 2 <= end:
                    code, length = e[i], e[i + 1]
                    _, value = self.decoder(None, e[i:i + 2 + length])
                    info["0x%02X" % code] = value
                    if code == self._TLV_CODE_CRC_32:
                        break
                    i += 2 + length
            self._info = info
        return self._info
