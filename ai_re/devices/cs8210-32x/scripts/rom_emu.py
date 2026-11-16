#!/usr/bin/env python3
"""Run the CS8210 boot ROM 3.1.0 (Alpine V3) in Unicorn with UART0 on a pty, to test the UART
recovery paths and uart_recovery.py offline. No hardware. Evidence: ../uart-recovery.md.

The ROM dump is vendor code and stays in analysis/boot-rom/ (git-ignored); this script only
holds addresses. MMIO is modelled as far as the ROM needs it: UART0/1 (DW 16550, LSR/RBR/THR),
the PBS strap words, OTP (all zero = non-secure), NB/ANPA ids; unknown pages are mapped as zero
RAM on first touch and listed at the end. Hooked ROM functions: time-in-us (host clock), udelay,
the LLC-as-SRAM init, the MMU enable (kept off), the cache maintenance, the SPI 'GBS' reader
(blank flash or an mtd0 file), and the fatal stop.

  rom_emu.py run --mode uart|cli|spi|spi-blank|route-r [--mtd0 F] [--debug]   pty path on stdout
  rom_emu.py consoled-sim PTY           stdin/stdout <-> PTY with the attach prefix rules
  rom_emu.py selftest [--mtd0 F]        all modes against uart_recovery.py, prints PASS/FAIL
"""
import argparse, hashlib, os, pathlib, queue, select, struct, subprocess, sys, termios, threading, time, tty, zlib

from unicorn import (Uc, UcError, UC_ARCH_ARM, UC_ARCH_ARM64, UC_MODE_ARM, UC_HOOK_CODE,
                     UC_HOOK_MEM_UNMAPPED, UC_PROT_ALL)
from unicorn.arm_const import *
from unicorn.arm64_const import *

ROOT = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
import uart_recovery as ur  # noqa: E402

DEV = ROOT.parent
ROM_PATH = DEV / "analysis/boot-rom/rom-fbff0000.bin"
ROM_SHA256 = "7e1ec7ef55d1b851337d4c9155d6b726207b7d5f0d0ca3429f6dd657316bc5f0"
MTD0 = DEV / "images/spi-2026-11-15/mtd0-uboot.bin"

# boot_strap for this unit, rebuilt from dev_info (uart-recovery.md 1.1): cpu 0xf (2000 MHz),
# ddr 6 (1200), sb_pll 1 (3000), sb_clk 3 (500), preload 2 (SPI slave), boot device 7 (SPI M0),
# debug bit 18 = 1 (off), ref 100 MHz, 4 clusters, spim_sw_mode bits 23:22 = 1 with strap2 bit 0
# = 0 (4x50 MHz), preload att 1, dcache and LLC on (bits 30/31 = 0). Check on the box:
# 'md.l 0xfd8a8110 1' / 'md.l 0xfd8a8278 1' at the vendor prompt (uart-recovery.md step R2).
STRAP_UNIT = (0xF | 6 << 4 | 1 << 7 | 3 << 9 | 2 << 12 | 7 << 15 | 1 << 18 | 1 << 19 | 3 << 20
              | 1 << 22 | 1 << 26)
STRAP2_UNIT = 0
BOOTDEV = {"spi": 7, "spi-blank": 7, "uart": 4, "cli": 0, "route-r": 7}

H_TIME_US, H_UDELAY, H_FATAL, H_IOCACHE = 0xFFFF2070, 0xFFFF1FA4, 0xFFFF1334, 0xFFFF0680
H_SCTLR, H_CPUACTLR, H_TTBR, H_CACHE = 0xFFFF0338, 0xFFFF0284, 0xFFFF039C, 0xFFFF0148
H_GBS_INIT, H_GBS_READ, H_XMODEM_RET = 0xFFFF1658, 0xFFFF16EC, 0xFFFF0B5C
STAGE2_EXEC = 0xFF000020


class Uart:
    """DW APB UART, 32-bit register stride: RBR/THR 0x0, IER 0x4, IIR/FCR 0x8, LCR 0xc, LSR 0x14."""
    def __init__(self, name, out):
        self.name, self.out, self.rx, self.lcr, self.regs = name, out, queue.Queue(), 0, {}

    def read(self, uc, off, size, _):
        if off == 0x00 and not self.lcr & 0x80:
            try:
                return self.rx.get_nowait()
            except queue.Empty:
                return 0
        if off == 0x08:
            return 0x01                            # IIR: no FIFO bits -> the ROM polls LSR
        if off == 0x14:
            return 0x60 | (0 if self.rx.empty() else 1)
        if off == 0x0C:
            return self.lcr
        return self.regs.get(off, 0)

    def write(self, uc, off, size, val, _):
        if off == 0x00 and not self.lcr & 0x80:
            self.out(bytes([val & 0xFF]))
        elif off == 0x0C:
            self.lcr = val
        else:
            self.regs[off] = val


