"""AS7760-32X chassis: 6 fan drawers (12 rotors), 5 thermals (thermal.py; a
derivative board has its own sensor list), 2 PSUs,
32 QSFP28 cages (physical index = SONiC port_config index = cage 1..32),
ONIE syseeprom.

No thermal manager: get_thermal_manager() returns None, so thermalctld only
publishes FAN_INFO/TEMPERATURE_INFO and never runs a fan policy (fand owns
the fans). No LEDs are driven (system LED: CPLD#1 0x41 is left alone).
"""
import time

from sonic_platform_base.chassis_base import ChassisBase

from . import hw
from .eeprom import Eeprom
from .fan import FanDrawer
from .psu import Psu
from .sfp import Sfp
from .thermal import AsicThermal, BoardThermal, sensors

POLL = 0.5                                # presence poll period of get_change_event (s)


class Chassis(ChassisBase):
    def __init__(self):
        ChassisBase.__init__(self)
        self._fan_drawer_list = [FanDrawer(i) for i in range(1, 7)]
        self._fan_list = [f for d in self._fan_drawer_list for f in d.get_all_fans()]
        n = len(sensors())
        self._thermal_list = [BoardThermal(i) for i in range(1, n + 1)] + [AsicThermal(n + 1)]
        self._psu_list = [Psu(1), Psu(2)]
        self._sfp_list = [None] + [Sfp(p) for p in range(1, hw.NUM_PORTS + 1)]
        self._eeprom = None
        self._present = self._presence()

    # ------------------------------------------------------------ identity
    def get_eeprom(self):
        if self._eeprom is None:
            self._eeprom = Eeprom()
        return self._eeprom

    def _tlv(self, code):
        try:
            return self.get_eeprom().get_info().get(code, "N/A")
        except (OSError, RuntimeError):
            return "N/A"

    def get_system_eeprom_info(self):
        return self.get_eeprom().get_info()

    def get_name(self):
        return self._tlv("0x21")

    def get_model(self):
        return self._tlv("0x22")

    def get_serial(self):
        return self._tlv("0x23")

    def get_base_mac(self):
        return self._tlv("0x24")

    def get_revision(self):
        return self._tlv("0x26")

    def get_presence(self):
        return True

    def get_status(self):
        return True

    def is_replaceable(self):
        return False

    def get_position_in_parent(self):
        return -1

    def get_thermal_manager(self):
        return None                         # monitor only, see the module docstring

    def is_liquid_cooled(self):
        return False

    def set_status_led(self, color):
        return False

    def get_reboot_cause(self):
        return self.REBOOT_CAUSE_NON_HARDWARE, None

    # ------------------------------------------------------------ cages
    def get_port_or_cage_type(self, index):
        from sonic_platform_base.sfp_base import SfpBase
        return SfpBase.SFP_PORT_TYPE_BIT_QSFP28

    def _presence(self):
        try:
            return hw.cpld_bits(hw.MODPRS_L)
        except (OSError, RuntimeError):
            return None

    def get_change_event(self, timeout=0):
        """Poll CPLD#1 ModPrsL; {'sfp': {'<port>': '1' inserted / '0' removed}}."""
        deadline = None if not timeout else time.monotonic() + timeout / 1000.0
        while True:
            now = self._presence()
            if now is not None:
                if self._present is None:
                    self._present = now
                changed = {str(p): "1" if now[p] else "0" for p in now if now[p] != self._present[p]}
                if changed:
                    self._present = now
                    for p in changed:
                        self._sfp_list[int(p)].invalidate()
                    return True, {"sfp": changed}
            if deadline is not None and time.monotonic() >= deadline:
                return True, {"sfp": {}}
            time.sleep(POLL if deadline is None else max(0, min(POLL, deadline - time.monotonic())))
