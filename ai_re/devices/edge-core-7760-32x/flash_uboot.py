#!/usr/bin/env python3
"""Flash upstream U-Boot onto an Accton/Edgecore AS7760-32X over its serial console.

Replaces only the 768 KiB "uboot" NOR partition (0xEFF40000..0xEFFFFFFF). RCW,
FMan microcode, ONIE, the environment and the NOS are not touched.

  ./flash_uboot.py --port /dev/ttyUSB0 --image u-boot.bin --tftp-server 192.0.2.10
  ./flash_uboot.py --port /dev/ttyUSB0 --image u-boot.bin --mmc u-boot-nor.bin
  ./flash_uboot.py ... --dry-run            # every check + RAM load, no flash write
  ./flash_uboot.py ... --image backup/uboot-before.bin --restore   # back to an earlier upstream build
                                            # (the factory 2014 U-Boot is refused: unsupported)

What it does, in order (anything unexpected aborts BEFORE the flash is touched):
  1. checks the image file (size, reset vector, board strings)
  2. gets a U-Boot prompt (factory "LOADER=>" or upstream "=>"); power-cycle
     the switch when asked
  3. hardware checks: SoC (SVR), reset configuration word, board identity,
     boot-flash mapping, DIMM part numbers
  4. backs up the current U-Boot and environment over the console (slow, ~7 min)
     and verifies the backup against a CRC computed by the switch
  5. loads the new image into RAM (TFTP or SD card) and verifies its CRC there;
     keeps a verified copy of the old U-Boot in RAM for rollback
  6. asks for confirmation, then unprotect / erase / write / compare / CRC
     (one retry; if that fails too, the old image is written back)
  7. resets, waits for the new banner, runs the DDR self-check ("ddrcmp")

There is ONE boot flash and no fallback bank: if the write is interrupted (power
loss) or the new image does not start, recovery needs a flash clip or JTAG and
the backup made in step 4 (NOR offset 0x7F40000). Keep that backup.

Needs python3 + pyserial (--port) or a tmux session whose pane is logged to a
file with "tmux pipe-pane" (--tmux/--capture). Nothing else may use the port.
"""
import argparse, hashlib, os, re, subprocess, sys, time, zlib
from pathlib import Path

FLASH_START, FLASH_END, SIZE = 0xEFF40000, 0xEFFFFFFF, 0xC0000
ENV_START, ENV_SIZE = 0xEFF20000, 0x20000
RAM_NEW, RAM_OLD = 0x01000000, 0x02000000
RESET_VECTOR = bytes.fromhex("4bfff004")          # "b .-0xffc" in the last word
SVR_T2080 = 0x85300000                            # T2080 / T2080E, any revision
RCW = ("0c060012 12000000 00000000 00000000 6c160002 00004200 fc027000 a1000000 "
       "00080000 00000001 00000000 00022002 00000000 50000009 00000000 00000005").split()
TESTED_DIMMS = {"U10U7211P8FU-BDE"}               # DDR setup was validated with these
KNOWN_IMAGES = {0x38c38261: "factory U-Boot 2014.07 1.7.0.10"}
PROMPT = re.compile(r"(?:LOADER)?=> ?$")
BAD = ("Unknown command", "ERROR", "Error:", "Bad ", "not erased", "Not Erased",
       "Timeout", "TIMEOUT", "Can't", "failed", "FAILED")


class Abort(Exception):
    pass


class Serial:
    def __init__(self, port, baud):
        import serial  # pyserial
        self.s = serial.Serial(port, baud, timeout=0.05)
    def write(self, text):
        for ch in text.encode():                  # old loaders drop fast input
            self.s.write(bytes([ch])); self.s.flush(); time.sleep(0.003)
    def read(self):
        return self.s.read(65536).decode(errors="replace")


class Tmux:
    def __init__(self, session, capture):
        self.session, self.cap = session, Path(capture)
        self.off = self.cap.stat().st_size
    def write(self, text):
        for ch in text.encode():                  # hex: tmux would eat a literal ";"
            subprocess.run(["tmux", "send-keys", "-t", self.session, "-H", f"{ch:02x}"], check=True)
            time.sleep(0.01)                      # old loaders drop fast input
    def read(self):
        with self.cap.open("rb") as f:
            f.seek(self.off); data = f.read()
        self.off += len(data)
        if not data:
            time.sleep(0.05)
        return data.decode(errors="replace")


