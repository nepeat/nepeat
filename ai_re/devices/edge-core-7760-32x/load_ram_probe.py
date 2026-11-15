#!/usr/bin/env python3
"""Stage a RAM-only probe through the existing tmux serial session and TFTP.

Requires U-Boot prompt, ai_re devshell, and the local atftpd /srv/tftpboot.
--boot explicitly performs the final volatile bootm handoff. Never saveenv.
"""
import argparse
import shutil
import subprocess
import sys
import time
from pathlib import Path

base = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("kernel", type=Path)
parser.add_argument("--bundle", type=Path, default=base / "nixos-build/result-ram-probe-final")
parser.add_argument("--boot", action="store_true")
parser.add_argument("--extra-bootargs", default="", help="temporary kernel arguments for controlled experiments")
args = parser.parse_args()
capture = base / "ram-probe-boot-20261114.txt"
last = subprocess.check_output(["tmux", "capture-pane", "-t", "serial-7760", "-p"], text=True).rstrip()
if not last.endswith("=>"):
    raise SystemExit("not at an idle U-Boot prompt; inspect serial-7760")
transfers = [(args.kernel, "uImage-current", "0x10000000"),
             (args.bundle / "uInitrd", "uInitrd", "0x20000000"),
             (args.bundle / "as7760.dtb", "as7760.dtb", "0x08000000")]
for source, name, _ in transfers:
    target = Path("/srv/tftpboot/as7760-probe") / name
    target.unlink(missing_ok=True)
    shutil.copyfile(source, target)
    target.chmod(0o644)

def command(text):
    offset = capture.stat().st_size
    subprocess.run([sys.executable, str(base / "serial_command.py"), text], check=True)
    for _ in range(1200):
        with capture.open("rb") as stream:
            stream.seek(offset)
            output = stream.read().decode(errors="replace")
        if output.rstrip().endswith("=>"):
            if any(error in output for error in ("TFTP error", "Unknown command", "Bad Magic")) or ("Retry count exceeded" in output and "DHCP client bound to address" not in output):
                raise SystemExit(output)
            return output
        time.sleep(0.1)
    raise SystemExit("no prompt after " + text + "\n" + output)

for text in ("setenv autoload no", "dhcp", "setenv serverip 10.60.2.208"):
    command(text)
for source, name, address in transfers:
    output = command(f"tftpboot {address} as7760-probe/{name}")
    if f"Bytes transferred = {source.stat().st_size} " not in output:
        raise SystemExit(output)
    print(name + ": transfer size verified", flush=True)
# Bundles may carry their own kernel command line (the netboot NixOS bundle
# needs init=<toplevel>/init); the RAM probe keeps the historical default.
bootargs_file = args.bundle / "bootargs.txt"
bootargs = (bootargs_file.read_text().strip() if bootargs_file.exists()
            else "console=ttyS0,115200n8 rdinit=/init panic=0 pcie_aspm.policy=performance")
command("setenv bootargs " + bootargs + " " + args.extra_bootargs)
if args.boot:
    subprocess.run([sys.executable, str(base / "serial_command.py"),
                    "bootm 0x10000000 0x20000000 0x08000000"], check=True)
    print("bootm sent; watch serial-7760", flush=True)
