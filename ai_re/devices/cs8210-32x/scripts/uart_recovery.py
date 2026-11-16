#!/usr/bin/env python3
"""CS8210 (Alpine V3, boot ROM 3.1.0): UART recovery host tool. Evidence: ../uart-recovery.md.

Offline subcommands never touch hardware. The ones that talk to a port (xmodem, rom-cli) need it
to themselves: a tty (--tty, nothing else may hold it) or the DMC-12 consoled (--consoled,
'ssh -T -e none HOST attach PORT': 8-bit clean, the attach prefix bytes C-b/^] are doubled).

  uart_recovery.py s2 images/spi-2026-11-15/mtd0-uboot.bin analysis/boot-rom/stg2.s2
        the S2 image (header + payload + sum16) the ROM's UART boot expects (max 0x78000 B here)
  uart_recovery.py xmodem --consoled console@10.60.2.10:cs8210 --take FILE
        XMODEM (128-byte SOH blocks, CRC16 or 8-bit sum as the receiver asks) to whatever is
        receiving: the ROM's UART boot, the ROM CLI's 'xmodem', or U-Boot's 'loadx'
  uart_recovery.py rom-cli --consoled ... [--load ADDR FILE] [CMD ...]
        drive the ROM's UART CLI ('> ' prompt; hex numbers without 0x; CR only, never LF)
  uart_recovery.py route-r build OUT.bin | plan | disasm
        Route R: from the vendor U-Boot into the ROM's UART CLI, no strap and no SPI write
  rom_emu.py selftest          the whole thing against the real ROM code in Unicorn, on a pty

Sending to the consoled needs root's ssh key on this host (PROGRESS: 'sudo ssh ...'): run with
sudo, or --ssh 'sudo ssh'.
"""
import argparse, os, pathlib, select, shlex, struct, subprocess, sys, termios, time, zlib

ROOT = pathlib.Path(__file__).resolve().parent
DEV = ROOT.parent

# ---- ROM facts (uart-recovery.md; ROM addresses are the 0xffff0000 alias, Thumb functions +1) ----
ROM_UART_MAX = 0x78000         # UART boot: dcache + LLC straps (this unit) -> 0xff000000, 0x78000 B
ROM_CLI_XMODEM_MAX = 0x1000000  # CLI 'xmodem <addr>': xmodemReceive(addr, 0x1000000)
ROM_PROMPT = b"> "

# ---- XMODEM ----
SOH, STX, EOT, ACK, NAK, CAN, SUB = 1, 2, 4, 6, 0x15, 0x18, 0x1a


def crc16(b):
    c = 0
    for x in b:
        c ^= x << 8
        for _ in range(8):
            c = ((c << 1) ^ 0x1021) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return c


class Tty:
    """A local serial device or pty, raw 8N1, nothing else may hold it."""
    def __init__(self, path, baud=115200, flush=True):
        busy = holders(path)
        if busy:
            raise SystemExit(f"{path} is open by pid {', '.join(busy)} (tio/consoled?); stop it first")
        self.fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        at = termios.tcgetattr(self.fd)
        at[0] = at[1] = at[3] = 0
        at[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
        speed = getattr(termios, f"B{baud}")
        at[4] = at[5] = speed
        at[6][termios.VMIN], at[6][termios.VTIME] = 0, 0
        termios.tcsetattr(self.fd, termios.TCSANOW, at)
        if flush:
            termios.tcflush(self.fd, termios.TCIOFLUSH)
        self.rfd = self.wfd = self.fd

    def write(self, b):
        while b:
            try:
                n = os.write(self.wfd, b)
            except BlockingIOError:
                select.select([], [self.wfd], [], 1)
                continue
            b = b[n:]

    def close(self):
        os.close(self.fd)


class Pipe:
    """A command whose stdin/stdout are the port, e.g. ssh to the consoled 'attach'. Bytes in
    escape (the attach prefix keys) are doubled so they reach the port as themselves."""
    def __init__(self, argv, escape=b"", take=False):
        self.p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, bufsize=0)
        self.rfd, self.wfd = self.p.stdout.fileno(), self.p.stdin.fileno()
        os.set_blocking(self.rfd, False)
        self.escape = set(escape)
        if take and escape:
            os.write(self.wfd, bytes([escape[0]]) + b"t")   # prefix t: take write

    def write(self, b):
        if self.escape:
            b = b"".join(bytes([x, x]) if x in self.escape else bytes([x]) for x in b)
        while b:
            n = os.write(self.wfd, b)
            b = b[n:]

    def close(self):
        try:
            self.p.stdin.close()
        except OSError:
            pass
        try:
            self.p.wait(3)
        except subprocess.TimeoutExpired:
            self.p.kill()
        err = self.p.stderr.read().decode(errors="replace").strip()
        if err:
            print(f"[transport stderr] {err}", file=sys.stderr)