class Console:
    def __init__(self, io, log):
        self.io, self.log = io, log
    def read_until(self, pattern, timeout, feed=None):
        buf, end = "", time.time() + timeout
        while time.time() < end:
            new = self.io.read()
            if new:
                buf += new; self.log.write(new); self.log.flush()
                if feed:
                    feed(buf)
                if pattern.search(buf.replace("\r", "")):
                    return buf.replace("\r", "")
        return None
    def cmd(self, text, timeout=30, check=True):
        self.io.read()
        self.io.write(text + "\r")
        out = self.read_until(PROMPT, timeout)
        if out is None:
            raise Abort(f"no prompt after {text!r} within {timeout}s")
        body = out.split("\n", 1)[1] if "\n" in out else ""
        body = PROMPT.sub("", body).strip()
        if check and any(b in body for b in BAD):
            raise Abort(f"{text!r} reported a problem:\n{body}")
        return body
    def crc32(self, addr, size):
        m = re.search(r"==>\s*([0-9a-f]{8})", self.cmd(f"crc32 {addr:x} {size:x}", 120))
        if not m:
            raise Abort("could not read a crc32 result")
        return int(m.group(1), 16)
    def words(self, addr, count):
        out = self.cmd(f"md.l {addr:x} {count:x}")
        w = []
        for line in out.splitlines():
            m = re.match(r"^[0-9a-f]{8}:((?: [0-9a-f]{8})+)", line)
            if m:
                w += m.group(1).split()
        if len(w) != count:
            raise Abort(f"md.l {addr:x}: expected {count} words, got {len(w)}")
        return w
    def dump(self, addr, size, label):
        data, step, t0 = bytearray(), 0x2000, time.time()
        while len(data) < size:
            n = min(step, size - len(data))
            out = self.cmd(f"md.b {addr + len(data):x} {n:x}", 180, check=False)
            got = bytearray()
            for line in out.splitlines():
                m = re.match(r"^[0-9a-f]{8}:((?: [0-9a-f]{2}){1,16})", line)
                if m:
                    got += bytes.fromhex(m.group(1).replace(" ", ""))
            if len(got) != n:
                raise Abort(f"console dump of {label} garbled at +0x{len(data):x} ({len(got)}/{n} bytes)")
            data += got
            done = len(data) / size
            eta = (time.time() - t0) / done * (1 - done)
            print(f"\r  {label}: {done:4.0%}  (about {eta:3.0f}s left) ", end="", flush=True)
        print()
        return bytes(data)


def step(title):
    print(f"\n== {title}")


def check_image(path, restore):
    data = path.read_bytes()
    problems = []
    if len(data) != SIZE:
        problems.append(f"size is {len(data)} bytes, must be exactly {SIZE} (0x{SIZE:x})")
    if data[-4:] != RESET_VECTOR:
        problems.append("last word is not the e6500 reset-vector branch (4bfff004): not a NOR boot image")
    if b"U-Boot 20" not in data:
        problems.append("no U-Boot version string inside")
    if b"U-Boot 2014." in data or (zlib.crc32(data) & 0xFFFFFFFF) in KNOWN_IMAGES:
        problems.append("this is the factory U-Boot (2014.07): going back to it is unsupported "
                        "(upstream -> upstream only; dumps/mtd7.bin stays as evidence, see u-boot-port.md)")
    if not restore and b"AS7760-32X" not in data:
        problems.append("no AS7760-32X board string inside (wrong board build? stock T2080RDB images must NOT be flashed)")
    if problems:
        raise Abort("image rejected:\n  - " + "\n  - ".join(problems))
    ver = re.search(rb"U-Boot 20[ -~]{5,70}", data)
    print(f"  {path}: {len(data)} bytes, sha256 {hashlib.sha256(data).hexdigest()}")
    print(f"  contains: {ver.group(0).decode()}")
    return data


