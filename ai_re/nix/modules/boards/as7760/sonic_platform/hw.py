"""Hardware access for the AS7760-32X sonic_platform (one place for every path).

Sources (devices/edge-core-7760-32x/{thermal,qsfp-control}.md):
  fans        hwmon "as7760_fan" (fan<N>_input/_fault/_label, pwm1 read only)
              + fand's state file (/run/fand/state.json: duty, expected rpm)
  board temps hwmon "lm75" at I2C 0x48..0x4b (PCA9548 0x76 channel 1)
  ASIC temp   /run/asic/temperature (openbcm-asic-temp or syncd's libsai)
  CPLD#1      I2C 0x60 behind PCA9548 0x76 channel 2: 0x02 PSU status,
              0x04-0x07 ResetL, 0x30-0x33 ModPrsL (port p: base + (p-1)//8,
              bit (p-1)%8, active low)
  modules     I2C 0x50/0x51 on the per-port mux channel (as7760-qsfp's bus_for)
  syseeprom   ONIE TlvInfo at offset 0 of the NOR partition "hw-info" (/dev/mtdN)

AS7760_PLATFORM_ROOT=<dir> prefixes every path and replaces the I2C layer
with files (<root>/i2c/cpld1.bin, <root>/i2c/port<N>.bin): the VM test and the
unit tests run the same code on a fake board.
"""
import contextlib
import errno
import fcntl
import glob
import json
import os

ROOT = os.environ.get("AS7760_PLATFORM_ROOT", "")
ASIC_TEMP_FILE = os.environ.get("AS7760_ASIC_TEMP_FILE", "/run/asic/temperature")
FAND_STATE_FILE = os.environ.get("AS7760_FAND_STATE_FILE", "/run/fand/state.json")

CPLD1 = 0x60
PSU_STATUS, RESET_L, INT_L, MODPRS_L = 0x02, 0x04, 0x10, 0x30
NUM_PORTS = 32


def path(p):
    return ROOT + p


def read_text(p):
    try:
        with open(path(p)) as f:
            return f.read().strip()
    except OSError:
        return None


def read_int(p):
    t = read_text(p)
    try:
        return int(t) if t is not None else None
    except ValueError:
        return None


def read_float(p):
    t = read_text(p)
    try:
        return float(t) if t is not None else None
    except ValueError:
        return None


def hwmon(name, i2c_addr=None):
    """sysfs dirs (without ROOT) of hwmon devices called name, optionally only
    the one whose I2C client is at i2c_addr (device -> .../<bus>-00<addr>)."""
    found = []
    for d in sorted(glob.glob(path("/sys/class/hwmon/hwmon*"))):
        if read_text(d[len(ROOT):] + "/name") != name:
            continue
        if i2c_addr is not None:
            dev = os.path.basename(os.path.realpath(d + "/device"))
            if not dev.endswith("-%04x" % i2c_addr):
                continue
        found.append(d[len(ROOT):])
    return found


def fand_state():
    """fand's status (dict) or None when fand is not running / no file."""
    try:
        with open(path(FAND_STATE_FILE)) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def mtd_device(label):
    for d in sorted(glob.glob(path("/sys/class/mtd/mtd*"))):
        n = os.path.basename(d)
        if n.endswith("ro") or not n[3:].isdigit():
            continue
        if read_text("/sys/class/mtd/%s/name" % n) == label:
            return path("/dev/" + n)
    return None


# ---------------------------------------------------------------- I2C