def read_some(t, timeout):
    r, _, _ = select.select([t.rfd], [], [], max(timeout, 0))
    if not r:
        return b""
    try:
        b = os.read(t.rfd, 4096)
    except BlockingIOError:
        return b""
    if b == b"" and isinstance(t, Pipe):
        raise SystemExit("transport closed (ssh/attach ended)")
    return b


def read_byte(t, timeout, junk=None):
    """Next byte (buffered); None on timeout."""
    end = time.monotonic() + timeout
    while True:
        if t.__dict__.setdefault("buf", b""):
            c, t.buf = t.buf[0], t.buf[1:]
            return c
        left = end - time.monotonic()
        if left <= 0:
            return None
        t.buf = read_some(t, left)


def settle(t, quiet, limit):
    """Drop what arrives (e.g. the attach scrollback replay) until 'quiet' s of silence."""
    end, last, got = time.monotonic() + limit, time.monotonic(), b""
    t.buf = b""
    while time.monotonic() < end:
        b = read_some(t, 0.1)
        if b:
            got += b
            last = time.monotonic()
        elif time.monotonic() - last >= quiet:
            break
    return got


def xmodem_send(t, data, wait=300.0, quiet=0.8, log=print):
    """Send data with 128-byte SOH blocks. Waits for an isolated 'C' (CRC16) or NAK (sum)."""
    settle(t, quiet, 10)
    log(f"waiting up to {wait:.0f} s for the receiver's 'C' or NAK ...")
    end, mode, prev_t = time.monotonic() + wait, None, time.monotonic()
    while time.monotonic() < end and mode is None:
        b = read_some(t, 0.5)
        now = time.monotonic()
        if b and set(b) <= {0x43, NAK} and now - prev_t > 0.3:
            mode = "crc" if b[-1] == 0x43 else "sum"
        if b:
            prev_t = now
            if not set(b) <= {0x43, NAK}:
                sys.stderr.write(b.decode("latin-1").replace("\r", ""))
    if mode is None:
        raise SystemExit("no receiver ('C'/NAK) seen")
    t.buf = b""
    blocks = [data[i:i + 128].ljust(128, bytes([SUB])) for i in range(0, len(data), 128)]
    naks, t0 = 0, time.monotonic()
    for n, blk in enumerate(blocks, 1):
        tail = struct.pack(">H", crc16(blk)) if mode == "crc" else bytes([sum(blk) & 0xFF])
        pkt = bytes([SOH, n & 0xFF, 0xFF - (n & 0xFF)]) + blk + tail
        for attempt in range(10):
            t.write(pkt)
            r = None
            deadline = time.monotonic() + 6     # the ROM flushes 1.5 s of input before a NAK
            while time.monotonic() < deadline:
                r = read_byte(t, deadline - time.monotonic())
                if r in (ACK, NAK, CAN):
                    break
            if r == ACK:
                break
            if r == CAN:
                raise SystemExit(f"receiver cancelled at block {n}")
            naks += 1
            log(f"block {n}: {'NAK' if r == NAK else 'no answer'}, retry {attempt + 1}")
        else:
            raise SystemExit(f"block {n}: no ACK after 10 tries")
        if n % 128 == 0:
            log(f"  {n * 128:#x}/{len(data):#x}")
    for _ in range(5):
        t.write(bytes([EOT]))
        r = None
        deadline = time.monotonic() + 6         # the ROM flushes 1.5 s of input before the ACK
        while time.monotonic() < deadline:
            r = read_byte(t, deadline - time.monotonic())
            if r in (ACK, NAK):
                break
        if r == ACK:
            break
    else:
        raise SystemExit("no ACK for EOT")
    dt = time.monotonic() - t0
    log(f"sent {len(data):#x} B in {len(blocks)} blocks ({mode}), {naks} retries, {dt:.1f} s, "
        f"crc32 {zlib.crc32(data):08x} (padded to {len(blocks) * 128:#x} with 0x1a)")
    return naks


