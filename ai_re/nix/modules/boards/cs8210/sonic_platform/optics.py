"""Client of cs8210-optics, the one owner of the I2C FPGA (PCI 1d0f:8571):
the Rust daemon in the monorepo's rust/cs8210 (nix/nixos/module/cs8210/optics).
JSON lines over its Unix socket; every cage access of this package goes
through here, nothing here touches the FPGA itself.

  sideband()                 {"present", "reset", "lpmode", "interrupt"}: 32-bit
                             masks, bit P-1 = cage P (cached SIDEBAND_TTL s); None
                             when the daemon is down or the FPGA unavailable
  read(cage, offset, n)      module memory at optoe flat offsets (upper page p at
                             128 + 128 * p; SFP A2h at 256); the daemon selects pages
  write(cage, offset, data)  same addressing; refused for undeclared cages
  set_lpmode(cage, on)       the cage's LPMode pin
  reset(cage)                ResetL pulse; the daemon brings the module up again

The daemon owns module bring-up (power class, CMIS application select), so
xcvrd runs with --skip_cmis_mgr.
"""
import errno
import json
import os
import socket
import time

from . import hw

SOCKET = os.environ.get("CS8210_OPTICS_SOCKET", "/run/cs8210-optics/optics.sock")
SIDEBAND_TTL = float(os.environ.get("CS8210_OPTICS_SIDEBAND_TTL", "0.5"))
TIMEOUT = 15.0


class OpticsError(OSError):
    pass


class Client:
    def __init__(self, path=SOCKET):
        self.path = path
        self._sb = None                   # (monotonic time, masks)
        self.problem = None               # last reason the daemon could not be used

    def call(self, **req):
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(TIMEOUT)
        try:
            s.connect(self.path)
            s.sendall((json.dumps(req) + "\n").encode())
            buf = b""
            while not buf.endswith(b"\n"):
                chunk = s.recv(65536)
                if not chunk:
                    break
                buf += chunk
        except OSError as e:
            raise OpticsError(e.errno or errno.EIO, "%s: %s" % (self.path, e))
        finally:
            s.close()
        try:
            r = json.loads(buf)
        except ValueError:
            raise OpticsError(errno.EIO, "cs8210-optics: bad reply")
        if not r.get("ok"):
            raise OpticsError(errno.EIO, r.get("error") or "cs8210-optics: error")
        return r

    def sideband(self):
        sb = self._sb
        if sb is not None and time.monotonic() - sb[0] < SIDEBAND_TTL:
            return sb[1]
        try:
            r = self.call(op="sideband")
        except OSError as e:
            self.problem = str(e)
            return None
        if r.get("dead"):
            self.problem = "I2C FPGA unavailable: %s" % r["dead"]
            return None
        self.problem = None
        masks = {k: int(r.get(k) or 0) for k in ("present", "reset", "lpmode", "interrupt")}
        self._sb = (time.monotonic(), masks)
        return masks

    def read(self, cage, offset, n):
        return bytes.fromhex(self.call(op="read", cage=cage, offset=offset, len=n)["data"])

    def write(self, cage, offset, data):
        self.call(op="write", cage=cage, offset=offset, data=bytes(data).hex())

    def set_lpmode(self, cage, on):
        self._sb = None
        self.call(op="lpmode", cage=cage, on=bool(on))

    def reset(self, cage):
        self._sb = None
        self.call(op="reset", cage=cage)


_client = None


def client():
    global _client
    if _client is None:
        if hw.ROOT:
            from .fake import FakeOptics
            _client = FakeOptics(hw.ROOT)
        else:
            _client = Client()
    return _client
