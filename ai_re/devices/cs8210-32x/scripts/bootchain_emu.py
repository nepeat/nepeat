#!/usr/bin/env python3
"""CS8210 (Alpine V3): run the factory stage2 and stage3 3.65.1 in Unicorn against a 32 MiB SPI
image and report what they would boot. Static verification of scheme A' (boot-chain.md section 7,
spi-upgrade-plan.md): the real TOC search and validation, stage2's DT/STG3 load and measured boot,
stage3's BOOT_MODE, key poll, app lookup, header/data checks and app load. No hardware.

The code comes from the image under test (stage2 = the S2 image at 0, stage3 = what the emulated
stage2 loads to 0x01000000) and must hash to the dumped factory stages (analysis/boot-chain/, vendor
code, git-ignored); this file holds addresses only. Stage2 runs from its C entry 0xff00ef88 (bootstrap
parse of the real strap word, console, main). Stage3 runs its app sequence 0x01037500 (board init,
then the app loader) with stage2's PBS SRAM (shared flags) and DRAM carried over.

Hardware calls are stubbed (return 0), each with its reason in STUBS2/STUBS3; MMIO pages that are
touched anyway are mapped as zero RAM and listed in the result. Stage2's SHA-256 is computed by
hashlib (the emulated code's digests matched it). UART0 is modelled: the console text is the
stages' own output; queued RX bytes are what stage3's key poll drains (keys sent from stage2's
banner on the box).

  bootchain_emu.py run IMAGE [--key u] [--ram]   one boot; --ram: stage3's UART-recovery (Route A)
                                                 reader, flash offset X at DRAM 0x01800000 + X
  bootchain_emu.py selftest                      the factory dump, scheme A' and its torn variants
                                                 (needs spi_upgrade.py build inputs), PASS/FAIL
"""
import argparse, hashlib, json, pathlib, re, struct, sys
from unicorn import Uc, UcError, UC_ARCH_ARM, UC_MODE_ARM, UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED
from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
                               UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_CPSR,
                               UC_CPU_ARM_CORTEX_A15)

ROOT = pathlib.Path(__file__).resolve().parent
DEV = ROOT.parent
FLASH = 0x2000000
STG2_LEN = 0x1B099                       # S2 header 0x20 + payload 0x1b079
STG2_SHA = "68de6763970b92b47b99c1aee688894c79a3dc019490719ebef3cab522251268"
STG3_SHA = "d48ceab8fe1a512f591b5cf3d53ed6b3abdb3239e8128b797c72d7054286c0a8"   # STG3 data
STRAP = 0x377FEEEF                       # PBS 0xfd8a8110 on this unit (PROGRESS 2026-11-15)
SRAM, SHARED = 0xFBFE0000, 0xFBFE0180    # PBS SRAM; general shared data (load-enforce flags +0x41)
UART0, PBS = 0xFD883000, 0xFD8A8000
MAGIC_LR = 0x0FFF0000

# stage2 (Thumb): function -> why it is stubbed
STUBS2 = {
    0xFF009CA4: "PLL init (polls hardware); on error stage2 keeps its default clock",
    0xFF00EF80: "timer set-up from the PLL rate",
    0xFF002C90: "early console/timer init (output is modelled at the UART instead)",
    0xFF00F2A8: "clock/board init",
    0xFF010B10: "uC mailbox init (in 0xff00f574; the shared-data writes there run)",
    0xFF010C84: "uC query (in 0xff00f574)",
    0xFF00FBE0: "memory repair / BIST",
    0xFF010984: "secure_boot_early_flow (OTP read, TPM); OTP secure boot is off on this unit",
    0xFF0098EC: "DDR init", 0xFF009B8C: "DDR init",
    0xFF00FCF8: "post-DDR init", 0xFF0110E4: "post-DDR init", 0xFF010FF8: "post-DDR init",
    0xFF00FEE4: "post-DDR init", 0xFF0081D0: "fabric query",
}
# 0xff0109cc (TPM/uC flow) is redirected straight into the measurement 0xff01072c (the legacy
# flow's call, measurement errors flag 0); 0xff010400 (TPM PCR extend) is recorded and returns 0.
S2_PRINT, S2_EXEC, S2_MEAS_FLOW, S2_MEASURE, S2_EXTEND = 0xFF000F14, 0xFF00EF58, 0xFF0109CC, 0xFF01072C, 0xFF010400
S2_SHA = (0xFF002ABC, 0xFF002BD8, 0xFF002B14)        # sha256 init(ctx) / update(ctx, p, n) / final(ctx, out)
S2_SPIN = 0xFF00F094                                  # main's "stop forever" loop

