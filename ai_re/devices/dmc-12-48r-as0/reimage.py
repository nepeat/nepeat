#!/usr/bin/env python3
"""Reimage a DMC-12 with the generic NixOS disk image, over its serial console.

Usable by humans and agents. Without --yes, each irreversible step asks first;
--yes answers yes (agents). --dry-run prints the plan. --from STEP resumes.

  reimage.py --unit u02 --plug 2                     # interactive
  reimage.py --unit u02 --plug 2 --yes --json        # agent
  reimage.py --unit u02 --plug 2 --from boot         # resume

Steps (see reimage.md):
  preflight  build outputs, tmux console session, TFTP dir, backups of this unit
  stage      copy the netboot bundle (public build artefacts only) to TFTP
  uboot      power-cycle (lab plug) and stop the factory autoboot
  identify   board serial from the U-Boot env must match units-private.txt
  envbackup  printenv -> inventory/<unit>/uboot-printenv-before-reimage-private.txt
  chainload  factory U-Boot TFTPs our U-Boot 2026.10 (al_eth) into RAM and bootm's it; stop at =>
  netboot    our U-Boot fetches kernel/initrd/DTB over HTTP and boots NixOS into RAM (no writes)
             (--via factory: the old path, factory U-Boot + TFTP, no chainload)
  write      [WRITES SSD] stream disk.img.zst to /dev/sda over ssh, verify sha256 on the box
  setenv     [WRITES SPI ENV] nixos_boot (from the image) + bootcmd = run nixos_boot; run dmc12_bootdisk
  boot       boot from the SSD, check NixOS came up on the board DT, then smoke.py
  record     append the result to inventory/<unit>/reimage.jsonl

Gates: no `write` unless dumps/<unit>/manifest.jsonl lists verified sda.img,
mtd0.bin and mtd1.bin (the original state), unless --no-backup-gate REASON.
No `write` unless the board serial matches the unit (never printed).
"""
import argparse, json, pathlib, re, shutil, subprocess, sys, time

ROOT = pathlib.Path(__file__).resolve().parent
AIRE = ROOT.parents[1]
SESSION = "serial-dmc12"
STEPS = ["preflight", "stage", "uboot", "identify", "envbackup", "chainload", "netboot",
         "write", "setenv", "boot", "record"]
NEW = "=>"  # upstream U-Boot prompt
UBOOT_BANNER = r"U-Boot 2026\.10-dmc12-chainload"
# uboot-menu images; a chainload image ships its own values in factory-env.txt.
NIXOS_BOOT = "scsi init; ext4load scsi 0:1 0x07000000 /boot.scr && source 0x07000000"
BOOTCMD = "run nixos_boot; run dmc12_bootdisk"
# Through sudo: the image trusts root@sea420-desktop (as the AS7760 deploy does), not erin.
SSH = ["sudo", "-n", "ssh", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no",
       "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR", "-o", "ConnectTimeout=10"]


class Fail(Exception):
    pass


HTTP = {"proc": None}


def http_up(args):
    """Temporary HTTP server for our U-Boot's wget: serves the TFTP dir (public build artefacts only)."""
    if HTTP["proc"] and HTTP["proc"].poll() is None:
        return
    HTTP["proc"] = subprocess.Popen([sys.executable, "-m", "http.server", str(args.http_port), "--bind", args.server,
                                     "--directory", str(args.tftp_root)],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    import urllib.request
    url = f"http://{args.server}:{args.http_port}/{args.tftp_dir}/SHA256SUMS"
    for _ in range(20):
        try:
            urllib.request.urlopen(url, timeout=2).read()
            return
        except OSError:
            time.sleep(0.3)
    raise Fail(f"HTTP server not reachable at {url} (port open in the host firewall? 8000/8080 are)")


def http_down():
    if HTTP["proc"] and HTTP["proc"].poll() is None:
        HTTP["proc"].terminate()
        HTTP["proc"].wait(timeout=10)


def say(args, msg):
    if not args.json:
        print(f"[reimage] {msg}", flush=True)


def confirm(args, what):
    if args.yes:
        say(args, f"--yes: {what}")
        return
    if not sys.stdin.isatty():
        raise Fail(f"needs confirmation ({what}); rerun with --yes")
    if input(f"{what}. Type the unit id ({args.unit}) to continue: ").strip() != args.unit:
        raise Fail("not confirmed")


def console(cmd, prompt="ALPINE_DB>", timeout=30):
    r = subprocess.run([sys.executable, str(ROOT / "console.py"), "--prompt", prompt,
                        "--timeout", str(timeout), cmd], capture_output=True, text=True)
    if r.returncode:
        raise Fail(f"console command failed: {cmd!r}: {r.stderr.strip()[-300:]}")
    return r.stdout.replace("\r", "")


def pane():
    return subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", SESSION],
                          capture_output=True, text=True, errors="replace").stdout


