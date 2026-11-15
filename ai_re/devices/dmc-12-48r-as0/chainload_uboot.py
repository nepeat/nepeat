#!/usr/bin/env python3
"""Start the RAM-only upstream U-Boot (#uboot-dmc12-chainload) from the factory loader.

Nothing is written anywhere but RAM: no saveenv, no sf, no scsi write (the
image has no SPI flash, no env storage and no ext4/FAT write support). A
power-cycle returns to the factory boot. See u-boot-port.md.

Needs an idle factory prompt `ALPINE_DB>` in tmux `serial-dmc12`
(./powercycle_to_uboot.py). All I/O goes through that pane: factory-side
commands via ./console.py, the jump and the new U-Boot via the same paced
send-keys.

  1. stage <result>/u-boot.uimage (+ .bin) in /srv/tftpboot/dmc12-uboot/
  2. factory: setenv autoload no; dhcp; setenv serverip; tftpboot 0x10000000;
     iminfo; bootm 0x10000000 - ${fdtaddr}      (or --go: u-boot.bin at
     0x08000000, go 0x08000000)
  3. new U-Boot: banner + entry line, stop autoboot, read-only checks
     (bdinfo, CPLD version over I2C, PCI, SCSI, partitions,
     bootflow list, memory compare), optional --boot / --reset
  --selfinit (#uboot-dmc12-selfinit): also PCIe port 0 teardown + our RC init
     + rescan (12d8:400c), sf read + crc32 of both SPI NOR chips against the
     u01 backups (read-only build: no write/erase code), CCU write + readback,
     MUIO readback; with --boot, the CCU line before "Starting kernel".
     The PCIe re-init and the CCU write change live SoC state (RAM-only, a
     power-cycle restores the factory path); nothing is written to flash.
Transcript: serial-chainload-<date>.txt here (git-ignored). Exit 0 only on PASS.

  ./chainload_uboot.py ../../devices/dmc-12-48r-as0/nixos-build/result-uboot-dmc12-chainload
  ./chainload_uboot.py nixos-build/result-uboot-dmc12-selfinit --selfinit [--boot]
"""
import argparse, hashlib, pathlib, re, shutil, subprocess, sys, time

HERE = pathlib.Path(__file__).resolve().parent
SESSION = "serial-dmc12"
TFTP_DIR = pathlib.Path("/srv/tftpboot/dmc12-uboot")
SERVER = "10.60.2.208"
FACTORY = "ALPINE_DB>"
NEW = "=>"

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument("result", type=pathlib.Path, help="nix result dir of #uboot-dmc12-chainload")
ap.add_argument("--go", action="store_true", help="enter with 'go' (MMU on) instead of bootm")
ap.add_argument("--banner", default=None,
                help="expected version banner (default: -dmc12-chainload, or -dmc12-selfinit)")
ap.add_argument("--selfinit", action="store_true",
                help="#uboot-dmc12-selfinit checklist: PCIe teardown + our init + bus scan, "
                     "SPI NOR read + CRC of both chips against the u01 backups, CCU write, MUIO readback")
ap.add_argument("--cmd", action="append", default=[],
                help="extra step 'command::expected substring' in the new U-Boot")
ap.add_argument("--boot", action="store_true",
                help="finish with 'bootflow scan -lb' and wait for 'Starting kernel'")
ap.add_argument("--reset", action="store_true",
                help="finish with 'reset' and wait for the board to come back (CPLD reset test)")
args = ap.parse_args()
if args.banner is None:
    args.banner = "U-Boot 2026.10-dmc12-" + ("selfinit" if args.selfinit else "chainload")