def holders(tty):
    real, me, out = os.path.realpath(tty), os.getpid(), []
    for p in os.listdir("/proc"):
        if not p.isdigit() or int(p) == me:
            continue
        try:
            for fd in os.listdir(f"/proc/{p}/fd"):
                if os.path.realpath(f"/proc/{p}/fd/{fd}") == real:
                    out.append(p)
                    break
        except OSError:
            continue
    return out


def transport(a):
    if a.tty:
        return Tty(a.tty, a.baud)
    if a.consoled:
        host, port = a.consoled.rsplit(":", 1)
        argv = shlex.split(a.ssh) + ["-T", "-e", "none", "-o", "BatchMode=yes", host, "attach", port]
        return Pipe(argv, escape=bytes([a.prefix_byte, 0x1D]), take=a.take)
    if a.cmd:
        return Pipe(shlex.split(a.cmd), escape=bytes([a.prefix_byte, 0x1D]) if a.escape else b"",
                    take=a.take)
    raise SystemExit("give --tty, --consoled HOST:PORT or --cmd")


# ---- the ROM CLI ----
def cli_cmd(t, line, timeout=10.0, echo=True):
    """Type one CLI line (CR only: the ROM keeps LF as a character) and return the output up to
    the next '> ' prompt."""
    if b"\n" in line.encode():
        raise ValueError("no LF in ROM CLI lines")
    t.buf = b""
    t.write(line.encode() + b"\r")
    out, end = b"", time.monotonic() + timeout
    while time.monotonic() < end:
        out += read_some(t, 0.2)
        if out.endswith(ROM_PROMPT) and b"\n" in out:
            break
    text = out.decode("latin-1").replace("\r", "")
    if echo:
        print(text, end="" if text.endswith("\n") or text.endswith("> ") else "\n")
    return text


def cmd_rom_cli(a):
    t = transport(a)
    try:
        settle(t, 0.8, 10)
        hello = cli_cmd(t, "", 5)              # an empty line answers with a fresh prompt
        if "> " not in hello:
            print("warning: no '> ' prompt seen (is the ROM CLI running?)", file=sys.stderr)
        if a.load:
            addr, path = a.load
            data = pathlib.Path(path).read_bytes()
            if len(data) > ROM_CLI_XMODEM_MAX:
                raise SystemExit("larger than the CLI's xmodem limit (0x1000000)")
            t.write(f"xmodem {int(addr, 16):x}\r".encode())
            xmodem_send(t, data, wait=a.wait)
            out, end = b"", time.monotonic() + 10
            while time.monotonic() < end and not out.endswith(ROM_PROMPT):
                out += read_some(t, 0.2)
            print(out.decode("latin-1").replace("\r", ""))
        for c in a.cmds:
            w = c.split()
            if w and w[0] in ("wr", "wrr", "eeprom_wr", "gpio_set", "go") and not a.allow_write:
                raise SystemExit(f"{c!r} writes or jumps: pass --allow-write")
            if any(x.lower().startswith("0x") for x in w[1:]):
                raise SystemExit(f"{c!r}: the ROM parses bare hex only ('0x' is a syntax error)")
            if w and w[0] in ("rdr", "wrr", "vrr") and (len(w) < 3 or int(w[2], 16) % 4):
                raise SystemExit(f"{c!r}: the byte count must be a multiple of 4 (the ROM loops "
                                 "'count -= 4' until 0)")
            cli_cmd(t, c, a.timeout)
    finally:
        t.close()


