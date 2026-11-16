"""SONiC platform API 2.0 for the Celestica CS8210-32X (NixOS, nix/modules/boards/cs8210).

READ ONLY in this phase: nothing here writes an I2C device, a CPLD, a fan
PWM, a cage's LPMode/ResetL line or a module register other than the CMIS /
SFF-8636 page select (byte 127, restored to 0) a paged EEPROM read needs.
fand owns the fans; no LEDs are driven.

  object             read from                                               module
  14 fan modules     fand's state file (top1..7, bottom1..7; rotors front,   fan.py
  x 2 rotors         rear); fallback: fan CPLD 0x66 tach/PWM registers
  13 board LM75s     fand's sensor values; fallback: hwmon by devicetree node thermal.py
  ASIC die           fand's "asic" sensor (MCP3425); fallback: the file     thermal.py
                     syncd publishes (SAI_OPENBCM_TEMP_FILE)
  4 PSU slots        system CPLD 0x60 regs 0x08 (PSU 1/2), 0xa8 (PSU 3/4)   psu.py
  32 QSFP-DD cages   I2C FPGA (PCI 1d0f:8571) BAR0: presence, ResetL, LPMode  sfp.py, fpga.py
                     bits; module EEPROM through its I2C engines
  syseeprom          ONIE TlvInfo, at24 at 0x56 on 0x71 channel 7            eeprom.py

Paths and environment: hw.py. CS8210_PLATFORM_ROOT=<dir> runs everything on
a fake board (fake.py, built by fake_board.py).
"""
from . import platform  # noqa: F401  (thermalctld: sonic_platform.platform.Platform())
