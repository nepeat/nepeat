#!/usr/bin/env python3
"""DMC-12: run production stage3 from a RAM flash image (Route A), and the boot ROM's XMODEM.

Background and evidence: uart-boot.md. Offline subcommands never touch hardware.

  uartboot.py image --app 0x110 --uboot <u-boot.bin> out.bin   RAM flash image: our U-Boot in IPXE 0
  uartboot.py image --app 0x10000005 out.bin                   `u` test: factory UBOOT 1 vs 0, no new code
  uartboot.py image --app none out.bin                         mtd0 as is (BOOT_MODE blank, app 5)
  uartboot.py tramp out.bin                                    the AArch64 trampoline (cache clean + reset)
  uartboot.py route-a --image out.bin [--key u] [--run]        dry run unless --run
  uartboot.py s2 dumps/u01/mtd0.bin.zst stg2.s2                boot-ROM image (S2 header + payload + sum)
  sudo uartboot.py xmodem --tty /dev/serial/by-id/... stg2.s2  needs the tty to itself (stop tio first)

Every image blanks UBOOT_SCRIPT 0/1: in RAM-load ("UART recovery") mode the factory U-Boot
would otherwise run it, and with a default env the script ends in `saveenv` (an SPI write).

route-a, at the factory `ALPINE_DB>` prompt, through console.py and the tmux pane `serial-dmc12`:
TFTP the image to 0x01800000 and the trampoline to 0x01700000, copy a clean stage3 out of the
image to 0x01000000 and zero its .bss, set the stage2 shared-data flags to RAM load
(0xfbff41c1: bit0 on, bit1 off), then `go 0x01700000`. The trampoline cleans D-cache over
0x01000000-0x02000000 and the flags line, sets CPU0 to AArch32 (NB 0xf0072004 = 0) with the
boot ROM resume address 0xf0072028 = 0x01000000, and warm-resets CPU0 (0xf0070024 = 0x13).
The ROM's reset handler jumps to the resume address; stage3 then reads the "flash" from RAM.
Afterwards **power-cycle** (AC off >= 10 s): 0xf0072028 stays set until then.
"""
import argparse, os, pathlib, struct, subprocess, sys, termios, time, zlib

ROOT = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
import alboot  # noqa: E402  (offline helpers; symlink into ai_re/nix/packages/alboot)

MTD0 = ROOT / "dumps/u01/mtd0.bin.zst"
IMG_SIZE = 0x800000            # stage3 clears DRAM from 0x02000000 = RAM image + 8 MiB
IMG_ADDR, TRAMP_ADDR, STG3_ADDR = 0x01800000, 0x01700000, 0x01000000
STG3_BSS_END = 0x0105E000      # stage3 .bss ends ~0x0105d322 (highest reference)
FLAGS = 0xFBFF41C1             # al_stage2_shared_data.flags (magic at 0xfbff41c0)
SESSION = "serial-dmc12"

# AArch64, position independent; disassembly checked in Ghidra (uart-boot.md section 4).
TRAMP = [
    0xD2A02000,  # mov  x0, #0x01000000
    0xD2A04001,  # mov  x1, #0x02000000
    0xD50B7E20,  # 1: dc civac, x0
    0x91010000,  # add  x0, x0, #64
    0xEB01001F,  # cmp  x0, x1
    0x54FFFFA3,  # b.lo 1b
    0xD2883804,  # mov  x4, #0x41c0
    0xF2BF7FE4,  # movk x4, #0xfbff, lsl #16
    0xD50B7E24,  # dc civac, x4            (shared-data flags line)
    0xD5033F9F,  # dsb  sy
    0xD2BE00E2,  # mov  x2, #0xf0070000    (NB service)
    0x52800003,  # mov  w3, #0
    0xB9200443,  # str  w3, [x2, #0x2004]  CPU0: AArch32 at next warm reset
    0x52A02003,  # mov  w3, #0x01000000
    0xB9202843,  # str  w3, [x2, #0x2028]  CPU0: boot-ROM resume address (ARM)
    0xD5033F9F,  # dsb  sy
    0x52800263,  # mov  w3, #0x13
    0xB9002443,  # str  w3, [x2, #0x24]    CPU0 warm reset (stage3 0x01023da0 value)
    0xD5033F9F,  # dsb  sy
    0xD5033FDF,  # isb
    0xD503207F,  # 2: wfi
    0x17FFFFFF,  # b    2b
]


