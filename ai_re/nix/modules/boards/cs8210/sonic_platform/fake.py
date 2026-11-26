"""The fake board's hardware (CS8210_PLATFORM_ROOT=<root>, built by
fake_board.py): files instead of I2C devices, and what cs8210-optics would
answer for the cages, so the tests run the real code paths above them.

  <root>/i2c/<dev>.bin        256 register bytes of sys_cpld / fan_top / fan_bottom;
                              missing file = device absent (ENXIO)
  <root>/fpga/port<N>.bin     module in cage N (present while it exists), optoe
                              layout: bytes 0-255 lower + upper page 0, then upper
                              pages 1, 2, ...; an SFP (byte 0 = 0x03) has A2h at 256-511
  <root>/fpga/sideband.json   {"reset": mask, "lpmode": mask, "int": mask,
                              "dead": "why": the daemon reports the FPGA unavailable}
  <root>/fpga/calls.txt       every write / lpmode / reset request, one JSON per line
The same files drive the daemon's own simulator (cs8210-optics daemon --simulate <root>/fpga).
"""
import errno
import json

from .optics import OpticsError


class FakeI2c:
    def __init__(self, root):
        self.dir = root + "/i2c"

    def read_byte(self, dev, reg):
        try:
            with open("%s/%s.bin" % (self.dir, dev), "rb") as f:
                regs = f.read().ljust(256, b"\xff")
        except OSError:
            raise OSError(errno.ENXIO, "fake %s missing" % dev)
        return regs[reg]


class FakeOptics:
    def __init__(self, root):
        self.dir = root + "/fpga"
        self.problem = None

    def _sbfile(self):
        try:
            with open(self.dir + "/sideband.json") as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}

    def _log(self, **call):
        with open(self.dir + "/calls.txt", "a") as f:
            f.write(json.dumps(call, sort_keys=True) + "\n")

    def _img(self, cage):
        try:
            with open("%s/port%d.bin" % (self.dir, cage), "rb") as f:
                return bytearray(f.read())
        except OSError:
            return None

    def sideband(self):
        sb = self._sbfile()
        if sb.get("dead"):
            self.problem = "I2C FPGA unavailable: %s" % sb["dead"]
            return None
        present = sum(1 << (c - 1) for c in range(1, 33) if self._img(c) is not None)
        return {"present": present, "reset": sb.get("reset", 0), "lpmode": sb.get("lpmode", 0),
                "interrupt": sb.get("int", 0)}

    def _usable(self, cage):
        sb = self.sideband()
        if sb is None:
            raise OpticsError(errno.EIO, self.problem)
        img = self._img(cage)
        if img is None:
            raise OpticsError(errno.EIO, "cage %d: no module" % cage)
        if sb["reset"] >> (cage - 1) & 1:
            raise OpticsError(errno.EIO, "cage %d: module held in reset" % cage)
        return img

    @staticmethod
    def _limit(img):
        """End of the module's memory in the flat layout (what the daemon refuses beyond)."""
        if img[0] == 0x03:
            return 512
        flat = img[2] & (0x80 if img[0] >= 0x18 else 0x04)
        return 256 if flat else len(img)

    def read(self, cage, offset, n):
        img = self._usable(cage)
        if offset + n > min(self._limit(img), len(img)):
            raise OpticsError(errno.EIO, "offset %d beyond the module's memory" % (offset + n - 1))
        return bytes(img[offset:offset + n])

    def write(self, cage, offset, data):
        img = self._usable(cage)
        if offset + len(data) > min(self._limit(img), len(img)):
            raise OpticsError(errno.EIO, "beyond the module's memory")
        self._log(op="write", cage=cage, offset=offset, data=bytes(data).hex())
        img[offset:offset + len(data)] = data
        with open("%s/port%d.bin" % (self.dir, cage), "wb") as f:
            f.write(img)

    def _set(self, key, cage, on):
        sb = self._sbfile()
        m = sb.get(key, 0)
        sb[key] = (m | 1 << (cage - 1)) if on else (m & ~(1 << (cage - 1)))
        with open(self.dir + "/sideband.json", "w") as f:
            json.dump(sb, f)

    def set_lpmode(self, cage, on):
        self._usable(cage)
        self._log(op="lpmode", cage=cage, on=bool(on))
        self._set("lpmode", cage, on)

    def reset(self, cage):
        self._usable(cage)
        self._log(op="reset", cage=cage)