def get_prompt(con, wait):
    con.io.write("\r")
    if con.read_until(PROMPT, 3):
        return
    print("  no prompt. POWER-CYCLE (or reset) THE SWITCH NOW; I will stop its autoboot.")
    stopped = []
    def feed(buf):
        if not stopped and re.search(r"autoboot", buf, re.I):
            stopped.append(1)
            con.io.write("123")               # factory stop string; any key for upstream
            time.sleep(0.3); con.io.write("\r")
    if con.read_until(PROMPT, wait, feed) is None:
        raise Abort("never reached a U-Boot prompt")
    time.sleep(1); con.io.write("\r"); con.read_until(PROMPT, 5)


def hardware_checks(con, skip):
    results = []
    def check(name, fn):
        if name in skip:
            results.append((name, "SKIPPED", "by --skip-check")); return
        try:
            results.append((name, "ok", fn()))
        except Abort as e:
            results.append((name, "FAIL", str(e)))
    def soc():
        svr = int(con.words(0xFE0E00A4, 1)[0], 16)
        if svr & 0xFFF70000 != SVR_T2080:
            raise Abort(f"SVR {svr:08x} is not a T2080")
        return f"SVR {svr:08x} (T2080)"
    def rcw():
        live = con.words(0xFE0E0100, 16)
        diff = [f"word {i}: {a} != {b}" for i, (a, b) in enumerate(zip(live, RCW)) if a != b]
        if diff:
            raise Abort("reset configuration word differs from the AS7760-32X this build was made for: " + "; ".join(diff))
        return "RCW matches (clocks, SerDes 0x6c/0x16, NOR boot)"
    def identity():
        out = con.cmd("printenv platform onie_platform onie_machine", check=False)
        if not re.search(r"as7760[_-]32x", out, re.I):
            raise Abort("environment does not identify an AS7760-32X (platform/onie_platform/onie_machine)")
        return re.search(r"\S*as7760[_-]32x\S*", out, re.I).group(0)
    def flash_map():
        last = con.words(FLASH_END - 3, 1)[0]
        if bytes.fromhex(last) != RESET_VECTOR:
            raise Abort(f"word at {FLASH_END - 3:x} is {last}, not the reset vector: boot flash is not mapped where expected")
        out = con.cmd("flinfo", 60, check=False)
        if not re.search(r"\b128 MB\b", out):
            raise Abort("flinfo does not show one 128 MB CFI flash")
        return "128 MB CFI NOR, running loader ends in the reset vector"
    def dimms():
        con.cmd("i2c dev 0", check=False)
        found = []
        for chip in (0x52, 0x53):
            out = con.cmd(f"i2c md {chip:x} 80 12", check=False)
            raw = bytearray()
            for line in out.splitlines():
                m = re.match(r"^[0-9a-f]{4}:((?: [0-9a-f]{2}){1,16})", line)
                if m:
                    raw += bytes.fromhex(m.group(1).replace(" ", ""))
            part = raw.decode(errors="replace").strip(" \x00")
            if len(raw) != 18:
                raise Abort(f"no SPD at I2C0 0x{chip:x}")
            if part not in TESTED_DIMMS:
                raise Abort(f"DIMM 0x{chip:x} is {part!r}; DDR timing was only validated with {sorted(TESTED_DIMMS)}")
            found.append(part)
        return "2x " + found[0]
    for name, fn in (("soc", soc), ("rcw", rcw), ("identity", identity),
                     ("flash-map", flash_map), ("dimms", dimms)):
        check(name, fn)
    for name, state, detail in results:
        print(f"  [{state:^7}] {name}: {detail}")
    if any(s == "FAIL" for _, s, _ in results):
        raise Abort("hardware checks failed; this image was built and tested for the AS7760-32X only "
                    "(--skip-check NAME overrides a check you have understood)")