STUBS3 = {
    0x01039BF8: "board pre-init", 0x010230D0: "NB PLL read (on error: 'PLL init failed' print only)",
    0x0103753C: "I2C init",
    0x01037EB4: "pre-DT hook", 0x01039984: "DT-based init (MUIO, GPIO)", 0x0103ACC0: "ETH init",
    0x0103ABF4: "SerDes init + firmware download", 0x01024478: "CPU handles", 0x010244FC: "CPU handles",
    0x01023B70: "secondary CPU power", 0x01023AE0: "secondary CPU power", 0x01005100: "console flush",
    0x01037558: "board EEPROM read (rev id)", 0x01036B70: "DRAM clear/compare (would also wipe a Route A image above 0x02000000)",
    0x010014A4: "stack-usage report",
}
S3_PRINT, S3_START = 0x0100143C, 0x01037500
S3_POLL_DONE = 0x010361F8                             # board init: TOC/BOOT_MODE/DT/app lookups done
S3_EXEC, S3_SPIN = 0x010371FC, 0x01037198             # app loaded / "stop forever"
G_APP, G_APPHDR_OFF, G_APPHDR_OK = 0x01081888, 0x0107E720, 0x0107E724
G_APPHDR, G_BOOTDEV, G_SHARED_PTR = 0x0107E6D8, 0x01081270, 0x01080E1C
G_UART0 = 0x0107D21C                                  # stage3's UART handle table, entry 0 = base


def u32(b, o=0):
    return struct.unpack_from("<I", b, o)[0]


