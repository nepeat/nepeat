#!/usr/bin/env python3
"""Run the alpine-v2 U-Boot board code on QEMU virt (EL2, Cortex-A57).

  1. Entered like the vendor bootm does (Image header, MMU off, x0 = DTB):
     banner, entry report, relocation below the /reserved-memory top range,
     then bootflow scan over ECAM -> AHCI -> SCSI -> GPT -> ext4 -> extlinux
     -> booti of a probe "kernel", which must see EL2, a valid DTB and the
     reserved ranges in its memreserve map (ft_board_setup). On the way the
     al_eth handoff (board_prep_linux) must run and report no NIC.
  2. From that U-Boot's prompt (MMU and caches on), "go" to a second copy
     loaded elsewhere: it must report the MMU-on entry and reach a prompt.
usage: qemu-test.py <qemu-system-aarch64> <u-boot.bin> <disk.img>
"""
import os, re, select, subprocess, sys, time

qemu, uboot, disk = sys.argv[1:4]
BASE = [qemu, "-M", "virt,highmem=off,virtualization=on,gic-version=3",
        "-cpu", "cortex-a57", "-m", "1G", "-nographic", "-monitor", "none",
        "-serial", "stdio", "-nic", "none", "-kernel", uboot]
# Board env addresses assume DRAM at 0 (Alpine); QEMU's starts at 1 GiB.
QENV = ("setenv fdt_addr_r 0x4f000000; setenv kernel_addr_r 0x50000000; "
        "setenv ramdisk_addr_r 0x60000000; setenv scriptaddr 0x4f200000; "
        "setenv pxefile_addr_r 0x4f300000")


class Run:
    def __init__(self, extra):
        self.p = subprocess.Popen(BASE + extra, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.buf = b""

    def until(self, pattern, timeout):
        end = time.time() + timeout
        while time.time() < end:
            m = re.search(pattern.encode(), self.buf)
            if m:
                self.buf = self.buf[m.end():]
                return m
            r, _, _ = select.select([self.p.stdout], [], [], 0.5)
            if r:
                chunk = os.read(self.p.stdout.fileno(), 4096)
                if not chunk:
                    break
                self.buf += chunk
                sys.stdout.write(chunk.decode(errors="replace")); sys.stdout.flush()
        raise SystemExit(f"FAIL: no /{pattern}/ within {timeout}s")

    def send(self, line):
        self.p.stdin.write(line.encode() + b"\r"); self.p.stdin.flush()

    def cmd(self, line, expect=None, timeout=30):
        self.buf = b""
        self.send(line)
        m = self.until(expect, timeout) if expect else None
        self.until(r"=> ", timeout)
        return m

    def prompt(self):
        self.until(r"Hit any key to stop autoboot", 60)
        self.send("")
        self.until(r"=> ", 30)

    def kill(self):
        self.p.kill()


r = Run(["-device", "ahci,id=ahci", "-device", "ide-hd,drive=d0,bus=ahci.0",
         "-drive", f"if=none,id=d0,format=raw,file={disk},snapshot=on"])
try:
    r.until(r"U-Boot 2026\.10-alpine-v2-qemu", 60)
    r.until(r"Entry: EL2, MMU\+caches were off \(bootm\)", 30)
    r.prompt()
    m = r.cmd("bdinfo", r"relocaddr\s+= (0x[0-9a-f]+)\r?\n")
    reloc = int(m.group(1), 16)
    assert 0x70000000 < reloc < 0x7c000000, f"relocaddr {reloc:#x} not below the reserved top"
    r.cmd(QENV)
    r.buf = b""
    r.send("bootflow scan -lb")
    r.until(r"ahci_scsi\S* /boot/extlinux/extlinux\.conf", 60)
    # board_prep_linux: the al_eth handoff runs and copes with no NIC.
    r.until(r"al_eth: no 1c36:0001 function on the PCI bus", 60)
    r.until(r"Starting kernel", 60)
    r.until(r"PROBE: EL2 x0=0x[0-9a-f]{16} magic=ok", 30)
    r.until(r"PROBE: rsv 0x0000000040000000 0x0000000000100000", 10)
    r.until(r"PROBE: rsv 0x000000007c000000 0x0000000004000000", 10)
    r.until(r"PROBE: done", 10)
finally:
    r.kill()
print("\n-- stage 1 ok: bootm-style entry, relocation, AHCI extlinux booti, memreserves\n")

r = Run(["-device", f"loader,file={uboot},addr=0x50000000,force-raw=on"])
try:
    r.until(r"Entry: EL2, MMU\+caches were off", 60)
    r.prompt()
    r.cmd("dcache", r"Cache is ON")
    r.buf = b""
    r.send("go 0x50000000")
    r.until(r"Starting application at 0x50000000", 10)
    r.until(r"U-Boot 2026\.10-alpine-v2-qemu", 30)
    r.until(r"Entry: EL2, MMU\+caches were on \(go\)", 30)
    r.prompt()
    r.cmd("version", r"U-Boot 2026\.10")
finally:
    r.kill()
print("\nPASS: alpine-v2 U-Boot on QEMU virt (bootm-style and MMU-on go entry)")
