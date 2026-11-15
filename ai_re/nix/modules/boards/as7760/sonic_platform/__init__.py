"""SONiC platform API 2.0 for the Accton/Edgecore AS7760-32X (NixOS, nix/modules/boards/as7760).

Read-only towards fans (fand owns fan control) and LEDs (the Broadcom LED
processor and CPLD#1 0x41 are not touched). Hardware access: hw.py.
"""
from . import platform  # noqa: F401  (thermalctld: sonic_platform.platform.Platform())