class Machine:
    def __init__(self, image, sram=None, dram=None, keys=b""):
        uc = self.uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        uc.ctl_set_cpu_model(UC_CPU_ARM_CORTEX_A15)
        from unicorn.arm_const import UC_ARM_REG_C1_C0_2, UC_ARM_REG_FPEXC
        uc.reg_write(UC_ARM_REG_C1_C0_2, uc.reg_read(UC_ARM_REG_C1_C0_2) | (0xF << 20))   # CPACR: VFP/NEON
        uc.reg_write(UC_ARM_REG_FPEXC, 0x40000000)                                         # FPEXC.EN
        for base, size in ((0xFF000000, 0x80000), (SRAM, 0x10000), (0, 0x10000000),
                           (0xF0070000, 0x30000), (0xF0400000, 0x200000), (0xF8000000, FLASH)):
            uc.mem_map(base, size)
        uc.mem_write(0xF0074400, struct.pack("<I", 0x0400))   # NB version 4 (sys fabric handle)
        uc.mem_write(0xF8000000, bytes(image))                  # SPI direct map (both stages read here)
        if sram:
            uc.mem_write(SRAM, sram)
        for base, data in (dram or {}).items():
            uc.mem_write(base, data)
        self.tx, self.rx, self.prints, self.auto, self.notes = bytearray(), list(keys), [], {}, []
        uc.mmio_map(UART0, 0x1000, self.uart_r, None, self.uart_w, None)
        pbs = {0x110: STRAP, 0x278: 0}
        uc.mmio_map(PBS, 0x1000, lambda uc, o, s, _: pbs.get(o, 0), None, lambda *a: None, None)
        uc.hook_add(UC_HOOK_MEM_UNMAPPED, self.unmapped)
        self.stop, self.stop_on_mmio = None, None

    def uart_r(self, uc, off, size, _):
        if off == 0x14:
            return 0x60 | (1 if self.rx else 0)     # THR empty, data ready while keys are queued
        if off == 0 and self.rx:
            return self.rx.pop(0)
        return 0

    def uart_w(self, uc, off, size, val, _):
        if off == 0:
            self.tx.append(val & 0xFF)

    def unmapped(self, uc, access, addr, size, value, _):
        page = addr & ~0xFFF
        if self.stop_on_mmio and self.stop_on_mmio():
            self.stop = f"stopped at the first unmodelled access after the probe point ({addr:#x})"
            uc.emu_stop()
        self.auto[page] = self.auto.get(page, 0) + 1
        uc.mem_map(page, 0x1000)
        return True

    def reg(self, r):
        return self.uc.reg_read(r)

    def ret(self, r0=0):
        self.uc.reg_write(UC_ARM_REG_R0, r0 & 0xFFFFFFFF)
        self.uc.reg_write(UC_ARM_REG_PC, self.reg(UC_ARM_REG_LR))

    def at(self, addr, fn):
        self.uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, _: fn(), begin=addr, end=addr)

    def stub(self, addrs):
        for a in addrs:
            self.at(a, self.ret)

    def cstr(self, a, n=300):
        try:
            return bytes(self.uc.mem_read(a, n)).split(b"\0")[0].decode("latin-1")
        except UcError:
            return "?"

    def printf(self):
        """Render printf(level, fmt, ...) (AAPCS: r2, r3, then the stack)."""
        lvl, fmt = self.reg(UC_ARM_REG_R0), self.cstr(self.reg(UC_ARM_REG_R1))
        args = [self.reg(UC_ARM_REG_R2), self.reg(UC_ARM_REG_R3)]
        sp = self.reg(UC_ARM_REG_SP)
        args += list(struct.unpack("<8I", self.uc.mem_read(sp, 32)))
        it = iter(args)

        def conv(m):
            flags, width, c = m.group(1), m.group(2), m.group(4)
            if c == "%":
                return "%"
            v = next(it)
            if c == "s":
                return self.cstr(v)
            if c == "c":
                return chr(v & 0xFF)
            spec = "%" + flags + width + ({"u": "d", "p": "x"}.get(c, c))
            return spec % (v if c != "d" else struct.unpack("<i", struct.pack("<I", v))[0])
        try:
            text = re.sub(r"%([-0 #+]*)(\d*)(l{0,2})([diouxXscp%])", conv, fmt)
        except (StopIteration, ValueError, TypeError):
            text = fmt
        self.prints.append((lvl, text.strip()))

    def run(self, start, count):
        uc = self.uc
        uc.reg_write(UC_ARM_REG_CPSR, 0x1D3 | (0x20 if start & 1 else 0))
        uc.reg_write(UC_ARM_REG_LR, MAGIC_LR | 1)
        try:
            uc.emu_start(start, MAGIC_LR, count=count)
        except UcError as e:
            self.stop = self.stop or f"emulation error {e} at pc {self.reg(UC_ARM_REG_PC):#x}"
        if self.stop is None:
            self.stop = f"instruction budget used up at pc {self.reg(UC_ARM_REG_PC):#x}"