class RealI2c:
    """The real board, through as7760-qsfp's code (nix/packages/as7760-qsfp)."""

    def __init__(self):
        import as7760_qsfp
        self.q = as7760_qsfp
        self._cpld_bus = None
        self._buses = {}

    def cpld_bus(self):
        if self._cpld_bus is None:
            self._cpld_bus = self.q.cpld_bus()
        return self._cpld_bus

    def cpld_read(self, reg):
        return self.q.read_atomic(self.cpld_bus(), CPLD1, reg, 1)[0]

    def cpld_reset(self, port):
        self.q.cpld_reset(self.cpld_bus(), port)

    def port_bus(self, port):
        if port not in self._buses:
            self._buses[port] = self.q.bus_for(port)
        return self._buses[port]

    def module_read(self, port, addr, offset, length):
        return self.q.read_atomic(self.port_bus(port), addr, offset, length)

    def module_write(self, port, addr, offset, data):
        self.q.write_atomic(self.port_bus(port), addr, offset, data)

    @contextlib.contextmanager
    def module_lock(self, port):
        with self.q.bus_lock(self.port_bus(port)):
            yield


class FakeI2c:
    """Files instead of I2C: <root>/i2c/cpld1.bin (CPLD#1 registers; ModPrsL
    is derived from which port<N>.bin exist) and <root>/i2c/port<N>.bin, an
    optoe-style linear image: 0x50 bytes 0-255, then upper pages 1, 2, 3...
    (QSFP: page select in byte 127); for SFP-type modules 0x51 (A2h) is bytes
    256-511. A missing file is a missing device (ENXIO)."""

    def __init__(self, root):
        self.dir = root + "/i2c"

    def _cpld(self):
        try:
            with open(self.dir + "/cpld1.bin", "rb") as f:
                regs = bytearray(f.read().ljust(256, b"\xff"))
        except OSError:
            raise OSError(errno.ENXIO, "fake CPLD#1 missing")
        for i in range(4):
            v = 0xff
            for b in range(8):
                if os.path.exists("%s/port%d.bin" % (self.dir, i * 8 + b + 1)):
                    v &= ~(1 << b)
            regs[MODPRS_L + i] = v
        return regs

    def cpld_read(self, reg):
        return self._cpld()[reg]

    def cpld_reset(self, port):
        reg, bit = RESET_L + (port - 1) // 8, 1 << ((port - 1) % 8)
        if not self._cpld()[reg] & bit:
            raise RuntimeError("port %d is already held in reset" % port)
        with open(self.dir + "/resets.log", "a") as f:
            f.write("%d\n" % port)

    def _image(self, port):
        try:
            with open("%s/port%d.bin" % (self.dir, port), "rb") as f:
                return bytearray(f.read())
        except OSError:
            raise OSError(errno.ENXIO, "no module in port %d" % port)

    def _linear(self, img, addr, offset):
        if addr == 0x51:
            return 256 + offset
        if offset < 128 or img[0] == 0x03:
            return offset
        return offset + 128 * img[127]          # selected upper page

    def module_read(self, port, addr, offset, length):
        img = self._image(port)
        start = self._linear(img, addr, offset)
        if start + length > len(img):
            raise OSError(errno.EIO, "fake read past the image")
        return bytes(img[start:start + length])

    def module_write(self, port, addr, offset, data):
        img = self._image(port)
        start = self._linear(img, addr, offset)
        img[start:start + len(data)] = data
        with open("%s/port%d.bin" % (self.dir, port), "wb") as f:
            f.write(img)

    @contextlib.contextmanager
    def module_lock(self, port):
        yield


_i2c = None


def i2c():
    global _i2c
    if _i2c is None:
        _i2c = FakeI2c(ROOT) if ROOT and os.path.isdir(ROOT + "/i2c") else RealI2c()
    return _i2c


def cpld_bits(base):
    """{port: asserted} for one active-low 32-line group of CPLD#1."""
    regs = [i2c().cpld_read(base + i) for i in range(4)]
    return {p: not (regs[(p - 1) // 8] >> ((p - 1) % 8)) & 1 for p in range(1, NUM_PORTS + 1)}


def lock_file(name):
    """flock()ed file under /run (best effort: no lock when /run is read-only)."""
    @contextlib.contextmanager
    def held():
        try:
            f = open(path("/run/" + name), "w")
        except OSError:
            yield
            return
        with f:
            fcntl.flock(f, fcntl.LOCK_EX)
            yield
    return held()