class Rom:
    def __init__(self, mode, mtd0=None, debug=False, log=None):
        rom = ROM_PATH.read_bytes()
        if hashlib.sha256(rom).hexdigest() != ROM_SHA256:
            raise SystemExit(f"{ROM_PATH}: not the dumped ROM 3.1.0 (sha256)")
        self.mode, self.log = mode, log or (lambda m: print(m, file=sys.stderr))
        self.mtd0 = pathlib.Path(mtd0).read_bytes() if mtd0 else None
        self.master, self.slave = os.openpty()
        tty.setraw(self.master)
        tty.setraw(self.slave)
        self.pty = os.ttyname(self.slave)
        self.result, self.automapped, self.t0 = None, [], time.monotonic()
        uc = self.uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        uc.ctl_set_cpu_model(UC_CPU_ARM_CORTEX_A15)
        for base, size in ((0, 0x04000000), (0xFBFE0000, 0x10000), (0xFBFF0000, 0x10000),
                           (0xFF000000, 0x80000), (0xFFFF0000, 0x10000), (0xF0070000, 0x30000),
                           (0xF0400000, 0x200000), (0xFD896000, 0x1000), (0xFD887000, 0x1000),
                           (0xFD892000, 0x1000)):
            uc.mem_map(base, size, UC_PROT_ALL)
        uc.mem_write(0xFFFF0000, rom)
        uc.mem_write(0xFBFF0000, rom)
        uc.mem_write(0xF0074400, struct.pack("<I", 0x0400))   # NB version 4 (cluster handles)
        self.uart0 = Uart("uart0", lambda b: os.write(self.master, b))
        self.uart1 = Uart("uart1", lambda b: sys.stderr.write(b.decode("latin-1")))
        for base, u in ((0xFD883000, self.uart0), (0xFD884000, self.uart1)):
            uc.mmio_map(base, 0x1000, u.read, None, u.write, None)
        strap = (STRAP_UNIT & ~(7 << 15)) | BOOTDEV[mode] << 15
        if debug:
            strap &= ~(1 << 18)
        self.pbs = {0x110: strap, 0x278: STRAP2_UNIT, 0x15C: 0x00030000}
        uc.mmio_map(0xFD8A8000, 0x1000, lambda uc, o, s, _: self.pbs.get(o, 0), None,
                    lambda uc, o, s, v, _: self.pbs.__setitem__(o, v), None)
        uc.hook_add(UC_HOOK_MEM_UNMAPPED, self.unmapped)
        hooks = {H_TIME_US: self.h_time, H_UDELAY: self.h_ret0, H_FATAL: self.h_fatal,
                 H_IOCACHE: self.h_ret0, H_CPUACTLR: self.h_ret0, H_TTBR: self.h_ret0,
                 H_CACHE: self.h_ret0, H_SCTLR: self.h_sctlr, STAGE2_EXEC: self.h_stage2}
        if mode in ("spi", "spi-blank", "cli"):
            hooks.update({H_GBS_INIT: self.h_ret0, H_GBS_READ: self.h_gbs_read})
        for a, f in hooks.items():
            uc.hook_add(UC_HOOK_CODE, f, begin=a, end=a)
        if mode == "route-r":
            blob = ur.route_r_blob()
            uc.mem_write(ur.R_BASE, blob)
            uc.mem_write(ur.PER_CORE0 + 0x30, struct.pack("<I", ur.R_BASE + ur.R_A32))  # EL3 stub's job
            q = ur.R_BASE + len(blob) - 8                      # the stub's wfi after 'Q'
            uc.hook_add(UC_HOOK_CODE, self.h_quit, begin=q, end=q)
        uc.reg_write(UC_ARM_REG_CPSR, 0x1D3)                  # SVC, A/I/F masked, ARM state
        threading.Thread(target=self.pump, daemon=True).start()

    # -- hooks --
    def ret(self, r0=None, r1=None):
        if r0 is not None:
            self.uc.reg_write(UC_ARM_REG_R0, r0 & 0xFFFFFFFF)
        if r1 is not None:
            self.uc.reg_write(UC_ARM_REG_R1, r1 & 0xFFFFFFFF)
        self.uc.reg_write(UC_ARM_REG_PC, self.uc.reg_read(UC_ARM_REG_LR))

    def h_time(self, uc, a, s, _):
        us = int((time.monotonic() - self.t0) * 1e6)
        self.ret(us, us >> 32)

    def h_ret0(self, uc, a, s, _):
        self.ret(0)

    def h_sctlr(self, uc, a, s, _):
        if uc.reg_read(UC_ARM_REG_R0):
            uc.reg_write(UC_ARM_REG_R0, 0)                    # keep the MMU off (identity anyway)

    def h_gbs_read(self, uc, a, s, _):
        off, dst, n = (uc.reg_read(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2))
        if self.mtd0 is None:
            data = b"\xff" * n
        else:
            data = self.mtd0[off:off + n].ljust(n, b"\xff")
        uc.mem_write(dst, data)
        self.ret(0)

    def h_fatal(self, uc, a, s, _):
        self.result = ("fatal", "ROM fatal stop (0xffff1334: Unrecoverable Error)")
        uc.emu_stop()

    def h_stage2(self, uc, a, s, _):
        img = bytes(uc.mem_read(0xFF000000, ur.ROM_UART_MAX))
        self.result = ("stage2", img)
        uc.emu_stop()

    def h_quit(self, uc, a, s, _):
        self.result = ("quit", None)
        uc.emu_stop()

    def unmapped(self, uc, access, addr, size, value, _):
        page = addr & ~0xFFF
        self.automapped.append(page)
        uc.mem_map(page, 0x1000, UC_PROT_ALL)
        return True

    def pump(self):
        while True:
            try:
                b = os.read(self.master, 4096)
            except OSError:
                return
            for x in b:
                self.uart0.rx.put(x)

    def run(self, timeout=None):
        if timeout:
            threading.Timer(timeout, self.uc.emu_stop).start()
        try:
            self.uc.emu_start(0xFFFF0000, 0xFFFFFFFF)
        except UcError as e:
            pc = self.uc.reg_read(UC_ARM_REG_PC)
            self.result = ("error", f"{e} at pc {pc:#x}")
        if self.result is None:
            self.result = ("timeout", None)
        pages = sorted(set(self.automapped))
        if pages:
            self.log("auto-mapped pages: " + " ".join(f"{p:#x}" for p in pages))
        return self.result


