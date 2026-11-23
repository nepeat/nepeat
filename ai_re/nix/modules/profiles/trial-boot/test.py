#!/usr/bin/env python3
"""trial-boot's U-Boot half on QEMU (see test.nix).
usage: test.py <qemu-system-aarch64> <u-boot.bin> <lba> name=disk.img... <outdir>
Each scenario copies a disk, optionally writes the one-shot request, and boots
until the default system powers off; every U-Boot start is stopped at its
prompt to set the QEMU load addresses (DRAM at 1 GiB) and, as the board's
persistent env does, bootmeths = "script extlinux".
"""
import os, re, select, shutil, subprocess, sys, time

qemu, uboot, lba = sys.argv[1], sys.argv[2], int(sys.argv[3])
disks = dict(a.split("=", 1) for a in sys.argv[4:-1])
out = sys.argv[-1]
QENV = ("setenv fdt_addr_r 0x4f000000; setenv kernel_addr_r 0x50000000; "
        "setenv ramdisk_addr_r 0x60000000; setenv scriptaddr 0x4f200000; "
        "setenv pxefile_addr_r 0x4f300000")
REQ = b"TRIALBT1 abc123 /nix/store/0123456789abcdfghijklmnpqrsvwxyz-trial\n".ljust(512, b"\0")


def sector(path):
    with open(path, "rb") as f:
        f.seek(lba * 512)
        return f.read(512)