def clear_pane():
    subprocess.run(["tmux", "clear-history", "-t", SESSION], check=True)


def wait_pane(pattern, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        m = re.search(pattern, pane())
        if m:
            return m
        time.sleep(1)
    raise Fail(f"timed out after {timeout}s waiting for /{pattern}/ on the console")


def manifest_ok(unit):
    path = ROOT / "dumps" / unit / "manifest.jsonl"
    if not path.exists():
        return [], ["sda.img", "mtd0.bin", "mtd1.bin"]
    have = {}
    for line in path.read_text().splitlines():
        e = json.loads(line)
        if e.get("sha256") and (e.get("zst_sha256") or e.get("file")):
            have[e["file"]] = e
    missing = [f for f in ("sda.img", "mtd0.bin", "mtd1.bin") if f not in have]
    return have, missing


def expected_serial(unit):
    p = ROOT / "units-private.txt"
    if not p.exists():
        return None
    for line in p.read_text().splitlines():
        f = line.split("\t")
        if f and f[0] == unit and len(f) > 1:
            return f[1]
    return None


def factory_env(image):
    """The two env values this disk image needs (exact stored strings)."""
    p = image / "factory-env.txt"
    if not p.exists():
        return {"nixos_boot": NIXOS_BOOT, "bootcmd": BOOTCMD, "loader": "uboot-menu"}
    env = dict(l.split("=", 1) for l in p.read_text().splitlines() if "=" in l)
    if set(env) != {"nixos_boot", "bootcmd"} or "'" in env["nixos_boot"] + env["bootcmd"]:
        raise Fail(f"unexpected contents in {p}")
    return {**env, "loader": "chainload"}


def step_preflight(args, st):
    for name in ("uImage", "uInitrd", "dmc12.dtb", "boot.scr", "uboot-commands.txt", "SHA256SUMS"):
        if not (args.netboot / name).exists():
            raise Fail(f"netboot bundle incomplete: {args.netboot}/{name} (nix build path:./nix#dmc12-netboot)")
    if args.via == "chainload" and not (args.uboot / "u-boot.uimage").exists():
        raise Fail(f"no {args.uboot}/u-boot.uimage (nix build path:./nix#uboot-dmc12-chainload)")
    if args.via == "chainload":
        blob = (args.uboot / "u-boot.uimage").read_bytes()
        if b"al_eth" not in blob or b"wget" not in blob:
            raise Fail(f"{args.uboot}/u-boot.uimage has no al_eth/wget (stale build link? "
                       "nix build path:./nix#uboot-dmc12-chainload -o ...)")
    needs_disk = "write" in args.steps
    for name in ("disk.img.zst", "disk.img.sha256", "disk.img.bytes"):
        if needs_disk and not (args.image / name).exists():
            raise Fail(f"disk image incomplete: {args.image}/{name} (nix build path:./nix#dmc12-disk-image)")
    if subprocess.run(["tmux", "has-session", "-t", SESSION], capture_output=True).returncode:
        raise Fail(f"no tmux session {SESSION}; start tio on the console first (PROGRESS.md)")
    have, missing = manifest_ok(args.unit)
    if missing and not args.no_backup_gate:
        raise Fail(f"unit {args.unit} has no verified backup of {', '.join(missing)} in "
                   f"dumps/{args.unit}/manifest.jsonl. Image it first (acquire.py), "
                   "or pass --no-backup-gate REASON")
    st["backups"] = sorted(have) if have else []
    st["backup_gate"] = "skipped: " + args.no_backup_gate if missing else "ok"
    if needs_disk:
        st["image_sha256"] = (args.image / "disk.img.sha256").read_text().strip()
        st["image_bytes"] = int((args.image / "disk.img.bytes").read_text())
    st["toplevel"] = (args.netboot / "toplevel").read_text().strip()
    st["loader"] = factory_env(args.image)["loader"]


def step_stage(args, st):
    d = args.tftp_root / args.tftp_dir
    if not d.exists():
        subprocess.run(["sudo", "-n", "install", "-d", "-o", str(pathlib.Path.home().owner()),
                        str(d)], check=True)
    files = [(args.netboot / n, n) for n in ("uImage", "uInitrd", "dmc12.dtb", "boot.scr", "SHA256SUMS")]
    if args.via == "chainload":
        files.append((args.uboot / "u-boot.uimage", "u-boot.uimage"))
    for src, name in files:
        dst = d / name
        tmp = d / (name + ".tmp")
        shutil.copyfile(src, tmp)
        tmp.replace(dst)
    r = subprocess.run(["sha256sum", "-c", "--quiet", "SHA256SUMS"], cwd=d, capture_output=True, text=True)
    if r.returncode:
        raise Fail(f"TFTP copy does not match SHA256SUMS: {r.stdout}{r.stderr}")
    st["tftp"] = str(d)


def step_uboot(args, st):
    if args.no_power and pane().rstrip().endswith("ALPINE_DB>"):
        say(args, "already at the factory U-Boot prompt")
        return
    cmd = [sys.executable, str(ROOT / "powercycle_to_uboot.py")]
    if args.no_power:
        cmd.append("--no-power")
        say(args, "reset the box now (or it must already be resetting); waiting for autoboot")
    else:
        say(args, f"power-cycling lab plug {args.plug}")
        cmd += ["--plug", str(args.plug)]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=240)
    if r.returncode or "ALPINE_DB>" not in r.stdout:
        raise Fail(f"did not reach the U-Boot prompt: {r.stdout[-300:]}{r.stderr[-300:]}")