def stage2(image):
    """Factory stage2 from 0xff00ef88. Returns its findings plus SRAM/DRAM for stage3."""
    s2 = bytes(image[:STG2_LEN])
    if hashlib.sha256(s2).hexdigest() != STG2_SHA:
        raise SystemExit("the image's stage2 is not the dumped factory stage2 v3.65.1")
    m = Machine(image)
    m.uc.mem_write(0xFF000000, s2)
    m.uc.reg_write(UC_ARM_REG_SP, 0xFF07F000)
    m.stub(STUBS2)
    m.at(S2_PRINT, m.printf)
    extends, shas = [], {}

    def meas_flow():                     # 0xff0109cc(reader, instance, toc) -> 0xff01072c(reader, toc, 0)
        m.uc.reg_write(UC_ARM_REG_R1, m.reg(UC_ARM_REG_R2))
        m.uc.reg_write(UC_ARM_REG_R2, 0)
        m.uc.reg_write(UC_ARM_REG_PC, S2_MEASURE | 1)

    def extend():
        extends.append((m.reg(UC_ARM_REG_R0), bytes(m.uc.mem_read(m.reg(UC_ARM_REG_R1) + 4, 32)).hex()))
        m.ret(0)

    def sha_init():
        shas[m.reg(UC_ARM_REG_R0)] = hashlib.sha256()
        m.ret(0)

    def sha_upd():
        shas[m.reg(UC_ARM_REG_R0)].update(bytes(m.uc.mem_read(m.reg(UC_ARM_REG_R1), m.reg(UC_ARM_REG_R2))))
        m.ret(0)

    def sha_fin():
        m.uc.mem_write(m.reg(UC_ARM_REG_R1), shas.pop(m.reg(UC_ARM_REG_R0)).digest())
        m.ret(0)

    def exec_next():
        m.stop = f"exec {m.reg(UC_ARM_REG_R0):#x}"
        m.uc.emu_stop()

    def spin():
        m.stop = "spin (stage2 stops forever)"
        m.uc.emu_stop()
    m.at(S2_MEAS_FLOW, meas_flow)
    m.at(S2_EXTEND, extend)
    for a, f in zip(S2_SHA, (sha_init, sha_upd, sha_fin)):
        m.at(a, f)
    m.at(S2_EXEC, exec_next)
    m.at(S2_SPIN, spin)
    m.run(0xFF00EF88 | 1, 400_000_000)
    stg3 = bytes(m.uc.mem_read(0x01000000, 0x7C539))
    return {"stop": m.stop, "prints": m.prints, "extends": extends, "auto": sorted(m.auto),
            "stg3_ok": hashlib.sha256(stg3).hexdigest() == STG3_SHA,
            "sram": bytes(m.uc.mem_read(SRAM, 0x10000)), "stg3": stg3}


def stage3(image, s2, keys=b"", ram=False, serdes=False):
    """Factory stage3 app sequence 0x01037500 with stage2's SRAM; keys = queued UART RX bytes.
    serdes: run the SerDes init 0x0103abf4 for real (its firmware lookups and loads read the
    TOC) and stop at its first hardware access after both firmware loads (the download)."""
    if not s2["stg3_ok"]:
        raise SystemExit("stage2 did not load the dumped factory stage3")
    dram = {0x01000000: s2["stg3"]}
    sram = bytearray(s2["sram"])
    if ram:                                   # Route A: flash image in DRAM, XMODEM_LOAD_ENFORCE only
        dram[0x01800000] = bytes(image[:0x800000])
        sram[SHARED - SRAM + 0x41] = (sram[SHARED - SRAM + 0x41] & ~0x3) | 0x1
    m = Machine(image if not ram else b"\xff" * FLASH, bytes(sram), dram, keys)
    m.uc.reg_write(UC_ARM_REG_SP, 0x01FF0000)
    m.uc.mem_write(G_SHARED_PTR, struct.pack("<I", SHARED))
    m.uc.mem_write(G_BOOTDEV, bytes([1 if ram else 2]))   # ROM boot device: 1 UART, 2 SPI
    m.uc.mem_write(G_UART0, struct.pack("<I", UART0))     # console handle (set by the skipped init)
    m.stub({k: v for k, v in STUBS3.items() if not (serdes and k == 0x0103ABF4)})
    m.at(S3_PRINT, m.printf)
    seen = {}
    if serdes:
        m.stop_on_mmio = lambda: sum("Loading SerDes" in t for _, t in m.prints) >= 2

    def lookups_done():
        seen["app_id"] = u32(m.uc.mem_read(G_APP, 4))
        seen["app_hdr_ok"] = u32(m.uc.mem_read(G_APPHDR_OK, 4))
        seen["app_off"] = u32(m.uc.mem_read(G_APPHDR_OFF, 4))
        seen["keys_left"] = len(m.rx)

    def exec_app():
        h = bytes(m.uc.mem_read(G_APPHDR, 0x48))
        load, ex, size = u32(h, 0x30), u32(h, 0x38), u32(h, 0x28)
        if not seen.get("app_hdr_ok"):
            load, ex = 0x01100000, 0x01100000     # raw path (size word at flash 0x40000)
        data = bytes(m.uc.mem_read(load, min(size, 0x200000)))
        seen.update(load=load, exec=ex, size=size, data_sha=hashlib.sha256(data).hexdigest())
        m.stop = "exec app"
        m.uc.emu_stop()

    def spin():
        m.stop = "spin (stage3 stops forever)"
        m.uc.emu_stop()
    m.at(S3_POLL_DONE, lookups_done)
    m.at(S3_EXEC, exec_app)
    m.at(S3_SPIN, spin)
    m.run(S3_START, 400_000_000)
    if serdes:
        seen["serdes_fw"] = {hex(a): hashlib.sha256(bytes(m.uc.mem_read(a, n))).hexdigest()
                             for a, n in ((0x01120000, 0x57D0), (0x01130000, 0x7570))}
    return {"stop": m.stop, "prints": m.prints, "auto": sorted(m.auto), **seen}