def toc_entries(d):
    o, n, ok = alboot.find_toc(d)
    if o is None or not ok:
        raise SystemExit("no valid TOC")
    for i in range(n):
        e = o + 0x14 + i * 0x20
        idi, = struct.unpack_from("<I", d, e)
        off, mx = struct.unpack_from("<II", d, e + 0x10)
        yield idi & 0x0FFFFFFF, idi >> 28, off, mx


def slot(d, oid, inst):
    for i, n, off, mx in toc_entries(d):
        if i == oid and n == inst:
            return off, mx
    raise SystemExit(f"object {oid:#x}/{inst} not in TOC")


def obj(oid, load, data):
    h = struct.pack("<6I16s3QI", alboot.OBJ_MAGIC, 0, oid, 2, 60, 4,
                    b"N/A".ljust(16, b"\0"), len(data), load, load, 0)
    h += struct.pack("<I", alboot.bsum(h))
    return h + data + struct.pack("<I", alboot.bsum(data))


def put(img, d, oid, inst, blob):
    off, mx = slot(d, oid, inst)
    if len(blob) > mx or off + mx > IMG_SIZE:
        raise SystemExit(f"{oid:#x}/{inst}: {len(blob):#x} B at {off:#x} does not fit")
    img[off:off + mx] = blob.ljust(mx, b"\xff")


def cmd_image(a):
    d = alboot.load(str(a.mtd))
    img = bytearray(d[:IMG_SIZE])
    for inst in (0, 1):
        put(img, d, 6, inst, b"")                       # UBOOT_SCRIPT blank: no saveenv path
    if a.app != "none":
        app = int(a.app, 0)
        if app & 0x0FFFFFFF == 0x110:
            if not a.uboot:
                raise SystemExit("--app 0x110 needs --uboot u-boot.bin (linked at 0x08000000)")
            put(img, d, 0x110, app >> 28, obj(0x110, 0x08000000, a.uboot.read_bytes()))
        put(img, d, 0, 0, obj(0, 0, struct.pack("<II", 0, app)))   # stage3 reads word +4 only
    so, _ = slot(d, 4, 0)
    st, f = alboot.check_object(img, so, 0)
    if st != "OK":
        raise SystemExit(f"STG3 0 in image: {st}")
    stg3 = bytes(img[so + alboot.HDR:so + alboot.HDR + f["size"]])
    if a.out.exists():
        raise SystemExit(f"refusing to replace {a.out}")
    a.out.write_bytes(img)
    meta = {"crc": zlib.crc32(img), "stg3_src": IMG_ADDR + so + alboot.HDR,
            "stg3_size": len(stg3), "stg3_crc": zlib.crc32(stg3)}
    a.out.with_suffix(".txt").write_text("".join(f"{k}={v:#x}\n" for k, v in meta.items()))
    for i, n, off, mx in toc_entries(img):
        if off < IMG_SIZE and i in (0, 4, 5, 6, 0x110):
            print(f"  {alboot.NAMES.get(i, hex(i)):12s} {n} @{off:#08x}: "
                  f"{alboot.check_object(img, off, mx, i)[0]}")
    print(f"{a.out}: {len(img):#x} B crc32 {meta['crc']:08x}; stage3 {meta['stg3_size']:#x} B "
          f"crc32 {meta['stg3_crc']:08x} (meta in {a.out.with_suffix('.txt').name})")