def step_identify(args, st):
    out = console("printenv bootp_dci board_name dmc12_bootenv_version")
    vals = dict(l.split("=", 1) for l in out.splitlines() if "=" in l)
    serial, want = vals.get("bootp_dci"), expected_serial(args.unit)
    st["board_name"] = vals.get("board_name")
    st["bootenv_version"] = vals.get("dmc12_bootenv_version")
    if want is None:
        raise Fail(f"{args.unit} not in units-private.txt; add its serial before reimaging")
    if serial != want:
        raise Fail(f"board serial does not match {args.unit} in units-private.txt; wrong box?")
    st["identity"] = "serial matches"


def step_envbackup(args, st):
    inv = ROOT / "inventory" / args.unit
    inv.mkdir(parents=True, exist_ok=True)
    out = inv / "uboot-printenv-before-reimage-private.txt"
    if out.exists():
        st["envbackup"] = f"kept existing {out.name}"
        return
    text = console("printenv", timeout=30)
    if "bootcmd=" not in text:
        raise Fail("printenv output has no bootcmd; console out of sync?")
    out.write_text(text + "\n")
    out.chmod(0o600)
    st["envbackup"] = out.name


def send_line(line):
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", line], check=True)
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "Enter"], check=True)


def step_chainload(args, st):
    if args.via != "chainload" or "ip" in st:
        return
    if pane().rstrip().endswith(NEW):
        st["chainload"] = "already at our U-Boot"
        return
    console("setenv autoload no")
    if "bound to address" not in console("dhcp", timeout=60):
        raise Fail("factory U-Boot DHCP failed")
    console(f"setenv serverip {args.server}")
    out = console(f"tftpboot 0x10000000 {args.tftp_dir}/u-boot.uimage", timeout=120)
    if "Bytes transferred" not in out:
        raise Fail(f"TFTP of u-boot.uimage failed: {out[-200:]}")
    if "Verifying Checksum ... OK" not in console("iminfo 0x10000000"):
        raise Fail("u-boot.uimage checksum bad after TFTP")
    clear_pane()
    send_line("bootm 0x10000000 - ${fdtaddr}")
    wait_pane(UBOOT_BANNER, 60)
    wait_pane(r"Hit any key to stop autoboot", 60)
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", " "], check=True)
    wait_pane(r"\n=> ?\s*$", 30)
    st["chainload"] = "ok"