# ---- S2 extraction ----
def cmd_s2(a):
    d = pathlib.Path(a.mtd0).read_bytes()
    h = d[:0x20]
    w = struct.unpack("<8I", h)
    if h[:2] != b"S2":
        raise SystemExit(f"no S2 magic at 0: {h[:4].hex()}")
    if sum(h[:0x1E]) & 0xFFFF != struct.unpack_from("<H", h, 0x1E)[0]:
        raise SystemExit("S2 header sum16 bad")
    size = w[3]
    psum = sum(d[0x20:0x20 + size]) & 0xFFFF
    if psum != struct.unpack_from("<H", d, 0x20 + size)[0]:
        raise SystemExit("S2 payload sum16 bad")
    n = 0x20 + size + 2
    if n > ROM_UART_MAX:
        raise SystemExit(f"{n:#x} B > the ROM's UART limit {ROM_UART_MAX:#x}")
    out = pathlib.Path(a.out)
    if out.exists():
        raise SystemExit(f"refusing to replace {out}")
    out.write_bytes(d[:n])
    print(f"{out}: {n:#x} B, exec word {w[4]:#010x}, flags byte {h[2]:#04x} (bit 1 = AArch64), "
          f"crc32 {zlib.crc32(d[:n]):08x}. The ROM receives it at 0xff000000 and enters 0xff000020 "
          "(A32) with no sum check (non-secure)")


def test_file(n, seed=1):
    """Deterministic test data, dense in the bytes a byte path likes to eat: the attach prefixes
    (0x02, 0x1d), XON/XOFF, CR/LF, ^C, CAN, 0x00, 0xff."""
    import random
    rnd = random.Random(seed)
    b = bytearray(rnd.getrandbits(8) for _ in range(n))
    for i in range(0, n, 97):
        b[i] = rnd.choice((0x02, 0x1D, 0x11, 0x13, 0x0D, 0x0A, 0x03, 0xFF, 0x00, 0x18, 0x7E))
    return bytes(b)


def cmd_testfile(a):
    out = pathlib.Path(a.out)
    data = test_file(int(a.size, 0), a.seed)
    out.write_bytes(data)
    print(f"{out}: {len(data):#x} B crc32 {zlib.crc32(data):08x} (U-Boot: crc32 <addr> {len(data):x})")


def cmd_xmodem(a):
    data = pathlib.Path(a.file).read_bytes()
    if a.limit and len(data) > int(a.limit, 0):
        raise SystemExit(f"{len(data):#x} B > --limit {a.limit} (the ROM truncates silently)")
    t = transport(a)
    try:
        if a.first:
            settle(t, 0.8, 10)
            t.write(a.first.encode() + b"\r")    # e.g. 'loadx 0x01800000' or the CLI's 'xmodem 1800000'
        xmodem_send(t, data, wait=a.wait)
        end = time.monotonic() + a.listen
        while time.monotonic() < end:
            b = read_some(t, 0.2)
            if b:
                sys.stdout.write(b.decode("latin-1"))
                sys.stdout.flush()
    finally:
        t.close()


# ---- Route R: vendor U-Boot (AArch64) -> CPU0 warm reset into the ROM (AArch32) -> UART CLI ----
# Blob at R_BASE (U-Boot 'loadx'): +0x000 AArch64 trampoline (EL2), +0x0c0 EL3 stub (copied to
# 0x1000 = CPU0 RVBAR, write-once, set by stage3), +0x180 A32 stub (entered from the ROM's reset
# handler through per_core[0].resume_addr). See uart-recovery.md section 4.
R_BASE, R_EL3, R_A32 = 0x01700000, 0x0C0, 0x180
RVBAR = 0x1000
A32_SP = 0x01F00000
ANPA = 0xF0400000                      # cluster 0; global.cpus_software_reset at +0x24
PER_CORE0 = ANPA + 0x10000             # aarch64 +0x08, resume_addr_l/h +0x30/+0x34, rvbar +0x48
UART0 = 0xFD883000
CPU0_POR = 0x00010013                  # WFI_MASK core0 | core0 | level CPU_PORESET | SWRESET_REQ
CLEAN = (0x01000000, 0x02000000)       # D-cache clean+invalidate: blob, A32 stack, CLI xmodem target


