#!/usr/bin/env python3
"""Read the DMC-12 board CPLD (Lattice MachXO3LF-2100C) back over its I2C config port, read-only.

Runs ON the box (no dependencies): ssh root@<box> 'python3 - --out /tmp/cpld' < cpld-readback.py
The workstation copies /tmp/cpld/* to dumps/<unit>/cpld/ afterwards (see board-notes.md, research-open-items.md 2).

The sequence is the read-only subset of the factory `cpldupd` (disassembled: machxo3_flow,
machxo3_verify_cfg/_ufm, machxo3_wait_ready, machxo3_exit_program_mode), sent to i2c-4 0x10:
  E0 000000 >4 IDCODE          C0 000000 >4 USERCODE      3C 000000 >4 STATUS
  19 000000 >8 TraceID         E7 000000 >8 feature row   FB 000000 >2 FEABITS
  74 08 00    transparent mode (ISC_ENABLE_X: the user logic keeps running; cpldupd's choice)
  46 000000   reset address to CFG      47 000000  reset address to UFM
  73 000001 >16 read one page, address auto-increments  (F0 000000 >1 busy poll in between)
  26 00 00    ISC_DISABLE (leave transparent mode, no reload)    FF FFFFFF  bypass
Every write goes through ALLOW: anything else raises before it reaches the bus. Never sent (and not
expressible here): erase 0E, program 70/C2/E4/F8, done 5E, refresh 79, offline C6, security CE.

  python3 - --dry-run           print every transaction, touch nothing
  python3 - --id-only           IDCODE/USERCODE/STATUS/TraceID/feature row (no transparent mode)
  python3 - --out DIR           everything above + CFG + UFM pages
"""
import argparse, ctypes, fcntl, json, os, sys, time

BUS, ADDR = 4, 0x10
CFG_PAGES, UFM_PAGES = 3198, 639      # MachXO3LF-2100 (= MachXO2-2000 array); checked against the read
EXPECT_IDCODE = 0x612BB043            # cpldupd's IDCODE config item

ALLOW = {  # exact write bytes -> expected read length
    bytes.fromhex("E0000000"): 4, bytes.fromhex("C0000000"): 4, bytes.fromhex("3C000000"): 4,
    bytes.fromhex("19000000"): 8, bytes.fromhex("E7000000"): 8, bytes.fromhex("FB000000"): 2,
    bytes.fromhex("F0000000"): 1,
    bytes.fromhex("740800"): 0, bytes.fromhex("46000000"): 0, bytes.fromhex("47000000"): 0,
    bytes.fromhex("73000001"): 16,
    bytes.fromhex("260000"): 0, bytes.fromhex("FFFFFFFF"): 0,
}

I2C_RDWR, I2C_M_RD = 0x0707, 0x0001


class Msg(ctypes.Structure):
    _fields_ = [("addr", ctypes.c_uint16), ("flags", ctypes.c_uint16),
                ("len", ctypes.c_uint16), ("buf", ctypes.POINTER(ctypes.c_uint8))]


class RdWr(ctypes.Structure):
    _fields_ = [("msgs", ctypes.POINTER(Msg)), ("nmsgs", ctypes.c_uint32)]


class Port:
    def __init__(self, dry):
        self.dry, self.log = dry, []
        self.fd = None if dry else os.open(f"/dev/i2c-{BUS}", os.O_RDWR)

    def xfer(self, cmd, quiet=False):
        """One allowlisted command: write, then (repeated start) read, in one I2C_RDWR."""
        cmd = bytes(cmd)
        if cmd not in ALLOW:
            raise SystemExit(f"refusing non-allowlisted command {cmd.hex()}")
        rlen = ALLOW[cmd]
        if not quiet:
            self.log.append(f"w {cmd.hex()}" + (f" r{rlen}" if rlen else ""))
        if self.dry:
            if not quiet:
                print(f"  i2c-{BUS} 0x{ADDR:02x}: w {cmd.hex(' ')}" + (f"  r {rlen}" if rlen else ""))
            return bytes(rlen) if cmd != bytes.fromhex("F0000000") else b"\0"
        wbuf = (ctypes.c_uint8 * len(cmd))(*cmd)
        rbuf = (ctypes.c_uint8 * max(rlen, 1))()
        msgs = (Msg * 2)(Msg(ADDR, 0, len(cmd), wbuf), Msg(ADDR, I2C_M_RD, rlen, rbuf))
        fcntl.ioctl(self.fd, I2C_RDWR, RdWr(msgs, 2 if rlen else 1))
        return bytes(rbuf[:rlen])

    def wait_ready(self, timeout=2.0):
        """cpldupd's machxo3_wait_ready: F0 (LSC_CHECK_BUSY) until bit 7 clears, 200 us apart."""
        t = time.monotonic()
        while self.xfer(bytes.fromhex("F0000000"), quiet=True)[0] & 0x80:
            if time.monotonic() - t > timeout:
                raise SystemExit("CPLD stayed busy")
            time.sleep(0.0002)


