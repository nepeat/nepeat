#!/usr/bin/env python3
"""CS8210: start the RAM-only upstream U-Boot (#uboot-cs8210) from the vendor U-Boot, then boot
NixOS with it. Milestone 2 of ../u-boot-port.md. Writes RAM only: our build has no SPI write
or erase path (patch 0019 + 0025), no env storage, no ext4/FAT write; the vendor side runs only
loadx/crc32/booti|go. Recovery from any hang: power-cycle lab plug 1 (vendor boot is untouched).

  sudo uboot_chainload.py capture LABEL      Linux side, read-only: dmesg, lspci, DT, links,
                                             AVS VOUT, GPIO, SONiC state, boot timeline
                                             -> ../analysis/uboot-compare/LABEL/
  sudo uboot_chainload.py run RESULT [--entry booti|go] [--no-boot]
        RESULT = nix build path:./nix#uboot-cs8210 output. Reboots Linux into the vendor
        prompt (123 Enter), loadx RESULT/u-boot.bin to 0x08000000 over the DMC-12 consoled
        (uart_recovery.py xmodem), crc32 check, enters it, stops our autoboot, read-only checks
        (banner/EL2/DRAM, PCI, SCSI, partitions, SPI NOR crc32 vs the 11-15 dump, GPIO bank 6,
        bootflow list), then 'boot' and waits for the login prompt and the cage guard line.
Transcript: ../serial-uboot-<date>.txt (git-ignored). Exit 0 only on PASS. Needs root's ssh key
(consoled and root@cs8210), so run with sudo.
"""
import argparse, pathlib, shlex, subprocess, sys, time, zlib

HERE = pathlib.Path(__file__).resolve().parent
DEV = HERE.parent
CONSOLE, PORT, HOST = "console@10.60.2.10", "cs8210", "root@10.60.2.190"
LOAD = 0x08000000
VENDOR = "ALPINE_DB> ?$"                     # the vendor prompt; ours is "=> "
# SPI NOR (images/spi-2026-11-15, = the live /dev/mtd0,1,4 on 2026-11-16), zlib CRC32
SPI = [(0x000000, 0x0380000, "73264376"),   # mtd0: stage2, TOC, DT, FW, stage3, vendor U-Boot
       (0x380000, 0x0040000, "8b4a0ac9"),   # mtd1: vendor env (changes only with a saveenv)
       (0x410000, 0x1bf0000, "d6937f49")]   # mtd4: ONIE, crosses 16 MiB (4-byte reads)
CAPTURE = [
    ("uname", "uname -a; cat /proc/cmdline"),
    ("dmesg", "dmesg"),
    ("lspci", "lspci -nnvvv"),
    ("lspci-tree", "lspci -tvnn"),
    ("fdt.dts", "dtc -q -I fs -O dts /sys/firmware/devicetree/base 2>/dev/null || echo no dtc"),
    ("links", "ip -br link; ip -br addr"),
    ("gpio", "cat /sys/kernel/debug/gpio 2>/dev/null || echo no debugfs"),
    # ASIC core regulator, PMBus reads only: i2c-16 = 0x70 ch2 -> 0x74 ch0 -> 0x72 ch2 (this boot's
    # numbering, check i2cdetect -l); VOUT_MODE 0x16, VOUT_COMMAND / READ_VOUT ~0x0333 = 800 mV
    ("avs", "for r in 0x20 0x21 0x8b; do printf '%s ' $r; i2cget -y -f 16 0x62 $r w; done"),
    ("units", "systemctl --failed --no-legend; systemctl is-system-running"),
    ("timeline", "journalctl -b -o short-monotonic --no-pager | grep -E 'cage guard|Credo|PortInitDone|"
                 "Established|BFD.*[Uu]p|LACP|Started sshd' | head -60"),
    ("sonic", "vtysh -c 'show bgp vrf all summary' 2>&1; vtysh -c 'show bfd peers brief' 2>&1; "
              "for p in PortChannel1 PortChannel2; do teamdctl $p state 2>&1 | head -20; done"),
    ("mtd", "cat /proc/mtd"),
]

log = DEV / time.strftime("serial-uboot-%Y%m%d-%H%M%S.txt")
lines = []


def note(text):
    lines.append(text)
    log.write_text("\n".join(lines) + "\n")


def finish(ok, why=""):
    msg = ("PASS" if ok else "FAIL") + (": " + why if why else "")
    note(msg)
    print(msg, f"(transcript {log.name})")
    sys.exit(0 if ok else 1)


def line(*words, timeout=60):
    """One consoled 'line' verb over ssh; words are quoted for its shell-like splitter."""
    cmd = ["ssh", "-o", "ConnectTimeout=10", CONSOLE, " ".join(shlex.quote(w) for w in words)]
    r = subprocess.run(cmd, capture_output=True, text=True, errors="replace", timeout=timeout + 30)
    return r.returncode, r.stdout + r.stderr


def send(text):
    rc, out = line("send", PORT, "--take", text)
    note(f">> {text}")
    if rc:
        finish(False, f"send {text!r}: rc {rc} {out.strip()}")


def expect(pattern, timeout):
    rc, out = line("expect", PORT, pattern, "--timeout", str(timeout), timeout=timeout)
    note(out.rstrip())
    if rc:
        finish(False, f"no /{pattern}/ within {timeout}s")
    return out


