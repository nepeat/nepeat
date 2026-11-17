#!/usr/bin/env python3
"""cs8210-asic-avs: the Teralynx 7 core voltage (AVS) step of the vendor U-Boot, in Linux.

The vendor U-Boot (2018.05-alpine_db-v3.65.1.015, main_loop hook 0x01104600)
reads four fuse words of the ASIC (PCI 1d98:1b58, BAR0 + 0x11178), decodes
a ring-oscillator bin (RoV) and a 6-character lot ID, picks the VDD core
target and writes it to the core regulator (TI TPS53688 "vddcore", 0x62
behind 0x70:2 -> 0x74:0 -> 0x72:2) as PMBus PAGE = 0, VOUT_COMMAND = code.
This does the same before the switch stack starts, with checks the vendor
code does not make. Decode and table: ai_re/devices/cs8210-32x/avs.md.

  apply [--dry-run] [--rewrite] [--fuse-words W0,W1,W2,W3]
        read the fuses, decode, check the regulator, write VOUT_COMMAND only
        if it differs (--rewrite: also when equal, the same value; test of the
        write path), read back, record /run/cs8210-avs.done
  status        regulator readings and the state file, human readable
  metrics       node_exporter textfile output (reads only)
  decode W0 W1 W2 W3   offline decode

Safety rules (PROGRESS hazards): no /dev/mem; the fuse words are the only
ASIC registers touched, read through sysfs resource0 after "enable" (memory
decode), each as one 32-bit load, and only while the root port answers
failed reads with all-ones (cs8210-pcie-errors). With IO_STRICT_DEVMEM the
BAR cannot be mapped while ipd owns it, so ipd is loaded after this step
(the module blacklists it for udev); a later run (ipd bound) takes the fuse
words from this boot's state file or the last BAR read
(/var/lib/cs8210-asic-avs/fuses). PMBus: only whitelisted commands, no
STORE/RESTORE/CLEAR_FAULTS, writes only PAGE = 0 and VOUT_COMMAND.
"""
import argparse
import ctypes
import fcntl
import glob
import mmap
import os
import sys
import time

# ---- vendor decode (0x01104600; ai_re/devices/cs8210-32x/avs.md) ----------
FUSE_OFFSET = 0x11178
FUSE_WORDS = 4
LOT_BITS = 42                       # 6 chars x 7 bits, MSB first, from bit 0 of word 0
# RoV 0 and one of these lots: 780 mV (string table at 0x011aad98)
LOTS_780 = (b"PNHB06", b"PNHB07", b"PNHB08", b"PNHB09", b"PNKW27", b"PNHB10", b"PNHB11", b"PPHN57")
ROV_MV = {2: 820, 5: 840, 8: 780, 10: 760}   # jump table 0x011a47d4; other RoV: 800
DEFAULT_MV = 800
# (VOUT_COMMAND code, mV) at 0x011a0670; linear16, exponent -10 (code = mV * 1.024)
TABLE = ((0x30A, 760), (0x31F, 780), (0x333, 800), (0x348, 820), (0x35C, 840))
MV_MIN, MV_MAX = 760, 840           # sanity limits: refuse anything else

# ---- board ---------------------------------------------------------------
ASIC_ID = (0x1D98, 0x1B58)
PCI_ROOT = "/sys/bus/pci/devices"
# the 0x72 mux (pca9548) as a devicetree node; its channel 2 has no DT child,
# so the bus is the mux's channel-2 link (Linux numbers channels in probe order)
MUX72_NODE = "i2c-mux@70/i2c@2/i2c-mux@74/i2c@0/i2c-mux@72"
MUX72_CHANNEL = 2
REG_ADDR = 0x62
IC_DEVICE_ID = bytes([0x54, 0x49, 0x53, 0x68, 0x80, 0x00])   # "TISh" 80 00, read 2026-11-17
VOUT_MODE_LINEAR_M10 = 0x16

STATE = "/run/cs8210-avs.done"
LOCK = "/run/cs8210-asic-avs.lock"
CACHE = "/var/lib/cs8210-asic-avs/fuses"