def be32(b):
    return int.from_bytes(b, "big")


def status_bits(s):
    """MachXO3 status register (TN1279 table): the bits we act on."""
    return {"done": bool(s >> 8 & 1), "isc_enabled": bool(s >> 9 & 1), "busy": bool(s >> 12 & 1),
            "fail": bool(s >> 13 & 1), "raw": f"0x{s:08x}"}


def pages(p, n, label):
    out = bytearray()
    for i in range(n):
        out += p.xfer(bytes.fromhex("73000001"), quiet=i > 1)
        p.wait_ready()
        if not p.dry and i % 400 == 0:
            print(f"  {label}: page {i}/{n}", file=sys.stderr, flush=True)
    return bytes(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--id-only", action="store_true")
    ap.add_argument("--out", default="/tmp/cpld")
    a = ap.parse_args()
    p = Port(a.dry_run)
    info = {}
    info["idcode"] = f"0x{be32(p.xfer(bytes.fromhex('E0000000'))):08x}"
    if not a.dry_run and int(info["idcode"], 16) != EXPECT_IDCODE:
        raise SystemExit(f"IDCODE {info['idcode']} != 0x{EXPECT_IDCODE:08x}: not the expected part, stopping")
    info["usercode"] = f"0x{be32(p.xfer(bytes.fromhex('C0000000'))):08x}"
    info["status_before"] = status_bits(be32(p.xfer(bytes.fromhex("3C000000"))))
    info["traceid"] = p.xfer(bytes.fromhex("19000000")).hex()
    info["feature_row"] = p.xfer(bytes.fromhex("E7000000")).hex()
    info["feabits"] = p.xfer(bytes.fromhex("FB000000")).hex()
    if not a.id_only:
        try:
            p.xfer(bytes.fromhex("740800"))
            time.sleep(0.001)
            p.wait_ready()
            info["status_transparent"] = status_bits(be32(p.xfer(bytes.fromhex("3C000000"))))
            # The feature row reads all-FF outside ISC mode (seen on u01): read it again here.
            info["feature_row_isc"] = p.xfer(bytes.fromhex("E7000000")).hex()
            info["feabits_isc"] = p.xfer(bytes.fromhex("FB000000")).hex()
            info["usercode_isc"] = f"0x{be32(p.xfer(bytes.fromhex('C0000000'))):08x}"
            p.xfer(bytes.fromhex("46000000")); p.wait_ready()
            cfg = pages(p, CFG_PAGES, "CFG")
            p.xfer(bytes.fromhex("47000000")); p.wait_ready()
            ufm = pages(p, UFM_PAGES, "UFM")
        finally:   # always leave transparent mode (no refresh: the running logic is untouched)
            p.xfer(bytes.fromhex("260000"))
            p.xfer(bytes.fromhex("FFFFFFFF"))
        info["status_after"] = status_bits(be32(p.xfer(bytes.fromhex("3C000000"))))
        if not a.dry_run:
            os.makedirs(a.out, exist_ok=True)
            open(f"{a.out}/cfg.bin", "wb").write(cfg)
            open(f"{a.out}/ufm.bin", "wb").write(ufm)
            info["cfg_bytes"], info["ufm_bytes"] = len(cfg), len(ufm)
            info["cfg_blank_pages"] = sum(cfg[i:i + 16] in (b"\0" * 16, b"\xff" * 16) for i in range(0, len(cfg), 16))
            info["ufm_blank_pages"] = sum(ufm[i:i + 16] in (b"\0" * 16, b"\xff" * 16) for i in range(0, len(ufm), 16))
    info["transactions"] = p.log
    print(json.dumps(info, indent=1))
    if not a.dry_run and not a.id_only:
        open(f"{a.out}/info.json", "w").write(json.dumps(info, indent=1) + "\n")


if __name__ == "__main__":
    main()