def run(cmd, want="", timeout=60, prompt="=> $", bad=("Unknown command", "Synchronous Abort", "\"Error\" handler",
                                       "data abort", "### ERROR", "resetting")):
    rc, out = line("run", PORT, "--take", "--prompt", prompt, "--timeout", str(timeout), cmd,
                   timeout=timeout)
    note(f"{prompt[:-1]}{cmd}\n{out.rstrip()}")
    if rc or any(b in out for b in bad) or (want and want not in out):
        finish(False, f"{cmd!r}: wanted {want!r}: {out.strip()[-600:]}")
    return out


def scrollback():
    rc, out = line("log", PORT, timeout=60)
    return out


def capture(label):
    out = DEV / "analysis" / "uboot-compare" / label
    out.mkdir(parents=True, exist_ok=True)
    for name, cmd in CAPTURE:
        r = subprocess.run(["ssh", "-o", "ConnectTimeout=10", HOST, cmd], capture_output=True,
                           text=True, errors="replace", timeout=120)
        (out / f"{name}.txt").write_text(r.stdout + r.stderr)
    print(f"captured {len(CAPTURE)} files in {out}")


def chainload(result, entry, boot):
    ubin = result / "u-boot.bin"
    data = ubin.read_bytes()
    size, crc = len(data), f"{zlib.crc32(data):08x}"
    note(f"image {ubin.resolve()} {size:#x} B crc32 {crc}")
    # 1. Linux -> vendor prompt (bootdelay 3, stop string "123" + Enter). 'expect' only sees
    #    output newer than its own start, so wait once, for the last line before each key press.
    subprocess.run(["ssh", "-o", "ConnectTimeout=10", HOST, "systemctl reboot"], timeout=60)
    note("ssh: systemctl reboot")
    expect("Type 123<ENTER> to STOP autoboot", 400)
    send("123")
    run("version", "2018.05-alpine_db", prompt=VENDOR)
    # 2. loadx + check (XMODEM pads to 128 B; compare the real size)
    r = subprocess.run([sys.executable, str(HERE / "uart_recovery.py"), "xmodem", "--consoled",
                        f"{CONSOLE}:{PORT}", "--take", "--first", f"loadx {LOAD:#x}", str(ubin)],
                       capture_output=True, text=True, errors="replace", timeout=600)
    note(r.stdout + r.stderr)
    if r.returncode:
        finish(False, "xmodem failed")
    run(f"crc32 {LOAD:#x} {size:#x}", f"==> {crc}", prompt=VENDOR)
    # 3. enter: booti = the Linux path (MMU off, x0 = the vendor DT, its pre-OS hooks run);
    #    go = MMU and caches on, our lowlevel.S turns them off. Stop our autoboot, then read
    #    the banner back from the scrollback.
    send(f"booti {LOAD:#x} - ${{fdtaddr}}" if entry == "booti" else f"go {LOAD:#x}")
    expect("Hit any key to stop autoboot", 90)
    send(" ")
    run("version", "U-Boot 2026.10-cs8210")
    sb = scrollback()
    banner = sb[sb.rfind("U-Boot 2026.10-cs8210"):][:3000]
    note("banner: " + " | ".join(l for l in banner.splitlines() if l.startswith(("CPU:", "Model:", "Entry:", "DRAM:"))))
    if "Entry: EL2" not in banner:
        finish(False, "not entered at EL2 (or no banner): stop here, power-cycle plug 1")
    if "DRAM:  2 GiB (total 8 GiB)" not in banner:
        finish(False, "DRAM banks not as the DT says")
    run("bdinfo", "relocaddr")
    run("pci", "1c36")                       # internal bus: al_eth, crypto, AHCI 00:0b.0
    run("scsi scan", "HBS3A1912A4M4B1", 120)
    run("part list scsi 0", "49152")
    run("ls scsi 0:2 /boot", "boot.scr")
    run("sf probe 0:0", "mx25u25635f")
    for off, length, want in SPI:
        run(f"sf read 0x20000000 {off:#x} {length:#x}", "OK", 300)
        run(f"crc32 0x20000000 {length:#x}", f"==> {want}")
    run("md.l 0xfd8983fc 1; md.l 0xfd898400 1", "")   # GPIO bank 6 data/dir (vendor: eb / fc)
    run("bootflow scan -l", "/boot/boot.scr", 120)
    if not boot:
        finish(True, "at our prompt; not booting (--no-boot)")
    # 4. boot NixOS: boot.scr -> bootmenu (5 s) -> booti
    send("boot")
    expect("login: $", 400)
    s = scrollback()[-200000:]
    for want in ("CS8210 NixOS bootmenu", "Loading init Ramdisk from Legacy Image at 20000000",
                 "Starting kernel", "Booting Linux on physical CPU"):
        if want not in s:
            finish(False, f"no {want!r} on the way to the login prompt")
    for _ in range(30):
        r = subprocess.run(["ssh", "-o", "ConnectTimeout=5", HOST,
                            "journalctl -b --no-pager | grep -m1 'cage guard'"],
                           capture_output=True, text=True, errors="replace", timeout=60)
        if "cage guard" in r.stdout:
            note(r.stdout.strip())
            ok = "within allowed cages" in r.stdout
            finish(ok, "NixOS up on our U-Boot; next: capture after-ours" if ok else "cage guard refused")
        time.sleep(5)
    finish(False, "no cage guard line in the journal")


ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
sub = ap.add_subparsers(dest="what", required=True)
p = sub.add_parser("capture"); p.add_argument("label")
p = sub.add_parser("run"); p.add_argument("result", type=pathlib.Path)
p.add_argument("--entry", choices=("booti", "go"), default="booti")
p.add_argument("--no-boot", action="store_true")
a = ap.parse_args()
if a.what == "capture":
    capture(a.label)
else:
    chainload(a.result, a.entry, not a.no_boot)