def step_netboot(args, st):
    if "ip" in st:  # resuming: reuse a box that is already netbooted
        r = subprocess.run(SSH + [f"root@{st['ip']}", "cat /proc/device-tree/model; uname -r"],
                           capture_output=True, text=True, timeout=60)
        if r.returncode == 0:
            st["netboot_model"], st["netboot_kernel"] = (r.stdout.replace("\0", "\n").split("\n") + ["", ""])[:2]
            return
    if args.via == "chainload":
        http_up(args)
        console("setenv autoload no", NEW)
        if "bound to address" not in console("dhcp", NEW, timeout=60):
            raise Fail("U-Boot DHCP failed (al_eth)")
        console(f"setenv serverip {args.server}", NEW)
        console(f"setenv httpdstp {args.http_port}", NEW)
        for addr, name in (("0x10000000", "uImage"), ("0x20000000", "uInitrd"), ("0x04078000", "dmc12.dtb")):
            out = console(f"wget {addr} {args.server}:/{args.tftp_dir}/{name}", NEW, timeout=600)
            want = (args.netboot / name).stat().st_size
            if "Transfer Successful" not in out or f"Bytes transferred = {want} " not in out:
                raise Fail(f"HTTP wget of {name} failed or short: {out[-200:]}")
        bootargs = (args.netboot / "bootargs.txt").read_text().strip()
        console("setenv initrd_high 0xffffffffffffffff", NEW)
        console("setenv fdt_high 0xffffffffffffffff", NEW)
        console(f"setenv bootargs '{bootargs}'", NEW)
        clear_pane()
        send_line("bootm 0x10000000 0x20000000 0x04078000")
        lines = []
    else:
        lines = (args.netboot / "uboot-commands.txt").read_text().splitlines()
    for line in lines:
        line = line.replace("<SERVER>", args.server).replace("<DIR>", args.tftp_dir)
        if line.startswith(("bootm", "source")):  # boots: no prompt comes back
            clear_pane()
            subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", line], check=True)
            subprocess.run(["tmux", "send-keys", "-t", SESSION, "Enter"], check=True)
            break
        timeout = 600 if line.startswith("tftpboot") else 60
        out = console(line, timeout=timeout)
        if line.startswith("tftpboot") and "Bytes transferred" not in out:
            raise Fail(f"TFTP failed: {line}: {out[-300:]}")
        if line == "dhcp" and "bound to address" not in out:
            raise Fail(f"U-Boot DHCP failed: {out[-200:]}")
    say(args, "booting NixOS in RAM (no writes)")
    wait_pane(r"root@dmc12[^\n]*#", 600)
    out = console("ip -4 -o addr show eth0 | head -1", prompt="# ", timeout=60)
    m = re.search(r"inet (\d+\.\d+\.\d+\.\d+)/", out)
    deadline = time.monotonic() + 120
    while not m and time.monotonic() < deadline:
        time.sleep(5)
        m = re.search(r"inet (\d+\.\d+\.\d+\.\d+)/",
                      console("ip -4 -o addr show eth0 | head -1", prompt="# ", timeout=30))
    if not m:
        raise Fail("netbooted NixOS got no IPv4 address on eth0")
    st["ip"] = m.group(1)
    # The serial autologin can beat sshd: retry for a while.
    deadline = time.monotonic() + 90
    while True:
        r = subprocess.run(SSH + [f"root@{st['ip']}", "cat /proc/device-tree/model; uname -r"],
                           capture_output=True, text=True, timeout=60)
        if r.returncode == 0 or time.monotonic() > deadline:
            break
        time.sleep(3)
    if r.returncode:
        raise Fail(f"ssh to the netbooted system failed: {r.stderr.strip()}")
    st["netboot_model"], st["netboot_kernel"] = (r.stdout.replace("\0", "\n").split("\n") + ["", ""])[:2]


def ssh(st, cmd, **kw):
    return subprocess.run(SSH + [f"root@{st['ip']}", cmd], capture_output=True, text=True, **kw)