# u01 SPI backups (dumps/u01/SHA256SUMS: mtd0 b66dbca5..., mtd1 9c3a9556...), zlib CRC32.
# CS0 0xfd0000-0xfeffff is the env pair, which reimage.py edits: compare below it.
CRC_CS0_BELOW_ENV = "08145ce6"   # mtd0[0:0xfd0000]
CRC_CS1_FULL = "db9e4937"        # mtd1[0:0x1000000]
CRC_CS1_BELOW_ENV = "3d086ac6"   # mtd1[0:0xfd0000]
SELFINIT_STEPS = [
    # Read-only and state steps first, the PCIe teardown last: a crash there
    # (u01 run 2) must not hide the rest.
    ("alpcie status", "link up 2.5GT/s x1"),    # the factory U-Boot trained the link; boot kept it
    ("pci 2", "0x400c"),                        # the Pericom bridge as the factory left it
    # SPI NOR, read-only build: probe writes nothing, only read opcodes leave the SSI
    ("sf probe 0:0", "mx25u12835f"),
    ("sf read 0x40000000 0 0xfd0000", "Read: OK"),
    ("crc32 0x40000000 0xfd0000", "==> " + CRC_CS0_BELOW_ENV),
    ("sf probe 0:1", "mx25u12835f"),
    ("sf read 0x40000000 0 0x1000000", "Read: OK"),
    ("crc32 0x40000000 0xfd0000", "==> " + CRC_CS1_BELOW_ENV),
    ("crc32 0x40000000 0x1000000", ""),         # full CS1; expect CRC_CS1_FULL if its env is untouched
    # MUIO: board_init replayed the DT pins (0 changed after the factory init)
    ("alpine state", "pin 21: function 2 (DT wants 2)"),
    # CCU: the factory bootm left 7 / c0000001; put the factory-prompt values back, apply ours
    ("alpine state", "SI4 snoop c0000001"),
    ("mw.l 0xf0090004 0; mw.l 0xf0094000 0xc0000000", ""),
    ("alpine ccu", "snoop c0000000 -> c0000001, speculation 0 -> 7"),
    # teardown + full RC init by our HAL code, then a fresh enumeration
    ("alpcie reinit", "x1 (our init after teardown"),
    ("alpcie status", "link partner (bus 2): 12d8:400c"),
    ("pci 2", "0x400c"),
    ("pci 3", ""),
    ("pci 4", "0x400e"),                        # the USB host functions behind it
    ("alpcie status", "SErrors drained: "),
]

log = HERE / time.strftime("serial-chainload-%Y%m%d-%H%M%S.txt")
transcript = []


def note(text):
    transcript.append(text)
    log.write_text("\n".join(transcript) + "\n")


def finish(ok, why=""):
    note(("PASS" if ok else "FAIL") + (": " + why if why else ""))
    print(("PASS" if ok else "FAIL") + (": " + why if why else ""), f"(transcript {log.name})")
    sys.exit(0 if ok else 1)


def pane():
    return subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", SESSION],
                          check=True, capture_output=True, text=True, errors="replace").stdout


def console(command, prompt, timeout=30, bad=("Unknown command", "ERROR", "Bad ", "TFTP error",
                                               "exception", "Synchronous Abort", "### ")):
    """One paced command via console.py; returns its output, FAIL on errors."""
    r = subprocess.run([sys.executable, str(HERE / "console.py"), "--prompt", prompt,
                        "--timeout", str(timeout), command], capture_output=True, text=True)
    out = r.stdout + r.stderr
    note(f"{prompt} {command}\n{out.rstrip()}")
    if r.returncode != 0 or any(b in out for b in bad):
        finish(False, f"{command!r}: {out.strip()[-500:]}")
    return out


def send_nowait(text):
    """Same pacing as console.py, but don't wait for a prompt (jumps, key presses)."""
    for byte in text.encode():
        subprocess.run(["tmux", "send-keys", "-t", SESSION, "-H", f"{byte:02x}"], check=True)
        time.sleep(0.02)
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "Enter"], check=True)


def wait_pane(pattern, start, timeout):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        text = pane()[start:]
        m = re.search(pattern, text)
        if m:
            return text
        time.sleep(0.1)
    note(pane()[start:])
    return None


# 0. preconditions: built image, idle factory prompt
uimage, ubin = args.result / "u-boot.uimage", args.result / "u-boot.bin"
for f in (uimage, ubin):
    if not f.is_file():
        raise SystemExit(f"missing {f}")
if not pane().rstrip().endswith(FACTORY):
    raise SystemExit(f"{SESSION} is not at an idle {FACTORY} prompt (powercycle_to_uboot.py)")

# 1. stage public build artefacts only
TFTP_DIR.mkdir(exist_ok=True)  # may need: sudo -n install -d -o $USER /srv/tftpboot/dmc12-uboot
staged = {}
for f in (uimage, ubin):
    dst = TFTP_DIR / f.name
    dst.unlink(missing_ok=True)
    shutil.copyfile(f, dst)
    dst.chmod(0o644)
    staged[f.name] = (dst.stat().st_size, hashlib.sha256(dst.read_bytes()).hexdigest())
note(f"staged {staged}")

# 2. factory side (RAM only)
console("setenv autoload no", FACTORY)
for _ in range(3):  # the uplink may still be negotiating after power-on
    if re.search(r"DHCP client bound|bound to address", console("dhcp", FACTORY, 60, bad=())):
        break