# PMBus commands this tool may send. Anything else (STORE_*/RESTORE_*,
# CLEAR_FAULTS, MFR_*) is refused before it reaches the bus.
PMBUS_READ = {
    0x00: ("PAGE", 1), 0x01: ("OPERATION", 1), 0x02: ("ON_OFF_CONFIG", 1),
    0x20: ("VOUT_MODE", 1), 0x21: ("VOUT_COMMAND", 2), 0x24: ("VOUT_MAX", 2),
    0x40: ("VOUT_OV_FAULT_LIMIT", 2), 0x79: ("STATUS_WORD", 2), 0x7A: ("STATUS_VOUT", 1),
    0x7E: ("STATUS_CML", 1), 0x88: ("READ_VIN", 2), 0x89: ("READ_IIN", 2),
    0x8B: ("READ_VOUT", 2), 0x8C: ("READ_IOUT", 2), 0x8D: ("READ_TEMPERATURE_1", 2),
    0x96: ("READ_POUT", 2), 0x97: ("READ_PIN", 2), 0xAD: ("IC_DEVICE_ID", 7),
}
PMBUS_WRITE = {0x00: "PAGE", 0x21: "VOUT_COMMAND"}


class Refuse(Exception):
    """A check failed: nothing (more) is written, the unit fails."""


def log(msg):
    print(msg, flush=True)


def lot_id(words):
    """Vendor loop: bit i of the 128-bit fuse value (word i>>5, bit i&31) is
    bit 6 - i%7 of character i//7."""
    out = bytearray()
    ch = 0
    for i in range(LOT_BITS):
        bit = (words[i >> 5] >> (i & 31)) & 1
        ch = (ch | (bit << (6 - i % 7))) & 0xFF
        if i % 7 == 6:
            out.append(ch)
            ch = 0
    return bytes(out)


def rov(words):
    return (words[2] >> 13) & 0xF


def target_mv(r, lot):
    if r == 0:
        return 780 if lot in LOTS_780 else DEFAULT_MV
    return ROV_MV.get(r, DEFAULT_MV)


def closest(t):
    """The vendor's "Closest VDD Core" step, branch for branch: (code, mV)."""
    if t == 760:
        return TABLE[0]
    if t <= 780:
        lower_code, upper_mv, k = 0x30A, 780, 1
    elif t <= 800:
        lower_code, upper_mv, k = 0x31F, 800, 2
    elif t <= 820:
        lower_code, upper_mv, k = 0x333, 820, 3
    else:
        lower_code, upper_mv, k = 0x348, 840, 4
    lower_mv = TABLE[k - 1][1]
    if upper_mv - t < t - lower_mv:
        return TABLE[k][0], upper_mv
    return lower_code, lower_mv


def decode(words):
    if len(words) != FUSE_WORDS:
        raise ValueError("need 4 fuse words")
    lot = lot_id(words)
    r = rov(words)
    t = target_mv(r, lot)
    code, mv = closest(t)
    return {"rov": r, "lot": lot, "target_mv": t, "code": code, "mv": mv}


def check_target(d):
    if not MV_MIN <= d["mv"] <= MV_MAX or (d["code"], d["mv"]) not in TABLE:
        raise Refuse(f"decoded {d['mv']} mV (code {d['code']:#06x}) is outside {MV_MIN}..{MV_MAX} mV / the vendor table")


def lot_str(lot):
    return lot.decode("ascii", "backslashreplace")


def linear16(code, mode=VOUT_MODE_LINEAR_M10):
    exp = mode & 0x1F
    exp = exp - 32 if exp & 0x10 else exp
    return code * 2.0 ** exp


def linear11(v):
    exp = v >> 11
    exp = exp - 32 if exp & 0x10 else exp
    mant = v & 0x7FF
    mant = mant - 2048 if mant & 0x400 else mant
    return mant * 2.0 ** exp


def mv_of(code):
    return linear16(code) * 1000


# ---- PCI -----------------------------------------------------------------
def read_hex(path):
    with open(path) as f:
        return int(f.read().strip(), 16)


def find_asic():
    hits = [d for d in sorted(glob.glob(os.path.join(PCI_ROOT, "*")))
            if os.path.exists(os.path.join(d, "vendor"))
            and (read_hex(os.path.join(d, "vendor")), read_hex(os.path.join(d, "device"))) == ASIC_ID]
    if len(hits) != 1:
        raise Refuse(f"expected one ASIC {ASIC_ID[0]:04x}:{ASIC_ID[1]:04x} on PCI, found {len(hits)}")
    return hits[0]


