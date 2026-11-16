"""The QSFP-DD cages' management plane: the "i2c_accel_fpga" (PCI 1d0f:8571)
BAR0, read path only.

Re-expressed from our cs8210-qsfp (monorepo nix/nixos/module/cs8210/tools/
cs8210-qsfp; protocol: devices/cs8210-32x/reviews/2026-11-10-i2c-fpga-protocol.md,
recovered from the vendor i2c_accel_fpga.ko v0.11): the same registers,
32-bit accesses through memoryview.cast("I") on the resource0 mmap, the same
transaction sequence and polling. The allowlist is narrower than the tool's:
no global register is ever written (so no LPMode, ResetL or interrupt-mask
change), per channel only the command/start registers and the write buffer.
An access the vendor driver never makes is an unacknowledged MMIO on this
SoC (SError, kernel panic), hence the allowlist and the decode check before
every session.

  cage P (1..32) = FPGA channel P-1 = sideband bit P-1
  0x140 present (1 = module), 0x160 IntL (raw, polarity unconfirmed),
  0x170 LPMode (1 = asserted, inferred), 0x178 ResetL (1 = held in reset;
  power-up 0xffffffff: every module in reset until something releases it)

Two things need writes this module does not do by default (they are what
`cs8210-qsfp --enable` and `--init P` do); CS8210_QSFP_PREPARE=1 allows both:
  * PCI memory decode: off after U-Boot. Turning it on is
    `echo 1 > /sys/bus/pci/devices/<bdf>/enable` (pci_enable_device: a write
    to the FPGA's PCI command register). Without it nothing is read: every
    cage reads "not present".
  * channel init: at power-up each channel's clock profiles are 0x00081766;
    the vendor probe writes +0x100c=0x80000000, +0x1008=0, +0x1000=0x00660918
    (400 kHz). A channel not in that state is not used (EEPROM unreadable).
"""
import contextlib
import errno
import fcntl
import mmap
import os
import time

from . import hw

VENDOR, DEVICE = 0x1D0F, 0x8571
MAP_LEN = 0x10000

R_VERSION, R_PRESENT, R_INT, R_LPMODE, R_RESET = 0x000, 0x140, 0x160, 0x170, 0x178
CH_BASE, CH_STRIDE, NCH = 0x1000, 0x100, 32
C_PROFILE, C_PROFSEL, C_MRESET, C_CMD, C_START, C_STATUS = 0x00, 0x08, 0x0C, 0x50, 0x54, 0x60
WBUF, RBUF, BUF_STRIDE, BUF_MAX = 0x4000, 0x8000, 0x200, 0x80
CMD_READ, START_BUSY, STATUS_ERR = 1 << 28, 1 << 31, 1 << 1
PROBE_PROFILE = 0x00660918
PROBE = ((C_MRESET, 0x80000000), (C_PROFSEL, 0), (C_PROFILE, PROBE_PROFILE))   # vendor probe order
POLLS = 50                       # vendor: 50 x ~1 ms, then -EBUSY
CHUNK = 32                       # bytes per read transaction (vendor sfp_module, cs8210-qsfp)
PAGE_SELECT = 127

READ_GLOBAL = {R_VERSION, R_PRESENT, R_INT, R_LPMODE, R_RESET}
READ_CH = {C_PROFILE, C_PROFSEL, C_MRESET, C_START, C_STATUS}
WRITE_CH = {C_CMD, C_START}
WRITE_CH_PREPARE = {C_MRESET, C_PROFSEL, C_PROFILE}

LOCK_FILE = os.environ.get("CS8210_QSFP_LOCK", "/run/lock/cs8210-qsfp.lock")   # cs8210-qsfp's
LOCK_WAIT = 5.0


class FpgaUnavailable(OSError):
    pass


class I2cError(OSError):
    pass


