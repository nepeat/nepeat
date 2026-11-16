#!/usr/bin/env python3
"""Run the alpine-v3 / CS8210 U-Boot board code and config on QEMU virt
(EL2, Cortex-A72, PSCI over SMC).

  1. Entered like the vendor booti/bootm does (Image header, MMU off,
     x0 = DTB): banner, entry report, two DRAM banks, relocation below the
     /reserved-memory top range. Then "bootflow scan -lb" goes ECAM -> AHCI
     (the disabled function left alone) -> SCSI -> MBR -> the bootable ext4
     partition -> /boot/boot.scr, which is the CS8210 NixOS launcher (scsi
     init; ext4load u-boot.menu; env import -t; bootmenu), whose entry loads
     a probe "kernel", a mkimage-wrapped initrd and a DTB and booti's them.
     The probe must see EL2, a valid DTB, and in its memreserve map the
     control DT range the OS DTB lacks, but not the one it has.
  2. From that U-Boot's prompt (MMU and caches on), "go" to a second copy
     loaded elsewhere: it must report the MMU-on entry and reach a prompt;
     "reset" then goes through PSCI SYSTEM_RESET back to a fresh banner.
usage: qemu-test.py <qemu-system-aarch64> <u-boot.bin> <disk.img>
"""
import os, re, select, subprocess, sys, time

qemu, uboot, disk = sys.argv[1:4]
BASE = [qemu, "-M", "virt,highmem=off,virtualization=on,gic-version=3",
        "-cpu", "cortex-a72", "-m", "2G", "-nographic", "-monitor", "none",
        "-serial", "stdio", "-nic", "none", "-kernel", uboot]
# Board env addresses assume DRAM at 0 (Alpine); QEMU's starts at 1 GiB.
QENV = ("setenv scriptaddr 0x47000000; setenv pxefile_addr_r 0x47200000; "
        "setenv kernel_addr_r 0x48000000; setenv fdt_addr_r 0x50000000; "
        "setenv ramdisk_addr_r 0x60000000")


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
         "-device", "ahci,id=ahci2,addr=5",
         "-drive", f"if=none,id=d0,format=raw,file={disk},snapshot=on"])
try:
    r.until(r"U-Boot 2026\.10-alpine-v3-qemu", 60)
    r.until(r"CPU:   MIDR 0x410fd08\d \(Cortex-A72\)", 30)
    r.until(r"Entry: EL2, MMU\+caches were off \(booti/bootm/stage3\)", 30)
    r.until(r"DRAM:\s+1 GiB \(total 2 GiB\)", 30)
    r.prompt()
    m = r.cmd("bdinfo", r"relocaddr\s+= (0x[0-9a-f]+)\r?\n")
    reloc = int(m.group(1), 16)
    assert 0x70000000 < reloc < 0x7c000000, f"relocaddr {reloc:#x} not below the reserved top"
    r.cmd("bdinfo", r"DRAM bank\s+= 0x0+1\r?\n-> start\s+= 0x0*80000000")
    r.cmd(QENV)
    r.buf = b""
    r.send("bootflow scan -lb")
    seen = r.until(r"(?s)(.*?)== CS8210 NixOS bootmenu ==", 60).group(1)
    assert b"ahci_scsi" in seen and b"/boot/boot.scr" in seen, "boot.scr not found by bootstd"
    assert seen.count(b"SATA link") <= 6, "a disabled AHCI function was probed"
    r.until(r"NixOS - Default \(test\)", 30)
    r.until(r"## Loading init Ramdisk from Legacy Image at 60000000", 30)
    r.until(r"Verifying Checksum \.\.\. OK", 10)
    r.until(r"Loading Device Tree to 0*7[0-9a-b]", 10)   # copied below U-Boot
    r.until(r"Starting kernel", 30)
    r.until(r"PROBE: EL2 x0=0x[0-9a-f]{16} magic=ok", 30)
    rsv = r.until(r"(?s)(.*?)PROBE: done", 10).group(1).decode(errors="replace")
    assert "PROBE: rsv 0x000000007c000000 0x0000000004000000" in rsv, rsv
    assert "PROBE: rsv 0x0000000040000000" not in rsv, "a range the OS DTB has was copied"
finally:
    r.kill()
print("\n-- stage 1 ok: booti-style entry, banks, relocation, AHCI boot.scr -> bootmenu -> booti, memreserves\n")

r = Run(["-device", f"loader,file={uboot},addr=0x50000000,force-raw=on"])
try:
    r.until(r"Entry: EL2, MMU\+caches were off", 60)
    r.prompt()
    r.cmd("dcache", r"Cache is ON")
    r.buf = b""
    r.send("go 0x50000000")
    r.until(r"Starting application at 0x50000000", 10)
    r.until(r"U-Boot 2026\.10-alpine-v3-qemu", 30)
    r.until(r"Entry: EL2, MMU\+caches were on \(go\)", 30)
    r.prompt()
    r.cmd("version", r"U-Boot 2026\.10")
    r.buf = b""
    r.send("reset")
    r.until(r"resetting", 10)
    r.until(r"U-Boot 2026\.10-alpine-v3-qemu", 30)
    r.until(r"Entry: EL2, MMU\+caches were off", 30)
finally:
    r.kill()
print("\nPASS: alpine-v3 U-Boot on QEMU virt (booti-style and MMU-on go entry, boot.scr bootmenu, PSCI reset)")