def run(name, disk, request, bootmeths=True, max_boots=4):
    img = os.path.join(out, f"{name}.img")
    shutil.copy(disk, img); os.chmod(img, 0o644)
    if request:
        with open(img, "r+b") as f:
            f.seek(lba * 512); f.write(REQ)
    p = subprocess.Popen([qemu, "-M", "virt,highmem=off,virtualization=on,gic-version=3",
                          "-cpu", "cortex-a57", "-m", "1G", "-nographic", "-monitor", "none",
                          "-serial", "stdio", "-nic", "none", "-kernel", uboot,
                          "-device", "ahci,id=ahci", "-device", "ide-hd,drive=d0,bus=ahci.0",
                          "-drive", f"if=none,id=d0,format=raw,file={img}"],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    buf, boots, log = b"", [], open(os.path.join(out, f"{name}.console.txt"), "wb")
    pos = 0
    def until(pat, timeout):
        nonlocal buf, pos
        end = time.time() + timeout
        rx = re.compile(pat.encode())
        while time.time() < end:
            m = rx.search(buf, pos)
            if m:
                pos = m.end(); return m
            r, _, _ = select.select([p.stdout], [], [], 0.5)
            if r:
                c = os.read(p.stdout.fileno(), 4096)
                if not c:
                    return None
                buf += c; log.write(c); log.flush()
        raise SystemExit(f"FAIL: {name}: no /{pat}/ within {timeout}s")
    def send(s):
        p.stdin.write(s.encode() + b"\r"); p.stdin.flush()
    try:
        while True:
            m = until(r"Hit any key to stop autoboot|TB: default powers off", 120)
            if m is None:
                break
            if m.group(0).startswith(b"TB"):
                p.wait(60); break
            boots.append(m.start())
            if len(boots) > max_boots:
                raise SystemExit(f"FAIL: {name}: more than {max_boots} boots")
            send(""); until(r"=> ", 30)
            send(QENV); until(r"=> ", 30)
            if bootmeths:
                send('setenv bootmeths "script extlinux"'); until(r"=> ", 30)
            send("bootflow scan -lb")
    finally:
        p.kill(); log.close()
    text = buf.decode(errors="replace")
    parts = [text[s:e] for s, e in zip(boots, boots[1:] + [len(buf)])]
    left = sector(img)
    os.unlink(img)
    return parts, left


def want(part, *pats, absent=()):
    pos = 0
    for pat in pats:
        m = re.compile(pat).search(part, pos)
        if not m:
            raise SystemExit(f"FAIL: missing /{pat}/ (in order) in:\n{part}")
        pos = m.end()
    for pat in absent:
        if re.search(pat, part):
            raise SystemExit(f"FAIL: unexpected /{pat}/ in:\n{part}")


ZERO = bytes(512)
results = []
def scenario(title, fn):
    t = time.time(); fn(); results.append(f"ok: {title} ({time.time() - t:.0f} s)"); print(results[-1], flush=True)

def s_idle():
    parts, left = run("idle", disks["base"], request=False)
    assert len(parts) == 1, len(parts)
    want(parts[0], r"with script", r"trial-boot: no request", r"tb-test: disarm", r"with extlinux",
         r"TB: role=default trialboot=\r?\n", absent=[r"role=trial"])
    assert left == ZERO

def s_trial():
    parts, left = run("trial", disks["base"], request=True)
    assert len(parts) == 2, len(parts)
    want(parts[0], r"trial-boot: trial requested", r"trial-boot: request cleared", r"tb-test: arm",
         r"watchdog armed, booting the trial", r"TB: role=trial trialboot=abc123", r"TB: trial reboots",
         absent=[r"role=default"])
    want(parts[1], r"trial-boot: no request", r"TB: role=default", absent=[r"role=trial"])
    assert left == ZERO, "request not consumed"

def s_panic():
    parts, left = run("panic", disks["panic"], request=True)
    assert len(parts) == 2, len(parts)
    want(parts[0], r"booting the trial", r"rdinit=/bogus", r"Kernel panic", r"Rebooting in 3 seconds",
         absent=[r"TB: role="])
    want(parts[1], r"trial-boot: no request", r"TB: role=default")
    assert left == ZERO

def s_noconf():
    parts, left = run("noconf", disks["noconf"], request=True)
    assert len(parts) == 1, len(parts)
    want(parts[0], r"request cleared", r"tb-test: arm", r"the trial did not boot, going on with the default",
         r"tb-test: disarm", r"with extlinux", r"TB: role=default", absent=[r"role=trial"])
    assert left == ZERO

def s_armfails():
    parts, left = run("armfails", disks["armfails"], request=True)
    assert len(parts) == 1, len(parts)
    want(parts[0], r"request cleared", r"tb-test: arm fails", r"could not arm the watchdog",
         r"tb-test: disarm", r"TB: role=default", absent=[r"role=trial", r"booting the trial"])
    assert left == ZERO

def s_noscript():
    parts, left = run("noscript", disks["noscript"], request=True)
    assert len(parts) == 1, len(parts)
    want(parts[0], r"with extlinux", r"TB: role=default", absent=[r"trial-boot:", r"role=trial"])
    assert left[:8] == b"TRIALBT1", "nothing may consume the request without the script"

def s_nobootmeths():
    parts, left = run("nobootmeths", disks["base"], request=True, bootmeths=False)
    assert len(parts) == 1, len(parts)
    want(parts[0], r"with extlinux", r"TB: role=default", absent=[r"trial-boot:", r"role=trial"])
    assert left[:8] == b"TRIALBT1"

scenario("no request: the script runs first, finds nothing, standard boot goes on to the default", s_idle)
scenario("request: trial once (watchdog armed, request cleared), its reboot lands in the default", s_trial)
scenario("request, trial kernel panics (rdinit=/bogus, panic=3): next boot is the default", s_panic)
scenario("request, trial conf missing: disarm, default; request consumed", s_noconf)
scenario("request, watchdog arm fails: trial not booted, default; request consumed", s_armfails)
scenario("no boot.scr: extlinux default as before; request left pending (stage refuses this setup)", s_noscript)
scenario("bootmeths unset: extlinux first, default; request left pending (stage refuses this setup)", s_nobootmeths)
with open(os.path.join(out, "summary.txt"), "w") as f:
    f.write("\n".join(results) + "\n")
print("PASS: trial-boot U-Boot script, 7 scenarios")
