from sonic_platform_base.platform_base import PlatformBase

from .chassis import Chassis


class Platform(PlatformBase):
    def __init__(self):
        PlatformBase.__init__(self)
        self._chassis = Chassis()