def step_write(args, st):
    if "ip" not in st:
        raise Fail("no netboot IP; run from 'netboot' (or pass --ip)")
    r = ssh(st, "lsblk -bdno SIZE,MODEL /dev/sda; findmnt -rno SOURCE | grep -c '^/dev/sda' || true", timeout=60)
    if r.returncode:
        raise Fail(f"cannot inspect /dev/sda: {r.stderr}")
    size_model, mounted = r.stdout.strip().splitlines()[0], r.stdout.strip().splitlines()[-1]
    size = int(size_model.split()[0])
    if mounted != "0":
        raise Fail("/dev/sda has mounted partitions on the netbooted system; refusing")
    if not (8e9 < size < 2.1e12) or size < st["image_bytes"]:
        raise Fail(f"/dev/sda does not look like the DMC-12 SSD ({size_model}); refusing")
    st["disk"] = size_model
    confirm(args, f"ERASE /dev/sda ({size_model}) on {args.unit} and write NixOS")
    with subprocess.Popen(["zstd", "-q", "-dc", "--long=27", str(args.image / "disk.img.zst")],
                          stdout=subprocess.PIPE) as z:
        w = subprocess.run(SSH + [f"root@{st['ip']}", "dd of=/dev/sda bs=4M conv=fsync status=none && sync"],
                           stdin=z.stdout, capture_output=True, text=True)
    if z.returncode or w.returncode:
        raise Fail(f"write failed (zstd {z.returncode}, ssh/dd {w.returncode}): {w.stderr[-300:]}")
    r = ssh(st, f"head -c {st['image_bytes']} /dev/sda | sha256sum", timeout=1800)
    got = r.stdout.split()[0] if r.stdout else ""
    if got != st["image_sha256"]:
        raise Fail(f"read-back sha256 {got} != image {st['image_sha256']}")
    st["write"] = "verified"


def step_setenv(args, st):
    if "ip" in st:
        ssh(st, "systemctl reboot", timeout=30)  # CPLD board reset via the shutdown hook
        args.no_power = True
    step_uboot(args, st)
    env = factory_env(args.image)
    want = [f"nixos_boot={env['nixos_boot']}", f"bootcmd={env['bootcmd']}"]
    cur = console("printenv bootcmd nixos_boot")
    st["bootcmd_before"] = cur.strip()
    if all(w in cur.splitlines() for w in want):
        st["setenv"] = "already set"
        return
    confirm(args, f"write the U-Boot environment on SPI ({env['loader']}: nixos_boot + bootcmd, "
                  "factory chain as fallback)")
    console(f"setenv nixos_boot '{env['nixos_boot']}'")
    console(f"setenv bootcmd '{env['bootcmd']}'")
    out = console("saveenv", timeout=60)
    if "done" not in out.lower() and "OK" not in out:
        raise Fail(f"saveenv did not report success: {out[-200:]}")
    check = console("printenv bootcmd nixos_boot")
    if not all(w in check.splitlines() for w in want):
        raise Fail(f"env read-back mismatch: {check}")
    st["setenv"] = "saved"