def cmd_tramp(a):
    if a.out.exists():
        raise SystemExit(f"refusing to replace {a.out}")
    a.out.write_bytes(b"".join(struct.pack("<I", w) for w in TRAMP))
    print(f"{a.out}: {4 * len(TRAMP)} B crc32 {zlib.crc32(a.out.read_bytes()):08x}")


def console(c, timeout=30):
    r = subprocess.run([sys.executable, str(ROOT / "console.py"), "--timeout", str(timeout), c],
                       capture_output=True, text=True)
    print(f"> {c}\n{r.stdout.rstrip()}")
    if r.returncode:
        raise SystemExit(f"console.py failed: {r.stderr.strip()}")
    return r.stdout


def pane():
    return subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", SESSION],
                          capture_output=True, text=True, errors="replace").stdout


def crc_of(out):
    last = None
    for w in out.split():
        if len(w) == 8 and all(c in "0123456789abcdef" for c in w):
            last = w
    if last is None:
        raise SystemExit(f"no crc32 in: {out!r}")
    return int(last, 16)


def cmd_route_a(a):
    meta = dict(l.split("=") for l in a.image.with_suffix(".txt").read_text().split())
    meta = {k: int(v, 0) for k, v in meta.items()}
    if zlib.crc32(a.image.read_bytes()) != meta["crc"]:
        raise SystemExit("image does not match its .txt")
    tramp = b"".join(struct.pack("<I", w) for w in TRAMP)
    d = a.tftp_root / a.tftp_dir
    steps = [f"setenv serverip {a.server}", "dhcp",
             f"tftpboot {IMG_ADDR:#x} {a.tftp_dir}/{a.image.name}",
             f"crc32 {IMG_ADDR:#x} {IMG_SIZE:#x}",
             f"tftpboot {TRAMP_ADDR:#x} {a.tftp_dir}/tramp.bin",
             f"crc32 {TRAMP_ADDR:#x} {len(tramp):#x}",
             f"cp.b {meta['stg3_src']:#x} {STG3_ADDR:#x} {meta['stg3_size']:#x}",
             f"crc32 {STG3_ADDR:#x} {meta['stg3_size']:#x}",
             f"mw.b {STG3_ADDR + meta['stg3_size']:#x} 0 {STG3_BSS_END - STG3_ADDR - meta['stg3_size']:#x}",
             "md.b 0xfbff41c0 4", "mw.b 0xfbff41c1 <old & ~3 | 1>", "md.b 0xfbff41c0 4",
             "md.l 0xf0072000 0x14", f"go {TRAMP_ADDR:#x}   (then key {a.key!r} every 20 ms)" if a.key
             else f"go {TRAMP_ADDR:#x}"]
    print(f"image crc32 {meta['crc']:08x}, tramp crc32 {zlib.crc32(tramp):08x}, "
          f"stage3 crc32 {meta['stg3_crc']:08x}; TFTP dir {d}")
    if not a.run:
        print("dry run; commands:\n  " + "\n  ".join(steps))
        return
    d.mkdir(parents=True, exist_ok=True)
    (d / a.image.name).write_bytes(a.image.read_bytes())
    (d / "tramp.bin").write_bytes(tramp)
    console(f"setenv serverip {a.server}")
    if "bound to address" not in console("dhcp", 60):
        raise SystemExit("dhcp failed")
    for addr, name, size, want in ((IMG_ADDR, a.image.name, IMG_SIZE, meta["crc"]),
                                   (TRAMP_ADDR, "tramp.bin", len(tramp), zlib.crc32(tramp))):
        console(f"tftpboot {addr:#x} {a.tftp_dir}/{name}", 180)
        if crc_of(console(f"crc32 {addr:#x} {size:#x}")) != want:
            raise SystemExit(f"{name}: crc mismatch")
    console(f"cp.b {meta['stg3_src']:#x} {STG3_ADDR:#x} {meta['stg3_size']:#x}", 60)
    if crc_of(console(f"crc32 {STG3_ADDR:#x} {meta['stg3_size']:#x}")) != meta["stg3_crc"]:
        raise SystemExit("stage3 copy: crc mismatch")
    console(f"mw.b {STG3_ADDR + meta['stg3_size']:#x} 0 {STG3_BSS_END - STG3_ADDR - meta['stg3_size']:#x}")
    line = [l for l in console("md.b 0xfbff41c0 4").splitlines() if l.startswith("fbff41c0:")][0]
    magic, old = int(line.split()[1], 16), int(line.split()[2], 16)
    if magic < 0x3B:
        raise SystemExit(f"shared-data magic {magic:#x} < 0x3b: stage3 would ignore the flags")
    console(f"mw.b {FLAGS:#x} {(old & ~3) | 1:#x}")
    console("md.b 0xfbff41c0 4")
    console("md.l 0xf0072000 0x14")
    subprocess.run(["tmux", "clear-history", "-t", SESSION], check=True)
    marks = ("Stage 3 version", "UART recovery!", "User request", "Loading application",
             "Executing application", "U-Boot 20", "recovery boot command", "Entry: EL",
             "Hit any key")
    base = pane()  # the visible screen survives clear-history: count only new matches
    b = {m: base.count(m) for m in marks}
    for byte in f"go {TRAMP_ADDR:#x}".encode():
        subprocess.run(["tmux", "send-keys", "-t", SESSION, "-H", f"{byte:02x}"], check=True)
        time.sleep(0.02)
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "Enter"], check=True)
    t0, sent = time.monotonic(), 0
    while time.monotonic() - t0 < a.timeout:
        t = pane()
        new = {m: t.count(m) - b[m] for m in marks}
        if a.key and new["Loading application"] <= 0:
            subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", a.key], check=True)
            sent += 1
        if new["Hit any key"] > 0:
            break
        time.sleep(0.02)
    time.sleep(2)
    t = pane()
    print(f"key {a.key!r} sent {sent}x" if a.key else "no key")
    for m in marks:
        hits = [l.strip() for l in t.splitlines() if m in l][b[m]:]
        print(f"  {m:22s} {hits[0] if hits else '-- not seen'}")
    print("Done. Power-cycle (AC off >= 10 s) before anything else: 0xf0072028 is still set.")