def a64_movz(rd, imm, hw=0, x=True):
    return (0xD2800000 if x else 0x52800000) | hw << 21 | (imm & 0xFFFF) << 5 | rd


def a64_movk(rd, imm, hw, x=True):
    return (0xF2800000 if x else 0x72800000) | hw << 21 | (imm & 0xFFFF) << 5 | rd


def a64_mov32(rd, val, x=True):
    return [a64_movz(rd, val & 0xFFFF, 0, x), a64_movk(rd, val >> 16, 1, x)]


def a64_str32(rt, rn, off):
    return 0xB9000000 | (off // 4) << 10 | rn << 5 | rt


def a64_ldp(rt, rt2, rn, off):
    return 0xA9400000 | ((off // 8) & 0x7F) << 15 | rt2 << 10 | rn << 5 | rt


def a64_stp(rt, rt2, rn, off):
    return 0xA9000000 | ((off // 8) & 0x7F) << 15 | rt2 << 10 | rn << 5 | rt


def a64_adr(rd, off):
    return (off & 3) << 29 | 0x10000000 | ((off >> 2) & 0x7FFFF) << 5 | rd


DSB, ISB, WFI, NOP = 0xD5033F9F, 0xD5033FDF, 0xD503207F, 0xD503201F
DC_CIVAC = 0xD50B7E20


def tramp_a64():
    w = [a64_adr(5, R_EL3)]                                  # x5 = EL3 stub template
    w += [a64_movz(6, RVBAR)]                                # x6 = 0x1000
    for off in (0, 16, 32, 48, 64, 80):                      # copy 0x60 B
        w += [a64_ldp(7, 8, 5, off), a64_stp(7, 8, 6, off)]
    w += [DC_CIVAC | 6, 0x910100C6, DC_CIVAC | 6]            # dc civac x6; add x6,x6,#64; dc civac x6
    w += a64_mov32(0, CLEAN[0]) + a64_mov32(1, CLEAN[1])
    loop = len(w)
    w += [DC_CIVAC | 0, 0x91010000, 0xEB01001F]              # dc civac x0; add x0,x0,#64; cmp x0,x1
    w += [0x54000003 | ((loop - len(w)) & 0x7FFFF) << 5]     # b.lo loop
    w += [DSB]
    w += a64_mov32(2, ANPA) + a64_mov32(3, CPU0_POR, x=False)
    w += [a64_str32(3, 2, 0x24), DSB, ISB, WFI, 0x17FFFFFF]  # str w3,[x2,#0x24]; wfi; b wfi
    assert len(w) * 4 <= R_EL3
    return w + [NOP] * (R_EL3 // 4 - len(w))


def el3_stub():
    """Runs at 0x1000 after CPU0's power-on reset: AArch64 EL3, MMU off. Secure, so it can set
    the AArch32 resume address itself; then RMR_EL3 {AA64=0, RR=1} and WFI = warm reset into
    AArch32 at the ROM's reset vector (VINITHI)."""
    w = a64_mov32(1, UART0) + [a64_movz(2, ord("#"), x=False), a64_str32(2, 1, 0)]
    w += a64_mov32(2, PER_CORE0) + a64_mov32(3, R_BASE + R_A32, x=False)
    w += [a64_str32(3, 2, 0x30), a64_str32(31, 2, 0x34), DSB]
    w += [a64_movz(0, 2), DSB, ISB, 0xD51EC040, ISB, WFI, 0x17FFFFFF]   # msr rmr_el3, x0
    assert len(w) * 4 <= 0x60
    return w + [NOP] * (0x60 // 4 - len(w))


def a32_movw(rd, imm):
    return 0xE3000000 | (imm >> 12 & 0xF) << 16 | rd << 12 | imm & 0xFFF


def a32_movt(rd, imm):
    return 0xE3400000 | (imm >> 12 & 0xF) << 16 | rd << 12 | imm & 0xFFF


def a32_mov32(rd, v):
    return [a32_movw(rd, v & 0xFFFF), a32_movt(rd, v >> 16)]


def a32_call(fn, *args):
    w = []
    for i, v in enumerate(args):
        w += a32_mov32(i, v) if not isinstance(v, str) else [0xE1A00000 | i << 12 | int(v[1:])]
    return w + a32_mov32(12, fn | 1) + [0xE12FFF3C]          # blx r12 (Thumb ROM function)


def a32_putc(ch):
    return a32_mov32(1, UART0) + [0xE3A02000 | ord(ch), 0xE5812000]   # mov r2,#ch; str r2,[r1]


# ROM 3.1.0 functions (Thumb; uart-recovery.md section 1). The stub replays the ROM's main
# (0xffff045c) for boot device 4 (UART CLI) and skips the SPI and LLC-SRAM parts.
F_SCTLR, F_MEMSET, F_MEMCPY, F_DBGLVL = 0xFFFF0338, 0xFFFF6BBC, 0xFFFF6B40, 0xFFFF20F4
F_BS_PARSE, F_TIME_INIT, F_DBG_UART, F_BANNER = 0xFFFF7E84, 0xFFFF1ED0, 0xFFFF17B4, 0xFFFF0740
F_OTP, F_BS_PRINT, F_CLI = 0xFFFF1814, 0xFFFF81B8, 0xFFFF0AD8
BS = 0xFBFE0164                         # struct al_bootstrap in PBS SRAM


def a32_stub():
    w = a32_putc("R") + a32_mov32(13, A32_SP)
    w += a32_call(F_SCTLR, 0, 1, 0, 0)                       # MMU off, I-cache on (as the ROM)
    w += a32_call(F_MEMSET, 0xFBFE0018, 0, 0xFBFE019C - 0xFBFE0018)
    w += a32_call(F_MEMCPY, 0xFBFE0000, 0xFFFFF2AD, 0x18)    # the ROM's initial state block
    w += a32_call(F_DBGLVL, 0)
    w += a32_call(F_BS_PARSE, 0xFD8A8000, BS)                # straps (PBS boot_strap 0x110/0x278)
    w += a32_mov32(4, BS) + [0xE3A00001, 0xE5941000, 0xE5942004, 0xE594300C]  # r0=1 r1..r3=bs[0,1,3]
    w += a32_mov32(12, F_TIME_INIT | 1) + [0xE12FFF3C]
    w += [0xE3A00004, 0xE5C40018, 0xE3A00000, 0xE5C40020]    # bs.boot_device = 4 (UART CLI), debug on
    w += a32_call(F_DBG_UART)                                # UART0 init + debug level 4
    w += a32_call(F_BANNER) + a32_call(F_OTP) + a32_call(F_BS_PRINT, BS)
    w += a32_call(F_CLI)                                     # returns on 'quit'
    w += a32_putc("Q") + [0xE320F003, 0xEAFFFFFD]            # wfi; b wfi
    return w


def route_r_blob():
    words = tramp_a64() + el3_stub() + [NOP] * ((R_A32 - R_EL3 - 0x60) // 4) + a32_stub()
    return b"".join(struct.pack("<I", x) for x in words)


def cmd_route_r(a):
    blob = route_r_blob()
    if a.what == "build":
        out = pathlib.Path(a.out)
        if out.exists() and out.read_bytes() != blob:
            raise SystemExit(f"refusing to replace {out} (differs)")
        out.write_bytes(blob)
        print(f"{out}: {len(blob):#x} B crc32 {zlib.crc32(blob):08x}, load at {R_BASE:#x}")
    elif a.what == "disasm":
        import capstone
        for name, off, end, arch in (("AArch64 trampoline (EL2)", 0, R_EL3, "a64"),
                                     ("EL3 stub (copied to 0x1000)", R_EL3, R_EL3 + 0x60, "a64"),
                                     ("A32 stub", R_A32, len(blob), "a32")):
            md = capstone.Cs(capstone.CS_ARCH_ARM64, capstone.CS_MODE_ARM) if arch == "a64" else \
                capstone.Cs(capstone.CS_ARCH_ARM, capstone.CS_MODE_ARM)
            base = RVBAR if off == R_EL3 else R_BASE + off
            print(f"== {name} @ {base:#x}")
            for i in md.disasm(blob[off:end], base):
                print(f"  {i.address:08x}  {i.mnemonic:8s} {i.op_str}")
    else:
        c = zlib.crc32(blob)
        print(PLAN.format(base=R_BASE, size=len(blob), crc=c, a32=R_BASE + R_A32))


PLAN = """Route R at the vendor prompt (ALPINE_DB>), every step read-back checked; RAM only, no SPI.
  1  md.l 0x1000 4                       (the agent stage3 left; note it, it gets overwritten)
  2  md.l 0xfd8a8110 1; md.l 0xfd8a8278 1  (straps; boot device = bits 17:15, expect 7 = SPI M0)
  3  uart_recovery.py xmodem --consoled ... --take --first 'loadx {base:#x}' route-r.bin
  4  crc32 {base:#x} {size:#x}             expect {crc:08x}
  5  go {base:#x}                         expect '#' (EL3 stub), 'R' (A32 stub, from the ROM's reset
                                          handler), then the ROM banner 'Annapurna Labs Boot ROM version:
                                          3.1.0', 'Boot configuration:' (same values as dev_info) and '> '
  6  uart_recovery.py rom-cli --consoled ... --take help 'rd fbfe0164' 'rdr fbfe0164 40'
  7  uart_recovery.py rom-cli --consoled ... --take --load 1800000 test.bin 'rdr 1800000 10'
  8  quit                                  expect 'Q'; then power-cycle (lab-power 1 off, 10 s, on)
A32 stub entry {a32:#x}; nothing here touches SPI, OTP writes or EEPROMs."""


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="action", required=True)

    def port_args(p):
        p.add_argument("--tty", help="local serial device or pty (exclusive)")
        p.add_argument("--baud", type=int, default=115200)
        p.add_argument("--consoled", help="HOST:PORT, e.g. console@10.60.2.10:cs8210")
        p.add_argument("--ssh", default="ssh", help="ssh command (e.g. 'sudo ssh')")
        p.add_argument("--cmd", help="any command whose stdin/stdout is the port")
        p.add_argument("--escape", action="store_true", help="with --cmd: double the prefix bytes")
        p.add_argument("--prefix-byte", type=lambda s: int(s, 0), default=0x02,
                       help="consoled attach prefix (C-b = 0x02); ^] (0x1d) is always doubled too")
        p.add_argument("--take", action="store_true", help="consoled: take write first (prefix t)")
        p.add_argument("--wait", type=float, default=300, help="seconds to wait for 'C'/NAK")
    p = sub.add_parser("s2")
    p.add_argument("mtd0")
    p.add_argument("out")
    p = sub.add_parser("xmodem")
    port_args(p)
    p.add_argument("--limit", help="refuse files larger than this (0x78000 for the ROM UART boot)")
    p.add_argument("--listen", type=float, default=10, help="seconds to echo output afterwards")
    p.add_argument("--first", help="type this line (+CR) first, e.g. 'loadx 0x01800000'")
    p.add_argument("file")
    p = sub.add_parser("testfile")
    p.add_argument("out")
    p.add_argument("--size", default="0x10000")
    p.add_argument("--seed", type=int, default=1)
    p = sub.add_parser("rom-cli")
    port_args(p)
    p.add_argument("--load", nargs=2, metavar=("HEXADDR", "FILE"), help="'xmodem ADDR' + send FILE")
    p.add_argument("--timeout", type=float, default=10)
    p.add_argument("--allow-write", action="store_true", help="allow wr/wrr/eeprom_wr/gpio_set/go")
    p.add_argument("cmds", nargs="*")
    p = sub.add_parser("route-r")
    p.add_argument("what", choices=("build", "plan", "disasm"))
    p.add_argument("out", nargs="?", default=str(DEV / "analysis/boot-rom/route-r.bin"))
    a = ap.parse_args()
    {"s2": cmd_s2, "xmodem": cmd_xmodem, "rom-cli": cmd_rom_cli, "route-r": cmd_route_r,
     "testfile": cmd_testfile}[a.action](a)


if __name__ == "__main__":
    main()