def step_boot(args, st):
    clear_pane()
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", "boot"], check=True)
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "Enter"], check=True)
    chain = factory_env(args.image)["loader"] == "chainload"
    # chainload: preamble (bootp) + scsi read, then upstream U-Boot's extlinux menu
    wait_pane(r"U-Boot 2026\.10-dmc12-chainload" if chain else r"DMC-12 NixOS bootmenu", 180)
    wait_pane(r"root@dmc12[^\n]*#", 600)
    out = console("cat /proc/device-tree/model; echo; findmnt -no SOURCE /; systemctl is-system-running",
                  prompt="# ", timeout=120)
    if "DMC-12" not in out or not re.search(r"DMC12ROOT|/dev/sda[23]\b", out):
        raise Fail(f"did not come up on the SSD with the board DT: {out[-300:]}")
    st["boot"] = out.strip().splitlines()
    ip = None
    deadline = time.monotonic() + 180
    while not ip and time.monotonic() < deadline:
        m = re.search(r"inet (\d+\.\d+\.\d+\.\d+)/",
                      console("ip -4 -o addr show eth0 | head -1", prompt="# ", timeout=30))
        ip = m and m.group(1)
        if not ip:
            time.sleep(5)
    if not ip:
        raise Fail("installed system got no IPv4 address on eth0")
    st["ip"] = ip
    # USB serial (48 ports) takes ~25 s to enumerate after boot.
    deadline = time.monotonic() + 120
    while ssh(st, "ls /dev/P* | wc -l", timeout=30).stdout.strip() != "48" and time.monotonic() < deadline:
        time.sleep(5)
    r = subprocess.run([sys.executable, str(ROOT / "smoke.py"), "--ip", ip, "--json"],
                       capture_output=True, text=True, timeout=600)
    res = json.loads(r.stdout) if r.stdout.strip().startswith("{") else {"pass": False}
    st["smoke"] = {"pass": res.get("pass"),
                   "failed": [c["check"] for c in res.get("results", []) if not c["pass"]]}
    if not res.get("pass"):
        raise Fail(f"installed system failed smoke checks: {st['smoke']['failed']}")


def step_record(args, st):
    inv = ROOT / "inventory" / args.unit
    inv.mkdir(parents=True, exist_ok=True)
    keep = {k: v for k, v in st.items() if k not in ("ip",)}
    keep["date"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    with (inv / "reimage.jsonl").open("a") as f:
        f.write(json.dumps(keep) + "\n")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--unit", required=True)
    p.add_argument("--plug", type=int, help="lab-power plug number (else --no-power)")
    p.add_argument("--no-power", action="store_true", help="you reset the box yourself")
    p.add_argument("--netboot", type=pathlib.Path, default=ROOT / "nixos-build/result-netboot")
    p.add_argument("--image", type=pathlib.Path, default=ROOT / "nixos-build/result-disk-chainload")
    p.add_argument("--uboot", type=pathlib.Path, default=ROOT / "nixos-build/result-uboot-dmc12-chainload",
                   help="upstream U-Boot build (u-boot.uimage) for the chainload")
    p.add_argument("--via", choices=["chainload", "factory"], default="chainload",
                   help="RAM boot through our U-Boot + HTTP (default) or the factory U-Boot + TFTP")
    p.add_argument("--http-port", type=int, default=8000, help="must be open in this host's firewall")
    p.add_argument("--tftp-root", type=pathlib.Path, default=pathlib.Path("/srv/tftpboot"))
    p.add_argument("--tftp-dir", default="dmc12")
    p.add_argument("--server", default="10.60.2.208", help="this host as U-Boot sees it")
    p.add_argument("--from", dest="start", choices=STEPS, default="preflight")
    p.add_argument("--to", dest="stop", choices=STEPS, default="record")
    p.add_argument("--ip", help="netbooted box address, when resuming after 'netboot'")
    p.add_argument("--no-backup-gate", metavar="REASON")
    p.add_argument("--yes", action="store_true")
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--json", action="store_true")
    args = p.parse_args()
    if not args.unit.startswith("u") or not args.unit[1:].isdigit():
        p.error("--unit looks like u02")
    if args.plug is None and not args.no_power:
        p.error("give --plug N or --no-power")
    steps = STEPS[STEPS.index(args.start):STEPS.index(args.stop) + 1]
    args.steps = steps
    st = {"unit": args.unit, "steps": []}
    if args.ip:
        st["ip"] = args.ip
    if args.dry_run:
        print("\n".join(steps))
        return 0
    if args.start != "preflight":
        step_preflight(args, st)
    try:
        for name in steps:
            say(args, f"== {name}")
            globals()[f"step_{name}"](args, st)
            st["steps"].append(name)
            if name == "netboot":
                http_down()
    except (Fail, subprocess.TimeoutExpired) as e:
        http_down()
        st["failed"] = {"step": name, "error": str(e)}
        if args.json:
            print(json.dumps(st))
        else:
            print(f"[reimage] FAILED at {name}: {e}", file=sys.stderr)
            print(f"[reimage] resume with --from {name}" + (f" --ip {st['ip']}" if "ip" in st else ""),
                  file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(st))
    else:
        say(args, f"done: {', '.join(st['steps'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