else:
    finish(False, "factory DHCP failed")
console(f"setenv serverip {SERVER}", FACTORY)
if args.go:
    name, addr, jump = "u-boot.bin", "0x08000000", "go 0x08000000"
else:
    name, addr, jump = "u-boot.uimage", "0x10000000", "bootm 0x10000000 - ${fdtaddr}"
out = console(f"tftpboot {addr} dmc12-uboot/{name}", FACTORY, 120)
if f"Bytes transferred = {staged[name][0]} " not in out:
    finish(False, "TFTP size mismatch")
if not args.go:
    if "Verifying Checksum ... OK" not in console(f"iminfo {addr}", FACTORY):
        finish(False, "uImage checksum")

start = len(pane())
note(f"{FACTORY} {jump}")
send_nowait(jump)

# 3. new U-Boot
text = wait_pane(re.escape(args.banner), start, 30)
if text is None:
    finish(False, "no new banner within 30 s (hung before the console came up?)")
text = wait_pane(r"Hit any key to stop autoboot", start, 60)
if text is None:
    finish(False, "banner but no autoboot countdown within 60 s")
subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", " "], check=True)
text = wait_pane(r"\n=> ?$", start, 30)
if text is None:
    finish(False, "no => prompt")
note(text)
entry = re.search(r"Entry: EL(\d), MMU\+caches were (\w+)", text)

checks = {"banner": True, "entry line": bool(entry)}
steps = [
    ("version", "U-Boot 2026.10"),
    ("bdinfo", "relocaddr"),
    ("i2c dev 4", ""),
    ("i2c md 0x68 0x01 1", "0001: 88"),     # CPLD version register (u01: 0x88)
    ("pci", "1c36"),
    ("scsi scan", "Device 0"),
    ("part list scsi 0", "Part"),
    ("bootflow scan -l", ""),               # list only, never boots
    ("mw.l 0x40000000 0x5a5aa5a5 0x1000", ""),
    ("cmp.l 0x40000000 0x40000004 0xfff", "Total of 4095 word(s) were the same"),
]
if args.selfinit:
    checks["MUIO replay at board_init"] = "MUIO:  4 pins from the DT, 0 changed" in text
    checks["PCIe kept at boot"] = "PCIe 0: link up 2.5GT/s x1 (firmware init, kept)" in text
    steps += SELFINIT_STEPS
steps += [tuple(s.split("::", 1)) if "::" in s else (s, "") for s in args.cmd]


def step_timeout(command):
    """sf read is bounded by the SPI clock: 16 MiB took 260 s at the old 500 kHz cap."""
    return 600 if command.startswith("sf read") else 120


for i, (command, want) in enumerate(steps):
    t0 = time.monotonic()
    out = console(command, NEW, step_timeout(command),
                  bad=("Unknown command", "Synchronous Abort", "exception", "### "))
    took = time.monotonic() - t0
    # keyed by position: the same command runs more than once (status, pci, crc32)
    checks[f"{i}:{command}"] = (want in out) if want else True
    m = re.match(r"sf read \S+ \S+ (0x[0-9a-f]+)", command)
    if m:
        rate = f"sf read {int(m.group(1), 16)} B in {took:.1f} s = {int(m.group(1), 16) / took / 1024:.0f} KiB/s"
        note(rate)
        print(rate)
m = re.search(r"relocaddr\s*=\s*(0x[0-9a-f]+)", "\n".join(transcript))
checks["relocated below 0xb0000000"] = bool(m) and int(m.group(1), 16) < 0xb0000000

if args.boot:
    start = len(pane())
    note("=> bootflow scan -lb")
    send_nowait("bootflow scan -lb")
    booted = wait_pane(r"Starting kernel", start, 180)
    checks["Starting kernel"] = booted is not None
    if args.selfinit:   # CCU writes run in board_prep_linux, right before
        checks["CCU set before Linux"] = bool(booted and re.search(r"CCU:\s+snoop \w+ -> c0000001", booted))
elif args.reset:
    start = len(pane())
    note("=> reset")
    send_nowait("reset")
    checks["board reset (stage2 banner)"] = wait_pane(r"stage2_loader_k2", start, 60) is not None

bad = [k for k, v in checks.items() if not v]
summary = f"entry EL{entry.group(1)}, MMU {entry.group(2)}" if entry else "no entry line"
finish(not bad, ("missing: " + ", ".join(bad)) if bad else f"new U-Boot up ({summary}); " + ", ".join(checks))