def cfg_read(dev, off, size):
    with open(os.path.join(dev, "config"), "rb") as f:
        f.seek(off)
        b = f.read(size)
    if len(b) != size:
        raise Refuse(f"{os.path.basename(dev)}: config read at {off:#x} returned {len(b)} bytes")
    return int.from_bytes(b, "little")


def pcie_cap(dev):
    p = cfg_read(dev, 0x34, 1)
    for _ in range(48):
        if p < 0x40:
            break
        if cfg_read(dev, p, 1) == 0x10:
            return p
        p = cfg_read(dev, p + 1, 1)
    return None


def check_root_port(asic):
    """The root port must answer failed reads with all-ones (DWC AMBA error
    response 0x8d0 = 0, cs8210-pcie-errors) and have the link up."""
    rp = os.path.dirname(os.path.realpath(asic))
    name = os.path.basename(rp)
    if not os.path.exists(os.path.join(rp, "config")):
        raise Refuse(f"ASIC parent {name} is not a PCI device")
    amba = cfg_read(rp, 0x8D0, 4)
    if amba != 0:
        raise Refuse(f"root port {name}: AMBA error response 0x8d0 = {amba:#x}, not 0 "
                     "(cs8210-pcie-errors not run): a failed BAR read would be an SError")
    cap = pcie_cap(rp)
    if cap is None:
        raise Refuse(f"root port {name}: no PCIe capability")
    lnkcap, lnksta = cfg_read(rp, cap + 0x0C, 4), cfg_read(rp, cap + 0x12, 2)
    if lnkcap & (1 << 20) and not lnksta & (1 << 13):
        raise Refuse(f"root port {name}: link to the ASIC is down (LnkSta {lnksta:#06x})")
    return name


def read_fuses_bar(asic):
    """Four 32-bit loads at BAR0 + 0x11178, nothing else."""
    bdf = os.path.basename(asic)
    cmd = cfg_read(asic, 0x04, 2)
    if not cmd & 0x2:
        log(f"{bdf}: memory decode off (COMMAND {cmd:#06x}); WRITE: echo 1 > enable")
        with open(os.path.join(asic, "enable"), "w") as f:
            f.write("1")
        cmd = cfg_read(asic, 0x04, 2)
        if not cmd & 0x2:
            raise Refuse(f"{bdf}: memory decode still off after enable (COMMAND {cmd:#06x})")
    with open(os.path.join(asic, "resource")) as f:
        start, end, flags = (int(x, 16) for x in f.readline().split())
    if start == 0 or not flags & 0x200:
        raise Refuse(f"{bdf}: BAR0 unassigned or not memory ({start:#x} flags {flags:#x})")
    size = end - start + 1
    page = FUSE_OFFSET & ~(mmap.PAGESIZE - 1)
    if FUSE_OFFSET + 4 * FUSE_WORDS > size or page + mmap.PAGESIZE > size:
        raise Refuse(f"{bdf}: BAR0 size {size:#x} too small for the fuse words")
    fd = os.open(os.path.join(asic, "resource0"), os.O_RDONLY | os.O_SYNC)
    try:
        try:
            m = mmap.mmap(fd, mmap.PAGESIZE, mmap.MAP_SHARED, mmap.PROT_READ, offset=page)
        except OSError as e:
            raise Refuse(f"{bdf}: cannot map BAR0 ({e}; a driver owns it?)")
        try:
            # memoryview.cast("I"): one 32-bit load per element (byte-wise
            # access, as struct/slicing would do, is not safe on this SoC)
            mv = memoryview(m).cast("I")
            base = (FUSE_OFFSET - page) // 4
            words = []
            for i in range(FUSE_WORDS):
                words.append(mv[base + i])
            mv.release()
        finally:
            m.close()
    finally:
        os.close(fd)
    if any(w == 0xFFFFFFFF for w in words):
        raise Refuse(f"{bdf}: fuse read returned all-ones ({fmt_words(words)}): ASIC not answering")
    return words


def fmt_words(words):
    return ",".join(f"{w:#010x}" for w in words)