def cmd_run(a):
    r = Rom(a.mode, a.mtd0, a.debug)
    print(r.pty, flush=True)
    if a.link:
        if os.path.lexists(a.link):
            os.unlink(a.link)
        os.symlink(r.pty, a.link)
    kind, val = r.run(a.timeout)
    if kind == "stage2":
        print(f"ROM jumped to 0xff000020; 0xff000000.. sha256 {hashlib.sha256(val).hexdigest()[:16]}")
    else:
        print(kind, val if val else "")


def consoled_sim(path, replay=True):
    """Like 'line attach' piped: stdin -> port with the C-b / ^] prefix rules, port -> stdout.
    A lone prefix followed by another key is a command (dropped, reported on stderr)."""
    fd = os.open(path, os.O_RDWR | os.O_NOCTTY)
    tty.setraw(fd, termios.TCSANOW)        # TCSAFLUSH (the default) would drop pending output
    out = sys.stdout.buffer
    if replay:   # the scrollback replay an attach starts with, with stray 'C's and a hint
        out.write(b"...old output C C\r\n## Ready for binary (xmodem) download\r\nCCC\r\n"
                  b"\r\n[line] attached (write)\r\n")
        out.flush()
    pend, inp = None, sys.stdin.buffer.raw.fileno()
    while True:
        r, _, _ = select.select([inp, fd], [], [])
        if fd in r:
            out.write(os.read(fd, 4096))
            out.flush()
        if inp in r:
            b = os.read(inp, 4096)
            if not b:
                return
            data = bytearray()
            for x in b:
                if pend is not None:
                    if x in (0x02, 0x1D):
                        data.append(x)
                    elif x == ord("t"):
                        out.write(b"\r\n[line] write taken\r\n")
                        out.flush()
                    else:
                        sys.stderr.write(f"consoled-sim: prefix + {x:#04x} (unescaped byte!)\n")
                    pend = None
                elif x in (0x02, 0x1D):
                    pend = x
                else:
                    data.append(x)
            if data:
                os.write(fd, bytes(data))


def cmd_consoled_sim(a):
    consoled_sim(a.pty, not a.no_replay)


