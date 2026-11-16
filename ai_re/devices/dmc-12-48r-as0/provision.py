#!/usr/bin/env python3
"""Take a new DMC-12 from factory state to our U-Boot in SPI + NixOS on the SSD, one unit at a time.

A thin sequencer over the tools that already hold the gates: every write happens in
a child tool and keeps its own checks and its own "type the unit id" prompt (all of them skipped by
--yes); units.md "New units" has the hands-on part. Resumable: each finished step is recorded, a rerun starts at the first step not done.

  provision.py --unit u02 --plug 2                 # interactive, from the first step not done
  provision.py --unit u02 --plug 2 --yes           # unattended (agents)
  provision.py --unit u02 --plug 2 --from rehearse # redo from a step
  provision.py --unit u02 --plug 2 --dry-run       # print the plan

Steps:
  register  power-cycle to the factory prompt; printenv -> inventory/<unit>/uboot-printenv-private.txt;
            board serial (bootp_dci) + MAC into units-private.txt (refuses a serial another unit
            id already has); version/uC/flash-TOC -> inventory/<unit>/factory-uboot-info.txt
  capture   factory rdinit shell (`setenv bootargsextra rdinit=/bin/sh; run dmc12_bootdisk`, not
            saved): proc/sys/dev, DHCP. Nothing from the SSD is mounted (checked).
  acquire   acquire.py: mtd0, mtd1, sda, triple-hashed + zstd (sda: ~10 min per 32 GB)
  small     fetch_small.py: live DTB, board EEPROM, SPD, vendor kernel config (missing ones: warned)
  reimage   reimage.py --spi-next: netboot, [WRITES SSD] standard image, stops before booting it
  rehearse  spi-upgrade.py status + rehearse: zero SPI writes; RAM U-Boot boots the SSD, smoke
  install   spi-upgrade.py install --write: [WRITES SPI] IPXE 0 + BOOT_MODE, cold boot, smoke
  smoke     smoke.py on the installed system
  inventory fw-inventory.py collect: every component's version/sha256 (SPI objects factory vs now,
            CPLD flash readback, uC, TPM, SSD, USB/PCI) -> inventory/<unit>/firmware.json + blobs
  summary   print the units.md column values from what the steps collected

Needs: the unit on a switchable lab plug (`tools/lab_power.py`; spi-upgrade power-cycles with
exact timing, so --plug is required), its front console on the FTDI in tmux `serial-dmc12`, mgmt
port on the lab LAN, and the builds of reimage.md "Before the first unit" (checked first).
Plugs with a known owner (PLUG_OWNERS) are refused unless --plug-free says it was moved.
Every step that talks to the box first checks it is this unit (board serial or MAC); the journal
gets step names and short reasons only, never console output (it is tracked; env lines aren't).
"""
import argparse, json, pathlib, re, subprocess, sys, time

ROOT = pathlib.Path(__file__).resolve().parent
PY = sys.executable
STEPS = ["register", "capture", "acquire", "small", "reimage", "rehearse", "install", "smoke", "inventory",
         "summary"]
SMALL = [  # (box path, dumps/<unit>/ name)
    ("/sys/firmware/fdt", "live.dtb"),
    ("/sys/bus/i2c/devices/7-0051/eeprom", "board-eeprom-7-0051.bin"),
    ("/sys/bus/i2c/devices/0-0057/eeprom", "spd-0-0057.bin"),
    ("/proc/config.gz", "vendor-config.gz"),
]
# Lab plugs that power something else (tools/lab_power.py). A new unit goes on a free plug;
# --plug-free REASON overrides (e.g. "u01 moved off plug 2").
PLUG_OWNERS = {"1": "the CS8210 (another session's switch)", "2": "u01, the lab console server"}
BUILDS = ["nixos-build/result-netboot/uImage", "nixos-build/result-disk-standard/disk.img.zst",
          "nixos-build/result-uboot-dmc12-chainload/u-boot.uimage",
          "nixos-build/result-uboot-dmc12-stage3app/u-boot-ipxe-08000000.fimg"]
RDINIT = "/ # "
INFO_CMDS = ["version", "uc_get_version", "uc_get_secure", "uc_watchdog_get_status",
             "flash_contents_toc_print"]


class Fail(Exception):
    """str(e) may hold console output (private); e.public is safe for the tracked journal."""

    def __init__(self, msg, public=None):
        super().__init__(msg)
        self.public = public or msg