def parse_words(s):
    w = [int(x, 16) for x in s.replace(",", " ").split()]
    if len(w) != FUSE_WORDS or any(not 0 <= x <= 0xFFFFFFFF for x in w):
        raise ValueError(f"need 4 32-bit hex words, got {s!r}")
    return w


def read_kv(path):
    kv = {}
    with open(path) as f:
        for line in f:
            if "=" in line:
                k, v = line.rstrip("\n").split("=", 1)
                kv[k] = v
    return kv


def write_kv(path, kv):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        for k, v in kv.items():
            f.write(f"{k}={v}\n")
        f.flush()
        os.fsync(f.fileno())
    os.rename(tmp, path)


def boot_id():
    with open("/proc/sys/kernel/random/boot_id") as f:
        return f.read().strip()


def get_fuses(asic, args):
    """(words, source)."""
    if args.fuse_words:
        return parse_words(args.fuse_words), "command line"
    drv = os.path.join(asic, "driver")
    if os.path.exists(drv):
        name = os.path.basename(os.path.realpath(drv))
        # IO_STRICT_DEVMEM: no resource0 mmap while a driver holds the BAR
        for path, what in ((STATE, "this boot's state file"), (CACHE, "the last BAR read")):
            if os.path.exists(path):
                kv = read_kv(path)
                if "fuse_words" in kv:
                    log(f"{os.path.basename(asic)}: bound to {name}, BAR not mappable; fuse words from {what} ({path}, read {kv.get('time', '?')})")
                    return parse_words(kv["fuse_words"]), f"{path}"
        raise Refuse(f"{os.path.basename(asic)}: bound to {name} and no recorded fuse words "
                     f"({STATE}, {CACHE}); stop the switch stack and rmmod {name}, then run again")
    words = read_fuses_bar(asic)
    try:
        if os.path.exists(CACHE):
            old = read_kv(CACHE).get("fuse_words")
            if old and parse_words(old) != words:
                log(f"WARNING: fuse words {fmt_words(words)} differ from the last BAR read {old} ({CACHE})")
        write_kv(CACHE, {"time": now(), "boot_id": boot_id(), "fuse_words": fmt_words(words)})
    except OSError as e:
        log(f"WARNING: cannot record the fuse words in {CACHE}: {e}")
    return words, "bar"


def now():
    return time.strftime("%Y-%m-%dT%H:%M:%S%z")


# ---- I2C / PMBus -------------------------------------------------------------
I2C_SLAVE = 0x0703
I2C_SMBUS = 0x0720
SMBUS_READ, SMBUS_WRITE = 1, 0
SMBUS_BYTE_DATA, SMBUS_WORD_DATA, SMBUS_I2C_BLOCK_DATA = 2, 3, 8


class SmbusData(ctypes.Union):
    _fields_ = [("byte", ctypes.c_uint8), ("word", ctypes.c_uint16), ("block", ctypes.c_uint8 * 34)]


class SmbusIoctl(ctypes.Structure):
    _fields_ = [("read_write", ctypes.c_uint8), ("command", ctypes.c_uint8),
                ("size", ctypes.c_uint32), ("data", ctypes.POINTER(SmbusData))]


def find_bus(timeout=60.0):
    """Linux bus number of 0x72 channel 2, found from the mux's DT node."""
    deadline = time.monotonic() + timeout
    while True:
        for dev in glob.glob("/sys/bus/i2c/devices/*-0072"):
            node = os.path.join(dev, "of_node")
            if os.path.exists(node) and os.path.realpath(node).endswith("/" + MUX72_NODE):
                ch = os.path.join(dev, f"channel-{MUX72_CHANNEL}")
                if os.path.exists(ch):
                    bus = int(os.path.basename(os.path.realpath(ch)).split("-", 1)[1])
                    if os.path.exists(f"/dev/i2c-{bus}"):
                        return bus
        if time.monotonic() > deadline:
            raise Refuse(f"I2C mux {MUX72_NODE} channel {MUX72_CHANNEL} (or its /dev/i2c-N) not found")
        time.sleep(0.5)


