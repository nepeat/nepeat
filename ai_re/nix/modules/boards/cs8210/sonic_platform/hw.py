"""Hardware access for the CS8210-32X sonic_platform: every path, the I2C
reads, fand's state. Read only: the only I2C operation here is an SMBus
read-byte-data (what `i2cget -y <bus> <addr> <reg>` does).

Buses are found by devicetree node, as fand finds them (Linux numbers mux
channels in probe order): board DT nix/nixos/module/cs8210/kernel/dts/amazon/
alpine-v3-cs8210.dts in the monorepo; topology devices/cs8210-32x/board-control.md.

  system CPLD 0x60     adapter .../i2c-mux@74/i2c@0/i2c-mux@71/i2c@2
  fan CPLD 0x66        adapter .../i2c-mux@74/i2c@2 (top), .../i2c-mux@74/i2c@3 (bottom)
  syseeprom 0x56       at24 device .../i2c-mux@71/i2c@7/eeprom@56 (sysfs "eeprom", read-only)
  LM75s                hwmon whose device/of_node ends in the node of thermal.py's table

Environment (all optional):
  CS8210_PLATFORM_ROOT     fake board root: prefixes every path, I2C and the
                           cages come from fake.py (fake_board.py builds it)
  CS8210_FAND_STATE_FILE   fand's runtime.state_file (default /run/fand/state.json)
  CS8210_FAND_MAX_AGE      seconds after which that file counts as stale (30)
  CS8210_ASIC_TEMP_FILE    ASIC temperature file syncd writes (SAI_OPENBCM_TEMP_FILE;
                           default /run/asic/temperature)
  CS8210_OPTICS_SOCKET     cs8210-optics' socket (optics.py)
"""
import ctypes
import errno
import fcntl
import glob
import json
import os
import time

ROOT = os.environ.get("CS8210_PLATFORM_ROOT", "")
FAND_STATE_FILE = os.environ.get("CS8210_FAND_STATE_FILE", "/run/fand/state.json")
FAND_MAX_AGE = float(os.environ.get("CS8210_FAND_MAX_AGE", "30"))
ASIC_TEMP_FILE = os.environ.get("CS8210_ASIC_TEMP_FILE", "/run/asic/temperature")

SYS_CPLD, FAN_CPLD = 0x60, 0x66
I2C_DEVICES = {                                   # name: (adapter of_node tail, address)
    "sys_cpld": ("i2c-mux@74/i2c@0/i2c-mux@71/i2c@2", SYS_CPLD),
    "fan_top": ("i2c-mux@74/i2c@2", FAN_CPLD),
    "fan_bottom": ("i2c-mux@74/i2c@3", FAN_CPLD),
}
SYSEEPROM_NODE = "i2c-mux@71/i2c@7/eeprom@56"


def path(p):
    return ROOT + p


def read_text(p):
    try:
        with open(path(p)) as f:
            return f.read().strip()
    except OSError:
        return None


def read_float(p):
    t = read_text(p)
    try:
        return float(t) if t is not None else None
    except ValueError:
        return None


def _node_matches(link, tail):
    try:
        real = os.path.realpath(link, strict=True)
    except OSError:
        return False
    return real == tail or real.endswith("/" + tail)


def hwmon_by_node(tail):
    """sysfs dir (without ROOT) of the hwmon device whose DT node ends in tail."""
    for d in sorted(glob.glob(path("/sys/class/hwmon/hwmon*"))):
        if _node_matches(d + "/device/of_node", tail):
            return d[len(ROOT):]
    return None


def adapter_by_node(tail):
    """Linux bus number of the I2C adapter (mux channel) at DT node tail."""
    for d in sorted(glob.glob(path("/sys/bus/i2c/devices/i2c-*"))):
        if _node_matches(d + "/of_node", tail):
            n = os.path.basename(d)[4:]
            if n.isdigit():
                return int(n)
    return None


def syseeprom_path():
    """The at24 sysfs file of the switch board's TlvInfo EEPROM, or None."""
    for d in sorted(glob.glob(path("/sys/bus/i2c/devices/*-0056"))):
        if _node_matches(d + "/of_node", SYSEEPROM_NODE) and os.path.exists(d + "/eeprom"):
            return d + "/eeprom"
    return None


# ---------------------------------------------------------------- fand

_fand_cache = [None, None]          # [(mtime_ns, size), parsed]


def fand_state():
    """fand's status (dict), or None when there is no file, it does not
    parse, or it is older than CS8210_FAND_MAX_AGE (fand stopped)."""
    p = path(FAND_STATE_FILE)
    try:
        st = os.stat(p)
    except OSError:
        return None
    if time.time() - st.st_mtime > FAND_MAX_AGE:
        return None
    key = (st.st_mtime_ns, st.st_size)
    if _fand_cache[0] != key:
        try:
            with open(p) as f:
                _fand_cache[1] = json.load(f)
        except (OSError, ValueError):
            return None
        _fand_cache[0] = key
    return _fand_cache[1]


def fand_sensor(name):
    """fand's current value of sensor name (deg C), or None (no fand, stale
    file, sensor missing, in error or without a value)."""
    st = fand_state()
    s = ((st or {}).get("sensors") or {}).get(name)
    if not s or s.get("error") or s.get("value") is None:
        return None
    return float(s["value"])


# ---------------------------------------------------------------- I2C

I2C_SLAVE, I2C_SMBUS = 0x0703, 0x0720
I2C_SMBUS_READ, I2C_SMBUS_BYTE_DATA = 1, 2


class _SmbusData(ctypes.Union):
    _fields_ = [("byte", ctypes.c_uint8), ("word", ctypes.c_uint16), ("block", ctypes.c_uint8 * 34)]


class _SmbusArgs(ctypes.Structure):
    _fields_ = [("read_write", ctypes.c_uint8), ("command", ctypes.c_uint8),
                ("size", ctypes.c_uint32), ("data", ctypes.POINTER(_SmbusData))]


class RealI2c:
    """i2c-dev, SMBus read byte data only. I2C_SLAVE (not _FORCE): an
    address a kernel driver owns is refused (EBUSY), never shared."""

    def read_byte(self, dev, reg):
        tail, addr = I2C_DEVICES[dev]
        bus = adapter_by_node(tail)
        if bus is None:
            raise OSError(errno.ENODEV, "no I2C adapter at DT node %s" % tail)
        fd = os.open("/dev/i2c-%d" % bus, os.O_RDWR)
        try:
            fcntl.ioctl(fd, I2C_SLAVE, addr)
            data = _SmbusData()
            args = _SmbusArgs(I2C_SMBUS_READ, reg, I2C_SMBUS_BYTE_DATA, ctypes.pointer(data))
            fcntl.ioctl(fd, I2C_SMBUS, args)
            return data.byte
        finally:
            os.close(fd)


_i2c = None


def i2c():
    global _i2c
    if _i2c is None:
        if ROOT:
            from .fake import FakeI2c
            _i2c = FakeI2c(ROOT)
        else:
            _i2c = RealI2c()
    return _i2c


def read_reg(dev, reg):
    """One register byte, or None on any error."""
    try:
        return i2c().read_byte(dev, reg)
    except (OSError, ValueError):
        return None