def serdes_probe(image):
    """The SerDes firmware stage3 loads (from whichever TOC is active), as sha256 by address."""
    image = bytes(image).ljust(FLASH, b"\xff")
    s3 = stage3(image, stage2(image), serdes=True)
    return {"loads": [t for _, t in s3["prints"] if "Loading SerDes" in t], "fw": s3.get("serdes_fw"), "stop": s3["stop"]}


def boot(image, keys=b"", ram=False):
    """Both stages; a one-line verdict plus details."""
    image = bytes(image).ljust(FLASH, b"\xff")
    s2 = stage2(image)
    r = {"stage2": {k: v for k, v in s2.items() if k not in ("sram", "stg3")}}
    if not s2["stop"].startswith("exec 0x1000000") or not s2["stg3_ok"]:
        r["verdict"] = "HANG in stage2: " + s2["stop"]
        return r
    s3 = stage3(image, s2, keys, ram)
    r["stage3"] = s3
    if s3["stop"] != "exec app":
        last = [t for lvl, t in s3["prints"] if lvl <= 2][-1:] or ["?"]
        r["verdict"] = f"HANG in stage3 ({last[0]})"
    else:
        r["verdict"] = (f"exec app {s3['app_id']:#x} from flash {s3['app_off']:#x}: load {s3['load']:#x} "
                        f"exec {s3['exec']:#x}, {s3['size']:#x} B, sha256 {s3['data_sha'][:16]}")
    return r


def obj_sha(image, off):
    size = u32(image, off + 0x28)
    return hashlib.sha256(bytes(image[off + 0x48:off + 0x48 + size])).hexdigest()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("run")
    p.add_argument("image")
    p.add_argument("--key", default="", help="queue these bytes for stage3's key poll (e.g. u)")
    p.add_argument("--ram", action="store_true", help="stage3 reads a RAM image (Route A)")
    p.add_argument("--json", action="store_true")
    a = ap.parse_args()
    img = pathlib.Path(a.image).read_bytes()
    r = boot(img, (a.key * 8).encode(), a.ram)
    if a.json:
        print(json.dumps(r, default=str))
        return
    for st in ("stage2", "stage3"):
        if st in r:
            print(f"-- {st}: {r[st]['stop']}")
            for lvl, t in r[st]["prints"]:
                print(f"   [{lvl}] {t}")
            if r[st]["auto"]:
                print(f"   MMIO pages touched (zero): {' '.join(hex(x) for x in r[st]['auto'])}")
    if "extends" in r["stage2"]:
        print(f"-- measured boot: {len(r['stage2']['extends'])} extends (PCR, sha256): " +
              ", ".join(f"{p}:{d[:8]}" for p, d in r["stage2"]["extends"]))
    print("VERDICT:", r["verdict"])


if __name__ == "__main__":
    main()