class Pmbus:
    def __init__(self, bus, addr=REG_ADDR):
        self.bus, self.addr = bus, addr
        claimed = f"/sys/bus/i2c/devices/{bus}-{addr:04x}"
        if os.path.exists(os.path.join(claimed, "driver")):
            raise Refuse(f"i2c-{bus} {addr:#04x} is bound to a kernel driver "
                         f"({os.path.basename(os.path.realpath(os.path.join(claimed, 'driver')))}); not touching it")
        self.fd = os.open(f"/dev/i2c-{bus}", os.O_RDWR)
        try:
            fcntl.ioctl(self.fd, I2C_SLAVE, addr)   # not _FORCE: EBUSY if a driver claims it
        except OSError as e:
            os.close(self.fd)
            raise Refuse(f"i2c-{bus} {addr:#04x}: {e} (claimed by a kernel driver?)")

    def close(self):
        os.close(self.fd)

    def _xfer(self, rw, cmd, size, data):
        arg = SmbusIoctl(rw, cmd, size, ctypes.pointer(data))
        fcntl.ioctl(self.fd, I2C_SMBUS, arg)

    def read(self, cmd):
        if cmd not in PMBUS_READ:
            raise ValueError(f"PMBus command {cmd:#04x} is not on the read whitelist")
        n = PMBUS_READ[cmd][1]
        d = SmbusData()
        if n == 1:
            self._xfer(SMBUS_READ, cmd, SMBUS_BYTE_DATA, d)
            return d.byte
        if n == 2:
            self._xfer(SMBUS_READ, cmd, SMBUS_WORD_DATA, d)
            return d.word
        d.block[0] = n
        self._xfer(SMBUS_READ, cmd, SMBUS_I2C_BLOCK_DATA, d)
        return bytes(d.block[1:1 + n])

    def write(self, cmd, value):
        if cmd not in PMBUS_WRITE or (cmd == 0x00 and value != 0):
            raise ValueError(f"PMBus write {cmd:#04x}={value:#x} is not allowed")
        d = SmbusData()
        if cmd == 0x00:
            d.byte = value
            self._xfer(SMBUS_WRITE, cmd, SMBUS_BYTE_DATA, d)
        else:
            d.word = value
            self._xfer(SMBUS_WRITE, cmd, SMBUS_WORD_DATA, d)


def open_regulator(identify=True):
    bus = find_bus()
    pm = Pmbus(bus)
    if identify:
        ident = pm.read(0xAD)
        if ident[0] != len(IC_DEVICE_ID) or ident[1:] != IC_DEVICE_ID:
            pm.close()
            raise Refuse(f"i2c-{bus} {REG_ADDR:#04x}: IC_DEVICE_ID {ident.hex(' ')}, expected TPS53688 "
                         f"({len(IC_DEVICE_ID):02x} {IC_DEVICE_ID.hex(' ')})")
    return pm


def lock(exclusive=True):
    fd = os.open(LOCK, os.O_RDONLY | (os.O_CREAT if exclusive else 0), 0o644)
    fcntl.flock(fd, fcntl.LOCK_EX if exclusive else (fcntl.LOCK_SH | fcntl.LOCK_NB))
    return fd