def reimage_mod():
    import importlib.util
    spec = importlib.util.spec_from_file_location("reimage", ROOT / "reimage.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def say(msg):
    print(f"[provision] {msg}", flush=True)


def run(cmd, timeout=None, capture=False):
    """Run a child tool. Not captured: its prompts and progress go to this terminal."""
    say("$ " + " ".join(str(c) for c in cmd))
    r = subprocess.run([str(c) for c in cmd], timeout=timeout, text=True,
                       capture_output=capture)
    if r.returncode:
        tool = pathlib.Path(str(cmd[1])).name
        tail = (r.stdout or "")[-400:] + (r.stderr or "")[-400:] if capture else ""
        raise Fail(f"{tool} exited {r.returncode} {tail}".strip(), f"{tool} exited {r.returncode}")
    return r.stdout if capture else ""


def console(cmd, prompt="ALPINE_DB>", timeout=20):
    return run([PY, ROOT / "console.py", "--prompt", prompt, "--timeout", timeout, cmd],
               timeout=timeout + 30, capture=True).replace("\r", "")


def env_vars(text):
    return dict(m.groups() for m in re.finditer(r"^([A-Za-z0-9_#.]+)=(.*)$", text, re.M))


class State:
    """inventory/<unit>/provision.jsonl (tracked: steps, no addresses) +
    provision-state-private.txt (JSON: the box's lab IP, ignored by git)."""

    def __init__(self, unit):
        self.inv = ROOT / "inventory" / unit
        self.inv.mkdir(parents=True, exist_ok=True)
        self.log = self.inv / "provision.jsonl"
        self.priv = self.inv / "provision-state-private.txt"
        self.data = json.loads(self.priv.read_text()) if self.priv.exists() else {}
        self.done = set()
        if self.log.exists():
            for line in self.log.read_text().splitlines():
                rec = json.loads(line)
                if rec.get("ok"):
                    self.done.add(rec["step"])

    def save(self):
        self.priv.write_text(json.dumps(self.data, indent=1) + "\n")
        self.priv.chmod(0o600)

    def record(self, step, ok, **extra):
        rec = {"step": step, "ok": ok, "date": time.strftime("%Y-%m-%dT%H:%M:%S%z"), **extra}
        with self.log.open("a") as f:
            f.write(json.dumps(rec) + "\n")
        if ok:
            self.done.add(step)

    @property
    def ip(self):
        if "ip" not in self.data:
            raise Fail("no box address yet: run from 'capture', or pass --ip")
        return self.data["ip"]


def power_args(a):
    return ["--plug", a.plug]


def registered(unit):
    """(serial, mac) of this unit in units-private.txt, or None."""
    for l in (ROOT / "units-private.txt").read_text().splitlines():
        r = l.split("\t")
        if l and not l.startswith("#") and r[0] == unit:
            return r[1], (r[2] if len(r) > 2 else "-").lower()
    return None


def check_uboot_identity(a):
    """At the factory prompt: the board serial must be this unit's."""
    reg = registered(a.unit)
    if not reg:
        raise Fail(f"{a.unit} is not registered: run from 'register'")
    serial = env_vars(console("printenv bootp_dci")).get("bootp_dci")
    if serial != reg[0]:
        raise Fail("the board on the console is not this unit (serial differs): wrong box/cable?")


def check_rdinit(a, st):
    """In the rdinit shell, nothing from the SSD mounted, eth0 is this unit's MAC."""
    out = console("echo SDA=$(grep -c sda /proc/mounts) MAC=$(cat /sys/class/net/eth0/address)",
                  prompt=RDINIT, timeout=20)
    m = re.search(r"SDA=(\d+) MAC=([0-9a-f:]+)", out)
    if not m:
        raise Fail("not in the factory rdinit shell (rerun from 'capture')")
    if m.group(1) != "0":
        raise Fail("something from the SSD is mounted in the rdinit shell")
    reg = registered(a.unit)
    if not reg or m.group(2).replace(":", "") != reg[1].replace(":", ""):
        raise Fail("eth0's MAC is not this unit's: wrong box on the console?")


def pane_guard():
    """Refuse when the console shows one of our installed systems (u01 or a finished unit)."""
    pane = subprocess.run(["tmux", "capture-pane", "-p", "-t", "serial-dmc12", "-S", "-200"],
                          capture_output=True, text=True).stdout
    if re.search(r"root@dmc12|U-Boot 2026\.", pane):
        raise Fail("the console shows NixOS/our U-Boot (an installed unit, maybe u01): "
                   "is the FTDI on the new box? clear the pane (tmux clear-history) once it is")


# --- steps -------------------------------------------------------------------------------------

def step_register(a, st):
    missing = [b for b in BUILDS if not (ROOT / b).exists()]
    if missing:
        raise Fail(f"builds missing (reimage.md \"Before the first unit\"): {missing}")
    pane_guard()
    run([PY, ROOT / "powercycle_to_uboot.py", *power_args(a)], timeout=300)
    text = console("printenv", timeout=30)
    if "bootcmd=" not in text:
        raise Fail("printenv has no bootcmd: console out of sync?")
    env = env_vars(text)
    serial, mac = env.get("bootp_dci", ""), env.get("ethaddr", "").replace(":", "").lower()
    if not serial:
        raise Fail("no bootp_dci (board serial) in the env")
    priv = ROOT / "units-private.txt"
    rows = [l.split("\t") for l in priv.read_text().splitlines() if l and not l.startswith("#")]
    mine = [r for r in rows if r[0] == a.unit]
    other = [r[0] for r in rows if len(r) > 1 and r[1] == serial and r[0] != a.unit]
    if other:
        raise Fail(f"this board is already registered as {other[0]}: wrong --unit?")
    if mine and mine[0][1] != serial:
        raise Fail(f"{a.unit} is registered to another board serial: wrong box on the console?")
    if not mine:
        with priv.open("a") as f:
            f.write(f"{a.unit}\t{serial}\t{mac or '-'}\t-\t{env.get('hostname', '-')}\n")
        say(f"{a.unit} registered in units-private.txt (service tag: fill in from the label)")
    out = st.inv / "uboot-printenv-private.txt"   # the factory env: never replaced
    if not out.exists():
        out.write_text(text)
        out.chmod(0o600)
    info = []
    for c in INFO_CMDS:
        info.append(f"=> {c}\n{console(c, timeout=30).strip()}\n")
    text = "\n".join(info)
    flat = text.lower().replace(":", "")
    name = "factory-uboot-info-private.txt" if (serial.lower() in flat or (mac and mac in flat)) \
        else "factory-uboot-info.txt"
    (st.inv / name).write_text(text)
    st.record("register", True, info=name, dirty_hint=env.get("hostname", "") not in ("", "localhost"))


def step_capture(a, st):
    if "register" not in st.done:
        raise Fail("register first (identity check)")
    # At the factory prompt from register (a rerun: power-cycle again first).
    if a.start == "capture":
        pane_guard()
        run([PY, ROOT / "powercycle_to_uboot.py", *power_args(a)], timeout=300)
    check_uboot_identity(a)
    console("setenv bootargsextra rdinit=/bin/sh; run dmc12_bootdisk", prompt=RDINIT, timeout=240)
    console("mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev; true",
            prompt=RDINIT, timeout=20)
    check_rdinit(a, st)
    out = console("ifconfig eth0 up; sleep 4; udhcpc -i eth0 -n -q -t 5 2>&1 | tail -2; "
                  "ip -4 addr show eth0", prompt=RDINIT, timeout=90)
    m = re.search(r"inet (\d+\.\d+\.\d+\.\d+)/", out)
    if not m:
        raise Fail(f"no DHCP lease in the rdinit shell: {out[-300:]}")
    st.data["ip"] = m.group(1)
    st.save()
    say(f"rdinit shell up at {st.ip}")
    st.record("capture", True)


def step_acquire(a, st):
    check_rdinit(a, st)
    d = ROOT / "dumps" / a.unit
    if list(d.glob("*.partial")):
        raise Fail(f"dumps/{a.unit} has a .partial from an interrupted transfer: delete it (it was "
                   "never verified), then rerun")
    raws = [n for n in ("mtd0.bin", "mtd1.bin", "sda.img") if (d / n).exists()]
    if raws:
        raise Fail(f"raw {raws} left uncompressed by an interrupted run: finish with "
                   f"`acquire.py --unit {a.unit} --compress NAME SHA256` (sha256 in its output)")
    run([PY, ROOT / "acquire.py", "--unit", a.unit, "--host", st.ip])
    _, missing = reimage_mod().manifest_ok(a.unit)
    if missing:
        raise Fail(f"manifest still lacks {missing}")
    st.record("acquire", True)


def step_small(a, st):
    check_rdinit(a, st)
    d = ROOT / "dumps" / a.unit
    got, missing = [], []
    for i, (src, name) in enumerate(SMALL):
        if (d / name).exists():
            got.append(name)
            continue
        try:
            run([PY, ROOT / "fetch_small.py", "--host", st.ip, src, d / name, 19300 + i], timeout=120)
            got.append(name)
        except (Fail, subprocess.TimeoutExpired) as e:
            say(f"warning: {src}: {getattr(e, 'public', type(e).__name__)}")
            missing.append(name)
    if "live.dtb" not in got:
        raise Fail("not even the live DTB came off the box: is it still in the rdinit shell?")
    st.record("small", True, got=got, missing=missing)


def step_reimage(a, st):
    cmd = [PY, ROOT / "reimage.py", "--unit", a.unit, *power_args(a), "--spi-next"]
    run(cmd + (["--yes"] if a.yes else []))
    st.record("reimage", True)


def step_rehearse(a, st):
    su = [PY, ROOT / "spi-upgrade.py", "--unit", a.unit, *power_args(a)]
    run(su + ["status"])
    run(su + ["rehearse", "--ip", st.ip])
    st.record("rehearse", True)


def step_install(a, st):
    if "rehearse" not in st.done:
        raise Fail("rehearse first")
    if reimage_mod().spi_uboot(a.unit):   # journal: BOOT_MODE already selects our U-Boot
        say("SPI journal: our U-Boot is already installed; not writing again")
        st.record("install", True, note="already installed (journal)")
        return
    su = [PY, ROOT / "spi-upgrade.py", "--unit", a.unit, *power_args(a)]
    run(su + ["install", "--ip", st.ip, "--write"] + (["--yes"] if a.yes else []))
    st.record("install", True)


def step_smoke(a, st):
    r = subprocess.run([PY, str(ROOT / "smoke.py"), "--ip", st.ip, "--json"], timeout=600,
                       capture_output=True, text=True)
    try:
        res = json.loads(r.stdout.strip().splitlines()[-1])
    except (IndexError, ValueError):
        raise Fail(f"smoke.py exited {r.returncode} without a result", f"smoke.py exited {r.returncode}")
    failed = [c["check"] for c in res.get("results", []) if not c["pass"]]
    st.record("smoke", not failed, failed=failed)
    if failed:
        raise Fail(f"smoke failed: {failed}")


def step_inventory(a, st):
    run([PY, ROOT / "fw-inventory.py", "collect", "--unit", a.unit, "--ip", st.ip], timeout=900)
    st.record("inventory", True)


def step_summary(a, st):
    inv, d = st.inv, ROOT / "dumps" / a.unit
    env = env_vars((inv / "uboot-printenv-private.txt").read_text())
    man = [json.loads(l) for l in (d / "manifest.jsonl").read_text().splitlines()] \
        if (d / "manifest.jsonl").exists() else []
    info = next((p for p in (inv / "factory-uboot-info.txt", inv / "factory-uboot-info-private.txt")
                 if p.exists()), None)
    print(f"\n## {a.unit}: values for units.md (identifiers stay in units-private.txt)")
    for key in ("dmc12_bootenv_version", "ver", "board_name"):
        if key in env:
            print(f"  env {key}: {env[key]}")
    if info:
        for line in info.read_text().splitlines():
            if re.search(r"U-Boot 20|al-boot|[Vv]ersion|SECURE", line):
                print(f"  {line.strip()}")
    for m in man:
        print(f"  {m['file']}: {m['bytes']} bytes, sha256 {m['sha256'][:8]}...")
    print(f"  provision log: inventory/{a.unit}/provision.jsonl\n")
    st.record("summary", True)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--unit", required=True)
    p.add_argument("--plug", required=True, help="lab-power plug of this unit (tools/lab_power.py)")
    p.add_argument("--from", dest="start", choices=STEPS, help="default: the first step not done")
    p.add_argument("--to", dest="stop", choices=STEPS, default=STEPS[-1])
    p.add_argument("--ip", help="the box's lab address (resume without redoing capture)")
    p.add_argument("--yes", action="store_true", help="pass --yes to the writing tools (agents)")
    p.add_argument("--plug-free", metavar="REASON", help="this plug's usual device was moved off it")
    p.add_argument("--dry-run", action="store_true")
    a = p.parse_args()
    if not re.fullmatch(r"u\d\d", a.unit):
        p.error("--unit looks like u02")
    if a.unit == "u01":
        p.error("u01 is already provisioned (and in service as the lab console server)")
    if a.plug in PLUG_OWNERS and not a.plug_free:
        p.error(f"plug {a.plug} powers {PLUG_OWNERS[a.plug]}; use a free plug, or --plug-free REASON "
                "once that device is unplugged from it")
    st = State(a.unit)
    if a.ip:
        st.data["ip"] = a.ip
        st.save()
    start = a.start or next((s for s in STEPS if s not in st.done), None)
    if start is None:
        say(f"{a.unit}: every step done ({', '.join(STEPS)}); --from STEP to redo one")
        return 0
    a.start = start
    steps = STEPS[STEPS.index(start):STEPS.index(a.stop) + 1]
    if a.dry_run:
        for s in steps:
            print(f"{s}{'  (done)' if s in st.done else ''}")
        return 0
    for name in steps:
        say(f"== {name}")
        try:
            globals()[f"step_{name}"](a, st)
        except (Fail, subprocess.TimeoutExpired, OSError, ValueError) as e:
            st.record(name, False, error=getattr(e, "public", type(e).__name__)[:200])
            say(f"FAILED at {name}: {e}")
            say(f"fix it, then rerun (it resumes at {name}); `--from {name}` to force")
            return 1
    say(f"{a.unit}: done ({', '.join(steps)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