# ---- AArch64 parts of Route R: emulate the trampoline and the EL3 stub up to the reset ----
def check_a64():
    blob = ur.route_r_blob()
    uc = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
    uc.mem_map(0, 0x04000000, UC_PROT_ALL)
    writes = []
    for base in (0xF0400000, 0xF0410000, 0xFD883000):
        uc.mmio_map(base, 0x1000, lambda uc, o, s, _: 0, None,
                    lambda uc, o, s, v, _, b=base: writes.append((b + o, v)), None)
    uc.mem_write(ur.R_BASE, blob)
    stops = []

    def stop(uc, a, s, _):
        stops.append(a)
        uc.emu_stop()
    uc.hook_add(UC_HOOK_CODE, stop, begin=ur.R_BASE + 0x84, end=ur.R_BASE + 0x84)  # tramp wfi
    uc.emu_start(ur.R_BASE, ur.R_BASE + 0x1000, count=10 ** 8)
    ok1 = (bytes(uc.mem_read(ur.RVBAR, 0x60)) == blob[ur.R_EL3:ur.R_EL3 + 0x60]
           and writes == [(ur.ANPA + 0x24, ur.CPU0_POR)])
    w1 = list(writes)
    writes.clear()
    msr = ur.RVBAR + 0x38                                   # msr rmr_el3: stop before it
    uc.hook_add(UC_HOOK_CODE, stop, begin=msr, end=msr)
    uc.emu_start(ur.RVBAR, ur.RVBAR + 0x60, count=1000)
    x0 = uc.reg_read(UC_ARM64_REG_X0)
    ok2 = (writes == [(ur.UART0, ord("#")), (ur.PER_CORE0 + 0x30, ur.R_BASE + ur.R_A32),
                      (ur.PER_CORE0 + 0x34, 0)] and x0 == 2)
    return ok1 and ok2, f"tramp writes {[(hex(a), hex(v)) for a, v in w1]}, stub copied; " \
                        f"EL3 stub writes {[(hex(a), hex(v)) for a, v in writes]}, x0={x0} at msr rmr_el3"


# ---- selftest ----
def emu_thread(mode, mtd0=None, debug=False, timeout=400):
    r = Rom(mode, mtd0, debug, log=lambda m: print(f"    [emu] {m}"))
    box = {}
    th = threading.Thread(target=lambda: box.setdefault("res", r.run(timeout)), daemon=True)
    th.start()
    return r, th, box