# ---- commands --------------------------------------------------------------
def cmd_apply(args):
    lk = lock() if not args.dry_run else None
    asic = find_asic()
    if not args.fuse_words:
        rp = check_root_port(asic)
        log(f"ASIC {os.path.basename(asic)} behind {rp}: all-ones error response on, link up")
    words, source = get_fuses(asic, args)
    d = decode(words)
    log(f"fuses {fmt_words(words)} ({source}): RoV {d['rov']}, lot {lot_str(d['lot'])}, "
        f"target {d['target_mv']} mV -> {d['mv']} mV, VOUT_COMMAND {d['code']:#06x}")
    check_target(d)
    pm = open_regulator()
    try:
        page = pm.read(0x00)
        if page != 0:
            if args.dry_run:
                raise Refuse(f"PAGE is {page}, would write 0 (dry run: stopping)")
            log(f"PAGE {page}: WRITE PAGE=0")
            pm.write(0x00, 0)
            time.sleep(0.001)
            if pm.read(0x00) != 0:
                raise Refuse("PAGE does not read back 0")
        mode = pm.read(0x20)
        if mode != VOUT_MODE_LINEAR_M10:
            raise Refuse(f"VOUT_MODE {mode:#04x}, expected {VOUT_MODE_LINEAR_M10:#04x} (linear, exponent -10)")
        vmax = pm.read(0x24)
        cur = pm.read(0x21)
        rv = pm.read(0x8B)
        log(f"regulator: VOUT_MODE {mode:#04x}, VOUT_MAX {vmax:#06x} ({mv_of(vmax):.1f} mV), "
            f"VOUT_COMMAND {cur:#06x} ({mv_of(cur):.1f} mV), READ_VOUT {rv:#06x} ({mv_of(rv):.1f} mV)")
        if d["code"] > vmax:
            raise Refuse(f"target code {d['code']:#06x} above VOUT_MAX {vmax:#06x}")
        wrote = 0
        if cur == d["code"] and not args.rewrite:
            log("VOUT_COMMAND already correct; no write")
        elif args.dry_run:
            log(f"dry run: would write VOUT_COMMAND {cur:#06x} -> {d['code']:#06x}")
        else:
            why = "same value (--rewrite)" if cur == d["code"] else "differs"
            log(f"WRITE VOUT_COMMAND {cur:#06x} ({mv_of(cur):.1f} mV) -> {d['code']:#06x} ({d['mv']} mV): {why}")
            pm.write(0x21, d["code"])
            wrote = 1
            back = pm.read(0x21)
            if back != d["code"]:
                raise Refuse(f"VOUT_COMMAND reads back {back:#06x} after writing {d['code']:#06x}")
            # settle: TRANSITION_RATE here is 10 mV/us; give it up to 1 s
            for _ in range(100):
                rv = pm.read(0x8B)
                if abs(rv - d["code"]) <= 16:
                    break
                time.sleep(0.01)
            else:
                raise Refuse(f"READ_VOUT {rv:#06x} not within 16 codes of {d['code']:#06x} after 1 s")
        after = pm.read(0x21)
        rv = pm.read(0x8B)
        sw = pm.read(0x79)
        log(f"VOUT_COMMAND {after:#06x} ({mv_of(after):.1f} mV), READ_VOUT {rv:#06x} ({mv_of(rv):.1f} mV), STATUS_WORD {sw:#06x}")
        if sw & 0x8000:
            raise Refuse(f"STATUS_WORD {sw:#06x}: VOUT fault/warning bit set")
        if abs(rv - d["code"]) > 16:
            raise Refuse(f"READ_VOUT {rv:#06x} not within 16 codes of the target {d['code']:#06x}")
    finally:
        pm.close()
    if args.dry_run:
        log("dry run: no state file")
        return 0
    write_kv(STATE, {
        "time": now(), "boot_id": boot_id(), "fuse_source": source, "fuse_words": fmt_words(words),
        "rov": d["rov"], "lot": lot_str(d["lot"]), "target_mv": d["mv"], "vout_code": f"{d['code']:#06x}",
        "vout_mode": f"{mode:#04x}", "vout_command_before": f"{cur:#06x}", "vout_command_after": f"{after:#06x}",
        "read_vout_after": f"{rv:#06x}", "wrote": wrote,
    })
    log(f"recorded {STATE}")
    if lk is not None:
        os.close(lk)
    return 0


def readings(pm):
    r = {}
    for c in (0x00, 0x20, 0x21, 0x24, 0x40, 0x79, 0x7A, 0x7E, 0x88, 0x89, 0x8B, 0x8C, 0x8D, 0x96, 0x97):
        try:
            r[c] = pm.read(c)
        except OSError:
            r[c] = None
    return r


def cmd_status(args):
    if os.path.exists(STATE):
        log(f"{STATE}:")
        for k, v in read_kv(STATE).items():
            log(f"  {k}={v}")
    else:
        log(f"{STATE}: missing (AVS not applied this boot)")
    pm = open_regulator()
    try:
        r = readings(pm)
    finally:
        pm.close()
    mode = r[0x20] if r[0x20] is not None else VOUT_MODE_LINEAR_M10
    for c, v in r.items():
        name = PMBUS_READ[c][0]
        if v is None:
            log(f"  {name:20s} read failed")
        elif c in (0x21, 0x24, 0x40, 0x8B):
            log(f"  {name:20s} {v:#06x}  {linear16(v, mode):.4f} V")
        elif c in (0x88, 0x89, 0x8C, 0x8D, 0x96, 0x97):
            log(f"  {name:20s} {v:#06x}  {linear11(v):.3f}")
        else:
            log(f"  {name:20s} {v:#x}")
    return 0


