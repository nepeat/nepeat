# Shared helpers for the cs8210-* tools. Sourced, not executed.
#
# I2C topology (all behind the "i2c-pld" DesignWare controller at 0xfd880000):
#   0x70 ch2 -> 0x74 ch0 -> 0x71 ch2 : system CPLD     0x60
#   0x70 ch2 -> 0x74 ch0 -> 0x71 ch5 : switch LED CPLD 0x64
#   0x70 ch2 -> 0x74 ch0 -> 0x71 ch7 : ONIE TlvInfo EEPROM 0x56
#   0x70 ch2 -> 0x74 ch1 -> 0x73 ch0 : MACsec LED CPLD 0x63
#   0x70 ch2 -> 0x74 ch2             : top fan CPLD    0x66
#   0x70 ch2 -> 0x74 ch3             : bottom fan CPLD 0x66
# Linux bus numbers depend on probe order, so they are resolved through the
# mux channel symlinks in sysfs instead of being hard-coded.

CS8210_I2C_ROOT=${CS8210_I2C_ROOT:-/sys/bus/platform/devices/fd880000.i2c}

# _bus <glob below the root adapter> -> bus number
_bus() {
  local p
  for p in $CS8210_I2C_ROOT/i2c-*/$1; do
    if [ -e "$p" ]; then
      p=$(readlink -f "$p")
      echo "${p##*/i2c-}"
      return 0
    fi
  done
  echo "cs8210: i2c path not found: $1" >&2
  return 1
}

bus_fan_top()    { _bus '*-0070/channel-2/*-0074/channel-2'; }
bus_fan_bottom() { _bus '*-0070/channel-2/*-0074/channel-3'; }
bus_sys_cpld()   { _bus '*-0070/channel-2/*-0074/channel-0/*-0071/channel-2'; }
bus_switchboard_eeprom() { _bus '*-0070/channel-2/*-0074/channel-0/*-0071/channel-7'; }
bus_swled_cpld() { _bus '*-0070/channel-2/*-0074/channel-0/*-0071/channel-5'; }
bus_macled_cpld(){ _bus '*-0070/channel-2/*-0074/channel-1/*-0073/channel-0'; }

FAN_CPLD=0x66
SYS_CPLD=0x60
# Fan CPLD registers (vendor diag cplds.yaml, confirmed on hardware 2026-11-09)
FAN_REG_VERSION=0x00
FAN_REG_WDT=0x0c          # bit0 enable, bits7:1 timeout in 5 s units
FAN_SLOTS="0x20 0x30 0x40 0x50 0x60 0x70 0x80"   # +0 front tach, +1 rear tach, +2 PWM, +4 LED, +6 status
# System CPLD
SYS_REG_VERSION=0x00
SYS_REG_FANMAX=0x05       # bit7 / bit5 CLEAR = force fans to max (inverted)

rd() { i2cget -y "$1" "$2" "$3"; }
wr() { i2cset -y "$1" "$2" "$3" "$4"; }

require_root() {
  if [ "$(id -u)" -ne 0 ]; then
    echo "** $0 needs root (raw I2C access to the CPLDs)." >&2
    exit 1
  fi
}