def write_flash(con, src, crc, what):
    con.cmd(f"protect off {FLASH_START:x} {FLASH_END:x}", 60)
    try:
        out = con.cmd(f"erase {FLASH_START:x} {FLASH_END:x}", 300)
        if "Erased 6 sectors" not in out:
            raise Abort("erase did not report 6 sectors:\n" + out)
        con.cmd(f"cp.b {src:x} {FLASH_START:x} {SIZE:x}", 600)
        out = con.cmd(f"cmp.b {src:x} {FLASH_START:x} {SIZE:x}", 120, check=False)
        if f"Total of {SIZE} byte(s) were the same" not in out:
            raise Abort("flash does not compare equal to RAM:\n" + out)
        if con.crc32(FLASH_START, SIZE) != crc:
            raise Abort("flash CRC mismatch after write")
    finally:
        con.cmd(f"protect on {FLASH_START:x} {FLASH_END:x}", 60, check=False)
    print(f"  {what} written and verified (crc32 {crc:08x})")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--image", type=Path, required=True, help="u-boot.bin of the as7760-nor build (768 KiB)")
    ap.add_argument("--port", help="serial device (115200 8N1), needs pyserial")
    ap.add_argument("--baud", type=int, default=115200)
    ap.add_argument("--tmux", help="tmux session holding the console instead of --port")
    ap.add_argument("--capture", help="file that session's pane is piped to (tmux pipe-pane)")
    ap.add_argument("--tftp-server", help="TFTP server holding the image")
    ap.add_argument("--tftp-path", help="path on the TFTP server (default: image file name)")
    ap.add_argument("--ip", help="static address for the switch (default: DHCP)")
    ap.add_argument("--mmc", metavar="FILE", help="load FILE from the SD card's first FAT partition instead of TFTP")
    ap.add_argument("--backup-dir", type=Path, default=None)
    ap.add_argument("--restore", action="store_true", help="--image is a backup of this switch's U-Boot")
    ap.add_argument("--dry-run", action="store_true", help="do everything except writing the flash")
    ap.add_argument("--no-reset", action="store_true", help="leave the switch at the old prompt after writing")
    ap.add_argument("--skip-check", action="append", default=[], metavar="NAME",
                    help="soc | rcw | identity | flash-map | dimms")
    ap.add_argument("--yes", action="store_true", help="do not ask for the typed confirmation")
    ap.add_argument("--wait", type=int, default=600, help="seconds to wait for the power cycle")
    a = ap.parse_args()
    if bool(a.port) == bool(a.tmux) or (a.tmux and not a.capture):
        ap.error("give either --port, or --tmux with --capture")
    if bool(a.tftp_server) == bool(a.mmc):
        ap.error("give exactly one of --tftp-server / --mmc")

    step("1/7 image file")
    image = check_image(a.image, a.restore)
    new_crc = zlib.crc32(image)

    bdir = a.backup_dir or Path(time.strftime("as7760-backup-%Y%m%d-%H%M%S"))
    bdir.mkdir(parents=True, exist_ok=True)
    log = (bdir / "console.txt").open("a")
    con = Console(Serial(a.port, a.baud) if a.port else Tmux(a.tmux, a.capture), log)

    step("2/7 U-Boot prompt")
    get_prompt(con, a.wait)
    running = con.cmd("version", check=False).splitlines()[0]
    print(f"  running: {running}")

    step("3/7 hardware checks (read-only)")
    hardware_checks(con, set(a.skip_check))

    step("4/7 backup of the current U-Boot and environment")
    old_crc = con.crc32(FLASH_START, SIZE)
    print(f"  current U-Boot crc32 {old_crc:08x}" + (f" = {KNOWN_IMAGES[old_crc]}" if old_crc in KNOWN_IMAGES else ""))
    if old_crc == new_crc:
        print("  the flash already holds exactly this image; nothing to do."); return 0
    for name, addr, size in (("uboot-before.bin", FLASH_START, SIZE), ("env-before.bin", ENV_START, ENV_SIZE)):
        want = old_crc if addr == FLASH_START else con.crc32(addr, size)
        data = con.dump(addr, size, name)
        if zlib.crc32(data) != want:
            raise Abort(f"{name}: console dump does not match the switch's CRC; not continuing without a good backup")
        f = bdir / name
        f.write_bytes(data); os.chmod(f, 0o600)
        print(f"  saved {f} (verified, sha256 {hashlib.sha256(data).hexdigest()[:16]}...)")
    print(f"  NOTE: env-before.bin holds this switch's private keys/MACs; keep {bdir} private.")

    step("5/7 load the new image into RAM")
    if a.mmc:
        con.cmd("mmc rescan", 30, check=False)
        out = con.cmd(f"fatload mmc 0:1 {RAM_NEW:x} {a.mmc}", 60)
        ok = f"{SIZE} bytes read" in out
    else:
        con.cmd("setenv autoload no")
        if a.ip:
            con.cmd(f"setenv ipaddr {a.ip}")
        else:
            for attempt in range(3):          # the uplink port may still be coming up
                if "bound to address" in con.cmd("dhcp", 120, check=False):
                    break
            else:
                raise Abort("DHCP failed (give --ip for a static address)")
        con.cmd(f"setenv serverip {a.tftp_server}")
        out = con.cmd(f"tftpboot {RAM_NEW:x} {a.tftp_path or a.image.name}", 180)
        ok = f"Bytes transferred = {SIZE} " in out
    if not ok:
        raise Abort("the switch did not load exactly 786432 bytes:\n" + out)
    if con.crc32(RAM_NEW, SIZE) != new_crc:
        raise Abort("image CRC in the switch's RAM differs from the local file")
    con.cmd(f"cp.b {FLASH_START:x} {RAM_OLD:x} {SIZE:x}", 120)
    if con.crc32(RAM_OLD, SIZE) != old_crc:
        raise Abort("RAM copy of the old U-Boot is not intact (bad RAM?)")
    print(f"  new image verified in RAM (crc32 {new_crc:08x}); old image kept in RAM for rollback")

    if a.dry_run:
        print("\nDRY RUN: all checks passed, flash not touched."); return 0

    step("6/7 write the boot flash")
    print("  From here on do NOT cut power or touch the console until it says 'verified'.\n"
          "  If the new loader does not start, recovery needs a flash clip or JTAG and\n"
          f"  {bdir}/uboot-before.bin (NOR offset 0x7F40000).")
    if not a.yes and input("  Type FLASH AS7760 to continue: ").strip() != "FLASH AS7760":
        raise Abort("not confirmed; flash untouched")
    try:
        try:
            write_flash(con, RAM_NEW, new_crc, "new U-Boot")
        except Abort as e:
            print(f"  write failed ({e}); retrying once")
            write_flash(con, RAM_NEW, new_crc, "new U-Boot")
    except Abort as e:
        print(f"  second attempt failed too ({e}); WRITING THE OLD U-BOOT BACK")
        try:
            write_flash(con, RAM_OLD, old_crc, "old U-Boot (rollback)")
        except Abort as e2:
            raise Abort("ROLLBACK FAILED: " + str(e2) + "\n  DO NOT RESET OR POWER OFF. The switch still runs from RAM: "
                        "retry by hand (protect off / erase / cp.b / cmp.b) or get a flash clip ready.")
        raise Abort("new image could not be written; the old U-Boot is back in place and verified")

    if a.no_reset:
        print("\nDone; the new U-Boot starts at the next reset."); return 0

    step("7/7 first boot of the new U-Boot")
    con.io.write("reset\r")
    banner = re.compile(re.escape(re.search(rb"U-Boot 20[ -~]{5,40}", image).group(0).decode().split(" (")[0]))
    if con.read_until(banner, 90) is None:
        raise Abort("NO BANNER from the new U-Boot within 90 s. Power-cycle once; if it stays silent, "
                    f"restore {bdir}/uboot-before.bin with a flash clip/JTAG at NOR offset 0x7F40000.")
    stopped = []
    def feed(buf):
        if not stopped and "autoboot" in buf:
            stopped.append(1); con.io.write("\r")
    if con.read_until(PROMPT, 120, feed) is None:
        raise Abort("banner seen but no prompt; watch the console")
    print("  " + con.cmd("version", check=False).splitlines()[0])
    out = con.cmd("ddrcmp", 60, check=False)
    m = re.search(r"(\d+) register\(s\) differ", out)
    if m:
        print(f"  DDR self-check: {m.group(0)}" + ("" if m.group(1) == "0" else "  <-- investigate before trusting RAM"))
    print("\nDone. The factory environment is kept: 'boot' runs the old bootcmd (NOS/ONIE).\n"
          "For standard boot (extlinux/boot.scr on SD, PXE): setenv nos_bootcmd 'bootflow scan -lb'; saveenv")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Abort as e:
        print(f"\nABORTED: {e}", file=sys.stderr); sys.exit(1)
    except KeyboardInterrupt:
        print("\ninterrupted. If the flash write had started, DO NOT reset the switch; "
              "rerun this script or finish by hand.", file=sys.stderr); sys.exit(130)
