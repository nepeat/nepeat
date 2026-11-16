"""The fake board's hardware (CS8210_PLATFORM_ROOT=<root>, built by
fake_board.py): files instead of I2C devices and an I2C FPGA simulator that
answers fpga.py's register protocol, so the tests run the real code paths.

  <root>/i2c/<dev>.bin        256 register bytes of sys_cpld / fan_top / fan_bottom;
                              missing file = device absent (ENXIO)
  <root>/fpga/port<N>.bin     module EEPROM in cage N (present while it exists):
                              bytes 0-255 = 0x50 lower + upper page 0, then upper
                              pages 1, 2, ... (byte 127 = selected page); an SFP
                              (byte 0 = 0x03, e.g. in a QSA) has A2h (0x51) at 256-511
  <root>/fpga/sideband.json   {"reset": mask, "lpmode": mask, "int": mask,
                              "uninit": [cages whose channel is at power-up values]}
  <root>/fpga/writes.txt      every BAR0 write, "offset value" (hex), appended
"""
import errno
import json
import os

from . import fpga as F


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


def fake_pci_enabled(devdir):
    """What the kernel does on "echo 1 > enable": PCI_COMMAND_MEMORY on."""
    with open(devdir + "/config", "r+b") as f:
        c = bytearray(f.read())
        c[4] |= 0x02
        f.seek(0)
        f.write(c)


class FakeBar:
    VERSION = 0x010b0009
    POWER_UP_PROFILE = 0x00081766

    def __init__(self, root):
        self.dir = root + "/fpga"
        n = F.NCH
        self.ch = {}                    # (channel, reg) -> value
        self.wbuf = [bytearray(F.BUF_MAX) for _ in range(n)]
        self.rbuf = [bytearray(F.BUF_MAX) for _ in range(n)]

    # ------------------------------------------------------------ state files
    def _sb(self):
        try:
            with open(self.dir + "/sideband.json") as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}

    def _img_path(self, port):
        return "%s/port%d.bin" % (self.dir, port)

    def _present(self):
        return sum(1 << (p - 1) for p in range(1, F.NCH + 1) if os.path.exists(self._img_path(p)))

    def _chreg(self, ch, reg):
        if (ch, reg) in self.ch:
            return self.ch[(ch, reg)]
        if reg == F.C_PROFILE:
            return self.POWER_UP_PROFILE if ch + 1 in self._sb().get("uninit", []) else F.PROBE_PROFILE
        return 0

    # ------------------------------------------------------------ word access
    def __getitem__(self, idx):
        off = idx * 4
        if off < F.CH_BASE:
            sb = self._sb()
            return {F.R_VERSION: self.VERSION, F.R_PRESENT: self._present(), F.R_RESET: sb.get("reset", 0),
                    F.R_LPMODE: sb.get("lpmode", 0), F.R_INT: sb.get("int", 0)}.get(off, 0)
        if off < F.CH_BASE + F.NCH * F.CH_STRIDE:
            ch, reg = divmod(off - F.CH_BASE, F.CH_STRIDE)
            return self._chreg(ch, reg)
        if F.RBUF <= off < F.RBUF + F.NCH * F.BUF_STRIDE:
            ch, i = divmod(off - F.RBUF, F.BUF_STRIDE)
            return int.from_bytes(self.rbuf[ch][i:i + 4], "little")
        raise OSError(errno.EFAULT, "fake BAR0+%#x: nothing there" % off)

    def __setitem__(self, idx, val):
        off = idx * 4
        with open(self.dir + "/writes.txt", "a") as f:
            f.write("%#x %#x\n" % (off, val))
        if F.WBUF <= off < F.WBUF + F.NCH * F.BUF_STRIDE:
            ch, i = divmod(off - F.WBUF, F.BUF_STRIDE)
            self.wbuf[ch][i:i + 4] = val.to_bytes(4, "little")
            return
        if not F.CH_BASE <= off < F.CH_BASE + F.NCH * F.CH_STRIDE:
            raise OSError(errno.EFAULT, "fake BAR0+%#x: global register written" % off)
        ch, reg = divmod(off - F.CH_BASE, F.CH_STRIDE)
        if reg == F.C_MRESET:
            val &= ~(1 << 31)                       # self-clearing soft reset
        if reg == F.C_START and val & F.START_BUSY:
            self.ch[(ch, F.C_STATUS)] = self._run(ch, val & 0xff)
            val &= ~F.START_BUSY                    # done at once
        self.ch[(ch, reg)] = val

    # ------------------------------------------------------------ engine
    def _run(self, ch, off):
        port, cmd = ch + 1, self._chreg(ch, F.C_CMD)
        addr, n, read = (cmd >> 8) & 0x7f, cmd & 0xff, bool(cmd & F.CMD_READ)
        if (self._chreg(ch, F.C_PROFILE) != F.PROBE_PROFILE or self._chreg(ch, F.C_MRESET) & 1
                or self._sb().get("reset", 0) >> ch & 1):
            return F.STATUS_ERR
        try:
            with open(self._img_path(port), "rb") as f:
                img = bytearray(f.read())
        except OSError:
            return F.STATUS_ERR                     # empty cage: NACK
        sfp = img[0] == 0x03
        if addr == 0x51 and sfp:
            base = 256
        elif addr == 0x50:
            base = 0
        else:
            return F.STATUS_ERR

        def lin(o):
            if base or o < 128 or sfp:
                return base + o
            return o + 128 * img[127]

        if read:
            if lin(off) + n > len(img):
                return F.STATUS_ERR
            self.rbuf[ch][:n] = img[lin(off):lin(off) + n]
        else:
            for i in range(n):
                img[lin(off + i)] = self.wbuf[ch][i]
            with open(self._img_path(port), "wb") as f:
                f.write(img)
        return 0