def cmd_s2(a):
    d = alboot.load(str(a.mtd))
    st, info = alboot.check_s2(d, 0, 0x40000)
    if not st.startswith("OK"):
        raise SystemExit(f"S2 at 0: {st}")
    n = 0x20 + info["payload"] + 2
    if n > 0x68000:
        raise SystemExit("larger than the ROM's XMODEM limit 0x68000")
    if a.out.exists():
        raise SystemExit(f"refusing to replace {a.out}")
    a.out.write_bytes(d[:n])
    print(f"{a.out}: {n:#x} B (S2 header + payload + sum16); the ROM runs it at 0xf2200020")


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


def crc16(b):
    c = 0
    for x in b:
        c ^= x << 8
        for _ in range(8):
            c = ((c << 1) ^ 0x1021) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return c


def cmd_xmodem(a):
    data = a.file.read_bytes()
    if len(data) > 0x68000:
        raise SystemExit("ROM XMODEM limit is 0x68000 B (it truncates silently)")
    busy = holders(a.tty)
    if busy:
        raise SystemExit(f"{a.tty} is open by pid {', '.join(busy)} (tio?). Stop it first, see --help")
    fd = os.open(a.tty, os.O_RDWR | os.O_NOCTTY)
    at = termios.tcgetattr(fd)
    at[0] = at[1] = at[3] = 0
    at[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    at[4] = at[5] = termios.B115200
    at[6][termios.VMIN], at[6][termios.VTIME] = 0, 1
    termios.tcsetattr(fd, termios.TCSANOW, at)
    termios.tcflush(fd, termios.TCIOFLUSH)

    def rd(t):
        end = time.monotonic() + t
        while time.monotonic() < end:
            b = os.read(fd, 1)
            if b:
                return b[0]
        return None
    print("waiting for the ROM's 'C' (CRC) or NAK (power the box on now) ...")
    end, crc = time.monotonic() + a.wait, None
    while time.monotonic() < end and crc is None:
        c = rd(1)
        if c == 0x43:
            crc = True
        elif c == 0x15:
            crc = False
        elif c is not None:
            sys.stderr.write(chr(c) if 32 <= c < 127 or c == 10 else ".")
    if crc is None:
        raise SystemExit("no receiver")
    blocks = [data[i:i + 128].ljust(128, b"\x1a") for i in range(0, len(data), 128)]
    for n, blk in enumerate(blocks, 1):
        tail = struct.pack(">H", crc16(blk)) if crc else bytes([sum(blk) & 0xFF])
        pkt = bytes([1, n & 0xFF, 0xFF - (n & 0xFF)]) + blk + tail
        for _ in range(10):
            os.write(fd, pkt)
            r = rd(3)
            if r == 0x06:
                break
            if r == 0x18:
                raise SystemExit(f"receiver cancelled at block {n}")
        else:
            raise SystemExit(f"block {n}: no ACK after 10 tries")
        if n % 64 == 0:
            print(f"  {n * 128:#x}/{len(data):#x}")
    for _ in range(10):
        os.write(fd, b"\x04")
        if rd(3) == 0x06:
            print(f"sent {len(data):#x} B in {len(blocks)} blocks ({'CRC' if crc else 'sum'})")
            break
    else:
        raise SystemExit("no ACK for EOT")
    end = time.monotonic() + a.listen
    while time.monotonic() < end:
        c = rd(0.2)
        if c is not None:
            sys.stdout.write(chr(c) if 32 <= c < 127 or c in (10, 13) else ".")
            sys.stdout.flush()
    os.close(fd)


ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
sub = ap.add_subparsers(dest="cmd", required=True)
p = sub.add_parser("image")
p.add_argument("--mtd", type=pathlib.Path, default=MTD0)
p.add_argument("--app", required=True, help="BOOT_MODE app id (0x110, 0x10000005, ...) or 'none'")
p.add_argument("--uboot", type=pathlib.Path, help="u-boot.bin linked at 0x08000000 (for 0x110)")
p.add_argument("out", type=pathlib.Path)
p = sub.add_parser("tramp")
p.add_argument("out", type=pathlib.Path)
p = sub.add_parser("route-a")
p.add_argument("--image", type=pathlib.Path, required=True)
p.add_argument("--key", help="stage3 key to send from `go` on (u, p, e)")
p.add_argument("--run", action="store_true", help="drive the factory prompt (default: dry run)")
p.add_argument("--server", default="10.60.2.208")
p.add_argument("--tftp-root", type=pathlib.Path, default=pathlib.Path("/srv/tftpboot"))
p.add_argument("--tftp-dir", default="dmc12/route-a")
p.add_argument("--timeout", type=float, default=60)
p = sub.add_parser("s2")
p.add_argument("mtd", type=pathlib.Path)
p.add_argument("out", type=pathlib.Path)
p = sub.add_parser("xmodem", description="Stop tio first: tmux send-keys -t serial-dmc12 C-t q "
                   "(the pane exits). Restart it afterwards with the same tio command and a new "
                   "--log-file (PROGRESS Status). Needs root for the tty, like tio.")
p.add_argument("--tty", required=True)
p.add_argument("--wait", type=float, default=600, help="seconds to wait for the receiver")
p.add_argument("--listen", type=float, default=20, help="seconds to echo output afterwards")
p.add_argument("file", type=pathlib.Path)
a = ap.parse_args()
{"image": cmd_image, "tramp": cmd_tramp, "route-a": cmd_route_a, "s2": cmd_s2,
 "xmodem": cmd_xmodem}[a.cmd](a)