def cmd_selftest(a):
    results = []

    def report(name, ok, detail=""):
        results.append(ok)
        print(f"{'PASS' if ok else 'FAIL'}  {name}  {detail}", flush=True)

    s2 = None
    if a.mtd0 and pathlib.Path(a.mtd0).exists():
        d = pathlib.Path(a.mtd0).read_bytes()
        s2 = d[:0x20 + struct.unpack_from("<I", d, 0xC)[0] + 2]
    payload = s2 or ur.test_file(0x1B09B)

    ok, detail = check_a64()
    report("route-r AArch64 trampoline + EL3 stub (emulated to the reset)", ok, detail)

    # 1. UART boot (strap 4) through a tty, the real XMODEM path of the ROM
    r, th, box = emu_thread("uart")
    t = ur.Tty(r.pty)
    try:
        naks = ur.xmodem_send(t, payload, wait=30, log=lambda m: print(f"    {m}"))
    finally:
        t.close()
    th.join(30)
    kind, img = box.get("res", ("timeout", None))
    report("UART boot (strap 4): ROM receives the S2 image at 0xff000000 and jumps to 0xff000020",
           kind == "stage2" and img[:len(payload)] == payload and naks == 0, f"{kind}, {naks} retries")

    # 2. SPI blank -> 16 offsets x 4 tries -> automatic UART fallback; via the consoled-sim pipe
    r, th, box = emu_thread("spi-blank")
    cmd = [sys.executable, str(ROOT / "rom_emu.py"), "consoled-sim", r.pty]
    t = ur.Pipe(cmd, escape=b"\x02\x1d", take=True)
    try:
        naks = ur.xmodem_send(t, payload, wait=60, log=lambda m: print(f"    {m}"))
    finally:
        t.close()
    th.join(30)
    kind, img = box.get("res", ("timeout", None))
    report("blank SPI: fallback to UART XMODEM, through consoled-sim (escaped prefixes, replay)",
           kind == "stage2" and img[:len(payload)] == payload and naks == 0, f"{kind}, {naks} retries")

    # 3. SPI with the real mtd0: the ROM's own S2 checks accept the factory stage2
    if s2:
        r, th, box = emu_thread("spi", a.mtd0, debug=True)
        th.join(60)
        kind, img = box.get("res", ("timeout", None))
        log = b""
        while select.select([r.slave], [], [], 0.2)[0]:
            log += os.read(r.slave, 65536)
        txt = log.decode("latin-1")
        report("SPI boot from the mtd0 image (debug strap): header + payload sum pass, exec 0xff000020",
               kind == "stage2" and img[0x20:len(s2)] == s2[0x20:] and "Checksum ok" in txt,
               f"{kind}; " + " | ".join(l.strip() for l in txt.splitlines()
                                        if "stg2" in l or "Checksum" in l or "boot_device" in l))
    else:
        print("skip  SPI boot from mtd0 (no --mtd0)")

    # 4. strap 0 (UART CLI) through the ROM's main: CLI, 'quit' -> SPI (blank) -> UART fallback
    r, th, box = emu_thread("cli")
    t = ur.Tty(r.pty, flush=False)
    try:
        banner = ur.settle(t, 1.0, 20).decode("latin-1")
        h = ur.cli_cmd(t, "help xmodem", echo=False)
        t.write(b"quit\r")
        naks = ur.xmodem_send(t, payload, wait=60, log=lambda m: print(f"    {m}"))
    finally:
        t.close()
    th.join(30)
    kind, img = box.get("res", ("timeout", None))
    report("strap 0 (UART CLI): banner + '> ', help, quit -> SPI blank -> UART XMODEM -> stage2",
           "boot_device UART CLI" in banner and "<address to receive to>" in h and kind == "stage2"
           and img[:len(payload)] == payload, f"{kind}, {naks} retries")
    if not results[-1]:
        print(banner, h, sep="\n----\n")

    # 5. Route R: reset vector -> resume address -> A32 stub -> ROM CLI, through consoled-sim
    r, th, box = emu_thread("route-r")
    cmd = [sys.executable, str(ROOT / "rom_emu.py"), "consoled-sim", r.pty]
    t = ur.Pipe(cmd, escape=b"\x02\x1d", take=True)
    blob = ur.test_file(0x10000, 2)
    try:
        banner = ur.settle(t, 1.5, 20).decode("latin-1")
        h = ur.cli_cmd(t, "help", echo=False)
        rd = ur.cli_cmd(t, "rd fbfe0164", echo=False)
        t.write(b"xmodem 1800000\r")
        naks = ur.xmodem_send(t, blob, wait=30, log=lambda m: print(f"    {m}"))
        out, end = b"", time.monotonic() + 15
        while time.monotonic() < end and not out.endswith(b"> "):
            out += ur.read_some(t, 0.2)
        rdr = ur.cli_cmd(t, "rdr 1800000 10", echo=False)
        zx = ur.cli_cmd(t, "rd 0x1800000", echo=False)
        t.write(b"quit\r")
        th.join(10)
    finally:
        t.close()
    kind, _ = box.get("res", ("timeout", None))
    got = bytes(r.uc.mem_read(0x01800000, len(blob)))
    words = [f"{w:X}" for w in struct.unpack_from("<4I", blob)]
    ok = ("R" in banner and "Boot ROM version: 3.1.0" in banner and "boot_device UART CLI" in banner
          and "spim_sw_mode 4x50MHz" in banner
          and "Supported commands" in h and "xmodem" in h and got == blob and naks == 0
          and all(w in rdr for w in words) and "Syntax error" in zx and kind == "quit")
    report("route R: A32 stub -> ROM CLI: help, rd, xmodem 64 KiB into DRAM, rdr, quit", ok,
           f"{kind}, {naks} retries; 'rd 0x1800000' -> Syntax error (no 0x prefixes)")
    if not ok:
        print(banner, h, rd, out.decode("latin-1"), rdr, repr(zx), sep="\n----\n")
    print("ALL PASS" if all(results) else "SOME FAILED")
    sys.exit(0 if all(results) else 1)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("run")
    p.add_argument("--mode", choices=sorted(BOOTDEV), required=True)
    p.add_argument("--mtd0", help="SPI contents for --mode spi")
    p.add_argument("--debug", action="store_true", help="debug strap on (bit 18 = 0): ROM prints")
    p.add_argument("--timeout", type=float, default=None)
    p.add_argument("--link", help="also make this symlink to the pty")
    p = sub.add_parser("consoled-sim")
    p.add_argument("pty")
    p.add_argument("--no-replay", action="store_true")
    p = sub.add_parser("selftest")
    p.add_argument("--mtd0", default=str(MTD0))
    a = ap.parse_args()
    {"run": cmd_run, "consoled-sim": cmd_consoled_sim, "selftest": cmd_selftest}[a.cmd](a)


if __name__ == "__main__":
    main()
