"""System EEPROM: ONIE TlvInfo at offset 0 of the NOR partition "hw-info".

The AS7760 has no ID EEPROM on I2C (the vendor DT's 24c256 at 0x50 never
answers); U-Boot and ONIE read the TLV from NOR (u-boot-port.md). Opened
read-only; nothing writes the partition.
"""
from sonic_platform_base.sonic_eeprom.eeprom_tlvinfo import TlvInfoDecoder

from . import hw


class Eeprom(TlvInfoDecoder):
    def __init__(self):
        dev = hw.mtd_device("hw-info")
        if dev is None:
            raise RuntimeError("no MTD partition named hw-info")
        TlvInfoDecoder.__init__(self, dev, 0, "", True)
        self._info = None

    def get_info(self):
        """{"0x21": "7760-32X-A-AC-F", ...} (cached; the TLV does not change)."""
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