def allowed(off, write, prepare=False):
    """Is BAR0+off a read (write) this module may make?"""
    if off % 4 or off < 0:
        return False
    if off < CH_BASE:
        return not write and off in READ_GLOBAL
    if off < CH_BASE + NCH * CH_STRIDE:
        r = (off - CH_BASE) % CH_STRIDE
        if write:
            return r in WRITE_CH or (prepare and r in WRITE_CH_PREPARE)
        return r in READ_CH
    base = WBUF if write else RBUF
    return base <= off < base + NCH * BUF_STRIDE and (off - base) % BUF_STRIDE < BUF_MAX


def chb(port):
    return CH_BASE + (port - 1) * CH_STRIDE


class Fpga:
    def __init__(self):
        self.pci_root = hw.path("/sys/bus/pci/devices")
        self.bdf = None
        self.v32 = None
        self._map = self._fd = None
        self.problem = None              # last reason the FPGA could not be used

    # ------------------------------------------------------------ PCI
    def _find(self):
        hits = []
        for name in sorted(os.listdir(self.pci_root)):
            d = os.path.join(self.pci_root, name)
            try:
                with open(d + "/vendor") as f:
                    v = int(f.read(), 16)
                with open(d + "/device") as f:
                    dv = int(f.read(), 16)
            except (OSError, ValueError):
                continue
            if (v, dv) == (VENDOR, DEVICE):
                hits.append(name)
        if len(hits) != 1:
            raise FpgaUnavailable(errno.ENODEV, "%d PCI devices %04x:%04x" % (len(hits), VENDOR, DEVICE))
        return hits[0]

    def _command(self, d):
        with open(d + "/config", "rb") as f:
            c = f.read(6)
        return int.from_bytes(c[4:6], "little")

    def check(self):
        """PCI checks before a session; raises FpgaUnavailable unless BAR0
        decode is on (turning it on only with CS8210_QSFP_PREPARE=1)."""
        if self.bdf is None:
            self.bdf = self._find()
        d = os.path.join(self.pci_root, self.bdf)
        if os.path.exists(d + "/driver"):
            raise FpgaUnavailable(errno.EBUSY, "%s is bound to a kernel driver" % self.bdf)
        with open(d + "/resource") as f:
            start, end, _ = (int(x, 16) for x in f.readline().split())
        if start == 0 or end - start + 1 < MAP_LEN:
            raise FpgaUnavailable(errno.ENODEV, "%s BAR0 unassigned or short" % self.bdf)
        if not self._command(d) & 2:
            if not hw.QSFP_PREPARE:
                raise FpgaUnavailable(errno.EACCES, "%s PCI memory decode is off (cs8210-qsfp --enable, "
                                      "or CS8210_QSFP_PREPARE=1)" % self.bdf)
            with open(d + "/enable", "w") as f:
                f.write("1")
            if hw.ROOT:
                from .fake import fake_pci_enabled
                fake_pci_enabled(d)
            if not self._command(d) & 2:
                raise FpgaUnavailable(errno.EIO, "%s memory decode still off after enable" % self.bdf)
        if self.v32 is None:
            self._open(d)

    def _open(self, d):
        if hw.ROOT:
            from .fake import FakeBar
            self.v32 = FakeBar(hw.ROOT)
            return
        self._fd = os.open(d + "/resource0", os.O_RDWR | os.O_SYNC)
        self._map = mmap.mmap(self._fd, MAP_LEN, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE)
        self.v32 = memoryview(self._map).cast("I")

    # ------------------------------------------------------------ BAR0
    def r32(self, off):
        if not allowed(off, False):
            raise ValueError("BAR0+%#x read not allowed" % off)
        return self.v32[off // 4]

    def w32(self, off, val, prepare=False):
        if not allowed(off, True, prepare):
            raise ValueError("BAR0+%#x write not allowed" % off)
        self.v32[off // 4] = val & 0xFFFFFFFF

    def sideband(self):
        """{"present", "reset", "lpmode", "int"}: 32-bit masks, bit P-1 = cage P."""
        self.check()
        return {"present": self.r32(R_PRESENT), "reset": self.r32(R_RESET),
                "lpmode": self.r32(R_LPMODE), "int": self.r32(R_INT)}

    # ------------------------------------------------------------ I2C
    @contextlib.contextmanager
    def locked(self):
        """cs8210-qsfp's flock (so the tool and the platform never interleave
        transactions), then the PCI checks."""
        lp = hw.path(LOCK_FILE)
        if not os.path.isdir(os.path.dirname(lp)):
            lp = hw.path("/tmp/cs8210-qsfp.lock")
        fd = os.open(lp, os.O_RDWR | os.O_CREAT, 0o600)
        try:
            deadline = time.monotonic() + LOCK_WAIT
            while True:
                try:
                    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() > deadline:
                        raise FpgaUnavailable(errno.EBUSY, "%s held (cs8210-qsfp running?)" % lp)
                    time.sleep(0.05)
            self.check()
            yield self
        finally:
            os.close(fd)

    def _ready(self, port):
        b = chb(port)
        ok = (self.r32(b + C_PROFILE) == PROBE_PROFILE and not self.r32(b + C_MRESET) & 1
              and self.r32(b + C_PROFSEL) & 3 != 3 and not self.r32(b + C_START) & START_BUSY)
        if ok:
            return
        if not hw.QSFP_PREPARE:
            raise I2cError(errno.EAGAIN, "cage %d: FPGA channel not initialised (cs8210-qsfp --init %d, "
                           "or CS8210_QSFP_PREPARE=1)" % (port, port))
        for reg, val in PROBE:
            self.w32(b + reg, val, prepare=True)
        if self.r32(b + C_PROFILE) != PROBE_PROFILE:
            raise I2cError(errno.EIO, "cage %d: channel init did not stick" % port)

    def _xfer(self, port, addr, off, n, data=None):
        ch, b = port - 1, chb(port)
        if not (0x03 <= addr <= 0x77 and 0 <= off and off + n <= 0x100 and 1 <= n <= BUF_MAX):
            raise ValueError("bad transfer %#x/%#x/%d" % (addr, off, n))
        if data is not None:
            for i in range(0, n, 4):
                self.w32(WBUF + ch * BUF_STRIDE + i, int.from_bytes(bytes(data[i:i + 4]).ljust(4, b"\0"), "little"))
            self.w32(b + C_CMD, (addr << 8) | n)
        else:
            self.w32(b + C_CMD, CMD_READ | (addr << 8) | n)
        self.w32(b + C_START, START_BUSY | off)
        busy = True
        for _ in range(POLLS):
            time.sleep(0.001)
            if not self.r32(b + C_START) & START_BUSY:
                busy = False
                break
        st = self.r32(b + C_STATUS)
        if st & STATUS_ERR:
            raise I2cError(errno.ENXIO, "cage %d addr %#x off %#x: NACK" % (port, addr, off))
        if busy:
            raise I2cError(errno.EBUSY, "cage %d: transaction still busy" % port)
        if data is not None:
            return b""
        out = bytearray()
        for i in range(0, n, 4):
            out += self.r32(RBUF + ch * BUF_STRIDE + i).to_bytes(4, "little")
        return bytes(out[:n])

    def i2c_read(self, port, addr, off, n):
        """n bytes at register off of addr on cage port (inside locked())."""
        self._ready(port)
        out = bytearray()
        while n > 0:
            k = min(CHUNK, n)
            out += self._xfer(port, addr, off, k)
            off, n = off + k, n - k
        return bytes(out)

    def select_page(self, port, page):
        """The one module write: page select (byte 127 at 0x50)."""
        self._ready(port)
        self._xfer(port, 0x50, PAGE_SELECT, 1, bytes([page & 0xff]))


_fpga = None


def fpga():
    global _fpga
    if _fpga is None:
        _fpga = Fpga()
    return _fpga