def cmd_metrics(args):
    out = []

    def g(name, help_, value, labels=""):
        out.append(f"# HELP cs8210_asic_avs_{name} {help_}")
        out.append(f"# TYPE cs8210_asic_avs_{name} gauge")
        out.append(f"cs8210_asic_avs_{name}{labels} {value}")

    st = read_kv(STATE) if os.path.exists(STATE) else None
    g("applied", "1 when cs8210-asic-avs recorded the ASIC core voltage this boot.", 1 if st else 0)
    if st:
        g("target_volts", "ASIC core voltage decoded from the fuses (vendor table).", int(st["target_mv"]) / 1000)
        g("rov", "Ring-oscillator bin from the ASIC fuses.", int(st["rov"]))
        g("wrote", "1 when this boot's apply wrote VOUT_COMMAND.", int(st["wrote"]))
        lot = st["lot"].replace("\\", "\\\\").replace('"', '\\"')
        g("info", "ASIC fuse lot ID and target code; always 1.", 1, f'{{lot="{lot}",vout_code="{st["vout_code"]}"}}')
    lk = None
    try:
        if os.path.exists(LOCK):
            lk = lock(exclusive=False)
        pm = open_regulator(identify=False)
        try:
            r = readings(pm)
        finally:
            pm.close()
    except (Refuse, OSError) as e:
        print("\n".join(out))
        print(f"cs8210-asic-avs metrics: no regulator readings: {e}", file=sys.stderr)
        return 0
    finally:
        if lk is not None:
            os.close(lk)
    ok = r[0x00] == 0 and r[0x20] == VOUT_MODE_LINEAR_M10
    g("pmbus_ok", "1 when the regulator answered with PAGE 0 and VOUT_MODE 0x16.", 1 if ok else 0)
    if ok:
        for c, name, help_ in ((0x21, "vout_command_volts", "Regulator VOUT_COMMAND."),
                               (0x8B, "read_vout_volts", "Regulator READ_VOUT.")):
            if r[c] is not None:
                g(name, help_, f"{linear16(r[c]):.5f}")
        if r[0x21] is not None:
            g("vout_command_code", "Regulator VOUT_COMMAND, raw linear16 code.", r[0x21])
        for c, name, help_ in ((0x8C, "read_iout_amperes", "Regulator READ_IOUT."),
                               (0x8D, "read_temperature_celsius", "Regulator READ_TEMPERATURE_1."),
                               (0x88, "read_vin_volts", "Regulator READ_VIN."),
                               (0x96, "read_pout_watts", "Regulator READ_POUT.")):
            if r[c] is not None:
                g(name, help_, f"{linear11(r[c]):.3f}")
        if r[0x79] is not None:
            g("status_word", "Regulator STATUS_WORD (raw).", r[0x79])
    print("\n".join(out))
    return 0


def cmd_decode(args):
    words = parse_words(" ".join(args.words))
    d = decode(words)
    print(f"RoV {d['rov']}, lot {lot_str(d['lot'])}, target {d['target_mv']} mV, closest {d['mv']} mV, VOUT_COMMAND {d['code']:#06x}")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(prog="cs8210-asic-avs", description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("apply", help="read fuses, set VOUT_COMMAND if needed, record the state file")
    a.add_argument("--dry-run", action="store_true", help="no writes at all (no PAGE/VOUT_COMMAND, no state file)")
    a.add_argument("--rewrite", action="store_true", help="write VOUT_COMMAND even when it already holds the target (same value)")
    a.add_argument("--fuse-words", help="W0,W1,W2,W3 instead of reading the ASIC (testing)")
    sub.add_parser("status", help="regulator readings + state file")
    sub.add_parser("metrics", help="node_exporter textfile metrics (reads only)")
    dd = sub.add_parser("decode", help="decode fuse words offline")
    dd.add_argument("words", nargs=4)
    args = ap.parse_args(argv)
    try:
        return {"apply": cmd_apply, "status": cmd_status, "metrics": cmd_metrics, "decode": cmd_decode}[args.cmd](args)
    except Refuse as e:
        print(f"cs8210-asic-avs: REFUSED: {e}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    sys.exit(main())
